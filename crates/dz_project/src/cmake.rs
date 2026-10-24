//! A library built by its own CMake (spec §15.1, D-0090): where its build lives, and what
//! its install says a program needs in order to use it.
//!
//! Running CMake is the CLI's job (`dz`'s `native.rs`): analysis is a pure function of
//! what is on disk, so it only *reads* an install here. The editor therefore sees a
//! library's headers once a build has installed them, and never builds anything itself.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::Optimize;

/// `cmake = "vendor/SDL"` in a recipe.
#[derive(Debug, Clone)]
pub struct CmakeBuild {
    /// The folder holding the library's `CMakeLists.txt`.
    pub source: PathBuf,
    /// `options = { SDL_TEST_LIBRARY = false }`: `-D` settings for the configure step, with
    /// `true` and `false` already written as `ON` and `OFF`.
    pub options: Vec<(String, String)>,
    /// `build/cmake/<lib>/<config>/` under the root project. One tree per configuration,
    /// so switching between a debug and a release build does not rebuild the library.
    pub dir: PathBuf,
    /// CMake's name for the configuration the build mode picks: `Debug` or `Release`.
    pub config: &'static str,
    /// Whether an install was found when the workspace was loaded.
    pub installed: bool,
}

impl CmakeBuild {
    pub fn build_dir(&self) -> PathBuf {
        self.dir.join("build")
    }

    pub fn install_dir(&self) -> PathBuf {
        self.dir.join("install")
    }
}

/// The CMake configuration a build mode builds (D-0090). Release and shipping share one:
/// CMake's `Release` is already `-O3 -DNDEBUG`, which is what shipping asks for.
pub fn config_for(optimize: Optimize) -> &'static str {
    match optimize {
        Optimize::Debug => "Debug",
        Optimize::Release | Optimize::Shipping => "Release",
    }
}

/// What an install says a program needs to compile against it and link it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Usage {
    pub include_dirs: Vec<PathBuf>,
    /// For every translation unit that includes the library's headers, unlike a source
    /// recipe's `defines`, which build the library's own sources and nothing else.
    pub defines: Vec<String>,
    pub lib_dirs: Vec<PathBuf>,
    pub link_libs: Vec<String>,
    pub frameworks: Vec<String>,
    /// Everything else on the link line, in order: `-pthread`, a library named by its
    /// path, a `-Wl,` option.
    pub link_flags: Vec<String>,
}

/// Read what the library installed into `install`, or `None` when nothing is there yet.
///
/// The pkg-config file says how to use the library, and is what most CMake libraries
/// install (`lib/pkgconfig/sdl3.pc`). The build is static, so its private libraries are
/// linked too, and the packages it requires are followed when they were installed beside
/// it. One named after the recipe is preferred; otherwise there must be exactly one. With
/// none to go by, the install's `include/` is the include dir and every library it
/// installed is linked, in name order.
pub fn read_install(install: &Path, lib_name: &str) -> Option<Usage> {
    if !install.is_dir() {
        return None;
    }
    let pc_dirs: Vec<PathBuf> = ["lib/pkgconfig", "lib64/pkgconfig", "share/pkgconfig"]
        .iter()
        .map(|d| install.join(d))
        .filter(|d| d.is_dir())
        .collect();
    let mut pcs: Vec<PathBuf> = pc_dirs
        .iter()
        .flat_map(|d| std::fs::read_dir(d).into_iter().flatten().flatten())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "pc"))
        .collect();
    pcs.sort();
    let stem = |p: &Path| p.file_stem().map(|s| s.to_string_lossy().to_lowercase());
    let chosen = pcs
        .iter()
        .find(|p| stem(p).as_deref() == Some(&lib_name.to_lowercase()))
        .or(if pcs.len() == 1 { pcs.first() } else { None });

    let mut usage = Usage::default();
    match chosen {
        Some(pc) => {
            let mut seen = HashSet::new();
            read_pc(pc, &pc_dirs, &mut seen, &mut usage);
        }
        None => installed_files(install, &mut usage),
    }
    Some(usage)
}

/// The install of a library with no pkg-config file: its headers and every library in it.
fn installed_files(install: &Path, usage: &mut Usage) {
    let include = install.join("include");
    if include.is_dir() {
        usage.include_dirs.push(include);
    }
    let lib = install.join("lib");
    let mut names: Vec<String> = std::fs::read_dir(&lib)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if let Some(n) = name.strip_prefix("lib").and_then(|n| n.strip_suffix(".a")) {
                Some(n.to_owned())
            } else {
                name.strip_suffix(".lib").map(str::to_owned)
            }
        })
        .collect();
    names.sort();
    if !names.is_empty() {
        usage.lib_dirs.push(lib);
    }
    usage.link_libs.extend(names);
}

/// One pkg-config file into `usage`, then the packages it requires that were installed
/// beside it. One that was not is the machine's, and D-0090 says a vendored build does not
/// link whatever the machine happens to have, so it is passed over.
fn read_pc(path: &Path, dirs: &[PathBuf], seen: &mut HashSet<PathBuf>, usage: &mut Usage) {
    if !seen.insert(path.to_path_buf()) {
        return;
    }
    let Ok(text) = std::fs::read_to_string(path) else { return };
    let pc = parse_pc(&text);
    apply_cflags(&pc.cflags, usage);
    apply_libs(&pc.libs, usage);
    for name in pc.requires {
        if let Some(found) = dirs.iter().map(|d| d.join(format!("{name}.pc"))).find(|p| p.is_file())
        {
            read_pc(&found, dirs, seen, usage);
        }
    }
}

/// The fields of a pkg-config file that matter for a static build, variables expanded and
/// split into arguments.
#[derive(Debug, Default, PartialEq, Eq)]
struct Pc {
    cflags: Vec<String>,
    /// `Libs`, then `Libs.private`.
    libs: Vec<String>,
    /// `Requires` and `Requires.private`, without their version constraints.
    requires: Vec<String>,
}

fn parse_pc(text: &str) -> Pc {
    let mut vars: Vec<(String, String)> = Vec::new();
    let (mut cflags, mut libs, mut private, mut requires) =
        (String::new(), String::new(), String::new(), String::new());
    for raw in text.lines() {
        let line = raw.split('#').next().unwrap_or("").trim();
        // `name=value` is a variable and `Name: value` a field; whichever of `=` and `:`
        // comes first says which, since a value may hold the other (`prefix=C:/sdk`).
        let eq = line.find('=');
        let colon = line.find(':');
        match (eq, colon) {
            (Some(e), c) if c.map_or(true, |c| e < c) => {
                let value = expand(line[e + 1..].trim(), &vars);
                vars.push((line[..e].trim().to_owned(), value));
            }
            (_, Some(c)) => {
                let value = expand(line[c + 1..].trim(), &vars);
                match line[..c].trim() {
                    "Cflags" | "Cflags.private" => push_words(&mut cflags, &value),
                    "Libs" => push_words(&mut libs, &value),
                    "Libs.private" => push_words(&mut private, &value),
                    "Requires" | "Requires.private" => {
                        requires.push(' ');
                        requires.push_str(&value);
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
    push_words(&mut libs, &private);
    Pc { cflags: split_args(&cflags), libs: split_args(&libs), requires: package_names(&requires) }
}

fn push_words(into: &mut String, value: &str) {
    if !value.is_empty() {
        into.push(' ');
        into.push_str(value);
    }
}

/// `${name}` replaced by the variable's value, as far as the variables before it define.
fn expand(value: &str, vars: &[(String, String)]) -> String {
    let mut out = String::new();
    let mut rest = value;
    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find('}') else {
            out.push_str(&rest[start..]);
            return out;
        };
        let name = &after[..end];
        if let Some((_, v)) = vars.iter().rev().find(|(n, _)| n == name) {
            out.push_str(v);
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

/// Arguments as a shell would split them: on whitespace, with quotes and backslashes
/// keeping a path that has a space in it together.
fn split_args(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut started = false;
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), c) => cur.push(c),
            (None, '"' | '\'') => {
                quote = Some(c);
                started = true;
            }
            (None, '\\') => {
                if let Some(next) = chars.next() {
                    cur.push(next);
                }
                started = true;
            }
            (None, c) if c.is_whitespace() => {
                if started || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
                started = false;
            }
            (None, c) => cur.push(c),
        }
    }
    if started || !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// `foo >= 1.2, bar baz` → `foo`, `bar`, `baz`.
fn package_names(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut skip_version = false;
    for word in text.split(|c: char| c == ',' || c.is_whitespace()).filter(|w| !w.is_empty()) {
        if matches!(word, "=" | "<" | ">" | "<=" | ">=" | "!=") {
            skip_version = true;
            continue;
        }
        if std::mem::take(&mut skip_version) {
            continue;
        }
        out.push(word.to_owned());
    }
    out
}

fn apply_cflags(args: &[String], usage: &mut Usage) {
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let dir = if a == "-I" || a == "-isystem" {
            it.next().cloned()
        } else {
            a.strip_prefix("-I").or_else(|| a.strip_prefix("-isystem")).map(str::to_owned)
        };
        if let Some(d) = dir {
            push_unique(&mut usage.include_dirs, PathBuf::from(d));
        } else if let Some(d) = a.strip_prefix("-D") {
            let d = if d.is_empty() { it.next().cloned().unwrap_or_default() } else { d.into() };
            push_unique(&mut usage.defines, d);
        }
        // Anything else is a compile option for the library's own build, such as
        // `-pthread`, which the link line carries where it matters.
    }
}

fn apply_libs(args: &[String], usage: &mut Usage) {
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "-L" {
            if let Some(d) = it.next() {
                push_unique(&mut usage.lib_dirs, PathBuf::from(d));
            }
        } else if let Some(d) = a.strip_prefix("-L") {
            push_unique(&mut usage.lib_dirs, PathBuf::from(d));
        } else if a == "-l" {
            if let Some(l) = it.next() {
                push_unique(&mut usage.link_libs, l.clone());
            }
        } else if let Some(l) = a.strip_prefix("-l") {
            push_unique(&mut usage.link_libs, l.to_owned());
        } else if a == "-framework" {
            if let Some(f) = it.next() {
                push_unique(&mut usage.frameworks, f.clone());
            }
        } else if let Some(f) = a.strip_prefix("-Wl,-framework,") {
            push_unique(&mut usage.frameworks, f.to_owned());
        } else {
            push_unique(&mut usage.link_flags, a.clone());
        }
    }
}

fn push_unique<T: PartialEq>(list: &mut Vec<T>, item: T) {
    if !list.contains(&item) {
        list.push(item);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pc_file_gives_the_static_link_line() {
        let pc = parse_pc(
            "prefix=/opt/sdl\n\
             libdir=${prefix}/lib\n\
             includedir=${prefix}/include # where the headers are\n\
             \n\
             Name: sdl3\n\
             Requires.private: zlib >= 1.2, png\n\
             Libs: -L${libdir}  -lSDL3\n\
             Libs.private: -pthread -lm -Wl,-framework,Cocoa\n\
             Cflags: -I${includedir} -DSDL_STATIC \"-I/opt/with space\"\n",
        );
        assert_eq!(pc.requires, ["zlib", "png"]);
        let mut usage = Usage::default();
        apply_cflags(&pc.cflags, &mut usage);
        apply_libs(&pc.libs, &mut usage);
        assert_eq!(
            usage,
            Usage {
                include_dirs: vec!["/opt/sdl/include".into(), "/opt/with space".into()],
                defines: vec!["SDL_STATIC".into()],
                lib_dirs: vec!["/opt/sdl/lib".into()],
                link_libs: vec!["SDL3".into(), "m".into()],
                frameworks: vec!["Cocoa".into()],
                link_flags: vec!["-pthread".into()],
            }
        );
    }

    #[test]
    fn a_variable_may_hold_a_colon_and_a_field_an_equals_sign() {
        let pc = parse_pc("prefix=C:/sdk\nCflags: -I${prefix}/include -DLEVEL=2\n");
        assert_eq!(pc.cflags, ["-IC:/sdk/include", "-DLEVEL=2"]);
    }

    #[test]
    fn an_install_with_no_pc_file_links_every_library_it_installed() {
        let dir = std::env::temp_dir().join(format!("dz-cmake-nopc-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("include")).unwrap();
        std::fs::create_dir_all(dir.join("lib")).unwrap();
        for f in ["libzeta.a", "libalpha.a", "beta.lib", "notes.txt"] {
            std::fs::write(dir.join("lib").join(f), "").unwrap();
        }
        let usage = read_install(&dir, "anything").unwrap();
        assert_eq!(usage.include_dirs, [dir.join("include")]);
        assert_eq!(usage.link_libs, ["alpha", "beta", "zeta"]);
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(read_install(&dir, "anything"), None);
    }
}
