//! Matching an invocation against a macro's pattern (spec §12.3).
//!
//! **Matching never backtracks.** An optional or repeated element is taken exactly when the
//! next token can begin it, and once taken it must complete. So a mismatch is reported at
//! the one token that does not fit, and the result of matching a prefix of an invocation
//! never changes as more is typed after it — which is what the editor relies on.

use std::collections::HashMap;

use dz_ast::*;
use dz_diag::{codes, Diagnostic, Diagnostics};
use dz_lex::{Token, TokenKind};
use dz_parse::Fragment;
use dz_span::FileId;

use crate::table::{cap_kind, CapKind};

/// What one capture matched.
#[derive(Clone, Debug)]
pub(crate) enum Binding {
    /// A fragment: tokens `start..end` of the invocation.
    Tokens { start: usize, end: usize },
    /// A `syntax` rule, with its own captures.
    Rule { start: usize, end: usize, captures: Captures },
    /// A repetition, with the whole run (separators included) and each element.
    Many { start: usize, end: usize, elems: Vec<Binding> },
    /// An optional that was not there, or a capture in a group that was not.
    Absent,
}

impl Binding {
    /// The tokens this binding covers, to emit it as written.
    pub(crate) fn range(&self) -> Option<(usize, usize)> {
        match self {
            Binding::Tokens { start, end }
            | Binding::Rule { start, end, .. }
            | Binding::Many { start, end, .. } => Some((*start, *end)),
            Binding::Absent => None,
        }
    }

    /// Present, for `#if`: matched, and if a repetition, at least once.
    pub(crate) fn present(&self) -> bool {
        match self {
            Binding::Absent => false,
            Binding::Many { elems, .. } => !elems.is_empty(),
            _ => true,
        }
    }
}

pub(crate) type Captures = HashMap<String, Binding>;

/// Rules may refer to one another, and one that begins with itself would recurse without
/// consuming anything. The limit is far beyond any real grammar's nesting.
const MAX_RULE_DEPTH: u32 = 64;

pub(crate) struct Matcher<'a> {
    pub file: FileId,
    pub decl: &'a MacroDecl,
    /// The invocation's tokens, ending with an `Eof`.
    pub toks: &'a [Token],
    pub texts: &'a [String],
    pub diags: Diagnostics,
    depth: u32,
}

impl<'a> Matcher<'a> {
    pub(crate) fn new(
        file: FileId,
        decl: &'a MacroDecl,
        toks: &'a [Token],
        texts: &'a [String],
    ) -> Matcher<'a> {
        Matcher { file, decl, toks, texts, diags: Diagnostics::new(), depth: 0 }
    }

    /// Match the whole invocation. `Err` means a diagnostic says why not.
    pub(crate) fn run(&mut self, pattern: &[PatElem]) -> Result<Captures, ()> {
        let mut pos = 0;
        let mut caps = Captures::new();
        self.seq(pattern, &mut pos, &mut caps)?;
        self.skip_terms(&mut pos);
        if self.kind(pos) != TokenKind::Eof {
            let found = self.describe(pos);
            self.fail(
                pos,
                format!("unexpected {found} in an invocation of `#{}`", self.decl.name.name),
                "the macro's pattern ends before this",
            );
            return Err(());
        }
        Ok(caps)
    }

    fn kind(&self, pos: usize) -> TokenKind {
        self.toks.get(pos).map_or(TokenKind::Eof, |t| t.kind)
    }

    fn skip_terms(&self, pos: &mut usize) {
        while self.kind(*pos) == TokenKind::Terminator {
            *pos += 1;
        }
    }

    /// The first position at or after `pos` that is not a line break.
    fn next_real(&self, pos: usize) -> usize {
        let mut p = pos;
        self.skip_terms(&mut p);
        p
    }

    fn describe(&self, pos: usize) -> String {
        match self.kind(pos) {
            TokenKind::Eof => "the end of the invocation".to_owned(),
            TokenKind::Terminator => "the end of the line".to_owned(),
            _ => format!("`{}`", self.texts.get(pos).map_or("", String::as_str)),
        }
    }

    fn fail(&mut self, pos: usize, msg: String, label: &str) {
        let span = self.toks.get(pos).or(self.toks.last()).map_or(dz_span::Span::DUMMY, |t| t.span);
        self.diags.push(
            Diagnostic::error(codes::MACRO_NO_MATCH, msg).at(self.file, span, label).with_help(
                format!(
                    "the invocation must follow `#{}`'s `pattern` (spec §12.3)",
                    self.decl.name.name
                ),
            ),
        );
    }

    fn seq(&mut self, elems: &[PatElem], pos: &mut usize, caps: &mut Captures) -> Result<(), ()> {
        for e in elems {
            self.skip_terms(pos);
            match e {
                PatElem::Token(t) => {
                    if !self.token_is(*pos, t) {
                        let found = self.describe(*pos);
                        self.fail(
                            *pos,
                            format!("expected `{}`, found {found}", t.text),
                            &format!("expected `{}` here", t.text),
                        );
                        return Err(());
                    }
                    *pos += 1;
                }
                PatElem::Capture { name, shape, kind, .. } => {
                    let binding = match shape {
                        CaptureShape::One => self.one(kind, pos)?,
                        CaptureShape::Optional => {
                            if self.can_start_kind(kind, *pos) {
                                self.one(kind, pos)?
                            } else {
                                Binding::Absent
                            }
                        }
                        CaptureShape::Many { sep } => self.many(kind, sep.as_ref(), pos)?,
                    };
                    caps.insert(name.name.clone(), binding);
                }
                PatElem::Group { elems, .. } => {
                    if self.can_start_seq(elems, *pos) {
                        self.seq(elems, pos, caps)?;
                    } else {
                        absent(elems, caps);
                    }
                }
            }
        }
        Ok(())
    }

    fn token_is(&self, pos: usize, t: &MacroToken) -> bool {
        let Some(tok) = self.toks.get(pos) else { return false };
        if tok.kind != t.kind {
            return false;
        }
        // An identifier or a literal is itself only with the same text; punctuation and
        // keywords are their kind.
        if tok.kind == TokenKind::Ident || tok.kind.is_literal() {
            return self.texts.get(pos).map(String::as_str) == Some(t.text.as_str());
        }
        true
    }

    fn many(
        &mut self,
        kind: &Ident,
        sep: Option<&MacroToken>,
        pos: &mut usize,
    ) -> Result<Binding, ()> {
        let start = self.next_real(*pos);
        let mut end = start;
        let mut elems = Vec::new();
        loop {
            self.skip_terms(pos);
            if !self.can_start_kind(kind, *pos) {
                break;
            }
            elems.push(self.one(kind, pos)?);
            end = *pos;
            if let Some(s) = sep {
                let p = self.next_real(*pos);
                if !self.token_is(p, s) {
                    break;
                }
                *pos = p + 1;
                end = *pos;
            }
        }
        Ok(Binding::Many { start, end: end.max(start), elems })
    }

    /// One fragment or rule, which must be there.
    fn one(&mut self, kind: &Ident, pos: &mut usize) -> Result<Binding, ()> {
        self.skip_terms(pos);
        let start = *pos;
        match cap_kind(kind, &self.decl.rules) {
            Some(CapKind::Fragment(f)) => {
                match dz_parse::fragment_len(self.file, self.toks, self.texts, start, f) {
                    Ok(n) => {
                        *pos += n;
                        Ok(Binding::Tokens { start, end: *pos })
                    }
                    Err(d) => {
                        // The parser's own words, at the token it stopped on: the same
                        // message the code would get anywhere else (spec §12.2).
                        self.diags.extend(d);
                        Err(())
                    }
                }
            }
            Some(CapKind::Rule(r)) => {
                if self.depth >= MAX_RULE_DEPTH {
                    self.fail(start, "a `syntax` rule nests too deeply".to_owned(), "here");
                    return Err(());
                }
                let rule = self.decl.rules.iter().find(|x| x.name.name == r).expect("a rule");
                let mut sub = Captures::new();
                self.depth += 1;
                let r = self.seq(&rule.pattern, pos, &mut sub);
                self.depth -= 1;
                r?;
                Ok(Binding::Rule { start, end: *pos, captures: sub })
            }
            // Reported where the macro is declared.
            None => Err(()),
        }
    }

    fn can_start_kind(&self, kind: &Ident, pos: usize) -> bool {
        let p = self.next_real(pos);
        match cap_kind(kind, &self.decl.rules) {
            Some(CapKind::Fragment(f)) => can_start_fragment(f, self.kind(p)),
            Some(CapKind::Rule(r)) => self
                .decl
                .rules
                .iter()
                .find(|x| x.name.name == r)
                .is_some_and(|rule| self.can_start_seq(&rule.pattern, p)),
            None => false,
        }
    }

    /// Can `elems` begin at `pos`? Decided by the first element that must be there, after
    /// any optional ones before it that are not.
    fn can_start_seq(&self, elems: &[PatElem], pos: usize) -> bool {
        let p = self.next_real(pos);
        for e in elems {
            match e {
                PatElem::Token(t) => return self.token_is(p, t),
                PatElem::Capture { shape: CaptureShape::One, kind, .. } => {
                    return self.can_start_kind(kind, p)
                }
                PatElem::Capture { kind, .. } => {
                    if self.can_start_kind(kind, p) {
                        return true;
                    }
                }
                PatElem::Group { elems, .. } => {
                    if self.can_start_seq(elems, p) {
                        return true;
                    }
                }
            }
        }
        false
    }
}

/// Every capture in a group that did not match is absent.
fn absent(elems: &[PatElem], caps: &mut Captures) {
    for e in elems {
        match e {
            PatElem::Capture { name, .. } => {
                caps.insert(name.name.clone(), Binding::Absent);
            }
            PatElem::Group { elems, .. } => absent(elems, caps),
            PatElem::Token(_) => {}
        }
    }
}

/// Whether a token can begin a fragment: how an optional or repeated one is decided.
fn can_start_fragment(f: Fragment, k: TokenKind) -> bool {
    use TokenKind::*;
    match f {
        Fragment::Ident => k == Ident,
        Fragment::Literal => k.is_literal(),
        Fragment::Type => matches!(
            k,
            Ident | SelfType | Caret | CaretCaret | Own | Amp | AmpAmp | LBracket | LParen | Fn
        ),
        Fragment::Expr => starts_expr(k),
        Fragment::Stmt => {
            starts_expr(k) || matches!(k, Let | While | For | Defer | Fn | Struct | Union | Enum)
        }
        Fragment::Block => k == LBrace,
        Fragment::Code => !matches!(k, Eof | RParen | RBracket | RBrace),
        Fragment::Decl => matches!(
            k,
            Pub | Fn
                | Struct
                | Union
                | Enum
                | Trait
                | Impl
                | Const
                | Static
                | Macro
                | Hash
                | Extern
                | Unsafe
                | Protected
        ),
    }
}

fn starts_expr(k: TokenKind) -> bool {
    use TokenKind::*;
    k.is_literal()
        || matches!(
            k,
            Ident
                | SelfValue
                | SelfType
                | LParen
                | LBracket
                | LBrace
                | Minus
                | Bang
                | Tilde
                | Amp
                | AmpAmp
                | Caret
                | Hash
                | Dot
                | Move
                | If
                | Switch
                | Unsafe
                | Return
                | Break
                | Continue
                | DotDot
                | DotDotEq
        )
}
