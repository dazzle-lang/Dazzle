---
id: T-0253
title: dz drives cl.exe: a toolchain with MSVC's own flags
milestone: M21
status: open
kind: feature
area: tooling
where: `crates/dz/src/build.rs` (`find_compiler`, the optimisation flags ~452, the fixed flags ~464–480, `-flto=auto` ~572, the precompiled header ~1206–1240, `-MD` depfiles ~1147, the linker choice ~1420–1460); `crates/dz/src/native.rs` (recipes: `-D`, `-c`, `-o`, `-MD` ~120–190; CMake already sets `CMAKE_MSVC_RUNTIME_LIBRARY` ~572)
---

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
