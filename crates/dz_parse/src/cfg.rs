//! `#if FLAG { … } #else #if OTHER { … } #else { … }` — compile-time conditionals (spec
//! §12.10).
//!
//! One parser for all three places a conditional may stand: among declarations, among a
//! type's members, and among a block's statements. Every branch is parsed whatever the flags
//! say, so the formatter and the editor see the whole file; the driver keeps the active one.

use dz_ast::*;
use dz_diag::codes;
use dz_lex::TokenKind;

use crate::Parser;

impl<'a> Parser<'a> {
    /// At `#if`?
    pub(crate) fn at_cfg_if(&self) -> bool {
        self.at(TokenKind::Hash) && self.peek_at(1) == TokenKind::If
    }

    /// `#if cond { … }` and whatever `#else` follows it, each branch's contents read by
    /// `each`. `each` returns `None` for something that could not start one, which is
    /// reported once and skipped.
    pub(crate) fn parse_cfg_if<T>(
        &mut self,
        what: &str,
        each: &mut dyn FnMut(&mut Self) -> Option<T>,
    ) -> CfgIf<T> {
        let start = self.start();
        self.bump(); // `#`
        self.bump(); // `if`
        let cond = self.parse_cfg_or();
        let then_start = self.start();
        let then = self.parse_cfg_branch(what, each);
        let then_span = self.span_from(then_start);

        // `#else` may follow on the same line as the `}` or on the next one.
        let save = self.pos;
        self.skip_terminators();
        let otherwise = if self.at(TokenKind::Hash) && self.peek_at(1) == TokenKind::Else {
            self.bump(); // `#`
            self.bump(); // `else`
            if self.at_cfg_if() {
                Some(CfgElse::If(Box::new(self.parse_cfg_if(what, each))))
            } else {
                let else_start = self.start();
                let items = self.parse_cfg_branch(what, each);
                Some(CfgElse::Else { span: self.span_from(else_start), items })
            }
        } else {
            self.pos = save;
            None
        };
        CfgIf { span: self.span_from(start), cond, then, then_span, otherwise }
    }

    fn parse_cfg_branch<T>(
        &mut self,
        what: &str,
        each: &mut dyn FnMut(&mut Self) -> Option<T>,
    ) -> Vec<T> {
        let mut out = Vec::new();
        if !self.expect_in(TokenKind::LBrace, "to begin the `#if` branch") {
            return out;
        }
        loop {
            self.skip_terminators();
            if self.at(TokenKind::RBrace) || self.at_end() {
                break;
            }
            if self.stalled() {
                continue;
            }
            match each(self) {
                Some(x) => out.push(x),
                None => {
                    let span = self.peek_token().span;
                    let found = self.describe_current();
                    self.error_labeled(
                        codes::EXPECTED_ITEM,
                        span,
                        format!("expected {what} inside `#if`, found {found}"),
                        format!("expected {what}"),
                    );
                    self.bump();
                    self.recover_to_item();
                }
            }
        }
        self.expect_in(TokenKind::RBrace, "to close the `#if` branch");
        out
    }

    // --- conditions -----------------------------------------------------------
    //
    // `||` binds loosest, then `&&`, then `!`; parentheses group. Nothing else is a
    // condition: a flag is on or off, so there is nothing to compare.

    fn parse_cfg_or(&mut self) -> CfgCond {
        let start = self.start();
        let mut lhs = self.parse_cfg_and();
        while self.eat(TokenKind::PipePipe) {
            let rhs = self.parse_cfg_and();
            lhs = CfgCond {
                span: self.span_from(start),
                kind: CfgCondKind::Or(Box::new(lhs), Box::new(rhs)),
            };
        }
        lhs
    }

    fn parse_cfg_and(&mut self) -> CfgCond {
        let start = self.start();
        let mut lhs = self.parse_cfg_unary();
        while self.eat(TokenKind::AmpAmp) {
            let rhs = self.parse_cfg_unary();
            lhs = CfgCond {
                span: self.span_from(start),
                kind: CfgCondKind::And(Box::new(lhs), Box::new(rhs)),
            };
        }
        lhs
    }

    fn parse_cfg_unary(&mut self) -> CfgCond {
        let start = self.start();
        if self.eat(TokenKind::Bang) {
            let inner = self.parse_cfg_unary();
            return CfgCond {
                span: self.span_from(start),
                kind: CfgCondKind::Not(Box::new(inner)),
            };
        }
        if self.eat(TokenKind::LParen) {
            let inner = self.parse_cfg_or();
            self.expect_in(TokenKind::RParen, "to close the condition");
            return CfgCond {
                span: self.span_from(start),
                kind: CfgCondKind::Paren(Box::new(inner)),
            };
        }
        if self.at(TokenKind::Ident) {
            if let Some(p) = self.parse_path_no_generics() {
                return CfgCond { span: self.span_from(start), kind: CfgCondKind::Flag(p) };
            }
        }
        let span = self.peek_token().span;
        let found = self.describe_current();
        self.error_help(
            codes::EXPECTED_EXPR,
            span,
            format!("expected a build flag, found {found}"),
            "not a flag",
            "a condition is flags joined by `!`, `&&` and `||`: `#if DEBUG && !OS_WINDOWS` \
             (spec §12.10)",
        );
        CfgCond { span, kind: CfgCondKind::Error }
    }
}
