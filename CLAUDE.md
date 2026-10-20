# Dazzle — working notes for Claude Code

Dazzle is a systems language for game development that compiles to readable C++17. It is
C++-grade in control, but with RAII reduced to three special members, enforced
encapsulation, and definition-site generics. The compiler is written in Rust.

Progress: M0 (lexer), M1 (parser, AST, formatter), M2 (projects, modules, imports,
visibility), M3 (type checker), M4 (codegen, runtime shim, `dz run`), M5 (ownership) and M6
(generics, traits, monomorphization) are done — **Dazzle programs compile, run, own their
resources, and are generic over checked bounds**. Since then, spec r12–r13 added `unsafe`,
`Copy`, and `impl Trait for Type` with `Drop`/`Copy` as traits, r14 made `move` end its
source for every type (18.33), and most of M9 was pulled
forward: `dz lsp` and a VS Code extension (`editors/vscode`), over an analysis that keeps
answering around syntax errors (`dz_driver::Mode::Editor`).

**M7 is done.** A C or C++ library vendored as *source* is compiled by `dz` from a
`[cpp.libs.<name>]` recipe and called through `import cpp` with no wrapper — no CMake, no
Makefile, no prebuilt library. **`examples/raylib` opens a raylib window with raylib built
from its own seven `.c` files**, which is M7b's criterion (spec §19). r21–r28 got there:
handles, C unions, array members and bit-fields, callbacks, compound-literal macros, then
the C++ half — namespaces, classes, methods, constructors, C++-owned destructors,
overloaded methods, references, and `[cpp.instantiate]` template instances. What is left is
narrower and listed in the deferral register (overloaded *free* functions, imported-class
inheritance). **Struct inheritance landed as r30** — single inheritance, `virtual`/`override`,
`protected`, base initialisers, inherited implementations — which also unblocks that last
M7 row. **`dz test` (M14) and `dz bench` (M15) have landed** — `#[test]` and `#[bench]`
functions beside the code they cover, each run in its own process, and a comparison suite
in `tests/bench` measuring Dazzle against hand-written C++. **CI runs on three platforms**
(`.github/workflows/ci.yml`) and has already earned its keep: its first runs found a shim
bug that broke `println("{}", xs.len)` on macOS, an `import cpp` that could not find the
SDK headers there, and two test bugs that had been passing vacuously. **M10 (macros) is
done as r63**: `#machine` in `tests/run/machine` writes M8's `ButtonState` and the editor
completes inside it; the notation is spec §12, settled with the user for cohesion with the
rest of the language, and unions gained methods for it. **Now: M12, the
stdlib** — see the "Next" row of `docs/PROGRESS.md` §1 for the current order. Later: an
installer that bundles libclang, then fetched packages (M16). Spec §19 is the roadmap.

**Formatting is rustfmt's**, configured by `rustfmt.toml` (`use_small_heuristics = "Max"`,
which is what keeps it close to how this repository was written by hand). Run `cargo fmt
--all` before finishing; CI checks it on a **pinned** toolchain, as it does clippy, so a new
Rust release cannot turn the build red on its own.

## Three documents

- **`docs/spec.md`** — the language contract. See below.
- **`docs/PROGRESS.md`** — the build log: what each milestone built and decided, the
  **deferral register** (everything knowingly postponed, and to which milestone), and the
  plan for the next milestone. Read it before starting work; update it when you finish a
  milestone. **When asked what there is to work on**, answer from it in two parts: the
  compiler and milestone work (§1 "Next", §4c, the §3 deferral register), and the tooling
  that would make Dazzle easy for an LLM to work in (§4d) — say both.
- **`CLAUDE.md`** (this file) — how to work in this repo.

## The spec is the contract

**`docs/spec.md` is the source of truth for the language.** Read the relevant section
before implementing anything, and cite section numbers in code comments and commit
messages (`spec §6.4`). Section numbers are stable — do not renumber them; if you must
insert a section, append rather than shift, and fix every `§N.M` cross-reference if you
truly have to (there is a reference audit one-liner at the bottom of this file).

- **§0.1** is a one-page summary of the whole language. Start there.
- **§18** is the decisions log. Every design question that has been settled is recorded
  with its resolution. **Do not silently re-decide anything in §18.** If implementation
  reveals a decision was wrong, say so explicitly and ask.
- **§19** is the roadmap (M0…M13). Work proceeds in milestone order.
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
or reversed §18 decision, or anything that changes what a valid Dazzle program looks like.

- One row per revision, newest first: `| rN | date | what changed and why |`.
- Write what a *user of the language* needs to know, not what the compiler now does.
  "`or` is non-associative" — not "added `CHAINED_OR` to `codes.rs`".
- A revision may cover several related edits; do not split one design change across rows.
- Bump the revision number in the document header at the same time.

**Compiler work is not a language change** and does not belong in §21 — new crates, new
diagnostics for existing rules, refactors, performance, test infrastructure. Those go in
`docs/PROGRESS.md`. The test: if a Dazzle program's meaning or spelling changes, it is §21;
if only the compiler's internals change, it is `PROGRESS.md`.

## Repository layout

```
docs/spec.md          the language specification (the contract)
docs/PROGRESS.md      build log, deferral register, next-milestone plan
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
editors/vscode/       the VS Code extension: TextMate grammar, language configuration, and
                      a client that runs `dz lsp` (see its README for building the .vsix)
tests/lex/*.dz        lexer snapshot corpus    (crates/dz_lex/tests/lex.rs)
tests/parse/*.dz      parser snapshot corpus   (crates/dz_parse/tests/parse.rs)
tests/fmt/*.dz        formatter corpus         (crates/dz_fmt/tests/fmt.rs)
tests/errors/*.dz     one program per mistake, whole pipeline, exact // expect: codes
                      (crates/dz_driver/tests/errors.rs) — where combinations are tested
crates/dz_driver/tests/feature_matrix/
                      every construct and mistake in every context, in two layouts; run
                      by dz_driver/tests/matrix.rs (mistakes) and dz/tests/matrix.rs
                      (constructs, compiled and executed). Add a row to extend it.
tests/expand/*.dz     macro expansions as `dz expand` prints them, snapshotted
                      (crates/dz_macro/tests/expand.rs)
tests/flow/*.dz       one file per ownership rule, each naming the code it must produce
                      in an `// expect:` line (crates/dz_flow/tests/flow.rs)
tests/run/            whole programs that are compiled and executed, each with an
                      expected.txt of exact stdout (crates/dz/tests/run.rs)
tests/workspace/      real multi-module workspaces (crates/dz_resolve/tests/resolve.rs):
                        good/        resolves cleanly — the M2 milestone case
                        private/     one violation of each §3/§4 rule
                        cycle/       circular module dependencies
                        badmanifest/ manifest validation
                        typed/       must type-check with ZERO diagnostics
                        typeerrors/  one deliberate failure per type rule
examples/             real projects, every one built by `every_example_builds`
                      (crates/dz/tests/run.rs): `raylib` and `rgfw` (M7b's criterion, each
                      opening a window), `breakout` (M12's, a raylib game whose rules are
                      `tests/run/breakout_sim/game`, which that test plays headless) and
                      `books`. The pre-spec sketches that used to live here were removed on
                      2026-10-20; git history has them.
```

Crates still to come are listed in spec §17.1. Follow that layout rather than inventing one.

## Commands

```bash
cargo build --workspace
cargo test --workspace           # 234 tests at M6–M11 + M14 + M15
cargo clippy --workspace --all-targets
cargo insta review               # review snapshot changes interactively
cargo run -p dz -- lex <file.dz>     # tokens
cargo run -p dz -- parse <file.dz>   # syntax tree
cargo run -p dz -- expand <file.dz>  # the file with every macro invocation expanded
cargo run -p dz -- fmt --stdout <file.dz>
cargo run -p dz -- check <dir>   # analyse only
cargo run -p dz -- cpp   <dir>   # generate C++ into <dir>/build/cpp
cargo run -p dz -- run   <dir>   # build and execute; DZ_CXX picks the C++ compiler
cargo run -p dz -- lsp           # the language server on stdio (what VS Code runs)
cd editors/vscode && npm install && npm run deploy    # build dazzle.vsix and install it
                                 # (--force: the version stays 0.1.0, so a plain install is a no-op)
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

## Environment gotchas

### NEVER use a heredoc. Not for files, not for scripts, not for anything.

This rule has been in this file since M1 and has been broken repeatedly — including three
times in the session that added `dz_hir`, each time costing a wasted edit and a confusing
failure. So, plainly:

> **Do not write `<<'EOF'`, `<<EOF`, `<<PY`, or any other heredoc in a Bash command.**
> Not to write a file. Not to pipe a Python or shell script into an interpreter. Not for
> "just a small one". There is no case where it is correct here.

**A heredoc is not faster, shorter, or cheaper than the alternative.** It is the same
number of tokens as `Write` and it fails silently, which makes it strictly worse. If you
catch yourself reaching for one because it "feels" more direct, that instinct is wrong in
this repo.

**What to do instead**, in order of preference:

1. **`Edit`** — for changing existing code. This is almost always the right tool, and it
   fails loudly when the match is wrong instead of writing something subtly different.
2. **`Write`** — for a new file, or a whole-file rewrite. Exact bytes, no escaping layer.
3. **A script that must run:** `Write` it to the scratchpad directory, then
   `python <path>` or `bash <path>` in a separate Bash call. Never pipe it in.

Why it is banned — heredocs **silently corrupt backslashes** in this environment. Verified behaviour of the *quoted* form `<<'EOF'`, which is supposed to be
completely literal:

| Written | Lands in the file | |
|---|---|---|
| `\\` | `\` | **corrupted** |
| `\t` | `\t` | ok |
| `$HOME` | `$HOME` | ok |
| `` `echo hi` `` | `` `echo hi` `` | ok |

So a heredoc is *nearly* literal, which is worse than being obviously broken — it works
until content contains a doubled backslash, then quietly changes it with no error. This
already caused two real bugs: it corrupted the `\\` escape-test cases in `tests/lex/`
(making the lexer look wrong when it was right), and it corrupted a Python script that was
itself being passed through a heredoc to fix the first problem.

Large heredocs have also failed outright with `unexpected EOF while looking for matching`.

Anything containing `\\` is affected: string-escape test fixtures, Windows paths, regexes,
Rust string continuations (`"long text \` at end of line), and any Rust or Dazzle source
with escapes. Note the second-order failure mode: a Python script passed through a heredoc
has *its* `\\` corrupted too, so `s.replace(a, b)` silently matches nothing and the script
reports success having done nothing. That is how this rule gets broken without noticing.

### Other

- Windows. The Bash tool is Git Bash (POSIX sh); PowerShell is also available. Prefer
  forward slashes in paths.
- `.gitignore` uses CRLF and had no trailing newline — appending with `>>` once produced
  `*.pdb/target`. Check the last byte before appending to any file.
- The `dz` binary is at `target/debug/dz.exe`.

## Handy checks

Audit that every `§N.M` cross-reference in the spec resolves to a real heading:

```bash
refs=$(grep -o '§[0-9]\+\.[0-9]\+' docs/spec.md | sed 's/§//' | sort -u -V); heads=$(grep -o '^### [0-9]\+\.[0-9]\+' docs/spec.md | sed 's/### //' | sort -u -V); tbl=$(grep -o '^| 18\.[0-9]\+' docs/spec.md | tr -d '| '); for r in $refs; do echo "$heads $tbl" | tr ' ' '\n' | grep -qx "$r" || echo "MISSING: §$r"; done
```

## History

The compiler was originally prototyped in Odin (lexer, diagnostics, a snapshot test
runner). That code was removed once the Rust port in `crates/` passed the same corpus with
better diagnostics. See spec §18 for why Rust was chosen. Git history has the Odin sources
if they are ever needed.
