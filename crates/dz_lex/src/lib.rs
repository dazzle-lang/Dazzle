//! The Dazzle lexer (spec §2).

mod lexer;
mod token;

pub use lexer::{lex, Comment, CommentKind, LexResult};
pub use token::{keyword, Token, TokenKind, RESERVED};

use dz_span::SourceFile;

/// Render a token stream as text, for snapshot tests and `dz lex`.
///
/// The format is line:column, token kind, and — for anything whose spelling is not implied
/// by its kind — the source text.
pub fn dump_tokens(file: &SourceFile, tokens: &[Token]) -> String {
    use std::fmt::Write;

    let mut out = String::new();
    for tok in tokens {
        let loc = file.location(tok.span.start as usize);
        let _ = write!(out, "{:>4}:{:<3} {}", loc.line, loc.column, tok.kind.name());

        let show_text = matches!(
            tok.kind,
            TokenKind::Ident
                | TokenKind::Int
                | TokenKind::Float
                | TokenKind::Str
                | TokenKind::RawStr
                | TokenKind::Char
                | TokenKind::DocComment
                | TokenKind::Error
        );
        if show_text {
            let text = tok.text(file.text());
            let _ = write!(out, " {}", escape_for_dump(text));
        }
        out.push('\n');
    }
    out
}

fn escape_for_dump(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('`');
    for c in text.chars() {
        match c {
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('`');
    out
}
