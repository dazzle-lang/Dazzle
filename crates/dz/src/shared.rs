//! The files every program shares, generated and compiled once for all of them (T-0182).
//!
//! The runtime and `std` come out of `dz_cpp` the same for every program. A project's
//! `build/` keeps a copy of them, which its own headers include, but they are compiled, and
//! `std.h` precompiled, from a folder in a per-user cache. A new project, a build from clean
//! or `dz test`'s harness then starts with them already compiled, as a C++ program starts
//! with its standard library already built.
//!
//! The folder is named for what is in it: a hash of those files and of the `std` sources
//! their `#line`s name. Anything that changes them (another `dz`, `check_trusted`, a harness)
//! gets a folder of its own, so a stale one is never used. Several `dz` processes can use
//! one folder at once, as the test suite does, so nothing in it is written in place: each
//! file is written beside its final name and renamed onto it.

use std::path::{Path, PathBuf};

use crate::native::fnv1a;

/// How many folders the cache keeps, the most recently used. Each `dz` and each setting that
/// changes `std` makes one, so this bounds what working on the compiler leaves behind.
const KEEP: usize = 8;

/// A folder used more recently than this is kept however many there are: a build may be
/// using it. The test suite alone uses about ten at once (`--track-memory`, `DZ_REGIONS=0`,
/// `check_trusted`, each harness).
const IN_USE: std::time::Duration = std::time::Duration::from_secs(60 * 60);

/// One folder of the cache: `std-<hash>/`, holding `cpp/` (the shared files, which are
/// compiled from here), `src/` (the `std` sources their `#line`s name, so a debugger can
/// open them) and `obj/` (their objects and the precompiled `std.h`).
pub struct Shared {
    pub dir: PathBuf,
    /// The shared files, in `cpp/`.
    pub files: Vec<PathBuf>,
}

impl Shared {
    /// The folder for `files`, the shared ones `dz_cpp` emitted, as they are before their
    /// `#line`s are renamed. `sources` says whether those name `std`'s sources, which then
    /// go into the key too: the copies a debugger opens must be the ones compiled. `build` is
    /// where the cache goes if there is no per-user one.
    pub fn new<'a>(
        build: &Path,
        files: impl Iterator<Item = &'a dz_cpp::GeneratedFile>,
        sources: bool,
    ) -> Shared {
        Shared::in_cache(&root(build), files, sources)
    }

    /// `new`, with the cache at `root`.
    fn in_cache<'a>(
        root: &Path,
        files: impl Iterator<Item = &'a dz_cpp::GeneratedFile>,
        sources: bool,
    ) -> Shared {
        // Bumped if the folder's layout changes, so an old one is not taken for a new one.
        let mut key = b"dz shared 1\0".to_vec();
        let mut names = Vec::new();
        for file in files {
            for part in [&file.name, &file.contents] {
                key.extend_from_slice(part.as_bytes());
                key.push(0);
            }
            names.push(file.name.clone());
        }
        if sources {
            for (pseudo, text) in dz_project::embedded_sources() {
                for part in [pseudo, text] {
                    key.extend_from_slice(part.as_bytes());
                    key.push(0);
                }
            }
        }
        let dir = root.join(format!("std-{:016x}", fnv1a(&key)));
        let new = !dir.exists();
        let _ = std::fs::create_dir_all(dir.join("cpp"));
        let _ = std::fs::create_dir_all(dir.join("obj"));
        // Its time is when a build last used the folder, which pruning goes by.
        let _ = std::fs::write(dir.join("used"), b"");
        if new {
            prune(root, &dir);
        }
        let files = names.iter().map(|n| dir.join("cpp").join(n)).collect();
        Shared { dir, files }
    }

    pub fn obj_dir(&self) -> PathBuf {
        self.dir.join("obj")
    }

    /// The shared copy of `file`, one of the project's generated files, if it has one.
    pub fn copy_of(&self, file: &Path) -> Option<&PathBuf> {
        let name = file.file_name()?;
        self.files.iter().find(|f| f.file_name() == Some(name))
    }

    /// Whether `file` is one of the shared copies.
    pub fn holds(&self, file: &Path) -> bool {
        file.starts_with(self.dir.join("cpp"))
    }
}

/// The per-user cache (`dz_project::user_cache_dir`), or where there is none, the build's
/// own `obj/shared/`, which still serves every build of that project.
fn root(build: &Path) -> PathBuf {
    if let Some(dir) = dz_project::user_cache_dir() {
        return dir;
    }
    let dir = build.join("obj").join("shared");
    // Absolute, since the files in it are named in `#line`s and by objects' stamps.
    if dir.is_absolute() {
        dir
    } else {
        std::env::current_dir().map(|cwd| cwd.join(&dir)).unwrap_or(dir)
    }
}

/// Remove all but the `KEEP` most recently used folders, never `current` and never one used
/// within `IN_USE`.
fn prune(root: &Path, current: &Path) {
    let Ok(entries) = std::fs::read_dir(root) else { return };
    let mut folders: Vec<(std::time::SystemTime, PathBuf)> = entries
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("std-"))
        .map(|e| {
            // A folder just made has no `used` yet, but is as new as its own time.
            let path = e.path();
            let used = std::fs::metadata(path.join("used")).or_else(|_| std::fs::metadata(&path));
            (used.and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH), path)
        })
        .collect();
    folders.sort_by_key(|(used, _)| std::cmp::Reverse(*used));
    let now = std::time::SystemTime::now();
    for (used, folder) in folders.into_iter().skip(KEEP) {
        let recent = now.duration_since(used).map_or(true, |age| age < IN_USE);
        if folder != current && !recent {
            // Another process may still be using it; it is made again if so.
            let _ = std::fs::remove_dir_all(folder);
        }
    }
}

/// Where a file is written before it is renamed onto `path`: beside it, named for this
/// process, so two processes writing the same file never write the same bytes at once.
pub fn temp_of(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".{}.tmp", std::process::id()));
    path.with_file_name(name)
}

/// Write `contents` to `path`, through a file renamed onto it, unless it already holds
/// them. A process reading it sees the old file or the new one, never half of one.
pub fn write(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    if std::fs::read(path).is_ok_and(|old| old == contents) {
        return Ok(());
    }
    let temp = temp_of(path);
    std::fs::write(&temp, contents)?;
    match std::fs::rename(&temp, path) {
        Ok(()) => Ok(()),
        // Windows refuses to replace a file another process has open. That process put it
        // there, from the same contents, since a folder's name is its contents.
        Err(_) if std::fs::read(path).is_ok_and(|old| old == contents) => {
            let _ = std::fs::remove_file(&temp);
            Ok(())
        }
        Err(e) => {
            let _ = std::fs::remove_file(&temp);
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime};

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("dz_shared_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A folder whose `used` is `age` old.
    fn folder(root: &Path, name: &str, age: Duration) -> PathBuf {
        let dir = root.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        let used = std::fs::File::create(dir.join("used")).unwrap();
        used.set_modified(SystemTime::now() - age).unwrap();
        dir
    }

    #[test]
    fn pruning_keeps_the_most_recent_and_any_in_use() {
        let root = scratch("prune");
        let day = Duration::from_secs(24 * 60 * 60);
        // Ten old enough to go, the newest `KEEP` of which stay; and three in use, which stay
        // although they would make more than `KEEP`.
        let old: Vec<PathBuf> =
            (0..10).map(|i| folder(&root, &format!("std-old{i:02}"), day * (i + 1))).collect();
        let busy: Vec<PathBuf> =
            (0..3).map(|i| folder(&root, &format!("std-busy{i}"), Duration::ZERO)).collect();
        // Not the cache's: never touched, whatever its age.
        let other = folder(&root, "notes", day * 100);
        prune(&root, &busy[0]);
        for dir in busy.iter().chain(&old[..KEEP - 3]).chain([&other]) {
            assert!(dir.exists(), "{} was removed", dir.display());
        }
        for dir in &old[KEEP - 3..] {
            assert!(!dir.exists(), "{} was kept", dir.display());
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn write_replaces_a_file_whole_and_leaves_an_unchanged_one_alone() {
        let root = scratch("write");
        let path = root.join("std.h");
        write(&path, b"one").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"one");
        // Unchanged: not written again, so its time stays, as clang++'s precompiled headers
        // need of their inputs.
        let before = std::fs::metadata(&path).unwrap().modified().unwrap();
        std::thread::sleep(Duration::from_millis(20));
        write(&path, b"one").unwrap();
        assert_eq!(std::fs::metadata(&path).unwrap().modified().unwrap(), before);
        write(&path, b"two").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"two");
        // Nothing left beside it.
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_folder_is_named_for_its_contents() {
        let build = scratch("key");
        let file = |contents: &str| dz_cpp::GeneratedFile {
            name: "std.cpp".into(),
            contents: contents.into(),
            shared: true,
        };
        let cache = build.join("cache");
        let a = Shared::in_cache(&cache, [file("int x;")].iter(), false);
        let again = Shared::in_cache(&cache, [file("int x;")].iter(), false);
        let b = Shared::in_cache(&cache, [file("int y;")].iter(), false);
        assert!(a.dir.starts_with(&cache), "{}", a.dir.display());
        assert!(a.dir.join("used").exists());
        assert_eq!(a.dir, again.dir);
        assert_ne!(a.dir, b.dir);
        assert_eq!(a.files, [a.dir.join("cpp").join("std.cpp")]);
        assert_eq!(a.copy_of(Path::new("build/cpp/std.cpp")), Some(&a.files[0]));
        assert!(a.holds(&a.files[0]) && !a.holds(Path::new("build/cpp/std.cpp")));
        let _ = std::fs::remove_dir_all(&build);
    }
}
