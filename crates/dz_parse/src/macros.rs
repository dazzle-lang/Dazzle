//! Macro declarations and invocations (spec §12).
//!
//! The parser reads a macro declaration into its parts — `syntax` rules, `pattern`,
//! `expand` — and an invocation into the tokens after its name. It does not decide what
//! either means: whether a fragment kind exists, whether a `$x` was captured, and whether an
//! invocation fits are `dz_macro`'s questions, asked after parsing and before resolution.

use dz_ast::*;
use dz_diag::codes;
use dz_lex::TokenKind;

use crate::Parser;

fn is_open(k: TokenKind) -> bool {
    matches!(k, TokenKind::LParen | TokenKind::LBracket | TokenKind::LBrace)
}

fn is_close(k: TokenKind) -> bool {
    matches!(k, TokenKind::RParen | TokenKind::RBracket | TokenKind::RBrace)
}

impl<'a> Parser<'a> {
    /// The current token, with its text.
    pub(crate) fn macro_token(&self) -> MacroToken {
        let t = self.peek_token();
        MacroToken { kind: t.kind, span: t.span, text: self.cur_text().to_owned() }
    }

    fn bump_macro_token(&mut self) -> MacroToken {
        let m = self.macro_token();
        self.bump();
        m
    }

    /// Is the cursor at an intrinsic — `#file`, `#location(…)` — rather than a macro
    /// invocation? Intrinsics are the compiler's own macros and stay expressions (§12.7).
    pub(crate) fn at_intrinsic(&self) -> bool {
        self.at(TokenKind::Hash)
            && self.peek_at(1) == TokenKind::Ident
            && self.peek_at(2) != TokenKind::ColonColon
            && INTRINSICS.contains(&self.text_after(1))
    }

    /// Is the cursor at `#name` or `#dep::name` — an invocation, in any position?
    pub(crate) fn at_macro_call(&self) -> bool {
        self.at(TokenKind::Hash) && self.peek_at(1) == TokenKind::Ident && !self.at_intrinsic()
    }

    /// `#machine Foo(…) { … }` in declaration or statement position, which runs to the end
    /// of the line outside brackets; `#max(a, b)` in expression position, which is one
    /// bracketed group (spec §12.5).
    pub(crate) fn parse_macro_call(&mut self, in_expression: bool) -> MacroCall {
        let start = self.start();
        self.bump(); // `#`
        let path_start = self.start();
        let mut segments = Vec::new();
        loop {
            let name = self.parse_ident_or_error();
            segments.push(PathSegment { span: name.span, name, generics: Vec::new() });
            if !self.eat(TokenKind::ColonColon) {
                break;
            }
        }
        let path = Path { span: self.span_from(path_start), segments };
        let name_span = self.span_from(start);

        let mut tokens = Vec::new();
        if in_expression {
            let takes_group = matches!(self.peek(), TokenKind::LParen | TokenKind::LBracket)
                || (self.at(TokenKind::LBrace) && self.no_struct_literal == 0);
            if takes_group {
                self.collect_group(&mut tokens);
            }
        } else {
            self.collect_to_line_end(&mut tokens);
        }
        MacroCall { span: self.span_from(start), name_span, path, tokens }
    }

    /// One balanced bracketed group, brackets included.
    fn collect_group(&mut self, out: &mut Vec<MacroToken>) {
        let open = self.peek_token().span;
        let mut depth = 0i32;
        loop {
            let k = self.peek();
            if k == TokenKind::Eof {
                self.error_labeled(
                    codes::UNCLOSED_DELIMITER,
                    open,
                    "unclosed bracket in a macro invocation",
                    "this is never closed",
                );
                return;
            }
            if is_open(k) {
                depth += 1;
            } else if is_close(k) {
                depth -= 1;
            }
            out.push(self.bump_macro_token());
            if depth <= 0 {
                return;
            }
        }
    }

    /// Tokens up to the end of the line, outside brackets — or up to a `}` that closes the
    /// block the invocation is in.
    fn collect_to_line_end(&mut self, out: &mut Vec<MacroToken>) {
        let mut depth = 0i32;
        let mut open = None;
        loop {
            let k = self.peek();
            match k {
                TokenKind::Eof => {
                    if let Some(span) = open.filter(|_| depth > 0) {
                        self.error_labeled(
                            codes::UNCLOSED_DELIMITER,
                            span,
                            "unclosed bracket in a macro invocation",
                            "this is never closed",
                        );
                    }
                    return;
                }
                TokenKind::Terminator if depth == 0 => return,
                k if is_close(k) && depth == 0 => return,
                k if is_open(k) => {
                    if depth == 0 {
                        open = Some(self.peek_token().span);
                    }
                    depth += 1;
                }
                k if is_close(k) => depth -= 1,
                _ => {}
            }
            out.push(self.bump_macro_token());
        }
    }

    // --- declarations ----------------------------------------------------------

    /// `macro machine -> decl { syntax … pattern { … } expand { … } }` (spec §12.1).
    pub(crate) fn parse_macro_decl(&mut self) -> MacroDecl {
        let start = self.start();
        self.bump(); // `macro`
        let name = self.parse_ident_or_error();
        self.expect_in(TokenKind::Arrow, "before the macro's kind");
        let kind = if self.at(TokenKind::Ident) {
            self.parse_ident_or_error()
        } else {
            // `-> type`, should `type` ever lex as a keyword, still names a kind.
            let span = self.peek_token().span;
            self.error_help(
                codes::EXPECTED_TOKEN,
                span,
                "expected the macro's kind",
                "expected `decl`, `stmt` or `expr`",
                "a macro is `-> decl`, `-> stmt` or `-> expr` (spec §12.1)",
            );
            Ident::new(span, "<error>")
        };

        let body_start = self.start();
        let mut rules = Vec::new();
        let mut pattern = None;
        let mut expand = None;
        if self.expect_in(TokenKind::LBrace, "to begin the macro's body") {
            loop {
                self.skip_terminators();
                if self.at(TokenKind::RBrace) || self.at_end() {
                    break;
                }
                if self.stalled() {
                    continue;
                }
                let part = if self.at(TokenKind::Ident) { self.cur_text() } else { "" };
                match part {
                    "syntax" => {
                        let r_start = self.start();
                        self.bump();
                        let r_name = self.parse_ident_or_error();
                        let p = self.parse_braced_pattern();
                        rules.push(SyntaxRule {
                            span: self.span_from(r_start),
                            name: r_name,
                            pattern: p,
                        });
                    }
                    "pattern" => {
                        let span = self.bump().span;
                        let p = self.parse_braced_pattern();
                        if pattern.is_some() {
                            self.error_labeled(
                                codes::EXPECTED_TOKEN,
                                span,
                                "a macro has one `pattern`",
                                "a second `pattern`",
                            );
                        } else {
                            pattern = Some(p);
                        }
                    }
                    "expand" => {
                        let span = self.bump().span;
                        let t = self.parse_braced_template();
                        if expand.is_some() {
                            self.error_labeled(
                                codes::EXPECTED_TOKEN,
                                span,
                                "a macro has one `expand`",
                                "a second `expand`",
                            );
                        } else {
                            expand = Some(t);
                        }
                    }
                    _ => {
                        let span = self.peek_token().span;
                        let found = self.describe_current();
                        self.error_help(
                            codes::EXPECTED_TOKEN,
                            span,
                            format!("expected `syntax`, `pattern` or `expand`, found {found}"),
                            "not part of a macro",
                            "a macro's body is its `syntax` rules, one `pattern` and one \
                             `expand` (spec §12.1)",
                        );
                        self.skip_token_tree();
                    }
                }
            }
            if self.expect_in(TokenKind::RBrace, "to close the macro's body") {
                if pattern.is_none() {
                    self.error_help(
                        codes::EXPECTED_TOKEN,
                        name.span,
                        format!("macro `{}` has no `pattern`", name.name),
                        "no pattern",
                        "write `pattern { … }`: the grammar of what follows `#name` (spec §12.1)",
                    );
                }
                if expand.is_none() {
                    self.error_help(
                        codes::EXPECTED_TOKEN,
                        name.span,
                        format!("macro `{}` has no `expand`", name.name),
                        "no expansion",
                        "write `expand { … }`: the code an invocation becomes (spec §12.1)",
                    );
                }
            }
        }
        let body = self.span_from(body_start);
        MacroDecl { span: self.span_from(start), name, kind, rules, pattern, expand, body }
    }

    /// Skip one token, or one whole bracketed group.
    fn skip_token_tree(&mut self) {
        let mut depth = 0i32;
        loop {
            let k = self.peek();
            if k == TokenKind::Eof {
                return;
            }
            if is_open(k) {
                depth += 1;
            } else if is_close(k) {
                if depth == 0 {
                    return;
                }
                depth -= 1;
            }
            self.bump();
            if depth == 0 {
                return;
            }
        }
    }

    /// `{ … }` around a pattern.
    fn parse_braced_pattern(&mut self) -> Vec<PatElem> {
        if !self.expect_in(TokenKind::LBrace, "to begin the pattern") {
            return Vec::new();
        }
        let elems = self.parse_pattern(TokenKind::RBrace);
        self.expect_in(TokenKind::RBrace, "to close the pattern");
        elems
    }

    /// Pattern elements up to a `close` outside any literal bracket, which is left for the
    /// caller. Line breaks are layout and are not kept (spec §12.3).
    fn parse_pattern(&mut self, close: TokenKind) -> Vec<PatElem> {
        let mut elems = Vec::new();
        let mut depth = 0i32;
        loop {
            let k = self.peek();
            if k == TokenKind::Eof || (k == close && depth == 0) {
                return elems;
            }
            if k == TokenKind::Terminator || k == TokenKind::DocComment {
                self.bump();
                continue;
            }
            if k == TokenKind::Dollar {
                elems.push(self.parse_pattern_dollar());
                continue;
            }
            if is_open(k) {
                depth += 1;
            } else if is_close(k) {
                depth -= 1;
            }
            elems.push(PatElem::Token(self.bump_macro_token()));
        }
    }

    /// `$name: kind`, `$xs: [,]Kind`, `$x: kind?`, or `$( … )?`.
    fn parse_pattern_dollar(&mut self) -> PatElem {
        let start = self.start();
        self.bump(); // `$`
        if self.eat(TokenKind::LParen) {
            let elems = self.parse_pattern(TokenKind::RParen);
            self.expect_in(TokenKind::RParen, "to close the group");
            if !self.eat(TokenKind::Question) {
                let span = self.span_from(start);
                self.error_help(
                    codes::EXPECTED_TOKEN,
                    span,
                    "a group in a pattern is optional, `$( … )?`",
                    "expected `?` after this group",
                    "repetition is written on a capture, `$xs: []Rule`, with the repeated part \
                     as a `syntax` rule (spec §12.3)",
                );
            }
            return PatElem::Group { span: self.span_from(start), elems };
        }
        let name = self.parse_ident_or_error();
        self.expect_in(TokenKind::Colon, "between a capture's name and its kind");
        let mut shape = CaptureShape::One;
        if self.eat(TokenKind::LBracket) {
            let sep =
                if self.at(TokenKind::RBracket) { None } else { Some(self.bump_macro_token()) };
            self.expect_in(TokenKind::RBracket, "after a repetition's separator");
            shape = CaptureShape::Many { sep };
        }
        let kind = self.parse_ident_or_error();
        if self.at(TokenKind::Question) {
            let q = self.bump().span;
            if shape == CaptureShape::One {
                shape = CaptureShape::Optional;
            } else {
                self.error_help(
                    codes::EXPECTED_TOKEN,
                    q,
                    "a repetition cannot also be optional",
                    "remove this `?`",
                    "`[]K` already matches none (spec §12.3)",
                );
            }
        }
        PatElem::Capture { span: self.span_from(start), name, shape, kind }
    }

    /// `{ … }` around an expansion.
    fn parse_braced_template(&mut self) -> Vec<TemplateNode> {
        if !self.expect_in(TokenKind::LBrace, "to begin the expansion") {
            return Vec::new();
        }
        let nodes = self.parse_template();
        self.expect_in(TokenKind::RBrace, "to close the expansion");
        nodes
    }

    /// Template nodes up to a `}` outside any literal bracket, which is left for the
    /// caller. Line breaks are kept: in an expansion they end statements (spec §12.3).
    fn parse_template(&mut self) -> Vec<TemplateNode> {
        let mut nodes = Vec::new();
        let mut depth = 0i32;
        loop {
            let k = self.peek();
            if k == TokenKind::Eof || (k == TokenKind::RBrace && depth == 0) {
                return nodes;
            }
            if k == TokenKind::Dollar && self.peek_at(1) == TokenKind::Ident {
                nodes.push(TemplateNode::Var(self.parse_var_ref()));
                continue;
            }
            if k == TokenKind::Hash {
                if let Some(n) = self.parse_template_directive() {
                    nodes.push(n);
                    continue;
                }
            }
            if is_open(k) {
                depth += 1;
            } else if is_close(k) {
                depth -= 1;
            }
            nodes.push(TemplateNode::Token(self.bump_macro_token()));
        }
    }

    /// `$x`, `$s.name`. Whether `.name` is a field of the capture or Dazzle's own `.` is
    /// for `dz_macro` to say, which knows what `$s` captured.
    fn parse_var_ref(&mut self) -> VarRef {
        let start = self.start();
        self.bump(); // `$`
        let mut path = vec![self.parse_ident_or_error()];
        while self.at(TokenKind::Dot) && self.peek_at(1) == TokenKind::Ident {
            self.bump();
            path.push(self.parse_ident_or_error());
        }
        VarRef { span: self.span_from(start), path }
    }

    /// `#for`, `#if $…`, `#error(…)` — or `None` for any other `#`, which an expansion
    /// emits as it is: an attribute, an invocation, an intrinsic, or a build-flag `#if`.
    fn parse_template_directive(&mut self) -> Option<TemplateNode> {
        let start = self.start();
        match self.peek_at(1) {
            TokenKind::For => {
                self.bump(); // `#`
                self.bump(); // `for`
                let var = self.parse_ident_or_error();
                self.expect_in(TokenKind::In, "in `#for v in $xs`");
                let list = if self.at(TokenKind::Dollar) {
                    self.parse_var_ref()
                } else {
                    let span = self.peek_token().span;
                    self.error_help(
                        codes::EXPECTED_TOKEN,
                        span,
                        "`#for` iterates a capture",
                        "expected `$name` here",
                        "write `#for v in $xs { … }` (spec §12.3)",
                    );
                    VarRef { span, path: vec![Ident::new(span, "<error>")] }
                };
                let sep = if self.at(TokenKind::LBracket) {
                    self.bump();
                    let s = (!self.at(TokenKind::RBracket)).then(|| self.bump_macro_token());
                    self.expect_in(TokenKind::RBracket, "after the separator");
                    s
                } else {
                    None
                };
                let body = self.parse_template_block();
                Some(TemplateNode::For { span: self.span_from(start), var, list, sep, body })
            }
            TokenKind::If => {
                let negated = self.peek_at(2) == TokenKind::Bang;
                let dollar_at = if negated { 3 } else { 2 };
                if self.peek_at(dollar_at) != TokenKind::Dollar {
                    return None; // a build-flag `#if`, emitted as written (§12.10)
                }
                self.bump(); // `#`
                self.bump(); // `if`
                if negated {
                    self.bump();
                }
                let cond = self.parse_var_ref();
                let then = self.parse_template_block();
                let otherwise = if self.at(TokenKind::Hash) && self.peek_at(1) == TokenKind::Else {
                    self.bump();
                    self.bump();
                    Some(self.parse_template_block())
                } else {
                    None
                };
                Some(TemplateNode::If {
                    span: self.span_from(start),
                    negated,
                    cond,
                    then,
                    otherwise,
                })
            }
            TokenKind::Ident if self.text_after(1) == "error" => {
                self.bump(); // `#`
                self.bump(); // `error`
                self.expect_in(TokenKind::LParen, "after `#error`");
                let at = if self.at(TokenKind::Dollar) {
                    let v = self.parse_var_ref();
                    self.expect_in(TokenKind::Comma, "between `#error`'s capture and message");
                    Some(v)
                } else {
                    None
                };
                let message = if matches!(self.peek(), TokenKind::Str | TokenKind::RawStr) {
                    self.bump_macro_token()
                } else {
                    let span = self.peek_token().span;
                    self.error_help(
                        codes::EXPECTED_TOKEN,
                        span,
                        "`#error` needs a message",
                        "expected a string here",
                        "write `#error(\"…\")` or `#error($x, \"…\")` (spec §12.6)",
                    );
                    MacroToken { kind: TokenKind::Str, span, text: "\"\"".to_owned() }
                };
                self.expect_in(TokenKind::RParen, "to close `#error`");
                Some(TemplateNode::Error { span: self.span_from(start), at, message })
            }
            _ => None,
        }
    }

    /// The `{ … }` body of `#for` or `#if`.
    fn parse_template_block(&mut self) -> Vec<TemplateNode> {
        if !self.expect_in(TokenKind::LBrace, "to begin the body") {
            return Vec::new();
        }
        let nodes = self.parse_template();
        self.expect_in(TokenKind::RBrace, "to close the body");
        nodes
    }
}
