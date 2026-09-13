//! Recursive-descent parser with Pratt expression parsing (spec §20).
//!
//! **The parser always produces a tree.** It never returns early on an error: it reports,
//! synthesises an `Error` node, resynchronises, and continues. Spec §17.2 requires this
//! because the LSP runs this code on half-typed source on every keystroke.

use dz_ast::*;
use dz_diag::{codes, Diagnostic, Diagnostics};
use dz_lex::{Token, TokenKind};
use dz_span::{FileId, Span};

mod cfg;
mod expr;
mod item;
mod stmt;
mod ty;

pub struct ParseResult {
    pub ast: SourceFileAst,
    pub diagnostics: Diagnostics,
}

/// Parse a token stream produced by [`dz_lex::lex`].
pub fn parse(file: FileId, src: &str, tokens: &[Token]) -> ParseResult {
    let mut p = Parser {
        file,
        src,
        tokens,
        pos: 0,
        diags: Diagnostics::new(),
        no_struct_literal: 0,
        pending_gt: None,
        last_error_at: None,
        fuel: 0,
    };
    let ast = p.parse_file();
    ParseResult { ast, diagnostics: p.diags }
}

pub(crate) struct Parser<'a> {
    pub(crate) file: FileId,
    pub(crate) src: &'a str,
    tokens: &'a [Token],
    pos: usize,
    pub(crate) diags: Diagnostics,
    /// Non-zero while parsing the head of `if`/`while`/`for`/`switch`/`with`, where a `{`
    /// starts the body rather than a struct literal.
    pub(crate) no_struct_literal: u32,
    /// The second half of a `>>` that closed a nested generic argument list.
    pending_gt: Option<Span>,
    /// Token index of the most recent diagnostic, used to suppress cascades.
    last_error_at: Option<usize>,
    /// Guards against a recovery path that fails to consume input.
    fuel: u32,
}

impl<'a> Parser<'a> {
    // --- cursor -----------------------------------------------------------

    pub(crate) fn peek(&self) -> TokenKind {
        if self.pending_gt.is_some() {
            return TokenKind::Gt;
        }
        self.tokens.get(self.pos).map(|t| t.kind).unwrap_or(TokenKind::Eof)
    }

    pub(crate) fn peek_at(&self, n: usize) -> TokenKind {
        if self.pending_gt.is_some() {
            if n == 0 {
                return TokenKind::Gt;
            }
            return self.tokens.get(self.pos + n - 1).map(|t| t.kind).unwrap_or(TokenKind::Eof);
        }
        self.tokens.get(self.pos + n).map(|t| t.kind).unwrap_or(TokenKind::Eof)
    }

    pub(crate) fn peek_token(&self) -> Token {
        if let Some(span) = self.pending_gt {
            return Token::new(TokenKind::Gt, span);
        }
        self.tokens
            .get(self.pos)
            .copied()
            .unwrap_or_else(|| Token::new(TokenKind::Eof, self.eof_span()))
    }

    /// Close one level of generic arguments, splitting a `>>` if that is what the lexer
    /// produced for `Buffer<Vec<T>>`.
    ///
    /// The second `>` becomes a synthetic token that `peek`/`bump` serve transparently, so
    /// no other part of the parser has to know this happened.
    pub(crate) fn eat_gt(&mut self) -> bool {
        if self.pending_gt.is_some() {
            self.bump();
            return true;
        }
        if self.at(TokenKind::Shr) {
            let t = self.bump();
            self.pending_gt = Some(Span { start: t.span.start + 1, end: t.span.end });
            return true;
        }
        self.eat(TokenKind::Gt)
    }

    pub(crate) fn expect_gt(&mut self, context: &str) -> bool {
        if self.eat_gt() {
            return true;
        }
        let found = self.describe_current();
        let span = self.peek_token().span;
        self.error_labeled(
            codes::EXPECTED_TOKEN,
            span,
            format!("expected `>` {context}, found {found}"),
            "expected `>` here",
        );
        false
    }

    fn eof_span(&self) -> Span {
        self.tokens.last().map(|t| t.span).unwrap_or(Span::DUMMY)
    }

    pub(crate) fn at(&self, kind: TokenKind) -> bool {
        self.peek() == kind
    }

    pub(crate) fn at_end(&self) -> bool {
        self.peek() == TokenKind::Eof
    }

    pub(crate) fn bump(&mut self) -> Token {
        if let Some(span) = self.pending_gt.take() {
            self.fuel = 0;
            return Token::new(TokenKind::Gt, span);
        }
        let t = self.peek_token();
        if self.pos < self.tokens.len() {
            self.pos += 1;
        }
        self.fuel = 0;
        t
    }

    pub(crate) fn eat(&mut self, kind: TokenKind) -> bool {
        if self.at(kind) {
            self.bump();
            true
        } else {
            false
        }
    }

    /// Position for a span that starts at the current token.
    pub(crate) fn start(&self) -> u32 {
        self.peek_token().span.start
    }

    /// End of the previously consumed token, so spans do not swallow trailing trivia.
    pub(crate) fn prev_end(&self) -> u32 {
        if self.pos == 0 {
            0
        } else {
            self.tokens[self.pos - 1].span.end
        }
    }

    pub(crate) fn span_from(&self, start: u32) -> Span {
        Span { start, end: self.prev_end().max(start) }
    }

    pub(crate) fn text(&self, span: Span) -> &'a str {
        &self.src[span.range()]
    }

    pub(crate) fn token_text(&self, t: Token) -> &'a str {
        self.text(t.span)
    }

    // --- diagnostics ------------------------------------------------------

    /// One diagnostic per token position.
    ///
    /// Without this, a single mistake like `fn f( {` reports four times — a missing name,
    /// then a missing `:`, then a missing type, then a missing `)` — all pointing at the
    /// same character. Only the first is useful; the rest are the parser talking to
    /// itself.
    fn should_report(&mut self) -> bool {
        if self.last_error_at == Some(self.pos) {
            return false;
        }
        self.last_error_at = Some(self.pos);
        true
    }

    pub(crate) fn error_labeled(
        &mut self,
        code: &'static str,
        span: Span,
        msg: impl Into<String>,
        label: impl Into<String>,
    ) {
        if !self.should_report() {
            return;
        }
        self.diags.push(Diagnostic::error(code, msg.into()).at(self.file, span, label.into()));
    }

    pub(crate) fn error_help(
        &mut self,
        code: &'static str,
        span: Span,
        msg: impl Into<String>,
        label: impl Into<String>,
        help: impl Into<String>,
    ) {
        if !self.should_report() {
            return;
        }
        self.diags.push(
            Diagnostic::error(code, msg.into())
                .at(self.file, span, label.into())
                .with_help(help.into()),
        );
    }

    /// Describe the current token for an error message.
    pub(crate) fn describe_current(&self) -> String {
        let t = self.peek_token();
        match t.kind {
            TokenKind::Eof => "end of file".to_owned(),
            TokenKind::Terminator => "end of line".to_owned(),
            TokenKind::Ident => format!("`{}`", self.token_text(t)),
            k if k.is_keyword() => format!("keyword `{}`", k.name()),
            k if k.is_literal() => format!("`{}`", self.token_text(t)),
            k => format!("`{}`", k.name()),
        }
    }

    /// Consume `kind`, or report what was expected and leave the cursor where it is.
    pub(crate) fn expect_in(&mut self, kind: TokenKind, context: &str) -> bool {
        if self.eat(kind) {
            return true;
        }
        let found = self.describe_current();
        let span = self.peek_token().span;
        self.error_labeled(
            codes::EXPECTED_TOKEN,
            span,
            format!("expected `{}` {context}, found {found}", kind.name()),
            format!("expected `{}` here", kind.name()),
        );
        false
    }

    // --- trivia -----------------------------------------------------------

    pub(crate) fn skip_terminators(&mut self) {
        while self.at(TokenKind::Terminator) {
            self.bump();
        }
    }

    /// Collect doc comments preceding a declaration.
    pub(crate) fn eat_docs(&mut self) -> Docs {
        let mut docs = Vec::new();
        loop {
            match self.peek() {
                TokenKind::DocComment => docs.push(self.bump().span),
                // Blank lines between doc lines are fine; a real token ends the run.
                TokenKind::Terminator if !docs.is_empty() => {
                    if self.peek_at(1) == TokenKind::DocComment {
                        self.bump();
                    } else {
                        break;
                    }
                }
                _ => break,
            }
        }
        docs
    }

    /// Save the cursor so a speculative parse can be undone (see
    /// [`Parser::try_parse_generic_args`]).
    pub(crate) fn pos_checkpoint(&self) -> usize {
        self.pos
    }

    pub(crate) fn restore_checkpoint(&mut self, pos: usize) {
        self.pos = pos;
        self.pending_gt = None;
    }

    /// Discard diagnostics produced by a speculative parse that was rolled back.
    pub(crate) fn truncate_diags(&mut self, len: usize) {
        self.diags.truncate(len);
    }

    /// Detect a loop that is failing to make progress and force it forward.
    pub(crate) fn stalled(&mut self) -> bool {
        self.fuel += 1;
        if self.fuel > 64 {
            self.fuel = 0;
            if !self.at_end() {
                self.bump();
            }
            return true;
        }
        false
    }

    // --- identifiers ------------------------------------------------------

    pub(crate) fn parse_ident(&mut self) -> Option<Ident> {
        if self.at(TokenKind::Ident) {
            let t = self.bump();
            let name = self.token_text(t);
            if dz_lex::RESERVED.contains(&name) {
                self.error_help(
                    codes::RESERVED_KEYWORD,
                    t.span,
                    format!("`{name}` is reserved for future use"),
                    "reserved word",
                    "choose a different name (spec §2.7 lists the reserved words)",
                );
            }
            return Some(Ident::new(t.span, name));
        }
        let found = self.describe_current();
        let span = self.peek_token().span;
        self.error_labeled(
            codes::EXPECTED_TOKEN,
            span,
            format!("expected a name, found {found}"),
            "expected an identifier here",
        );
        None
    }

    pub(crate) fn parse_ident_or_error(&mut self) -> Ident {
        let span = self.peek_token().span;
        self.parse_ident().unwrap_or_else(|| Ident::new(span, "<error>"))
    }

    // --- recovery ---------------------------------------------------------

    /// Skip forward to something that can begin a declaration, or to `}` / EOF.
    pub(crate) fn recover_to_item(&mut self) {
        let mut depth = 0i32;
        loop {
            match self.peek() {
                TokenKind::Eof => return,
                TokenKind::LBrace => {
                    depth += 1;
                    self.bump();
                }
                TokenKind::RBrace => {
                    if depth == 0 {
                        return;
                    }
                    depth -= 1;
                    self.bump();
                }
                k if depth == 0 && starts_item(k) => return,
                _ => {
                    self.bump();
                }
            }
        }
    }

    /// Skip to the end of the current statement.
    pub(crate) fn recover_to_statement_end(&mut self) {
        let mut depth = 0i32;
        loop {
            match self.peek() {
                TokenKind::Eof => return,
                TokenKind::Terminator if depth == 0 => return,
                TokenKind::RBrace if depth == 0 => return,
                TokenKind::LParen | TokenKind::LBracket | TokenKind::LBrace => {
                    depth += 1;
                    self.bump();
                }
                TokenKind::RParen | TokenKind::RBracket | TokenKind::RBrace => {
                    depth -= 1;
                    self.bump();
                }
                _ => {
                    self.bump();
                }
            }
        }
    }

    /// Consume a balanced `{ ... }` group and return its span, for constructs whose
    /// interior is not parsed yet (macros — spec §12, deferred to M10).
    pub(crate) fn consume_braced_group(&mut self) -> Span {
        let start = self.start();
        if !self.at(TokenKind::LBrace) {
            return self.span_from(start);
        }
        let open = self.bump().span;
        let mut depth = 1i32;
        while depth > 0 {
            match self.peek() {
                TokenKind::Eof => {
                    self.error_labeled(
                        codes::UNCLOSED_DELIMITER,
                        open,
                        "unclosed `{`",
                        "this brace is never closed",
                    );
                    break;
                }
                TokenKind::LBrace => {
                    depth += 1;
                    self.bump();
                }
                TokenKind::RBrace => {
                    depth -= 1;
                    self.bump();
                }
                _ => {
                    self.bump();
                }
            }
        }
        self.span_from(start)
    }

    /// Consume a balanced `( ... )` group and return its span.
    pub(crate) fn consume_paren_group(&mut self) -> Span {
        let start = self.start();
        if !self.at(TokenKind::LParen) {
            return self.span_from(start);
        }
        let open = self.bump().span;
        let mut depth = 1i32;
        while depth > 0 {
            match self.peek() {
                TokenKind::Eof => {
                    self.error_labeled(
                        codes::UNCLOSED_DELIMITER,
                        open,
                        "unclosed `(`",
                        "this parenthesis is never closed",
                    );
                    break;
                }
                TokenKind::LParen => {
                    depth += 1;
                    self.bump();
                }
                TokenKind::RParen => {
                    depth -= 1;
                    self.bump();
                }
                _ => {
                    self.bump();
                }
            }
        }
        self.span_from(start)
    }
}

/// Tokens that can begin a top-level declaration, used as resynchronisation points.
pub(crate) fn starts_item(k: TokenKind) -> bool {
    use TokenKind::*;
    // `unsafe` is not here: it begins a block as often as an `unsafe fn`, and the statement
    // parser asks this question to tell a nested declaration from a statement.
    matches!(
        k,
        Import
            | Pub
            | Fn
            | Mut
            | Struct
            | Union
            | Enum
            | Trait
            | Impl
            | Const
            | Static
            | Macro
            | Hash
    )
}
