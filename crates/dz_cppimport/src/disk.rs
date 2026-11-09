//! Imported headers kept on disk between processes (T-0101).
//!
//! Every `dz check`, `dz build` and `dz run` is a new process, and the standard library
//! imports `stdlib.h`, so without this each one loads libclang and parses it again: about
//! 85 ms of a build whose own phases take 90. An entry holds what one import produced and
//! every file the parse read. It is used only while each of those files still has the
//! contents it had then: its size and time are compared first, and a file whose time
//! changed is read and hashed, so touching a header costs one read rather than a parse.
//!
//! The key is the in-process cache's (the headers, the directory, every flag and the
//! instantiations) and also what produced the entry: the `dz` executable, since another
//! importer may read the same header differently, and `LIBCLANG_PATH`, which picks the
//! parser. It does not name the libclang the search would find otherwise, since finding
//! that means loading it, which is the cost this avoids. A new libclang's own headers are
//! among the files read, so installing one over the old changes them; an entry left by a
//! different libclang whose files are all still there is used, and gives the declarations
//! that one read from the same headers.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use crate::Header;

/// Bumped when an entry's layout changes.
const FORMAT: &str = "dz header cache 1";

/// How many entries are kept, the most recently written; one written within `IN_USE` is
/// kept regardless, since another process may be about to read it.
const KEEP: usize = 64;
const IN_USE: std::time::Duration = std::time::Duration::from_secs(60 * 60);

#[derive(Serialize, Deserialize)]
struct Entry {
    /// The whole key, so two keys with one hash are told apart.
    key: String,
    header: Header,
    inputs: Vec<Input>,
}

/// A file the parse read, as it was.
#[derive(Serialize, Deserialize)]
struct Input {
    path: PathBuf,
    len: u64,
    modified: Option<SystemTime>,
    hash: u64,
}

impl Input {
    fn of(path: &Path) -> Option<Input> {
        let bytes = std::fs::read(path).ok()?;
        let modified = std::fs::metadata(path).and_then(|m| m.modified()).ok();
        Some(Input {
            path: path.to_path_buf(),
            len: bytes.len() as u64,
            modified,
            hash: fnv1a(&bytes),
        })
    }

    /// Does the file still hold what it held?
    fn holds(&self) -> bool {
        let Ok(meta) = std::fs::metadata(&self.path) else { return false };
        if meta.len() != self.len {
            return false;
        }
        if self.modified.is_some() && meta.modified().ok() == self.modified {
            return true;
        }
        std::fs::read(&self.path).is_ok_and(|bytes| fnv1a(&bytes) == self.hash)
    }
}

/// The full key for `key`, the in-process cache's: with what produced the entry.
pub(crate) fn full_key(key: &str) -> String {
    let exe = std::env::current_exe().ok().and_then(|exe| {
        let meta = std::fs::metadata(&exe).ok()?;
        let modified = meta.modified().ok()?.duration_since(SystemTime::UNIX_EPOCH).ok()?;
        Some(format!("{}\0{}\0{}", exe.display(), meta.len(), modified.as_nanos()))
    });
    let libclang = std::env::var("LIBCLANG_PATH").unwrap_or_default();
    format!("{FORMAT}\0{}\0{libclang}\0{key}", exe.unwrap_or_default())
}

/// Where the entry for `full_key` lives in `dir`.
pub(crate) fn path_of(dir: &Path, full_key: &str) -> PathBuf {
    dir.join("headers").join(format!("{:016x}.json", fnv1a(full_key.as_bytes())))
}

/// The header stored at `path` for `full_key`, and the files it was read from, if every one
/// of them is unchanged.
pub(crate) fn load(path: &Path, full_key: &str) -> Option<(Header, Vec<PathBuf>)> {
    let text = std::fs::read(path).ok()?;
    let entry: Entry = serde_json::from_slice(&text).ok()?;
    if entry.key != full_key || !entry.inputs.iter().all(Input::holds) {
        return None;
    }
    Some((entry.header, entry.inputs.into_iter().map(|i| i.path).collect()))
}

/// Store `header`, read from `inputs`, at `path`. Best effort: an entry that cannot be
/// written means the next process parses the header again, nothing worse.
pub(crate) fn store(path: &Path, full_key: &str, header: &Header, inputs: &[PathBuf]) {
    let Some(inputs) = inputs.iter().map(|p| Input::of(p)).collect::<Option<Vec<_>>>() else {
        return;
    };
    let entry = Entry { key: full_key.to_owned(), header: header.clone(), inputs };
    let Ok(text) = serde_json::to_vec(&entry) else { return };
    let Some(dir) = path.parent() else { return };
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    // Through a file renamed onto it, so a process reading it never sees half of one.
    let mut temp = path.as_os_str().to_owned();
    temp.push(format!(".{}.tmp", std::process::id()));
    if std::fs::write(&temp, &text).is_ok() && std::fs::rename(&temp, path).is_ok() {
        prune(dir);
    } else {
        let _ = std::fs::remove_file(&temp);
    }
}

/// Remove all but the `KEEP` most recently written entries, and never one written within
/// `IN_USE`.
fn prune(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut files: Vec<(SystemTime, PathBuf)> = entries
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    if files.len() <= KEEP {
        return;
    }
    files.sort_by_key(|(written, _)| std::cmp::Reverse(*written));
    let now = SystemTime::now();
    for (written, file) in files.into_iter().skip(KEEP) {
        if now.duration_since(written).is_ok_and(|age| age >= IN_USE) {
            let _ = std::fs::remove_file(file);
        }
    }
}

/// FNV-1a, 64 bits: stable across Rust versions and platforms, as a key on disk needs.
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CInt, CType, ConstValue, Decl, DeclKind, Loc, Param};

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("dz_disk_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn header(file: &Path) -> Header {
        let loc = Loc { file: 0, start: 1, end: 2 };
        let decl = |name: &str, kind| Decl {
            name: name.into(),
            scope: vec![],
            cpp: format!("::{name}"),
            loc,
            docs: "a comment".into(),
            kind,
        };
        let int = CType::Int(CInt { bits: 32, signed: true });
        Header {
            requested: vec!["lib.h".into()],
            paths: vec![file.to_path_buf()],
            path: file.to_path_buf(),
            files: vec![file.to_path_buf()],
            decls: vec![
                decl(
                    "f",
                    DeclKind::Fn {
                        params: vec![Param {
                            name: "p".into(),
                            ty: CType::Ptr { mutable: true, pointee: Box::new(int.clone()) },
                            c_cast: Some("long *".into()),
                            has_default: false,
                        }],
                        ret: int,
                        ret_cast: false,
                    },
                ),
                decl(
                    "BIG",
                    DeclKind::Const { value: ConstValue::Int(i128::MIN), from_macro: true },
                ),
                decl(
                    "HALF",
                    DeclKind::Const {
                        value: ConstValue::Float { value: 0.5, single: true },
                        from_macro: true,
                    },
                ),
            ],
            skipped: vec![],
        }
    }

    #[test]
    fn an_entry_comes_back_as_it_went_in() {
        let dir = scratch("roundtrip");
        let file = dir.join("lib.h");
        std::fs::write(&file, "int f(long *p);\n").unwrap();
        let key = full_key("lib.h\0-std=c++17");
        let path = path_of(&dir, &key);
        store(&path, &key, &header(&file), std::slice::from_ref(&file));
        let (loaded, inputs) = load(&path, &key).expect("stored");
        assert_eq!(loaded, header(&file));
        assert_eq!(inputs, std::slice::from_ref(&file));
        // Another key with the same file name is not this one.
        assert!(load(&path, &full_key("other.h")).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_entry_holds_while_its_files_hold_the_same_contents() {
        let dir = scratch("stale");
        let file = dir.join("lib.h");
        std::fs::write(&file, "int f(long *p);\n").unwrap();
        let key = full_key("lib.h");
        let path = path_of(&dir, &key);
        store(&path, &key, &header(&file), std::slice::from_ref(&file));
        // Written again with the same contents: a new time, but it holds.
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(&file, "int f(long *p);\n").unwrap();
        assert!(load(&path, &key).is_some());
        // The same length, other contents: it does not.
        std::fs::write(&file, "int g(long *p);\n").unwrap();
        assert!(load(&path, &key).is_none());
        // Nor once the file is gone.
        std::fs::remove_file(&file).unwrap();
        assert!(load(&path, &key).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
