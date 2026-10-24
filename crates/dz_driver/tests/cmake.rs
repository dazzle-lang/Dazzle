//! A recipe that names its CMake build, as analysis sees it (spec §15.1, D-0090). Analysis
//! never runs CMake (`dz`'s own tests build one); it reads what a build installed, and says
//! so when nothing has been.

use std::path::{Path, PathBuf};

use dz_driver::{analyse_with, Mode};

/// A fresh project in the temp dir: `files` as (relative path, contents).
fn project(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("dz-cmake-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for (path, text) in files {
        let p = dir.join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
    dir
}

fn codes(dir: &Path, mode: Mode) -> Vec<&'static str> {
    analyse_with(dir, &|_| None, mode).diags.iter().filter_map(|d| d.code).collect()
}

const MANIFEST: &str = "[module]\nname = \"app\"\nsources = [\"src/*.dz\"]\n\n\
    [dependencies]\ntally = { cpp = \"tally\" }\n\n\
    [cpp.libs.tally]\ncmake = \"vendor/tally\"\nimport = [\"tally.h\"]\n";
const MAIN: &str = "fn main() {\n    let n: i32 = tally::tally_add(1, 2)\n}\n";
const CMAKELISTS: &str = "cmake_minimum_required(VERSION 3.16)\nproject(tally C)\n";

#[test]
fn a_cmake_library_no_build_has_installed_is_reported_as_not_built() {
    let dir = project(
        "unbuilt",
        &[
            ("dazzle.toml", MANIFEST),
            ("src/main.dz", MAIN),
            ("vendor/tally/CMakeLists.txt", CMAKELISTS),
        ],
    );
    for mode in [Mode::Build, Mode::Editor] {
        let found = codes(&dir, mode);
        assert!(found.contains(&"DZ0709"), "{mode:?}: {found:?}");
        // Not clang's "file not found", which would not say why.
        assert!(!found.contains(&"DZ0702"), "{mode:?}: {found:?}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_cmake_library_is_read_from_its_install() {
    // What a debug build of the library leaves in `build/cmake/tally/Debug/install`: the
    // header requires the define the pkg-config file gives, so it reads only if both the
    // include dir and the define came from there.
    let pc = "prefix=${pcfiledir}/../..\nincludedir=${prefix}/include\n\
              Name: tally\nLibs: -ltally\nCflags: -I${includedir} -DTALLY_OK=1\n";
    let header = "#ifndef TALLY_OK\n#error \"no define from tally.pc\"\n#endif\n\
                  #ifdef __cplusplus\nextern \"C\"\n#endif\nint tally_add(int a, int b);\n";
    let dir = project(
        "installed",
        &[
            ("dazzle.toml", MANIFEST),
            ("src/main.dz", MAIN),
            ("vendor/tally/CMakeLists.txt", CMAKELISTS),
            ("build/cmake/tally/Debug/install/include/tally.h", header),
            ("build/cmake/tally/Debug/install/lib/pkgconfig/tally.pc", ""),
        ],
    );
    // `${pcfiledir}` is pkg-config's own; this reader expands only the file's variables, so
    // the prefix is written out.
    let install = dir.join("build/cmake/tally/Debug/install");
    let pc = pc.replace("${pcfiledir}/../..", &install.to_string_lossy().replace('\\', "/"));
    std::fs::write(install.join("lib/pkgconfig/tally.pc"), pc).unwrap();

    let analysis = analyse_with(&dir, &|_| None, Mode::Build);
    if analysis.diags.iter().any(|d| d.code == Some("DZ0703")) {
        eprintln!("skipping: libclang is not available");
        return;
    }
    let found: Vec<_> = analysis.diags.iter().filter_map(|d| d.code).collect();
    assert!(!analysis.diags.has_errors(), "{found:?}");
    let ws = analysis.ws.as_ref().unwrap();
    let link = ws.link_inputs();
    assert!(link.libs.contains(&"tally".to_owned()), "{:?}", link.libs);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_cmake_recipe_refuses_the_keys_that_build_from_source() {
    let manifest = "[module]\nname = \"app\"\nsources = [\"src/*.dz\"]\n\n\
        [cpp.libs.tally]\ncmake = \"vendor/tally\"\nsources = [\"vendor/tally/*.c\"]\n\
        defines = [\"X=1\"]\n\n\
        [cpp.libs.plain]\nsources = [\"vendor/tally/*.c\"]\noptions = { X = true }\n";
    let dir = project(
        "keys",
        &[
            ("dazzle.toml", manifest),
            ("src/main.dz", "fn main() {}\n"),
            ("vendor/tally/CMakeLists.txt", CMAKELISTS),
            ("vendor/tally/tally.c", "int tally;\n"),
        ],
    );
    let analysis = analyse_with(&dir, &|_| None, Mode::Build);
    let messages: Vec<&str> = analysis
        .diags
        .iter()
        .filter(|d| d.code == Some("DZ0004"))
        .map(|d| d.message.as_str())
        .collect();
    assert_eq!(
        messages,
        [
            "`sources` cannot be used with `cmake`",
            "`defines` cannot be used with `cmake`",
            "`options` in `[cpp.libs.plain]` needs `cmake`",
        ]
    );
    let _ = std::fs::remove_dir_all(&dir);
}
