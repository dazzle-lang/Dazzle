//! A small TOML parser covering exactly the subset `dazzle.toml` uses.
//!
//! Hand-written rather than pulled from a crate for one reason that matters: **every key
//! and value keeps its [`Span`]**, so a mistake in a manifest gets the same rustc-quality
//! diagnostic as a mistake in a `.dz` file, pointing at the offending value rather than at
//! the file as a whole.
//!
//! Supported: `#` comments, `[table]` and `[dotted.table]` headers, bare keys, basic
//! strings with `\\ \" \n \r \t \0` escapes, integers, booleans, arrays (multi-line
//! allowed, trailing comma allowed), and inline tables.
//!
//! Deliberately **not** supported, because the manifest format does not use them:
//! multi-line strings, literal (single-quoted) strings, floats, datetimes, dotted keys,
//! and arrays of tables (`[[x]]`). Each is rejected with a diagnostic that says so.

use dz_diag::{codes, Diagnostic, Diagnostics};
use dz_span::{FileId, Span};

#[derive(Debug, Clone)]
pub struct Table {
    /// Dotted name, e.g. `dependencies` or `cpp`. Empty for the root table.
    pub name: String,
    pub header_span: Span,
    pub entries: Vec<Entry>,
}

#[derive(Debug, Clone)]
pub struct Entry {
    pub key: String,
    pub key_span: Span,
    pub value: Value,
}

#[derive(Debug, Clone)]
pub enum Value {
    Str(String, Span),
    Int(i64, Span),
    Bool(bool, Span),
    Array(Vec<Value>, Span),
    InlineTable(Vec<Entry>, Span),
}

impl Value {
    pub fn span(&self) -> Span {
        match self {
            Value::Str(_, s)
            | Value::Int(_, s)
            | Value::Bool(_, s)
            | Value::Array(_, s)
            | Value::InlineTable(_, s) => *s,
        }
    }

    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Str(..) => "a string",
            Value::Int(..) => "an integer",
            Value::Bool(..) => "a boolean",
            Value::Array(..) => "an array",
            Value::InlineTable(..) => "a table",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Document {
    pub tables: Vec<Table>,
}

impl Document {
    pub fn table(&self, name: &str) -> Option<&Table> {
        self.tables.iter().find(|t| t.name == name)
    }
}

impl Table {
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.entries.iter().find(|e| e.key == key).map(|e| &e.value)
    }

    pub fn entry(&self, key: &str) -> Option<&Entry> {
        self.entries.iter().find(|e| e.key == key)
    }
}

pub fn parse(file: FileId, src: &str, diags: &mut Diagnostics) -> Document {
    let mut p = Parser { file, src, pos: 0, diags };
    p.document()
}

struct Parser<'a> {
    file: FileId,
    src: &'a str,
    pos: usize,
    diags: &'a mut Diagnostics,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<char> {
        self.src[self.pos..].chars().next()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += c.len_utf8();
        Some(c)
    }

    fn error(&mut self, span: Span, msg: impl Into<String>, label: impl Into<String>) {
        self.diags.push(Diagnostic::error(codes::MANIFEST_SYNTAX, msg.into()).at(
            self.file,
            span,
            label.into(),
        ));
    }

    /// Skip spaces, tabs and comments; `stop_at_newline` keeps line structure visible.
    fn skip_trivia(&mut self, stop_at_newline: bool) {
        loop {
            match self.peek() {
                Some(' ') | Some('\t') | Some('\r') => {
                    self.bump();
                }
                Some('\n') if !stop_at_newline => {
                    self.bump();
                }
                Some('#') => {
                    while !matches!(self.peek(), None | Some('\n')) {
                        self.bump();
                    }
                }
                _ => return,
            }
        }
    }

    fn document(&mut self) -> Document {
        let mut doc = Document::default();
        let mut current =
            Table { name: String::new(), header_span: Span::DUMMY, entries: Vec::new() };

        loop {
            self.skip_trivia(false);
            let Some(c) = self.peek() else { break };

            if c == '[' {
                let start = self.pos;
                self.bump();
                if self.peek() == Some('[') {
                    let span = Span::new(start, self.pos + 1);
                    self.error(
                        span,
                        "arrays of tables are not supported in dazzle.toml",
                        "`[[...]]` has no meaning here",
                    );
                    // Skip the whole header so the rest of the file still parses.
                    while !matches!(self.peek(), None | Some('\n')) {
                        self.bump();
                    }
                    continue;
                }
                let name = self.table_name();
                self.skip_trivia(true);
                if self.peek() == Some(']') {
                    self.bump();
                } else {
                    let span = Span::new(start, self.pos);
                    self.error(span, "unclosed table header", "expected `]`");
                }
                let header_span = Span::new(start, self.pos);

                doc.tables.push(std::mem::replace(
                    &mut current,
                    Table { name, header_span, entries: Vec::new() },
                ));
                continue;
            }

            match self.entry() {
                Some(e) => current.entries.push(e),
                None => {
                    // Unparseable line: report once and move to the next.
                    let start = self.pos;
                    while !matches!(self.peek(), None | Some('\n')) {
                        self.bump();
                    }
                    let span = Span::new(start, self.pos);
                    if !span.is_empty() {
                        self.error(span, "expected a `key = value` entry", "not a valid entry");
                    }
                }
            }
        }

        doc.tables.push(current);
        // The synthetic root table is only interesting if something was written above the
        // first header.
        doc.tables.retain(|t| !t.name.is_empty() || !t.entries.is_empty());
        doc
    }

    fn table_name(&mut self) -> String {
        let mut name = String::new();
        loop {
            self.skip_trivia(true);
            let start = self.pos;
            while self.peek().is_some_and(is_bare_key_char) {
                self.bump();
            }
            name.push_str(&self.src[start..self.pos]);
            self.skip_trivia(true);
            if self.peek() == Some('.') {
                self.bump();
                name.push('.');
                continue;
            }
            return name;
        }
    }

    fn entry(&mut self) -> Option<Entry> {
        self.skip_trivia(true);
        let key_start = self.pos;
        // A quoted key, which TOML allows and `[cpp.instantiate]` needs: a C++ type name
        // has `:` and `<` in it and is no bare key (spec §15.4).
        let key = if self.peek() == Some('"') {
            match self.string() {
                Value::Str(s, _) => s,
                _ => return None,
            }
        } else {
            while self.peek().is_some_and(is_bare_key_char) {
                self.bump();
            }
            if self.pos == key_start {
                return None;
            }
            self.src[key_start..self.pos].to_owned()
        };
        let key_span = Span::new(key_start, self.pos);

        self.skip_trivia(true);
        if self.peek() != Some('=') {
            let span = Span::new(key_start, self.pos);
            self.error(span, format!("expected `=` after key `{key}`"), "missing `=`");
            return None;
        }
        self.bump();

        let value = self.value()?;
        Some(Entry { key, key_span, value })
    }

    fn value(&mut self) -> Option<Value> {
        self.skip_trivia(true);
        let start = self.pos;
        match self.peek() {
            Some('"') => Some(self.string()),
            Some('\'') => {
                while !matches!(self.peek(), None | Some('\n')) {
                    self.bump();
                }
                let span = Span::new(start, self.pos);
                self.error(
                    span,
                    "literal strings are not supported in dazzle.toml",
                    "use a double-quoted string",
                );
                None
            }
            Some('[') => Some(self.array()),
            Some('{') => Some(self.inline_table()),
            Some('t') | Some('f') => {
                while self.peek().is_some_and(|c| c.is_ascii_alphabetic()) {
                    self.bump();
                }
                let span = Span::new(start, self.pos);
                match &self.src[start..self.pos] {
                    "true" => Some(Value::Bool(true, span)),
                    "false" => Some(Value::Bool(false, span)),
                    other => {
                        let other = other.to_owned();
                        self.error(
                            span,
                            format!("expected a value, found `{other}`"),
                            "unquoted text is not a value",
                        );
                        None
                    }
                }
            }
            Some(c) if c == '-' || c.is_ascii_digit() => {
                self.bump();
                while self.peek().is_some_and(|c| c.is_ascii_digit() || c == '_') {
                    self.bump();
                }
                // A `.` here means a float or a date, neither of which the manifest uses.
                if matches!(self.peek(), Some('.') | Some('-') | Some(':')) {
                    while self
                        .peek()
                        .is_some_and(|c| !c.is_whitespace() && c != ',' && c != ']' && c != '}')
                    {
                        self.bump();
                    }
                    let span = Span::new(start, self.pos);
                    self.error(
                        span,
                        "only integers are supported in dazzle.toml",
                        "floats, dates and times have no meaning here",
                    );
                    return None;
                }
                let span = Span::new(start, self.pos);
                let text: String =
                    self.src[start..self.pos].chars().filter(|c| *c != '_').collect();
                match text.parse::<i64>() {
                    Ok(v) => Some(Value::Int(v, span)),
                    Err(_) => {
                        self.error(span, "integer is out of range", "does not fit in 64 bits");
                        None
                    }
                }
            }
            _ => {
                while !matches!(self.peek(), None | Some('\n')) {
                    self.bump();
                }
                let span = Span::new(start, self.pos);
                if !span.is_empty() {
                    self.error(
                        span,
                        "expected a value",
                        "not a string, integer, boolean, array or table",
                    );
                }
                None
            }
        }
    }

    fn string(&mut self) -> Value {
        let start = self.pos;
        self.bump(); // opening quote
        let mut out = String::new();
        loop {
            match self.bump() {
                None | Some('\n') => {
                    let span = Span::new(start, self.pos);
                    self.error(span, "unterminated string", "expected a closing `\"`");
                    break;
                }
                Some('"') => break,
                Some('\\') => match self.bump() {
                    Some('n') => out.push('\n'),
                    Some('r') => out.push('\r'),
                    Some('t') => out.push('\t'),
                    Some('0') => out.push('\0'),
                    Some('\\') => out.push('\\'),
                    Some('"') => out.push('"'),
                    Some(c) => {
                        let span = Span::new(self.pos - c.len_utf8() - 1, self.pos);
                        self.error(
                            span,
                            format!("unknown escape `\\{c}`"),
                            "valid escapes are \\n \\r \\t \\0 \\\\ \\\"",
                        );
                    }
                    None => break,
                },
                Some(c) => out.push(c),
            }
        }
        Value::Str(out, Span::new(start, self.pos))
    }

    fn array(&mut self) -> Value {
        let start = self.pos;
        self.bump(); // `[`
        let mut items = Vec::new();
        loop {
            self.skip_trivia(false);
            match self.peek() {
                None => {
                    let span = Span::new(start, self.pos);
                    self.error(span, "unclosed array", "expected `]`");
                    break;
                }
                Some(']') => {
                    self.bump();
                    break;
                }
                _ => {}
            }
            match self.value() {
                Some(v) => items.push(v),
                None => break,
            }
            self.skip_trivia(false);
            if self.peek() == Some(',') {
                self.bump();
            }
        }
        Value::Array(items, Span::new(start, self.pos))
    }

    fn inline_table(&mut self) -> Value {
        let start = self.pos;
        self.bump(); // `{`
        let mut entries = Vec::new();
        loop {
            self.skip_trivia(false);
            match self.peek() {
                None => {
                    let span = Span::new(start, self.pos);
                    self.error(span, "unclosed inline table", "expected `}`");
                    break;
                }
                Some('}') => {
                    self.bump();
                    break;
                }
                _ => {}
            }
            match self.entry() {
                Some(e) => entries.push(e),
                None => break,
            }
            self.skip_trivia(false);
            if self.peek() == Some(',') {
                self.bump();
            }
        }
        Value::InlineTable(entries, Span::new(start, self.pos))
    }
}

fn is_bare_key_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '-'
}
