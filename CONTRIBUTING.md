# Contributing to Dazzle

Thank you for looking. Dazzle is **pre-release**: the language still changes from week to
week, and it is designed by one person. That shapes what kind of help fits best right now.

## What is welcome

- **Bug reports.** A program the compiler gets wrong is the most useful thing you can send:
  a crash, a diagnostic that points at the wrong place or says the wrong thing, a valid
  program it rejects, an invalid one it accepts, or generated C++ that does not compile
  (that one is always a compiler bug, never yours). Please include the smallest `.dz`
  program that shows it, what you expected, and what `dz --version`, your OS and your C++
  compiler are.
- **Confusing errors.** If a diagnostic did not tell you what to do, that is a bug too. The
  compiler is meant to teach the language, so "I did not understand this message" is a
  report worth filing.
- **Questions and opinions about the design**, as an issue. Disagreement is welcome, and
  the reasoning behind every settled decision is in [`docs/decisions/`](docs/decisions),
  so start there: an argument that engages with the recorded reasons is far more likely to
  change something.
- **Pull requests that fix bugs, improve diagnostics, tooling or documentation**, or that
  take an open task from the backlog, [`docs/tasks.md`](docs/tasks.md). For anything larger
  than a small fix, open an issue first so we can agree on the approach.

## What is not, yet

**Pull requests that change the language** (new syntax, new keywords, different semantics)
will not be merged as they stand, however good. Language design goes through an issue and
ends as a recorded decision in `docs/decisions/` and a revision row in the spec's
changelog (§21); the implementation follows from that. Open the issue instead, and the
idea is not lost.

## Building and testing

You need a stable Rust toolchain, clang++ or g++, and for the C and C++ interop tests,
libclang. The [README](README.md#installation) has the details.

```bash
git clone --recursive https://github.com/dazzle-lang/Dazzle.git
```

```bash
cargo build --workspace
```

```bash
cargo test -p dz_check
```

Test the crate you touched; the whole suite (`cargo test --workspace`) compiles and runs
hundreds of C++ programs and takes a while. Before you open a pull request:

```bash
cargo fmt --all
```

```bash
cargo clippy --workspace --all-targets -- -D warnings
```

CI runs both on a pinned toolchain, and the tests on Linux, macOS and Windows.

## How the repository works

[`CLAUDE.md`](CLAUDE.md) is the working guide. It is written for an AI assistant but holds
for everyone: the layout, the commands, and above all **the invariants that must not be
broken**. The short version:

- **The spec in [`docs/spec/`](docs/spec/README.md) is the contract.** Read the section you
  are touching, and cite it in comments and commit messages (`spec §6.4`).
- **Diagnostic codes are stable and never reused**, and tests assert on codes, not on
  message text, so wording can always improve.
- **Errors point at the user's code**, never at the generated C++.
- **The lexer and parser never give up**: every error produces a diagnostic and a node, so
  the language server keeps working on half-typed code.
- **A checker rule gets a test on both sides**: a program it must accept in
  `tests/workspace/typed/` and one it must reject in `tests/workspace/typeerrors/`.
- **A test must not depend on the machine it runs on**: no OS error text, line endings or
  environment variables in a snapshot.

[`docs/testing.md`](docs/testing.md) says which test corpus covers what, so you know where a
new test belongs.

## License

By contributing, you agree that your contribution is licensed under the
[MIT License](LICENSE), like the rest of the project.
