//! The Dazzle lexer. Implements spec §2.
//!
//! Two properties are load-bearing and worth stating up front:
//!
//! * **The lexer never bails.** Every error produces a diagnostic *and* a token, so the
//!   parser always receives a complete stream. Spec §17.2 requires this because the LSP
//!   runs this code on half-typed source on every keystroke.
//! * **Line endings are handled during the scan, not by rewriting the buffer.** `\r\n` and
//!   lone `\r` are recognised where they occur, so byte offsets always match the file on
//!   disk.

use dz_diag::{codes, Diagnostic, Diagnostics};
use dz_span::{FileId, Span};

use crate::token::{keyword, Token, TokenKind};

/// Integer and float suffixes (spec §2.5), longest first so matching is greedy.
const SUFFIXES: &[&str] = &[
    "usize", "isize", "i16", "i32", "i64", "u16", "u32", "u64", "f32", "f64", "i8", "u8", "f",
];

const INT_SUFFIXES: &[&str] =
    &["usize", "isize", "i16", "i32", "i64", "u16", "u32", "u64", "i8", "u8"];

pub struct LexResult {
    pub tokens: Vec<Token>,
    pub diagnostics: Diagnostics,
    /// Non-doc comments, in source order. They are not tokens — the parser ignores them —
    /// but the formatter needs them to avoid deleting the user's comments.
    pub comments: Vec<Comment>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Comment {
    pub span: Span,
    pub kind: CommentKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommentKind {
    Line,
    Block,
}

/// Tokenize `src`. Never fails; malformed input yields diagnostics plus recovery tokens.
pub fn lex(file: FileId, src: &str) -> LexResult {
    let mut lexer = Lexer {
        file,
        src,
        pos: 0,
        tokens: Vec::new(),
        diags: Diagnostics::new(),
        comments: Vec::new(),
    };
    lexer.run();
    let tokens = apply_terminator_rules(lexer.tokens);
    LexResult { tokens, diagnostics: lexer.diags, comments: lexer.comments }
}

struct Lexer<'a> {
    file: FileId,
    src: &'a str,
    pos: usize,
    tokens: Vec<Token>,
    diags: Diagnostics,
    comments: Vec<Comment>,
}

impl<'a> Lexer<'a> {
    // --- cursor -----------------------------------------------------------

    #[inline]
    fn peek(&self) -> Option<char> {
        self.src[self.pos..].chars().next()
    }

    #[inline]
    fn peek_at(&self, n: usize) -> Option<char> {
        self.src[self.pos..].chars().nth(n)
    }

    #[inline]
    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += c.len_utf8();
        Some(c)
    }

    #[inline]
    fn eat(&mut self, expected: char) -> bool {
        if self.peek() == Some(expected) {
            self.pos += expected.len_utf8();
            true
        } else {
            false
        }
    }

    #[inline]
    fn push(&mut self, kind: TokenKind, start: usize) {
        self.tokens.push(Token::new(kind, Span::new(start, self.pos)));
    }

    /// Report an error whose primary label carries its own message.
    fn error_labeled(
        &mut self,
        code: &'static str,
        span: Span,
        message: impl Into<String>,
        label: impl Into<String>,
    ) {
        self.diags
            .push(Diagnostic::error(code, message.into()).at(self.file, span, label.into()));
    }

    fn error_with_help(
        &mut self,
        code: &'static str,
        span: Span,
        message: impl Into<String>,
        label: impl Into<String>,
        help: impl Into<String>,
    ) {
        self.diags.push(
            Diagnostic::error(code, message.into())
                .at(self.file, span, label.into())
                .with_help(help.into()),
        );
    }

    // --- main loop --------------------------------------------------------

    fn run(&mut self) {
        while let Some(c) = self.peek() {
            let start = self.pos;
            match c {
                '\n' | '\r' => {
                    self.bump_newline();
                    self.push(TokenKind::Terminator, start);
                }
                ' ' | '\t' => {
                    self.bump();
                }
                // A BOM is only legal as the very first character of the file (§2.1).
                '\u{FEFF}' if start == 0 => {
                    self.bump();
                }
                '\u{FEFF}' | '\u{200B}' | '\u{200C}' | '\u{200D}' | '\u{2060}' | '\u{00A0}' => {
                    self.bump();
                    let span = Span::new(start, self.pos);
                    let name = match c {
                        '\u{FEFF}' => "a byte-order mark",
                        '\u{00A0}' => "a non-breaking space",
                        _ => "a zero-width character",
                    };
                    self.error_with_help(
                        codes::INVISIBLE_CHARACTER,
                        span,
                        format!("invisible character in source: U+{:04X}", c as u32),
                        format!("{name} is here"),
                        "delete it — it is invisible in your editor but not to the compiler",
                    );
                }
                '/' => match self.peek_at(1) {
                    Some('/') => self.line_comment(start),
                    Some('*') => self.block_comment(start),
                    Some('=') => {
                        self.bump();
                        self.bump();
                        self.push(TokenKind::SlashEq, start);
                    }
                    _ => {
                        self.bump();
                        self.push(TokenKind::Slash, start);
                    }
                },
                '"' => self.string(start),
                '`' => self.raw_string(start),
                '\'' => self.char_literal(start),
                '0'..='9' => self.number(start),
                c if is_ident_start(c) => self.ident(start),
                _ => self.operator(start, c),
            }
        }
        let start = self.pos;
        self.push(TokenKind::Eof, start);
    }

    /// Consume one line terminator: `\r\n`, lone `\r`, or `\n`.
    fn bump_newline(&mut self) {
        if self.eat('\r') {
            self.eat('\n');
        } else {
            self.bump();
        }
    }

    // --- comments ---------------------------------------------------------

    fn line_comment(&mut self, start: usize) {
        self.bump();
        self.bump(); // "//"
        // `///` is a doc comment, but `////` is just a comment.
        let is_doc = self.peek() == Some('/') && self.peek_at(1) != Some('/');
        while let Some(c) = self.peek() {
            if c == '\n' || c == '\r' {
                break;
            }
            self.bump();
        }
        if is_doc {
            self.push(TokenKind::DocComment, start);
        } else {
            self.comments
                .push(Comment { span: Span::new(start, self.pos), kind: CommentKind::Line });
        }
    }

    fn block_comment(&mut self, start: usize) {
        self.bump();
        self.bump(); // "/*"
        let mut depth = 1usize;
        while depth > 0 {
            match self.peek() {
                None => {
                    let span = Span::new(start, self.pos);
                    self.error_labeled(
                        codes::UNTERMINATED_BLOCK_COMMENT,
                        span,
                        "unterminated block comment",
                        format!(
                            "this comment is never closed ({} level{} still open)",
                            depth,
                            if depth == 1 { "" } else { "s" }
                        ),
                    );
                    self.record_block_comment(start);
                    return;
                }
                Some('/') if self.peek_at(1) == Some('*') => {
                    self.bump();
                    self.bump();
                    depth += 1;
                }
                Some('*') if self.peek_at(1) == Some('/') => {
                    self.bump();
                    self.bump();
                    depth -= 1;
                }
                Some(_) => {
                    self.bump();
                }
            }
        }
        self.record_block_comment(start);
    }

    fn record_block_comment(&mut self, start: usize) {
        self.comments
            .push(Comment { span: Span::new(start, self.pos), kind: CommentKind::Block });
    }

    // --- identifiers ------------------------------------------------------

    fn ident(&mut self, start: usize) {
        while let Some(c) = self.peek() {
            if is_ident_continue(c) {
                self.bump();
            } else {
                break;
            }
        }
        let text = &self.src[start..self.pos];
        let kind = keyword(text).unwrap_or(TokenKind::Ident);
        self.push(kind, start);
    }

    // --- numbers ----------------------------------------------------------

    fn number(&mut self, start: usize) {
        // Radix prefixes: 0x, 0o, 0b.
        if self.peek() == Some('0') {
            if let Some(p) = self.peek_at(1) {
                let radix = match p {
                    'x' | 'X' => Some(16),
                    'o' | 'O' => Some(8),
                    'b' | 'B' => Some(2),
                    _ => None,
                };
                if let Some(radix) = radix {
                    self.bump();
                    self.bump();
                    return self.radix_number(start, radix, p);
                }
            }
        }

        let mut is_float = false;
        self.eat_digits();

        // A `.` only starts a fraction when a digit follows, so `0..n` is a range and
        // `4.hello` is a field access.
        if self.peek() == Some('.') && self.peek_at(1).is_some_and(|c| c.is_ascii_digit()) {
            is_float = true;
            self.bump();
            self.eat_digits();

            // `1.2.3.466` — consume the whole run so it becomes one error, not three tokens.
            if self.peek() == Some('.') && self.peek_at(1).is_some_and(|c| c.is_ascii_digit()) {
                while self.peek() == Some('.') && self.peek_at(1).is_some_and(|c| c.is_ascii_digit())
                {
                    self.bump();
                    self.eat_digits();
                }
                self.eat_ident_run();
                let span = Span::new(start, self.pos);
                self.error_with_help(
                    codes::INVALID_NUMBER,
                    span,
                    "invalid number literal",
                    "a number may contain at most one `.`",
                    "if you meant a field access, put the operand in parentheses",
                );
                self.push(TokenKind::Float, start);
                return;
            }
        }

        // Exponent, e.g. 1e9 or 2.5e-3.
        if matches!(self.peek(), Some('e' | 'E')) {
            let after = self.peek_at(1);
            let exp_ok = match after {
                Some(c) if c.is_ascii_digit() => true,
                Some('+' | '-') => self.peek_at(2).is_some_and(|c| c.is_ascii_digit()),
                _ => false,
            };
            if exp_ok {
                is_float = true;
                self.bump(); // e
                if matches!(self.peek(), Some('+' | '-')) {
                    self.bump();
                }
                self.eat_digits();
            }
        }

        self.check_trailing_underscore(start);
        self.finish_number(start, is_float, None);
    }

    fn radix_number(&mut self, start: usize, radix: u32, prefix: char) {
        let body_start = self.pos;
        self.eat_ident_run(); // digits and any suffix are both alphanumeric

        let run = &self.src[body_start..self.pos];
        // Hex digits include `f`, so only integer suffixes may be split off a hex literal.
        let allowed: &[&str] = if radix == 16 { INT_SUFFIXES } else { SUFFIXES };
        let (digits, suffix) = split_suffix(run, allowed);

        if digits.is_empty() || digits.chars().all(|c| c == '_') {
            let span = Span::new(start, self.pos);
            self.error_labeled(
                codes::INVALID_NUMBER,
                span,
                format!("`0{prefix}` literal has no digits"),
                format!("expected at least one {} digit here", radix_name(radix)),
            );
            self.push(TokenKind::Int, start);
            return;
        }

        // Report the first digit that does not belong to this radix.
        let mut offset = body_start;
        for c in digits.chars() {
            if c != '_' && c.to_digit(radix).is_none() {
                let span = Span::new(offset, offset + c.len_utf8());
                self.error_with_help(
                    codes::INVALID_DIGIT,
                    span,
                    format!("invalid digit `{c}` in {} literal", radix_name(radix)),
                    format!("`{c}` is not a valid {} digit", radix_name(radix)),
                    format!("valid digits are {}", radix_digits(radix)),
                );
                break;
            }
            offset += c.len_utf8();
        }

        self.check_trailing_underscore_in(body_start, digits);
        self.finish_number(start, false, suffix.map(|s| (s, false)));
    }

    /// Validate a suffix that follows the numeric body, then emit the token.
    ///
    /// `pre_split` carries a suffix already separated out by [`Lexer::radix_number`].
    fn finish_number(&mut self, start: usize, is_float: bool, pre_split: Option<(&str, bool)>) {
        let suffix_start = self.pos;
        let suffix: Option<String> = match pre_split {
            Some((s, _)) => Some(s.to_owned()),
            None => {
                if self.peek().is_some_and(is_ident_start) {
                    let s = self.pos;
                    self.eat_ident_run();
                    Some(self.src[s..self.pos].to_owned())
                } else {
                    None
                }
            }
        };

        let kind = if is_float { TokenKind::Float } else { TokenKind::Int };

        if let Some(suffix) = suffix {
            let span = if pre_split.is_some() {
                Span::new(start, self.pos)
            } else {
                Span::new(suffix_start, self.pos)
            };

            if !SUFFIXES.contains(&suffix.as_str()) {
                let looks_like_radix = matches!(suffix.chars().next(), Some('x' | 'o' | 'b'))
                    && suffix.len() > 1;
                let d = if looks_like_radix {
                    // e.g. `1x42` — almost always a mistyped `0x42`.
                    let whole = Span::new(start, self.pos);
                    Diagnostic::error(
                        codes::INVALID_NUMBER,
                        format!("invalid number literal `{}`", &self.src[whole.range()]),
                    )
                    .at(self.file, whole, "letters cannot follow the digits of a decimal literal")
                    .with_help(
                        "hexadecimal, octal and binary literals start with `0x`, `0o` and `0b`",
                    )
                } else {
                    Diagnostic::error(
                        codes::INVALID_SUFFIX,
                        format!("invalid suffix `{suffix}` on number literal"),
                    )
                    .at(self.file, span, "not a valid type suffix")
                    .with_help(format!("valid suffixes are {}", suffix_list()))
                };
                self.diags.push(d);
            } else if is_float && INT_SUFFIXES.contains(&suffix.as_str()) {
                self.error_with_help(
                    codes::INVALID_SUFFIX,
                    span,
                    format!("integer suffix `{suffix}` on a float literal"),
                    "this literal has a fractional part or exponent",
                    "use `f32` or `f64`, or remove the fractional part",
                );
            }

            // A float suffix makes an otherwise-integer literal a float.
            if matches!(suffix.as_str(), "f" | "f32" | "f64") {
                self.push(TokenKind::Float, start);
                return;
            }
        }

        self.push(kind, start);
    }

    fn eat_digits(&mut self) {
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() || c == '_' {
                self.bump();
            } else {
                break;
            }
        }
    }

    /// Consume a run of identifier characters (used to swallow malformed literal tails).
    fn eat_ident_run(&mut self) {
        while let Some(c) = self.peek() {
            if is_ident_continue(c) {
                self.bump();
            } else {
                break;
            }
        }
    }

    fn check_trailing_underscore(&mut self, start: usize) {
        let text = &self.src[start..self.pos];
        self.check_trailing_underscore_in(start, text);
    }

    fn check_trailing_underscore_in(&mut self, text_start: usize, text: &str) {
        if text.ends_with('_') {
            let underscores = text.len() - text.trim_end_matches('_').len();
            let span = Span::new(text_start + text.len() - underscores, text_start + text.len());
            self.error_labeled(
                codes::TRAILING_UNDERSCORE,
                span,
                "number literal ends with `_`",
                "a `_` separator must sit between digits",
            );
        }
    }

    // --- strings and chars ------------------------------------------------

    fn string(&mut self, start: usize) {
        self.bump(); // opening quote
        loop {
            match self.peek() {
                None => {
                    let span = Span::new(start, self.pos);
                    self.error_labeled(
                        codes::UNTERMINATED_STRING,
                        span,
                        "unterminated string literal",
                        "this string reaches the end of the file without a closing `\"`",
                    );
                    break;
                }
                // Recover at the line break rather than swallowing the rest of the file.
                Some('\n' | '\r') => {
                    let span = Span::new(start, self.pos);
                    self.error_with_help(
                        codes::UNTERMINATED_STRING,
                        span,
                        "unterminated string literal",
                        "this string is not closed before the end of the line",
                        "use `\\n` for a line break, or a `` ` ``-delimited raw string to span lines",
                    );
                    break;
                }
                Some('"') => {
                    self.bump();
                    break;
                }
                Some('\\') => {
                    self.bump();
                    self.escape();
                }
                Some(_) => {
                    self.bump();
                }
            }
        }
        self.push(TokenKind::Str, start);
    }

    fn raw_string(&mut self, start: usize) {
        self.bump(); // opening backtick
        loop {
            match self.peek() {
                None => {
                    let span = Span::new(start, self.pos);
                    self.error_labeled(
                        codes::UNTERMINATED_RAW_STRING,
                        span,
                        "unterminated raw string literal",
                        "this raw string is never closed with `` ` ``",
                    );
                    break;
                }
                Some('`') => {
                    self.bump();
                    break;
                }
                Some('\r' | '\n') => {
                    self.bump_newline();
                }
                Some(_) => {
                    self.bump();
                }
            }
        }
        self.push(TokenKind::RawStr, start);
    }

    fn char_literal(&mut self, start: usize) {
        self.bump(); // opening quote
        let mut count = 0usize;
        let mut terminated = false;
        loop {
            match self.peek() {
                None | Some('\n' | '\r') => break,
                Some('\'') => {
                    self.bump();
                    terminated = true;
                    break;
                }
                Some('\\') => {
                    self.bump();
                    self.escape();
                    count += 1;
                }
                Some(_) => {
                    self.bump();
                    count += 1;
                }
            }
        }

        let span = Span::new(start, self.pos);
        if !terminated {
            self.error_with_help(
                codes::INVALID_CHAR_LITERAL,
                span,
                "unterminated character literal",
                "expected a closing `'`",
                "for text, use double quotes: \"...\"",
            );
        } else if count == 0 {
            self.error_labeled(
                codes::INVALID_CHAR_LITERAL,
                span,
                "empty character literal",
                "a `char` must hold exactly one character",
            );
        } else if count > 1 {
            self.error_with_help(
                codes::INVALID_CHAR_LITERAL,
                span,
                "character literal holds more than one character",
                format!("found {count} characters"),
                "for text, use double quotes: \"...\"",
            );
        }
        self.push(TokenKind::Char, start);
    }

    /// Consume one escape sequence. The backslash has already been consumed.
    fn escape(&mut self) {
        let bs = self.pos - 1;
        match self.peek() {
            // Let the caller's loop report the unterminated literal.
            None | Some('\n' | '\r') => {}
            Some('n' | 'r' | 't' | '\\' | '"' | '\'' | '0') => {
                self.bump();
            }
            Some('x') => {
                self.bump();
                let digits_start = self.pos;
                for _ in 0..2 {
                    if self.peek().is_some_and(|c| c.is_ascii_hexdigit()) {
                        self.bump();
                    }
                }
                if self.pos - digits_start != 2 {
                    let span = Span::new(bs, self.pos);
                    self.error_labeled(
                        codes::UNKNOWN_ESCAPE,
                        span,
                        "invalid `\\x` escape",
                        "`\\x` must be followed by exactly two hex digits, e.g. `\\x1B`",
                    );
                }
            }
            Some('u') => {
                self.bump();
                if !self.eat('{') {
                    let span = Span::new(bs, self.pos);
                    self.error_labeled(
                        codes::UNKNOWN_ESCAPE,
                        span,
                        "invalid `\\u` escape",
                        "expected `{` — the form is `\\u{1F600}`",
                    );
                    return;
                }
                let digits_start = self.pos;
                while self.peek().is_some_and(|c| c.is_ascii_hexdigit()) {
                    self.bump();
                }
                let digits = self.pos - digits_start;
                if !self.eat('}') || digits == 0 {
                    let span = Span::new(bs, self.pos);
                    self.error_labeled(
                        codes::UNKNOWN_ESCAPE,
                        span,
                        "invalid `\\u` escape",
                        "expected one to six hex digits followed by `}`",
                    );
                }
            }
            Some(c) => {
                let s = self.pos;
                self.bump();
                let span = Span::new(bs, self.pos);
                self.error_with_help(
                    codes::UNKNOWN_ESCAPE,
                    span,
                    format!("unknown escape sequence `\\{c}`"),
                    "not a recognised escape",
                    "valid escapes are \\n \\r \\t \\0 \\\\ \\\" \\' \\xNN \\u{...}",
                );
                let _ = s;
            }
        }
    }

    // --- operators --------------------------------------------------------

    fn operator(&mut self, start: usize, c: char) {
        use TokenKind::*;
        self.bump();
        let kind = match c {
            '+' => {
                if self.eat('=') {
                    PlusEq
                } else {
                    Plus
                }
            }
            '-' => {
                if self.eat('=') {
                    MinusEq
                } else if self.eat('>') {
                    Arrow
                } else {
                    Minus
                }
            }
            '*' => {
                if self.eat('=') {
                    StarEq
                } else {
                    Star
                }
            }
            '%' => {
                if self.eat('=') {
                    PercentEq
                } else {
                    Percent
                }
            }
            '=' => {
                if self.eat('=') {
                    EqEq
                } else {
                    Eq
                }
            }
            '!' => {
                if self.eat('=') {
                    BangEq
                } else {
                    Bang
                }
            }
            '<' => {
                if self.eat('=') {
                    LtEq
                } else if self.eat('<') {
                    Shl
                } else {
                    Lt
                }
            }
            '>' => {
                if self.eat('=') {
                    GtEq
                } else if self.eat('>') {
                    Shr
                } else {
                    Gt
                }
            }
            '&' => {
                if self.eat('&') {
                    AmpAmp
                } else {
                    Amp
                }
            }
            '|' => {
                if self.eat('|') {
                    PipePipe
                } else {
                    Pipe
                }
            }
            // `^` is the pointer sigil, so xor has to be `^^` (spec §2.6).
            '^' => {
                if self.eat('^') {
                    CaretCaret
                } else {
                    Caret
                }
            }
            '~' => Tilde,
            '.' => {
                if self.eat('.') {
                    if self.eat('=') {
                        DotDotEq
                    } else {
                        DotDot
                    }
                } else {
                    Dot
                }
            }
            ':' => {
                if self.eat(':') {
                    ColonColon
                } else {
                    Colon
                }
            }
            ';' => Terminator,
            ',' => Comma,
            '?' => Question,
            '#' => Hash,
            '$' => Dollar,
            '@' => At,
            '(' => LParen,
            ')' => RParen,
            '[' => LBracket,
            ']' => RBracket,
            '{' => LBrace,
            '}' => RBrace,
            _ => {
                let span = Span::new(start, self.pos);
                self.error_labeled(
                    codes::UNEXPECTED_CHARACTER,
                    span,
                    format!("unexpected character `{c}`"),
                    "this cannot begin a token",
                );
                Error
            }
        };
        self.push(kind, start);
    }
}

// ---------------------------------------------------------------------------
// Terminator rules (spec §2.2)
// ---------------------------------------------------------------------------

/// Collapse runs of terminators and drop the ones §2.2 suppresses.
///
/// Doing this as a pass over the finished stream, rather than inline, keeps the scanner
/// free of lookahead and makes each rule independently testable.
fn apply_terminator_rules(tokens: Vec<Token>) -> Vec<Token> {
    let mut out: Vec<Token> = Vec::with_capacity(tokens.len());
    for tok in tokens {
        if tok.kind == TokenKind::Terminator {
            match out.last() {
                // Nothing to terminate yet, or the previous token already terminated.
                None => continue,
                Some(prev) if prev.kind == TokenKind::Terminator => continue,
                Some(prev) if prev.kind.suppresses_following_terminator() => continue,
                _ => {}
            }
            out.push(tok);
        } else {
            // A terminator immediately before a closing bracket is not one.
            if matches!(tok.kind, TokenKind::RParen | TokenKind::RBracket) {
                while out.last().map(|t| t.kind) == Some(TokenKind::Terminator) {
                    out.pop();
                }
            }
            out.push(tok);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

#[inline]
fn is_ident_start(c: char) -> bool {
    c == '_' || unicode_ident::is_xid_start(c)
}

#[inline]
fn is_ident_continue(c: char) -> bool {
    c == '_' || unicode_ident::is_xid_continue(c)
}

/// Split a trailing type suffix off a numeric run, if one is present.
fn split_suffix<'a>(run: &'a str, allowed: &[&str]) -> (&'a str, Option<&'a str>) {
    for suffix in allowed {
        if let Some(digits) = run.strip_suffix(suffix) {
            // `1_f32` is fine; a bare suffix with no digits is caught by the caller.
            if !digits.is_empty() || run == *suffix {
                return (digits, Some(&run[digits.len()..]));
            }
        }
    }
    (run, None)
}

fn radix_name(radix: u32) -> &'static str {
    match radix {
        2 => "binary",
        8 => "octal",
        16 => "hexadecimal",
        _ => "numeric",
    }
}

fn radix_digits(radix: u32) -> &'static str {
    match radix {
        2 => "`0` and `1`",
        8 => "`0` through `7`",
        16 => "`0` through `9` and `a` through `f`",
        _ => "`0` through `9`",
    }
}

fn suffix_list() -> String {
    let mut v: Vec<&str> = SUFFIXES.to_vec();
    v.sort_unstable();
    v.iter().map(|s| format!("`{s}`")).collect::<Vec<_>>().join(", ")
}
