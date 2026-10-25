# Tooling — build modes, `dz test` (M14), `dz bench` (M15), CI

Part of the build log in `docs/log/` — what was built and decided, kept as the record. Open work is in `docs/tasks/`; nothing here needs reading to decide what to do next.

## `--timings`, and where a build's time goes (2026-10-25)

The user asked for the benchmarks to show compile times, broken down by phase. Nothing in
`dz` measured a phase, so `check`, `cpp`, `build` and `run` now take `--timings`, which
prints each phase and its time on standard error once the command is done (spec §17.3).

- `dz_driver::Timings` is a list of named durations, filled by `analyse_prepared`:
  manifests, C and C++ headers, CMake builds (only when there was one), lex and parse, `#if`
  and macros, resolve names, type check, ownership, and instantiate generics. `dz` adds
  emit C++, C and C++ libraries, and the C++ compiler. Nothing reads a duration back, so
  every phase is still a pure function of its input (invariant 3).
- Header import and CMake happen inside `dz_project::load_for`, which cannot depend on the
  driver, so the workspace carries `LoadTimes`, and the driver subtracts them from loading.
- A test (`timings_name_every_phase_on_standard_error`) pins the phase names, since
  `tools/bench_programs.py` reads them. The runner now builds each half three times from
  clean, takes the medians, and reports Dazzle's own phases, the header import and the C++
  compiler separately.

**What it showed**, on the three benchmark programs at `-O2` with g++ 16: Dazzle's own
phases take about 35 ms, and importing `stdlib.h` with libclang takes about 380 ms. The
standard library imports it for `Allocators::Global`, so every program pays that. The C++
compiler on the generated code takes about 1.4–2.4 s, against 0.3–0.5 s for the same
program written in C++, because the standard library's C++ is compiled again every time.
Both are filed: T-0101 (cache imported headers) and T-0102 (stop recompiling the standard
library).

## Whole-program benchmarks against C++ (T-0090; 2026-10-24)

`tests/bench` compares one construct at a time. The website wanted a number for a whole
program, so `tests/bench_programs` has three, each written idiomatically twice: `astar` (A*
over a grid with a generic heap and struct-keyed maps), `expr` (generate, tokenize, parse into
`Box`ed unions, evaluate, with errors through `!->`) and `words` (a word count over generated
text, then a sort). `tests/bench_programs/README.md` has the rules.

- **Same compiler, same flags, same result.** For each compiler, the Dazzle half is `dz build
  --release --cxx X`, and the C++ half is `X -std=c++17 -O2 -fno-exceptions -fno-rtti`.
  Dazzle's code also gets `-fwrapv`, the cost of its wrapping arithmetic, which the C++ does
  not get. Input comes from SplitMix64, written out on both sides. Every program prints a
  checksum over its whole result, and any order that could tie is made total, since the two
  sides' heaps and sorts are different algorithms.
- **`tools/bench_programs.py`** builds from clean, times `dz cpp` separately as the front end,
  runs the halves alternately, takes the median of ten, and refuses to report a pair whose
  checksums differ. It writes Markdown (`RESULTS.md`) and JSON, with the CPU, the compilers'
  versions, the commit and the load average before and after. Runs go through
  `measure.cpp`, a fork/exec/`wait4` helper, because a process started from Python reports
  the interpreter's ~18 MiB as its own peak until it calls exec.
- **A test**, `whole_program_benchmarks_agree_with_their_cpp_halves`, builds every pair at
  `-O0`/`-O1` on the input in `quick_args.txt` and requires the same checksum.

**The temporary arena.** The Dazzle halves first allocated everything from the global heap.
The user pointed out that a Dazzle program would put scratch memory in `Allocators::Temp`,
so each program was measured both ways, interleaved in one sitting. `expr`, with each
statement's tree in the arena and a reset before the next, was 5–11% faster. `words`, with
the whole count in it, was 6–7% faster, but peaked at 71 MiB instead of 44, because what a
growing text or table leaves behind is not reused. `astar`, with each search's heap and
maps in it and a reset after the search, was 4–5% *slower* and 30% larger. Its maps grow
past the arena's 64 KiB chunk, so every resize became a `malloc` of its own (T-0099). It stays on the
global heap, with a comment saying why. The C++ halves use the default allocator;
`std::pmr::monotonic_buffer_resource` would be the matching strategy, and is not used.

**The published run:** Dazzle at 1.3–1.45× C++ with g++ and 1.55–2.1× with clang. Build time
is 1.8–3 s against 0.3–0.5 s, of which the front end is about 0.5 s. Phase timings, taken with
`Instant` in throwaway copies, located most of the gap. Neither `perf` nor valgrind works
on this machine; valgrind cannot decode its glibc's AVX-512.

- `words`: `List::sort_by` takes 100 ms where `std::sort` takes 19 (T-0096).
- `expr`: parsing takes 120 ms where C++ parses and evaluates in about 80. Each `!->` step
  moves a union twice through a default-constructed temporary (T-0097).
- `astar`: not broken down yet.

**Three codegen bugs the programs found, fixed** (each with a case in `tests/run`):

- A `const` array built from casts, `const OPS: [2]u8 = ['+' as u8, '-' as u8]`, failed to
  compile. The C++ is `constexpr`, and `dz::cast` was not. It is now (`tests/run/statics`).
- `let (name, tree) = parser.statement() or { … }` copied each element out of the tuple
  temporary, and `Expr` is move-only, so the C++ did not compile. An element that owns
  anything is now moved out (`tests/run/failure`).
- `let tokens = tokenize(text) or { … }` **copied the whole list twice**: out of the result,
  then into `tokens`. `List<Token>` is copyable because `Token` is, and both lowerings moved
  only what could not be copied. Both sources are compiler temporaries that are never read
  again, so anything that owns something is now moved out of them (`copy_is_free` in
  `dz_cpp`). `expr`'s peak memory went from 298 MiB to 140, below C++'s 151, and its g++ time
  fell from 402 ms to 310.

Writing them also turned up some friction, not filed as tasks: `while true { … return }`
does not count as returning on every path (DZ0405); a moved-out local must be returned as
`move(x)` rather than `x`; and `if let (a, b) = opt` does not parse. A `str` cannot be cut
safely at all, which is T-0098.

## `--no-line-directives`, and `dz` as WebAssembly for the website's playground (2026-10-24)

The website gained a playground: Dazzle on one side, the C++ it compiles to on the other,
regenerated as you type. It needed two things from the compiler.

- **`--no-line-directives`**, which spec §16.3 had promised and nothing implemented. `dz
  run`, `build`, `cpp`, `test` and `bench` take it; `dz_cpp::emit` now takes an `Options`
  (the `Mode` plus `line_directives`). Every place a directive would go is still tracked, so
  the output is the default output with its `#line`s removed and nothing else — a test
  (`no_line_directives_leaves_out_the_directives_and_nothing_else`) holds every generated
  file, `std`'s included, to exactly that. `#location` and panic locations are unaffected:
  they are rendered from spans, never from C++'s `__LINE__`. Without directives nothing
  names the copies of the standard library's sources written beside the C++, so they are
  not written.
- **`dz` builds for `wasm32-wasip1`.** The browser runs the real CLI over an in-memory
  filesystem (a WASI shim), so the manifest, the analysis and the emitter are exactly
  `dz cpp`'s. Two things stood in the way. libclang cannot be loaded in WebAssembly, so the
  libclang half of `dz_cppimport` moved to `clang.rs`, built only off-wasm, and `clang-sys`
  became a target-specific dependency. But `std` itself imports `stdlib.h` for
  `Allocators::Global` (spec §14.2), so a WebAssembly build carries that one header in
  `builtin.rs`: `malloc` and `free`, the declarations `std` calls, under a pseudo-path whose
  text `dz_cppimport::read_file` serves. The generated C++ is the same either way, since it
  includes the real `<stdlib.h>`. Any other `import cpp` reports that this build has no
  libclang. A test holds the carried declarations to what libclang reads from the real
  header, parameter names aside (`__size`, `_Size`), so the two cannot drift. The other
  obstacle was `std::env::temp_dir()`, which panics on WASI; `dz_project` used it only as
  the directory `std`'s header is imported from, which the carried header never reads.
- CI builds `dz` for `wasm32-wasip1` on Linux, so a dependency that cannot target it fails
  there rather than in the website's deploy.

Under Node's WASI on Windows, a glob in `sources` matched nothing (directory listing failed)
while literal paths worked. The playground names `src/main.dz` literally, so it does not
depend on listing directories.

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

## Diagnostics a program can read, and act on (T-0060, T-0062; 2026-10-22)

The first two `llm-tooling` rows, chosen by the user with the r70–r72 batch.

- **`dz check --format=json`** (`dz_diag::render_json`): one JSON document on stdout, a
  diagnostic to a line inside it, with code, severity, message, the primary file/line/column
  flat, every label, notes and helps split, and the fix. Each span is given as byte offsets
  into the file as it is on disk, which is what an edit is applied with, and as 1-based line
  and column in characters. Written by hand — `dz_diag` has no serde — and checked by parsing
  it with `serde_json` in `crates/dz/tests/fix.rs`.
- **Certain fixes and suggestions.** `Fix` gained `certain`. A certain fix resolves the
  diagnostic and nothing else a reader would need to check; a suggestion is a guess an editor
  offers by its title. `Diagnostic::did_you_mean` turns every "did you mean `x`?" in the
  resolver and checker into a suggestion replacing the misspelt name (twelve sites); certain
  fixes were added for an unused variable (`_` in front), `mut fn` and `#println(…)`, beside
  the two that existed (missing switch arms, missing trait members). The parser attaches a
  fix only to tokens of its own file, and `dz_macro::annotate` drops any fix whose edit
  touches a macro invocation's name — the span every token a macro wrote carries.
- **`dz fix [--dry-run]`** applies the certain fixes: per file, last edit first, a fix
  whose edits clash with one already taken skipped whole, never a file not on disk (the
  standard library). It says to run `dz check` again, since a fix can let a later phase run.
- **`dz explain <code>`** (`dz_diag::explain`): the rule is the code's doc comment in
  `codes.rs`, read from the file as compiled; the why and the examples are in
  `crates/dz_diag/explain/*.md`, a `## DZNNNN` section per code with a wrong and a right
  program. `dz_driver/tests/explain.rs` requires a section for every code and compiles every
  example — the wrong one must report exactly its code, the right one nothing — so the text
  cannot drift. Codes no single-file program can produce (manifests, encodings, C++ libraries,
  compiler bugs) carry a `No example:` line instead. Writing them found four compiler bugs,
  listed under r70–r72 in `m12.md`.

## Getting the repository ready to be public (2026-10-22)

The repository moved to the `dazzle-lang` organisation, ahead of being made public.

- **raylib and RGFW are git submodules** in `examples/vendor/`, no longer 527 files of
  raylib copied into `examples/raylib/vendor`. raylib is pinned to its `6.0` tag: the copy
  matched 6.0 except for a newer RGFW backend, which the GLFW build never compiles. RGFW is
  pinned to `d7eb573`, the upstream commit whose `RGFW.h` was byte-identical to the copy.
  `examples/raylib` and `examples/breakout` share the one raylib. CI checks out submodules,
  and `every_example_builds` names `git submodule update --init` when they are missing,
  rather than failing deep inside the recipe. The copies remain in git history. A recipe
  that fetches its own source, which would retire the submodules, is now part of T-0077.
- **The examples were cleaned up.** `raylib` had leftover `List` pushes and an unused
  static. `rgfw` called `RGFW_window_close`, which frees the window, inside the event loop
  and then again after it, so pressing a key was a use-after-free. It now asks the window to
  close and frees it once with `defer`. `books` reimplemented `Buffer` and `List` over
  movies, and is now `ownership`, the README's companion: a growable `Shelf<T>` with `own`,
  a constructor, `drop`, `copy`, growth through `move_out`, a copy and a move, and no leaks
  under `--track-memory`. Breakout's rules moved from `tests/run/breakout_sim/game` to
  `examples/breakout/game`, so an example no longer depends on a test fixture;
  `breakout_sim` points there instead. Every example is now `dz fmt`-clean, and CI checks
  that. Formatting them found T-0081 and T-0082.
- **README**: new sections on modules and imports, and on `own` and the three special
  members with the C++ they become, taken from `dz cpp`'s real output. The stale revision
  number and the "planned" `dz fix`, `dz explain` and `--format=json`, all shipped, are
  gone. Added `LICENSE` (MIT, previously declared only in `Cargo.toml`) and `CONTRIBUTING.md`.

## Field alignment counts the visibility (T-0081; 2026-10-23)

`dz fmt` aligns a run of fields on the width of the longest name, but measured the name
without its `pub`, `protected` or `pub(module)`, so a struct mixing visibilities came out
ragged. It now measures and pads everything before the `:`: visibility, `own` and the name.
`examples/ownership` and `tests/run/ownership` re-format. Four fixtures that had been aligned
by hand the new way were already right. The standard library is not `dz fmt`-clean for other
reasons and was not re-formatted here.

## CI runs once per change (2026-10-23)

CI ran on every pull request and again on the merge into master, and pull requests here are
merged as soon as they open, so every change paid for two runs of up to half an hour and the
PR run was usually cancelled. It now runs only on a push to master, plus by hand
(`gh workflow run CI --ref <branch>`) to try a branch first. A push that touches only the
root Markdown files, `docs/`, `editors/`, `tools/`, `.claude/` or the logo skips it (not
every `.md`: `dz explain` compiles in `crates/dz_diag/explain/*.md`), and a newer push to the
same ref cancels the run still going.

## CI on the pull request again, and linear history (2026-10-23)

The repository now merges pull requests only by rebase, deletes the branch once merged, and
commits carry no per-commit AI credit (the README discloses it once). With rebase-merge, a
PR's commits land on master as new commits whose committer is GitHub, so CI runs on the pull
request again and the job skips a push to master that GitHub committed: each change is still
tested once, on the PR (as merged into master), or on master for a commit pushed there
directly. Both triggers share the same paths-ignore.

## Trailing comments stay where they were written (T-0082; 2026-10-23)

`dz fmt` placed a trailing comment only after the nodes that asked for one — a statement, a
field, a variant — so a comment after a block's `{` went to the next line inside the block,
and one after a `}` to a line of its own. Now any line comment written after code goes at
the end of the last line printed, which is that code: the head of a `fn`, `struct`, `impl`,
`while`, `for` or `#if`, any closing `}`. Blocks inside expressions (`if`, `else`, `case`,
`unsafe`, an `or { e -> }`) are printed by a sub-printer that now starts on the `{`, so the
same rule covers them; `switch x {` takes its comment explicitly, or the first `case` would.
A `fn` whose body is only a comment no longer prints as `{}` with the comment after it.

Trailing comments also keep their column instead of collapsing to one space. Each is
written as a placeholder carrying the column it was written at, and a last pass over the
finished text places it: at that column, counted from its line's indentation so it moves
with a re-indented line, never closer than one space to the code, and with adjacent lines
whose comments were written at one column kept aligned at the column the widest needs. The
placeholder is a private-use character absent from the source, so it can never be mistaken
for one written there. The README's samples, aligned by hand, format unchanged; nothing in
`examples/`, `tests/` or the standard library formats differently.

## CI in a third of the time, and the standard library checked once (2026-10-23)

The Linux job took 29 minutes. Per-step timings put it in three places: `apt-get install`,
7 of them on a day the Ubuntu mirror served `libclang-dev`'s 29 MB at 70 kB/s; the test
suite at 10; and the whole suite again against clang++, another 10. Within the suite,
`dz_driver/tests/tolerant.rs` was six and a half minutes of each pass. Now:

- **Tests build at opt-level 1 in CI** (`CARGO_PROFILE_{DEV,TEST}_OPT_LEVEL`, in the
  workflow and not `Cargo.toml`, so local rebuilds stay fast). Debug assertions and overflow
  checks are separate settings and stay on. `tolerant.rs` went from 293 s to 96 s on an
  8-core machine for 36 s more of build.
- **The clang++ pass runs only `dz/tests/run.rs`**, the one test that reads `DZ_CXX`;
  `matrix.rs` already tries every compiler present, and nothing else compiles C++.
- **`libclang-dev` is installed only when the runner has no libclang**: clang-sys loads
  `libclang.so.1`, which the image's LLVM ships, and the headers and static archives on top
  are what made the download slow.

**The standard library is checked once per thread rather than once per analysis.** With
its trees already cached, checking its bodies was still about 20 of the 44 ms an analysis of
`hello` took — and the editor analyses on every pause in typing. The checker now works in two
layers: the prelude's (`std` and the headers it imports), then everything else, each pass
running over a whole layer before the next starts. Nothing in the prelude's layer can see
past it, so its checker state — the type table, its HIR, the parameter counter, the `impl`
bookkeeping — is a function of the standard library alone, and `dz_check::PreludeLayer`
captures it. `dz_driver` keeps one per thread (the types share names through `Rc`) keyed on
what can change it: the `#if` flags on for `std` (none when a parse error left every `#if`
in place), the `Arc` of each header it imports, and the root's `type_info`, the one root
setting the checker reads. An analysis that matches starts from a copy and checks only the
rest.

That needed the layer's ids to be the same in every workspace, and they were not:
- `libc`, the module `std` imports `stdlib.h` into, was created after every project module,
  so its module id and symbol ids depended on the program. `dz_project::load` now imports
  the prelude's headers straight after adding the prelude, so they are modules 1.. and their
  files follow `std`'s. **`std`'s own `stdlib.h` is therefore read with C++17**, the oldest
  standard the generated code compiles as, rather than the root manifest's `cpp_standard`:
  anything declared there is declared under any later standard too.
- `std`'s 75 files are added to the source map first, before any manifest
  (`Module::embedded_files`), instead of after the manifests of however many modules the
  project has.
- The checker numbered a method's generic parameters after every module's type parameters;
  checking by layer numbers `std`'s first.

`flatten_bases`, `attach_special_impls` and `collect_foreign` each walked the whole
workspace; each now does only its layer, since flattening a type twice gives it its base's
fields twice. Impl overlap is checked only against earlier impls, and `std`'s come first, so
layering changes nothing there. A layer is used only if its symbol count matches the
resolution it is given.

`dz_driver/tests/prelude_cache.rs` holds reuse to being invisible: a program analysed after a
different one must give exactly the diagnostics and HIR dump it gives checked afresh, in both
modes, and a broken program and a clean one never share a layer. It caught a race on its
first run: two threads importing one header for the first time both parsed it, and the
second replaced the first's cache entry, so the first thread's `Arc` never matched again.
`dz_cppimport::import` now keeps an entry another thread made meanwhile, so an unchanged
header is one `Arc` per process — which `Workspace::add_foreign`'s sharing relied on already.

`hello` now analyses in about 29 ms steady, from 44; copying the layer is 3.5 ms of that.
`tolerant.rs` takes 132 s in a debug build (from 293) and 55 s at opt-level 1 (from 96).
