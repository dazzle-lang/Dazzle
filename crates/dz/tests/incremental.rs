//! An incremental build equals a clean one (spec §16.6, T-0193).
//!
//! The safety net for building only what an edit can affect: for each scenario below, a
//! project is built, one scripted edit is applied, and it is built again in place; then the
//! edited sources are copied to a folder of their own and built from clean. The generated
//! C++ in `build/cpp` must be the same byte for byte (once each folder's own path is
//! replaced, since `#line` names sources by their full path), and the two programs must
//! print the same thing.
//!
//! Today's compiler analyses the whole program on every build, so this passes trivially.
//! It is written first so that each step that starts reusing earlier work (a build stamp, a
//! module's generated files kept when an edit cannot reach it) has to keep it passing. The
//! edits are the ones such reuse is most likely to get wrong: a change in one module that
//! shows up in another's generated code, through a signature, a constant, a default, a
//! generic or `Drop` body, or the reflection table.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

#[path = "../../dz_driver/tests/parallel/mod.rs"]
mod parallel;

fn dz() -> &'static str {
    env!("CARGO_BIN_EXE_dz")
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// One change to a project, by path relative to its root.
enum Edit {
    /// Replace text that occurs exactly once in the file, so an edit that no longer matches
    /// the generator's output fails loudly instead of testing nothing.
    Replace(&'static str, &'static str, &'static str),
    /// Copy a directory in from the 11-module generation (a module added).
    AddModule(&'static str),
    /// Delete a file or a directory (a module removed).
    Remove(&'static str),
}

/// Where a scenario's project comes from.
#[derive(Clone, Copy)]
enum Project {
    /// `tools/bench_scale.py generate 10`: modules `m000` (under every other) to `m009`
    /// (under none), and the program `app` running them all.
    Scale,
    /// A project under `tests/run/`.
    Run(&'static str),
}

struct Scenario {
    name: &'static str,
    project: Project,
    /// Applied before the first build: what the edit then changes.
    setup: &'static [Edit],
    /// Applied between the first build and the incremental one.
    edit: &'static [Edit],
}

const M000: &str = "m000/src/m000.dz";
const M001: &str = "m001/src/m001.dz";
const M009: &str = "m009/src/m009.dz";
/// `m001` is the only module that calls into `m000`.
const M001_CALL: &str = "touch0(seed + 1)";
/// The end of `run1`, where a statement can be added to `m001`.
const M001_END: &str = "    return acc\n}";
/// Where `m009` builds a list, and the same with a call of a `List` method nothing else in
/// the program calls: every instance of `List` outside `std` gains it (T-0199).
const M009_SHELF: &str = "let mut shelf0 = Shelf9<Part9_0>{ items: List<Part9_0>() }";
const M009_SHELF_REVERSED: &str =
    "let mut shelf0 = Shelf9<Part9_0>{ items: List<Part9_0>() }\n    shelf0.items.reverse()";

use Edit::*;
use Project::*;

const SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "leaf_body",
        project: Scale,
        setup: &[],
        edit: &[Replace(M009, "return (p.score() + 0) % 1000003", "return (p.score() + 1) % 1000003")],
    },
    // A body edit in a leaf that makes the first call of a generic struct's method: every
    // module's header with an instance of it changes, though no interface did (T-0199).
    Scenario {
        name: "first_call_of_a_method",
        project: Scale,
        setup: &[],
        edit: &[Replace(M009, M009_SHELF, M009_SHELF_REVERSED)],
    },
    // And the last call gone: the method leaves every instance again.
    Scenario {
        name: "last_call_of_a_method",
        project: Scale,
        setup: &[Replace(M009, M009_SHELF, M009_SHELF_REVERSED)],
        edit: &[Replace(M009, M009_SHELF_REVERSED, M009_SHELF)],
    },
    Scenario {
        name: "root_body",
        project: Scale,
        setup: &[],
        edit: &[Replace(M000, "return (p.score() + 0) % 1000003", "return (p.score() + 1) % 1000003")],
    },
    // A body edit to a module every other depends on, which leaves its interface as it was
    // (T-0198): the modules above keep their C++.
    Scenario {
        name: "root_constructor_body",
        project: Scale,
        setup: &[],
        edit: &[Replace(M000, "label = format!(\"p0_0_{}\", id)", "label = format!(\"q0_0_{}\", id)")],
    },
    // A body that grows a line moves everything after it, and `pick0<T>`, a generic only
    // `m001` instantiates, is further down: its instance is in `m001`'s C++ and names its
    // lines in `#line`s, so `m001` must be written again (T-0198).
    Scenario {
        name: "root_body_grows_above_a_generic",
        project: Scale,
        setup: &[
            Replace(
                M000,
                "pub fn touch0(",
                "pub fn pick0<T: Copy>(x: T) -> T {\n    let y = x\n    return y\n}\n\npub fn touch0(",
            ),
            Replace(M001, M001_CALL, "touch0(pick0(seed + 1))"),
        ],
        edit: &[Replace(
            M000,
            "weight = (id * 3 + 1) % 1009",
            "weight = (id * 3 + 1) % 1009\n        weight = weight % 1009",
        )],
    },
    Scenario {
        name: "root_signature",
        project: Scale,
        setup: &[],
        edit: &[
            Replace(M000, "pub fn touch0(x: i64) -> i64", "pub fn touch0(x: i64, k: i64) -> i64"),
            Replace(M000, "return (p.score() + 0) % 1000003", "return (p.score() + k) % 1000003"),
            Replace(M001, M001_CALL, "touch0(seed + 1, 2)"),
        ],
    },
    Scenario {
        name: "module_added",
        project: Scale,
        setup: &[],
        edit: &[
            AddModule("m010"),
            Replace(
                "dazzle.toml",
                "m009 = { path = \"m009\", open = true }\n",
                "m009 = { path = \"m009\", open = true }\nm010 = { path = \"m010\", open = true }\n",
            ),
            Replace(
                "src/main.dz",
                "    println!(",
                "    total = (total + run10(7)) % 1000003\n    println!(",
            ),
        ],
    },
    Scenario {
        name: "module_removed",
        project: Scale,
        setup: &[],
        edit: &[
            Remove("m009"),
            Replace("dazzle.toml", "m009 = { path = \"m009\", open = true }\n", ""),
            Replace("src/main.dz", "    total = (total + run9(7)) % 1000003\n", ""),
        ],
    },
    Scenario {
        name: "constant",
        project: Scale,
        setup: &[
            Replace(M000, "pub fn touch0(", "pub const BIAS0: i64 = 3\n\npub fn touch0("),
            Replace(M001, M001_CALL, "touch0(seed + BIAS0)"),
        ],
        edit: &[Replace(M000, "BIAS0: i64 = 3", "BIAS0: i64 = 4")],
    },
    Scenario {
        name: "default_argument",
        project: Scale,
        setup: &[
            Replace(M000, "pub fn touch0(x: i64) -> i64", "pub fn touch0(x: i64, k: i64 = 0) -> i64"),
            Replace(M000, "return (p.score() + 0) % 1000003", "return (p.score() + k) % 1000003"),
        ],
        edit: &[Replace(M000, "k: i64 = 0", "k: i64 = 5")],
    },
    Scenario {
        name: "field_default",
        project: Scale,
        setup: &[
            Replace(M000, "pub fn touch0(", "pub struct Knob0 {\n    pub v: i64 = 1\n}\n\npub fn touch0("),
            Replace(M001, M001_CALL, "touch0(seed + Knob0{}.v)"),
        ],
        edit: &[Replace(M000, "pub v: i64 = 1", "pub v: i64 = 2")],
    },
    Scenario {
        name: "drop_body",
        project: Scale,
        setup: &[
            Replace(
                M000,
                "pub fn touch0(",
                "pub struct Bell0 {\n    pub n: i64 = 0\n\n    pub fn ring(&self) -> i64 {\n        return n\n    }\n}\n\nimpl Drop for Bell0 {\n    fn drop(&mut self) {\n        println!(\"bell {}\", n)\n    }\n}\n\npub fn touch0(",
            ),
            Replace(M001, M001_END, "    acc = (acc + Bell0{ n: 1 }.ring()) % 1000003\n    return acc\n}"),
        ],
        edit: &[Replace(M000, "println!(\"bell {}\", n)", "println!(\"bell {} rung\", n)")],
    },
    Scenario {
        name: "reflected_type",
        project: Scale,
        setup: &[
            Replace(M000, "pub fn touch0(", "pub struct Tag0 {\n    pub a: i64 = 1\n}\n\npub fn touch0("),
            Replace(
                M001,
                M001_END,
                "    acc = (acc + typeinfo_of<Tag0>().fields.len as i64) % 1000003\n    return acc\n}",
            ),
        ],
        edit: &[Replace(M000, "    pub a: i64 = 1\n", "    pub a: i64 = 1\n    pub b: i64 = 2\n")],
    },
    // The same kinds of edit in hand-written projects, where the edited module is a library
    // the program depends on.
    Scenario {
        name: "generic_body",
        project: Run("generic_methods"),
        setup: &[],
        edit: &[Replace("shapes/src/grid.dz", "while i < n {", "while i < n + 1 {")],
    },
    Scenario {
        name: "dependency_field_default",
        project: Run("dynamic"),
        setup: &[],
        edit: &[Replace("libs/shapes/src/dot.dz", "pub r: f32 = 0.5", "pub r: f32 = 0.25")],
    },
    Scenario {
        name: "reflected_field_added",
        project: Run("reflect"),
        setup: &[],
        edit: &[Replace(
            "src/main.dz",
            "    pub agility:  f32 = 0 `json:\"agi\"`\n",
            "    pub agility:  f32 = 0 `json:\"agi\"`\n    pub luck:     i32 = 2\n",
        )],
    },
];

/// The C++ compiler every build here uses: `DZ_CXX` when set (CI picks one per job), or the
/// first one on the `PATH`. Named explicitly, so both builds of a scenario use the same one.
fn compiler() -> Option<String> {
    if let Ok(cxx) = std::env::var("DZ_CXX") {
        if !cxx.is_empty() {
            return Some(cxx);
        }
    }
    ["clang++", "g++", "c++"]
        .into_iter()
        .find(|c| matches!(Command::new(c).arg("--version").output(), Ok(o) if o.status.success()))
        .map(str::to_owned)
}

/// `tools/bench_scale.py generate`, or `None` without a Python to run it.
fn generate(modules: usize, into: &Path) -> Option<PathBuf> {
    let script = repo_root().join("tools/bench_scale.py");
    for python in ["python3", "python"] {
        let Ok(out) = Command::new(python)
            .arg(&script)
            .args(["generate", &modules.to_string()])
            .arg(into)
            .output()
        else {
            continue;
        };
        assert!(
            out.status.success(),
            "bench_scale.py generate failed\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
        return Some(into.join("dazzle"));
    }
    None
}

/// Copy a project, leaving out its `build/`.
fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let p = e.path();
        if p.file_name().is_some_and(|n| n == "build") {
            continue;
        }
        if p.is_dir() {
            copy_dir(&p, &to.join(e.file_name()));
        } else {
            std::fs::copy(&p, to.join(e.file_name())).unwrap();
        }
    }
}

fn apply(edits: &[Edit], project: &Path, scale11: &Path) -> Result<(), String> {
    for edit in edits {
        match edit {
            Replace(file, from, to) => {
                let path = project.join(file);
                let text = std::fs::read_to_string(&path).map_err(|e| format!("{file}: {e}"))?;
                // Git may check a hand-written fixture out with CRLF; the edits are in LF.
                let text = text.replace("\r\n", "\n");
                let found = text.matches(from).count();
                if found != 1 {
                    return Err(format!("{file}: {from:?} occurs {found} times, not once"));
                }
                std::fs::write(&path, text.replace(from, to)).unwrap();
            }
            AddModule(dir) => copy_dir(&scale11.join(dir), &project.join(dir)),
            Remove(path) => {
                let path = project.join(path);
                if path.is_dir() {
                    std::fs::remove_dir_all(path).unwrap();
                } else {
                    std::fs::remove_file(path).unwrap();
                }
            }
        }
    }
    Ok(())
}

/// One `dz` command on a project, with every variable that changes its output set here
/// rather than inherited. `DZ_INCREMENTAL` is what later stages read; today's compiler
/// ignores it.
fn dz_cmd(stage: &str, project: &Path, cxx: &str, incremental: &str) -> std::process::Output {
    Command::new(dz())
        .arg(stage)
        .arg(project)
        .env("DZ_CXX", cxx)
        .env("DZ_INCREMENTAL", incremental)
        .env_remove("DZ_CXXFLAGS")
        .env_remove("DZ_REGIONS")
        .output()
        .expect("failed to run dz")
}

/// Every file under `build/cpp`, by its path there, with the project's own path replaced.
fn generated(project: &Path) -> BTreeMap<String, String> {
    // Every spelling a path can take in the output: as given, through links resolved
    // (macOS's temporary directory), and with forward slashes (Windows). Longest first, so
    // a shorter spelling never replaces part of a longer one.
    let mut spellings = Vec::new();
    let canonical = std::fs::canonicalize(project).unwrap_or_else(|_| project.to_owned());
    for p in [project.to_owned(), canonical] {
        let s = p.to_string_lossy().trim_start_matches(r"\\?\").to_string();
        spellings.push(s.replace('\\', "/"));
        spellings.push(s);
    }
    spellings.sort_by_key(|s| std::cmp::Reverse(s.len()));
    spellings.dedup();

    let mut out = BTreeMap::new();
    let root = project.join("build/cpp");
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
                continue;
            }
            let bytes = std::fs::read(&p).unwrap();
            let mut text = String::from_utf8_lossy(&bytes).into_owned();
            for s in &spellings {
                text = text.replace(s.as_str(), "<project>");
            }
            let name = p.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
            out.insert(name, text);
        }
    }
    out
}

/// Where two generated trees differ, for a human: the files only one has, and the first
/// differing line of each file both have.
fn differences(inc: &BTreeMap<String, String>, clean: &BTreeMap<String, String>) -> Vec<String> {
    let mut out = Vec::new();
    for name in inc.keys().filter(|k| !clean.contains_key(*k)) {
        out.push(format!("{name}: only in the incremental build"));
    }
    for name in clean.keys().filter(|k| !inc.contains_key(*k)) {
        out.push(format!("{name}: only in the clean build"));
    }
    for (name, a) in inc {
        let Some(b) = clean.get(name) else { continue };
        if a == b {
            continue;
        }
        let line = a.lines().zip(b.lines()).position(|(x, y)| x != y);
        let line = line.unwrap_or_else(|| a.lines().count().min(b.lines().count()));
        out.push(format!(
            "{name}:{}: incremental {:?}, clean {:?}",
            line + 1,
            a.lines().nth(line).unwrap_or("<end>"),
            b.lines().nth(line).unwrap_or("<end>")
        ));
    }
    out
}

fn verify(
    s: &Scenario,
    root: &Path,
    scale: &Path,
    scale11: &Path,
    cxx: &str,
) -> Result<(), String> {
    let name = s.name;
    let inc = root.join(name).join("inc");
    let clean = root.join(name).join("clean");
    match s.project {
        Scale => copy_dir(scale, &inc),
        Run(fixture) => copy_dir(&repo_root().join("tests/run").join(fixture), &inc),
    }
    apply(s.setup, &inc, scale11)?;

    let first = dz_cmd("build", &inc, cxx, "verify");
    if !first.status.success() {
        return Err(format!("the first build failed\n{}", String::from_utf8_lossy(&first.stderr)));
    }

    let before = generated(&inc);
    if before.is_empty() {
        return Err("the first build generated no C++ in build/cpp".into());
    }

    apply(s.edit, &inc, scale11)?;
    let again = dz_cmd("run", &inc, cxx, "verify");
    copy_dir(&inc, &clean);
    let fresh = dz_cmd("run", &clean, cxx, "0");
    if !fresh.status.success() {
        return Err(format!(
            "the clean build of the edited project failed\n{}",
            String::from_utf8_lossy(&fresh.stderr)
        ));
    }
    if !again.status.success() {
        return Err(format!(
            "the incremental build failed where a clean one succeeded\n{}",
            String::from_utf8_lossy(&again.stderr)
        ));
    }

    // A removed module's files included: the build deletes them (T-0194).
    let after = generated(&inc);
    let fresh_cpp = generated(&clean);
    let diffs = differences(&after, &fresh_cpp);
    if !diffs.is_empty() {
        return Err(format!("the generated C++ differs:\n  {}", diffs.join("\n  ")));
    }
    // An edit that changes no generated C++ would pass however stale a build was.
    if before == fresh_cpp {
        return Err("the edit changed no generated C++, so this scenario tests nothing".into());
    }
    let printed =
        |o: &std::process::Output| String::from_utf8_lossy(&o.stdout).replace("\r\n", "\n");
    if printed(&again) != printed(&fresh) {
        return Err(format!(
            "the programs print different things\nincremental:\n{}\nclean:\n{}",
            printed(&again),
            printed(&fresh)
        ));
    }
    Ok(())
}

#[test]
fn an_incremental_build_equals_a_clean_one() {
    let Some(cxx) = compiler() else {
        eprintln!("skipping: no C++ compiler available");
        return;
    };
    // Per process: another `cargo test` on the same machine must not build in this folder.
    let root = std::env::temp_dir().join(format!("dz_incremental_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();

    // The 11-module project is where an added module comes from: its `m010` depends on
    // modules the 10-module one already has.
    let scale = generate(10, &root.join("scale10"));
    let scale11 = generate(11, &root.join("scale11"));
    let (scale, scale11) = match (scale, scale11) {
        (Some(a), Some(b)) => (a, b),
        _ => {
            eprintln!("no Python to run tools/bench_scale.py: only the tests/run scenarios run");
            (PathBuf::new(), PathBuf::new())
        }
    };
    let runnable: Vec<&Scenario> = SCENARIOS
        .iter()
        .filter(|s| matches!(s.project, Run(_)) || !scale.as_os_str().is_empty())
        .collect();

    let mut failures = parallel::filter_map(&runnable, |s| {
        verify(s, &root, &scale, &scale11, &cxx).err().map(|e| format!("{}: {e}", s.name))
    });
    failures.sort();
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
    let _ = std::fs::remove_dir_all(&root);
}
