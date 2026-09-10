//! Declarations: imports, structs, unions, enums, traits, functions, constants.

use dz_ast::*;
use dz_diag::codes;
use dz_lex::TokenKind;
use dz_span::Span;

use crate::{starts_item, Parser};

impl<'a> Parser<'a> {
    pub(crate) fn parse_file(&mut self) -> SourceFileAst {
        let mut ast = SourceFileAst::default();
        loop {
            self.skip_terminators();
            if self.at_end() {
                break;
            }
            if self.stalled() {
                continue;
            }

            if self.at(TokenKind::Import) {
                ast.imports.push(self.parse_import());
                continue;
            }
            match self.parse_item() {
                Some(item) => ast.items.push(item),
                None => {
                    let span = self.peek_token().span;
                    let found = self.describe_current();
                    self.error_help(
                        codes::EXPECTED_ITEM,
                        span,
                        format!("expected a declaration, found {found}"),
                        "not the start of a declaration",
                        "declarations begin with `fn`, `struct`, `union`, `enum`, `trait`, \
                         `const`, `macro`, `import`, `pub` or `#[`",
                    );
                    self.bump();
                    self.recover_to_item();
                }
            }
        }
        ast
    }

    // --- imports ----------------------------------------------------------

    fn parse_import(&mut self) -> Import {
        let start = self.start();
        self.bump(); // `import`

        // `import cpp "raylib.h" as rl` (spec §15.1)
        if self.at(TokenKind::Ident) && self.token_text(self.peek_token()) == "cpp" {
            self.bump();
            let header = if self.at(TokenKind::Str) {
                self.bump().span
            } else {
                let span = self.peek_token().span;
                self.error_labeled(
                    codes::EXPECTED_TOKEN,
                    span,
                    "expected a header path string after `import cpp`",
                    "expected a string like \"raylib.h\"",
                );
                span
            };
            let alias = if self.eat(TokenKind::As) { self.parse_ident() } else { None };
            return Import { span: self.span_from(start), kind: ImportKind::Cpp { header, alias } };
        }

        let path = match self.parse_path_no_generics() {
            Some(p) => p,
            None => {
                self.recover_to_statement_end();
                return Import { span: self.span_from(start), kind: ImportKind::Error };
            }
        };

        // `import math::{ pow, sqrt }`
        if self.at(TokenKind::ColonColon) && self.peek_at(1) == TokenKind::LBrace {
            self.bump(); // `::`
            self.bump(); // `{`
            let mut names = Vec::new();
            loop {
                self.skip_terminators();
                if self.at(TokenKind::RBrace) || self.at_end() {
                    break;
                }
                if let Some(id) = self.parse_ident() {
                    names.push(id);
                } else {
                    self.recover_to_statement_end();
                    break;
                }
                self.skip_terminators();
                if !self.eat(TokenKind::Comma) {
                    break;
                }
            }
            self.skip_terminators();
            self.expect_in(TokenKind::RBrace, "to close the import list");
            return Import {
                span: self.span_from(start),
                kind: ImportKind::Selective { path, names },
            };
        }

        let alias = if self.eat(TokenKind::As) { self.parse_ident() } else { None };
        Import { span: self.span_from(start), kind: ImportKind::Module { path, alias } }
    }

    // --- items ------------------------------------------------------------

    /// Returns `None` when the current token cannot begin a declaration.
    pub(crate) fn parse_item(&mut self) -> Option<Item> {
        let start = self.start();
        let docs = self.eat_docs();
        let attrs = self.parse_attrs();
        self.skip_terminators();

        // `#machine Foo(Bar) { ... }` — a declaration macro invocation (spec §12.5).
        if self.at(TokenKind::Hash) && self.peek_at(1) == TokenKind::Ident {
            self.bump();
            let name = self.parse_ident_or_error();
            while !self.at(TokenKind::LBrace) && !self.at(TokenKind::Terminator) && !self.at_end() {
                self.bump();
            }
            let body = self.consume_braced_group();
            return Some(Item {
                span: self.span_from(start),
                docs,
                attrs,
                vis: Vis::Private,
                kind: ItemKind::MacroInvocation { name, body },
            });
        }

        let vis = self.parse_vis();

        let kind = match self.peek() {
            TokenKind::Fn => ItemKind::Fn(self.parse_fn(false)),
            TokenKind::Mut if self.peek_at(1) == TokenKind::Fn => ItemKind::Fn(self.parse_fn(true)),
            TokenKind::Struct => ItemKind::Struct(self.parse_struct()),
            TokenKind::Union => ItemKind::Union(self.parse_union()),
            TokenKind::Enum => ItemKind::Enum(self.parse_enum()),
            TokenKind::Trait => ItemKind::Trait(self.parse_trait()),
            TokenKind::Const => ItemKind::Const(self.parse_const()),
            TokenKind::Macro => ItemKind::Macro(self.parse_macro_decl()),
            _ => {
                // A visibility or attribute prefix commits us to a declaration, so report
                // here rather than making the caller guess.
                if vis != Vis::Private || !attrs.is_empty() || !docs.is_empty() {
                    let span = self.peek_token().span;
                    let found = self.describe_current();
                    self.error_labeled(
                        codes::EXPECTED_ITEM,
                        span,
                        format!("expected a declaration after this, found {found}"),
                        "expected `fn`, `struct`, `union`, `enum`, `trait`, `const` or `macro`",
                    );
                    self.recover_to_item();
                    ItemKind::Error
                } else {
                    return None;
                }
            }
        };

        Some(Item { span: self.span_from(start), docs, attrs, vis, kind })
    }

    fn parse_vis(&mut self) -> Vis {
        if !self.eat(TokenKind::Pub) {
            return Vis::Private;
        }
        if self.at(TokenKind::LParen) {
            self.bump();
            // `pub(module)` — `module` is a keyword, so match the token not the text.
            if self.at(TokenKind::Module) {
                self.bump();
                self.expect_in(TokenKind::RParen, "to close `pub(module)`");
                return Vis::Module;
            }
            let span = self.peek_token().span;
            self.error_help(
                codes::EXPECTED_TOKEN,
                span,
                "expected `module` in `pub(...)`",
                "only `pub(module)` is valid here",
                "visibility is `private` (the default), `pub(module)` or `pub` (spec §4)",
            );
            while !self.at(TokenKind::RParen) && !self.at_end() && !self.at(TokenKind::Terminator) {
                self.bump();
            }
            self.eat(TokenKind::RParen);
            return Vis::Module;
        }
        Vis::Public
    }

    // --- attributes -------------------------------------------------------

    fn parse_attrs(&mut self) -> Vec<Attr> {
        let mut attrs = Vec::new();
        loop {
            self.skip_terminators();
            if !(self.at(TokenKind::Hash) && self.peek_at(1) == TokenKind::LBracket) {
                break;
            }
            let start = self.start();
            self.bump(); // `#`
            self.bump(); // `[`
            let name = self.parse_ident_or_error();
            let args = if self.at(TokenKind::LParen) { self.parse_attr_args() } else { Vec::new() };
            self.expect_in(TokenKind::RBracket, "to close the attribute");
            attrs.push(Attr { span: self.span_from(start), name, args });
        }
        attrs
    }

    fn parse_attr_args(&mut self) -> Vec<AttrArg> {
        let mut args = Vec::new();
        self.bump(); // `(`
        loop {
            self.skip_terminators();
            if self.at(TokenKind::RParen) || self.at_end() {
                break;
            }
            if self.stalled() {
                continue;
            }

            if self.at(TokenKind::Ident) {
                // `cfg(os = "windows")` or a nested `derive(Serialize)`.
                match self.peek_at(1) {
                    TokenKind::Eq => {
                        let key = self.parse_ident_or_error();
                        self.bump();
                        let value = self.parse_expr();
                        args.push(AttrArg::KeyValue { key, value });
                    }
                    TokenKind::LParen => {
                        let start = self.start();
                        let name = self.parse_ident_or_error();
                        let inner = self.parse_attr_args();
                        args.push(AttrArg::Nested(Attr {
                            span: self.span_from(start),
                            name,
                            args: inner,
                        }));
                    }
                    _ => args.push(AttrArg::Ident(self.parse_ident_or_error())),
                }
            } else {
                args.push(AttrArg::Literal(self.parse_expr()));
            }

            self.skip_terminators();
            if !self.eat(TokenKind::Comma) {
                break;
            }
        }
        self.expect_in(TokenKind::RParen, "to close the attribute arguments");
        args
    }

    // --- generics ---------------------------------------------------------

    pub(crate) fn parse_generics(&mut self) -> Generics {
        let start = self.start();
        if !self.at(TokenKind::Lt) {
            return Generics { span: self.span_from(start), params: Vec::new() };
        }
        self.bump();
        let mut params = Vec::new();
        loop {
            self.skip_terminators();
            if self.at(TokenKind::Gt) || self.at_end() {
                break;
            }
            if self.stalled() {
                continue;
            }
            let p_start = self.start();
            let name = self.parse_ident_or_error();
            let mut bounds = Vec::new();
            if self.eat(TokenKind::Colon) {
                while let Some(p) = self.parse_path_no_generics() {
                    bounds.push(p);
                    if !self.eat(TokenKind::Plus) {
                        break;
                    }
                }
            }
            params.push(GenericParam { span: self.span_from(p_start), name, bounds });
            if !self.eat(TokenKind::Comma) {
                break;
            }
        }
        self.expect_gt("to close the generic parameter list");
        Generics { span: self.span_from(start), params }
    }

    // --- functions --------------------------------------------------------

    pub(crate) fn parse_fn(&mut self, is_mut: bool) -> FnDecl {
        let start = self.start();
        if is_mut {
            self.bump(); // `mut`
        }
        self.bump(); // `fn`
        let name = self.parse_ident_or_error();
        let generics = self.parse_generics();
        let (self_param, params) = self.parse_params();
        // `!->` in place of `->` marks the signature fallible (spec §9). A fallible
        // function with no return type is legal — it either succeeds with nothing or
        // fails — so the marker is recorded even when no type follows.
        let fallible = self.at(TokenKind::BangArrow);
        let ret = if self.eat(TokenKind::Arrow) || self.eat(TokenKind::BangArrow) {
            Some(self.parse_type())
        } else {
            None
        };

        let body = if self.at(TokenKind::LBrace) { Some(self.parse_block()) } else { None };
        FnDecl {
            span: self.span_from(start),
            is_mut,
            name,
            generics,
            self_param,
            params,
            ret,
            fallible,
            body,
        }
    }

    fn parse_params(&mut self) -> (Option<SelfParam>, Vec<Param>) {
        let mut self_param = None;
        let mut params = Vec::new();
        if !self.expect_in(TokenKind::LParen, "to begin the parameter list") {
            return (self_param, params);
        }
        loop {
            self.skip_terminators();
            if self.at(TokenKind::RParen) || self.at_end() {
                break;
            }
            if self.stalled() {
                continue;
            }

            let p_start = self.start();

            // `&self` / `&mut self`
            if self.at(TokenKind::Amp)
                && (self.peek_at(1) == TokenKind::SelfValue
                    || (self.peek_at(1) == TokenKind::Mut
                        && self.peek_at(2) == TokenKind::SelfValue))
            {
                self.bump();
                let is_mut = self.eat(TokenKind::Mut);
                self.bump(); // `self`
                if self_param.is_none() && params.is_empty() {
                    self_param = Some(SelfParam { span: self.span_from(p_start), is_mut });
                } else {
                    let span = self.span_from(p_start);
                    self.error_labeled(
                        codes::EXPECTED_TOKEN,
                        span,
                        "`self` must be the first parameter",
                        "move this to the front of the parameter list",
                    );
                }
            } else if self.at(TokenKind::SelfValue) {
                let t = self.bump();
                self.error_help(
                    codes::EXPECTED_TOKEN,
                    t.span,
                    "`self` must be taken by reference",
                    "expected `&self` or `&mut self`",
                    "methods are `fn` (taking `&self`) or `mut fn` (taking `&mut self`) — spec §7.3",
                );
                if self_param.is_none() {
                    self_param = Some(SelfParam { span: t.span, is_mut: false });
                }
            } else {
                let Some(name) = self.parse_ident() else {
                    // Not a parameter at all: stop rather than reporting a missing `:`,
                    // a missing type and a missing `)` for the same character.
                    break;
                };
                self.expect_in(TokenKind::Colon, "after a parameter name");
                let ty = self.parse_type();
                let default = if self.eat(TokenKind::Eq) { Some(self.parse_expr()) } else { None };
                params.push(Param { span: self.span_from(p_start), name, ty, default });
            }

            self.skip_terminators();
            if !self.eat(TokenKind::Comma) {
                break;
            }
        }
        self.skip_terminators();
        self.expect_in(TokenKind::RParen, "to close the parameter list");
        (self_param, params)
    }

    // --- structs ----------------------------------------------------------

    fn parse_struct(&mut self) -> StructDecl {
        let start = self.start();
        self.bump(); // `struct`
        let name = self.parse_ident_or_error();
        let generics = self.parse_generics();
        let traits = self.parse_trait_list();
        let members = self.parse_members(&name.name);
        StructDecl { span: self.span_from(start), name, generics, traits, members }
    }

    /// `: Allocator + Drawable`
    fn parse_trait_list(&mut self) -> Vec<Path> {
        let mut traits = Vec::new();
        if self.eat(TokenKind::Colon) {
            while let Some(p) = self.parse_path() {
                traits.push(p);
                if !self.eat(TokenKind::Plus) {
                    break;
                }
            }
        }
        traits
    }

    fn parse_members(&mut self, struct_name: &str) -> Vec<Member> {
        let mut members = Vec::new();
        if !self.expect_in(TokenKind::LBrace, "to begin the body") {
            return members;
        }
        loop {
            self.skip_terminators();
            if self.at(TokenKind::RBrace) || self.at_end() {
                break;
            }
            if self.stalled() {
                continue;
            }
            members.push(self.parse_member(struct_name));
        }
        self.expect_in(TokenKind::RBrace, "to close the body");
        members
    }

    fn parse_member(&mut self, struct_name: &str) -> Member {
        let start = self.start();
        let docs = self.eat_docs();
        let attrs = self.parse_attrs();
        self.skip_terminators();
        let vis = self.parse_vis();

        let kind = match self.peek() {
            TokenKind::Fn => MemberKind::Fn(self.parse_fn(false)),
            TokenKind::Mut if self.peek_at(1) == TokenKind::Fn => {
                MemberKind::Fn(self.parse_fn(true))
            }
            TokenKind::Drop => MemberKind::Drop(self.parse_special_member()),
            TokenKind::Copy => MemberKind::Copy(self.parse_special_member()),
            // `type Item = &T` (spec §8.5)
            TokenKind::Ident if self.token_text(self.peek_token()) == "type" => {
                self.bump();
                let name = self.parse_ident_or_error();
                let ty = if self.eat(TokenKind::Eq) { Some(self.parse_type()) } else { None };
                MemberKind::TypeAlias { name, ty }
            }
            // `own m_data: ^T`
            TokenKind::Own => MemberKind::Field(self.parse_field()),
            TokenKind::Ident => match self.peek_at(1) {
                // `Buffer(cap: u32) { ... }` — a constructor.
                TokenKind::LParen => {
                    let ctor = self.parse_ctor();
                    if ctor.name.name != struct_name {
                        let span = ctor.name.span;
                        let got = ctor.name.name.clone();
                        self.error_help(
                            codes::EXPECTED_ITEM,
                            span,
                            format!("`{got}` is not a valid member of `{struct_name}`"),
                            "a constructor's name must match its struct",
                            format!("rename it to `{struct_name}`, or write `fn {got}(...)` to \
                                     make it a method"),
                        );
                    }
                    MemberKind::Ctor(ctor)
                }
                TokenKind::Colon => MemberKind::Field(self.parse_field()),
                _ => {
                    let span = self.peek_token().span;
                    let found = self.describe_current();
                    self.error_help(
                        codes::EXPECTED_ITEM,
                        span,
                        format!("expected a member, found {found}"),
                        "not a field, method, constructor, `copy` or `drop`",
                        "a field is `name: Type`; a method is `fn name(...)` or `mut fn name(...)`",
                    );
                    self.recover_to_statement_end();
                    MemberKind::Error
                }
            },
            _ => {
                let span = self.peek_token().span;
                let found = self.describe_current();
                self.error_labeled(
                    codes::EXPECTED_ITEM,
                    span,
                    format!("expected a member, found {found}"),
                    "expected a field, method, constructor, `copy` or `drop`",
                );
                self.recover_to_statement_end();
                MemberKind::Error
            }
        };

        Member { span: self.span_from(start), docs, attrs, vis, kind }
    }

    /// `copy(other: &Self) { ... }` and `drop() { ... }` share this shape.
    fn parse_special_member(&mut self) -> FnDecl {
        let start = self.start();
        let t = self.bump();
        let name = Ident::new(t.span, t.kind.name());
        let (self_param, params) = self.parse_params();
        let body = if self.at(TokenKind::LBrace) { Some(self.parse_block()) } else { None };
        FnDecl {
            span: self.span_from(start),
            is_mut: false,
            name,
            generics: Generics { span: t.span, params: Vec::new() },
            self_param,
            params,
            ret: None,
            fallible: false,
            body,
        }
    }

    fn parse_ctor(&mut self) -> FnDecl {
        let start = self.start();
        let name = self.parse_ident_or_error();
        let (self_param, params) = self.parse_params();
        let body = if self.at(TokenKind::LBrace) { Some(self.parse_block()) } else { None };
        FnDecl {
            span: self.span_from(start),
            is_mut: false,
            name,
            generics: Generics { span: Span::DUMMY, params: Vec::new() },
            self_param,
            params,
            ret: None,
            fallible: false,
            body,
        }
    }

    fn parse_field(&mut self) -> Field {
        let start = self.start();
        let is_own = self.eat(TokenKind::Own);
        let name = self.parse_ident_or_error();
        self.expect_in(TokenKind::Colon, "after a field name");
        let ty = self.parse_type();
        let default = if self.eat(TokenKind::Eq) { Some(self.parse_expr()) } else { None };
        Field { span: self.span_from(start), name, is_own, ty, default }
    }

    // --- unions -----------------------------------------------------------

    fn parse_union(&mut self) -> UnionDecl {
        let start = self.start();
        self.bump(); // `union`
        let name = self.parse_ident_or_error();
        let generics = self.parse_generics();
        let mut variants = Vec::new();

        if self.expect_in(TokenKind::LBrace, "to begin the union body") {
            loop {
                self.skip_terminators();
                if self.at(TokenKind::RBrace) || self.at_end() {
                    break;
                }
                if self.stalled() {
                    continue;
                }
                let v_start = self.start();
                let docs = self.eat_docs();
                self.skip_terminators();
                let v_name = self.parse_ident_or_error();
                let mut fields = Vec::new();
                let mut has_payload = false;
                if self.at(TokenKind::LBrace) {
                    has_payload = true;
                    self.bump();
                    loop {
                        self.skip_terminators();
                        if self.at(TokenKind::RBrace) || self.at_end() {
                            break;
                        }
                        if self.stalled() {
                            continue;
                        }
                        fields.push(self.parse_field());
                        self.skip_terminators();
                        if !self.eat(TokenKind::Comma) {
                            self.skip_terminators();
                            if !self.at(TokenKind::RBrace) {
                                continue;
                            }
                            break;
                        }
                    }
                    self.expect_in(TokenKind::RBrace, "to close the variant payload");
                }
                variants.push(Variant {
                    span: self.span_from(v_start),
                    docs,
                    name: v_name,
                    fields,
                    has_payload,
                });
                // Variants may be separated by commas or newlines.
                self.eat(TokenKind::Comma);
            }
            self.expect_in(TokenKind::RBrace, "to close the union body");
        }

        UnionDecl { span: self.span_from(start), name, generics, variants }
    }

    // --- enums ------------------------------------------------------------

    fn parse_enum(&mut self) -> EnumDecl {
        let start = self.start();
        self.bump(); // `enum`
        let name = self.parse_ident_or_error();
        let backing = if self.eat(TokenKind::Colon) { Some(self.parse_type()) } else { None };
        let mut variants = Vec::new();

        if self.expect_in(TokenKind::LBrace, "to begin the enum body") {
            loop {
                self.skip_terminators();
                if self.at(TokenKind::RBrace) || self.at_end() {
                    break;
                }
                if self.stalled() {
                    continue;
                }
                let v_start = self.start();
                let docs = self.eat_docs();
                self.skip_terminators();
                let v_name = self.parse_ident_or_error();
                let value = if self.eat(TokenKind::Eq) { Some(self.parse_expr()) } else { None };
                variants.push(EnumVariant {
                    span: self.span_from(v_start),
                    docs,
                    name: v_name,
                    value,
                });
                self.eat(TokenKind::Comma);
            }
            self.expect_in(TokenKind::RBrace, "to close the enum body");
        }

        EnumDecl { span: self.span_from(start), name, backing, variants }
    }

    // --- traits -----------------------------------------------------------

    fn parse_trait(&mut self) -> TraitDecl {
        let start = self.start();
        self.bump(); // `trait`
        let name = self.parse_ident_or_error();
        let generics = self.parse_generics();
        let supertraits = self.parse_trait_list();
        let items = self.parse_members(&name.name);
        TraitDecl { span: self.span_from(start), name, generics, supertraits, items }
    }

    // --- constants --------------------------------------------------------

    fn parse_const(&mut self) -> ConstDecl {
        let start = self.start();
        self.bump(); // `const`
        let name = self.parse_ident_or_error();
        let ty = if self.eat(TokenKind::Colon) { Some(self.parse_type()) } else { None };
        self.expect_in(TokenKind::Eq, "in a `const` declaration");
        let value = self.parse_expr();
        ConstDecl { span: self.span_from(start), name, ty, value }
    }

    // --- macros (shallow until M10) ---------------------------------------

    fn parse_macro_decl(&mut self) -> MacroDecl {
        let start = self.start();
        self.bump(); // `macro`
        let name = self.parse_ident_or_error();
        self.expect_in(TokenKind::Arrow, "before the macro's expansion kind");
        let kind = self.parse_ident_or_error();
        let body = self.consume_braced_group();
        MacroDecl { span: self.span_from(start), name, kind, body }
    }
}

/// Exposed for the statement parser, which allows nested declarations.
pub(crate) fn item_starts_here(k: TokenKind) -> bool {
    starts_item(k)
}
