//! Types and paths (spec §5, §3.3).

use dz_ast::*;
use dz_diag::codes;
use dz_lex::TokenKind;

use crate::Parser;

#[derive(Clone, Copy, PartialEq, Eq)]
enum GenericsMode {
    /// Imports and trait bounds: `<` is never generic arguments.
    Never,
    /// Type position: `<` is always generic arguments.
    Greedy,
    /// Expression position: `<` may be less-than, so try and roll back.
    Speculative,
}

impl<'a> Parser<'a> {
    pub(crate) fn parse_type(&mut self) -> Type {
        let start = self.start();
        let kind = match self.peek() {
            // `^T` / `^mut T` (spec §5.2)
            TokenKind::Caret => {
                self.bump();
                let mutable = self.eat(TokenKind::Mut);
                TypeKind::Ptr { mutable, inner: Box::new(self.parse_type()) }
            }
            // `^^T` — the lexer reads `^^` as xor, so unpick it here, as `&&` is below.
            TokenKind::CaretCaret => {
                let t = self.bump();
                let inner_start = t.span.start + 1;
                let mutable = self.eat(TokenKind::Mut);
                let inner = self.parse_type();
                let inner_span = dz_span::Span { start: inner_start, end: inner.span.end };
                TypeKind::Ptr {
                    mutable: false,
                    inner: Box::new(Type {
                        span: inner_span,
                        kind: TypeKind::Ptr { mutable, inner: Box::new(inner) },
                    }),
                }
            }
            // `own ^T` / `own ^mut T`
            TokenKind::Own => {
                self.bump();
                if !self.expect_in(TokenKind::Caret, "after `own`") {
                    TypeKind::Error
                } else {
                    let mutable = self.eat(TokenKind::Mut);
                    TypeKind::OwnPtr { mutable, inner: Box::new(self.parse_type()) }
                }
            }
            // `&T` / `&mut T`
            TokenKind::Amp => {
                self.bump();
                let mutable = self.eat(TokenKind::Mut);
                TypeKind::Ref { mutable, inner: Box::new(self.parse_type()) }
            }
            // `&&T` — the lexer merges the ampersands, so unpick it here.
            TokenKind::AmpAmp => {
                let t = self.bump();
                let inner_start = t.span.start + 1;
                let mutable = self.eat(TokenKind::Mut);
                let inner = self.parse_type();
                let inner_span = dz_span::Span { start: inner_start, end: inner.span.end };
                TypeKind::Ref {
                    mutable: false,
                    inner: Box::new(Type {
                        span: inner_span,
                        kind: TypeKind::Ref { mutable, inner: Box::new(inner) },
                    }),
                }
            }
            // `[N]T`, `[]T` and `[]mut T`
            TokenKind::LBracket => {
                self.bump();
                let size = if self.at(TokenKind::RBracket) {
                    None
                } else {
                    Some(Box::new(self.parse_expr()))
                };
                self.expect_in(TokenKind::RBracket, "in an array or slice type");
                // `mut` belongs to a slice only: a fixed array is a value, so its
                // mutability comes from the binding that holds it (spec §5.3, §18.22).
                let mut_span = self.peek_token().span;
                let mutable = self.eat(TokenKind::Mut);
                match size {
                    Some(size) => {
                        if mutable {
                            self.error_help(
                                codes::EXPECTED_TYPE,
                                mut_span,
                                "`mut` is not allowed on a fixed-size array",
                                "only a slice carries its writability in its type",
                                "a `[N]T` is a value: write `let mut a: [N]T` to make it \
                                 writable, or `[]mut T` for a writable view (spec §5.3)",
                            );
                        }
                        TypeKind::Array { size, elem: Box::new(self.parse_type()) }
                    }
                    None => TypeKind::Slice { mutable, elem: Box::new(self.parse_type()) },
                }
            }
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
                    elems.push(self.parse_type());
                    if !self.eat(TokenKind::Comma) {
                        break;
                    }
                }
                self.expect_in(TokenKind::RParen, "to close a tuple type");
                TypeKind::Tuple(elems)
            }
            // `fn(i32, f32) -> bool`
            TokenKind::Fn => {
                self.bump();
                let mut params = Vec::new();
                if self.expect_in(TokenKind::LParen, "in a function type") {
                    loop {
                        self.skip_terminators();
                        if self.at(TokenKind::RParen) || self.at_end() {
                            break;
                        }
                        if self.stalled() {
                            continue;
                        }
                        params.push(self.parse_type());
                        if !self.eat(TokenKind::Comma) {
                            break;
                        }
                    }
                    self.expect_in(TokenKind::RParen, "to close a function type");
                }
                let ret = if self.eat(TokenKind::Arrow) || self.eat(TokenKind::BangArrow) {
                    Some(Box::new(self.parse_type()))
                } else {
                    None
                };
                TypeKind::Fn { params, ret }
            }
            TokenKind::SelfType => {
                self.bump();
                TypeKind::SelfType
            }
            TokenKind::Ident => match self.parse_type_path() {
                Some(p) => TypeKind::Path(p),
                None => TypeKind::Error,
            },
            _ => {
                let span = self.peek_token().span;
                let found = self.describe_current();
                self.error_help(
                    codes::EXPECTED_TYPE,
                    span,
                    format!("expected a type, found {found}"),
                    "expected a type here",
                    "types are names, `^T`, `&T`, `&mut T`, `own ^T`, `T?`, `[N]T`, `[]T`, \
                     `[]mut T`, \
                     tuples, or `fn(...) -> T`",
                );
                TypeKind::Error
            }
        };
        // `?` is postfix and binds to the type name, so it wraps whatever was just parsed
        // (spec §5.6). `^T?` is therefore a pointer to an optional, and access sigils —
        // parsed above, as prefixes — always sit outside it.
        let mut ty = Type { span: self.span_from(start), kind };
        while self.at(TokenKind::Question) {
            self.bump();
            ty = Type { span: self.span_from(start), kind: TypeKind::Optional(Box::new(ty)) };
        }
        ty
    }

    // --- paths ------------------------------------------------------------

    /// How `<` after a path segment should be treated.
    ///
    /// In a type there is no ambiguity, so generic arguments are taken greedily. In an
    /// expression `<` may be less-than, so the parse is speculative (see
    /// [`Parser::try_parse_generic_args`]).
    pub(crate) fn parse_path(&mut self) -> Option<Path> {
        self.parse_path_inner(GenericsMode::Speculative)
    }

    /// A path in type position: `Buffer<f32>`, `HashMap<K, V>::Entry`.
    pub(crate) fn parse_type_path(&mut self) -> Option<Path> {
        self.parse_path_inner(GenericsMode::Greedy)
    }

    /// A path used where generics are never valid (imports, trait bounds).
    pub(crate) fn parse_path_no_generics(&mut self) -> Option<Path> {
        self.parse_path_inner(GenericsMode::Never)
    }

    fn parse_path_inner(&mut self, mode: GenericsMode) -> Option<Path> {
        let start = self.start();
        let mut segments = Vec::new();
        loop {
            let seg_start = self.start();
            let name = self.parse_ident()?;
            let generics = if self.at(TokenKind::Lt) {
                match mode {
                    GenericsMode::Never => Vec::new(),
                    GenericsMode::Greedy => self.parse_generic_args(),
                    GenericsMode::Speculative => self.try_parse_generic_args().unwrap_or_default(),
                }
            } else {
                Vec::new()
            };
            segments.push(PathSegment { span: self.span_from(seg_start), name, generics });
            // `math::{ pow, sqrt }` — the `::` belongs to the import, not to the path.
            if self.at(TokenKind::ColonColon) && self.peek_at(1) == TokenKind::LBrace {
                break;
            }
            if !self.eat(TokenKind::ColonColon) {
                break;
            }
        }
        Some(Path { span: self.span_from(start), segments })
    }

    /// `<T, U>` where generic arguments are the only possible reading.
    fn parse_generic_args(&mut self) -> Vec<Type> {
        self.bump(); // `<`
        let mut args = Vec::new();
        loop {
            self.skip_terminators();
            if self.at(TokenKind::Gt) || self.at_end() {
                break;
            }
            if self.stalled() {
                continue;
            }
            args.push(self.parse_type());
            if !self.eat(TokenKind::Comma) {
                break;
            }
        }
        self.expect_gt("to close the generic arguments");
        args
    }

    /// Speculatively parse `<T, U>` in expression position.
    ///
    /// There is no turbofish (spec §3.3), so `<` is ambiguous with less-than. Comparison
    /// being non-associative removes most of the ambiguity; what remains is resolved by
    /// requiring the closing `>` to be followed by something that can only continue a
    /// path or a call. If that fails, the cursor is restored and `<` is a comparison.
    pub(crate) fn try_parse_generic_args(&mut self) -> Option<Vec<Type>> {
        let saved_pos = self.pos_checkpoint();
        let saved_diags = self.diags.len();

        self.bump(); // `<`
        let mut args = Vec::new();
        let ok = loop {
            if self.at(TokenKind::Gt) || self.at(TokenKind::Shr) {
                break true;
            }
            if self.at_end() || self.at(TokenKind::Terminator) {
                break false;
            }
            let before = self.pos_checkpoint();
            let ty = self.parse_type();
            if matches!(ty.kind, TypeKind::Error) || self.pos_checkpoint() == before {
                break false;
            }
            args.push(ty);
            if self.eat(TokenKind::Comma) {
                continue;
            }
            break self.at(TokenKind::Gt) || self.at(TokenKind::Shr);
        };

        if ok && self.eat_gt() {
            // Only these can follow generic arguments in expression position: a path or a
            // call going on, or — for a generic function taken as a value, `sort(xs,
            // less<i32>)` — the end of the expression. None of them can follow `a < b > c`'s
            // `>`, which would be a chained comparison anyway (spec §3.3).
            let follows = matches!(
                self.peek(),
                TokenKind::ColonColon
                    | TokenKind::LParen
                    | TokenKind::LBrace
                    | TokenKind::RParen
                    | TokenKind::RBracket
                    | TokenKind::RBrace
                    | TokenKind::Comma
                    | TokenKind::Terminator
                    | TokenKind::Eof
            );
            if follows {
                self.truncate_diags(saved_diags);
                return Some(args);
            }
        }

        self.restore_checkpoint(saved_pos);
        self.truncate_diags(saved_diags);
        None
    }
}
