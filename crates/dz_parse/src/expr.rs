//! Pratt expression parsing, with the precedence table from spec §11.1.

use dz_ast::*;
use dz_diag::codes;
use dz_lex::TokenKind;

use crate::Parser;

/// Left and right binding powers. Higher binds tighter; `(bp, bp + 1)` is left-associative.
///
/// Mirrors spec §11.1 read from the bottom up: `or` is loosest, `* / %` tightest of the
/// infix operators. Postfix (`.`, `()`, `[]`, `^`) and `as` bind tighter still and are
/// handled in the postfix loop rather than here.
fn infix_bp(k: TokenKind) -> Option<(u8, u8)> {
    use TokenKind::*;
    Some(match k {
        Or => (1, 2),
        PipePipe => (2, 3),
        AmpAmp => (3, 4),
        EqEq | BangEq | Lt | Gt | LtEq | GtEq => (4, 5),
        DotDot | DotDotEq => (5, 6),
        Pipe => (6, 7),
        CaretCaret => (7, 8),
        Amp => (8, 9),
        Shl | Shr => (9, 10),
        Plus | Minus => (10, 11),
        Star | Slash | Percent => (11, 12),
        // `use TokenKind::*` puts `None` in scope as the `none` literal token.
        _ => return Option::None,
    })
}

fn binary_op(k: TokenKind) -> Option<BinaryOp> {
    use TokenKind as T;
    Some(match k {
        T::Plus => BinaryOp::Add,
        T::Minus => BinaryOp::Sub,
        T::Star => BinaryOp::Mul,
        T::Slash => BinaryOp::Div,
        T::Percent => BinaryOp::Rem,
        T::Shl => BinaryOp::Shl,
        T::Shr => BinaryOp::Shr,
        T::Amp => BinaryOp::BitAnd,
        T::CaretCaret => BinaryOp::BitXor,
        T::Pipe => BinaryOp::BitOr,
        T::EqEq => BinaryOp::Eq,
        T::BangEq => BinaryOp::Ne,
        T::Lt => BinaryOp::Lt,
        T::Gt => BinaryOp::Gt,
        T::LtEq => BinaryOp::Le,
        T::GtEq => BinaryOp::Ge,
        T::AmpAmp => BinaryOp::And,
        T::PipePipe => BinaryOp::Or,
        _ => return None,
    })
}

impl<'a> Parser<'a> {
    pub(crate) fn parse_expr(&mut self) -> Expr {
        self.parse_expr_bp(0)
    }

    /// Parse where a `{` would start a block, not a struct literal (spec §7.3).
    pub(crate) fn parse_expr_no_struct(&mut self) -> Expr {
        self.no_struct_literal += 1;
        let e = self.parse_expr_bp(0);
        self.no_struct_literal -= 1;
        e
    }

    fn parse_expr_bp(&mut self, min_bp: u8) -> Expr {
        let start = self.start();
        let mut lhs = self.parse_prefix();
        lhs = self.parse_postfix(lhs);

        loop {
            let op_kind = self.peek();
            let Some((l_bp, r_bp)) = infix_bp(op_kind) else { break };
            if l_bp < min_bp {
                break;
            }

            if op_kind == TokenKind::Or {
                self.bump();
                let clause = self.parse_or_clause();
                lhs = Expr {
                    span: self.span_from(start),
                    kind: ExprKind::Or { expr: Box::new(lhs), clause: Box::new(clause) },
                };
                // `a() or b() or c` is rejected rather than silently misparsed. An `or`
                // clause already produces the success type or diverges (spec §9.4), so a
                // second `or` can only apply to a value that has stopped being fallible —
                // it reads as a fallback chain and is not one.
                if self.at(TokenKind::Or) {
                    let span = self.peek_token().span;
                    self.error_help(
                        codes::CHAINED_OR,
                        span,
                        "`or` clauses cannot be chained",
                        "this `or` follows another",
                        "`or` is non-associative — the first clause already produced a \
                         value or diverged, so nothing here can fail (spec §9.4)",
                    );
                    break;
                }
                continue;
            }

            if matches!(op_kind, TokenKind::DotDot | TokenKind::DotDotEq) {
                let inclusive = op_kind == TokenKind::DotDotEq;
                self.bump();
                let end = if self.starts_expr() {
                    Some(Box::new(self.parse_expr_bp(r_bp)))
                } else {
                    None
                };
                lhs = Expr {
                    span: self.span_from(start),
                    kind: ExprKind::Range { start: Some(Box::new(lhs)), end, inclusive },
                };
                continue;
            }

            let op_tok = self.bump();
            let op = binary_op(op_kind).expect("infix_bp and binary_op must agree");
            let rhs = self.parse_expr_bp(r_bp);

            // `a < b < c` is rejected rather than silently misparsed (spec §11.1).
            if op.is_comparison() {
                if let Some(next) = binary_op(self.peek()) {
                    if next.is_comparison() {
                        let span = self.peek_token().span;
                        self.error_help(
                            codes::CHAINED_COMPARISON,
                            span,
                            "comparison operators cannot be chained",
                            "this comparison follows another",
                            format!(
                                "comparison is non-associative — write `(a {} b) && (b {} c)`",
                                op.as_str(),
                                next.as_str()
                            ),
                        );
                    }
                }
            }
            let _ = op_tok;

            lhs = Expr {
                span: self.span_from(start),
                kind: ExprKind::Binary { op, lhs: Box::new(lhs), rhs: Box::new(rhs) },
            };
        }

        lhs
    }

    /// The right-hand side of an `or` (spec §9.4).
    ///
    /// `return`, `break`, `continue` and `panic(...)` need no special handling here: they
    /// are already expressions, and the single rule — produce a `T` or diverge — is a type
    /// question rather than a syntactic one, so the checker enforces it, not the parser.
    fn parse_or_clause(&mut self) -> OrClause {
        match self.peek() {
            TokenKind::LBrace => {
                let binding = self.parse_closure_binding();
                let body = self.parse_block_after_binding(binding.is_some());
                OrClause::Block { binding, body }
            }
            TokenKind::Ignore => OrClause::Ignore(self.bump().span),
            TokenKind::Unreachable => OrClause::Unreachable(self.bump().span),
            _ => OrClause::Value(self.parse_expr_bp(2)),
        }
    }

    // --- prefix -----------------------------------------------------------

    fn parse_prefix(&mut self) -> Expr {
        let start = self.start();
        let kind = match self.peek() {
            TokenKind::Minus => {
                self.bump();
                ExprKind::Unary { op: UnaryOp::Neg, operand: Box::new(self.parse_unary_operand()) }
            }
            TokenKind::Bang => {
                self.bump();
                ExprKind::Unary { op: UnaryOp::Not, operand: Box::new(self.parse_unary_operand()) }
            }
            TokenKind::Tilde => {
                self.bump();
                ExprKind::Unary {
                    op: UnaryOp::BitNot,
                    operand: Box::new(self.parse_unary_operand()),
                }
            }
            TokenKind::Amp => {
                self.bump();
                let mutable = self.eat(TokenKind::Mut);
                ExprKind::Ref { mutable, expr: Box::new(self.parse_unary_operand()) }
            }
            TokenKind::Move => {
                self.bump();
                // Spec §20 spells this `move ( expr )`.
                let parens = self.eat(TokenKind::LParen);
                let inner = self.parse_expr();
                if parens {
                    self.expect_in(TokenKind::RParen, "to close `move(...)`");
                }
                ExprKind::Move(Box::new(inner))
            }
            TokenKind::Return => {
                self.bump();
                let value = if self.starts_expr() { Some(Box::new(self.parse_expr())) } else { None };
                ExprKind::Return(value)
            }
            TokenKind::Break => {
                self.bump();
                let label = if self.at(TokenKind::Ident) { self.parse_ident() } else { None };
                // `break` carries no value (spec §10.2, §18.28). One is still parsed, so the
                // rest of the line is consumed rather than cascading into more errors.
                if self.starts_expr() {
                    let value = self.parse_expr();
                    self.error_help(
                        codes::BREAK_WITH_VALUE,
                        value.span,
                        "`break` does not take a value",
                        "a value here",
                        "a loop is a statement, not an expression — to leave one with a result, \
                         assign it to a variable declared before the loop (spec §10.2)",
                    );
                }
                ExprKind::Break { label }
            }
            TokenKind::Unsafe => {
                self.bump();
                if self.at(TokenKind::LBrace) {
                    ExprKind::Unsafe(self.parse_block())
                } else {
                    self.expect_in(TokenKind::LBrace, "to open the `unsafe` block");
                    ExprKind::Error
                }
            }
            TokenKind::Continue => {
                self.bump();
                let label = if self.at(TokenKind::Ident) { self.parse_ident() } else { None };
                ExprKind::Continue { label }
            }
            TokenKind::DotDot | TokenKind::DotDotEq => {
                let inclusive = self.peek() == TokenKind::DotDotEq;
                self.bump();
                let end = if self.starts_expr() { Some(Box::new(self.parse_expr_bp(6)))} else { None };
                ExprKind::Range { start: None, end, inclusive }
            }
            _ => return self.parse_primary(),
        };
        Expr { span: self.span_from(start), kind }
    }

    /// Operand of a prefix operator: binds tighter than any infix operator.
    fn parse_unary_operand(&mut self) -> Expr {
        let e = self.parse_prefix();
        self.parse_postfix(e)
    }

    // --- postfix ----------------------------------------------------------

    fn parse_postfix(&mut self, mut lhs: Expr) -> Expr {
        let start = lhs.span.start;
        loop {
            match self.peek() {
                TokenKind::Dot => {
                    // `..` is a range, not two field accesses.
                    self.bump();
                    let name = if self.at(TokenKind::Int) {
                        // Tuple access: `t.0`
                        let t = self.bump();
                        Ident::new(t.span, self.token_text(t))
                    } else {
                        self.parse_ident_or_error()
                    };
                    lhs = Expr {
                        span: self.span_from(start),
                        kind: ExprKind::Field { base: Box::new(lhs), name },
                    };
                }
                TokenKind::LParen => {
                    let args = self.parse_call_args();
                    lhs = Expr {
                        span: self.span_from(start),
                        kind: ExprKind::Call { callee: Box::new(lhs), args },
                    };
                }
                TokenKind::LBracket => {
                    self.bump();
                    let index = self.parse_expr();
                    self.expect_in(TokenKind::RBracket, "to close an index");
                    lhs = Expr {
                        span: self.span_from(start),
                        kind: ExprKind::Index { base: Box::new(lhs), index: Box::new(index) },
                    };
                }
                // Postfix `^` is dereference; xor is `^^`, so there is no ambiguity.
                TokenKind::Caret => {
                    self.bump();
                    lhs = Expr {
                        span: self.span_from(start),
                        kind: ExprKind::Deref(Box::new(lhs)),
                    };
                }
                TokenKind::As => {
                    self.bump();
                    let ty = self.parse_type();
                    lhs = Expr {
                        span: self.span_from(start),
                        kind: ExprKind::Cast { expr: Box::new(lhs), ty },
                    };
                }
                _ => break,
            }
        }
        lhs
    }

    /// The elements of `[a, b, c]` (spec §5.3), with a trailing comma allowed.
    fn parse_array_lit_elements(&mut self) -> Vec<Expr> {
        let mut elems = Vec::new();
        self.bump(); // `[`
        // Struct literals are legal again inside brackets, so `[Vec3{1, 2, 3}]` parses
        // even in a position where a bare `{` would have begun a block.
        let saved = std::mem::replace(&mut self.no_struct_literal, 0);
        loop {
            self.skip_terminators();
            if self.at(TokenKind::RBracket) || self.at_end() {
                break;
            }
            if self.stalled() {
                continue;
            }
            elems.push(self.parse_expr());
            self.skip_terminators();
            if !self.eat(TokenKind::Comma) {
                break;
            }
        }
        self.skip_terminators();
        self.expect_in(TokenKind::RBracket, "to close the array literal");
        self.no_struct_literal = saved;
        elems
    }

    fn parse_call_args(&mut self) -> Vec<Expr> {
        let mut args = Vec::new();
        self.bump(); // `(`
        // Struct literals are legal again inside parentheses.
        let saved = std::mem::replace(&mut self.no_struct_literal, 0);
        loop {
            self.skip_terminators();
            if self.at(TokenKind::RParen) || self.at_end() {
                break;
            }
            if self.stalled() {
                continue;
            }
            args.push(self.parse_expr());
            self.skip_terminators();
            if !self.eat(TokenKind::Comma) {
                break;
            }
        }
        self.skip_terminators();
        self.expect_in(TokenKind::RParen, "to close the argument list");
        self.no_struct_literal = saved;
        args
    }

    // --- primary ----------------------------------------------------------

    fn parse_primary(&mut self) -> Expr {
        let start = self.start();
        let kind = match self.peek() {
            TokenKind::Int => {
                let t = self.bump();
                ExprKind::Int(self.token_text(t).to_owned())
            }
            TokenKind::Float => {
                let t = self.bump();
                ExprKind::Float(self.token_text(t).to_owned())
            }
            TokenKind::Str => {
                let t = self.bump();
                ExprKind::Str(self.token_text(t).to_owned())
            }
            TokenKind::RawStr => {
                let t = self.bump();
                ExprKind::RawStr(self.token_text(t).to_owned())
            }
            TokenKind::Char => {
                let t = self.bump();
                ExprKind::Char(self.token_text(t).to_owned())
            }
            TokenKind::True => {
                self.bump();
                ExprKind::Bool(true)
            }
            TokenKind::False => {
                self.bump();
                ExprKind::Bool(false)
            }
            TokenKind::Null => {
                self.bump();
                ExprKind::Null
            }
            TokenKind::None => {
                self.bump();
                ExprKind::None
            }
            TokenKind::If => return self.parse_if(),
            TokenKind::Switch => return self.parse_switch(),
            TokenKind::LBrace => {
                let block = self.parse_block();
                ExprKind::Block(block)
            }
            // `[1, 2, 3]` — an array literal (spec §5.3). A `[` can only begin one here:
            // indexing is postfix, so it never reaches primary position.
            TokenKind::LBracket => ExprKind::ArrayLit(self.parse_array_lit_elements()),
            // `.Transition{}` — variant inferred from context (spec §7.3).
            TokenKind::Dot if self.peek_at(1) == TokenKind::Ident => {
                self.bump();
                let name = self.parse_ident_or_error();
                let init = if self.at(TokenKind::LBrace) && self.no_struct_literal == 0 {
                    Some(self.parse_struct_lit_body())
                } else {
                    None
                };
                ExprKind::DotInfer { name, init }
            }
            // `#location`, `#file` (spec §12.7)
            TokenKind::Hash if self.peek_at(1) == TokenKind::Ident => {
                self.bump();
                let name = self.parse_ident_or_error();
                let args =
                    if self.at(TokenKind::LParen) { self.parse_call_args() } else { Vec::new() };
                ExprKind::Intrinsic { name, args }
            }
            TokenKind::LParen => {
                self.bump();
                let saved = std::mem::replace(&mut self.no_struct_literal, 0);
                let mut elems = Vec::new();
                let mut trailing_comma = false;
                loop {
                    self.skip_terminators();
                    if self.at(TokenKind::RParen) || self.at_end() {
                        break;
                    }
                    if self.stalled() {
                        continue;
                    }
                    elems.push(self.parse_expr());
                    self.skip_terminators();
                    if self.eat(TokenKind::Comma) {
                        trailing_comma = true;
                    } else {
                        trailing_comma = false;
                        break;
                    }
                }
                self.skip_terminators();
                self.expect_in(TokenKind::RParen, "to close a parenthesised expression");
                self.no_struct_literal = saved;
                if elems.len() == 1 && !trailing_comma {
                    // A plain parenthesised expression keeps the inner node, widened to
                    // include the parentheses so diagnostics point at what was written.
                    let mut e = elems.pop().unwrap();
                    e.span = self.span_from(start);
                    return e;
                }
                ExprKind::Tuple(elems)
            }
            TokenKind::SelfValue | TokenKind::SelfType | TokenKind::Ident => {
                // `name!(...)` — an expression macro (spec §12.5), raw until M10.
                if self.at(TokenKind::Ident) && self.peek_at(1) == TokenKind::Bang
                    && self.peek_at(2) == TokenKind::LParen
                {
                    let name = self.parse_ident_or_error();
                    self.bump(); // `!`
                    let body = self.consume_paren_group();
                    return Expr {
                        span: self.span_from(start),
                        kind: ExprKind::MacroCall { name, body },
                    };
                }

                let path = match self.peek() {
                    TokenKind::SelfValue | TokenKind::SelfType => {
                        let t = self.bump();
                        let mut segments = vec![PathSegment {
                            span: t.span,
                            name: Ident::new(t.span, t.kind.name()),
                            generics: Vec::new(),
                        }];
                        while self.eat(TokenKind::ColonColon) {
                            let seg_start = self.start();
                            let name = self.parse_ident_or_error();
                            let generics = if self.at(TokenKind::Lt) {
                                self.try_parse_generic_args().unwrap_or_default()
                            } else {
                                Vec::new()
                            };
                            segments.push(PathSegment {
                                span: self.span_from(seg_start),
                                name,
                                generics,
                            });
                        }
                        Path { span: self.span_from(start), segments }
                    }
                    _ => match self.parse_path() {
                        Some(p) => p,
                        None => {
                            return Expr { span: self.span_from(start), kind: ExprKind::Error }
                        }
                    },
                };

                if self.at(TokenKind::LBrace) && self.no_struct_literal == 0 {
                    let fields = self.parse_struct_lit_body();
                    ExprKind::StructLit { path, fields }
                } else {
                    ExprKind::Path(path)
                }
            }
            _ => {
                let span = self.peek_token().span;
                let found = self.describe_current();
                self.error_labeled(
                    codes::EXPECTED_EXPR,
                    span,
                    format!("expected an expression, found {found}"),
                    "expected an expression here",
                );
                // Do not consume a terminator or closing brace: the caller resynchronises
                // on those, and eating one here would cascade.
                if !matches!(
                    self.peek(),
                    TokenKind::Terminator
                        | TokenKind::RBrace
                        | TokenKind::RParen
                        | TokenKind::RBracket
                        | TokenKind::Eof
                ) {
                    self.bump();
                }
                ExprKind::Error
            }
        };
        Expr { span: self.span_from(start), kind }
    }

    fn parse_struct_lit_body(&mut self) -> Vec<FieldInit> {
        let mut fields = Vec::new();
        self.bump(); // `{`
        let saved = std::mem::replace(&mut self.no_struct_literal, 0);
        loop {
            self.skip_terminators();
            if self.at(TokenKind::RBrace) || self.at_end() {
                break;
            }
            if self.stalled() {
                continue;
            }
            let f_start = self.start();
            // `name: value`, or a positional value like `Vec3{1, 2, 3}`.
            let name = if self.at(TokenKind::Ident) && self.peek_at(1) == TokenKind::Colon {
                let id = self.parse_ident_or_error();
                self.bump(); // `:`
                Some(id)
            } else {
                None
            };
            let value = self.parse_expr();
            fields.push(FieldInit { span: self.span_from(f_start), name, value });
            self.skip_terminators();
            if !self.eat(TokenKind::Comma) {
                break;
            }
        }
        self.skip_terminators();
        self.expect_in(TokenKind::RBrace, "to close a struct literal");
        self.no_struct_literal = saved;
        fields
    }

    // --- if / switch / with -----------------------------------------------

    fn parse_if(&mut self) -> Expr {
        let start = self.start();
        self.bump(); // `if`
        // `if let name = value` (spec §9.4). A missing name or `=` is reported and the
        // rest still parses, as for any `if`.
        let binding = if self.eat(TokenKind::Let) {
            let mutable = self.eat(TokenKind::Mut);
            let name = self.parse_ident_or_error();
            self.expect_in(TokenKind::Eq, "after the `if let` binding");
            Some(dz_ast::IfBinding { name, mutable })
        } else {
            None
        };
        let cond = self.parse_expr_no_struct();
        let then = self.parse_block();
        let else_branch = if self.eat(TokenKind::Else) {
            if self.at(TokenKind::If) {
                Some(Box::new(self.parse_if()))
            } else {
                let b_start = self.start();
                let block = self.parse_block();
                Some(Box::new(Expr {
                    span: self.span_from(b_start),
                    kind: ExprKind::Block(block),
                }))
            }
        } else {
            None
        };
        let span = self.span_from(start);
        Expr {
            span,
            kind: ExprKind::If(IfExpr { span, binding, cond: Box::new(cond), then, else_branch }),
        }
    }

    fn parse_switch(&mut self) -> Expr {
        let start = self.start();
        self.bump(); // `switch`
        let subject = self.parse_expr_no_struct();
        let mut arms = Vec::new();
        let mut default = None;

        if self.expect_in(TokenKind::LBrace, "to begin the switch body") {
            loop {
                self.skip_terminators();
                if self.at(TokenKind::RBrace) || self.at_end() {
                    break;
                }
                if self.stalled() {
                    continue;
                }
                if self.at(TokenKind::Ident) && self.token_text(self.peek_token()) == "default" {
                    self.bump();
                    default = Some(self.parse_block());
                    continue;
                }
                if !self.at(TokenKind::Case) {
                    let span = self.peek_token().span;
                    let found = self.describe_current();
                    self.error_help(
                        codes::EXPECTED_TOKEN,
                        span,
                        format!("expected `case`, found {found}"),
                        "expected a `case` arm",
                        "arms are `case Variant { ... }`, `case A, B { ... }`, or `default { ... }`",
                    );
                    self.recover_to_statement_end();
                    continue;
                }
                let a_start = self.start();
                self.bump(); // `case`
                let mut patterns = Vec::new();
                loop {
                    let p_start = self.start();
                    let name = self.parse_ident_or_error();
                    let binding = if self.eat(TokenKind::LParen) {
                        let b = self.parse_ident();
                        self.expect_in(TokenKind::RParen, "to close a case binding");
                        b
                    } else {
                        None
                    };
                    patterns.push(VariantPat { span: self.span_from(p_start), name, binding });
                    if !self.eat(TokenKind::Comma) {
                        break;
                    }
                }
                let body = self.parse_block();
                arms.push(SwitchArm { span: self.span_from(a_start), patterns, body });
            }
            self.expect_in(TokenKind::RBrace, "to close the switch body");
        }

        let span = self.span_from(start);
        Expr {
            span,
            kind: ExprKind::Switch(SwitchExpr {
                span,
                subject: Box::new(subject),
                arms,
                default,
            }),
        }
    }

    /// Look for `{ name ->` and consume `{ name ->` if present, leaving the block body.
    ///
    /// Returns `None` for `{ ... }`, where the binding is the implicit `it`.
    fn parse_closure_binding(&mut self) -> Option<Ident> {
        if self.at(TokenKind::LBrace)
            && self.peek_at(1) == TokenKind::Ident
            && self.peek_at(2) == TokenKind::Arrow
        {
            self.bump(); // `{`
            let id = self.parse_ident();
            self.bump(); // `->`
            return id;
        }
        None
    }

    /// Parse a block; `consumed_brace` says the opening `{` was already eaten by
    /// [`Parser::parse_closure_binding`].
    fn parse_block_after_binding(&mut self, consumed_brace: bool) -> Block {
        if consumed_brace {
            self.parse_block_body(self.prev_end())
        } else {
            self.parse_block()
        }
    }

    /// Whether the current token can begin an expression — used to decide if `return`,
    /// `break` and `..` have an operand.
    pub(crate) fn starts_expr(&self) -> bool {
        use TokenKind::*;
        matches!(
            self.peek(),
            Int | Float
                | Str
                | RawStr
                | Char
                | True
                | False
                | Null
                | None
                | Ident
                | SelfValue
                | SelfType
                | LParen
                | LBracket
                | Minus
                | Bang
                | Tilde
                | Amp
                | Move
                | If
                | Switch
                | Hash
                | Dot
                | Return
                | Break
                | Continue
                | Unsafe
        ) || (self.peek() == LBrace && self.no_struct_literal == 0)
    }
}
