---
id: T-0076
title: A release workflow and an installer with libclang bundled
milestone: none
status: done
kind: feature
area: packaging
---

**Done 2026-11-04** (`docs/log/tooling.md`, "Releases, the installers and `dz new`"). A tag
`v0.1.N` publishes archives for x86-64 and ARM64 Linux, Apple-silicon macOS and x86-64
Windows, each smoke-tested. Linux and Windows carry LLVM's libclang and clang's headers, and
macOS uses the one in Xcode's tools. The installers are one line each from the website. What
the text below says about libclang being needed only for `import cpp` is no longer so:
`std` imports `stdlib.h`, so every build loads it (T-0101).

**Where the code is**

| Piece | Where |
|---|---|
| The workflow: build, package, smoke test, publish | `.github/workflows/release.yml` |
| Fetching libclang, the archive layout, the smoke program, `SHA256SUMS` | `tools/release.py` |
| The installers | `install/install.sh`, `install/install.ps1`; the site serves them (`scripts/sync.mjs` there) |
| Where `dz` looks for libclang and its headers | `bundled`, `find_resource_dir` in `crates/dz_cppimport/src/clang.rs` |
| `dz --version` naming a release (`DZ_RELEASE`) | the `--version` arm of `main` in `crates/dz/src/main.rs` |

The goal: **a released `dz` depends on nothing on the machine except a C++ compiler**, and
the compiler is the user's choice rather than a fixed requirement (r33 made that
configurable at four levels). What stands in the way is **libclang**, which `import cpp`
loads at run time to read C and C++ headers.

**Done now:** `dz` looks for libclang beside its own executable, or in a `lib/` next to it,
before falling back to an installed LLVM. So a release that unpacks as

```text
dz/
  bin/dz
  lib/libclang.so
```

reads headers on a machine with no LLVM. Nothing is bundled by a `cargo build`, so a
development build behaves exactly as before.

**What is left is packaging, not code.** The options, and why the shipped-beside-`dz` one
is the one to take:

| | | |
|---|---|---|
| **Ship the binary beside `dz`** | ~50–130 MB per platform, built once per release. Apache-2.0 with the LLVM exception, so redistributable with attribution | **the plan.** Fetch it in the release workflow rather than checking it into git — a 300 MB repository is its own problem |
| Link libclang statically | One binary, nothing to install | Needs LLVM built with the right flags, which every contributor building from source would then need too. Hours per build |
| Parse headers ourselves | No dependency at all | A C preprocessor plus enough C++ to read Jolt's templates is a multi-year project, and every mistake is a miscompile of somebody's engine |
| Download on first use | Small release | Still a dependency, just a later one, and it needs the network on a machine that may not have it |

Two things make this less pressing than it looks. libclang is needed **only** for
`import cpp`: a Dazzle program with no C interop never loads it. And it is needed at
*compile* time only — nothing links into the program `dz` produces.

**Owed:** a release workflow that builds `dz` for the three platforms, fetches the matching
libclang, and produces an archive in the layout above — and, per the user on 2026-09-15,
**an installer** that sets `dz` up with libclang integrated.
