//! The build stamp (T-0192): a build with nothing changed skips straight to the executable,
//! and every kind of input a build reads makes the next one run again when it changes.
//!
//! Each test builds a small project in a folder of its own, then builds it again after one
//! change, and asks `--timings` which happened: a skipped build reports one phase, `up to
//! date`. The `dz` identity is pinned through `DZ_STAMP_IDENTITY`, so a test never depends on
//! when this `dz` was built, and every variable the stamp keys on is set or cleared here,
//! never inherited from the machine.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const MANIFEST: &str = "[module]\nname = \"stamped\"\nsources = [\"src/**/*.dz\"]\n\n\
                        [dependencies]\nstd = { open = true }\n\n[build]\noutput = \"executable\"\n";

/// A program with one warning, `unused`, so a skipped build can be seen to print it again.
const MAIN: &str = "fn main() {\n    let unused = 1\n    println!(\"one\")\n}\n";

fn project(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("dz_stamp_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(dir.join("dazzle.toml"), MANIFEST).unwrap();
    std::fs::write(dir.join("src/main.dz"), MAIN).unwrap();
    dir
}

/// `dz <args> --timings` in `dir`, with the environment the stamp keys on under the test's
/// control: `env` sets variables, everything else in `KEYED` is cleared.
fn dz(dir: &Path, args: &[&str], env: &[(&str, &str)]) -> Output {
    const KEYED: &[&str] = &["DZ_REGIONS", "DZ_CXXFLAGS", "DZ_FUZZ_ALLOW", "DZ_STAMP_IDENTITY"];
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_dz"));
    cmd.args(args).arg("--timings").current_dir(dir);
    for name in KEYED {
        cmd.env_remove(name);
    }
    // One cache for every test here: what it holds is the same for all of them.
    cmd.env("DZ_CACHE_DIR", std::env::temp_dir().join("dz_stamp_cache"));
    cmd.env("DZ_STAMP_IDENTITY", "test");
    for (k, v) in env {
        cmd.env(k, v);
    }
    let out = cmd.output().expect("failed to run dz");
    assert!(out.status.success(), "dz {args:?} failed:\n{}", String::from_utf8_lossy(&out.stderr));
    out
}

fn skipped(out: &Output) -> bool {
    String::from_utf8_lossy(&out.stderr).contains("up to date")
}

/// Build, and check a second build with nothing changed is skipped: the state each test
/// changes one thing from.
fn built(dir: &Path) {
    let first = dz(dir, &["build"], &[]);
    assert!(!skipped(&first), "a first build cannot be up to date");
    assert!(skipped(&dz(dir, &["build"], &[])), "nothing changed, so the build is skipped");
}

#[test]
fn nothing_changed_skips_the_build_and_prints_its_warnings_again() {
    let dir = project("unchanged");
    let first = dz(&dir, &["build"], &[]);
    let again = dz(&dir, &["build"], &[]);
    assert!(skipped(&again));
    let stderr = String::from_utf8_lossy(&again.stderr);
    assert!(stderr.contains("DZ0406"), "the warning is printed again:\n{stderr}");
    assert!(!stderr.contains("type check"), "nothing is analysed:\n{stderr}");
    // Named the way the build that made it named it.
    assert_eq!(first.stdout, again.stdout);
    // `dz run` runs it.
    let run = dz(&dir, &["run"], &[]);
    assert!(skipped(&run));
    assert_eq!(String::from_utf8_lossy(&run.stdout).trim(), "one");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_edited_source_builds_again() {
    let dir = project("edited");
    built(&dir);
    std::fs::write(dir.join("src/main.dz"), MAIN.replace("one", "two")).unwrap();
    let run = dz(&dir, &["run"], &[]);
    assert!(!skipped(&run));
    assert_eq!(String::from_utf8_lossy(&run.stdout).trim(), "two");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The same size and the same time as the stamp recorded: only the contents tell. A file
/// whose time was within 2 s of the stamp's when it was written is not trusted by its time,
/// since a file system's clock may tick that coarsely, and is read. Its time is set after
/// the build here, which no clock tick can settle.
#[test]
fn a_source_edited_to_the_same_size_and_time_builds_again() {
    let dir = project("same_size");
    let path = dir.join("src/main.dz");
    let time = std::time::SystemTime::now() + std::time::Duration::from_secs(60);
    std::fs::File::options().write(true).open(&path).unwrap().set_modified(time).unwrap();
    dz(&dir, &["build"], &[]);
    std::fs::write(&path, MAIN.replace("one", "eno")).unwrap();
    std::fs::File::options().write(true).open(&path).unwrap().set_modified(time).unwrap();
    let run = dz(&dir, &["run"], &[]);
    assert!(!skipped(&run));
    assert_eq!(String::from_utf8_lossy(&run.stdout).trim(), "eno");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_file_added_to_or_removed_from_the_sources_builds_again() {
    let dir = project("added");
    built(&dir);
    let extra = dir.join("src/extra.dz");
    std::fs::write(&extra, "fn helper() -> i32 {\n    return 1\n}\n").unwrap();
    assert!(!skipped(&dz(&dir, &["build"], &[])), "a file was added");
    assert!(skipped(&dz(&dir, &["build"], &[])));
    std::fs::remove_file(&extra).unwrap();
    assert!(!skipped(&dz(&dir, &["build"], &[])), "a file was removed");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_edited_manifest_builds_again() {
    let dir = project("manifest");
    built(&dir);
    std::fs::write(dir.join("dazzle.toml"), format!("{MANIFEST}# edited\n")).unwrap();
    assert!(!skipped(&dz(&dir, &["build"], &[])));
    let _ = std::fs::remove_dir_all(&dir);
}

/// Another option is another build with a stamp of its own, so switching back finds the
/// first still up to date. Both write the same executable, so each sees it changed and
/// builds again, which the link stamp makes cheap.
#[test]
fn an_option_or_a_variable_is_another_build() {
    let dir = project("options");
    built(&dir);
    assert!(!skipped(&dz(&dir, &["build", "--no-line-directives"], &[])));
    assert!(skipped(&dz(&dir, &["build", "--no-line-directives"], &[])));
    assert!(!skipped(&dz(&dir, &["build"], &[("DZ_REGIONS", "0")])));
    assert!(skipped(&dz(&dir, &["build"], &[("DZ_REGIONS", "0")])));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A program names its sources as the command line named the project (T-0188), so building
/// it from beside the project makes a different program, which the stamp must not skip.
#[test]
fn a_build_named_from_another_folder_is_another_build() {
    let dir = project("elsewhere");
    built(&dir);
    let parent = dir.parent().unwrap();
    let name = dir.file_name().unwrap().to_str().unwrap();
    assert!(!skipped(&dz(parent, &["build", name], &[])));
    assert!(skipped(&dz(parent, &["build", name], &[])));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A `dz` built again may generate other C++ from the same source.
#[test]
fn another_dz_builds_again() {
    let dir = project("identity");
    built(&dir);
    assert!(!skipped(&dz(&dir, &["build"], &[("DZ_STAMP_IDENTITY", "another dz")])));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_deleted_executable_builds_again() {
    let dir = project("deleted");
    built(&dir);
    std::fs::remove_dir_all(dir.join("build/bin")).unwrap();
    let run = dz(&dir, &["run"], &[]);
    assert!(!skipped(&run));
    assert_eq!(String::from_utf8_lossy(&run.stdout).trim(), "one");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A C header the program imports, and a library's own source, are inputs as much as a
/// `.dz` file is: `tests/run/cinterop_header` imports and compiles one single-header library.
#[test]
fn an_edited_c_header_builds_again() {
    let from = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/run/cinterop_header");
    let dir = std::env::temp_dir().join(format!("dz_stamp_header_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    copy_tree(&from, &dir);
    built(&dir);
    let header = dir.join("vendor/shl/shl.h");
    let text = std::fs::read_to_string(&header).unwrap();
    std::fs::write(&header, format!("{text}\n/* edited */\n")).unwrap();
    assert!(!skipped(&dz(&dir, &["build"], &[])));
    assert!(skipped(&dz(&dir, &["build"], &[])));
    let _ = std::fs::remove_dir_all(&dir);
}

/// `from` into `to`, without its `build/`.
fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let (src, dst) = (e.path(), to.join(e.file_name()));
        if e.file_type().unwrap().is_dir() {
            if e.file_name() != "build" {
                copy_tree(&src, &dst);
            }
        } else {
            std::fs::copy(&src, &dst).unwrap();
        }
    }
}
