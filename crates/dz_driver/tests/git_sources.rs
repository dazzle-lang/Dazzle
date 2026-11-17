//! Sources fetched with git (spec §3.2, §15.1; T-0077): a Dazzle module and a C++ recipe,
//! each from a repository named by URL and pinned by `version` or `rev`, the commit
//! recorded in `dazzle.lock`, and each commit checked out once into the per-user cache.
//!
//! The repositories are made here, in a temporary folder, and named by path, which git
//! fetches like any URL: the test needs no network. It needs `git`, and the header case
//! libclang; without either it says so and passes, as the other tests of the machine do.

use std::path::{Path, PathBuf};
use std::process::Command;

use dz_diag::{codes, Severity};

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"])
        .args(args)
        .output()
        .expect("git runs");
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

fn have_git() -> bool {
    Command::new("git").arg("--version").output().is_ok_and(|o| o.status.success())
}

/// A folder of its own for one test, with the cache inside it so nothing outside is
/// touched, and every `dz_driver::analyse` in it sees the same cache.
struct Sandbox {
    dir: PathBuf,
}

impl Sandbox {
    fn new(tag: &str) -> Sandbox {
        let dir = std::env::temp_dir().join(format!("dz_git_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Sandbox { dir }
    }

    /// A repository at `name` holding `files`, committed and tagged `tag`: its commit.
    fn repo(&self, name: &str, files: &[(&str, &str)], tag: &str) -> String {
        let repo = self.dir.join(name);
        std::fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-q"]);
        for (path, text) in files {
            let p = repo.join(path);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        }
        git(&repo, &["add", "-A"]);
        git(&repo, &["commit", "-q", "-m", "release"]);
        git(&repo, &["tag", "-a", tag, "-m", tag]);
        git(&repo, &["rev-parse", "HEAD"])
    }

    fn url(&self, name: &str) -> String {
        self.dir.join(name).to_string_lossy().replace('\\', "/")
    }

    fn app(&self, deps: &str, extra: &str, source: &str) -> PathBuf {
        let app = self.dir.join("app");
        std::fs::create_dir_all(app.join("src")).unwrap();
        std::fs::write(
            app.join("dazzle.toml"),
            format!(
                "[module]\nname = \"app\"\nsources = [\"src/**/*.dz\"]\n\n[dependencies]\n\
                 std = {{ open = true }}\n{deps}\n{extra}\n[build]\noutput = \"executable\"\n"
            ),
        )
        .unwrap();
        std::fs::write(app.join("src/main.dz"), source).unwrap();
        app
    }

    fn analyse(&self, app: &Path, mode: dz_driver::Mode) -> dz_driver::Analysis {
        // The cache is the sandbox's: `DZ_CACHE_DIR` is read when the loader starts.
        std::env::set_var("DZ_CACHE_DIR", self.dir.join("cache"));
        dz_driver::analyse_with(app, &|_| None, mode)
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn errors(a: &dz_driver::Analysis) -> Vec<&'static str> {
    a.diags.iter().filter(|d| d.severity == Severity::Error).filter_map(|d| d.code).collect()
}

const LIB_MANIFEST: &str = "[module]\nname = \"greet\"\nsources = [\"src/**/*.dz\"]\n\n\
                            [dependencies]\nstd = { open = true }\n\n[build]\noutput = \"static-lib\"\n";

// One test, since `DZ_CACHE_DIR` is the process's: tests in one binary run on threads at
// once, and two sandboxes setting it would race.
#[test]
fn git_sources() {
    if !have_git() {
        eprintln!("skipping: git is not available");
        return;
    }
    a_dazzle_module_is_fetched_by_version_and_pinned();
    a_rev_is_a_commit_and_a_branch_is_refused();
    the_editor_never_fetches();
    a_recipe_is_fetched_and_its_root_is_the_checkout();
    a_missing_version_is_one_error_at_the_url();
}

fn a_dazzle_module_is_fetched_by_version_and_pinned() {
    let s = Sandbox::new("module");
    let commit = s.repo(
        "greet",
        &[
            ("dazzle.toml", LIB_MANIFEST),
            ("src/lib.dz", "pub fn hello() -> i32 {\n    return 42\n}\n"),
        ],
        "v1.2.0",
    );
    let deps = format!("greet = {{ git = \"{}\", version = \"1.2.0\" }}", s.url("greet"));
    let app = s.app(&deps, "", "fn main() {\n    println!(\"{}\", greet::hello())\n}\n");
    let a = s.analyse(&app, dz_driver::Mode::Build);
    let rendered = dz_diag::render_to_string(&a.map, &a.diags);
    assert!(!a.diags.has_errors(), "{rendered}");
    assert!(a.hir.is_some(), "{rendered}");

    // The lockfile pins the commit the tag named, resolved through the annotated tag.
    let lock = std::fs::read_to_string(app.join("dazzle.lock")).expect("a lockfile");
    assert!(lock.contains(&commit), "{lock}");
    assert!(lock.contains("version = \"1.2.0\""), "{lock}");

    // A new commit tagged again upstream changes nothing: the lockfile holds the old one.
    let repo = s.dir.join("greet");
    std::fs::write(repo.join("src/lib.dz"), "pub fn hello() -> i32 {\n    return 7\n}\n").unwrap();
    git(&repo, &["commit", "-q", "-am", "moved"]);
    git(&repo, &["tag", "-f", "-a", "v1.2.0", "-m", "moved"]);
    let again = s.analyse(&app, dz_driver::Mode::Build);
    assert!(!again.diags.has_errors());
    assert_eq!(std::fs::read_to_string(app.join("dazzle.lock")).unwrap(), lock);
}

fn a_rev_is_a_commit_and_a_branch_is_refused() {
    let s = Sandbox::new("rev");
    let commit = s.repo(
        "greet",
        &[
            ("dazzle.toml", LIB_MANIFEST),
            ("src/lib.dz", "pub fn hello() -> i32 {\n    return 1\n}\n"),
        ],
        "v0.1.0",
    );
    let source = "fn main() {\n    println!(\"{}\", greet::hello())\n}\n";
    let deps = format!("greet = {{ git = \"{}\", rev = \"{commit}\" }}", s.url("greet"));
    let app = s.app(&deps, "", source);
    let a = s.analyse(&app, dz_driver::Mode::Build);
    let rendered = dz_diag::render_to_string(&a.map, &a.diags);
    assert!(!a.diags.has_errors(), "{rendered}");

    let deps = format!("greet = {{ git = \"{}\", rev = \"master\" }}", s.url("greet"));
    let app = s.app(&deps, "", source);
    let a = s.analyse(&app, dz_driver::Mode::Build);
    let rendered = dz_diag::render_to_string(&a.map, &a.diags);
    assert_eq!(errors(&a), vec![codes::MANIFEST_BAD_VALUE], "{rendered}");
    assert!(rendered.contains("is not a commit"), "{rendered}");

    let deps = format!("greet = {{ git = \"{}\" }}", s.url("greet"));
    let app = s.app(&deps, "", source);
    let a = s.analyse(&app, dz_driver::Mode::Build);
    assert_eq!(errors(&a), vec![codes::MANIFEST_MISSING_FIELD]);
}

fn the_editor_never_fetches() {
    let s = Sandbox::new("editor");
    s.repo(
        "greet",
        &[
            ("dazzle.toml", LIB_MANIFEST),
            ("src/lib.dz", "pub fn hello() -> i32 {\n    return 3\n}\n"),
        ],
        "v2.0.0",
    );
    let deps = format!("greet = {{ git = \"{}\", version = \"2.0.0\" }}", s.url("greet"));
    let app = s.app(&deps, "", "fn main() {\n    println!(\"{}\", greet::hello())\n}\n");
    let a = s.analyse(&app, dz_driver::Mode::Editor);
    let rendered = dz_diag::render_to_string(&a.map, &a.diags);
    assert!(errors(&a).contains(&codes::GIT_NOT_FETCHED), "{rendered}");
    assert!(!app.join("dazzle.lock").exists(), "the editor wrote the lockfile");
    assert!(!s.dir.join("cache/git").exists(), "the editor fetched");

    // Once a build has fetched it, the editor reads the checkout.
    let built = s.analyse(&app, dz_driver::Mode::Build);
    assert!(!built.diags.has_errors(), "{}", dz_diag::render_to_string(&built.map, &built.diags));
    let a = s.analyse(&app, dz_driver::Mode::Editor);
    let rendered = dz_diag::render_to_string(&a.map, &a.diags);
    assert!(!a.diags.has_errors(), "{rendered}");
}

fn a_recipe_is_fetched_and_its_root_is_the_checkout() {
    let s = Sandbox::new("recipe");
    s.repo(
        "tiny",
        &[(
            "include/tiny.h",
            "#pragma once\nstatic inline int tiny_add(int a, int b) { return a + b; }\n",
        )],
        "v0.3.0",
    );
    let deps = "tiny = { cpp = \"tiny\" }";
    let recipe = format!(
        "[cpp.libs.tiny]\ngit = \"{}\"\nversion = \"0.3.0\"\nroot = \"include\"\n\
         include_dirs = [\".\"]\nimport = [\"tiny.h\"]\nkeeps = []\n",
        s.url("tiny")
    );
    let app = s.app(deps, &recipe, "fn main() {\n    println!(\"{}\", tiny::tiny_add(1, 2))\n}\n");
    let a = s.analyse(&app, dz_driver::Mode::Build);
    if a.diags.iter().any(|d| d.code == Some(codes::CPP_UNAVAILABLE)) {
        eprintln!("skipping the recipe: libclang is not available");
        return;
    }
    let rendered = dz_diag::render_to_string(&a.map, &a.diags);
    assert!(!a.diags.has_errors(), "{rendered}");
    let ws = a.ws.as_ref().unwrap();
    let lib = ws.modules().flat_map(|m| &m.cpp.libs).find(|l| l.name == "tiny").unwrap();
    let cache = std::fs::canonicalize(s.dir.join("cache")).unwrap();
    let include =
        lib.include_dirs.iter().map(|d| std::fs::canonicalize(d).unwrap()).next().unwrap();
    assert!(include.starts_with(&cache), "{include:?} is not in the cache");
}

fn a_missing_version_is_one_error_at_the_url() {
    let s = Sandbox::new("missing");
    s.repo(
        "greet",
        &[("dazzle.toml", LIB_MANIFEST), ("src/lib.dz", "pub fn hello() {}\n")],
        "v1.0.0",
    );
    let deps = format!("greet = {{ git = \"{}\", version = \"9.9.9\" }}", s.url("greet"));
    let app = s.app(&deps, "", "fn main() {\n    greet::hello()\n}\n");
    let a = s.analyse(&app, dz_driver::Mode::Build);
    let rendered = dz_diag::render_to_string(&a.map, &a.diags);
    assert_eq!(errors(&a), vec![codes::GIT_FETCH_FAILED], "{rendered}");
    assert!(rendered.contains("no tag v9.9.9 or 9.9.9"), "{rendered}");
    assert!(!app.join("dazzle.lock").exists());
}
