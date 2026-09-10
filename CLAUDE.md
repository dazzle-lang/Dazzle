# Dazzle — working notes for Claude Code

Dazzle is a systems language for game development that compiles to readable C++17. It is
C++-grade in control, but with RAII reduced to three special members, enforced
encapsulation, and definition-site generics. The compiler is written in Rust.

Progress: M0 (lexer), M1 (parser, AST, formatter), M2 (projects, modules, imports,
visibility), M3 (type checker) and M4 (codegen, runtime shim, `dz run`) are done —
**Dazzle programs compile and run**. Next is M5 — ownership: `own`, `move`, `copy`,
`drop` and flow analysis. See spec §19 for the full roadmap.

## Three documents

- **`docs/spec.md`** — the language contract. See below.
- **`docs/PROGRESS.md`** — the build log: what each milestone built and decided, the
  **deferral register** (everything knowingly postponed, and to which milestone), and the
  plan for the next milestone. Read it before starting work; update it when you finish a
  milestone.
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

The user drives language design; Claude writes the compiler. Propose alternatives freely,
but design changes are the user's call, not something to slip into an implementation.

## Repository layout

```
docs/spec.md          the language specification (the contract)
docs/PROGRESS.md      build log, deferral register, next-milestone plan
crates/
  dz_span/            Span, SourceMap, line/column mapping
  dz_diag/            Diagnostic model + rendering (codespan-reporting)
  dz_lex/             the lexer (spec §2)
  dz_ast/             the syntax tree, plus a span-free `dump` used by tests and `dz parse`
  dz_parse/           recursive-descent + Pratt parser (spec §20)
  dz_fmt/             the formatter
  dz_project/         dazzle.toml, source globs, the dependency graph (spec §3.1)
  dz_resolve/         module symbol tables, name resolution, visibility (spec §3, §4)
  dz_check/           the type checker (spec §5, §7, §9, §10, §11)
  dz_cpp/             C++17 emission + the runtime shim (spec §16)
  dz/                 the `dz` CLI
tests/lex/*.dz        lexer snapshot corpus    (crates/dz_lex/tests/lex.rs)
tests/parse/*.dz      parser snapshot corpus   (crates/dz_parse/tests/parse.rs)
tests/fmt/*.dz        formatter corpus         (crates/dz_fmt/tests/fmt.rs)
tests/run/            whole programs that are compiled and executed, each with an
                      expected.txt of exact stdout (crates/dz/tests/run.rs)
tests/workspace/      real multi-module workspaces (crates/dz_resolve/tests/resolve.rs):
                        good/        resolves cleanly — the M2 milestone case
                        private/     one violation of each §3/§4 rule
                        cycle/       circular module dependencies
                        badmanifest/ manifest validation
                        typed/       must type-check with ZERO diagnostics
                        typeerrors/  one deliberate failure per type rule
examples/*.dz         the user's design sketches. NOT valid Dazzle and NOT expected to
                      parse — they are prototyping scratch from before the spec existed
                      (nested `struct` in `union`, native `machine`, `def`, `@(...)`).
                      Design input only. Do not "fix" them, and do not treat a parse
                      failure on them as a bug.
```

Crates still to come are listed in spec §17.1. Follow that layout rather than inventing one.

## Commands

```bash
cargo build --workspace
cargo test --workspace           # 79 tests at M4
cargo clippy --workspace --all-targets
cargo insta review               # review snapshot changes interactively
cargo run -p dz -- lex <file.dz>     # tokens
cargo run -p dz -- parse <file.dz>   # syntax tree
cargo run -p dz -- fmt --stdout <file.dz>
cargo run -p dz -- check <dir>   # analyse only
cargo run -p dz -- cpp   <dir>   # generate C++ into <dir>/build/cpp
cargo run -p dz -- run   <dir>   # build and execute; DZ_CXX picks the C++ compiler
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
- **Comments** explain *why*, especially where a choice looks arbitrary. Do not narrate
  what the code already says.
- **Do not commit or push unless the user asks.**

## Environment gotchas

### Do not use heredocs to write file content

Shell heredocs (`cat > file <<'EOF' … EOF`) **silently corrupt backslashes** in this
environment. Verified behaviour of the *quoted* form `<<'EOF'`, which is supposed to be
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

**Instead:** use the `Write` and `Edit` tools for all file content. They write exact bytes.
Use Bash for running commands, not for authoring files. If a file genuinely must be built
by a script, have the script read its input from a file written by `Write`.

Anything containing `\\` is affected: string-escape test fixtures, Windows paths, regexes,
and any Rust or Dazzle source with escapes.

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
