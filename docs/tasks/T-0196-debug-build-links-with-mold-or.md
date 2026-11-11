---
id: T-0196
title: A debug build links with mold or lld when one is installed
milestone: M15
status: done
kind: feature
area: tooling
where: `fast_linker` and `links_with` in `crates/dz/src/build.rs`, called from `compile`; `which` in `crates/dz/src/fresh.rs`; test `a_debug_build_links_with_a_faster_linker_when_one_works` in `crates/dz/tests/run.rs`
---

**Done** (2026-11-11), stage 3c of the incremental-build plan. **What was wrong:** after
T-0195 the link was the largest part of an edit's C++: 0.44 s with GNU `ld` in the
100-module project, against 0.24 s to compile the edited file.

**Now** a debug build on Linux (any Unix but macOS) links with `-fuse-ld=mold` when `ld.mold`
is on the `PATH`, or `-fuse-ld=lld` when `ld.lld` is, if the compiler links an empty program
with it. That answer is kept in the shared cache under the compiler's and the linker's files
(path, size, time), so it is asked once per pair, again after an upgrade of either, and a
linker that is uninstalled is simply not found.

**What it leaves out:**
- optimised builds, which link with LTO, which `lld` cannot do for g++'s objects, and which
  are not what an edit waits for;
- macOS, whose linker is already fast, and Windows;
- a `-fuse-ld` of the user's own in `DZ_CXXFLAGS`, which wins.

**Not measured here:** neither linker is installed on the machine this was written on, and
the test stands one in that wraps `ld`. `mold` and `lld` are commonly several times faster
than GNU `ld`. Installing one (`apt install mold`) and running `python3
tools/bench_scale.py` shows the gain in the leaf and root edit columns.
