# The Dazzle Language Specification

**Version:** 0.1 — **unreleased**, revision 94
**Status:** M0–M8, M10, M11, M14 and M15 implemented, and most of M9 and M12 — Dazzle
programs compile and run, own, move and drop their resources deterministically, have
generics, traits, unions, macros and reflection, call C and C++ libraries that `dz`
builds from vendored source, and are edited with a language server. Not yet 0.1: the
editor still lacks code actions and incremental analysis (M9), and the standard library,
which has carried a game written without C++ (M12's criterion), is still growing.
Open work is the backlog, `docs/tasks.md`; what was built, milestone by milestone, is in `docs/log/`.

**The version number stays 0.1 until 0.1 is actually finished.** 0.1 means: an LSP good
enough to work in (§19 M9), a stdlib you can write a game against (M12), and C++ interop
(M7). Until all three land, this document is revised, not versioned — the revision numbers
in §21 track *changes to the language*, and none of them is a release.
**Target:** a systems language that compiles to readable C++17, for the places C++ is used
today: games and engines, embedded systems, tools, and real-time code.

One file per top-level section; **start with §0.1**, the whole language on one page. Section
numbers are stable and are what code, commits and the other documents cite (`spec §6.4`);
the file names only make them findable.

| § | Section |
|---|---|
| §0 | [How to read this document](00-how-to-read-this.md) |
| §1 | [Design principles](01-design-principles.md) |
| §2 | [Lexical structure](02-lexical-structure.md) |
| §3 | [Projects, modules, and imports](03-projects-modules-and-imports.md) |
| §4 | [Visibility](04-visibility.md) |
| §5 | [Types](05-types.md) |
| §6 | [Value semantics: ownership, copy, move, drop](06-value-semantics-ownership-copy.md) |
| §7 | [Declarations](07-declarations.md) |
| §8 | [Generics](08-generics.md) |
| §9 | [Errors and absence](09-errors-and-absence.md) |
| §10 | [Statements and control flow](10-statements-and-control-flow.md) |
| §11 | [Operators](11-operators.md) |
| §12 | [Metaprogramming](12-metaprogramming.md) |
| §13 | [Reflection](13-reflection.md) |
| §14 | [Memory and allocators](14-memory-and-allocators.md) |
| §15 | [C++ interoperability](15-c-interoperability.md) |
| §16 | [Code generation](16-code-generation.md) |
| §17 | [Compiler architecture and toolchain](17-compiler-architecture-and-toolchain.md) |
| §18 | [Decisions log](18-decisions-log.md) |
| §19 | [Roadmap](19-roadmap.md) |
| §20 | [Appendix: grammar sketch](20-appendix-grammar-sketch.md) |
| §21 | [Changelog](21-changelog.md) |
