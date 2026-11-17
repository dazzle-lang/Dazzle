//! Sources fetched with git (spec §3.2, §15.1; T-0077, D-0110): a dependency
//! `ui = { git = "…", version = "0.2.0" }` or a recipe `[cpp.libs.imgui] git = "…"`.
//!
//! A source is pinned to one commit by the **lockfile**, `dazzle.lock` beside the root
//! manifest, which is checked in: a `version` names a release tag and `rev` a commit, and
//! the lockfile records the commit each resolved to, so two machines build the same code
//! and a tag moved upstream changes nothing until the entry is deleted. The commit is
//! git's own hash of the whole tree, so it is the content hash too.
//!
//! Each commit is checked out once into the per-user cache, `<cache>/git/<repo>-<url
//! hash>/<commit>/`, and from there a fetched source is source: a dependency's
//! `dazzle.toml` is read from the checkout, a recipe's `root` is relative to it.
//!
//! Only `dz` itself fetches, by running the system's `git`. Analysis for the editor is
//! offline (`online: false`): it reads a checkout already in the cache and otherwise says
//! the source is not fetched yet, as it does for a CMake library not yet built.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use dz_diag::Diagnostics;
use dz_span::SourceMap;

use crate::toml::{self, Value};

/// What a git source is pinned by in the manifest.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Pin {
    /// `version = "1.93.0"`: a release, the tag `v1.93.0` or `1.93.0`.
    Version(String),
    /// `rev = "6e9f…"`: a commit, written in full.
    Rev(String),
}

impl Pin {
    fn key(&self) -> (&'static str, &str) {
        match self {
            Pin::Version(v) => ("version", v),
            Pin::Rev(r) => ("rev", r),
        }
    }

    /// As the manifest writes it: `version "1.93.0"`.
    pub fn describe(&self) -> String {
        let (k, v) = self.key();
        format!("{k} \"{v}\"")
    }
}

/// Whether `s` is a commit written in full: 40 hex digits, or 64 for a SHA-256 repository.
pub fn is_commit(s: &str) -> bool {
    (s.len() == 40 || s.len() == 64) && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Why a source has no checkout.
#[derive(Debug)]
pub enum FetchError {
    /// No per-user cache to keep checkouts in.
    NoCache,
    /// Offline (the editor), and the commit is not checked out yet, or not in the lockfile.
    NotFetched,
    /// `git` could not be run at all.
    NoGit(String),
    /// The remote has no tag for the version.
    NoSuchVersion { tried: Vec<String> },
    /// `git` ran and failed: what it said.
    Git(String),
}

/// The lockfile and the cache, for one load of a workspace.
pub struct Fetcher {
    /// `dazzle.lock` beside the root manifest.
    path: PathBuf,
    /// What the lockfile held when it was read: (url, pin) to commit.
    locked: BTreeMap<(String, Pin), String>,
    /// What this load used, which is what the lockfile will hold.
    used: BTreeMap<(String, Pin), String>,
    online: bool,
    cache: Option<PathBuf>,
}

impl Fetcher {
    /// The lockfile beside `root_manifest`, read if it exists. A lockfile that does not
    /// parse is reported and read as empty, so each source is resolved again.
    pub fn new(
        root_manifest: &Path,
        online: bool,
        map: &mut SourceMap,
        diags: &mut Diagnostics,
    ) -> Self {
        let path = root_manifest.with_file_name("dazzle.lock");
        let mut locked = BTreeMap::new();
        if let Ok(text) = std::fs::read_to_string(&path) {
            let file = map.add(path.clone(), text.clone());
            let doc = toml::parse(file, &text, diags);
            let entries = doc.table("").and_then(|t| t.get("git"));
            if let Some(Value::Array(items, _)) = entries {
                for item in items {
                    let Value::InlineTable(fields, _) = item else { continue };
                    let get = |k: &str| {
                        fields.iter().find(|f| f.key == k).and_then(|f| match &f.value {
                            Value::Str(s, _) => Some(s.clone()),
                            _ => None,
                        })
                    };
                    let pin = match (get("version"), get("rev")) {
                        (Some(v), None) => Pin::Version(v),
                        (None, Some(r)) => Pin::Rev(r),
                        _ => continue,
                    };
                    if let (Some(url), Some(commit)) = (get("url"), get("commit")) {
                        if is_commit(&commit) {
                            locked.insert((url, pin), commit);
                        }
                    }
                }
            }
        }
        Fetcher { path, locked, used: BTreeMap::new(), online, cache: crate::user_cache_dir() }
    }

    /// The checkout of `url` at `pin`, fetched first when this load is online and it is
    /// not in the cache yet.
    pub fn checkout(&mut self, url: &str, pin: &Pin) -> Result<PathBuf, FetchError> {
        let key = (url.to_owned(), pin.clone());
        let commit = match (self.locked.get(&key), pin) {
            (Some(c), _) => c.clone(),
            (None, Pin::Rev(r)) => r.to_ascii_lowercase(),
            (None, Pin::Version(_)) if !self.online => return Err(FetchError::NotFetched),
            (None, Pin::Version(v)) => resolve_version(url, v)?,
        };
        let cache = self.cache.clone().ok_or(FetchError::NoCache)?;
        let dir = cache.join("git").join(repo_dir(url)).join(&commit);
        if !dir.is_dir() {
            if !self.online {
                return Err(FetchError::NotFetched);
            }
            fetch(url, pin, &commit, &dir)?;
        }
        self.used.insert(key, commit);
        Ok(dir)
    }

    /// Write the lockfile when what this load used differs from what it held: a new
    /// source, one resolved again, or one no longer used. Never offline, since the editor
    /// does not decide what a build pins. `Ok(true)` when it was written.
    pub fn save(&self) -> std::io::Result<bool> {
        if !self.online || self.used == self.locked {
            return Ok(false);
        }
        // Nothing fetched and nothing locked: no lockfile to write or keep.
        if self.used.is_empty() && !self.path.exists() {
            return Ok(false);
        }
        let mut text = String::from(
            "# Written by `dz`: the commit each git source is pinned to (spec §3.2). Check it\n\
             # in, so that every machine builds the same code. Delete an entry to resolve its\n\
             # version again.\n\ngit = [\n",
        );
        for ((url, pin), commit) in &self.used {
            let (k, v) = pin.key();
            text.push_str(&format!(
                "    {{ url = \"{}\", {k} = \"{}\", commit = \"{commit}\" }},\n",
                escape(url),
                escape(v)
            ));
        }
        text.push_str("]\n");
        std::fs::write(&self.path, text)?;
        Ok(true)
    }

    pub fn lockfile(&self) -> &Path {
        &self.path
    }
}

fn escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// `imgui-3f2a…`: the repository's name, for a person looking in the cache, and a hash of
/// the whole URL, so two repositories of one name do not share a folder.
fn repo_dir(url: &str) -> String {
    let name = url.trim_end_matches('/').trim_end_matches(".git");
    let name = name.rsplit(['/', ':', '\\']).next().unwrap_or("repo");
    let name: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    // FNV-1a: stable across Rust releases and machines, unlike `DefaultHasher`.
    let mut h: u64 = 0xcbf29ce484222325;
    for b in url.bytes() {
        h = (h ^ b as u64).wrapping_mul(0x100000001b3);
    }
    format!("{name}-{h:016x}")
}

/// `git` with no prompt for a password, which would hang a build waiting on a terminal
/// nobody is looking at.
fn git() -> Command {
    let mut c = Command::new("git");
    c.env("GIT_TERMINAL_PROMPT", "0");
    c
}

fn run(cmd: &mut Command) -> Result<String, FetchError> {
    let out = cmd.output().map_err(|e| FetchError::NoGit(e.to_string()))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_owned();
        Err(FetchError::Git(if err.is_empty() {
            format!("git exited with {}", out.status)
        } else {
            err
        }))
    }
}

/// The tags a version may be written as: `v1.93.0` first, as most projects tag, then
/// `1.93.0`; a version already starting with `v` is tried as written first.
fn tag_candidates(version: &str) -> Vec<String> {
    let mut out = vec![version.to_owned()];
    match version.strip_prefix('v') {
        Some(bare) => out.push(bare.to_owned()),
        None => out.insert(0, format!("v{version}")),
    }
    out
}

/// The commit a release tag names on the remote, through an annotated tag to the commit
/// it tags.
fn resolve_version(url: &str, version: &str) -> Result<String, FetchError> {
    let tags = tag_candidates(version);
    let refs: Vec<String> = tags.iter().map(|t| format!("refs/tags/{t}")).collect();
    // A pattern matches a ref's name exactly, so the peeled `^{}` line of an annotated tag
    // is listed only when asked for too.
    let patterns: Vec<String> =
        refs.iter().flat_map(|r| [r.clone(), format!("{r}^{{}}")]).collect();
    let listing = run(git().arg("ls-remote").arg("--tags").arg(url).args(&patterns))?;
    for r in &refs {
        // An annotated tag lists itself and then, as `<ref>^{}`, the commit it tags.
        let peeled = format!("{r}^{{}}");
        let find = |name: &str| {
            listing.lines().find_map(|l| {
                let (sha, rf) = l.split_once('\t')?;
                (rf == name).then(|| sha.to_owned())
            })
        };
        if let Some(sha) = find(&peeled).or_else(|| find(r)) {
            return Ok(sha);
        }
    }
    Err(FetchError::NoSuchVersion { tried: tags })
}

/// Check `commit` out into `dir`, through a folder of its own that is renamed into place
/// only once it is complete, so a fetch that fails or is interrupted leaves nothing a
/// later load would take for a checkout.
fn fetch(url: &str, pin: &Pin, commit: &str, dir: &Path) -> Result<(), FetchError> {
    let parent = dir.parent().expect("a checkout has a parent");
    std::fs::create_dir_all(parent).map_err(|e| FetchError::Git(e.to_string()))?;
    let tmp = parent.join(format!(".{commit}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).map_err(|e| FetchError::Git(e.to_string()))?;
    let result = (|| {
        let at = |c: &mut Command| {
            c.arg("-C").arg(&tmp);
        };
        let mut init = git();
        at(&mut init);
        run(init.args(["init", "-q"]))?;
        // The commit alone first, which most hosts serve; then the tag, for a host that
        // serves only what a ref names; then everything.
        let mut tries: Vec<Vec<String>> = vec![vec![commit.to_owned()]];
        if let Pin::Version(v) = pin {
            for t in tag_candidates(v) {
                tries.push(vec![format!("refs/tags/{t}")]);
            }
        }
        let mut last = None;
        let mut fetched = false;
        for refspec in &tries {
            let mut f = git();
            at(&mut f);
            match run(f.args(["fetch", "-q", "--depth", "1", url]).args(refspec)) {
                Ok(_) => {
                    let mut has = git();
                    at(&mut has);
                    if run(has.args(["cat-file", "-e", &format!("{commit}^{{commit}}")])).is_ok() {
                        fetched = true;
                        break;
                    }
                }
                Err(e) => last = Some(e),
            }
        }
        if !fetched {
            let mut all = git();
            at(&mut all);
            if let Err(e) = run(all.args([
                "fetch",
                "-q",
                url,
                "+refs/heads/*:refs/remotes/origin/*",
                "+refs/tags/*:refs/tags/*",
            ])) {
                return Err(last.unwrap_or(e));
            }
        }
        let mut co = git();
        at(&mut co);
        run(co.args(["-c", "advice.detachedHead=false", "checkout", "-q", commit]))?;
        let mut head = git();
        at(&mut head);
        let got = run(head.args(["rev-parse", "HEAD"]))?;
        if !got.trim().eq_ignore_ascii_case(commit) {
            return Err(FetchError::Git(format!(
                "checked out {} where {commit} was asked for",
                got.trim()
            )));
        }
        Ok(())
    })();
    match result {
        Ok(()) => match std::fs::rename(&tmp, dir) {
            Ok(()) => Ok(()),
            // Another process put the same checkout there first: it is the same commit.
            Err(_) if dir.is_dir() => {
                let _ = std::fs::remove_dir_all(&tmp);
                Ok(())
            }
            Err(e) => Err(FetchError::Git(e.to_string())),
        },
        Err(e) => {
            let _ = std::fs::remove_dir_all(&tmp);
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_version_is_tried_with_and_without_v() {
        assert_eq!(tag_candidates("1.93.0"), ["v1.93.0", "1.93.0"]);
        assert_eq!(tag_candidates("v2.0"), ["v2.0", "2.0"]);
    }

    #[test]
    fn a_commit_is_written_in_full() {
        assert!(is_commit(&"a".repeat(40)));
        assert!(!is_commit("abc123"));
        assert!(!is_commit(&"g".repeat(40)));
    }

    #[test]
    fn the_cache_folder_names_the_repository_and_tells_urls_apart() {
        let a = repo_dir("https://github.com/ocornut/imgui");
        assert!(a.starts_with("imgui-"), "{a}");
        assert_eq!(a, repo_dir("https://github.com/ocornut/imgui"));
        assert_ne!(a, repo_dir("https://example.com/fork/imgui.git"));
        assert!(repo_dir("git@github.com:ocornut/imgui.git").starts_with("imgui-"));
    }
}
