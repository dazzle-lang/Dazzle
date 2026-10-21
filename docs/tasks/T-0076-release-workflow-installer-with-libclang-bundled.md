---
id: T-0076
title: A release workflow and an installer with libclang bundled
milestone: none
status: open
kind: feature
area: packaging
---

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
