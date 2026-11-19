---
id: T-0253
title: dz drives cl.exe: a toolchain with MSVC's own flags
milestone: M21
status: done
kind: feature
area: tooling
where: `crates/dz/src/toolchain.rs` (every flag of both families, Visual Studio discovery with `find-msvc-tools`, `/sourceDependencies` JSON, the link); its callers: `crates/dz/src/build.rs` (`find_compiler`, `compile`, `Jobs::start`, the link in `compile_and_link`), `crates/dz/src/native.rs` (`build_libraries`, `run_jobs`, `write_stamp`, CMake's commands, `c_compiler_for`); `crates/dz_cpp/src/lib.rs` (the MSVC 14.50 array-default workaround in `struct_decl`; `float_zero` and `num::fdiv` for `0.0 / 0.0`, which `cl.exe` refuses as a constant)
---

## Done (2026-11-19)

`--cxx cl` (or `DZ_CXX=cl`) builds and runs a project from a plain shell: `dz` finds Visual
Studio with `find-msvc-tools` and gives its compiler, SDK and linker environment to child
processes. Recipe flags are translated
(`recipe_flags`). Checked with MSVC 14.38 and 14.50; the build log `docs/log/m21-msvc.md`
has what was found. `/Zc:nrvo` is passed because of T-0260.

Left:

- **Precompiled headers** under `cl.exe` (`/Yc`, `/Yu`, `/Fp`): files are compiled from the
  real headers for now, which is slower, never wrong.
- `clang-cl` speaks the same command line but is not supported: it is told apart from `cl`
  and treated as GCC's family, which it is not.

## The task as written

Asked by the user on 2026-11-18. MSVC's `cl.exe` is not supported because `dz` speaks only
GCC's command line (README, Compatibility); the generated code already compiles against
MSVC's headers and standard library, since Windows CI uses clang++ targeting MSVC.

**What it would take.** A toolchain that the build asks for each flag instead of writing
GCC's, chosen from what `find_compiler` found (`cl` or `clang-cl` speak MSVC's; clang++,
g++ and `c++` GCC's):

| GCC-style | MSVC |
|---|---|
| `-std=c++17` | `/std:c++17` |
| `-fno-exceptions` | no `/EHsc`, and `_HAS_EXCEPTIONS=0` |
| `-fno-rtti` | `/GR-` |
| `-Wall -Wextra` | `/W4` |
| `-O0 -g` / `-O2` / `-O3 -DNDEBUG` | `/Od /Zi` / `/O2` / `/O2 /DNDEBUG` |
| `-flto=auto` | `/GL` and `/LTCG` at link |
| `.gch`, `-include` | `/Yc` / `/Yu` with `/FI` |
| `-MD -MF` depfile | `/sourceDependencies <file>.json` |
| `-c -o x.o`, `-o app` | `/c /Fo:x.obj`, `/Fe:app.exe` |
| `-fuse-ld=mold/lld` | `link.exe` or `lld-link` |
| `-D`, `-I`, `-l` | `/D`, `/I`, `x.lib` |

`cl.exe` also needs its environment (`vcvars64.bat`), which `dz` should find itself through
`vswhere` rather than require a Developer Prompt. The semantic flags are T-0255's.

Recipe flags in the examples' manifests (`examples/*/dazzle.toml`) are GCC-style today; a
recipe needs a way to give each family its own, or flags `dz` translates.
