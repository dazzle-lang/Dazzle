//! The build stamp: `dz build` with nothing changed checks the size and time of what the
//! last build read, rather than analysing the whole program again (spec §16.6, T-0192).
//!
//! A build that succeeds writes `build/obj/dz/build-<key>.stamp`. The next `dz build` or
//! `dz run` with the same key reads it before anything else, and when every input still
//! holds and the executable is the one that build made, it prints that build's warnings and
//! stops there; `dz run` runs the program. Analysing T-0012's 100-module project took 2 s,
//! where `make` takes 0.05 s to say nothing needs doing.
//!
//! What the stamp holds, and so what makes a build run again:
//!
//! - **the key**: the `dz` executable (path, size and time, so a `dz` built again counts as
//!   another compiler, as it may generate other C++), the entry, the options, and the
//!   environment variables that change what a build makes. A different key is another
//!   stamp file, so a debug and a release build each keep their own;
//! - **the C++ compiler**: the file its name resolves to on the `PATH`, by size and time;
//! - **the executable**, by size and time;
//! - **every source pattern** with what it matched, expanded again, so a file added or
//!   removed counts;
//! - **every CMake library's source tree**, by its files' paths, sizes and times;
//! - **every file read**: manifests, `.dz` sources, what each `import cpp` read, and every
//!   file each object was compiled from (generated C++, library sources, C and system
//!   headers), by size, time and hash, as an object's own stamp is (`native::up_to_date`).
//!
//! A source's hash is of the bytes the build read. If the file changed while the build ran,
//! its time is recorded as unknown, so the next build hashes it, finds a difference, and
//! builds again. Anything unreadable or unexpected means building again: the stamp only
//! ever skips a build it can prove would change nothing.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::native::{fnv1a, inputs_hold, nanos, refreshed, stamp_inputs, Input};

const FORMAT: &str = "dz build stamp 1";

/// The environment variables that change what a build makes: the compiler and its flags,
/// the regions switch, where the shared `std` and the header cache live, and the libclang
/// that imports headers.
const ENV: &[&str] = &[
    "DZ_CXX",
    "DZ_CXXFLAGS",
    "DZ_REGIONS",
    "DZ_CACHE_DIR",
    "XDG_CACHE_HOME",
    "LOCALAPPDATA",
    "HOME",
    "LIBCLANG_PATH",
    "SDKROOT",
    "DZ_FUZZ_ALLOW",
];

/// What a build was asked for, other than its inputs: two builds with different keys are
/// different builds, and keep different stamps.
pub fn key(
    entry: &str,
    options: &dz_driver::BuildOptions,
    cxx: Option<&str>,
    line_directives: bool,
) -> String {
    let mut key = String::new();
    // A test can stand in another identity, as if `dz` had been built again.
    let identity = std::env::var("DZ_STAMP_IDENTITY").ok().unwrap_or_else(|| {
        let exe = std::env::current_exe().ok();
        let meta = exe.as_ref().and_then(|e| std::fs::metadata(e).ok());
        let time = meta.as_ref().and_then(|m| m.modified().ok()).and_then(nanos).unwrap_or(0);
        format!(
            "{} {} {time} {}",
            option_env!("DZ_RELEASE").unwrap_or(env!("CARGO_PKG_VERSION")),
            meta.map_or(0, |m| m.len()),
            exe.map(|e| e.display().to_string()).unwrap_or_default()
        )
    });
    key.push_str(&format!("dz {identity}\n"));
    // The entry as it was written, and where from: a program names its sources the way the
    // command line did (T-0188), so `dz build` in a project and `dz build game` beside it
    // make different programs, and neither may stand in for the other.
    key.push_str(&format!("entry {entry}\n"));
    key.push_str(&format!("from {}\n", absolute(Path::new(".")).display()));
    key.push_str(&format!("options {options:?}\n"));
    key.push_str(&format!("cxx {}\n", cxx.unwrap_or("")));
    key.push_str(&format!("line-directives {line_directives}\n"));
    for name in ENV {
        match std::env::var_os(name) {
            Some(v) => key.push_str(&format!("env {name}={}\n", v.to_string_lossy())),
            None => key.push_str(&format!("env {name} unset\n")),
        }
    }
    key
}

/// Where the stamp of a build with `key` lives, under the build's output directory.
pub fn stamp_path(build_root: &Path, key: &str) -> PathBuf {
    build_root.join("obj").join("dz").join(format!("build-{:016x}.stamp", fnv1a(key.as_bytes())))
}

/// What a build found still up to date leaves to do: name the executable, and print the
/// warnings it printed.
pub struct Fresh {
    pub exe: PathBuf,
    pub warnings: String,
}

/// The executable of the last build with `key`, if nothing it was built from has changed.
pub fn check(build_root: &Path, key: &str) -> Option<Fresh> {
    let path = stamp_path(build_root, key);
    let text = std::fs::read_to_string(&path).ok()?;
    let written = std::fs::metadata(&path).ok()?.modified().ok()?;
    let (head, warnings) = text.split_once("\nwarnings\n")?;
    let mut lines = head.lines();
    if lines.next()? != FORMAT {
        return None;
    }
    // The key exactly, line by line.
    for want in key.lines() {
        if lines.next()? != want {
            return None;
        }
    }
    if lines.next()? != "end key" {
        return None;
    }

    let mut exe = None;
    let mut files = Vec::new();
    for line in lines {
        let (kind, rest) = line.split_once(' ')?;
        match kind {
            "cxx" => {
                let (recorded, name) = rest.split_once('\t')?;
                if tool_line(name)? != recorded {
                    return None;
                }
            }
            "exe" => {
                let (recorded, path) = split_path(rest, 2)?;
                if file_line(&path)? != recorded {
                    return None;
                }
                exe = Some(path);
            }
            "glob" => {
                let (hash, pattern) = rest.split_once(' ')?;
                let found = dz_project::glob_entries(pattern).ok()?;
                if format!("{:016x}", list_hash(&found)) != hash {
                    return None;
                }
            }
            "tree" => {
                let (hash, dir) = rest.split_once(' ')?;
                if format!("{:016x}", tree_signature(Path::new(dir))) != hash {
                    return None;
                }
            }
            "file" => files.extend(stamp_inputs(rest)?),
            _ => return None,
        }
    }
    // Named as the build that made it named it: under the output directory as given.
    let exe = exe?;
    let exe = match exe.strip_prefix(absolute(build_root)).ok() {
        Some(rest) => build_root.join(rest),
        None => exe,
    };
    let hashed = inputs_hold(&files, written)?;
    // Hashing was needed only for files written just before the stamp; with their times
    // settled now, the next check need not read them.
    if hashed {
        if let Some(fresh) = refreshed(&files) {
            let mut text = String::with_capacity(text.len());
            for line in head.lines().filter(|l| !l.starts_with("file ")) {
                text.push_str(line);
                text.push('\n');
            }
            for input in &fresh {
                text.push_str("file ");
                text.push_str(&input.line());
            }
            text.push_str("warnings\n");
            text.push_str(warnings);
            let _ = crate::shared::write(&path, text.as_bytes());
        }
    }
    Some(Fresh { exe, warnings: warnings.to_owned() })
}

/// Write the stamp of a build that has just made `exe` with `cxx`, from `analysis`.
/// `warnings` is what it printed. Nothing is written when anything it needs cannot be read:
/// the next build then simply runs.
pub fn write(
    build_root: &Path,
    key: &str,
    analysis: &crate::build::Analysis,
    cxx: &str,
    exe: &Path,
    warnings: &str,
) {
    let _ = try_write(build_root, key, analysis, cxx, exe, warnings);
}

fn try_write(
    build_root: &Path,
    key: &str,
    analysis: &crate::build::Analysis,
    cxx: &str,
    exe: &Path,
    warnings: &str,
) -> Option<()> {
    let ws = analysis.ws.as_ref()?;
    let cwd = std::env::current_dir().ok()?;
    let mut inputs: BTreeMap<PathBuf, Input> = BTreeMap::new();

    // What each object was compiled from, and what the link read: the link stamp names the
    // objects, and each object's stamp the files it was compiled from. No link stamp means
    // a link this check could not trust (one from temporary objects), so no build stamp.
    let stem = exe.file_stem()?.to_string_lossy();
    let link = std::fs::read_to_string(
        build_root.join("obj").join("dz").join(format!("link-{stem}.stamp")),
    )
    .ok()?;
    for line in link.lines().skip(2) {
        let Some((part, _)) = split_path_back(line, 2) else { continue };
        let part = Path::new(part);
        if !part.is_file() {
            continue;
        }
        if part.extension().is_some_and(|e| e == "o" || e == "obj") {
            // As the object's stamp recorded them, hashed when it was compiled: the object
            // was compiled from exactly those, and this build linked it.
            let stamp = std::fs::read_to_string(part.with_extension("stamp")).ok()?;
            for input in stamp_inputs(&stamp)? {
                inputs.entry(input.path.clone()).or_insert(input);
            }
        } else {
            inputs.entry(part.to_path_buf()).or_insert(Input::of(part)?);
        }
    }
    for p in dz_project::cppimport::files_read() {
        if let Some(input) = Input::of(&p) {
            inputs.entry(p).or_insert(input);
        }
    }
    // Last, so they win: the files analysis read, hashed as they were read. A file that has
    // changed since is recorded with no time, so the next check reads it and builds again.
    for file in analysis.map.iter() {
        if file.name().starts_with('<') {
            continue;
        }
        let path = cwd.join(file.path());
        let read = fnv1a(file.text().as_bytes());
        let mut input = Input::of(&path)?;
        if input.hash != read {
            input.hash = read;
            input.modified = 0;
        }
        inputs.insert(path, input);
    }

    let mut text = format!("{FORMAT}\n{key}end key\n");
    text.push_str(&format!("cxx {}\t{cxx}\n", tool_line(cxx)?));
    let exe = cwd.join(exe);
    text.push_str(&format!("exe {} {}\n", file_line(&exe)?, exe.display()));
    // Absolute, so a build run from another folder checks the same files. Expanded again
    // here, it must find exactly what loading found: a file added while the build ran, or a
    // path the expansion spells differently, would make the check disagree, so then there is
    // no stamp and the next build simply runs.
    for (pattern, found) in ws.globs() {
        let pattern = cwd.join(pattern).to_string_lossy().replace('\\', "/");
        let found: Vec<PathBuf> = found.iter().map(|p| cwd.join(p)).collect();
        if dz_project::glob_entries(&pattern).ok()? != found {
            return None;
        }
        text.push_str(&format!("glob {:016x} {pattern}\n", list_hash(&found)));
    }
    for (_, lib) in ws.cpp_libs() {
        if let Some(cmake) = &lib.cmake {
            let dir = cwd.join(&cmake.source);
            text.push_str(&format!("tree {:016x} {}\n", tree_signature(&dir), dir.display()));
        }
    }
    for input in inputs.values() {
        text.push_str("file ");
        text.push_str(&input.line());
    }
    text.push_str("warnings\n");
    text.push_str(warnings);
    crate::shared::write(&stamp_path(build_root, key), text.as_bytes()).ok()
}

/// `path` from the current folder, as given if it is already absolute.
fn absolute(path: &Path) -> PathBuf {
    match std::env::current_dir() {
        Ok(cwd) if path.is_relative() => cwd.join(path),
        _ => path.to_path_buf(),
    }
}

/// `path`'s size and time, as the stamp records an executable or a tool.
fn file_line(path: &Path) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    Some(format!("{} {}", meta.len(), meta.modified().ok().and_then(nanos)?))
}

/// The file the compiler `name` is, found as running it would find it, links followed, with
/// its size and time: an upgraded compiler is another file, or the same one changed.
fn tool_line(name: &str) -> Option<String> {
    let path = which(name)?;
    let real = std::fs::canonicalize(&path).unwrap_or(path);
    Some(format!("{} {}", file_line(&real)?, real.display()))
}

/// Where `name` is: itself if it names a folder, otherwise the first match on the `PATH`.
pub(crate) fn which(name: &str) -> Option<PathBuf> {
    let direct = Path::new(name);
    if direct.components().count() > 1 {
        return direct.is_file().then(|| direct.to_path_buf());
    }
    let exts: &[&str] = if cfg!(windows) { &["", ".exe", ".cmd", ".bat"] } else { &[""] };
    std::env::split_paths(&std::env::var_os("PATH")?).find_map(|dir| {
        exts.iter().map(|ext| dir.join(format!("{name}{ext}"))).find(|p| p.is_file())
    })
}

/// A hash of a list of paths, in order.
fn list_hash(paths: &[PathBuf]) -> u64 {
    let mut all = Vec::new();
    for p in paths {
        all.extend_from_slice(p.to_string_lossy().as_bytes());
        all.push(0);
    }
    fnv1a(&all)
}

/// Every file under `dir` by path, size and time, in a fixed order; `.git` is passed over,
/// as the CMake build's own check does (`native::tree_hash`). Reading no contents keeps it
/// to a few milliseconds for a tree like SDL's.
fn tree_signature(dir: &Path) -> u64 {
    let mut files = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(d) = pending.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            match e.file_type() {
                Ok(t) if t.is_dir() && e.file_name() != ".git" => pending.push(e.path()),
                Ok(t) if t.is_file() => {
                    let meta = e.metadata().ok();
                    let time = meta.as_ref().and_then(|m| m.modified().ok()).and_then(nanos);
                    files.push((e.path(), meta.map_or(0, |m| m.len()), time.unwrap_or(0)));
                }
                _ => {}
            }
        }
    }
    files.sort();
    let mut all = Vec::new();
    for (path, len, time) in files {
        all.extend_from_slice(path.to_string_lossy().as_bytes());
        all.push(0);
        all.extend_from_slice(&len.to_le_bytes());
        all.extend_from_slice(&time.to_le_bytes());
    }
    fnv1a(&all)
}

/// `n` space-separated fields, then a path that may hold spaces: the fields and the path.
fn split_path(line: &str, n: usize) -> Option<(String, PathBuf)> {
    let mut parts = line.splitn(n + 1, ' ');
    let fields: Vec<&str> = parts.by_ref().take(n).collect();
    let path = parts.next()?;
    (fields.len() == n).then(|| (fields.join(" "), PathBuf::from(path)))
}

/// A path that may hold spaces, then `n` space-separated fields: the link stamp's lines.
fn split_path_back(line: &str, n: usize) -> Option<(&str, Vec<&str>)> {
    let mut parts = line.rsplitn(n + 1, ' ');
    let mut fields: Vec<&str> = parts.by_ref().take(n).collect();
    fields.reverse();
    Some((parts.next()?, fields))
}
