//! How the C++ compiler `dz` drives is spoken to (T-0253).
//!
//! `dz` was written against GCC's command line, which clang++, g++ and `c++` all speak.
//! `cl.exe` speaks MSVC's: other spellings for the same flags, an object named by `/Fo`, an
//! executable by `/Fe`, its dependencies as JSON, and its diagnostics on standard output. It
//! also needs the environment a Visual Studio Developer Prompt sets (`INCLUDE`, `LIB`, and a
//! `PATH` holding its linker), which `dz` finds itself rather than requiring the prompt.
//!
//! Everything that differs between the two is here, asked for by the build, so the build
//! writes no flag of either family itself.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use dz_project::Optimize;

/// Which command line a compiler speaks.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Family {
    /// clang++, g++, `c++`, and anything else not named below.
    Gnu,
    /// `cl.exe`. Told by its name, since it has no `--version` to ask; `clang-cl` speaks the
    /// same command line but is not supported yet, being neither family's tools.
    Msvc,
}

pub fn family(cxx: &str) -> Family {
    let stem = Path::new(cxx).file_stem().map(|s| s.to_string_lossy().to_ascii_lowercase());
    match stem.as_deref() {
        Some("cl") => Family::Msvc,
        _ => Family::Gnu,
    }
}

pub fn is_msvc(cxx: &str) -> bool {
    family(cxx) == Family::Msvc
}

/// The compiler `cxx` names, as a path when `cl` is not on the `PATH` but Visual Studio
/// has one: the build stamp and the object cache key on the compiler's file, so they need
/// one they can find. Anything else is returned as it was given.
pub fn resolve(cxx: &str) -> String {
    #[cfg(windows)]
    if is_msvc(cxx) && Path::new(cxx).components().count() == 1 {
        if let Some(tool) = msvc_tool() {
            return tool.path().to_string_lossy().into_owned();
        }
    }
    cxx.to_owned()
}

/// A command running `program` (the compiler, or a tool it needs such as CMake) with the
/// environment `cxx` needs.
fn command_for(cxx: &str, program: impl AsRef<std::ffi::OsStr>) -> Command {
    let command = Command::new(program);
    #[cfg(windows)]
    if is_msvc(cxx) {
        if let Some(tool) = msvc_tool() {
            let mut command = command;
            command.envs(tool.env().into_iter().map(|(k, v)| (k, v)));
            return command;
        }
    }
    #[cfg(not(windows))]
    let _ = cxx;
    command
}

/// CMake needs to find Ninja itself. Visual Studio's CMake workload installs
/// both, but an ordinary Windows shell often has neither on PATH.
pub fn cmake_command(cxx: &str, program: &str) -> Command {
    #[cfg(windows)]
    if is_msvc(cxx) {
        static TOOLS: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
        if let Some(dir) = TOOLS.get_or_init(vs_cmake_dir) {
            let mut cmd = command_for(cxx, program);
            let path = cmd
                .get_envs()
                .find(|(k, _)| k.eq_ignore_ascii_case("PATH"))
                .and_then(|(_, v)| v.map(std::ffi::OsStr::to_owned))
                .or_else(|| std::env::var_os("PATH"))
                .unwrap_or_default();
            let dirs =
                std::env::split_paths(&path).chain([dir.join("CMake/bin"), dir.join("Ninja")]);
            if let Ok(path) = std::env::join_paths(dirs) {
                cmd.env("PATH", path);
            }
            return cmd;
        }
    }
    command_for(cxx, program)
}

#[cfg(windows)]
fn vs_cmake_dir() -> Option<PathBuf> {
    if crate::fresh::which("cmake").is_some() && crate::fresh::which("ninja").is_some() {
        return None;
    }
    let installer = PathBuf::from(std::env::var_os("ProgramFiles(x86)")?)
        .join("Microsoft Visual Studio/Installer/vswhere.exe");
    let output = Command::new(installer)
        .args([
            "-latest",
            "-products",
            "*",
            "-requires",
            "Microsoft.VisualStudio.Component.VC.CMake.Project",
            "-property",
            "installationPath",
            "-utf8",
        ])
        .output()
        .ok()?;
    let root = std::str::from_utf8(&output.stdout).ok()?.trim();
    (output.status.success() && !root.is_empty())
        .then(|| Path::new(root).join("Common7/IDE/CommonExtensions/Microsoft/CMake"))
}

/// A command running the compiler itself.
pub fn command(cxx: &str) -> Command {
    command_for(cxx, cxx)
}

/// What the compiler said about a file it refused. `cl.exe` writes its diagnostics to
/// standard output, after a line naming the file it is compiling, which says nothing.
pub fn complaints(cxx: &str, out: &Output) -> String {
    let stderr = String::from_utf8_lossy(&out.stderr);
    if !is_msvc(cxx) {
        return stderr.into_owned();
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let said: Vec<&str> = stdout.lines().filter(|line| !names_a_source(line)).collect();
    let mut text = said.join("\n");
    if !stderr.trim().is_empty() {
        text.push('\n');
        text.push_str(&stderr);
    }
    text
}

/// A line that is only a source file's name, as `cl.exe` prints for each one it compiles.
fn names_a_source(line: &str) -> bool {
    let line = line.trim();
    !line.is_empty()
        && !line.contains([' ', '/', '\\', ':'])
        && [".cpp", ".cc", ".cxx", ".c"].iter().any(|ext| line.ends_with(ext))
}

/// The flags that make an optimisation level, for the generated code and the libraries
/// alike. Bounds checks are not a build setting (spec §5.3), so none of these turns one off.
pub fn optimisation(cxx: &str, level: Optimize) -> Vec<String> {
    let flags: &[&str] = match (family(cxx), level) {
        (Family::Gnu, Optimize::Debug) => &["-O0", "-g"],
        (Family::Gnu, Optimize::Release) => &["-O2"],
        (Family::Gnu, Optimize::Shipping) => &["-O3", "-DNDEBUG"],
        // `/Z7` puts the debug information in each object rather than in one `.pdb` that every
        // compile running at once would have to share.
        (Family::Msvc, Optimize::Debug) => &["/Od", "/Z7"],
        (Family::Msvc, Optimize::Release) => &["/O2"],
        (Family::Msvc, Optimize::Shipping) => &["/O2", "/DNDEBUG"],
    };
    flags.iter().map(|f| (*f).to_owned()).collect()
}

/// `-std=c++17`, or `/std:c++17`. MSVC has no `/std:c++23`; past C++20 it is `c++latest`.
pub fn standard(cxx: &str, standard: &str) -> String {
    match family(cxx) {
        Family::Gnu => format!("-std={standard}"),
        Family::Msvc => match standard {
            "c++14" | "c++17" | "c++20" => format!("/std:{standard}"),
            _ => "/std:c++latest".to_owned(),
        },
    }
}

/// The flags every file of Dazzle's own code is compiled with, besides the standard and the
/// optimisation level. Not for a vendored library, which keeps the semantics its authors
/// wrote it against.
pub fn generated_code(cxx: &str) -> Vec<String> {
    let flags: &[&str] = match family(cxx) {
        // spec §16.5 — nothing generated may need exceptions or RTTI. `-fwrapv`: integer
        // arithmetic wraps in every build (spec §5.1, 18.63). The generated code no longer
        // relies on it, since `cl.exe` has no such flag (T-0255), but it stays until the
        // benchmarks show it changes nothing.
        //
        // Two more things C++ leaves undefined that safe Dazzle can reach (T-0133, D-0094).
        // `-fno-finite-loops`: C++ lets the optimiser assume a loop with no side effects ends,
        // and clang++ and g++ both deleted `while x < 10 {}` with `x` unchanged, returning past
        // it; Dazzle runs the loop as written. `-fno-strict-aliasing`: a pointer cast inside
        // `unsafe`, or in `std`'s trusted bodies, reads the bytes there as the type it names,
        // rather than letting the optimiser assume two types never share memory. Neither moved
        // the whole-program benchmarks beyond their noise.
        Family::Gnu => &[
            "-fno-exceptions",
            "-fno-rtti",
            "-fwrapv",
            "-Wall",
            "-Wextra",
            "-fno-finite-loops",
            "-fno-strict-aliasing",
        ],
        // The same promises under `cl.exe` (T-0255). No exceptions: no `/EH`, and the
        // standard library told so. Wrapping arithmetic is in the generated code itself, and a
        // loop with no effect is kept by the barrier it starts with, `DZ_LOOP_ALIVE()`. Strict
        // aliasing needs nothing: MSVC does no type-based alias analysis.
        //
        // `/utf-8`: the generated code and its string literals are UTF-8, where `cl.exe` would
        // read them in the system's code page. `/permissive-`: the standard's rules, two-phase
        // lookup among them, as the other compilers apply them. `/Zc:nrvo`: a named value
        // returned is built in place even at `/Od`, as clang++ and g++ always do; without it,
        // the moved-from copy's `drop` runs where theirs does not (T-0260). `/bigobj`: `std`'s
        // instances are more sections than an object holds by default.
        //
        // `/W4`, with the warnings that are about a correct program turned off (T-0256), each
        // for its reason. C4723, "potential divide by 0": the optimiser saw a float divided by
        // a zero, which is an infinity or NaN by IEEE 754 (spec §5.1). An integer division
        // never reaches C++'s `/` with a divisor that can be zero; it goes through `num::div`,
        // which panics. C4702, "unreachable code": at `/O2` it is code the optimiser found
        // dead after inlining, a bounds check's panic known to fail in one caller, reported
        // against the lines of what was inlined; at `/Od` it is the trap `dz` writes after a
        // body whose every path returns where C++ cannot always see it (an exhaustive
        // `switch`), unreachable on purpose. Neither says anything is wrong. C4459, "declaration
        // hides global declaration", and C4458 for a member: a local may shadow a field,
        // a constant, static or function
        // of its module, and the emitter writes every use of the item qualified (T-0121).
        Family::Msvc => &[
            "/nologo",
            "/GR-",
            "/D_HAS_EXCEPTIONS=0",
            "/W4",
            "/wd4458",
            "/wd4459",
            "/wd4702",
            "/wd4723",
            "/utf-8",
            "/permissive-",
            "/Zc:nrvo",
            "/bigobj",
        ],
    };
    flags.iter().map(|f| (*f).to_owned()).collect()
}

pub fn define(cxx: &str, define: &str) -> String {
    match family(cxx) {
        Family::Gnu => format!("-D{define}"),
        Family::Msvc => format!("/D{define}"),
    }
}

pub fn include_dir(cxx: &str, dir: &Path) -> String {
    match family(cxx) {
        Family::Gnu => format!("-I{}", dir.display()),
        Family::Msvc => format!("/I{}", dir.display()),
    }
}

/// The language a library's source is compiled as, which a recipe may change by adding its
/// own after (`-x objective-c`).
pub fn language(cxx: &str, c: bool, cpp_standard: &str) -> Vec<String> {
    match (family(cxx), c) {
        (Family::Gnu, true) => vec!["-x".to_owned(), "c".to_owned()],
        (Family::Gnu, false) => {
            vec!["-x".to_owned(), "c++".to_owned(), standard(cxx, cpp_standard)]
        }
        (Family::Msvc, true) => vec!["/nologo".to_owned(), "/TC".to_owned(), "/utf-8".to_owned()],
        (Family::Msvc, false) => vec![
            "/nologo".to_owned(),
            "/TP".to_owned(),
            "/utf-8".to_owned(),
            standard(cxx, cpp_standard),
        ],
    }
}

/// A recipe's own `flags`, which manifests write in GCC's spelling (`-std=c17`, `-include
/// sokol_gfx.h`), so one recipe serves every compiler. For `cl.exe` the ones with a
/// counterpart are translated; any other is passed as written, and `cl.exe` says it ignored
/// it (`-x objective-c` is macOS's, in a table `cl.exe` never reads).
pub fn recipe_flags(cxx: &str, flags: &[String]) -> Vec<String> {
    if !is_msvc(cxx) {
        return flags.to_vec();
    }
    let mut out = Vec::new();
    let mut flags = flags.iter();
    while let Some(flag) = flags.next() {
        let translated = if let Some(std) = flag.strip_prefix("-std=") {
            if std.starts_with("c++") || std.starts_with("gnu++") {
                standard(cxx, &std.replace("gnu++", "c++"))
            } else {
                // `cl.exe` knows `c11` and `c17`; GNU dialects are those plus extensions.
                format!("/std:{}", std.replace("gnu", "c"))
            }
        } else if flag == "-include" {
            match flags.next() {
                Some(header) => format!("/FI{header}"),
                None => continue,
            }
        } else if let Some(header) = flag.strip_prefix("-include") {
            format!("/FI{header}")
        } else if let Some(rest) = flag.strip_prefix("-D") {
            format!("/D{rest}")
        } else if let Some(rest) = flag.strip_prefix("-I") {
            format!("/I{rest}")
        } else if flag == "-w" {
            "/w".to_owned()
        } else {
            flag.clone()
        };
        out.push(translated);
    }
    out
}

/// No warnings, for a library: they are not the project's to fix (invariant 7 is about the
/// code Dazzle generates).
pub fn no_warnings(cxx: &str) -> String {
    match family(cxx) {
        Family::Gnu => "-w".to_owned(),
        Family::Msvc => "/w".to_owned(),
    }
}

/// An object's extension: `cl.exe` names any other file it is given to link as an
/// "unrecognized source file type" (D9024), once per object.
pub fn object_ext(cxx: &str) -> &'static str {
    match family(cxx) {
        Family::Gnu => "o",
        Family::Msvc => "obj",
    }
}

/// Link-time optimisation: the flag each file is compiled with.
pub fn lto(cxx: &str) -> String {
    match family(cxx) {
        // `=auto` runs the link-time work in parallel, and keeps g++ from printing a note
        // about running it serially.
        Family::Gnu => "-flto=auto".to_owned(),
        Family::Msvc => "/GL".to_owned(),
    }
}

/// Compiling one file: `source` to `object`, with the headers it read listed in `depfile`.
/// `cl.exe` lists them as JSON, which `native::write_stamp` reads as it reads a make rule.
pub fn compile_one(
    cxx: &str,
    source: &Path,
    object: &Path,
    depfile: &Path,
) -> Vec<std::ffi::OsString> {
    let mut args: Vec<std::ffi::OsString> = Vec::new();
    match family(cxx) {
        Family::Gnu => {
            args.extend(["-c".into(), source.into(), "-o".into(), object.into()]);
            args.extend(["-MD".into(), "-MF".into(), depfile.into()]);
        }
        Family::Msvc => {
            let mut fo = std::ffi::OsString::from("/Fo");
            fo.push(object);
            args.extend(["/c".into(), source.into(), fo]);
            args.extend(["/sourceDependencies".into(), depfile.into()]);
        }
    }
    args
}

/// What the libraries the program links with become on the link's command line: `dirs` to
/// search, `libs` by name.
pub fn libraries(cxx: &str, dirs: &[PathBuf], libs: &[String]) -> Vec<String> {
    match family(cxx) {
        Family::Gnu => dirs
            .iter()
            .map(|d| format!("-L{}", d.display()))
            .chain(libs.iter().map(|l| format!("-l{l}")))
            .collect(),
        Family::Msvc => dirs
            .iter()
            .map(|d| format!("/LIBPATH:{}", d.display()))
            .chain(libs.iter().map(|l| {
                if l.ends_with(".lib") {
                    l.clone()
                } else {
                    format!("{l}.lib")
                }
            }))
            .collect(),
    }
}

/// The link: `objects` and then `link_args` (the libraries' objects and `libraries`' flags)
/// into `exe`. GCC's drivers take the compile flags too, which carry `-flto` and a
/// sanitizer's runtime; `cl.exe` hands what follows `/link` to `link.exe`.
pub fn link(
    cxx: &str,
    args: &[String],
    objects: &[PathBuf],
    link_args: &[String],
    exe: &Path,
    lto: bool,
    debug: bool,
) -> Command {
    let mut command = command(cxx);
    match family(cxx) {
        Family::Gnu => {
            command.args(args).args(objects).args(link_args);
            // Threads (spec §22.3) need it on older glibc, where they are not in libc itself.
            // Windows has its own threads, and MinGW's win32 thread model has no `-pthread`.
            if cfg!(not(windows)) {
                command.arg("-pthread");
            }
            command.arg("-o").arg(exe);
        }
        Family::Msvc => {
            let mut fe = std::ffi::OsString::from("/Fe");
            fe.push(exe);
            command.args(args).arg(fe).args(objects).arg("/link");
            if lto {
                command.arg("/LTCG");
            }
            if debug {
                command.arg("/DEBUG");
            }
            command.args(link_args);
        }
    }
    command
}

/// Use the active Developer Prompt or discover the native toolset and Windows SDK.
#[cfg(windows)]
fn msvc_tool() -> Option<&'static find_msvc_tools::Tool> {
    static TOOL: std::sync::OnceLock<Option<find_msvc_tools::Tool>> = std::sync::OnceLock::new();
    TOOL.get_or_init(|| find_msvc_tools::find_tool(std::env::consts::ARCH, "cl.exe")).as_ref()
}

/// The native Windows toolset is the fallback when no GNU-style driver is installed.
pub fn native_compiler() -> Option<String> {
    #[cfg(windows)]
    {
        msvc_tool().map(|t| t.path().to_string_lossy().into_owned())
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// The files a dependency list names: `cl.exe`'s `/sourceDependencies` JSON, its source and
/// every header it included. `None` if `text` is not that.
pub fn json_dependencies(text: &str) -> Option<Vec<PathBuf>> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    let data = value.get("Data")?;
    let mut out = vec![PathBuf::from(data.get("Source")?.as_str()?)];
    for include in data.get("Includes")?.as_array()? {
        out.push(PathBuf::from(include.as_str()?));
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cl_is_told_by_its_name() {
        assert_eq!(family("cl"), Family::Msvc);
        assert_eq!(family("CL.EXE"), Family::Msvc);
        #[cfg(windows)]
        assert_eq!(family(r"C:\VS\bin\Hostx64\x64\cl.exe"), Family::Msvc);
        for gnu in ["clang++", "g++", "c++", "/usr/bin/clang++-18", "clang-cl"] {
            assert_eq!(family(gnu), Family::Gnu, "{gnu}");
        }
    }

    #[test]
    fn a_line_naming_the_file_cl_compiles_is_not_a_complaint() {
        assert!(names_a_source("main.cpp"));
        assert!(names_a_source("dz_runtime.cpp "));
        assert!(!names_a_source(r"C:\x\main.dz(3): error C2065: 'y': undeclared identifier"));
        assert!(!names_a_source("main.obj : error LNK2019: unresolved external symbol"));
    }

    #[test]
    fn msvc_dependencies_are_read_from_json() {
        let json = r#"{"Version":"1.2","Data":{"Source":"c:\\p\\a.cpp","ProvidedModule":"",
            "Includes":["c:\\p\\a.h","c:\\vs\\include\\new"],"ImportedModules":[],
            "ImportedHeaderUnits":[]}}"#;
        let deps = json_dependencies(json).unwrap();
        let deps: Vec<String> = deps.iter().map(|p| p.display().to_string()).collect();
        assert_eq!(deps, [r"c:\p\a.cpp", r"c:\p\a.h", r"c:\vs\include\new"]);
        assert!(json_dependencies("a.o: a.cpp a.h").is_none());
    }

    #[test]
    fn each_family_spells_the_same_flags_its_own_way() {
        assert_eq!(standard("g++", "c++17"), "-std=c++17");
        assert_eq!(standard("cl", "c++17"), "/std:c++17");
        assert_eq!(standard("cl", "c++23"), "/std:c++latest");
        assert_eq!(define("cl", "X=1"), "/DX=1");
        assert_eq!(
            libraries("cl", &[PathBuf::from("lib")], &["user32".into()]),
            ["/LIBPATH:lib", "user32.lib"]
        );
        assert_eq!(libraries("clang++", &[PathBuf::from("lib")], &["m".into()]), ["-Llib", "-lm"]);
    }

    #[test]
    fn a_recipes_gcc_flags_are_translated_for_cl() {
        let flags: Vec<String> =
            ["-std=c17", "-include", "sokol_gfx.h", "-DX=1", "-Iinc", "-w", "-x", "objective-c"]
                .map(str::to_owned)
                .to_vec();
        assert_eq!(
            recipe_flags("cl", &flags),
            ["/std:c17", "/FIsokol_gfx.h", "/DX=1", "/Iinc", "/w", "-x", "objective-c"]
        );
        assert_eq!(recipe_flags("g++", &flags), flags);
    }
}
