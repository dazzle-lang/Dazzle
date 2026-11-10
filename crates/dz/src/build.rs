//! The build pipeline: source to C++ to a running program (spec §16).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use dz_diag::{codes, Diagnostic, Diagnostics};
use dz_project::{Optimize, OutputKind, Workspace};

// Analysis is shared with the language server, so it lives in `dz_driver` (spec §17.4).
pub use dz_driver::Analysis;

pub struct BuildOutput {
    pub cpp_dir: PathBuf,
    /// Every file this build emitted, changed or not. The compiler is given these rather
    /// than whatever `cpp_dir` holds, so a file left behind by a module that has since been
    /// renamed or removed is never compiled (T-0087).
    pub files: Vec<PathBuf>,
    /// Only the files whose contents actually changed (spec §16.6).
    pub written: Vec<PathBuf>,
    /// The folder the runtime and `std` are compiled from, shared with every other project
    /// built with the same `std` (T-0182). `files` names the project's own copies.
    pub shared: crate::shared::Shared,
}

/// Generate C++ for a project, writing only files whose contents changed (spec §16.6).
pub fn emit_cpp(
    analysis: &Analysis,
    root: &Path,
    options: dz_cpp::Options,
) -> Result<BuildOutput, Diagnostics> {
    let (Some(ws), Some(resolution), Some(table), Some(hir), Some(mono)) =
        (&analysis.ws, &analysis.resolution, &analysis.table, &analysis.hir, &analysis.mono)
    else {
        return Err(Diagnostics::new());
    };
    // Regions are on (T-0117 kept them); `DZ_REGIONS=0` builds the heap-only program, so
    // `tools/bench_regions.py` builds one source both ways without a flag of its own, and a
    // wrong result can be checked against the heap. Any other value, or none, leaves them on.
    let options = dz_cpp::Options {
        regions: options.regions && !std::env::var_os("DZ_REGIONS").is_some_and(|v| v == "0"),
        ..options
    };
    let emitted = dz_cpp::emit(
        ws,
        resolution,
        table,
        hir,
        mono,
        analysis.placements.as_ref(),
        &analysis.map,
        &analysis.files,
        options,
    );
    if emitted.diagnostics.has_errors() {
        return Err(emitted.diagnostics);
    }

    let cpp_dir = root.join("cpp");
    if let Err(e) = std::fs::create_dir_all(&cpp_dir) {
        let mut d = Diagnostics::new();
        d.push(Diagnostic::error(
            codes::CPP_COMPILER_FAILED,
            format!("cannot create {}: {e}", cpp_dir.display()),
        ));
        return Err(d);
    }

    // The runtime and `std` are compiled from a folder every project shares (T-0182).
    let shared = crate::shared::Shared::new(
        root,
        emitted.files.iter().filter(|f| f.shared),
        options.line_directives,
    );
    let cannot_write = |path: &Path, e: std::io::Error| {
        let mut d = Diagnostics::new();
        d.push(Diagnostic::error(
            codes::CPP_COMPILER_FAILED,
            format!("cannot write {}: {e}", path.display()),
        ));
        d
    };

    // The standard library is compiled into `dz` and has no files, so its `#line`
    // directives name copies written into the shared folder — which is what a debugger opens
    // when stepping into `List::push` or `Vec3::length` (spec §16.3). Only the directives
    // change: a panic message still says `<std>/collections/list.dz`. Without directives
    // nothing names them, so they are not written.
    let mut renames: std::collections::HashMap<String, String> = Default::default();
    if options.line_directives {
        for (pseudo, text) in dz_project::embedded_sources() {
            // `<std>/math/vec3.dz` → `<cache>/std-<hash>/src/std/math/vec3.dz`.
            let copy = shared.dir.join("src").join(pseudo.replace(['<', '>'], ""));
            if let Some(dir) = copy.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = crate::shared::write(&copy, text.as_bytes());
            renames.insert(pseudo.to_owned(), copy.display().to_string().replace('\\', "/"));
        }
        // A project named by a relative path has its sources named that way, and each
        // `#line` would follow: building it once as `dz build game` and once as `dz build
        // ./game` or by its full path would change every generated file, and compile every
        // one again. Named absolutely, the C++ is the same however `dz` was run, and a
        // debugger started elsewhere still finds the sources (spec §16.3).
        if let Ok(cwd) = std::env::current_dir() {
            for file in analysis.map.iter() {
                let name = file.name().replace('\\', "/");
                if !name.starts_with('<') && Path::new(&name).is_relative() {
                    let absolute = cwd.join(file.path()).display().to_string().replace('\\', "/");
                    renames.insert(name, absolute);
                }
            }
        }
    }

    // Every file is written to `build/cpp`, the shared ones too: a module's header includes
    // `std.h` from beside it, and the folder is the whole program's C++ for anyone who wants
    // to read or compile it without `dz`.
    let mut files = Vec::new();
    let mut written = Vec::new();
    for file in &emitted.files {
        let path = cpp_dir.join(&file.name);
        files.push(path.clone());
        let contents = with_embedded_lines(&file.contents, &renames);
        if file.shared {
            let copy = shared.dir.join("cpp").join(&file.name);
            crate::shared::write(&copy, contents.as_bytes()).map_err(|e| cannot_write(&copy, e))?;
        }
        // Rewriting an unchanged file would defeat the C++ compiler's own incremental
        // build, which is the only incrementality that exists so far (spec §16.6).
        let unchanged =
            std::fs::read_to_string(&path).map(|existing| existing == contents).unwrap_or(false);
        if !unchanged {
            std::fs::write(&path, &contents).map_err(|e| cannot_write(&path, e))?;
            written.push(path.clone());
        }
    }

    Ok(BuildOutput { cpp_dir, files, written, shared })
}

/// `text` with every `#line N "path"` whose path `renames` holds naming its replacement
/// instead: `std`'s pseudo-paths their real copies, and relative paths absolute ones.
fn with_embedded_lines(text: &str, renames: &std::collections::HashMap<String, String>) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.split_inclusive('\n') {
        // Indented inside an immediately-invoked lambda, which C++ allows.
        let renamed = line.trim_start().strip_prefix("#line ").and_then(|rest| {
            let bare = line.trim_end();
            let path = rest.trim_end().split_once(' ')?.1.strip_prefix('"')?.strip_suffix('"')?;
            let real = renames.get(path)?;
            // Everything before the quoted path, which ends the line.
            let head = &bare[..bare.len() - path.len() - 2];
            Some(format!("{head}\"{real}\"\n"))
        });
        match renamed {
            Some(new_line) => out.push_str(&new_line),
            None => out.push_str(line),
        }
    }
    out
}

/// The C++ compiler to drive (spec §16.5).
///
/// Four places may say which, and the nearest to the person running the build wins:
///
/// 1. `--cxx` on the command line, passed here as `chosen` — one build, one compiler.
/// 2. `DZ_CXX` in the environment, which is how CI pins a matrix entry.
/// 3. `[build] cxx` in the manifest, or `[build.target.<os>] cxx` for one platform —
///    where a project says what it is known to build with.
/// 4. Otherwise the first of `clang++`, `g++`, `c++` found on the path.
///
/// A name given explicitly is **not** probed: if it does not exist the compile fails
/// saying so, which is better than silently using a different compiler than was asked for.
pub fn find_compiler(ws: Option<&Workspace>, chosen: Option<&str>) -> Option<String> {
    if let Some(c) = chosen.filter(|c| !c.is_empty()) {
        return Some(c.to_owned());
    }
    if let Ok(cxx) = std::env::var("DZ_CXX") {
        if !cxx.is_empty() {
            return Some(cxx);
        }
    }
    if let Some(ws) = ws {
        if let Some(cxx) = ws.module(ws.root).build.cxx.clone() {
            if !cxx.is_empty() {
                return Some(cxx);
            }
        }
    }
    for candidate in ["clang++", "g++", "c++"] {
        let probe = Command::new(candidate).arg("--version").output();
        if matches!(probe, Ok(o) if o.status.success()) {
            return Some(candidate.to_owned());
        }
    }
    None
}

/// Is `cxx` GCC? Told by what `--version` says rather than by its name: macOS's `g++` is
/// clang, and a cross compiler's name can be anything.
fn is_gcc(cxx: &str) -> bool {
    Command::new(cxx).arg("--version").output().is_ok_and(|out| {
        let text = String::from_utf8_lossy(&out.stdout);
        text.contains("Free Software Foundation") && !text.contains("clang")
    })
}

/// Compile the generated C++ into an executable, after the vendored libraries it links.
/// `timings` gets the time each of the two took (`--timings`).
pub fn compile(
    analysis: &Analysis,
    out: &BuildOutput,
    root: &Path,
    cxx: Option<&str>,
    timings: &mut dz_driver::Timings,
) -> Result<PathBuf, Diagnostics> {
    let mut diags = Diagnostics::new();
    let Some(cxx) = find_compiler(analysis.ws.as_ref(), cxx) else {
        diags.push(
            Diagnostic::error(codes::CPP_COMPILER_FAILED, "no C++ compiler found").with_help(
                "install clang++ or g++, or name one: `--cxx <compiler>`, `DZ_CXX` in the \
                 environment, or `cxx` in the manifest's `[build]` table",
            ),
        );
        return Err(diags);
    };

    let Some(ws) = &analysis.ws else { return Err(diags) };
    let module = ws.module(ws.root);
    if module.build.output != OutputKind::Executable {
        diags.push(Diagnostic::error(
            codes::CPP_COMPILER_FAILED,
            format!(
                "module `{}` has output = \"{}\"; only executables can be built or run yet",
                module.name,
                match module.build.output {
                    OutputKind::StaticLib => "static-lib",
                    OutputKind::SharedLib => "shared-lib",
                    OutputKind::Executable => "executable",
                }
            ),
        ));
        return Err(diags);
    }

    // The executable goes in `bin/` rather than straight into the output directory, so
    // that directory holds only subdirectories `dz` names itself — `bin`, `cpp`, `obj`,
    // and the harness roots `dz test` and `dz bench` create (spec §16.1).
    //
    // Otherwise the two collide: on Unix the executable of a module named `bench` *is*
    // `build/bench`, which is exactly the directory `dz bench` wants, and the build fails
    // with "Not a directory". Windows never saw it, because there the file is `bench.exe`.
    let bin_dir = root.join("bin");
    if let Err(e) = std::fs::create_dir_all(&bin_dir) {
        diags.push(Diagnostic::error(
            codes::CPP_COMPILER_FAILED,
            format!("cannot create {}: {e}", bin_dir.display()),
        ));
        return Err(diags);
    }
    let exe_name = if cfg!(windows) {
        format!("{}.exe", file_stem(&module.name))
    } else {
        file_stem(&module.name)
    };
    let exe = bin_dir.join(exe_name);

    // Bounds checks are not a build setting: indexing an array or slice is safe code, and
    // stays checked in every build (spec §5.3). Unchecked access is `unsafe { xs.ptr[i] }`.
    let opt: &[&str] = match module.build.optimize {
        Optimize::Debug => &["-O0", "-g"],
        Optimize::Release => &["-O2"],
        Optimize::Shipping => &["-O3", "-DNDEBUG"],
    };

    // Vendored libraries first: a failure there is the library's, and should be reported
    // as such before the generated code is even tried (spec §15.1).
    // `root`, not `bin_dir`: the object cache is `build/obj/`, a sibling of `bin/`.
    let started = std::time::Instant::now();
    let objects = crate::native::build_libraries(ws, &cxx, root, opt, &module.build.cpp_standard)?;
    timings.lap("C and C++ libraries", started);

    // spec §16.5 — nothing generated may need exceptions or RTTI. `-fwrapv`: integer
    // arithmetic wraps in every build (spec §5.1, 18.63), where C++ leaves signed overflow
    // undefined and an optimiser is free to assume it never happens. Only for Dazzle's own
    // code; a vendored library keeps the semantics its authors wrote it against.
    //
    // Two more things C++ leaves undefined that safe Dazzle can reach (T-0133, D-0094).
    // `-fno-finite-loops`: C++ lets the optimiser assume a loop with no side effects ends,
    // and clang++ and g++ both deleted `while x < 10 {}` with `x` unchanged, returning past
    // it; Dazzle runs the loop as written. `-fno-strict-aliasing`: a pointer cast inside
    // `unsafe`, or in `std`'s trusted bodies, reads the bytes there as the type it names,
    // rather than letting the optimiser assume two types never share memory. Neither moved
    // the whole-program benchmarks beyond their noise.
    let mut args: Vec<String> = vec![format!("-std={}", module.build.cpp_standard)];
    args.extend(
        ["-fno-exceptions", "-fno-rtti", "-fwrapv", "-Wall", "-Wextra"]
            .into_iter()
            .chain(["-fno-finite-loops", "-fno-strict-aliasing"])
            .chain(opt.iter().copied())
            .map(str::to_owned),
    );
    // g++ at -O2 inlines a function not declared `inline` only up to 15 instructions, against
    // 70 for one that is. Since T-0185 a generic instance's methods are ordinary functions
    // in one file, where they used to be `inline` in headers, and `astar` ran 12% slower on
    // g++ for it. With the budget a declared-`inline` function gets, all three benchmark
    // programs run 6-17% faster than with either, for 0.1-0.17 s more per release build
    // (T-0190). clang++ weighs the two alike, and warns about the flag, so it is g++ alone,
    // told apart by what it says it is.
    if module.build.optimize != Optimize::Debug && is_gcc(&cxx) {
        args.extend(["--param".to_owned(), "max-inline-insns-auto=70".to_owned()]);
    }
    // Every module's generated C++ gets the flags of everything the root reaches (spec
    // §15.1).
    let flags = ws.cpp_flags(ws.root);
    args.extend(flags.include_dirs.iter().map(|dir| format!("-I{}", dir.display())));
    args.extend(flags.defines.iter().map(|define| format!("-D{define}")));
    // `DZ_CXXFLAGS`: extra flags for Dazzle's own code and the link, last so they win. It
    // is how CI builds every program under AddressSanitizer and UBSan (T-0132). Split on
    // whitespace, with no quoting: a flag holding a space is not supported.
    if let Ok(extra) = std::env::var("DZ_CXXFLAGS") {
        args.extend(extra.split_whitespace().map(str::to_owned));
    }

    // Only what this build emitted: `build/cpp/` can still hold the `.cpp` of a module that
    // has been renamed or removed, and compiling it fails against a runtime it was not
    // generated for, with an error naming source that no longer exists (T-0087). Nothing in
    // the folder is deleted, since a user may have put something there.
    //
    // The runtime and `std` are compiled from their shared copies, into objects every project
    // shares (T-0182).
    let files: Vec<PathBuf> =
        out.files.iter().map(|f| out.shared.copy_of(f).unwrap_or(f).clone()).collect();
    let mut sources: Vec<&PathBuf> =
        files.iter().filter(|p| p.extension().is_some_and(|x| x == "cpp")).collect();
    sources.sort();
    // What each of them may start from, precompiled (T-0181).
    let std_h = files.iter().find(|p| p.file_name().is_some_and(|n| n == "std.h"));
    let sources: Vec<(&PathBuf, Start)> = sources
        .into_iter()
        .map(|s| (s, if std_h.is_some() { start_of(s, &files) } else { Start::Nothing }))
        .collect();

    let link = ws.link_inputs();
    let mut link_args: Vec<String> = objects.iter().map(|o| o.display().to_string()).collect();
    link_args.extend(link.dirs.iter().map(|dir| format!("-L{}", dir.display())));
    link_args.extend(link.libs.iter().map(|lib| format!("-l{lib}")));
    // macOS system libraries are frameworks, which `-l` does not find (spec §15.1).
    for f in &link.frameworks {
        link_args.extend(["-framework".to_owned(), f.clone()]);
    }
    // What a CMake install's pkg-config file asks for besides libraries: `-pthread`.
    link_args.extend(link.flags.iter().cloned());

    // An optimised build is optimised across modules too (T-0154): each module is a
    // translation unit, and without this a call into `std` that is not `#[inline]` (spec
    // §12.8) is never inlined. `=auto` runs the link-time work in parallel, and keeps g++
    // from printing a note about running it serially. Only Dazzle's code: the vendored
    // libraries above stay as their recipes build them.
    let mut lto = module.build.optimize != Optimize::Debug;
    let build = |lto: bool, pch: bool, timings: &mut dz_driver::Timings| {
        let mut args = args.clone();
        if lto {
            args.push("-flto=auto".to_owned());
        }
        let std_h = std_h.filter(|_| pch).map(PathBuf::as_path);
        let dirs = Dirs { own: root.join("obj").join("dz"), shared: &out.shared };
        compile_and_link(&cxx, &sources, std_h, &args, &link_args, &dirs, &exe, timings)
    };
    // Not every toolchain links with it: LLVM's clang targeting MSVC needs `lld`, for one.
    // A link that fails with it is tried once without it, and says so. Where that works, it
    // is remembered, for this compiler and these flags, so later builds do not compile
    // everything twice for a link known to fail (T-0183). Only then: a missing definition
    // fails the link without it too, and that says nothing about the toolchain.
    let no_lto = {
        let mut key = Vec::new();
        for part in std::iter::once(cxx.as_str()).chain(args.iter().map(String::as_str)) {
            key.extend_from_slice(part.as_bytes());
            key.push(0);
        }
        out.shared.obj_dir().join(format!("no-lto-{:016x}", crate::native::fnv1a(&key)))
    };
    let remembered = lto && no_lto.exists();
    let mut output = build(lto && !remembered, true, timings);
    if remembered {
        lto = false;
        if output.is_ok() {
            eprintln!(
                "note: built without link-time optimisation: {cxx} could not link with \
                 `-flto=auto` before; delete {} to try again",
                no_lto.display()
            );
        }
    } else if let Some(why) = match &output {
        Err(Failed::Rejected(stderr)) if lto && linker_failure(stderr) => {
            Some(stderr.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim().to_owned())
        }
        _ => None,
    } {
        lto = false;
        output = build(false, true, timings);
        if output.is_ok() {
            let _ = crate::shared::write(&no_lto, format!("{why}\n").as_bytes());
            eprintln!(
                "note: built without link-time optimisation: {cxx} could not link with \
                 `-flto=auto`"
            );
        }
    }
    // A precompiled header is an optimisation, so it is never the reason a build fails
    // (T-0181): a file compiled from it that fails is compiled again without it, whatever the
    // compiler said. g++ quietly ignores a header that does not fit, but clang++ stops, and
    // its words for a damaged one do not always name it.
    if matches!(&output, Err(Failed::FromPch(_))) {
        output = build(lto, false, timings);
        if output.is_ok() {
            eprintln!("note: built without a precompiled header: {cxx} could not use it");
        }
    }
    // Said only of the build that made the program, not of one tried and dropped above.
    if let Ok(Some(note)) = &output {
        eprintln!("note: {note}");
    }

    if let Err(failed) = output {
        let stderr = match failed {
            Failed::Spawn(message) => {
                diags.push(Diagnostic::error(codes::CPP_COMPILER_FAILED, message));
                return Err(diags);
            }
            Failed::Rejected(stderr) | Failed::FromPch(stderr) => stderr,
        };
        // A *compile* error in generated code is a compiler bug (invariant 7). A *link*
        // error is almost never one: the code was accepted, and what is missing is a
        // definition — most often a vendored library whose implementation nothing compiles.
        // Saying "this is a bug in the Dazzle compiler" for that sends the reader to the
        // wrong place entirely, so the two are told apart.
        let d = if linker_failure(&stderr) {
            Diagnostic::error(
                codes::CPP_COMPILER_FAILED,
                "the linker could not find something the program calls",
            )
            .with_note(format!("using {cxx}"))
            .with_help(
                "the declarations were found but nothing defines them. For a vendored \
                 library, check its `[cpp.libs.<name>]` recipe compiles the right \
                 `sources` — a single-header library needs one source that defines its \
                 `*_IMPLEMENTATION` macro, since a library's `defines` build its own \
                 sources and nothing else — and that `link_libs` names every system \
                 library it needs (spec §15.1)",
            )
        } else {
            Diagnostic::error(
                codes::CPP_COMPILER_FAILED,
                "the C++ compiler rejected the generated code",
            )
            .with_note(format!("using {cxx}"))
            // A C++ error here is a compiler bug, not a user error. `#line` directives
            // mean it still points at the user's `.dz` source (spec §16.3).
            .with_help(
                "this is a bug in the Dazzle compiler: generated code should always \
                 compile. The C++ output is in build/cpp; `dz cpp` regenerates it",
            )
        };
        diags.push(d.with_note(stderr.trim().to_owned()));
        return Err(diags);
    }

    Ok(exe)
}

/// Why the generated code did not become an executable.
enum Failed {
    /// The compiler could not be run, or its output not stored: the message says which.
    Spawn(String),
    /// The compiler or the linker refused: its standard error.
    Rejected(String),
    /// The compiler refused a file compiled from the precompiled `std.h`, which may be the
    /// header's fault rather than the file's: its standard error.
    FromPch(String),
}

/// Compile each generated file to an object of its own, then link them. The program's own
/// objects go in `build/obj/dz/`; the runtime's and `std`'s go in their shared folder, where
/// every project finds them already compiled (T-0182). `dirs` says which is which.
///
/// An object whose inputs have not changed is reused, judged by content as the vendored
/// libraries' objects are (`native::up_to_date`), so a rebuild compiles only the files an
/// edit reached, and an unchanged `std` is not compiled again (T-0102). The object's name
/// carries the compiler and every flag, so a debug build never links a release object.
/// Each object is compiled beside its name and renamed onto it, since another `dz` may be
/// reading a shared one.
///
/// `sources` says what each file can start from, precompiled, and `std_h` is the generated
/// `std.h`, or `None` to compile without precompiled headers (T-0181). They live in a
/// `pch-<key>/` folder beside the objects of what they include:
///
/// - **`std.h`**, the runtime and `std`, about 0.5 MB of C++, in the shared folder. Every
///   module's `.cpp` and `dz_main.cpp` use it, and it is built when one of them needs
///   compiling. `std.cpp` itself does not, so on a first build it compiles while the header
///   does.
/// - **`std.h` and a module's C and C++ headers** (`<SDL3/SDL.h>`, `<imgui.h>`), one for
///   each distinct set, so modules that import the same libraries share one. In
///   `examples/sdl3` those headers are two thirds of what a recompile parses. This one is
///   slower to build than a file is to compile with the first (0.5 s for SDL3 and ImGui on
///   clang++, 1.5 s on g++), so it is built *alongside*: a file whose set is not built yet is
///   compiled from `std.h` while its set's header is built, and uses it from the next build
///   on. A build from clean, or one that never edits the module, never waits for it.
///
/// A header that could not be built is a note for the caller to print if this build is the
/// one kept.
#[allow(clippy::too_many_arguments)]
fn compile_and_link(
    cxx: &str,
    sources: &[(&PathBuf, Start)],
    std_h: Option<&Path>,
    args: &[String],
    link_args: &[String],
    dirs: &Dirs,
    exe: &Path,
    timings: &mut dz_driver::Timings,
) -> Result<Option<String>, Failed> {
    let started = std::time::Instant::now();
    let dir = &dirs.own;
    for dir in [dir, &dirs.shared.obj_dir()] {
        std::fs::create_dir_all(dir)
            .map_err(|e| Failed::Spawn(format!("cannot create {}: {e}", dir.display())))?;
    }
    let std_h = std_h.filter(|_| sources.iter().any(|(_, start)| *start != Start::Nothing));
    let std_pch = match std_h {
        Some(std_h) => Some(Pch::prepare(cxx, args, &dirs.of(std_h), &include_of(std_h))?),
        None => None,
    };

    // Which header each file is compiled from, and so its command and its object's name.
    // `None` is none.
    let mut sets: Vec<(String, Pch, bool)> = Vec::new();
    let mut wanted = vec![];
    let mut files = Vec::new();
    for &(source, ref start) in sources {
        let from = match (start, &std_pch) {
            (Start::Nothing, _) | (_, None) => None,
            (Start::Std, Some(_)) => Some(Header::Std),
            (Start::Headers(text), Some(_)) => {
                let i = match sets.iter().position(|(t, _, _)| t == text) {
                    Some(i) => i,
                    None => {
                        let pch = Pch::prepare(cxx, args, dir, text)?;
                        let fresh = pch.fresh();
                        sets.push((text.clone(), pch, fresh));
                        sets.len() - 1
                    }
                };
                if sets[i].2 {
                    Some(Header::Set(i))
                } else {
                    // Not built yet: `std.h`'s this time, and the set's is built if this
                    // file needs compiling.
                    wanted.push((i, source));
                    Some(Header::Std)
                }
            }
        };
        files.push((source.as_path(), from));
    }
    let pch_of = |from: Option<Header>| -> Option<&Pch> {
        match from? {
            Header::Std => std_pch.as_ref(),
            Header::Set(i) => Some(&sets[i].1),
        }
    };
    let args_of = |pch: Option<&Pch>| -> Vec<String> {
        let mut with = args.to_vec();
        if let Some(pch) = pch {
            with.extend(["-include".to_owned(), pch.header.display().to_string()]);
        }
        with
    };
    // Each file's object, and whether it needs compiling.
    let mut jobs_of: Vec<Job> = files
        .into_iter()
        .map(|(source, from)| {
            let file_args = args_of(pch_of(from));
            let object = object_path(&dirs.of(source), cxx, source, &file_args);
            let stale = !crate::native::up_to_date(&object);
            if stale && matches!(from, Some(Header::Set(_))) {
                // Its set's header has just been built, while the file has not changed since
                // it was compiled from `std.h`'s: that object will do until it does.
                let std_args = args_of(std_pch.as_ref());
                let std_object = object_path(&dirs.of(source), cxx, source, &std_args);
                if crate::native::up_to_date(&std_object) {
                    return (source, Some(Header::Std), std_args, std_object, false);
                }
            }
            (source, from, file_args, object, stale)
        })
        .collect();
    let stale_source =
        |source: &Path| jobs_of.iter().any(|(s, _, _, _, stale)| *s == source && *stale);

    // `std.h`'s header first, if a file compiled from it needs compiling; the sets' headers
    // alongside, for the next build.
    let mut jobs = Jobs::new(cxx);
    let mut std_child = None;
    if let Some(pch) = &std_pch {
        let needed =
            jobs_of.iter().any(|(_, from, _, _, stale)| *from == Some(Header::Std) && *stale);
        if needed && !pch.fresh() {
            std_child = Some(pch.spawn(cxx, args)?);
        }
    }
    let mut set_children = Vec::new();
    for (i, source) in wanted {
        if stale_source(source) && !set_children.iter().any(|(j, _)| *j == i) {
            set_children.push((i, sets[i].1.spawn(cxx, args)?));
        }
    }

    // The files that need no header being built start now.
    for (source, from, file_args, object, stale) in &jobs_of {
        if *stale && *from != Some(Header::Std) {
            jobs.start(file_args, source, object, pch_of(*from).map(|p| p.compiled.clone()))?;
        }
    }
    let waiting: Vec<bool> =
        jobs_of.iter().map(|(_, from, ..)| *from == Some(Header::Std)).collect();
    let mut note = None;
    if let (Some(pch), Some(child)) = (&std_pch, std_child) {
        let out = child
            .wait_with_output()
            .map_err(|e| Failed::Spawn(format!("could not run {cxx}: {e}")))?;
        if !pch.finish(out.status.success()) {
            note = Some(why_not(cxx, "std.h", &out.stderr));
            // Its files are compiled without one.
            for (source, from, file_args, object, stale) in &mut jobs_of {
                if *from == Some(Header::Std) {
                    *from = None;
                    *file_args = args.to_vec();
                    *object = object_path(&dirs.of(source), cxx, source, file_args);
                    *stale = !crate::native::up_to_date(object);
                }
            }
        }
    }
    let mut objects = Vec::new();
    for ((source, from, file_args, object, stale), waited) in jobs_of.into_iter().zip(waiting) {
        if stale && waited {
            jobs.start(&file_args, source, &object, pch_of(from).map(|p| p.compiled.clone()))?;
        }
        objects.push(object);
    }
    let (errors, refused, kept) = jobs.finish()?;
    let kept_any = !kept.is_empty();
    // An object another process had open, so its fresh copy could not replace it.
    for object in &mut objects {
        if let Some((_, temp)) = kept.iter().find(|(o, _)| o == object) {
            *object = temp.clone();
        }
    }
    for (i, child) in set_children {
        let pch = &sets[i].1;
        let out = child
            .wait_with_output()
            .map_err(|e| Failed::Spawn(format!("could not run {cxx}: {e}")))?;
        if !pch.finish(out.status.success()) && note.is_none() {
            note = Some(why_not(cxx, "a module's C and C++ headers", &out.stderr));
        }
    }
    let started = timings.lap("C++ compiler", started);
    if !refused.is_empty() {
        // Built again next time, rather than refused again; `compile` tries this build
        // once more without them.
        for compiled in refused {
            let _ = std::fs::remove_file(compiled.with_extension("stamp"));
        }
        return Err(Failed::FromPch(errors));
    }
    if !errors.is_empty() {
        return Err(Failed::Rejected(errors));
    }

    // Linked again only if something it is linked from changed, as `make` does: the
    // command, and the size and time of every file it names. 0.4 s of a build with nothing
    // changed in a 100-module project (T-0012).
    let link_stamp = dirs.own.join(format!(
        "link-{}.stamp",
        exe.file_stem().map(|s| s.to_string_lossy()).unwrap_or_default()
    ));
    let link_key = {
        let mut key = format!("{cxx}\n{}\n", exe.display());
        let objects = objects.iter().map(|o| o.display().to_string());
        for part in args.iter().cloned().chain(objects).chain(link_args.iter().cloned()) {
            // A file's size and time too, so an object compiled again is another key.
            let meta = std::fs::metadata(&part).ok();
            let time = meta.as_ref().and_then(|m| m.modified().ok());
            let time = time.and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok());
            let size = meta.map_or(0, |m| m.len());
            key.push_str(&format!("{part} {size} {}\n", time.map_or(0, |t| t.as_nanos())));
        }
        key
    };
    if exe.exists() && std::fs::read_to_string(&link_stamp).is_ok_and(|k| k == link_key) {
        timings.lap("link", started);
        return Ok(note);
    }
    // Gone until the link succeeds, so a link cut short is never taken for a good one.
    let _ = std::fs::remove_file(&link_stamp);
    let output = Command::new(cxx)
        .args(args)
        .args(&objects)
        .args(link_args)
        .arg("-o")
        .arg(exe)
        .output()
        .map_err(|e| Failed::Spawn(format!("could not run {cxx}: {e}")))?;
    timings.lap("link", started);
    for (_, temp) in kept {
        let _ = std::fs::remove_file(temp);
    }
    if output.status.success() {
        // Not when an object was a temporary copy, which is gone now.
        if kept_any {
            return Ok(note);
        }
        let _ = std::fs::write(&link_stamp, &link_key);
        Ok(note)
    } else {
        Err(Failed::Rejected(String::from_utf8_lossy(&output.stderr).into_owned()))
    }
}

/// Where `source`'s object goes: its name carries the compiler and every flag, so objects
/// built different ways never stand in for each other.
fn object_path(dir: &Path, cxx: &str, source: &Path, args: &[String]) -> PathBuf {
    let mut key = Vec::new();
    let source_name = source.to_string_lossy();
    for part in [cxx, &source_name].into_iter().chain(args.iter().map(String::as_str)) {
        key.extend_from_slice(part.as_bytes());
        key.push(0);
    }
    let stem = source.file_stem().map(|s| s.to_string_lossy()).unwrap_or_default();
    dir.join(format!("{stem}-{:016x}.o", crate::native::fnv1a(&key)))
}

/// The C++ compiles running at once, no more than there are cores.
struct Jobs<'a> {
    cxx: &'a str,
    cores: usize,
    /// Each compile running: its source, its object, the precompiled header it starts from,
    /// if any, and the compiler.
    running: std::collections::VecDeque<(PathBuf, PathBuf, Option<PathBuf>, std::process::Child)>,
    /// What the compiler said about each file it refused.
    errors: String,
    /// The precompiled headers those files started from.
    refused: Vec<PathBuf>,
    /// Objects compiled but not renamed into place, since another process had the old one
    /// open, which Windows refuses to replace: each object, and where its new copy is.
    kept: Kept,
}

impl<'a> Jobs<'a> {
    fn new(cxx: &'a str) -> Self {
        let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
        Jobs {
            cxx,
            cores,
            running: Default::default(),
            errors: String::new(),
            refused: vec![],
            kept: vec![],
        }
    }

    /// Compile `source` to `object`, once fewer than `cores` compiles are running. `pch` is
    /// the precompiled header its `args` include, if any.
    fn start(
        &mut self,
        args: &[String],
        source: &Path,
        object: &Path,
        pch: Option<PathBuf>,
    ) -> Result<(), Failed> {
        while self.running.len() >= self.cores {
            self.finish_one()?;
        }
        // Written beside the object and renamed onto it once complete: the shared objects
        // are read and written by every `dz` running (T-0182).
        let child = Command::new(self.cxx)
            .args(args)
            .arg("-c")
            .arg(source)
            .arg("-o")
            .arg(crate::shared::temp_of(object))
            .args(["-MD", "-MF"])
            .arg(crate::shared::temp_of(&object.with_extension("d")))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| Failed::Spawn(format!("could not run {}: {e}", self.cxx)))?;
        self.running.push_back((source.to_path_buf(), object.to_path_buf(), pch, child));
        Ok(())
    }

    /// Wait for the compile started first.
    fn finish_one(&mut self) -> Result<(), Failed> {
        let Some((source, object, pch, child)) = self.running.pop_front() else { return Ok(()) };
        let out = child
            .wait_with_output()
            .map_err(|e| Failed::Spawn(format!("could not run {}: {e}", self.cxx)))?;
        let temp = crate::shared::temp_of(&object);
        if out.status.success() {
            match put_in_place(&object, &source) {
                Ok(()) => {}
                Err(_) if crate::native::up_to_date(&object) => {
                    let _ = std::fs::remove_file(&temp);
                }
                Err(_) => self.kept.push((object, temp)),
            }
        } else {
            let _ = std::fs::remove_file(&temp);
            let _ = std::fs::remove_file(crate::shared::temp_of(&object.with_extension("d")));
            self.errors.push_str(&String::from_utf8_lossy(&out.stderr));
            self.refused.extend(pch);
        }
        Ok(())
    }

    /// Wait for every compile, and return what the compiler said about those it refused,
    /// the precompiled headers they started from, and the objects `kept` names.
    fn finish(mut self) -> Result<(String, Vec<PathBuf>, Kept), Failed> {
        while !self.running.is_empty() {
            self.finish_one()?;
        }
        Ok((self.errors, self.refused, self.kept))
    }
}

/// Rename `output`, just compiled from `source` beside its final name, onto it, with its
/// depfile, and record its stamp. The stamp goes last: until it is written, the old one says
/// what the old output was built from, or there is none.
fn put_in_place(output: &Path, source: &Path) -> std::io::Result<()> {
    let depfile = output.with_extension("d");
    std::fs::rename(crate::shared::temp_of(&depfile), &depfile)?;
    std::fs::rename(crate::shared::temp_of(output), output)?;
    crate::native::write_stamp(source, output);
    Ok(())
}

/// A precompiled header (T-0181): `std.h`, or `std.h` and a module's C and C++ headers.
///
/// `header` is a short header that includes them, and `compiled` sits beside it as
/// `std.h.gch`. A file compiled with `-include <header>` then starts from the precompiled
/// form: g++ looks for `<header>.gch` itself, and clang++'s driver looks for it too and
/// passes it on as `-include-pch`. Where neither can use it, `-include` still includes the
/// real headers, so the file compiles the same either way. The folder is named for the
/// compiler, the flags and what it includes, since a header precompiled one way cannot be
/// used another.
struct Pch {
    header: PathBuf,
    compiled: PathBuf,
}

impl Pch {
    /// Write the header holding `includes`, if it is not already there as it should be.
    fn prepare(cxx: &str, args: &[String], dir: &Path, includes: &str) -> Result<Pch, Failed> {
        let mut key = Vec::new();
        let parts = std::iter::once(cxx).chain(args.iter().map(String::as_str));
        for part in parts.chain(std::iter::once(includes)) {
            key.extend_from_slice(part.as_bytes());
            key.push(0);
        }
        let folder = dir.join(format!("pch-{:016x}", crate::native::fnv1a(&key)));
        std::fs::create_dir_all(&folder)
            .map_err(|e| Failed::Spawn(format!("cannot create {}: {e}", folder.display())))?;
        let header = folder.join("std.h");
        let text = format!("// Generated by the Dazzle compiler. Do not edit.\n{includes}");
        // Rewritten only when it changes: clang++ refuses a precompiled header whose inputs
        // are newer than it, even with the same contents.
        crate::shared::write(&header, text.as_bytes())
            .map_err(|e| Failed::Spawn(format!("cannot write {}: {e}", header.display())))?;
        let compiled = folder.join("std.h.gch");
        Ok(Pch { header, compiled })
    }

    /// Is the precompiled header there, built from what its inputs hold now, and newer than
    /// each of them? The contents are what the objects are judged by; the time is for
    /// clang++, which checks it and refuses the header if an input was touched since.
    fn fresh(&self) -> bool {
        if !crate::native::up_to_date(&self.compiled) {
            return false;
        }
        let Ok(built) = std::fs::metadata(&self.compiled).and_then(|m| m.modified()) else {
            return false;
        };
        let Ok(stamp) = std::fs::read_to_string(self.compiled.with_extension("stamp")) else {
            return false;
        };
        crate::native::stamp_paths(&stamp).iter().all(|path| {
            std::fs::metadata(path).and_then(|m| m.modified()).is_ok_and(|time| time <= built)
        })
    }

    /// Build it beside its final name; `finish` puts it in place. A build cut short leaves the
    /// old one as it was, and two `dz` processes building it at once each write their own.
    fn spawn(&self, cxx: &str, args: &[String]) -> Result<std::process::Child, Failed> {
        Command::new(cxx)
            .args(args)
            .args(["-x", "c++-header"])
            .arg(&self.header)
            .arg("-o")
            .arg(crate::shared::temp_of(&self.compiled))
            .args(["-MD", "-MF"])
            .arg(crate::shared::temp_of(&self.compiled.with_extension("d")))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| Failed::Spawn(format!("could not run {cxx}: {e}")))
    }

    /// After `spawn`'s compiler has finished, `built` saying whether it succeeded: put what
    /// it built in place, and say whether a header good to use is there now. It may be
    /// another process's, if this one could not replace it.
    fn finish(&self, built: bool) -> bool {
        if built && put_in_place(&self.compiled, &self.header).is_ok() {
            return true;
        }
        let _ = std::fs::remove_file(crate::shared::temp_of(&self.compiled));
        let _ = std::fs::remove_file(crate::shared::temp_of(&self.compiled.with_extension("d")));
        built && self.fresh()
    }
}

/// Where objects and precompiled headers go: the project's own in `build/obj/dz/`, the
/// shared files' in their shared folder (T-0182).
struct Dirs<'a> {
    own: PathBuf,
    shared: &'a crate::shared::Shared,
}

impl Dirs<'_> {
    /// The folder for `source`'s object, or for a header precompiled from it.
    fn of(&self, source: &Path) -> PathBuf {
        if self.shared.holds(source) {
            self.shared.obj_dir()
        } else {
            self.own.clone()
        }
    }
}

/// What a generated `.cpp` can be compiled from, precompiled (T-0181).
#[derive(Clone, PartialEq)]
enum Start {
    /// Nothing: it does not start with `std.h`, or is `std.cpp`, which gains little from it
    /// and on a build from clean compiles while the header is built.
    Nothing,
    /// `std.h`.
    Std,
    /// `std.h` and the C and C++ headers its module imports: the `#include` lines of a
    /// header that includes them, in the order its module's header does.
    Headers(String),
}

/// The precompiled header a file is compiled from, in `compile_and_link`.
#[derive(Clone, Copy, PartialEq)]
enum Header {
    /// `std.h`'s.
    Std,
    /// The one for `std.h` and a set of C and C++ headers: an index into its `sets`.
    Set(usize),
}

/// A file to compile: its source, the header it starts from, its command's flags, its
/// object, and whether that object needs compiling.
type Job<'a> = (&'a Path, Option<Header>, Vec<String>, PathBuf, bool);

/// Objects compiled but left beside their final names, in `Jobs`: each object, and its copy.
type Kept = Vec<(PathBuf, PathBuf)>;

/// An `#include` of `path`, a generated file, by its absolute path, since the header that
/// holds it is in another folder.
fn include_of(path: &Path) -> String {
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().map(|d| d.join(path)).unwrap_or_else(|_| path.to_path_buf())
    };
    format!("#include \"{}\"\n", path.display().to_string().replace('\\', "/"))
}

/// What `source` can start from. A module's `.cpp` and `dz_main.cpp` start by including a
/// module's header, which starts with the runtime, `std.h`, and then the module's C and C++
/// headers (`<SDL3/SDL.h>`), all before a line of its own. Including that run of headers
/// before anything else, precompiled, changes nothing. It ends at the first line that is
/// not one of them, such as another module's header, which changes with an edit.
fn start_of(source: &Path, files: &[PathBuf]) -> Start {
    // Lines that are neither blank, a comment, nor a module header's include guard.
    fn lines(text: &str) -> impl Iterator<Item = &str> {
        text.lines().map(str::trim).filter(|line| {
            let guard = |head: &str| {
                line.strip_prefix(head).is_some_and(|m| m.starts_with("DZ_") && m.ends_with("_H"))
            };
            !(line.is_empty() || line.starts_with("//") || guard("#ifndef ") || guard("#define "))
        })
    }
    fn quoted(line: &str) -> Option<&str> {
        line.strip_prefix("#include")?.trim().strip_prefix('"')?.split('"').next()
    }
    let generated = |name: &str| files.iter().find(|f| f.file_name().is_some_and(|n| n == name));
    let Ok(text) = std::fs::read_to_string(source) else { return Start::Nothing };
    let first = lines(&text).next().and_then(quoted);
    let Some(header) = first.filter(|&name| name != "std.h").and_then(generated) else {
        return Start::Nothing;
    };
    let Ok(text) = std::fs::read_to_string(header) else { return Start::Nothing };
    let (mut std, mut includes, mut libraries) = (false, String::new(), false);
    for line in lines(&text) {
        match quoted(line) {
            Some(name @ ("dz_runtime.h" | "dz_prelude.h" | "std.h")) if !libraries => {
                let Some(path) = generated(name) else { break };
                includes.push_str(&include_of(path));
                std |= name == "std.h";
            }
            None if std
                && line.strip_prefix("#include").is_some_and(|n| n.trim().starts_with('<')) =>
            {
                includes.push_str(line);
                includes.push('\n');
                libraries = true;
            }
            _ => break,
        }
    }
    match (std, libraries) {
        (false, _) => Start::Nothing,
        (true, false) => Start::Std,
        (true, true) => Start::Headers(includes),
    }
}

/// The note for a precompiled header `cxx` could not build, from its standard error.
fn why_not(cxx: &str, what: &str, stderr: &[u8]) -> String {
    let stderr = String::from_utf8_lossy(stderr);
    let first = stderr.lines().find(|l| !l.trim().is_empty()).unwrap_or("no reason given");
    format!("compiled without precompiling {what}: {cxx} could not: {}", first.trim())
}

pub fn run(exe: &Path) -> std::io::Result<std::process::ExitStatus> {
    Command::new(exe).status()
}

fn file_stem(module: &str) -> String {
    module.replace("::", "_")
}

/// Did the C++ compiler get as far as linking, and fail there?
///
/// Each linker says so in its own words: MSVC's `link.exe` reports `LNK2019`/`LNK1120` for
/// an unresolved symbol, the GNU and LLVM linkers "undefined reference" or "undefined
/// symbol", and every driver prints a "linker command failed" line of its own.
fn linker_failure(stderr: &str) -> bool {
    const MARKS: &[&str] = &[
        "LNK2001",
        "LNK2019",
        "LNK1120",
        "undefined reference",
        "undefined symbol",
        "linker command failed",
        "ld returned",
        // clang's driver, before any linker runs, when its default linker cannot do LTO.
        "LTO requires",
    ];
    MARKS.iter().any(|m| stderr.contains(m))
}
