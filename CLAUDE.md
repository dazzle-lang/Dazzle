# Dazzle — working notes for Claude Code

Dazzle is a systems language that compiles to readable C++17, meant as a replacement for C++
wherever it is used: games and engines, embedded systems, tools, real-time code. It is
C++-grade in control, but with RAII reduced to three special members, enforced
encapsulation, and definition-site generics. The compiler is written in Rust.

**Always `git pull` first**, before reading or changing anything else in a session. The
work happens from more than one machine and in cloud sessions, so a checkout that has not
pulled is reading stale tasks and may redo or contradict work already merged. A SessionStart
hook (`.claude/settings.json`) runs `git pull --ff-only` and prints the result; if it says
the pull failed, or the pull cannot fast-forward cleanly (uncommitted changes, a conflict, a
diverged branch), stop and tell the user rather than stashing, rebasing or resolving it.

Where it stands: M0–M8, M10, M11, M14 and M15 are done; most of M9 (`dz lsp` and the VS
Code extension) was pulled forward; **now: M12, the stdlib**, whose criterion (Breakout) is
met. **`docs/tasks.md` is the backlog** — what is next, and every open task by milestone.
Spec §19 is the roadmap.

**Formatting is rustfmt's**, configured by `rustfmt.toml` (`use_small_heuristics = "Max"`,
which is what keeps it close to how this repository was written by hand). Run `cargo fmt
--all` before finishing; CI checks it on a **pinned** toolchain, as it does clippy, so a new
Rust release cannot turn the build red on its own.

## The documents

Each is a folder of small files, so read the one you need rather than all of it.

- **`docs/spec/`** — the language contract, one file per top-level section; `README.md` is
  the index and holds the revision number. See below.
- **`docs/decisions/`** — spec §18: every design question the user has settled or left open,
  one `D-NNNN` file each. `D-00NN` is what older text cites as `18.NN`.
- **`docs/tasks.md`** — **the backlog: read it first.** What is next, then every open task,
  one line each, grouped by milestone. **When asked what there is to work on, answer from
  it** in two parts: the milestone work (M12 first) and the tooling that would make Dazzle
  easy for an LLM to work in (the `llm-tooling` rows) — say both. It is generated from
  `docs/tasks/`, so it is always current: **never edit it by hand.**
- **`docs/tasks/`** — open and done work, one `T-NNNN` file each with a `milestone`, `status`,
  `kind` and `area` header; deferrals are `kind: deferral`. Add a task with `python
  tools/track.py new task "…" --milestone M12 --area …`; close one by setting `status: done`.
  A PostToolUse hook then rewrites `docs/tasks.md` (or run `python tools/track.py index`).
  `docs/tasks/README.md` has the rules.
- **`docs/log/`** — the build log, one file per milestone (`README.md` is the index): what
  was built and decided, and why. Add what you built to the matching file when you finish;
  read one only when you need the history behind something.
- **`docs/testing.md`** — the test map: which corpus covers what.

Run `python tools/track.py check` after editing any of them: it checks every task's and
decision's header, that ids are unique and exist where cited, that every `§N.M` in the
spec names a real heading, and that `docs/tasks.md` matches the task files.

## The spec is the contract

**`docs/spec/` is the source of truth for the language.** Read the relevant section before
implementing anything, and cite section numbers in code comments and commit messages
(`spec §6.4`). Section numbers are stable — do not renumber them; if you must insert a
section, append rather than shift.

- **§0.1** (`docs/spec/00-how-to-read-this.md`) is a one-page summary of the whole language.
  Start there.
- **§18** is `docs/decisions/`. **Do not silently re-decide anything there.** If
  implementation reveals a decision was wrong, say so explicitly and ask.
- **§19** is the roadmap (M0…M16); its rows are the milestones tasks belong to.
- **§21** is the language changelog. See below.

The user drives language design; Claude writes the compiler. Propose alternatives freely,
but design changes are the user's call, not something to slip into an implementation.

### The version is 0.1 and stays 0.1

The language is **pre-release**. 0.1 is not finished until there is an LSP good enough to
work in (§19 M9), a stdlib you can write a game against (M12), and C++ interop (M7). Until
then Dazzle is a toy: it compiles and runs programs, and that is all.

So **do not bump the version number.** The spec is *revised*, not versioned: `r1`, `r2`, …
in §21. If a change feels big enough to deserve a version bump, it deserves a revision row
and nothing more.

### Every language change gets a changelog row

**Any change to the language goes in §21 as a new revision row**, in the same edit that
changes the language. That means syntax, semantics, keywords, operators, precedence, a new
or reversed decision, or anything that changes what a valid Dazzle program looks like.

- One row per revision, newest first: `| rN | date | what changed and why |`.
- Write what a *user of the language* needs to know, not what the compiler now does.
  "`or` is non-associative" — not "added `CHAINED_OR` to `codes.rs`".
- A revision may cover several related edits; do not split one design change across rows.
- Bump the revision number in `docs/spec/README.md` at the same time.

**Compiler work is not a language change** and does not belong in §21 — new crates, new
diagnostics for existing rules, refactors, performance, test infrastructure. Those go in
`docs/log/`. The test: if a Dazzle program's meaning or spelling changes, it is §21; if
only the compiler's internals change, it is `docs/log/`.

## Repository layout

```
docs/spec/            the language specification, one file per section (the contract)
docs/decisions/       D-NNNN: design decisions, spec §18
docs/tasks.md         the backlog: open tasks by milestone, generated — read it first
docs/tasks/           T-NNNN: open and done work, one file each
docs/log/             build log, one file per milestone
docs/testing.md       test map: what every corpus below covers
tools/track.py        lists, creates and checks tasks and decisions; writes docs/tasks.md
tools/hooks/          Claude Code hooks, wired up in .claude/settings.json
crates/
  dz_span/            Span, SourceMap, line/column mapping
  dz_diag/            Diagnostic model + rendering (codespan-reporting)
  dz_lex/             the lexer (spec §2)
  dz_ast/             the syntax tree, plus a span-free `dump` used by tests and `dz parse`
  dz_hir/             the resolved, typed tree + `Ty`, built by the checker (spec §17.1)
  dz_parse/           recursive-descent + Pratt parser (spec §20)
  dz_fmt/             the formatter
  dz_project/         dazzle.toml, source globs, the dependency graph (spec §3.1, §3.2), and
                      the standard library, `src/std/**/*.dz` — real Dazzle, module `std`,
                      one declaration to a file, listed in `STD_SOURCES` (spec §14.4)
  dz_macro/           macro declarations checked, invocations matched and expanded with
                      hygiene, between parsing and resolution (spec §12); a token the macro
                      wrote has the invocation's `#name` as its span, a captured one its own
  dz_resolve/         module symbol tables, name resolution, visibility (spec §3, §4)
  dz_check/           the type checker (spec §5, §7, §9, §10, §11)
  dz_flow/            ownership, moves, drops, definite return (spec §6)
  dz_mono/            monomorphization: which instances each module needs (spec §8.4)
  dz_cpp/             C++17 emission + the runtime shim (spec §16)
  dz_cppimport/       `import cpp`: libclang (loaded at run time) reads a header into plain
                      declarations, which become a module of their own (spec §15)
  dz_driver/          the analysis pipeline as a library, shared by the CLI and the LSP;
                      `analyse_with` lets unsaved editor buffers stand in for files
  dz_lsp/             the language server (spec §17.4): `lib.rs` is the protocol loop,
                      `ide.rs` the queries (hover, definition, completion, outline), which
                      take an `Analysis` and an offset and are tested without a client
  dz/                 the `dz` CLI
editors/vscode/       the VS Code extension (it has its own CLAUDE.md)
tests/lex|parse|fmt/  lexer, parser and formatter snapshot corpora
tests/errors/*.dz     one program per mistake, whole pipeline, exact // expect: codes —
                      where rules in combination are tested
crates/dz_driver/tests/feature_matrix/
                      every construct and mistake in every context; add a row to extend it
tests/expand/*.dz     macro expansions as `dz expand` prints them
tests/flow/*.dz       one file per ownership rule, naming its code in `// expect:`
tests/run/            whole programs compiled and executed against an expected.txt
tests/workspace/      multi-module workspaces; typed/ must check with ZERO diagnostics,
                      typeerrors/ has one deliberate failure per type rule
examples/             real projects, every one built by `every_example_builds`, held to `dz fmt`
examples/vendor/      raylib and RGFW as git submodules (`git submodule update --init`),
                      until a recipe can fetch its own source (T-0077)
```

Crates still to come are listed in spec §17.1. Follow that layout rather than inventing one.

## Commands

```bash
cargo build --workspace
cargo test -p <crate>            # test the crate you touched; the full suite is slow
cargo clippy --workspace --all-targets
cargo insta review               # review snapshot changes interactively
cargo run -p dz -- lex <file.dz>     # tokens
cargo run -p dz -- parse <file.dz>   # syntax tree
cargo run -p dz -- expand <file.dz>  # the file with every macro invocation expanded
cargo run -p dz -- fmt --stdout <file.dz>
cargo run -p dz -- check <dir>   # analyse only; --format=json for every diagnostic as JSON
cargo run -p dz -- fix <dir>     # apply the fixes the compiler is certain of (--dry-run)
cargo run -p dz -- explain DZ0401    # a diagnostic's rule, why, and a wrong and right program
cargo run -p dz -- cpp   <dir>   # generate C++ into <dir>/build/cpp
cargo run -p dz -- run   <dir>   # build and execute; DZ_CXX picks the C++ compiler
cargo run -p dz -- lsp           # the language server on stdio (what VS Code runs)
python tools/track.py tasks --milestone M12   # open tasks; see docs/tasks/README.md
python tools/track.py check      # after editing anything under docs/
```

## Invariants that must not be broken

These are load-bearing. Breaking one is a real regression even if tests still pass.

1. **The lexer and parser never bail.** Every error produces a diagnostic *and* a token or
   AST node, so the stream is always complete. The LSP runs this code on half-typed source
   on every keystroke (spec §17.2).
2. **Spans are byte offsets into the file exactly as it is on disk.** Never normalise line
   endings by rewriting the buffer — handle `\r\n` and lone `\r` during the scan. A UTF-8
   BOM stays in the text and is skipped as whitespace, so offsets never shift.
3. **Every phase is a pure function of its input**, so results can be cached per module and
   the LSP can re-run only what changed.
4. **Diagnostic codes are stable and never reused.** Ranges are documented in
   `crates/dz_diag/src/codes.rs`. Tests assert on codes, not on message text, so wording
   stays free to improve.
5. **Error messages point at user code, never at generated code.** This is spec principle 4
   and the reason for definition-site generics and Dazzle-side monomorphization.
6. **One diagnostic per token position.** The parser suppresses cascades, so a single
   mistake produces a single error rather than four restatements of it.
7. **Generated C++ must compile warning-clean on more than one compiler.** `-std=c++17
   -fno-exceptions -fno-rtti -Wall -Wextra`, and check both clang++ and g++: the shim once
   used `__is_pointer`, which exists only in Clang. A C++ error in generated code is a
   compiler bug, never a user error.
8. **`typed/` and `typeerrors/` are a matched pair.** A checker that rejects valid code is
   as broken as one that accepts invalid code. When you add a rule, add a case to BOTH.
9. **`::` is resolved at compile time, `.` is not.** Name resolution handles `::` paths;
   `.field` and `.method()` need the receiver's type and belong to the type checker. Do not
   blur that line — it is what keeps M2 and M3 separable (spec §3.3).
10. **The formatter only ever changes layout.** Its tests assert idempotence, tree
   preservation, and that output re-parses; it refuses to touch a file with syntax errors
   rather than silently dropping the broken parts.

## Conventions

- **Diagnostics** carry a code, a primary label with a span, and usually a `help:`. Look at
  `crates/dz_lex/src/lexer.rs` for the house style — the message says what is wrong, the
  label says what is wrong *at that span*, and the help says what to do about it.
- **Tests**: snapshot corpora for breadth (`tests/lex/*.dz`), plus targeted unit tests that
  pin one spec rule each so a regression names the rule it broke instead of producing a
  large snapshot diff. Every `[DECIDED]` rule in the spec should end up with a test.
- **A test must not depend on the host.** CI runs on Linux, macOS and Windows, and it has
  caught this three separate times — each one a test that passed only on the machine it was
  written on:
  - an **OS error message** in a snapshot ("The system cannot find the path specified. (os
    error 3)" against "No such file or directory"). Keep it in the *diagnostic*, where
    "permission denied" versus "not found" matters to a user; redact it in the test.
  - **line endings**: git checks non-`eol=lf` files out as CRLF on Windows, so a fixture
    compared byte-for-byte needs a `.gitattributes` entry *and* a test that normalises.
  - **an environment variable the test does not control**: `DZ_CXX` is set by CI, and a
    test asserting compiler precedence passed vacuously until it cleared it.

  The rule that catches all three: a snapshot pins *Dazzle's* behaviour. Anything in it
  that comes from the operating system, the checkout, or the environment is noise and
  belongs behind a redaction.
- **Comments** explain *why*, especially where a choice looks arbitrary. Do not narrate
  what the code already says.
- **Do not commit or push unless the user asks.**
- **No AI attribution in commits or PRs**: no `Co-Authored-By: Claude` trailer, no
  "Generated with Claude Code" line. The README's "How Dazzle is made" section discloses it
  once for the whole repository; `.claude/settings.json` turns the trailers off.
- **PRs merge by rebase** (the repository allows nothing else), so history stays linear: keep
  a PR to one commit with a full message, which lands on master as it is.

## Environment gotchas

- **Never use a heredoc** (`<<'EOF'`, `<<PY`, …) in a Bash command. Here they silently turn
  `\\` into `\`, which once made the lexer's escape tests look wrong and then corrupted the
  Python script written to fix them. A PreToolUse hook (`tools/hooks/no_heredoc.py`) refuses
  them. Use `Edit` to change a file, `Write` for a new one, and for a script: `Write` it to
  the scratchpad and run it in a separate Bash call — never pipe it in.
- Windows. The Bash tool is Git Bash (POSIX sh); PowerShell is also available. Prefer
  forward slashes in paths.
- `.gitignore` uses CRLF and had no trailing newline — appending with `>>` once produced
  `*.pdb/target`. Check the last byte before appending to any file.
- The `dz` binary is at `target/debug/dz.exe`.

## History

The compiler was prototyped in Odin and ported to Rust once the port passed the same corpus
with better diagnostics (spec §17 says why Rust); git history has the Odin sources.
