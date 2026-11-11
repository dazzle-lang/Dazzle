---
id: T-0192
title: A build with nothing changed skips analysis: the build stamp
milestone: M15
status: done
kind: feature
area: tooling
where: `crates/dz/src/fresh.rs` (`key`, `check`, `write`); `cmd_pipeline` in `crates/dz/src/main.rs`; `inputs_hold`, `refreshed` in `crates/dz/src/native.rs`; `Workspace::globs` and `glob_entries` in `crates/dz_project/src/lib.rs`; `files_read` in `crates/dz_cppimport/src/clang.rs`; tests in `crates/dz/tests/stamp.rs`
---

**Done** (2026-11-11), stage 1 of the incremental-build plan. **What was wrong:** every `dz
build` analysed the whole program before it could find that nothing needed compiling, 2 s
for T-0012's 100-module project, where `make` takes 0.05 s.

**Now** a successful `dz build` or `dz run` writes `build/obj/dz/build-<key>.stamp`. The next
one with the same key checks it before anything else. If every input still holds, it prints
the warnings that build printed and stops; `dz run` runs the program.

The **key** is what the build was asked for. Another key is another stamp file, so a debug
and a release build each keep their own. It holds:
- the `dz` executable's path, size and time, and its version, so a `dz` built again counts
  as another compiler;
- the entry as written and the folder `dz` ran from, since a program names its sources as
  the command line did (T-0188);
- the options;
- the environment variables that change a build.

The **inputs** are:
- the C++ compiler, the file its name resolves to on the `PATH`;
- the executable;
- every source pattern, expanded again;
- every CMake source tree, by its files' sizes and times;
- every file read, by size, time and hash, with the 2 s settled rule of T-0186: manifests,
  `.dz` sources, what each `import cpp` read, and every file each object was compiled from,
  taken from the objects' own stamps;
- the warnings.

A source's hash is of the bytes the build read, so an edit made during a build is seen by
the next one.

**Measured** (release `dz`, clang++): nothing changed in the 100-module project went from
2.0 s to 5 ms, `tests/run/hello` 7 ms. Writing the stamp costs a real build 20–40 ms.

**What it does not cover:** `dz test`'s binary (its harness builds through another path),
`dz cpp` and `dz check`, which analyse because showing diagnostics is their purpose.
