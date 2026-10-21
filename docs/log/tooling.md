# Tooling — build modes, `dz test` (M14), `dz bench` (M15), CI

Part of the build log in `docs/log/` — what was built and decided, kept as the record. Open work is in `docs/tasks/`; nothing here needs reading to decide what to do next.

## Raised on 2026-09-12

Struct inheritance is done (r30). What follows is the tooling the language needs before it
can be worked in seriously, raised by the user on 2026-09-12 and sized here against what
the compiler actually does today.

### Already there, and what was added

- **Build modes were already in the manifest** — `[build] optimize = "debug" | "release" |
  "shipping"`, reaching the C++ compiler as `-O0 -g` / `-O2` / `-O3 -DNDEBUG`. What was
  missing was a way to choose one without editing the manifest, so `dz run`, `dz build` and
  `dz cpp` now take `--debug` / `--release` / `--shipping`. The override applies to **every
  module**, not just the root: one command builds one program, and a dependency left at a
  different optimisation level is the classic way to get a binary that matches neither
  setting. **Done.**
- **Submodules already work, in exactly the shape asked about.** A package with a `libs/`
  folder of Dazzle modules is `[dependencies] engine = { path = "libs/engine" }`, with the
  dependency graph, cycle detection and per-module visibility already built on it —
  `tests/workspace/good` *is* a `game` with `libs/engine` and `libs/math`. Include dirs,
  defines and `[cpp.libs]` recipes are inherited through those edges too (r19), so a Dazzle
  module wrapping a C++ library passes its build settings to whatever depends on it.
  **Nothing to do.**

### M14 — `dz test` ✅ done

`#[test] fn name() { … }`, written beside the code it tests. It passes by returning and
fails by panicking, which needed no new machinery: `panic` already aborts with a location
(§9.8).

- **One process per test.** A panic aborts and there are no exceptions to catch (§16.5), so
  a runner that called every test in one process would stop at the first failure and report
  nothing about the rest. The generated entry point therefore runs *one* test per
  invocation, named on the command line, and lists them with `--list`; `dz test` lists,
  then spawns once per name. It costs a process per test and buys a complete report — and
  isolates a test that corrupts memory from the ones after it.
- **A normal build leaves the tests out entirely**, which is the half that is easy to lose:
  `dz_cpp::Mode::Program` skips a `#[test]` item, `Mode::Tests` emits it and generates the
  runner instead of calling the program's `main`. Both halves are asserted — the fixture's
  `expected.txt` covers the program side and `dz_test_runs_the_projects_tests` greps the
  generated C++ for a test's name.
- The test binary builds into `build/test/`, so `dz test` never overwrites the program's
  objects and a later `dz run` does not relink a test entry point.
- `dz test [path] [filter]` filters by substring; the build-mode flags work here too, so
  `dz test . --release` is how you check that a release build still passes.
- **Where tests live was a decision, and a deliberately reversible one.** Beside the code
  needs no manifest change — `sources` already globs `src/**` — and adding a `tests/`
  source root later is purely additive rather than a reversal.

The one thing still missing is `assert`, which is typed but not emitted (a deferral-register row, M12);
until then a test says what went wrong with `panic`, which is what the runner reports.


1. `#[test]` on a `fn` with no parameters and no return, checked as one: a test that takes
   an argument or returns a value is an error at the attribute, not at the call.
2. Tests are **excluded from a normal build** — `dz build` must not link them — and are the
   only entry points `dz test` generates a `main` for. That means a second codegen
   configuration, which is also what M15 needs.
3. A failure is a `panic` (§9.8), which already aborts with a location, so the first
   version needs no assertion machinery beyond `assert` becoming real (a deferral-register row, M12).
4. Reporting: name, pass/fail, counts, non-zero exit. Running each test in its own process
   is what makes a panic recoverable; doing it in-process is faster and cannot survive the
   first abort. Start with one process per binary and a fail-fast report, since that is
   what a compiler's own test suite needs first.

**Open question for the user:** where do tests live? Beside the code in the same file is
simplest and keeps a test next to what it tests; a `tests/` directory per module matches
the way this repo's own corpora are organised. It changes what `sources` means, so it is a
manifest decision rather than a compiler one.

### M15 — `dz bench`, and what the guarantees cost ✅ done

`#[bench] fn name() { … }`, timed by `dz bench`. The same shape as `#[test]` and sharing
its machinery: `dz_cpp::Mode` picks which attribute a build keeps, and a program keeps
neither.

**What makes a measurement trustworthy, and what was needed for each:**

- **`--release` by default.** Timing `-O0` measures a build nobody ships. `dz bench
  --debug` is still there for anyone who wants to know what the debug build costs.
- **A calibrated iteration count.** Timing a cheap body once measures the clock, so the
  count doubles until a batch lasts 50ms.
- **The fastest of five batches**, not the mean. A machine running other work perturbs a
  batch upward and never downward, so the minimum is the closest thing to the body's own
  cost that a shared machine can show. This was not a theoretical worry: before it, one
  pair reported 550ns and 1171ns for the same work on consecutive runs.
- **`blackbox`, a new builtin** (§21 r31). A benchmark has two ways to measure nothing —
  its result is never read, so the work computing it may be deleted; its input is usually
  a literal, so the answer may be folded at compile time. A *void* barrier only fixes the
  first, and the first version of this was exactly that: it reported 5.40ns and 5.39ns for
  loops of 1000 and 10 iterations. Returning the value fixes both.
- **A monotonic clock** in the shim, `now_nanos`, defined in the `.cpp` so `<chrono>`
  reaches one translation unit rather than every build.

### The comparison suite, and what it says

`tests/bench` is the point of the milestone. Each benchmark is a **pair** — one Dazzle,
one calling hand-written C++ vendored in `vendor/baseline` — compiled by `dz` at the same
optimisation level into the same binary and timed by the same clock. `main` prints each
pair's two results side by side, and `the_benchmark_suite_agrees_with_its_cpp_baseline`
asserts they match: two benchmarks computing different things measure nothing.

Getting this honest took two corrections, both worth recording because both produced
confident wrong answers first:

1. **The baseline needed the same barrier.** With `blackbox` inside the Dazzle loop and
   nothing inside the C++ one, the compiler closed the C++ loop into a formula and reported
   3.38ns for 1024 iterations — physically impossible, and it would have been written up as
   "RAII is 166× slower in Dazzle". `barrier()` in `baseline.cpp` matches `blackbox`.
2. **The length had to be opaque on both sides.** The Dazzle half read a `const`, so it
   could specialise on a length the C++ half received at run time. Both now take it through
   `blackbox`.

With those fixed, the three pairs land **within a few percent of each other**: a `drop`
costs what a C++ destructor costs, a `virtual` call costs what a C++ virtual call costs,
and bounds-checked indexing costs no more than unchecked C++ on this machine — the check
*is* emitted (`::dz::index` is there in the generated code), the compiler simply makes it
free in this loop. Those are results from one machine and one compiler; the suite exists so
the claim can be re-checked rather than believed.

**Still owed:** a generic-instantiation pair, and somewhere to track the numbers over time
so a regression is visible. A single run is a snapshot, not a baseline.

## CI — GitHub Actions across Linux, macOS and Windows ✅ running

`.github/workflows/ci.yml`: build, clippy with `-D warnings`, the test suite, and `dz test`
and `dz bench` driven as a user would meet them, on `ubuntu-latest`, `macos-latest` and
`windows-latest`. `fail-fast` is off, because one platform failing is a fact worth having
about the other two.

**It has found eight bugs across its first runs, three of them in the compiler rather
than in the workflow** — every one of them invisible on Windows, which is what the matrix was for.

| Where | What | Why it only showed up there |
|---|---|---|
| shim | **`println("{}", xs.len)` did not compile on macOS.** `write_value` was overloaded on `int32_t`, `uint64_t` and the rest, which are *aliases*: on macOS `size_t` is `unsigned long` while `uint64_t` is `unsigned long long`, so a `usize` matched no overload exactly and the call was ambiguous | On Windows and Linux x86-64 `size_t` happens to alias one of the fixed-width names, so the set covered it by luck |
| importer | **No `import cpp` worked on macOS**: "'assert.h' file not found". The C and C++ standard headers live inside the active SDK there, and libclang does not run `xcrun` to find it the way the `clang` driver does | Only macOS puts the standard headers behind an SDK path |
| tests | **`#panic` fixtures failed on Windows.** The marker was stripped with `strip_prefix("#panic\n")` *before* normalising, and git checks `expected.txt` out with CRLF on Windows — so the marker stayed in the expected text and the test looked for the literal "#panic" in the program's own stderr | `.gitattributes` forced LF for `.dz` and `.snap` but not `expected.txt` |
| tests | **A snapshot pinned the operating system's wording.** `cannot read …: The system cannot find the path specified. (os error 3)` on Windows is `No such file or directory (os error 2)` elsewhere, so the snapshot failed everywhere it had not been recorded | The message comes from the OS, not from Dazzle |
| tests | **The compiler-precedence test passed vacuously.** `DZ_CXX` sits above the manifest in the order it checks, and CI sets it for the whole job — so the manifest levels were unobservable | It passed locally only because that machine had no `DZ_CXX` set |
| recipe | **raylib did not build on Linux or macOS.** On Linux GLFW refuses to guess a backend and stops at `#error "Cannot disable Wayland and X11 at the same time"` — it needed `_GLFW_X11` in `defines`. On macOS `rglfw.c` includes GLFW's Cocoa backend, five Objective-C `.m` files, so the translation unit has to be compiled as Objective-C despite its `.c` name | Windows selects its backend from the platform macros and has no Objective-C |
| layout | **`dz bench` could not run on Linux or macOS**: "cannot create `build/bench/cpp`: Not a directory". The built program sat in `build/`, so a module named `bench` *was* the file `build/bench` — the very directory the harness needed. The executable moved to `build/bin/` (§21 r35), leaving `build/` to directories alone | Windows names it `bench.exe`, so the two never met |
| workflow | **libclang was never found on Linux**, so every interop test reported `DZ0703` and *skipped*. `llvm-config --libdir` produced an empty string: Ubuntu installs it versioned (`llvm-config-18`) and the bare name is absent | — |

raylib needed a new recipe key to be buildable at all on macOS: **`flags`** (§21 r34),
raw compiler flags for a library's own sources. Nothing a `define` can say makes a `.c`
file compile as Objective-C, and raylib's own Makefile passes the same `-x objective-c`.

The other fixes: `write_value` and `write_hex` are now overloaded on C++'s ten **fundamental**
integer types, which are always ten distinct types and which every `intN_t`/`size_t`/
`ptrdiff_t` aliases exactly one of — so the set covers all of them on every platform, and
needs no `PRId32`-style macros to do it. The importer passes `-isysroot` from `xcrun` on
macOS. The run-test normalises before stripping, and `expected.txt` is `eol=lf` in
`.gitattributes`. The precedence test clears `DZ_CXX`, and the resolve snapshot redacts the OS's own
message — which stays in the *diagnostic*, where "permission denied" against "not
found" is something a user needs to be told. CI finds libclang by locating the
library rather than asking `llvm-config`, and **fails the job** when it cannot — a skipped
interop suite reads as a pass, which is the worst way for this to go wrong.

What writing it turned up, all of it fixed here rather than left for CI to discover:

- **`tests/run/cinterop_rgfw` could only ever link on Windows.** Its recipe listed
  `opengl32` and `gdi32` as plain `link_libs`, which apply on every platform; nothing
  else has those libraries. They are now in a `target.windows` section, with the X11 and
  Cocoa equivalents beside them.
- **macOS could not have linked either fixture**, because a macOS system library is a
  *framework* and `link_libs` becomes `-lNAME`, which finds nothing. Hence **`frameworks`**
  in a recipe (§15.1), linked as `-framework NAME`. `raylib`'s and `rgfw`'s macOS sections
  now name the ones GLFW's Cocoa backend needs; without them the link fails with undefined
  Objective-C symbols, which name nothing that looks like a missing framework.
- **No `cargo fmt --check` step**, deliberately. This repository is hand-formatted to a
  wider style than rustfmt's default: running it rewrites nearly every file, including ones
  untouched for months. Adopting rustfmt needs a `rustfmt.toml` and a one-time reformat,
  which is a decision rather than a CI step.
- The suite was run against **both** `clang++` and `g++` locally and passes on each, which
  is invariant 7's "more than one compiler" checked rather than assumed. The Linux job
  repeats the suite under `clang++` for the same reason.

Still open:

- **libclang parses with its own default target** (a deferral-register row): on Windows that is MSVC while
  `g++` builds against mingw. CI running both compilers is what should surface it.
- **Lints run on a pinned toolchain.** `-D warnings` against a floating `stable` means any
  new lint Rust ships turns CI red through no change of ours, which is what happened on the
  very first run: 1.98 added `question_mark`. `LINT_TOOLCHAIN` is bumped deliberately.

## The suite runs on every core

`cargo test` parallelises **across** test functions, never inside one, so a test that is a
loop over thousands of cases pins a single core however many the machine has. Four tests
were that shape and were three quarters of the wall time: `tolerant.rs` (~7,200 analyses),
`dz/tests/run.rs` (every project built and run, then 25 fixtures checked warning-clean),
`dz_driver/tests/matrix.rs` (232 placements) and `dz/tests/matrix.rs`. They now spread
their own work over the cores through `dz_driver/tests/parallel/mod.rs`, a work-stealing
`thread::scope` shared by `#[path]` as `feature_matrix` is. A full `cargo test --workspace`
went from **324 s to 77 s** on a 20-core machine.

This is sound because every phase is a pure function of its input (invariant 3) and the two
process-wide caches — `std_tree`'s parsed trees and `dz_cppimport`'s header cache — were
already behind a `OnceLock` and a `Mutex`.

It also exposed a race that had been latent: `hello_world_runs`, `generated_cpp_snapshot`
and `generated_cpp_carries_line_directives_back_to_dazzle` all drove `tests/run/hello/build/`
directly, which `every_program_produces_its_expected_output` builds too — and those already
ran on separate harness threads. Serial timing just made the window rare. They take scratch
copies now, for the reason `copy_fixture` had documented all along. **A test that builds a
fixture must copy it first**; the shared directory is the failure mode, not the parallelism.
