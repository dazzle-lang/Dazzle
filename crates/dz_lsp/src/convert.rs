//! Between compiler positions and protocol ones.
//!
//! The compiler speaks byte offsets into the file exactly as it is (invariant 2); the
//! protocol speaks zero-based lines and UTF-16 code units, because that is what VS Code's
//! strings are. Every conversion goes through here so the two never meet anywhere else.

use std::path::Path;

use lsp_types::{Position, Range};

use dz_span::{SourceFile, Span};

/// The protocol position of a byte offset.
pub fn position(file: &SourceFile, offset: u32) -> Position {
    let text = file.text();
    let mut offset = (offset as usize).min(text.len());
    while !text.is_char_boundary(offset) {
        offset -= 1;
    }
    let line = file.line_index(offset);
    let start = file.line_range(line).map_or(0, |r| r.start);
    let character = text[start..offset].encode_utf16().count() as u32;
    Position::new(line as u32, character)
}

/// The byte offset of a protocol position, clamped to the end of its line.
pub fn offset(file: &SourceFile, pos: Position) -> u32 {
    let text = file.text();
    let Some(range) = file.line_range(pos.line as usize) else {
        return text.len() as u32;
    };
    let content = text[range.clone()].trim_end_matches(['\n', '\r']);
    let mut units = 0u32;
    for (i, c) in content.char_indices() {
        if units >= pos.character {
            return (range.start + i) as u32;
        }
        units += c.len_utf16() as u32;
    }
    (range.start + content.len()) as u32
}

pub fn range(file: &SourceFile, span: Span) -> Range {
    Range::new(position(file, span.start), position(file, span.end))
}

/// A path as a map key, so a buffer the editor names `c:\Game\src\main.dz` is found under the
/// `C:\Game\src/main.dz` the manifest's glob produced.
pub fn path_key(path: &Path) -> String {
    let mut key = path.to_string_lossy().replace('\\', "/");
    if let Some(rest) = key.strip_prefix("//?/") {
        key = rest.to_owned();
    }
    while key.contains("/./") {
        key = key.replace("/./", "/");
    }
    // Windows paths are case-insensitive, and the editor and the file system disagree about
    // the drive letter's case.
    if cfg!(windows) {
        key = key.to_lowercase();
    }
    key
}

#[cfg(test)]
mod tests {
    use super::*;
    use dz_span::SourceMap;

    #[test]
    fn columns_are_utf16_and_every_line_ending_counts() {
        let mut map = SourceMap::new();
        // `é` is two bytes and one unit; the emoji is four bytes and two units.
        let id = map.add("t.dz", "a\r\nhé😀x\rz".to_owned());
        let f = map.get(id);
        let x = "a\r\nhé😀".len() as u32;
        assert_eq!(position(f, x), Position::new(1, 4));
        assert_eq!(offset(f, Position::new(1, 4)), x);
        assert_eq!(position(f, x + 2), Position::new(2, 0));
    }

    #[test]
    fn a_position_past_the_line_end_clamps_to_it() {
        let mut map = SourceMap::new();
        let id = map.add("t.dz", "ab\r\ncd".to_owned());
        let f = map.get(id);
        assert_eq!(offset(f, Position::new(0, 99)), 2);
        assert_eq!(offset(f, Position::new(9, 0)), 6);
    }

    #[test]
    fn path_keys_ignore_separators_and_dot_segments() {
        assert_eq!(path_key(Path::new("/g/./src/main.dz")), path_key(Path::new("/g/src/main.dz")));
        if cfg!(windows) {
            assert_eq!(
                path_key(Path::new(r"c:\Game\src\main.dz")),
                path_key(Path::new("C:/Game/src/main.dz"))
            );
        }
    }
}
