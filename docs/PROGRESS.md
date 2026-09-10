# Dazzle — build log and working register

Companion to `docs/spec.md` (the language contract) and `CLAUDE.md` (how to work in this
repo). This file answers three questions the other two do not:

1. **What is actually built**, milestone by milestone, and what each one decided.
2. **What has been deliberately postponed**, to which milestone, and why — the deferral
   register in §3 is the authoritative list, gathered from the code itself.
3. **What to do next.**

Keep it current at the end of every milestone. If a deferral in §3 is resolved, delete the
row; if a new one is introduced, add it there *and* leave a comment in the code that names
the milestone.

---

## 1. Status

| | |
|---|---|
| Milestones done | M0, M1, M2, M3, **M4 — Dazzle programs compile and run** |
| Next | **M5 — ownership: `own`, `move`, `copy`, `drop`, flow analysis** |
| Tests | 79, all green |
| Clippy | clean (`--workspace --all-targets`) |
| Compiler crates | 9 |
| Language spec | v0.4, no open questions |
| C++ compilers verified | clang++ 21, g++ 13 (`DZ_CXX` overrides the choice) |

```bash
cargo test --workspace
cargo clippy --workspace --all-targets
cargo run -p dz -- check tests/workspace/typed
cargo run -p dz -- run   tests/run/features
```

### Crates

| Crate | Does | Spec |
|---|---|---|
| `dz_span` | spans, `SourceMap`, line/column | §2.1 |
| `dz_diag` | diagnostic model, codes, rendering | §17.1 |
| `dz_lex` | lexer | §2 |
| `dz_ast` | syntax tree + span-free dump | §20 |
| `dz_parse` | recursive descent + Pratt | §20 |
| `dz_fmt` | formatter | §17.3 |
| `dz_project` | `dazzle.toml`, globs, dependency graph | §3.1 |
| `dz_resolve` | symbol tables, name resolution, visibility | §3, §4 |
| `dz_check` | type checker | §5, §7, §9, §10, §11 |
| `dz_cpp` | C++17 emission + the runtime shim | §16 |

Crates still to come are listed in spec §17.1: `dz_hir`, `dz_mono`, `dz_cppimport`,
`dz_macro`, `dz_lsp`.

---

## 2. What each milestone built

### M0 — spans, diagnostics, lexer

Ported the Odin prototype to Rust. The Odin sources were deleted once this passed the same
corpus with better diagnostics; git history has them.

- **Spans are byte offsets into the file as it sits on disk.** Line endings are recognised
  during the scan rather than by rewriting the buffer, and a UTF-8 BOM stays in the text and
  is skipped as whitespace. This is what keeps `#line` mapping (§16.3) honest.
- **Our own diagnostic model, rendering delegated to `codespan-reporting`.** Every phase
  builds `Diagnostic` values; the renderer can be swapped, or joined by an LSP serialiser,
  without touching any of them.
- **Diagnostic codes are stable and never reused** (`crates/dz_diag/src/codes.rs`). Tests
  assert on codes, never on message text, so wording stays free to improve.
- `^` does not suppress an end-of-line terminator: it is *postfix* dereference, so `let b =
  p^` is a complete statement. Neither does `>`, which also closes generic arguments.

### M1 — parser, AST, formatter

- **Turbofish removed.** `Buffer<f32>::with_capacity(8)`, not `Buffer::<f32>::…`. Safe
  precisely because comparison is non-associative (§11.1), so `a < b > (c)` is already a
  syntax error and the ambiguity Rust works around does not exist here.
- **`>>` closing nested generics** (`Buffer<Buffer<f32>>`) is split into a synthetic second
  `>` that `peek`/`bump` serve transparently, so no other part of the parser knows.
- **Types parse generics greedily; expressions parse them speculatively.** One shared path
  parser cannot do both.
- **One diagnostic per token position.** `fn f( {` used to report four times at the same
  character; the cascade guard cut `errors.dz` from 12 diagnostics to 9 distinct ones.
- **"Round-trips" is pinned as three properties**, because a formatter that eats comments
  round-trips nothing: formatting is idempotent, preserves the tree (compared through a
  span-free AST dump), and re-parses cleanly.

### M2 — projects, modules, imports, visibility

- **Hand-rolled the TOML subset** rather than take the `toml` crate (which pulls serde + syn
  + quote). The manifest is our own small format, and writing it bought what the crate would
  not: every key and value keeps its span, so `output = "library"` puts a caret under
  `"library"` and lists the valid choices.
- **Paths are normalised lexically, not with `canonicalize`** — the latter needs the path to
  exist and returns `\\?\` UNC paths on Windows that would then appear in every diagnostic.
- **A builtin prelude**, each name grounded in a specific spec section rather than invented:
  `sizeof`/`alignof` §5.5, `error` §9.1, `panic` §9.8, `println`/`format` §12.9,
  `typeinfo_of` §13.1, `construct`/`destroy` §14.3.

### M3 — type checker

- **`typed/` and `typeerrors/` are a matched pair.** A checker that rejects valid code is as
  broken as one that accepts invalid code, and only the pair catches both failures.
- **Named calls route through the signature, not through `Ty::Fn`**, because a function type
  does not record which parameters have defaults (§7.2).
- **`error(...)` is the failure case of any `!T`** — its payload is `Never`, so it unifies
  with whatever success type the context wants (§9.1).
- Live consequence of decision §18.1: `let mut total = 0.0` is **f64**, so `total += v.x` on
  an `f32` field is an error. That is the f64 literal default meeting the
  no-implicit-conversions rule. Correct, and one annotation to fix, but it will come up in
  f32-heavy game code.

### M4 — codegen, runtime shim, `dz run`

**Hello world runs**, and so does a program exercising structs, enums, methods,
constructors, control flow, pointers, arrays, casts, `switch` and `defer`.

- **No HIR was built, contrary to the M4 plan.** The plan said to introduce `dz_hir` first.
  While building it became clear that emitting C++ needs far less type information than
  expected, because *C++ has its own inference*: `let x = e` becomes `auto x = e`, and three
  one-line shim helpers remove the rest of the need — `dz::obj(x)` resolves
  value/reference/pointer for `.`, `dz::addr(x)` yields a proxy convertible to both `T&` and
  `T*`, and `dz::cast<T>` picks `static_cast` or `reinterpret_cast`. Building an IR that
  M5's flow analysis would immediately rewrite was the worse trade. **HIR is now owed by
  M5**, which genuinely needs it. The one place codegen still needs a type it cannot read
  off the syntax is a `switch` subject, since C++ `enum class` labels must be qualified; a
  small table of enum-typed bindings covers it and disappears when the HIR lands.
- **The shim's templates are not a retreat from §8.6.** That section argues against mapping
  Dazzle *generics* onto C++ templates, because C++ then re-checks them per instantiation,
  slowly and with bad errors. These are a handful of one-line inline functions.
- **`&mut T` now satisfies `^T`** (`dz_check/src/ty.rs`). Spec §5.2 writes exactly this
  (`let p: ^Player = &mut world.players[0]`) and the checker did not allow it. Giving up the
  non-null guarantee is safe; the reverse is not, and neither is `&T`.
- **Test on more than one C++ compiler.** The shim used `__is_pointer`, a Clang builtin GCC
  does not have, so the output compiled on clang++ and failed on g++. Both are now verified,
  warning-clean under `-Wall -Wextra -fno-exceptions -fno-rtti`.
- **`abort()` does not flush.** A panic was discarding everything the program had printed —
  exactly the output someone needs in order to debug the panic.
- **A failing exit status must never come back as 0.** Windows reports an abort as
  `0xC0000409`, negative as an `i32`; clamping it into `0..=255` reported a crash as success.
- **The access helpers are always-inline, not merely `inline`.** Measured: at `-O0` each
  `xs[i]` was three real calls, which would wreck debug-build framerate. With
  `always_inline` the only surviving call is the cold `panic_bounds`. At `-O2` the
  generated function's assembly is byte-identical to hand-written C++, bounds checks on or
  off — with them on, the optimiser proves the loop condition already implies the check.
  A test asserts this so it cannot silently regress.
- **C++20 is opt-in** (`cpp_standard = "c++20"`). It buys designated initialisers for
  struct literals — real readability, nothing else. C++17 remains the default.

---

## 3. Deferral register

Everything knowingly postponed. Each row has a comment in the code naming the same
milestone. **This is the list to check before claiming a milestone is complete.**

| Owed by | What is deferred | Where |
|---|---|---|
| **M5** | **HIR.** Types are still computed and discarded. M4 got by on C++'s own inference, but flow analysis needs a real IR with stable node ids. Introduce the `dz_hir` of spec §17.1 — do **not** bolt a span-keyed map onto the AST | `dz_check/src/lib.rs` header |
| **M5** | Ownership: `own`, `move`, `copy`, `drop`, use-after-move, `own` field without `drop` (DZ0412) | spec §6 |
| **M5** | Definite return: a non-void function with a path that falls off the end | not yet reported |
| **M5** | Codegen: `?T`/`!T` with `try`/`with`/`catch` — needs the statement-level desugaring HIR gives | `dz_cpp`, reports `DZ0801` |
| **M5** | Codegen: tagged unions and `switch` over them; needs payload layout and drop rules | reports `DZ0801` |
| **M5** | Codegen: tuples, tuple destructuring, and `if`/`switch`/block used as a *value* | reports `DZ0801` |
| **M5** | Codegen: `move(x)` emits a copy, and `copy`/`drop` are not emitted as C++ special members | `dz_cpp/src/lib.rs` |
| **M6** | Generic bodies are skipped, not checked; generic items are not emitted | `dz_check/src/check.rs`, `dz_cpp` |
| **M6** | Trait conformance: `struct Arena : Allocator` resolves the name but nothing verifies the methods | `dz_check/src/lib.rs` |
| **M6** | `for` over user types needs `Iterate` | `dz_check/src/check.rs` |
| **M6** | Constant folding; array lengths accept only literals; `const` without an annotation infers from literals only | `dz_check/src/lib.rs` |
| **M6** | `sizeof`/`alignof` and the reflection builtins are not emitted | `dz_cpp`, reports `DZ0801` |
| **M7** | `import cpp "…"` is accepted and the alias is opaque; no header is read | `dz_resolve/src/walk.rs` |
| **M10** | Macro declarations parse shallowly; bodies are kept as raw source and reproduced verbatim by the formatter | `dz_ast/src/lib.rs`, `dz_fmt/src/lib.rs` |
| **M12** | No stdlib. `dz_prelude.h` stands in for §12.9's `Format` trait so `println` works at all. The operator traits of §11.2 (`Ord`, `Add`, …), `Iterate`, `Allocator`, `String`, `Buffer` do not exist, so naming one is correctly an unresolved-name error | `dz_cpp/src/runtime.rs` |

### Not deferred — deliberately out of scope forever

- Lifetimes. `&T` cannot be stored in a struct field (§5.2, §18.3) and that is the whole
  mechanism; a dangling `^T` is the programmer's problem (§1.1).
- Function overloading, until a later language version (§18.5).

---

## 4. Next: M5 — ownership

Ends when `Buffer<T>` and `String` compile and run leak-free under ASan (spec §19). Those
are *generic*, so M5 and M6 overlap; the ownership rules themselves can be built and tested
on non-generic types first.

Planned order:

1. **`dz_hir`** — the resolved, desugared, typed tree M4 deferred. Flow analysis needs a
   real IR with stable node ids, and codegen needs types for `?T`/`!T` lowering. Build it as
   the checker's output rather than as a separate pass.
2. Ownership rules (§6): `own` makes a type move-only; `copy` and `drop` become C++ special
   members with the `drop() + reset-to-defaults + copy()` assignment lowering of §6.6;
   move-only types get their C++ copy constructor `= delete`d as a second line of defence.
3. Flow analysis: use-after-move, partial moves, `own` field with no `drop` (DZ0412),
   definite return.
4. Codegen for everything marked M5 in §3: `?T`/`!T`, tagged unions, tuples, block-valued
   expressions.
5. Tests: `tests/flow/*.dz` for the ownership diagnostics, and a run test that allocates and
   frees to prove `drop` fires in the right order.

Constraints already decided:

- A moved-from value is reset to its defaults, never left unspecified (§6.4).
- `drop` runs in reverse declaration order, and `defer` after that (§6.5).
- No lifetimes, ever. `&T` in a struct field stays banned (§18.3).

---

## 5. Test map

| Corpus | Runner | Covers |
|---|---|---|
| `tests/lex/*.dz` | `dz_lex/tests/lex.rs` | tokens + lexer diagnostics, snapshotted |
| `tests/parse/*.dz` | `dz_parse/tests/parse.rs` | AST + parser diagnostics, incl. `errors.dz` for recovery |
| `tests/fmt/*.dz` | `dz_fmt/tests/fmt.rs` | idempotence, tree preservation, comment survival |
| `tests/workspace/good` | `dz_resolve/tests/resolve.rs` | three-module workspace, resolves clean |
| `tests/workspace/private` | " | one violation per §3/§4 rule |
| `tests/workspace/cycle` | " | circular module dependencies |
| `tests/workspace/badmanifest` | " | manifest validation |
| `tests/workspace/typed` | `dz_check/tests/check.rs` | **must produce zero diagnostics** |
| `tests/workspace/typeerrors` | " | one deliberate failure per type rule |
| `tests/run/*/` | `dz/tests/run.rs` | whole programs compiled by a real C++ compiler and executed, stdout compared against `expected.txt`. A first line of `#panic` marks a program that must abort |

`examples/*.dz` are **not** tests. They are the user's pre-spec design sketches and are not
expected to parse.
