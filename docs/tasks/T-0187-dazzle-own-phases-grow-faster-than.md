---
id: T-0187
title: Dazzle's own phases grow faster than the program: emitting, placement, ownership, instantiation
milestone: M15
status: done
kind: feature
area: tooling
where: `crates/dz_check/src/impls.rs`: `impls_for`, `impls_that_might_apply`, `impl_head`, and `impls_by_head` on `TypeTable` (`lib.rs`); `crates/dz_cpp/src/lib.rs`: `place_instance_body`, `emit_body`, `inline_only`, `param_names`, `struct_sig_or_empty`; `crates/dz_mono/src/lib.rs`: `Index`, `Mono::instance`; `crates/dz/src/main.rs`: `cmd_pipeline`
---

**Done** (2026-11-11). Every phase now grows with the program: the same table after, in
milliseconds, best of three.

| Phase | 10 modules | 25 | 50 | 100 |
|---|---:|---:|---:|---:|
| emit C++ | 90 | 202 | 398 | 808 |
| ownership | 51 | 93 | 162 | 302 |
| placement | 29 | 70 | 142 | 290 |
| instantiate generics | 26 | 62 | 117 | 232 |
| type check | 21 | 36 | 59 | 110 |
| all of Dazzle's phases | 235 | 498 | 937 | 1 844 |

Found by sampling stacks with gdb (`perf` is not allowed on the machine it was measured on).
Three causes:

- **`TypeTable::impls_for` tried every `impl` in the program** against the type it was asked
  about, and it is under every "does this type implement that trait" question: ownership's
  `elements_in`, placement, instantiation and emit all ask it, about every type they meet.
  Each module adds implementations (`Eq` and `Ord` for each struct here), so the questions
  and the implementations each grew with the program: 59% of the time at 100 modules. The
  implementations are now also indexed by the head of their type (`impls_by_head`: a
  declaration, or a shape such as a pointer or `i32`), and only those with the type's own
  head, and any for a bare parameter, are tried. `match_ty` never matches a type against a
  pattern with another head, so the answers and their order are the same.
- **`place_instance_body` emitted an instance's bodies once for every module that needs
  it** and threw the text away in all but the first, keeping only the `#[inline]` part for
  the header. An `inline_only` flag now makes `emit_body` skip the others there. The generated
  C++ is the same but for the numbers in temporaries' names.
- **Freeing the analysis** at the end of `dz build` took 0.1 s at 100 modules; the CLI now
  leaves that to the operating system (`ManuallyDrop`).

At 100 modules Dazzle's phases went from 4.5 s to 1.84 s; small programs gain too, `hello`
75 → 65 ms and the benchmark programs 90–101 → 74–80 ms.

A second round took emit from 0.79 s to 0.69 s at 100 modules, with the same C++: a header's
declaration of an instance's methods names only their parameters (`param_names`, rather than
walking each whole body), instances are looked up by declaration (`Mono::instance`,
`impl_instance`, `method_instance`) rather than by scanning every module's, and a struct's
signature (`struct_sig_or_empty`) and `Namer`'s nodes are borrowed rather than cloned.

**What is wrong.** T-0012's generated project (`tools/bench_scale.py`), a build with nothing
changed, clang++, release `dz`. Milliseconds per phase as the project doubles:

| Phase | 10 modules | 25 | 50 | 100 |
|---|---:|---:|---:|---:|
| emit C++ | 114 | 317 | 583 | 1 576 |
| ownership | 86 | 209 | 474 | 1 396 |
| placement | 45 | 142 | 352 | 1 104 |
| instantiate generics | 32 | 98 | 206 | 589 |
| type check | 24 | 50 | 81 | 211 |
| lex and parse | 12 | 20 | 27 | 52 |

Parsing grows with the program; the four at the top nearly triple each time it doubles. At
100 modules (51 000 lines) Dazzle's own phases are 4.6 s of every build, an edit or not.

**What it would take.** Profile the 100-module project to find what in each phase is not
linear: a scan of every module or every instance inside a loop over them is the usual
shape. Separate from T-0023 (re-analysing only what changed), which would hide the slope
for an edit but not for a build from clean. Compiler work, for `docs/log/`.

