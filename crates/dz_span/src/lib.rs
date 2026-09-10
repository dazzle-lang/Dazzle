//! Source files, byte spans, and line/column mapping.
//!
//! Spans are byte offsets into the file **exactly as it exists on disk**. Nothing is
//! rewritten before lexing — line endings are normalised by the lexer as it scans, not by
//! editing the buffer — so a span always points at real bytes in the user's file. That is
//! what keeps diagnostics honest once `#line` directives (spec §16.3) map generated C++
//! back to Dazzle source.

use std::ops::Range;
use std::path::{Path, PathBuf};

/// Index of a file within a [`SourceMap`].
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct FileId(pub u32);

/// A half-open byte range `[start, end)` within a single file.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Span {
    pub start: u32,
    pub end: u32,
}

impl Span {
    pub const DUMMY: Span = Span { start: 0, end: 0 };

    #[inline]
    pub fn new(start: usize, end: usize) -> Span {
        debug_assert!(start <= end, "span start must not exceed end");
        Span { start: start as u32, end: end as u32 }
    }

    #[inline]
    pub fn at(offset: usize) -> Span {
        Span::new(offset, offset)
    }

    #[inline]
    pub fn len(&self) -> u32 {
        self.end - self.start
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }

    #[inline]
    pub fn range(&self) -> Range<usize> {
        self.start as usize..self.end as usize
    }

    /// The smallest span covering both `self` and `other`.
    #[inline]
    pub fn to(&self, other: Span) -> Span {
        Span { start: self.start.min(other.start), end: self.end.max(other.end) }
    }
}

impl std::fmt::Debug for Span {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}..{}", self.start, self.end)
    }
}

/// A 1-based line and column, for display only.
///
/// `column` counts **characters**, not bytes, so a line of CJK text or emoji reports the
/// column a human would count in an editor.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Location {
    pub line: u32,
    pub column: u32,
}

/// Why a file could not be accepted as Dazzle source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EncodingError {
    /// A UTF-16 byte-order mark was found (spec §2.1).
    Utf16 { little_endian: bool },
    /// The bytes are not valid UTF-8.
    InvalidUtf8 { offset: usize },
}

impl std::fmt::Display for EncodingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EncodingError::Utf16 { little_endian } => write!(
                f,
                "this file is UTF-16{}, which Dazzle cannot read; convert it to UTF-8",
                if *little_endian { "LE" } else { "BE" }
            ),
            EncodingError::InvalidUtf8 { offset } => {
                write!(f, "invalid UTF-8 byte at offset {offset}")
            }
        }
    }
}

/// A single source file plus its precomputed line table.
pub struct SourceFile {
    id: FileId,
    path: PathBuf,
    text: String,
    /// Byte offset of the first character of each line. Always starts with 0.
    line_starts: Vec<u32>,
}

impl SourceFile {
    #[inline]
    pub fn id(&self) -> FileId {
        self.id
    }

    #[inline]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The path as shown in diagnostics.
    pub fn name(&self) -> String {
        self.path.display().to_string().replace('\\', "/")
    }

    #[inline]
    pub fn text(&self) -> &str {
        &self.text
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.text.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    #[inline]
    pub fn line_count(&self) -> usize {
        self.line_starts.len()
    }

    /// 0-based index of the line containing `offset`.
    pub fn line_index(&self, offset: usize) -> usize {
        let offset = offset.min(self.text.len()) as u32;
        match self.line_starts.binary_search(&offset) {
            Ok(exact) => exact,
            Err(next) => next - 1,
        }
    }

    /// Byte range of a 0-based line, including its terminator.
    pub fn line_range(&self, line: usize) -> Option<Range<usize>> {
        let start = *self.line_starts.get(line)? as usize;
        let end = self
            .line_starts
            .get(line + 1)
            .map(|&e| e as usize)
            .unwrap_or(self.text.len());
        Some(start..end)
    }

    /// The text of a 0-based line, without its line terminator.
    pub fn line_text(&self, line: usize) -> Option<&str> {
        let range = self.line_range(line)?;
        Some(self.text[range].trim_end_matches(['\n', '\r']))
    }

    /// 1-based line and character column for a byte offset.
    pub fn location(&self, offset: usize) -> Location {
        let line = self.line_index(offset);
        let line_start = self.line_starts[line] as usize;
        let offset = offset.min(self.text.len());
        // Count characters, not bytes, so columns match what an editor shows.
        let column = self.text[line_start..offset].chars().count() + 1;
        Location { line: line as u32 + 1, column: column as u32 }
    }
}

/// Every file in one compilation.
#[derive(Default)]
pub struct SourceMap {
    files: Vec<SourceFile>,
}

impl SourceMap {
    pub fn new() -> SourceMap {
        SourceMap::default()
    }

    /// Add a file from raw bytes, rejecting encodings Dazzle does not accept.
    ///
    /// A UTF-8 BOM is *kept* in the text — the lexer skips it as leading whitespace — so
    /// that byte offsets continue to match the file on disk.
    pub fn add_bytes(
        &mut self,
        path: impl Into<PathBuf>,
        bytes: &[u8],
    ) -> Result<FileId, EncodingError> {
        if bytes.len() >= 2 {
            match (bytes[0], bytes[1]) {
                (0xFE, 0xFF) => return Err(EncodingError::Utf16 { little_endian: false }),
                (0xFF, 0xFE) => return Err(EncodingError::Utf16 { little_endian: true }),
                _ => {}
            }
        }
        let text = std::str::from_utf8(bytes)
            .map_err(|e| EncodingError::InvalidUtf8 { offset: e.valid_up_to() })?;
        Ok(self.add(path, text.to_owned()))
    }

    pub fn add(&mut self, path: impl Into<PathBuf>, text: String) -> FileId {
        let id = FileId(self.files.len() as u32);
        let line_starts = compute_line_starts(&text);
        self.files.push(SourceFile { id, path: path.into(), text, line_starts });
        id
    }

    #[inline]
    pub fn get(&self, id: FileId) -> &SourceFile {
        &self.files[id.0 as usize]
    }

    #[inline]
    pub fn try_get(&self, id: FileId) -> Option<&SourceFile> {
        self.files.get(id.0 as usize)
    }

    pub fn iter(&self) -> impl Iterator<Item = &SourceFile> {
        self.files.iter()
    }
}

/// Byte offsets of line starts, treating `\n`, `\r\n` and lone `\r` as terminators.
fn compute_line_starts(text: &str) -> Vec<u32> {
    let mut starts = vec![0u32];
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\n' => {
                starts.push(i as u32 + 1);
                i += 1;
            }
            b'\r' => {
                // `\r\n` is one terminator, a lone `\r` is also one.
                let advance = if bytes.get(i + 1) == Some(&b'\n') { 2 } else { 1 };
                starts.push((i + advance) as u32);
                i += advance;
            }
            _ => i += 1,
        }
    }
    starts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locations_are_one_based() {
        let mut map = SourceMap::new();
        let id = map.add("t.dz", "let a = 1\nlet b = 2\n".to_owned());
        let f = map.get(id);
        assert_eq!(f.location(0), Location { line: 1, column: 1 });
        assert_eq!(f.location(4), Location { line: 1, column: 5 });
        assert_eq!(f.location(10), Location { line: 2, column: 1 });
    }

    #[test]
    fn all_three_line_endings_split_lines() {
        let mut map = SourceMap::new();
        let id = map.add("t.dz", "a\nb\r\nc\rd".to_owned());
        let f = map.get(id);
        assert_eq!(f.line_count(), 4);
        assert_eq!(f.line_text(0), Some("a"));
        assert_eq!(f.line_text(1), Some("b"));
        assert_eq!(f.line_text(2), Some("c"));
        assert_eq!(f.line_text(3), Some("d"));
    }

    #[test]
    fn columns_count_characters_not_bytes() {
        let mut map = SourceMap::new();
        // "héllo" — the é is two bytes, so byte 6 is column 6, not column 7.
        let id = map.add("t.dz", "héllo x".to_owned());
        let f = map.get(id);
        assert_eq!(f.location(6).column, 6);
    }

    #[test]
    fn utf16_is_rejected() {
        let mut map = SourceMap::new();
        let err = map.add_bytes("t.dz", &[0xFF, 0xFE, 0x61, 0x00]).unwrap_err();
        assert_eq!(err, EncodingError::Utf16 { little_endian: true });
    }
}
