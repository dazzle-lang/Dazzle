# Tasks

One file per task, `T-NNNN-slug.md`. **`docs/tasks.md` is the backlog**: every open task,
one line each, grouped by milestone — read it first. It is generated from these files by
`python tools/track.py index` and never edited by hand; a hook rewrites it whenever a task
file changes, and `check` fails while it is stale. To change the backlog, change a task file
(or the `## Now` section below, which it copies). For other views, the script reads only
each file's header:

```bash
python tools/track.py tasks                      # every open task
python tools/track.py tasks --milestone M12      # one milestone's open tasks
python tools/track.py tasks --area llm-tooling   # one area; --kind deferral|design|…
python tools/track.py tasks --status all         # done and dropped ones too
python tools/track.py milestones                 # open and done counts per milestone
python tools/track.py show T-0050                # one task in full
python tools/track.py new task "Title" --milestone M12 --area stdlib --kind feature
python tools/track.py check                      # headers, ids, milestones, §-references
```

## Now

**The 0.2 release**, planned with the user on 2026-11-09. Six parts, roughly in this order;
the stdlib and the benchmarks go together, since the new programs show what `std` lacks:

1. **Bugs in the generated C++ and the tools.** A C++ error in generated code is a compiler
   bug (invariant 7): T-0121, T-0167, T-0034 and T-0053; then the parser's `as` precedence
   (T-0161) and `dz fmt` on array literals (T-0092).
2. **Build times.** The C++ compiler is about 90% of a build. T-0102 is done
   (2026-11-09): each generated file has a cached object, so an unchanged `std` is not
   compiled again. T-0180 holds the targets for 0.2 (a debug rebuild under 0.5 s, a clean
   release build within twice C++'s), both met on Linux on 2026-11-09 by a precompiled
   header (T-0181), one `std` for every project (T-0182), the LTO fallback remembered
   (T-0183) and imported headers kept on disk (T-0101): a build from clean now costs what a
   one-line change does, 0.29–0.35 s in debug (C++: 0.28–0.47 s), and a clean release
   build is 1.1–1.6× C++'s.
   T-0184 (emitting less) is not needed for them. Left: T-0012 (build time at scale), T-0103
   (stale generated files and objects) and T-0025 (`std` checked in every analysis, now
   the largest fixed cost of a build after the C++ compiler). T-0012 (done 2026-11-09)
   measured a generated 100-module project: Dazzle is 11× C++ from clean and far behind
   after an edit, for three reasons, T-0185 (instance bodies in headers, the largest),
   T-0186 (the up-to-date check) and T-0187 (phases that grow faster than the program).
   T-0185 and T-0186 are done the same day: 100 modules from clean 60 s to 12 s (C++: 5 s),
   an edit 15 s to 6 s (C++: 1 s). T-0187 is done (2026-11-11): Dazzle's own phases now
   grow with the program, 4.5 s to 1.9 s at 100 modules, so nothing changed is 2.1 s and an
   edit 3.3 s; the rest of an edit is re-analysing what it did not touch (T-0023). Then, from
   a plan the user approved the same day (incremental builds, in stages, each its own
   commit): T-0192, a build with nothing changed checks a stamp and skips analysis (2.0 s to
   5 ms at 100 modules); T-0191, a module's instance bodies in a file of their own; T-0193,
   a test that an incremental build equals a clean one; T-0195, the edited module compiles
   from its own header, precompiled (a leaf edit's C++ 0.91 s to 0.24 s); T-0196, a debug
   build links with `mold` or `lld` when one is installed. Next in that plan: re-running
   the later phases only for the modules an edit can affect. Left besides: the rest
   of T-0190: its g++ half is done (an optimised g++ build gets the inline budget of an
   `inline` function, and the benchmarks run 6-17% faster), and what is left, compiling
   only the methods something calls, is worth 0.03-0.06 s on a small program's debug build. T-0189, found on the way, is fixed: the
   runtime's `destroy` of a word ended its lifetime, and g++ deleted `String`'s store to it.
3. **The stdlib.** `BinaryHeap` (T-0175, with a decision on which end pops first), `Deque`
   (T-0176) and `BitSet` (T-0177); then `str[a..b]` (T-0098), `indexed()` (T-0037) and
   generic trait methods (T-0007); then whatever the benchmarks and the ImGui panel miss.
4. **C++ interop: Dear ImGui in the SDL3 scene** (T-0171), the ImGui half of T-0010, with
   what it needs on the way: overloaded free functions (T-0016), variadics (T-0008), enum
   variants (T-0048). No Jolt. Then a C++ namespace reached as a module of its own,
   `ImGui::Begin` rather than `imgui::ImGui::Begin` (T-0179, added by the user the same
   day; its spelling is the user's decision).
5. **Benchmarks.** Two whole programs against C++, `json` (T-0173) and `raytrace`
   (T-0174); an ECS against Rust and C++, a simple one of a few hundred lines, showing a
   query inside a query (T-0172); and the generic-instantiation pair with numbers tracked
   over time (T-0075). The other programs get no Rust half.
6. **The release.** Release notes from the §21 rows since r117 (0.1's revision), and
   packages (T-0077) if there is time, otherwise 0.3. The version stays 0.1 and nothing is
   tagged until the user asks. **The notes must also cover build times**, which §21 does
   not hold since they are not a language change (the user asked for this on 2026-11-09):
   per-file cached objects (T-0102), precompiled headers (T-0181), `std` compiled once per
   machine (T-0182), the LTO fallback remembered (T-0183) and imported headers kept on
   disk (T-0101). Take the numbers from T-0180 and `tests/bench_programs/RESULTS.md`: a
   debug build from clean went from 0.76–0.89 s to 0.29–0.35 s, the same as the C++
   versions (0.28–0.47 s), and a clean release build from 2.3–2.8× C++'s to 1.1–1.6×. And
   the test suite: `cargo test -p dz` went from 77 s to 51 s (112 s to 88 s from cold),
   about a third less CPU, since no test compiles `std` any more. And T-0012's numbers at
   scale (`tests/bench_programs/SCALE.md`): a 10-module project's build went from 9.7 s on
   every build to 0.5–1.6 s; and at 100 modules (51 000 lines) from clean 60 s to 12 s and an edit
   15 s to 6 s, against 5 s and 1 s for C++ (T-0185, T-0186); and with T-0187 from clean
   9.5 s, nothing changed 4.8 s to 2.1 s and an edit 3.3 s, and Dazzle's own phases of a
   small program 13-21% less; and with T-0192 a build with nothing changed 5 ms (`make`:
   50 ms); update with the rest of the incremental-build stages.

Not in 0.2: the placement gaps below (T-0141, T-0142, T-0147, T-0150), which keep memory on
the heap but are not safety holes, and the ten-thousand-line game (T-0138).

**Memory placed by the compiler, then safe code made sound**, chosen by the user on
2026-11-01. Automatic placement matters most (T-0117 and what follows it), and threads can
wait (T-0011 stays in v2). A review that day found safe programs that still reach freed
memory. D-0094 proposes the rules that close each hole, for the user to decide. The order
proposed:

1. T-0117 is done (2026-11-02): the gate kept regions. Peak memory, the loop and safety
   pass; with r105 in, `astar` and `words` are faster than the heap, and `expr` is even,
   its parser nodes kept on the heap by the loop rule.
2. The holes the placement check shares, since a placement is checked like a reference:
   T-0123, T-0124, T-0125 and T-0127 are closed (r103), and T-0131 makes a wrong placement
   show in tests (all 2026-11-02).
3. Frame regions (T-0139) turned out not to be needed yet (2026-11-02): the function
   called each frame already has its region. Two gaps that kept frame data on the heap are
   closed instead (`format!`'s text, and a value handed to a parameter that stays in the
   call), and a field replaced every frame joined T-0120. T-0119 is done (r104,
   2026-11-02): ordinary code names no allocator, and a container reads where it was
   placed as `#placement`. The speed hand-placed `Temp` gave `words` and `expr` waits
   for T-0145 and T-0146. T-0118 is done (2026-11-02): a drop that only frees is skipped
   when its region frees everything the value owns; the cases it still leaves are T-0147.
   T-0145 is done too (2026-11-02): a value stored into a local container is built where
   the container lives, so a `List<String>` filled in its region skips its drop. T-0149
   and T-0140 are done (r105, 2026-11-02), asked for by the user before T-0117's gate since
   they change what it measures: a map filled under a lookup that found nothing, `format!`'s
   text in a loop and a program's own container keep their values in the region, and a copy
   is built where it is placed. `words`' keys and `expr`'s names are now in `main`'s region.
   A container lent to a function is left as T-0150, a design question. The user's idea of
   a checked marker for a drop that only frees is T-0148, for later.
4. The rest of D-0094, decided in full on 2026-11-03 (r107) and to be built: T-0128,
   T-0129, T-0126, T-0130, T-0135 and T-0136 are done (r108–r112, 2026-11-03), and so is
   §1.1's promise (T-0160, r113), with the hole found on the way (T-0163, statics dropped
   at exit); then its tests: the sanitizer job is done (T-0132), then T-0133 and T-0134;
   and the placement gaps (T-0141, T-0142, T-0147, T-0150). T-0146 is done (2026-11-04):
   a value that holds the one it replaces, a parser's tree, keeps its region.
5. A game of ten thousand lines (T-0138), before deciding how exact a clause must be
   (T-0137).

**M12, the stdlib**, chosen by the user on 2026-09-13. Its criterion — a small raylib game
with no C++ of its own — is met: Breakout, 2026-09-15 (`examples/breakout`, its rules played
headless by `tests/run/breakout_sim`). The order the user set on 2026-09-15 is done through
r57 (stderr, `Time`, generic methods, `Random`, `Path` and `File`); what follows is chosen
from M12's open tasks. Later, by the roadmap: fetched packages (M16), hot reload (M13), and
the release workflow and installer (T-0076).

**When asked what there is to work on**, answer from `docs/tasks.md` in two parts and say
both: the milestone work (M12 first, then the other milestones' open tasks), and the tooling
that would make Dazzle easy for an LLM to work in (the `llm-tooling` rows under `none`).

## A task file

```markdown
---
id: T-0050
title: Associated-type projection
milestone: M12
status: open
kind: deferral
area: checker
where: `dz_check/src/check.rs`, `iterate_item`
---

What is missing or wrong, why it matters, and what has been tried.
```

**The body is written for someone who has not met the problem.** The title is a reminder;
the body is the explanation, and it is what the backlog's link leads to. Four things, in
this order, each under a bold lead-in so a reader can skip to the one they want:

1. **What it is** — the feature or the defect, *spelled out*, with a short Dazzle snippet
   showing the refused or wrong form and the working one beside it. Do not assume the reader
   knows what the title's words mean: "format specifiers" meant nothing to the user who had
   to choose whether to schedule it, and that is the bar to clear.
2. **Why it matters** — concretely, in terms of a program someone would write. "A game needs
   it for X" beats "it is a gap". If it does not matter much, say so: a row that admits it is
   low value is more useful than one that implies everything is urgent.
3. **Why it is this way** — for a deferral, what was tried and what blocked it; for a
   feature, why it has not been built. Name the diagnostic code where there is one.
4. **What it would take** — the files and functions, and the **decision the user has to
   make** if there is one, with the options and what each costs. Say explicitly whether it is
   a language change (a §21 revision row and usually a `D-NNNN`) or only compiler work
   (`docs/log/`).

Link related tasks as `[T-0050](T-0050-associated-type-projection.md)` — a relative link to
the sibling file, so it is clickable from the backlog and in an editor. `check` verifies every
`T-NNNN` and `D-NNNN` mentioned anywhere under `docs/` exists.

**Do not quote the `title:` value.** `header` splits on the first colon and keeps the rest
verbatim, so backticks and a second colon are fine as they are; quotes would become part of
the title and appear in the backlog.

| Field | Values |
|---|---|
| `milestone` | a row of the roadmap, spec §19 (`M9`, `M12`, …), `v2` for after the first release, or `none` for not scheduled. A milestone is a group of tasks; when milestones become versions, that mapping goes in §19 and no task changes |
| `status` | `open`, `done` (add `done: r65, 2026-10-22` — the revision or commit and the date), or `dropped` (add a line saying why) |
| `kind` | `feature`; `deferral`, something the code knowingly postpones; `design`, which changes the language and is the user's decision (it gets a decision file and a spec §21 row); `bug` |
| `area` | free text, kept to the words already in use: `ownership`, `text`, `generics`, `interop`, `checker`, `codegen`, `stdlib`, `macros`, `resolve`, `editor`, `bench`, `packaging`, `concurrency`, `tooling`, `llm-tooling`, `language` |
| `where` | for a deferral, the file and function that postpone it |

**Rules.**

- **Done is a status, not a deletion.** Set `status: done` and record what was built in the
  milestone's file in `docs/log/`. Done tasks are how a milestone, and later a version, says
  what it contains.
- **A deferral has a comment in the code naming its task** (`// deferred: T-0034`), so
  either end finds the other. Comments written before tasks had ids name the milestone.
- **Before claiming a milestone is complete**, `python tools/track.py tasks --milestone M…`
  must print nothing, or every row left must be moved to another milestone with the user's
  agreement.
- Ids are never reused or renumbered; `new` takes the next free one.
- **A task whose code spans several files keeps a "Where the code is" table** near the
  top: each piece, and the file and functions that hold it ([T-0107](T-0107-design-ties-kills-clauses-compiler-placed.md)
  has one). Whoever works on it updates the table in the same change that moves the code, so
  the next session reads where to work instead of searching for it.

## Out of scope for good

Not tasks, and not deferred: lifetime parameters — a tie belongs to a type, never to a
struct that holds references (§6.7, D-0091), and a reference lives only where references
live, a view struct included (D-0003, D-0093); a raw `^T` is the programmer's problem, inside
`unsafe` (spec §1.1) — and function overloading until a later language version (D-0005).
