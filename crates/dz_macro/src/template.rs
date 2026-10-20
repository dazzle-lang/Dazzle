//! Writing an expansion: a macro's `expand` with its captures filled in (spec §12.3).
//!
//! The result is a run of tokens, each carrying its own text, so the real parser can read
//! it. A token the macro wrote is given the invocation's name as its span, which is where a
//! diagnostic about code the user did not write belongs (spec §12.6) and how hygiene tells
//! the macro's names from the caller's (spec §12.4). A captured token keeps its own span:
//! a mistake in a state's body is reported in the body.

use dz_ast::*;
use dz_diag::{codes, Diagnostic};
use dz_lex::TokenKind;
use dz_span::{FileId, Span};

use crate::matcher::{Binding, Captures};

pub(crate) struct Writer<'a> {
    pub file: FileId,
    /// The invocation's tokens, as matched.
    pub input: &'a [MacroToken],
    /// Where the macro's own tokens point: the invocation's `#name`.
    pub name_span: Span,
    pub out: Vec<MacroToken>,
    pub error: Option<Diagnostic>,
}

/// The `#for` variables in scope, innermost last.
type Env<'b> = Vec<(&'b str, &'b Binding)>;

impl<'a> Writer<'a> {
    pub(crate) fn write<'b>(
        &mut self,
        nodes: &'b [TemplateNode],
        top: &'b Captures,
        env: &mut Env<'b>,
    ) {
        for n in nodes {
            if self.error.is_some() {
                return;
            }
            match n {
                TemplateNode::Token(t) => self.own(t.kind, &t.text),
                TemplateNode::Var(v) => {
                    let Some((b, used)) = lookup(v, top, env) else { continue };
                    self.binding(b);
                    // `$e.len` where `$e` is an expression: the rest is Dazzle's own `.`.
                    for name in &v.path[used..] {
                        self.own(TokenKind::Dot, ".");
                        self.own(TokenKind::Ident, &name.name);
                    }
                }
                TemplateNode::For { var, list, sep, body, .. } => {
                    let Some((b, _)) = lookup(list, top, env) else { continue };
                    let elems: Vec<&Binding> = match b {
                        Binding::Many { elems, .. } => elems.iter().collect(),
                        Binding::Absent => Vec::new(),
                        other => vec![other],
                    };
                    for (i, e) in elems.iter().enumerate() {
                        if i > 0 {
                            if let Some(s) = sep {
                                self.own(s.kind, &s.text);
                            }
                        }
                        env.push((var.name.as_str(), e));
                        self.write(body, top, env);
                        env.pop();
                    }
                }
                TemplateNode::If { negated, cond, then, otherwise, .. } => {
                    let present = lookup(cond, top, env).is_some_and(|(b, _)| b.present());
                    if present != *negated {
                        self.write(then, top, env);
                    } else if let Some(o) = otherwise {
                        self.write(o, top, env);
                    }
                }
                TemplateNode::Error { at, message, .. } => {
                    let span = at
                        .as_ref()
                        .and_then(|v| lookup(v, top, env))
                        .and_then(|(b, _)| b.range())
                        .and_then(|(s, e)| crate::table::run_span(&self.input[s..e]))
                        .unwrap_or(self.name_span);
                    self.error = Some(
                        Diagnostic::error(codes::MACRO_ERROR, string_value(&message.text)).at(
                            self.file,
                            span,
                            "rejected by the macro",
                        ),
                    );
                }
            }
        }
    }

    /// A token the macro itself wrote.
    fn own(&mut self, kind: TokenKind, text: &str) {
        self.out.push(MacroToken { kind, span: self.name_span, text: text.to_owned() });
    }

    /// A capture, exactly as the invocation wrote it.
    fn binding(&mut self, b: &Binding) {
        if let Some((s, e)) = b.range() {
            self.out.extend(self.input[s..e].iter().cloned());
        }
    }
}

/// What `$a.b` names: a `#for` variable or a capture, then a capture inside each `syntax`
/// rule the path goes through. Returns how many of the path's names that used.
fn lookup<'b>(v: &VarRef, top: &'b Captures, env: &Env<'b>) -> Option<(&'b Binding, usize)> {
    let first = v.path[0].name.as_str();
    let mut b: &'b Binding =
        env.iter().rev().find(|(n, _)| *n == first).map(|(_, b)| *b).or_else(|| top.get(first))?;
    let mut used = 1;
    while used < v.path.len() {
        let Binding::Rule { captures, .. } = b else { break };
        let Some(next) = captures.get(&v.path[used].name) else { break };
        b = next;
        used += 1;
    }
    Some((b, used))
}

/// A string literal's value, for `#error`'s message.
fn string_value(lit: &str) -> String {
    let inner = lit.strip_prefix('"').and_then(|s| s.strip_suffix('"'));
    let Some(inner) = inner else {
        return lit.trim_matches('`').to_owned();
    };
    let mut out = String::new();
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

/// Line breaks as the lexer leaves them (spec §2.2): never two in a row, none at the start,
/// none after a token that continues the line, and none before a closing `)` or `]`. A
/// capture that emitted nothing, or a `#for` that ran no times, would otherwise leave gaps
/// the lexer could never have produced.
pub(crate) fn normalise(tokens: Vec<MacroToken>) -> Vec<MacroToken> {
    let mut out: Vec<MacroToken> = Vec::with_capacity(tokens.len());
    for t in tokens {
        if t.kind == TokenKind::Terminator {
            match out.last() {
                None => continue,
                Some(p) if p.kind == TokenKind::Terminator => continue,
                Some(p) if p.kind.suppresses_following_terminator() => continue,
                _ => {}
            }
        } else if matches!(t.kind, TokenKind::RParen | TokenKind::RBracket) {
            while out.last().is_some_and(|p| p.kind == TokenKind::Terminator) {
                out.pop();
            }
        }
        out.push(t);
    }
    out
}
