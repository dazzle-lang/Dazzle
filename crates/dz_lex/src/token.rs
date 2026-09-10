//! Token kinds, per spec §2.6 (operators) and §2.7 (keywords).

use dz_span::Span;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

impl Token {
    #[inline]
    pub fn new(kind: TokenKind, span: Span) -> Token {
        Token { kind, span }
    }

    /// The token's text, as written in the source.
    #[inline]
    pub fn text<'a>(&self, src: &'a str) -> &'a str {
        &src[self.span.range()]
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum TokenKind {
    // --- trivia and structure ---
    /// A newline or `;`. Spec §2.2.
    Terminator,
    /// `/// ...`, kept because it attaches to the following declaration.
    DocComment,
    Eof,

    // --- literals ---
    Ident,
    Int,
    Float,
    Str,
    RawStr,
    Char,

    // --- keywords (spec §2.7) ---
    As,
    Break,
    Case,
    Catch,
    Const,
    Continue,
    Copy,
    Defer,
    Drop,
    Else,
    Enum,
    Extern,
    False,
    Fn,
    For,
    If,
    Import,
    In,
    Let,
    Macro,
    Module,
    Move,
    Mut,
    None,
    Null,
    Own,
    Pub,
    Return,
    SelfValue,
    SelfType,
    Struct,
    Switch,
    Trait,
    True,
    Try,
    Union,
    While,
    With,

    // --- operators and punctuation (spec §2.6) ---
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    PlusEq,
    MinusEq,
    StarEq,
    SlashEq,
    PercentEq,
    Eq,
    EqEq,
    BangEq,
    Bang,
    Lt,
    Gt,
    LtEq,
    GtEq,
    AmpAmp,
    PipePipe,
    /// `&` — bitwise and, and reference formation.
    Amp,
    /// `|` — bitwise or.
    Pipe,
    /// `^` — pointer type and dereference. Never xor.
    Caret,
    /// `^^` — xor.
    CaretCaret,
    Tilde,
    Shl,
    Shr,
    Dot,
    DotDot,
    DotDotEq,
    Colon,
    ColonColon,
    Comma,
    Arrow,
    Question,
    /// `#` — attribute, macro invocation, or intrinsic (spec §12.8).
    Hash,
    /// `$` — macro variable.
    Dollar,
    /// `@` — overload disambiguation on imported C++ (spec §15.3).
    At,
    LParen,
    RParen,
    LBracket,
    RBracket,
    LBrace,
    RBrace,

    /// Produced for input the lexer could not classify. Always accompanied by a
    /// diagnostic; kept in the stream so the parser still sees a complete token sequence.
    Error,
}

impl TokenKind {
    /// The name used in token dumps and snapshot tests.
    pub fn name(&self) -> &'static str {
        use TokenKind::*;
        match self {
            Terminator => "Terminator",
            DocComment => "DocComment",
            Eof => "Eof",
            Ident => "Ident",
            Int => "Int",
            Float => "Float",
            Str => "Str",
            RawStr => "RawStr",
            Char => "Char",
            As => "as",
            Break => "break",
            Case => "case",
            Catch => "catch",
            Const => "const",
            Continue => "continue",
            Copy => "copy",
            Defer => "defer",
            Drop => "drop",
            Else => "else",
            Enum => "enum",
            Extern => "extern",
            False => "false",
            Fn => "fn",
            For => "for",
            If => "if",
            Import => "import",
            In => "in",
            Let => "let",
            Macro => "macro",
            Module => "module",
            Move => "move",
            Mut => "mut",
            None => "none",
            Null => "null",
            Own => "own",
            Pub => "pub",
            Return => "return",
            SelfValue => "self",
            SelfType => "Self",
            Struct => "struct",
            Switch => "switch",
            Trait => "trait",
            True => "true",
            Try => "try",
            Union => "union",
            While => "while",
            With => "with",
            Plus => "+",
            Minus => "-",
            Star => "*",
            Slash => "/",
            Percent => "%",
            PlusEq => "+=",
            MinusEq => "-=",
            StarEq => "*=",
            SlashEq => "/=",
            PercentEq => "%=",
            Eq => "=",
            EqEq => "==",
            BangEq => "!=",
            Bang => "!",
            Lt => "<",
            Gt => ">",
            LtEq => "<=",
            GtEq => ">=",
            AmpAmp => "&&",
            PipePipe => "||",
            Amp => "&",
            Pipe => "|",
            Caret => "^",
            CaretCaret => "^^",
            Tilde => "~",
            Shl => "<<",
            Shr => ">>",
            Dot => ".",
            DotDot => "..",
            DotDotEq => "..=",
            Colon => ":",
            ColonColon => "::",
            Comma => ",",
            Arrow => "->",
            Question => "?",
            Hash => "#",
            Dollar => "$",
            At => "@",
            LParen => "(",
            RParen => ")",
            LBracket => "[",
            RBracket => "]",
            LBrace => "{",
            RBrace => "}",
            Error => "Error",
        }
    }

    pub fn is_keyword(&self) -> bool {
        use TokenKind::*;
        matches!(
            self,
            As | Break
                | Case
                | Catch
                | Const
                | Continue
                | Copy
                | Defer
                | Drop
                | Else
                | Enum
                | Extern
                | False
                | Fn
                | For
                | If
                | Import
                | In
                | Let
                | Macro
                | Module
                | Move
                | Mut
                | None
                | Null
                | Own
                | Pub
                | Return
                | SelfValue
                | SelfType
                | Struct
                | Switch
                | Trait
                | True
                | Try
                | Union
                | While
                | With
        )
    }

    pub fn is_literal(&self) -> bool {
        use TokenKind::*;
        matches!(self, Int | Float | Str | RawStr | Char | True | False | Null | None)
    }

    /// After these, a newline does not terminate a statement (spec §2.2).
    pub fn suppresses_following_terminator(&self) -> bool {
        use TokenKind::*;
        matches!(
            self,
            Plus | Minus
                | Star
                | Slash
                | Percent
                | PlusEq
                | MinusEq
                | StarEq
                | SlashEq
                | PercentEq
                | Eq
                | EqEq
                | BangEq
                | Bang
                | Lt
                // `Gt` is deliberately absent: `>` also closes a generic argument list,
                // so a field like `m_verts: Buffer<Vertex>` ends the line.
                | LtEq
                | GtEq
                | AmpAmp
                | PipePipe
                | Amp
                | Pipe
                // `Caret` is deliberately absent: `^` is postfix dereference, so a line
                // ending in `p^` is a complete statement. `^^` (xor) is infix and does
                // suppress.
                | CaretCaret
                | Tilde
                | Shl
                | Shr
                | Dot
                | DotDot
                | DotDotEq
                | Colon
                | ColonColon
                | Comma
                | Arrow
                | Question
                | LParen
                | LBracket
                | LBrace
                | Hash
                | Dollar
                | At
        )
    }
}

/// Map an identifier to its keyword kind, if it is one.
pub fn keyword(text: &str) -> Option<TokenKind> {
    use TokenKind::*;
    Some(match text {
        "as" => As,
        "break" => Break,
        "case" => Case,
        "catch" => Catch,
        "const" => Const,
        "continue" => Continue,
        "copy" => Copy,
        "defer" => Defer,
        "drop" => Drop,
        "else" => Else,
        "enum" => Enum,
        "extern" => Extern,
        "false" => False,
        "fn" => Fn,
        "for" => For,
        "if" => If,
        "import" => Import,
        "in" => In,
        "let" => Let,
        "macro" => Macro,
        "module" => Module,
        "move" => Move,
        "mut" => Mut,
        "none" => None,
        "null" => Null,
        "own" => Own,
        "pub" => Pub,
        "return" => Return,
        "self" => SelfValue,
        "Self" => SelfType,
        "struct" => Struct,
        "switch" => Switch,
        "trait" => Trait,
        "true" => True,
        "try" => Try,
        "union" => Union,
        "while" => While,
        "with" => With,
        _ => return Option::None,
    })
}

/// Reserved for future use (spec §2.7). Lexed as identifiers today; the parser will
/// reject them with a clear message rather than a confusing syntax error.
pub const RESERVED: &[&str] =
    &["async", "await", "impl", "where", "yield", "unsafe", "comptime"];
