//! Blocks and statements (spec §10).

use dz_ast::*;
use dz_diag::codes;
use dz_lex::TokenKind;

use crate::{item::item_starts_here, Parser};

fn assign_op(k: TokenKind) -> Option<AssignOp> {
    Some(match k {
        TokenKind::Eq => AssignOp::Assign,
        TokenKind::PlusEq => AssignOp::Add,
        TokenKind::MinusEq => AssignOp::Sub,
        TokenKind::StarEq => AssignOp::Mul,
        TokenKind::SlashEq => AssignOp::Div,
        TokenKind::PercentEq => AssignOp::Rem,
        _ => return None,
    })
}

impl<'a> Parser<'a> {
    pub(crate) fn parse_block(&mut self) -> Block {
        let start = self.start();
        if !self.expect_in(TokenKind::LBrace, "to begin a block") {
            return Block { span: self.span_from(start), stmts: Vec::new() };
        }
        self.parse_block_body(start)
    }

    /// Parse statements up to the closing `}`. The opening brace is already consumed.
    pub(crate) fn parse_block_body(&mut self, start: u32) -> Block {
        let mut stmts = Vec::new();
        // A block body is a fresh expression context.
        let saved = std::mem::replace(&mut self.no_struct_literal, 0);
        loop {
            self.skip_terminators();
            if self.at(TokenKind::RBrace) || self.at_end() {
                break;
            }
            if self.stalled() {
                continue;
            }
            stmts.push(self.parse_stmt());
        }
        self.no_struct_literal = saved;
        self.expect_in(TokenKind::RBrace, "to close a block");
        Block { span: self.span_from(start), stmts }
    }

    pub(crate) fn parse_stmt(&mut self) -> Stmt {
        let start = self.start();
        let docs = self.eat_docs();
        self.skip_terminators();

        // `#if FLAG { … }` among statements (spec §12.10). Its closing `}` ends it, as a
        // block's does.
        if self.at_cfg_if() {
            let c = self.parse_cfg_if("a statement", &mut |p| Some(p.parse_stmt()));
            return Stmt { span: self.span_from(start), docs, kind: StmtKind::CfgIf(c) };
        }
        let kind = match self.peek() {
            TokenKind::Let => StmtKind::Let(self.parse_let()),
            TokenKind::While => StmtKind::While(self.parse_while(None)),
            TokenKind::For => StmtKind::For(self.parse_for(None)),
            TokenKind::Defer => {
                self.bump();
                // The inner statement consumes its own terminator, so return directly
                // rather than falling through to `expect_statement_end`.
                let inner = self.parse_stmt();
                return Stmt {
                    span: self.span_from(start),
                    docs,
                    kind: StmtKind::Defer(Box::new(inner)),
                };
            }
            // `outer: while ...` / `outer: for ...` (spec §10.2)
            TokenKind::Ident
                if self.peek_at(1) == TokenKind::Colon
                    && matches!(self.peek_at(2), TokenKind::While | TokenKind::For) =>
            {
                let label = self.parse_ident();
                self.bump(); // `:`
                if self.at(TokenKind::While) {
                    StmtKind::While(self.parse_while(label))
                } else {
                    StmtKind::For(self.parse_for(label))
                }
            }
            k if item_starts_here(k) && !self.at(TokenKind::Hash) => match self.parse_item() {
                Some(item) => StmtKind::Item(Box::new(item)),
                None => {
                    self.recover_to_statement_end();
                    StmtKind::Error
                }
            },
            // `#trace "here"` — an invocation in statement position, to the end of the line
            // (spec §12.5). `#location` is an intrinsic, and so an expression.
            TokenKind::Hash if self.at_macro_call() => {
                StmtKind::Macro(self.parse_macro_call(false))
            }
            // `#[attr] fn ...` nested in a body, but `#location` is an expression.
            TokenKind::Hash if self.peek_at(1) == TokenKind::LBracket => match self.parse_item() {
                Some(item) => StmtKind::Item(Box::new(item)),
                None => {
                    self.recover_to_statement_end();
                    StmtKind::Error
                }
            },
            _ => {
                let expr = self.parse_expr();
                if let Some(op) = assign_op(self.peek()) {
                    self.bump();
                    let value = self.parse_expr();
                    StmtKind::Assign { target: expr, op, value }
                } else {
                    StmtKind::Expr(expr)
                }
            }
        };

        let span = self.span_from(start);
        self.expect_statement_end();
        Stmt { span, docs, kind }
    }

    /// A statement ends at a terminator, a `}`, or end of file.
    fn expect_statement_end(&mut self) {
        match self.peek() {
            TokenKind::Terminator => {
                self.bump();
            }
            TokenKind::RBrace | TokenKind::Eof => {}
            _ => {
                let span = self.peek_token().span;
                let found = self.describe_current();
                // Assignment is a statement, so `a = b = c` gets a targeted message.
                if assign_op(self.peek()).is_some() {
                    self.error_help(
                        codes::ASSIGN_IN_EXPRESSION,
                        span,
                        "assignment is not an expression",
                        "a second assignment cannot appear here",
                        "assignment is a statement (spec §11.1) — split this into two lines",
                    );
                } else {
                    self.error_help(
                        codes::EXPECTED_TOKEN,
                        span,
                        format!("expected end of statement, found {found}"),
                        "expected a newline or `;` here",
                        "statements are separated by newlines; `;` is optional (spec §2.2)",
                    );
                }
                self.recover_to_statement_end();
                self.eat(TokenKind::Terminator);
            }
        }
    }

    fn parse_let(&mut self) -> LetStmt {
        let start = self.start();
        self.bump(); // `let`
        let mutable = self.eat(TokenKind::Mut);
        let pat = self.parse_pat();
        let ty = if self.eat(TokenKind::Colon) { Some(self.parse_type()) } else { None };
        let init = if self.eat(TokenKind::Eq) { Some(self.parse_expr()) } else { None };

        if ty.is_none() && init.is_none() {
            let span = self.span_from(start);
            self.error_help(
                codes::EXPECTED_TYPE,
                span,
                "`let` needs a type or an initialiser",
                "neither a type nor a value is given",
                "write `let x: T` to default-initialise (spec §6.2), or `let x = value`",
            );
        }

        LetStmt { span: self.span_from(start), mutable, pat, ty, init }
    }

    pub(crate) fn parse_pat(&mut self) -> Pat {
        let start = self.start();
        let kind = match self.peek() {
            TokenKind::Ident => {
                if self.cur_text() == "_" {
                    self.bump();
                    PatKind::Wild
                } else {
                    PatKind::Ident(self.parse_ident_or_error())
                }
            }
            // `let (x, y) = point()`
            TokenKind::LParen => {
                self.bump();
                let mut elems = Vec::new();
                loop {
                    self.skip_terminators();
                    if self.at(TokenKind::RParen) || self.at_end() {
                        break;
                    }
                    if self.stalled() {
                        continue;
                    }
                    elems.push(self.parse_pat());
                    if !self.eat(TokenKind::Comma) {
                        break;
                    }
                }
                self.expect_in(TokenKind::RParen, "to close a tuple pattern");
                PatKind::Tuple(elems)
            }
            // A `&` here is someone reaching for a reference binding, which `if let`, `for`
            // and `case` have (§10.3, §10.4, §18.61) and a plain `let` does not. The bare
            // "expected a name" left them with no way to find the form that does work —
            // and there is no `or` clause that binds by reference, since `or` only ever
            // appears in a `let`, so `if let … else` is the whole answer.
            TokenKind::Amp => {
                let span = self.peek_token().span;
                self.error_help(
                    codes::EXPECTED_PATTERN,
                    span,
                    "expected a binding name, found `&`",
                    "a `let` binding is always by value",
                    "reference bindings are written in `if let`, `for` and `case`: \
                     `if let &name = value { … } else { … }` refers to the value where it \
                     lives, and its `else` can diverge the way an `or` clause would \
                     (spec §9.4, §18.61)",
                );
                PatKind::Error
            }
            _ => {
                let span = self.peek_token().span;
                let found = self.describe_current();
                self.error_labeled(
                    codes::EXPECTED_PATTERN,
                    span,
                    format!("expected a binding name, found {found}"),
                    "expected a name, `_`, or a tuple pattern",
                );
                PatKind::Error
            }
        };
        Pat { span: self.span_from(start), kind }
    }

    fn parse_while(&mut self, label: Option<Ident>) -> WhileStmt {
        let start = label.as_ref().map(|l| l.span.start).unwrap_or_else(|| self.start());
        self.bump(); // `while`
        let cond = self.parse_expr_no_struct();
        let body = self.parse_block();
        WhileStmt { span: self.span_from(start), label, cond, body }
    }

    fn parse_for(&mut self, label: Option<Ident>) -> ForStmt {
        let start = label.as_ref().map(|l| l.span.start).unwrap_or_else(|| self.start());
        self.bump(); // `for`
        let mut bindings = vec![self.parse_for_binding()];
        while self.eat(TokenKind::Comma) {
            bindings.push(self.parse_for_binding());
        }
        self.expect_in(TokenKind::In, "in a `for` loop");
        let iter = self.parse_expr_no_struct();
        let body = self.parse_block();
        ForStmt { span: self.span_from(start), label, bindings, iter, body }
    }

    /// `e`, `&e` or `&mut e` (spec §10.4).
    fn parse_for_binding(&mut self) -> ForBinding {
        let start = self.start();
        let mode = self.parse_bind_mode();
        let pat = self.parse_pat();
        ForBinding { span: self.span_from(start), mode, pat }
    }

    /// The `&` or `&mut` in front of a `for` or `case` binding, if any.
    pub(crate) fn parse_bind_mode(&mut self) -> BindMode {
        if !self.eat(TokenKind::Amp) {
            return BindMode::Value;
        }
        if self.eat(TokenKind::Mut) {
            BindMode::RefMut
        } else {
            BindMode::Ref
        }
    }
}
