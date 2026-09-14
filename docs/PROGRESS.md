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
| Milestones done | M0–M5, **M6 — generics, traits, monomorphization**, **M7 — C and C++ interop from vendored source** (both halves; what is left of it is narrower than the milestone and sits in §3); most of **M9 — language server and VS Code extension**, pulled forward. **Struct inheritance** (18.32) landed as r30. **M11 — reflection** landed as r51. **M8** is done — its `ButtonState` criterion runs as of r38 — and r37 settled how `for` and `case` bindings hold what they name |
| Next | **M12 — the stdlib**, chosen over M10/M11 by the user on 2026-09-13; M9's remaining editor rows stay open. r39 laid the language groundwork (`static`, associated functions, trait objects). Next in order: `#location` evaluated at the caller, associated constants, then `Allocators::Global`/`Temp` with opt-in tracking. See §4, and §4c for everything a game still needs — smart pointers (`Rc`/`Weak`/`Box`) first |
| Tests | 193, all green — plus the `tests/errors` corpus, one file per mistake, run as one test |
| Clippy | clean (`--workspace --all-targets`) |
| Compiler crates | 16, plus the `dz` CLI |
| Language spec | 0.1 **unreleased**, revision 42; three open questions (§18.2) — 18.18 `collect`, 18.19 `ignore`, 18.20 `or unreachable`'s release lowering |
| C++ compilers verified | clang++ 21, g++ 13 (`DZ_CXX` overrides the choice) |

```bash
cargo test --workspace
cargo clippy --workspace --all-targets
cargo run -p dz -- check tests/workspace/typed
cargo run -p dz -- run   tests/run/ownership
cargo run -p dz -- hir   tests/workspace/typed
```

Ownership is checked against a sanitizer rather than against reading. The clang runtime
has to be on `PATH` on Windows, which is the only fiddly part:

```bash
cargo run -p dz -- cpp tests/run/ownership
cd tests/run/ownership/build/cpp
clang++ -std=c++17 -fno-exceptions -fno-rtti -fsanitize=address -g -O1 *.cpp -o asan.exe
./asan.exe
```

### Crates

| Crate | Does | Spec |
|---|---|---|
| `dz_span` | spans, `SourceMap`, line/column | §2.1 |
| `dz_diag` | diagnostic model, codes, rendering | §17.1 |
| `dz_lex` | lexer | §2 |
| `dz_ast` | syntax tree + span-free dump | §20 |
| `dz_hir` | resolved, typed tree + `Ty` + span-free dump | §17.1 |
| `dz_parse` | recursive descent + Pratt | §20 |
| `dz_fmt` | formatter | §17.3 |
| `dz_project` | `dazzle.toml`, globs, dependency graph, **the standard library** (`src/std/**/*.dz`) | §3.1, §3.2 |
| `dz_resolve` | symbol tables, name resolution, visibility | §3, §4 |
| `dz_check` | type checker | §5, §7, §9, §10, §11 |
| `dz_flow` | ownership, moves, definite return | §6 |
| `dz_mono` | monomorphization: the instances each module needs, and their concrete bodies | §8.4 |
| `dz_cpp` | C++17 emission + the runtime shim | §16 |
| `dz_cppimport` | reads a C header through libclang (`clang-sys`, loaded at run time) into plain declarations; an in-process cache | §15 |
| `dz_driver` | the analysis pipeline as a library (`analyse`), with editor buffers able to stand in for files | §17.2 |
| `dz_lsp` | the language server, `dz lsp`; the queries themselves are protocol-free, in `ide.rs` | §17.4 |

Crates still to come are listed in spec §17.1: `dz_macro`. The VS Code
extension is `editors/vscode`: plain JavaScript, a TextMate grammar, and a client that runs
`dz lsp`.

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
- **`error(...)` is the failure case of any fallible signature** — its payload is `Never`,
  so it unifies with whatever success type the context wants (§9.1).
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

### M5 — ownership: `own`, `move`, `copy`, `drop`, flow analysis

**Programs own, move and drop their resources deterministically**, and the C++ that comes
out is warning-clean and ASan-clean on both compilers.

- **Codegen moved onto the HIR, and that was the whole unlock.** M4 emitted C++ from the
  AST and let C++ infer the rest. `or`, block values and moves cannot work that way: `f()
  or return` is three C++ statements, and whether `x = y` copies or moves needs `y`'s type.
  Once `dz_cpp` read the typed tree, the five `DZ0801` rows M4 left behind — `T?`, fallible
  returns, tagged unions, tuples, block-valued `if`/`switch` — fell out in one pass.
- **Anything that is an expression in Dazzle and a statement in C++ hoists.** `expr()` may
  write lines *before* the statement being built and leave a temporary as its value. An
  immediately-invoked lambda would have been shorter and is wrong: `return` inside one
  returns from the lambda, so `or return` — the most common clause in the language — would
  silently not propagate. Two places need more than hoisting: a `while` condition, whose
  hoisted work has to run every iteration, and the right operand of `&&`/`||`, which must
  not run at all when the left short-circuits. Both become an `if` instead.
- **The declaration/body split is now explicit.** The HIR has bodies but no items, so
  `dz_cpp` reads declarations from the AST and the checker's signature table, and bodies
  from the HIR. That is not a wart — it is the reason a struct's field *names* and a
  function's *body* have different sources of truth.
- **`own` describes the struct, not the pointer.** The first version made `own ^T` a
  move-only *value*, which meant `if handle != null` inside `File::drop` was a move out of
  `self`. Spec §6.3 is careful to say "`own` on a field": the pointer is a bit-copy, the
  struct that declares it is what is move-only. Now written down as `[DECIDED]` (spec r7).
- **A move needs a place the function owns** (`DZ0402`). `move` resets its source, and a
  `&`, a `^` or an array element is not something the function has the right to reset.
  `dz_flow` classifies every expression as owned, borrowed or temporary, and only the first
  can be moved out of.
- **Union merge, not may/must.** At a join, a place moved on *any* path is moved after it,
  because §6.4 makes "moved on some paths reaching here" an error at the use. A loop body
  is replayed from `entry ∪ what-it-moves`, which is what turns "moved on the previous
  iteration" into a diagnostic rather than silence.
- **One mistake, one diagnostic.** The first version reported a use-after-move twice — once
  for the `move(…)` node's span and once for the operand inside it — and reported both
  `DZ0403` and `DZ0401` for a copy of an already-moved value. Both are now suppressed at
  the source, and `tests/flow` asserts the *count*, not just the presence.
- **Drop timing was ABI-dependent, and is not any more.** The same program printed its
  drops in a different order under g++ and clang++: under the Itanium ABI the *caller*
  destroys a by-value parameter at the end of the full expression, under the MSVC ABI the
  *callee* destroys it before returning. Spec §6.5 says the enclosing scope, which for a
  parameter is the function. Codegen now moves every owning by-value parameter into a local
  at function entry; the local dies at the end of the body on every ABI, and what the ABI
  eventually destroys is the moved-from husk, whose `drop` §6.4 already guarantees is a
  no-op. The language's own reset rule is what makes the fix free.
- **Placement `new` is the one C++ header the shim needs.** §6.6's copy assignment is
  "destroy, put back at defaults, then copy", and the middle step has no other spelling.
  `<new>` declares the operator and pulls in no runtime.
- **`Error` and `SourceLocation` became language-provided types.** `or { e -> … }` binds an
  `Error`, and until M12 there is no stdlib for one to come from — so `e.message` resolved
  to nothing and printed `0`. They are `Ty::Builtin`, not synthesised symbols: a fake
  `SymbolId` would put two kinds of thing in the symbol table and make every consumer ask
  which it had.
- **Three checker bugs the HIR switch exposed**, all of which had been passing silently
  because an unresolved name types as `Ty::Error` and `Ty::Error` is compatible with
  everything. `Layer::Ui` resolved to nothing (`path_symbol` only ever tried a module
  alias for a multi-segment path). `Shape::Circle{ radius: 2 }` was accepted without
  checking a single field. `copy(other: &Self)` bound `other` as `&Self` rather than
  substituting the enclosing type, so `other.n` failed. **`Ty::Error` compatibility is a
  cascade guard, not a licence** — anything that produces it without also producing a
  diagnostic is a hole, and codegen is where the hole finally shows.
- **Generated names follow scopes, not a counter.** Dazzle lets a `let` shadow another and
  C++ does not, so a collision has to be renamed. Renaming on "this name was seen before"
  turned every second `for i in …` into `i__3`; renaming on "this name is *still in scope*"
  keeps both `i`s. The generated code is meant to be read (§16.2).
- **Formatting is written against a sink.** `error(…)` and `format(…)` need the same `{}`
  substitution writing somewhere other than the console, so `write_value`/`write_format`
  take an output parameter. Messages land in a small static ring buffer: §9.2 requires that
  `error(…)` never allocate *and* that the message outlive the call, and a ring is the
  cheapest thing that does both. It is bounded — the fifth live message overwrites the
  first — which is a real limit, stated rather than hidden.


#### Step 1 — `dz_hir`, the tree the rest is built on

The tree the rest of M5 is built on. `dz check` produces it for every non-generic body;
`dz hir <dir>` prints it, and `hir_typed.snap` pins the whole tree for the `typed/` fixture.

- **The checker builds it while checking**, not afterwards. Every rule now returns both a
  type and the node it built (`Checked`), because a second pass would recompute the same
  types and could disagree with the first.
- **Three properties define the IR**, and each is a rule for anything added later: every
  name is already bound (a variable is a `LocalId`, a field an index into declaration
  order); every expression carries the type the checker concluded; node identity is stable,
  so a dataflow pass can key a bitset on `ExprId`.
- **`Ty` moved from `dz_check` into `dz_hir`**, since every node stores one and `dz_hir`
  sits below the checker. `dz_check::ty` re-exports it, so nothing else moved.
- **Literal defaulting happens at `finish()`, not at the leaf.** Defaulting a literal's type
  when its node is pushed decides `0` is `i32` before `0..xs.len` ever sees `usize` — it
  broke exactly that case. A node still holding `{integer}` when the body ends never met a
  context, so its default is right by definition. A test asserts no untyped literal survives.
- **An unqualified field in a method body stays a field.** §7.3 puts a struct's fields in
  scope inside its methods, so scope entries record whether a name is a local or `self.f`;
  without that the HIR would turn a field into a local that codegen emits as an undeclared
  identifier.
- **Place analysis rolls the arena back.** It re-checks an already-checked expression to ask
  whether it is assignable and throws the answer away; without a rollback the arena
  collected a duplicate subtree per query, which every later pass would have to know to
  ignore.
- **Literals hold values, not source text.** The AST keeps the exact characters because the
  formatter must reproduce them; the HIR keeps the decoded bytes because everything
  downstream wants those. `copy` and `drop` get `BodyOwner` variants of their own rather
  than being methods named "copy" and "drop", because flow analysis asks "does this type
  have a `drop`?" constantly and that should not be a string comparison.

Moving `dz_cpp` onto this tree was step 2, and it is what unblocked the `T?`,
fallible-return, tagged-union, tuple and block-value codegen rows.

#### Step 1a — spec r5/r6, the access/modality split

Landed between step 1 and step 2, because both new checks live in the code M5's ownership
rules then rewrote. Spec §5.6 and §9 are the contract; what the compiler now does:

- **`?` is postfix and binds to the type name.** `^T?` is a pointer to an optional. The
  type parser wraps whatever it just parsed, so access sigils — parsed as prefixes — always
  sit outside it, and no grammar ambiguity arises.
- **`!` left the type system.** `!->` is a single lexer token and sets `FnDecl::fallible`;
  there is no `TypeKind::Fallible` any more. `Ty::Fallible` survives *inside the checker
  only*, as the type of a call between the call and the `or` that must consume it — which
  is exactly what makes the consumption rule enforceable rather than advisory.
- **One `or` replaced `with`/`try`/`catch`**, in the AST, the HIR, the resolver, the
  formatter and the checker. Bare `or return` is recognised as propagation before the
  general clause rule, since a bare `return` in a non-void function would otherwise be
  reported as a missing value.
- **`^T` → `&T` is now checked** (DZ0522), and `or` on a `^T` produces the `&T`.
- New codes: DZ0522 (pointer needs a check), DZ0523 (`or` clause type), DZ0524 (`ignore`
  in value position), DZ0525 (`or` on something that cannot fail). DZ0516 keeps its number
  and changes meaning from "`try` outside a fallible function" to "the failure has nowhere
  to go", which is the same rule under the new spelling.
- **`or ignore` needed real statement-position tracking.** An expected type is not the
  signal — `let v = f()` and a bare `f()` both arrive with no expectation — so the checker
  carries a flag, cleared when descending into an operand.
- **`or` is non-associative** (r6), enforced in the parser with `DZ0211` the same way
  chained comparison is. Nesting is written with the block clause — `a() or { b() or c }`
  — so each fallible call has exactly one clause and the nesting is visible rather than
  inferred from precedence.
- **18.21 is settled in the direction you chose**: a pointer iterator returns `^T`. What
  remains open is only how the *trait* spells one signature for both item kinds.

### M6 — generics, traits, monomorphization

**`Buffer<T>` runs** — a generic struct with an `own ^T` field, a `drop`, a folded
compile-time branch on `T::is_trivially_copyable`, and `construct`/`destroy` over
`alloc`/`free` — instantiated for `i32` and for a type that owns a resource, in
`tests/run/generics`. Four questions were settled before any code was written, and are in
spec §18.21–18.23 and r8: `Iterate` is `valid`/`get`/`advance`, slices carry their
writability as `[]T` / `[]mut T`, the traits live in a prelude written in Dazzle, and a
generic parameter is a real type rather than a hole.

**The checker half.**

- **`Ty::Param`** (§8.1). A generic parameter is a type whose identity is a unique id,
  compatible with nothing it has not been proved compatible with. `Ty::Named` grew type
  arguments in the same change, since `Buffer<f32>` and `Buffer<i32>` are two types;
  `ty::substitute` / `substitute_with` are the one place parameters and `Self` are replaced.
- **Definition-site checking** (§8.1, §8.6). A generic body is checked once, against its
  bounds, and `a.frobnicate()` in `fn f<T: Ord>` is reported at that line, naming the bound
  that does not provide it. Method lookup walks the bound's supertraits. Checking against
  *nothing* has a consequence worth knowing: an unbounded `T` is move-only, so `largest`
  returns `move(b)` — correct by §6.4, and now written into §8.2 (r11).
- **Traits** (§7.6). Conformance is verified at the declaration: every non-default method,
  with its declared signature, and every associated type chosen, supertraits included. A
  trait reference carries its arguments (`TraitRef`), because `Mul<f32>` and `Mul<Vec2>` are
  different obligations. **An associated type is a `Ty::Param` allocated per trait** — inside
  the trait it is a type nobody has chosen, which is exactly what a generic parameter is, so
  every rule already built applies to it unchanged.
- **Call-site inference** (§8.1): type arguments solved from the argument types or taken as
  written (`largest<Score>(a, b)`), an undetermined parameter reported, bounds checked.
- **Operators dispatch through the prelude's traits** (§11.2), which closed invariant 7's
  standing comparison hole. `==`/`!=` go to `Eq::eq`; `<`, `>`, `<=`, `>=` all go to
  `Ord::cmp`, so four operators cannot disagree. For arithmetic the implementation is
  *found* rather than constructed from the operand — `v * 3` has to pick `Mul<f32>` while the
  literal is still untyped, then re-check the `3` against it so C++ sees `3.0f` rather than
  an `int` it would quietly convert. Primitives satisfy the traits for the purpose of bounds
  and keep their machine operators; codegen lowers a trait call on a primitive back to one.
- **Four passes, in dependency order**: type parameters and associated types, then bounds
  and trait lists, then signatures, then bodies. `struct Vec3 : Mul<f32>` must check its
  argument count against `Mul`, which may be declared after it, and order never matters
  within a module (§3.2).
- **The prelude** (§3.2, §18.23) is `crates/dz_project/src/prelude.dz`: ordinary Dazzle,
  compiled into the binary and injected as module `dz`, on which every module depends
  implicitly. It lexes, parses, resolves, checks and is emitted like any module, and a
  mistake in it renders against `<prelude>/dz.dz` with a caret. Two things fell out:
  **shim references became `::dz::`**, since a module named `dz` otherwise shadows the
  shim's namespace, and **a signed enum may be negative** (`Less = -1`).
- **Array literals** (§5.3, §18.24) and **`.Variant` on either side of a comparison** (§7.4)
  landed alongside, at the user's request.
- **§12.9 format strings are checked** (DZ0526, DZ0527): a literal first argument, the
  placeholder count, whether each spec suits its argument, and whether the argument can be
  written at all. That removed invariant 7's last known violation — `println("{}", s)` on a
  struct used to reach C++ and fail there. The shim grew `{:x}`, `{:X}`, `{:.N}` and brace
  escapes to match.
- **Constant folding** (§8.3). Array lengths and un-annotated `const`s fold from literals,
  operators and other `const`s, with a cycle guard. `[n]i32` over a local is now an error
  rather than silently a slice.

**The emission half.**

- **`dz_mono` decides what exists; `dz_cpp` only spells it.** Per module, a worklist starts
  from every non-generic body and struct field, collects each `(generic, arguments)` pair it
  reaches, and builds the concrete body by substituting the *already checked* generic body
  (`dz_hir::substitute_body`). An instance is never re-checked, so it cannot fail
  differently from its definition — which is §8.6's whole argument. It runs inside
  `analyse`, so `dz check` reports what monomorphization reports.
- **Runaway instantiation is `DZ0536`, once per module**, at depth 32 — a `f<Box<T>>()`
  inside `f<T>` names the call instead of exhausting C++'s template depth (§8.4).
- **Readable mangling** (§8.4, §16.2): `Buffer<f32>` is `Buffer__f32`, a named argument is
  qualified by its module, nested arguments are bracketed `_L_…_R`. An instance is emitted in
  its generic's home namespace behind its own include guard, with method bodies `inline` in
  the header, since several translation units may need the same one.
- **One layout sort over `Ty`s** replaced the per-module declaration sort: a struct holding a
  `Buffer<Tracked>` by value needs the instance defined first, and the instance needs
  `Tracked`, so named types and instances have to be ordered together.
- **§8.3 folding.** A condition built from type queries, `bool` constants, `!`, `&&` and `||`
  is evaluated per instance and the dead branch is not emitted. `sizeof(T)`, `alignof(T)`
  and every `T::` query lower to one `TypeQuery` node, answered from the instance's
  concrete type — `T::name` included.
- **Default methods are copied into each implementer**: checked once with `Self` as a
  parameter, then substituted and emitted as the struct's own method, so `c.heavy()` is an
  ordinary member call. **Trait default bodies had never been checked at all** — the body
  pass skipped traits, and nothing noticed until one had to be emitted.
- **`for` over a user `Iterate`** is `for (auto it = x; it.valid(); it.advance())` with the
  `get()` first in the body — §10.4's desugaring, with `continue` landing on `advance` for
  free. A range held in a variable is a `::dz::Range<T>` and iterates the same way.
- **`DZ0804`: codegen refuses an `Error` node nobody reported.** It exists because of what it
  found. `.len` had never been a node the checker built, so it lowered to `Ty::Error`, which
  C++ received as `0`: **every `xs.len` in a generated program was zero**, silently, since
  `Ty::Error` is compatible with everything. It is now `ExprKind::Len`. M5 wrote that
  `Ty::Error` without a diagnostic is a hole; this is the rule that makes the next one a
  compiler error instead of a miscompile.
- **Trait objects are refused, not half-emitted**: `^Trait` reports `DZ0801` naming M12.
- **Every `DZ0801` that said "owed by M6" was re-homed**, each to the milestone whose work it
  actually is — see §3.

**Found, and handed back rather than decided** (spec §18.2, r11) — all three settled in r12,
below:

- **§8.3's own example did not check** (18.26). Its `else` branch copies an element, and an
  unbounded `T` cannot be copied (§8.2). Folding happens after checking, so a branch that is
  dead for `f32` must still check for every `T`.
- **One right-hand side per operator** (18.27). `Mat4 : Mul<Vec4> + Mul<f32>` needs two
  `mul`s, which §18.5's deferral of overloading forbade.
- **`break` with a value** (18.28) was parsed but absent from §20's grammar and undefined.

**Noticed, not fixed:**

- **Slice element types are not invariant.** `assignable` recurses with `assignable` rather
  than equality, so `[]^T` accepts a `[]&mut T`. Pre-existing, and orthogonal to §18.22.
- **Returning a slice of a local array is not caught.** The temporary rule (§5.3) is keyed
  on placehood, so `return [1, 2, 3]` from a `-> []i32` is rejected but `let a: [4]i32;
  return a` is not. Seeing that needs escape analysis, and §1.1 leaves a stale view to the
  programmer as it does a stale `^T`.
- **§18.16 bit in a small way**: §5.2 said `&T` → `^T` is implicit, and the checker allowed
  only `&mut T` → `^T`, which M4 did deliberately (§2, M4). r12 made the spec agree with the
  checker.

### r12 — `unsafe`, `Copy`, one trait at several arguments

A language revision rather than a milestone: the answers to r11's open questions, built and
tested before M7 starts. Spec §21 r12 is the contract; what the compiler does:

- **`unsafe` is checked where the operation is built, not by a pass.** The body checker
  carries an `unsafe` depth — one inside `unsafe { }`, and from the start of an `unsafe fn` —
  and each raw-memory operation asks for it: deref, `.`/`[i]`/a method call through a `^T`,
  a pointer cast, the four raw builtins, and a call to an `unsafe fn` (`DZ0528`). Past the
  checker an `unsafe` block is an ordinary HIR block; nothing about it reaches C++.
- **One missing `unsafe` is one diagnostic.** `construct(m_data[i], v)` is two unsafe
  operations and one mistake, so a report is suppressed when its span nests with one
  already made — and the list rolls back with the checker's quiet re-checks, or a probe
  that runs before the real check would swallow its diagnostic.
- **`Copy` is structural.** `satisfies` answers it from `TypeTable::copyable` rather than
  from an implements-list, and a `T: Copy` is copyable everywhere copyability is asked.
  A generic struct's `copy` body is checked — and flow-analysed — with the struct's
  parameters assumed copyable (`copyable_assuming`); an instance whose arguments are not
  gets deleted copy operations in C++, and `dz_mono` never instantiates its `copy` body.
  `tests/run/generics`' `Buffer<T>` now has a `copy`, so the M6 criterion holds as §8.3
  writes it.
- **Several methods of one name are keyed, not overwritten.** `StructSig.methods` used to
  be a plain name map, so a second `fn mul` **silently replaced the first** — a duplicate
  method was never reported at all. Now the second is stored as `mul#1`, conformance
  matches each trait instantiation to the method whose signature fits, and every leftover
  repeat is `DZ0537`. A call picks by its first argument's type (`DZ0538` when a literal
  fits several and defaults to none).
- *(Superseded in r13, where each implementation is its own `impl` block and its methods
  free functions; the reasoning about C++ overloading still holds.)* **Overloads get their
  own C++ names**, `mul__f32` beside `mul__generics_Vec2`, and codegen
  resolves the name per call from the argument's concrete type. C++ overloading was the
  obvious alternative and is wrong: `3ll` is not `int64_t` everywhere, and `i8 + i8`
  promotes to `int`, so C++ could pick a different method than the checker did. Resolving
  in codegen also covers a call written against a bound, which only has a concrete
  receiver once it is an instance.
- **`or` on a `^T` gives a `&mut T`**, and `.ptr` (a new `DataPtr` HIR node) gives the
  unchecked way into a writable array or slice. Bounds checks are no longer compiled out
  of release builds: `DZ_NO_BOUNDS_CHECK` is gone from the shim and the build flags.
- **`break` lost its value** in the AST, the HIR and every consumer; the parser reports one
  as `DZ0212` and still consumes it, so the line does not cascade.
- **Two existing bugs, found on the way.** A struct's trait list was parsed with the
  expression-style path parser, which accepts `>` only before `::`, `(` or `{` — so
  `: Mul<f32> + Mul<Vec2>` was a syntax error and only a generic trait written *last*
  worked. And the formatter wrote a block-valued `let` or assignment with its body
  unindented (`let x = if c {` then `1` at column 4); the committed snapshot had pinned the
  wrong output.

### r13 — implementations: `impl Trait for Type`, and `Drop` / `Copy` as traits

The second half of the answers to r11's questions, and the reason for it: adding behaviour
to a type without touching it — an editor's `Inspect` for an engine's components. Spec §21
r13 and §7.6 are the contract; what the compiler does:

- **An implementation is a table entry, not a property of a struct.** `TypeTable::impls`
  holds one `ImplSig` per `impl` — the trait with its arguments, the type, its own generic
  parameters, its methods and its associated-type choices — and every question that used
  to read a struct's trait list reads it instead: bounds (`implements`), method lookup
  (`impls_for`), operators, `for` over an `Iterate`, and associated types, which are now
  looked up per implementation (`assoc_bindings` takes the `TraitRef`). That last is all
  18.29 needed: `Mat4 × Vec4` and `Mat4 × f32` have different `Output`s because they are
  different entries.
- **A generic implementation is a pattern.** `impl<T: Copy> Copy for Buffer<T>` applies to a
  type by matching `Buffer<T>` against it and then checking the bounds on what `T` became
  (`impl_applies`). The same matching drives codegen and monomorphization, so a call
  written against a bound resolves to the right implementation only once it is an instance.
- **A misplaced implementation is never registered.** The orphan rule, an `impl` for every
  type, and the placement rules for `Drop`/`Copy` are checked in pass 1, and an
  implementation that breaks one is reported and dropped. Registering it and reporting
  later was the first version, and `impl<T> Same for T` then made every type satisfy
  `Same` — silently removing a real error elsewhere in the fixture.
- **`Drop` and `Copy` implementations fill the slots special members used to.** After pass
  2 they are attached to their struct (`attach_special_impls`) as `StructSig::drop` /
  `copy`, and their bodies are owned as `BodyOwner::Drop` / `Copy`. Flow analysis, the
  C++ special members and monomorphization therefore did not change shape; only where the
  bodies come from did. r12's implicit "`T` counts as `Copy` inside `copy`" is gone —
  `impl<T: Copy>` says it, and a `T: Copy` is copyable everywhere.
- **Other implementations' methods are free functions**, `Mul_f32__generics_Vec2__mul(const
  Vec2& self, float rhs)`, because C++ cannot add members to a class from another header
  or to `float`. Calls to a trait's method resolve the implementation at emission, per
  instance; `for` over an `Iterate` calls `Iterate__T__valid(it)` and friends. The
  receiver is `[[maybe_unused]]`, since a method that ignores `self` otherwise warns — the
  two-compiler check caught it, invariant 7 working as intended.
- **Trait defaults are per implementation.** A default the `impl` does not override is
  instantiated for that implementation (`impl_defaults`), replacing r12's per-struct copies.
- **A struct's `: Base` is parsed and reserved.** The checker reports it as not yet
  implemented; inheritance is designed (18.32) and is the next piece of work.
- **`Error.location` is a `&SourceLocation`** in the type system and still a pointer in the
  shim; reading the field dereferences it, so `let at = e.location` binds a C++ reference.
- Explicitly calling `drop` or `copy` is an error: they reach a destructor and a copy
  constructor, not functions, and there is no correct C++ to emit for the call.

### M9 part 1 — language server and VS Code extension (pulled forward)

Pulled ahead of struct inheritance and M7 at the user's request, to have an editor to feel
the language in. Split in two: this part is everything the existing pipeline can answer;
part 2 makes analysis tolerate errors (§4). Compiler work only — nothing in §21.

- **`dz_driver`** holds `analyse`, moved out of the CLI so the server runs the very same
  pipeline. `analyse_with` takes an overlay: a buffer the editor has open stands in for the
  file on disk, and spans are offsets into that buffer.
- **`dz_lsp`, `dz lsp`.** `lsp-server`, not `tower-lsp` — `Ty` holds `Rc`s, so an analysis
  cannot live in async `Send + Sync` state; spec §17.4 now says so. One synchronous loop. A
  project is re-analysed whole after 150 ms of quiet. A compiler panic is caught, logged to
  the client, and does not take the server down.
- **Diagnostics**: every compiler diagnostic, code included, notes appended to the message,
  and every labelled span as related information (so a use-after-move shows where the move
  was). A file its project does not list, or one outside any project, gets a lex+parse check.
- **Hover and go to definition.** The innermost HIR expression containing the cursor,
  accepted only when the name it uses is the one under the cursor — otherwise `Tracked` in
  `Buffer<Tracked>(3)` would answer as the constructor call around it. Before that, a
  declaration's own name and a local's binding; after it, the resolver's `lookup_in_file`
  for names with no expression (types in signatures, imports, `::` paths), so the two can
  never disagree. Hover quotes the declaration's source header plus its `///` docs rather
  than rebuilding a signature from types.
- **Methods are found as declarations**: the type's own body, then its `impl`s, then the
  defaults of the traits those implement (§7.6). The last two are matched by type name.
- **Completion** reads the context (`.`, `::`, a bare name) from the buffer and names from
  the last analysis that resolved, since the line being typed rarely parses: locals of the
  enclosing body, bare fields of `self`, module, import and prelude items, keywords,
  builtins; after `.`, a local's fields and methods, private ones only from inside the type;
  after `::`, a module's items, a type's members and the §5.5 type queries.
- **Outline and formatting** need only the buffer, so they work in any `.dz` file.
  Formatting refuses a file with syntax errors (invariant 10).
- **The extension** (`editors/vscode`): a TextMate grammar, language configuration, and a
  client. It runs a *copy* of `dz`, because Windows cannot replace a running executable and
  `cargo build` would otherwise fail while VS Code is open, and restarts the server when the
  binary changes.
- **Tests**: `dz_lsp/tests/ide.rs` pins each query against `tests/run/generics`;
  `convert.rs` pins UTF-16 columns and all three line endings. The protocol itself was
  smoke-tested by driving `dz lsp` over stdio with VS Code's `c%3A` URI spelling.

### M9 part 2 — analysis that keeps answering around errors

- **`Mode::Editor`** in `dz_driver`. Once a phase fails, resolution and checking still run
  over whatever parsed, but the diagnostics are exactly `Mode::Build`'s: only the first
  failing phase's. The user's call — no type errors while there is a syntax error. The CLI
  is untouched. Resolve and check needed no changes: neither has a panic path, and error
  nodes already type as `Ty::Error`. `dz_driver/tests/tolerant.rs` holds that: every
  `tests/run` program is damaged about 200 ways (a token deleted, the file cut off, a stray
  `.`, `(` or `{`), and each must analyse without panicking and without a later phase's
  errors showing. The server still falls back to `Mode::Build` if editor mode ever panics.
- **Completion types the receiver.** The server re-analyses the buffer when completion is
  asked for, and the widest expression ending at the `.` is the receiver, so `a.b().` and
  `Countdown(3).` work. The last analysis that parsed cleanly remains a by-name fallback.
  (A `.` at the end of a line continues onto the next, so a `p.` typed above a line usually
  parses as a method call with a type error rather than a syntax error.)
- **One resolver behind every query.** `target_at` answers hover, definition, references,
  rename and signature help alike. References compare an identity: a constructor stands for
  its struct, and an implementing method for the trait method it implements, so a rename
  keeps them together. Declarations the checker records by their whole extent (fields,
  variants) are narrowed to their name.
- **Rename** refuses keywords, non-names, `self` and `Self`, the prelude's declarations, and
  — in the server — any project with a syntax error, which could hide a use.
- **Inlay hints** for bindings without a written type (`let`, `for`, `case`); **signature
  help** from the declaration's header, receiver left out; **workspace symbols** by fuzzy
  subsequence; **document highlight** from references.
- **Tests**: 30 in `dz_lsp/tests/ide.rs` — references are counted against every identifier
  token of the name in the fixture — and 3 in `dz_driver/tests/tolerant.rs`. The stdio smoke
  test drives each request through the real binary.
- **Then, from using it**: semantic tokens (`ide::semantic_tokens`, the same resolution as
  hover, so a name's colour and its hover cannot disagree; a member of a built-in type such
  as `e.message` falls back to property/method after a `.`); a grammar using the scopes
  common themes colour (`move`, `as` and `or` had been drawn as plain text, and primitives
  as keywords); a ▷ Run button and Ctrl+F5 (`dz run` on the file's project as a VS Code
  task), and **Check Project**. `npm run deploy` packages and installs with `--force`: at
  the same version, a plain `code --install-extension` silently keeps the old build.

### r14 — `move` always moves; the formatter fixed

- **`move` on a copyable value** had been a copy in both `dz_flow` (never marked moved-from)
  and `dz_cpp` (no reset), each citing §6.3 for a rule §6.3 does not contain; §6.4 always
  said `move` ends its source. The user's call, recorded as 18.33 / r14: it moves, for every
  type. Flow now marks the source moved-from and refuses a move out of a borrow whatever the
  type (`tests/flow/move_copyable.dz`, `move_copyable_out_of_borrow.dz`). A copyable type
  has no move constructor to reset its source, so codegen moves a copyable *place* through
  a new shim function, `::dz::take`, which returns the value and assigns `T{}` back.
- **The formatter changed meaning**, breaking invariant 10 three ways, all found by
  formatting `tests/run/failure` in the editor: it printed every `!->` as `->`; it rendered
  blocks inside expressions (`if`/`switch` bodies, `or { … }`, `unsafe`) with an empty
  comment table, so their comments were emitted after the block; and it dropped the author's
  blank line before a scope's first item or statement. The comment cursor is now shared with
  the sub-printers, an empty block holding a comment keeps it, block comments re-indent
  relative to their first line (so a nested one no longer drifts right on each pass), and
  "no blank line at the top of a scope" is enforced in one place, `flush_blank`. The tests
  missed all of it because the corpus had no fallible signature and no comment inside an
  expression block; it now includes every `tests/run` program, and a fourth property —
  every comment survives, in order — is checked across it.
- Three help messages had a run of 30-odd spaces in the middle, a line continuation that
  lost its backslash (`dz_check`).

### Unused bindings

- **`DZ0406`, a warning**: a local or parameter nothing reads, and a local that is only ever
  assigned to ("assigned but never read"). `self`, names starting with `_`, and bindings
  the checker made itself (an implicit `it`, whose span is not its name) are exempt, and
  the prelude is never warned about. The user's call: a warning, with `_name` for "unused
  on purpose", the Rust and Odin convention. A guard kept only for its `drop` needs it too,
  as `tests/run/ownership` now shows.
- **One answer, two consumers.** `dz_hir::local_uses` decides; `dz_flow` warns from it, and
  `dz_cpp` marks exactly those bindings `[[maybe_unused]]` at every declaration site —
  parameters, owning parameters' locals, `let`, `let (a, b)`, all three `for` forms, `case`
  payloads, `or { e -> }` — so the C++ compiler never repeats the warning. Before this an
  unused `let` put a `-Wunused-variable` into the generated code (invariant 7).
- In the editor the diagnostic carries the protocol's *unnecessary* tag, so the name is
  drawn faded.
- `tests/run/unused` builds each kind of unused binding warning-clean under both compilers.
  The flow harness counts `DZ0406` only in the fixture that asks for it
  (`tests/flow/unused_variable.dz`), since rule fixtures and `typed/` are full of throwaway
  bindings on purpose.

### Fixes from `examples/books`

A user program — a generic `List<T>` with a `ListIterator<T>` pointing back at it — hit
three compiler bugs, none a language change.

- **`self` in a generic struct's method was the bare struct.** `Body::function` typed it
  `Ty::plain(sym)`, so inside `List<T>`, `&mut self` was a `&mut List` and matched no
  `^List<T>`. It now uses `self_type_of`, the declaration applied to its own parameters —
  what `Self` already meant. The `hir_typed` snapshot shows the difference (`self: Pair<T>`).
- **C++ declaration order.** `sort_types` orders definitions by what each type stores by
  value, which is not enough when a signature merely *mentions* a type defined later:
  `List__Movie::iter()` returns a `ListIterator__Movie`, which holds a `List__Movie*`.
  Every module header now forward-declares all its types — its own structs, unions and
  enums, and its instances in their home namespaces — before defining any. Unguarded,
  since repeating a declaration is harmless. `tests/run/iterator` pins it.
- **`fn f(&mut self)` produced C++ that did not compile.** The receiver was said twice, and
  each half of the compiler believed a different half: the checker went by `&mut self` and
  allowed mutation, codegen went by the missing `mut` and emitted a `const` member. The
  user's call, recorded as 18.34 / r15: **`mut fn` is removed** and the receiver alone
  decides. `FnDecl.is_mut` is gone from the AST, so nothing can consult the old keyword;
  `FnSig.is_mut` is now derived from `&mut self`. The parser reports `mut fn` as
  **`DZ0214`** and parses on as if the `mut` were absent. Every fixture, the prelude and
  the spec's examples were rewritten.
- **Inlay hints name the fields of a positional literal**: `Movie{ id: 1, rating: 7.1, … }`
  is shown for `Movie{ 1, 7.1, … }`, and the same for a union variant's payload. A value
  after `name:` gets none, and neither does one spelled as its field's name (`Vec2{ x, y }`).
  They are sent as the protocol's *parameter* hints, so they take the editor's styling for
  argument names.

### r16 — a reference reads as a value

- The user's call, recorded as 18.35 / r16, settling the half of 18.25 left open: `return m`
  from inside `for m in list.iter()` should return the element. `Body::expr` — the one
  place every expected type passes through — now wraps a `&T` that meets an expected `T`
  in a `Deref` node typed `T`. Nothing else in the checker changed.
- Making the read a node rather than loosening `assignable` is what keeps it honest: flow
  already treats a `Deref` as a borrowed place, so a move-only or unbounded generic `T`
  gets the existing `DZ0402` (`tests/flow/read_move_only_through_reference.dz`), and
  codegen emits a `Deref` of a reference as the reference itself, since C++ copies it at
  the use site.
- `typed/` has the accepted forms (return, annotated `let`, argument, `T: Copy` generic),
  and `tests/run/iterator` runs the user's `find_movie`.

### r17 — `if let`, and comparing an optional

- **`m == none` produced C++ that did not compile** (`Opt == Opt`; `Opt` has no `==`), and
  so did every other comparison with an optional operand, all accepted by the checker.
  `x == none` / `x != none`, from either side, now lower to a test of the flag
  (`(!(x).has)`); every other comparison on a `T?` is `DZ0514` at the checker. Written into
  §9.4 with r17, since the spec had never said.
- **`if let`** — the user's call, 18.36 / r17. The AST's `IfExpr` gains an optional
  `binding`, resolved in a scope around the `then` block only. The HIR gets its own node,
  `IfLet { binding, operand, then, else_branch }`, rather than a desugaring through `or
  unreachable`, which would have emitted the null check twice (§16.2); it lowers to the
  test `or` makes with the local declared inside the `if`. Flow reads the operand as `or`
  does and treats the two blocks as branches. The binding's type is `or`'s success type; a
  fallible operand is the new **`DZ0529`** (never shipped under its short-lived r15-era
  meaning, so not a reuse), a plain value is `NOT_FALLIBLE`.
- `tests/run/iterator` runs all three forms on both compilers — `T?`, `^T` with a write
  through the binding, and `if let` as a value — and the formatter corpus round-trips them.

### M7 — C and C++ interop, split in two

M7 is the milestone that turns Dazzle from a toy that prints into something that can draw,
and the first of 0.1's three release conditions (the others are M9's LSP and M12's stdlib).
It was pulled ahead of struct inheritance at the user's call, and split in two.

**Decided with the user (2026-09-11), before any code:**

- **The importer only teaches the checker what exists.** A call into a header is emitted
  exactly as written — no wrappers, no marshalling (§15). A nicer API (`rl::Color::White`)
  is a Dazzle wrapper module written on top, once the raw names are visible.
- **Libraries are vendored as source and built by `dz`**, never by CMake, XMake or Make —
  avoiding that tooling is a reason Dazzle exists. A build recipe (sources, include dirs,
  defines, system libraries, with `[cpp.target.<os>]` sections so one recipe serves every
  platform) lives in a `[cpp.libs.<name>]` table of the manifest that uses it. A Dazzle
  wrapper module can carry the recipe instead, and its dependents inherit its include
  dirs and link inputs. `.c` files go through the C compiler, and library sources are
  built without `-Wall -Wextra`: invariant 7 is about *generated* code. Objects are cached
  in `build/obj`, keyed on the content of the source and every header it included plus
  the flags. Prebuilt binaries stay possible (`lib_dirs` + `link_libs`) for the libraries
  that generate files at configure time. All paths are relative to the manifest (today
  `include_dirs` is passed through as written and resolves against the working
  directory — a bug to fix on the way). Cross-platform is designed in, Windows is built.
- **Pointers: `^T` is read-only, `^mut T` writable, `own ^mut T` the owning writable
  pointer** — the pattern `&T`/`&mut T` and `[]T`/`[]mut T` already follow. `const T*`
  maps to `^T`, `T*` to `^mut T`. A `^const T` was rejected as the Rust shape, which reads
  against Dazzle's "read-only unless `mut`". Done first, as r18, since the mapping needs it.
- **Unscoped C enums:** each enumerator is also an untyped integer constant at the
  header's scope, so `rl::IsKeyDown(rl::KEY_A)` works against raylib's `int` parameters.
  The enum type is imported too; an `enum class` is a real, scoped Dazzle enum.
- **Macros:** an object-like macro imports as a constant when clang can type its
  expansion as an importable value, and is emitted as the macro's own name. Function-like
  macros and variadic functions are not imported.
- **libclang through `clang-sys`, loaded at run time**, so `dz` starts without it and only
  `import cpp` reports its absence. In-process because the LSP needs it: loaded once per
  process, each header's import cached on the content of every file it pulled in plus the
  flags, and mirrored into `build/` so `dz check` does not re-parse either. Hover and
  go-to-definition on an imported name land in the header.

### M7a — a small C library, from source

Ends when **a C library in `tests/` — one header and one `.c` file, never prebuilt — is
compiled by `dz` from source and called from Dazzle**, on clang++ and g++.

1. **r18 — `^T` / `^mut T`.** Language, compiler, corpus, spec (§5.2, §15.2, 18.16).
2. **Manifest.** `[cpp.libs.<name>]` with `sources`, `include_dirs`, `defines`,
   `link_libs`, `lib_dirs`, and `[cpp.target.<os>]` overrides; paths relative to the
   manifest; recipes inherited through dependencies.
3. **`dz_cppimport`** (spec §17.1): libclang to a declaration table — C functions, plain
   structs, unscoped and scoped enums, integer and float `#define`s, pointers, `cstr`.
4. **Resolver and checker.** `alias::name` resolves into the table; imported structs,
   enums and functions become symbols like any module's, carrying their C spelling.
5. **Codegen.** `#include` the header, emit names verbatim, zero-initialise imported
   structs (§6.2), struct literals of imported types.
6. **Build.** Compile each library source to a cached object, link.
7. **LSP.** The per-process cache; hover and definition into the header.
8. **Tests.** A `tests/run/` program with a vendored library; `typed/` and `typeerrors/`
   cases for what imports and what is refused.

### M7b — raylib from source, and C++

Ends when **a raylib window opens from Dazzle**, raylib compiled from its own vendored
source (spec §19). Adds what raylib needs beyond M7a — typedefs, compound-literal macros
such as `RAYWHITE`, many sources, per-platform system libraries — and C++ (the user's call
to keep it in M7b): classes and member functions, constructors and destructors (§15.2),
namespaces, overload sets (§15.3), explicit template instantiation (§15.4). A Dazzle struct
inheriting a C++ class (Jolt's listeners) waits for struct inheritance, which follows.
`impl` blocks are what an imported type gets traits through — raylib's `Color` can only
ever be `Inspect` that way — so M7b is also their first real test.

**Met at r28.** `tests/run/raylib` opens a raylib window with raylib 6.0 compiled from its
own vendored source. The C++ language work is covered by `tests/run/cppinterop` on clang++
and g++: namespaces, classes, methods, constructors, C++-owned destructors, overloaded
methods, references, and named template instantiations. Handles, unions, arrays,
bit-fields, callbacks and compound-literal macros landed in r21–r27.

What is left is narrower than the milestone, and is in the deferral register rather than
here: overloaded *free* functions (methods do resolve), the boundary cast for an overloaded
method, and inheritance between imported C++ classes — which wants struct inheritance
first, so Jolt's listeners wait for that either way.

### r18 — `^T` and `^mut T` (M7a step 1)

- The user's call, 18.37 / r18, made so that C's `const T*` has somewhere to go. `Ty::Ptr`
  and `Ty::OwnPtr` (and the AST's `TypeKind` pair) become `{ mutable, inner }`, the shape
  `Ref` and `Slice` already had. Every rule that read "through a pointer" as writable now
  reads the flag: `place_mutability`, the mutable-receiver check, `or` and `if let` (a `^T`
  gives a `&T`), `.ptr` (as writable as its view, where it used to be refused on a
  read-only one), and `construct`/`destroy`/`memcpy`, which refuse a read-only
  destination. A write through a `^T` is `DZ0507` with a help naming `^mut`.
- `assignable` shares one pointee rule between pointers and the references that widen to
  them: writing needs a writable source, and behind a `^mut` the pointee must match both
  ways — otherwise `^mut ^mut T` → `^mut ^T` would be accepted and emit `T**` →
  `const T**`, which C++ rightly refuses.
- Codegen: `^T` is `const T*`, with the `const` on the right after a pointer pointee
  (`int32_t* const*`); mangling gains `pm_`/`opm_`. The shim's pointer `cast` became a
  C-style cast, since `^T as ^mut T` casts away `const`, which `reinterpret_cast` cannot;
  `dealloc` takes a `const void*`; `data` and `index` gained `const Array` overloads; and a
  two-binding `for` binds `auto&`, which is `const` over a read-only view. All three were
  found by `tests/run/failure`, whose `find` now takes and returns read-only pointers.
- The parser unpicks `^^T` (lexed as xor) into two pointers, the way it already did `&&T`.
- Tests: `a_pointer_is_as_writable_as_it_says` pins the conversions, including the
  two-level one; `typed/` and `typeerrors/` gained an r18 section each; the corpus now
  says `^mut` wherever it writes. 141 tests, clang++ and g++.

### r19 — recipes, and building vendored sources (M7a steps 2 and 6)

- **Manifest.** `CppConfig` holds resolved `PathBuf`s and a list of `CppLib` recipes read
  from `[cpp.libs.<name>]`; `[cpp.target.<os>]` and `[cpp.libs.<name>.target.<os>]` are
  folded in for the host (`host_target()`; there is no cross-compilation yet) and checked
  for every other target. A library's `root` is read in a first pass, since a target
  section may come before its base table. Unknown keys, unknown tables, unknown targets,
  missing directories and sources, and a non-source written out as a source are all
  `DZ0004`/`DZ0005` at the offending span — a typo such as `source =` must not silently
  build nothing. Two recipes for one library are refused in `load`. `[cpp.instantiate]`
  is refused as not yet implemented (M7b).
- **Paths are relative to the manifest.** The bug noted in §4 — `include_dirs` passed to
  the compiler as written — is gone with the move to resolved paths.
- **Flags travel down the graph.** `Workspace::cpp_flags(m)` is a module's include dirs
  and defines followed by its dependencies'; the generated C++ is compiled with the
  root's, and the importer will parse headers with the importing module's.
  `link_inputs` gathers `-L`/`-l` from everything the root reaches.
- **`dz/src/native.rs`** compiles each vendored source with the same driver as the
  generated code (`-x c` for `.c`), `-w`, the build's optimisation flags, and the
  library's own defines and include dirs, as many at once as there are cores. The object
  name carries a hash of the command, so a changed flag compiles a new object; a stamp
  beside it lists the content hash of the source and every header in its `-MD` depfile,
  and the object is reused only while all of them match. A failure is the new
  **`DZ0701`**, pointing at the recipe and blaming the library, never the Dazzle
  compiler.
- Tests: `tests/run/cinterop` vendors `mini` — a header in `include/` and a `.c` that
  `#error`s without the recipe's define — found by a glob; two run tests pin that an
  unchanged library is not recompiled and a changed header is, and that a broken library
  is `DZ0701` and not `DZ0803`; `badmanifest/` gained one case per new manifest check; a
  unit test parses a depfile with continuations, CRLF and an escaped space.

### r20 — `import cpp` (M7a steps 3–5 and 8; M7a's criterion met)

- **`dz_cppimport`** loads libclang through `clang-sys`'s runtime feature: once per
  process, shared to every thread (clang-sys keeps it per thread), and found beside `clang`
  on the path when `LIBCLANG_PATH` is unset. A header is reached through an `#include` in an
  unsaved file placed where the generated C++ is compiled, parsed as C++ with the importing
  module's `cpp_flags`, so the search is the C++ compiler's. The walk keeps what is outside
  system headers, looks inside `extern "C"`, names an anonymous struct or enum after its
  typedef, reads numeric macros from their tokens, and turns each C type into a `CType`;
  anything that cannot be named is recorded in `skipped`, and a function using something
  that was skipped goes too. Results are cached per process, keyed on header, directory and
  flags, and held while every included file keeps its size and timestamp — the tolerant
  test's 1,200 analyses of `cinterop` parse `mini.h` once.
- **The imported header is a module.** The driver adds it to the workspace (`Module::foreign`,
  and `cpp_imports` on the importer) after parsing and before resolution. The resolver
  builds its symbols straight from the declarations — pointing into the header's own text,
  added to the source map — and maps a typedef to its target's symbol, which is why the
  header's declarations were not turned into Dazzle source: an alias needs no syntax this
  way, and a span can be the header's. The checker builds signatures from them before any
  Dazzle signature is read. Codegen skips the module, `#include`s the header as written,
  spells its names `::name` (a macro bare), and casts an imported constant to its Dazzle
  type, since C's is an enum or an `int`.
- New codes: **`DZ0702`** a header clang could not read (with clang's messages as notes),
  **`DZ0703`** no libclang. The parser now keeps the header's text on `ImportKind::Cpp`.
- Tests: `dz_cppimport/tests/import.rs` against `mini.h` (every kind of declaration, a span
  landing on the name, a missing header); `tests/run/cinterop` calls into `mini` — struct
  literals and zero-initialisation, an enum parameter by variant, an unscoped enumerator as
  an `i32`, macros, `^mut`/`^` pointers from an array's `.ptr`, a `cstr` return — on both
  compilers, and its C++ is warning-clean under `-Wall -Wextra`; run tests for DZ0702 and for
  a type mismatch at a C call. 150 tests.

### The feature matrix, and what it found

Bugs in this compiler have mostly lived where two features meet, so the corpus stopped being
hand-picked. `dz_driver/tests/feature_matrix/mod.rs` holds **constructs** (a statement list
leaving a known value in `r`), **mistakes** (a statement list that must produce exactly one
diagnostic) and **contexts** (a plain function, a method, a generic function, a loop body, a
`while` body, a `switch` arm, an `or` block, `if let`, `unsafe`, nested blocks, a constructor,
a trait implementation's method, a generic struct's method). Every construct and every mistake
goes in every context, in two layouts — the shared types beside the code, and in a module of
their own imported by name.

- `dz/tests/matrix.rs` compiles all the constructs into one program per layout, runs it, checks
  every placement's value, and requires the C++ to be warning-clean under each compiler present:
  36 constructs × 13 contexts × 2 layouts.
- `dz_driver/tests/matrix.rs` analyses each mistake in each placement and requires exactly its
  one code: 26 mistakes × 13 contexts × 2 layouts.

Adding a construct, a mistake or a context multiplies across the rest, which is the point. What
it found on its first runs, all fixed:

- **Loop labels were ignored.** `break outer` and `continue outer` parsed, and the checker threw
  the label away, so both acted on the innermost loop — a nested loop that meant to `continue`
  its outer loop ran its own remaining passes instead. The checker now resolves each label
  against a stack of enclosing loops, and a jump to anything but the innermost carries that
  loop's id (`hir::ExprKind::Break(Some(id))`). A loop that is jumped to records it
  (`hir::LoopLabel`), and codegen places `_dz_continue_N:` after the body — in a block of its own,
  so the `goto` leaves the body's locals rather than jumping past their declarations — and
  `_dz_break_N:` after the loop. Only labels something jumps to are emitted, since C++ warns
  about the rest. A `break` outside any loop, or naming a label no enclosing loop has, is the new
  **`DZ0550`**.
- **A zero-argument constructor did not compile**: codegen also emitted `S() = default;` beside
  it. §6.2 already says such a constructor is what `let s: S` runs, so the defaulted one is left
  out when a constructor needs no arguments.
- **An unconsumed fallible call was reported once per enclosing block** when it ended an
  `unsafe` block or a nested one: each block's tail was checked, and then the block itself.
  `check_unused` no longer looks at a block, `if` or `switch` as a whole.
- **A `case` label of the wrong type was two errors**, a mismatch and "not a constant"; now the
  mismatch alone.

### r37 — how `for` and `case` bindings hold what they name, and an error corpus

Decided with the user (spec 18.42–18.44) after the M8 work showed that `for e in &mut xs` did
not work at all and that a `case` binding could write into a value declared immutable.

- **`for` bindings.** `e` copies and cannot be changed, `&e` refers, `&mut e` refers and
  writes; the old `for e in &xs` / `&mut xs` forms are gone. `ast::ForBinding` carries a
  `BindMode`, and the checker classifies what a pass yields as a `ForItem`: a **place** (an
  array or slice element, or what an iterator's `&T`/`&mut T` `get` points at, with whether
  it may be written) or a **value** (a range's number, a by-value `get`, a consuming loop).
  Each binding's local then has the element's type or a `Ty::Ref` to it, and codegen needs
  nothing else — `for_binding` declares `T`, `const T&` or `T&` from that type.
- **Tuples and `indexed()`.** Several names take a tuple element apart, each part with its
  own mode; a part that is itself a reference is a place of its own, which is exactly an
  ECS query's `(Entity, &Button, &mut State)`. The pass count moved to `xs.indexed()`, a
  loop-head adapter recognised by the checker (`ForShape::Indexed`) rather than a method, so
  it works on arrays, ranges and any `Iterate` without associated-type projection, which
  M12 still owes. `hir::ForShape` says which of `Single`, `Tuple`, `Indexed`, `Consuming`.
- **`case` bindings follow the `for` rules exactly**: `c` a copy, `&c` a read-only
  reference, `&mut c` a writable one on a writable subject; a payload that cannot be copied
  needs `&` (`DZ0403`). The binding's local type says which, as for `for`. A copy is never
  turned into a reference behind the scenes — with RAII that would decide whether a `copy`
  and a `drop` run — so there is no such optimisation to defer.
- **Assigning to a `&mut T`** writes through it (18.44); `local_uses` no longer calls that
  "assigned but never read".
- **Variant literals infer their type arguments from the payload**, the way generic calls
  do: `Choice::One{ value: 4 }` is a `Choice<i32>`, and inside `fn wrap<T>` it is a
  `Choice<T>`.
- **Bugs the new tests found**, all fixed: every refused `&mut` binding also reported each
  write in the body (six cases of one mistake, two errors — invariant 6); assigning through a
  `&i32` added a type mismatch to the real error; a block yielding a reference (`let p: &P =
  unsafe { &xs^[i] }`) did not compile, and is now held as a pointer the way `or` already
  did; a C array member made `::dz::len` ambiguous once loops indexed, so sequences use a
  range-for again and the pass count is taken at the top of each pass.
- **`tests/errors`**, a new corpus in `dz_driver/tests/errors.rs`: one small program per
  mistake, each naming its exact diagnostics in `// expect:` lines, or `(none)` for a
  program that must be accepted. The match is exact, so a second diagnostic for one mistake
  fails. It runs the whole pipeline, and it is where combinations live — a `switch` inside a
  `for` inside a generic function, `or break` inside an arm, a move inside a loop. 48 files.
  `tests/run/loops` runs the valid combinations on both compilers, warning-clean.

### r43 — `Buffer<T>` renamed to `List<T>`

A rename only, chosen by the user: the prelude struct, its panic messages, spec §14 and its
usage examples, `tests/run/containers` and the `typed/` case. Nothing in the compiler named
it. Two help messages in `dz_check/src/impls.rs` that used `Buffer<T>` as their example
now say `List<T>`, so the example is a type the user actually has. The r42 notes below keep
the old name.

### r42 — `Buffer<T>` and `String`

Designed with the user (18.50): range slicing, `str` fields and casts with a checked
`str::from_utf8`, and `b[i]` through `Index`/`IndexMut`.

- **`Buffer<T>`** in the prelude, over `^mut Allocator`: doubling growth — through the
  allocator's `realloc` when `T::is_trivially_copyable`, by `move_out` into a new allocation
  otherwise — `push`, `pop -> T?`, `insert`, `remove`, `swap_remove`, `clear`, `truncate`,
  `resize`, `reserve`, `len`, `capacity`, `is_empty`, `as_slice`, `as_mut_slice`; `Drop`,
  `Copy` for a copyable `T`, `Index` and `IndexMut`. Every allocating call forwards `loc`.
- **`String`** over a `Buffer<u8>` that keeps a trailing zero: `String(s)`,
  `String::from(s)`, `push_str`, `push(char)` (UTF-8 encoded by a prelude `encode_utf8`),
  `clear`, `reserve`, `len`, `capacity`, `is_empty`, `as_str`, `as_cstr`, `Eq`.
- **Language pieces**: `hir::ExprKind::SliceRange` and `::dz::subslice` (checked); `.len`
  and `.ptr` on `str`; `str as []u8`, and `bytes as str` / `p as cstr` inside `unsafe`;
  `str::from_utf8` validating in the shim (shortest forms, no surrogates, ≤ U+10FFFF);
  `IndexMut` in the prelude, chosen by the checker for a writable place, with an assignment
  through the returned `&mut` checked against its referent; the `move_out` builtin.
- **Codegen fixes the containers found**: the shim's `Opt<T>` and `Res<T>` copied their
  value, so no optional of a move-only type compiled; a value produced by a block or an `or`
  clause was copied out of its temporary rather than moved; `subslice` took only lvalues.
- Tests: `tests/run/containers` (in the `-Wall -Wextra` set; verified leak-free under
  `--track-memory` on both compilers), and cases in `typed/` and `typeerrors/` for slicing,
  the unchecked conversion, a pointer slice with no end, and indexing without `Index`.

### r41 — the standard allocators, and what they needed

The user asked for traits and allocators to be finished (2026-09-13), and settled one
question the r39 tests raised: **arguments are evaluated left to right**, which is recorded
(§7.2, 18.49) but not yet enforced by codegen — see §3.

- **`Allocators::Global` and `Allocators::Temp`** in the prelude (§14.2), in Dazzle: the
  `Allocator` trait on `&mut self` with `loc: SourceLocation = #location`; `Heap` over the
  `alloc`/`free` builtins, over-allocating for an alignment past 8 and finding the heap's
  pointer again from the sized free; `TempArena`, 64 KiB chunks linked through a 16-byte
  header, bump allocation aligned by address, in-place growth of the last allocation,
  `reset()` keeping the first chunk, `used()`, and a `Drop` that returns every chunk at
  thread exit. Both constants are `&mut` references, so `Allocators::Temp.reset()` needs no
  `unsafe`.
- **Tracking**, all of it inside `#if TRACK_MEMORY`: `Tracker`, an open-addressing table in
  memory of its own, recording address, size, alignment and `loc`; `release` panics on a
  foreign free, a double free (naming the first free) and a size or alignment mismatch; its
  `Drop` — the static's C++ destructor at exit — prints each leak with its allocating line;
  `TempArena::free` panics on a pointer it does not own, and `reset()` poisons with `0xDD`.
  Verified by `a_tracked_build_reports_leaks_and_bad_frees`.
- **Associated constants** (§7.3). `MemberKind::Const`, `StructSig::consts`,
  `BodyOwner::AssocConst`, `hir::ExprKind::AssocConst`. The value is a constant or a
  static's address (**`DZ0553`** otherwise); codegen makes it a C++ `static` member function,
  so it is evaluated where it is named and a `thread_local` static's address is per thread.
- **Trait-object default arguments.** A trait method with no body gets a
  `BodyOwner::TraitDefaults` body holding its parameters' defaults; the handle's forwarding
  members declare them, and an implementation's function takes the trait's default for any
  parameter it does not default itself — so `al.alloc(16, 8)` compiles whether `al` is a
  `Heap` or a `^mut Allocator`, and records its caller either way. `ParamSig::caller_default`
  carries `#location` and friends.
- **`^T as usize`, and `usize as ^T` inside `unsafe`** (§10.6, r41) — the shim's `cast`
  takes the C-style cast whenever a pointer is on either side.
- **Two checker bugs.** `ptr == null` on a pointer to a struct went looking for the struct's
  `Eq`, because the comparison peeled pointers as well as references; pointers compare as
  addresses now. And a trait-object reference stored into a `^Trait` field took the address
  of the handle.
- **Two g++ failures CI found.** A `#location` default was spelled
  `SourceLocation{__builtin_FILE(), __builtin_FUNCTION(), __builtin_LINE(), 0}`, and g++
  evaluates a builtin at the caller only when it is *directly* a default argument — nested in
  an initialiser it gave the declaration's line and an empty function name, while clang gave
  the caller's. Every intrinsic default is now a shim function whose own defaults are the
  builtins (`::dz::caller_location()`, `caller_line`, `caller_file`, `caller_function`),
  the pattern libstdc++'s `source_location::current()` rests on. And an `inline thread_local`
  with a destructor — the arena's `TEMP` — linked twice on mingw g++; a thread-local static is
  now `extern` in the header and defined once in its module's file. `dz test --test run`
  passes with `DZ_CXX=g++`.
- Tests: `tests/run/allocators` (in the `-Wall -Wextra` set; the tracked build of it checked
  on both compilers too), `a_tracked_build_reports_leaks_and_bad_frees`, and cases in
  `typed/` and `typeerrors/` for associated constants and the address casts. 193 tests.

### r40 — compile-time conditionals, and `#location` at the caller

Raised by the user while designing tracking: the tracker's static must not exist unless
tracking is on, and nothing in the language could say so. Designed with the user (18.48).

- **`#if`** (§12.10). `ItemKind`, `MemberKind` and `StmtKind` each gained `CfgIf<T>` —
  one generic node, one parser (`dz_parse/src/cfg.rs`), one formatter routine and one dump
  routine for all three places. Every branch is parsed and formatted; `dz_driver::cfg`
  settles each module's flags and replaces every `#if` with its active branch's contents
  **before resolution**, so no later phase knows `#if` existed and an inactive branch may
  name anything. The later phases' matches carry an empty arm for it; the LSP outline shows
  every branch's declarations.
- **Flags.** Built-ins from the build mode and the host; `[flags]` in dazzle.toml
  (`NAME = false` or `{ default, doc }`, a built-in name refused); a dependency's turned on by
  `flags = [...]` on the dependency; `--flag NAME`, `--flag dep::NAME` and `--track-memory`
  on `run`, `build`, `cpp`, `check`, `hir`, `test` and `bench`, which now share one argument
  parser; `[build] track_memory`. An unregistered flag anywhere is **`DZ0309`**, with a
  did-you-mean over the package's flags or the built-ins. Every condition in a chain is
  evaluated, taken or not, so a misspelling in a later `#else #if` is still reported.
- **The build mode is an input to analysis now.** `--release` used to be applied to the
  workspace after analysing it; `#if DEBUG` needs it before, so `dz_driver::analyse_for`
  takes `BuildOptions`.
- **`#location` at the caller** (§12.7). It was typed as nothing and not emitted. It is a
  `SourceLocation`; written in an expression it is exact, and as a default argument codegen
  writes `__builtin_FILE()`, `__builtin_LINE()` and `__builtin_FUNCTION()` into the C++
  declaration — evaluated at each call, and through the `#line` directives already reporting
  the Dazzle file and line. The column is 0 there. Filling defaults in the checker instead
  was rejected: an ordinary default before `loc` (`align = 8`) would have had to be
  re-evaluated in the caller's scope, since C++ cannot skip a positional argument.
- **CI fix**: a union variant carrying a struct is a member named like its type, which g++ 14
  rejects unless the type is fully qualified (`-Wchanges-meaning`); g++ 13 and clang accept
  it, which is why it passed here and failed on Linux CI.
- Tests: `tests/run/flags` (a dependency, flags at all three levels, inactive branches
  naming nothing that exists), `build_flags_choose_the_branches` (`--release --flag CHEATS`,
  and `--flag CHEAT` refused), `tests/run/location`, `tests/errors/unregistered_flag.dz`, and
  `cfg_if.dz` in the parse and fmt corpora.

### r39 — M12 groundwork: `static`, associated functions, trait objects

The user put M12 ahead of M10 and M11 and settled the allocator design (spec 18.46,
18.47): `Allocators::Global` (malloc) and `Allocators::Temp` (a per-thread arena), no
`Arena` yet; tracking opt-in and reporting leaks, double and foreign frees, size/alignment
mismatches and use after a Temp reset; the `Allocator` trait on `&mut self` with a trailing
`loc: &SourceLocation = #location`; core stdlib types in the prelude. Writing that down
showed three things the allocators need that the language did not have, and those are r39.

- **`static`** (§7.7). A keyword, an item, `SymbolKind::Static`, `ItemSig::Static`,
  `BodyOwner::Static` and `hir::ExprKind::Static` beside their `const` counterparts. The
  initialiser must be constants (**`DZ0553`**, `is_static_initialiser`), because C++ builds
  statics before `main` in no promised order across files. A static is a writable place, and
  every way of changing one — assignment, `&mut`, a `&mut self` method, a `for`/`case`
  `&mut` binding, viewing an array as `[]mut` — goes through
  `require_unsafe_static_write` (**`DZ0528`**). Flow treats it as a borrow: nothing is moved
  or copied out (**`DZ0402`**, worded for a static). Codegen writes `inline [thread_local] T
  NAME = …;` after every type; a struct literal's `[&]` lambda is not legal at namespace
  scope, so its capture-default is dropped there.
- **Associated functions** (§7.3). `Quat::identity()` was already in the spec and parsed, but
  a `fn` with no receiver was checked as a qualified method call with its first argument
  taken as the receiver. It is now `hir::Callee::Assoc`, emitted as a C++ `static` member;
  the resolver no longer puts the type's fields in its scope; calling one on a value is
  **`DZ0520`**; `dz_mono` instantiates the owner of `Pair<i32>::both(4)`.
- **Trait objects** (§7.6, 18.6 — decided long ago, owed since M6). `&S`/`^S` convert to
  `&Trait`/`^Trait` in `base_convertible` when `S` implements it; a trait's methods are as
  visible as the trait. In C++ each object-safe trait gets `Shape__vtable` and
  `Shape__dyn : ::dz::Dyn<Shape__vtable>` — two words, forwarding members, null-comparable,
  convertible from `T*`, `T&` and `::dz::Addr<T>`. Each implementation declares
  `dz_vtable(const Shape__vtable*, const T*)` in its own module's namespace and the handle
  finds it by argument-dependent lookup: the orphan rule puts every implementation in the
  trait's module or the type's, which are exactly the namespaces ADL searches, so no
  specialisation or namespace juggling is needed. Default methods are vtable entries like
  any other. Not object-safe (generic trait, associated types, a method mentioning `Self`)
  stays `DZ0801`, as does a trait object of a type whose `impl` is generic.
- **Found on the way.** A `&T` produced by a block or an `if` and stored where a `^T` was
  wanted emitted `(*tmp)` into a pointer and did not compile (`Emitter::stored`, applied to
  `let`, assignment, `return` and struct-literal fields). And **C++ does not define the order
  a call's arguments are evaluated in**, so `println("{} {}", f(), x)` where `f` changes `x`
  printed different things under different compilers — the spec does not say what Dazzle
  promises. Two tests hit it; they now bind first. Raised with the user.
- Tests: `tests/run/statics`, `tests/run/assoc`, `tests/run/dynamic` (with a dependency
  module, so the `-Wall -Wextra` harness now copies whole fixtures), a case each way in
  `typed/` and `typeerrors/` for all three, `tests/errors/` gained five files, and the parse
  and fmt corpora gained statics.

### r38 — M8's criterion: a union of existing structs, and base arguments built by statements

M8 was marked done at r36, but its criterion had never been run. Writing the `ButtonState`
sample as §7.5 specifies it (`tests/run/button`) showed the one thing that made it the
sample: **a variant that is an existing struct did not exist.** `union ButtonState {
DefaultState … }` parsed as a variant with no payload, so `case DefaultState(d) {
d.update() }` had no `update` to find.

- **Design, settled with the user as 18.45:** a bare variant always names a struct in scope
  and carries it; a variant with no payload is `Point {}`. A bare name with nothing behind
  it is **`DZ0551`** rather than an empty variant, so a misspelling cannot change what a
  union holds. Every fixture with a bare no-payload variant gained `{}`; the formatter now
  prints `Point {}` rather than `Point {  }`.
- `VariantSig::carries` holds the struct, resolved in collection with `lookup_in_file`.
  A variant literal is checked by `struct_literal`, so the struct's defaults, visibility
  and constructor rule all apply, and its field values become the variant's payload; a
  bare `S::Idle` runs the same check with no fields. A `case` binding is the struct's own
  type, so methods resolve, and a plain binding of a move-only struct is `DZ0403`.
- Codegen puts the struct itself in the C++ union under its qualified name, orders it
  before the union in `sort_types`, and builds the variant by assigning a whole struct
  literal, so field defaults run even when the variant is not the union's first member.
  A carried struct with a constructor, `copy`, base or vtable is `DZ0801` (register, M12).
- **Two checker gaps the sample found.** `return .DefaultState{}` from a `-> ButtonState?`
  inferred from the `T?` and failed; a leading dot in `return` now infers from `T`. And an
  arm whose value was a `T?` warned `DZ0510` whenever the `switch` had no *expected type*,
  which includes `let next = switch …`: a block now asks whether its value is wanted
  (`value_wanted`, read on entry from `stmt_position`) rather than whether a type is.
- **The last M8 row, a base initialiser needing statements**, is closed: each argument
  that hoists statements becomes a `[&]` lambda called in place inside the
  member-initialiser list. `return` there has no meaning — nothing has begun to return from
  — so it is refused by the checker as **`DZ0552`** (r38, §7.3) rather than miscompiled.
- Tests: `tests/run/button` (in the `-Wall -Wextra` set), a `Guarded` constructor in
  `tests/run/inherit`, `a_bare_variant_carries_the_struct_it_names`, a case each way in
  `typed/` and `typeerrors/` for both codes, and `tests/errors/variant_names_no_struct.dz`
  and `return_in_base_initialiser.dz`. 191 tests.

### M8, and r36 — `switch` on constants, generic unions, values built by statements

Most of M8 — unions, `switch`, `T?`, `!->`, `or` — had landed with M5. What was left were
four rows in the deferral register; three are closed here.

- **`switch` on an integer, `char`, `bool` or `str`** (r36, spec §10.3). Labels are
  constants of the subject's type. The parser cannot tell `case Idle` from `case MAX`, so
  `ast::CasePat` keeps anything shaped `Name` or `Name(x)` as a `Variant` and everything else
  as a `Value`; the checker decides from the subject's type. Constness is read off the
  checked HIR — a literal, a `Const`, or operators and casts over those — rather than a
  second AST walk. Two new codes: **`DZ0547`** a label that is not a constant or not the
  kind the subject takes (a value on an enum, a binding on an integer, a float subject), and
  **`DZ0548`** a label matched twice, which now also covers `case A` twice on an enum.
  Integer duplicates compare by folded value, so `case 4, 2 + 2` is caught.
- **A `break` in an arm left only the `switch`.** Found while writing the tests: `switch`
  was a C++ `switch`, so Dazzle's `break` — which means the loop, there being no
  fallthrough — broke out of the C++ `switch` instead and the loop carried on. Every
  `switch` is now an `if`/`else if` chain over a subject evaluated once into `auto&&`
  (so a temporary subject lives, and a payload binding writes into the union), which also
  gives `str` labels somewhere to go. A pointer or reference subject goes through
  `dz::obj`; a value does not, since `obj` of a temporary would hand back a dangling
  reference. r36 states the rule in the spec.
- **Generic unions** (§7.5, §8.4). Deeper than the row said: the checker built every variant
  as a bare `Maybe`, dropping the arguments. `Ty::Variant` now carries the union's arguments
  so `case Some(s)` on a `Maybe<i32>` gives `s.value` the type `i32`, and a variant literal
  takes its arguments from `Maybe<i32>::Some`, from the expected type (`let m: Maybe<i32> =
  Maybe::Nothing`, `.Some{ … }`), or from the union's own parameters in scope; otherwise it
  is DZ0534. Mono already walked union field types; codegen gained `union_asts` beside
  `struct_asts`, and `union_decl` takes the instance's arguments.
- **Values built by statements where C++ allows only an expression** — a `const`, a field
  default, a default argument whose value is an `or` clause or a block-valued `if` or
  `switch`. Each becomes a lambda called in place (`immediately_invoked`); a constant built
  that way is `inline const` rather than `constexpr`. Two bugs fell out: **`const X: str =
  "…"` did not compile** (`dz::str`'s constructor casts, so it cannot be `constexpr`; a `str`
  constant is `inline const` now), and **constants were emitted after the structs**, so a
  field default naming one did not compile either. Enums are now defined first, then every
  constant whose type needs no struct, in the order they name each other, then the types.
- Tests: `tests/run/switching` runs all of it on both compilers and is in the `-Wall
  -Wextra` set; `switch_on_a_scalar_matches_constants` and
  `a_generic_union_is_read_through_its_arguments` pin the rules; `typed/` and `typeerrors/`
  gained a case each way; `tests/parse` and `tests/fmt` gained `switch_values.dz`. 187 tests.

### r30 — struct inheritance (spec 18.32)

All five steps of §4's plan, and the four codegen collisions it warned about.

- **Representation.** `StructSig` gains `base`, `inherited` and `polymorphic`. A base's
  fields are **flattened into the derived type's list, base-first**, which is what makes a
  field index mean one thing everywhere — the HIR, flow analysis, codegen, the LSP — and
  what makes the layout claim true. `inherited` says where the type's own begin, so the
  C++ struct emits only those and inherits the rest. `FieldSig::declared_in` remembers
  which type wrote an inherited field, since its span is in that type's file.
  Two new passes: `collect_base` resolves `: Base` once every signature exists, and
  `flatten_bases` copies the fields in parent-first so a chain of three works from one
  pass and a cycle is reported rather than recursed into.
- **The resolver links bases too** (`Symbol::base`, `members_with_bases`), because a
  derived type's methods see the base's fields unqualified and name resolution runs long
  before the checker. It is a pass of its own between the existing two: a base is a path,
  no path resolves until every declaration exists, and every link must be made before any
  body is walked. It repeats pass 2's import step and discards what that reports.
- **Conversions.** `&Derived` → `&Base` and `^Derived` → `^Base`, checked in `dz_check`
  rather than in `dz_hir::assignable`, which has no table to walk a base chain with. A
  by-value `Derived` gets a diagnostic of its own that says what to write instead.
- **Constructors.** `: UiElement(w)` parses into `FnDecl::base_init` and is checked inside
  the constructor's scope, so its arguments see the parameters; it lands in `Body::base_init`,
  outside `root`, and becomes C++'s member-initialiser list. A base with a constructor must
  be constructed through one, and a type with a constructor — its own or inherited — is no
  longer built with a struct literal.
- **`virtual` / `override` / `protected`** are new keywords (§21 r30 says so; they were not
  reserved, so this is a real, if tiny, breaking change — `tests/run/keywords` used
  `virtual` as a method name and had to be renamed). `override` is checked against the base
  in both directions, including the signature; a body-less `virtual` is C++'s `= 0` and
  makes the type abstract, refused by value the way an imported handle already was.
  `Base::method(self)` is a new HIR callee, `BaseMethod`, emitted as C++'s qualified
  `recv.Base::method()`.
- **Implementations are inherited** by `impls_for` walking the chain, nearest-first so a
  more specific `impl` wins. `Drop` and `Copy` are deliberately excluded: they are special
  members, C++ already runs a base's destructor as part of the derived one, and inheriting
  the base's `impl Drop` would emit a second destructor running the same body twice.

**The four collisions §4 predicted, all real:**

- `trivially_copyable` now answers `false` for a polymorphic struct — a C++ class with a
  virtual function is never trivially copyable, and `Buffer<T>`'s `memcpy` path over one
  would have corrupted the vtable pointer.
- The C++20 designated-initialiser path in `struct_literal` now also excludes a type with
  a base or a `virtual`, neither of which is an aggregate in the way that path assumes.
- `~T()` is `virtual` whenever the type or a base has a `virtual` method, and a polymorphic
  type with no `drop` of its own gets `virtual ~T() = default`.
- `field_list` walks bases by construction, since the flattening did it.

One more the survey had not predicted: **a base's `drop` makes the derived type
move-only.** C++ deletes the base's copy operations, so a copy Dazzle allowed would have
been refused by the C++ compiler — a generated-code error, which invariant 7 forbids.
`base_has_drop` is asked by both `copyable` and `trivially_copyable`.

### After M7 — a C++ keyword is a Dazzle name, and the criterion is guarded

Compiler-only work, so there is no §21 row. Two things M7 left behind, both found by
surveying rather than by a failing test:

- **A declaration named after a C++ keyword miscompiled, and blamed the user.** `sanitize`
  was applied to locals only, so `fn double(…)` reached C++ as `int32_t double(int32_t)`
  and clang's parse error was mapped back onto the user's own `.dz` line by the `#line`
  directives — a C++ error in generated code presented as a user error, which invariants 5
  and 7 both forbid. Every identifier the generator writes now goes through `sanitize`:
  structs, enums and their variants, unions and their variants and payload fields, fields,
  methods, free functions, constants, and the namespace a module opens — a declaration and
  every reference to it through the same function, which is what keeps the two spellings in
  step. It is applied unconditionally, imported names included, because a C++ header cannot
  declare something named after a C++ keyword and there is nothing to special-case.
  `tests/run/keywords` is a program whose every declaration is a keyword, and it is in the
  `-Wall -Wextra` set as well.
- **M7b's own acceptance criterion was not tested.** `tests/run/raylib` and
  `tests/run/cinterop_rgfw` open windows, so neither can carry an `expected.txt`, and
  `projects()` picks up only projects that have one — which meant nothing in `cargo test`
  covered the milestone's criterion. `the_windowed_interop_programs_build` takes the other
  half of that partition and runs `dz build` on it: libclang reads the headers, the
  library's own sources compile, the program links, and no window opens. It asserts the set
  is non-empty, so it cannot quietly come to cover nothing.

### r29 — single-header libraries, and the editor on imported names

- **`header` in a library's recipe** (§15.1). A single-header library needs exactly one
  translation unit that includes it with the implementation macro defined, and nothing else
  about that file is interesting — so `dz` writes `build/obj/<lib>/dz_<lib>_impl.c` and
  compiles it with the recipe's `defines` in force. `tests/run/cinterop_header` and
  `tests/run/cinterop_rgfw` both dropped their hand-written one-line `.c`. The include is
  absolute for the same reason the generated C++'s is: the file lives in `build/obj/`, and a
  quoted include is searched from there first.
- **Hover on an imported name** — the M7a deferral. There is no Dazzle source to quote, so
  the signature is rebuilt from the checker's own types (`fn area_of(kind: Kind, size: i32)
  -> i32`), followed by the C++ path it really is and the header's comment.
- **Doxygen is rendered**, for Dazzle's `///` comments and imported headers alike.
  `@param`/`\param`, `@return`, `@brief`, `@note`, `@see`, `@code` and the inline `@p`/`@c`
  become Markdown; anything else passes through untouched, so no comment is made worse by
  being rendered. The importer strips comment markers (`doc_comment`) and keeps the commands
  as the header's author wrote them.
- **Completion after a path walks every segment written before it.** `complete_path` was
  given only the last identifier, so `rl::MouseButton::` looked for `MouseButton` in the
  *current* file's scope and found nothing. It now takes the whole prefix: module alias, then
  a step per C++ namespace, then the symbol's members. A head that resolves to nothing still
  offers the §5.5 type queries, which `T::` on a generic parameter needs.
- **Signature help on an imported call**, through the same `foreign_decl` hover uses, so the
  two cannot disagree — the parameter being typed is named with its Dazzle type, and the
  header's Doxygen comes with it. Dazzle's own signature help renders Doxygen now too.
- **A module reads as a module.** `TokenClass::Namespace`: the head of a qualified path
  resolves to no symbol, so `rl` in `rl::SetTargetFPS(60)` was coloured as a variable.
  Semantic tokens now emit a namespace for a module alias and for a C++ namespace. The
  grammar gained an `imports` rule to match — `cpp` in `import cpp` is part of the keyword,
  and what follows `as` is a module name, not a variable.

### r28 — raylib from source, and C++ references (M7b's criterion met)

- **`tests/run/raylib` opens a raylib window**, raylib 6.0 compiled from its own vendored
  source by `dz` — seven `.c` files, `rglfw.c` among them, no CMake and no prebuilt
  library. This is M7b's acceptance criterion (spec §19). It carries **no `expected.txt`**,
  so the run suite does not execute it: it opens a window and waits, as
  `tests/run/cinterop_rgfw` does. `dz build tests/run/raylib` is the check.
- **A module named after the header it imports included itself.** Module `raylib` generates
  `build/cpp/raylib.h`; a quoted `#include "raylib.h"` is searched from the including
  file's own directory first, so it found the generated file, whose guard was already
  defined — and every raylib declaration vanished. The *importer* had the same bug, and
  worse: it depended on whether an earlier build had left the file there, so `dz check`
  passed and `dz build` failed. Both now emit the **angle form**, which searches only the
  `-I` paths — exactly the include dirs the header was resolved with — and can never reach
  generated output. The two sides must agree, or the compiler would check one header and
  compile against another.
- **C++ references import** (§15.2): `CType::Ref`, mapped to `Ty::Ref`, which `cpp_ty`
  already spelled `T&` / `const T&`. `adopt_handles` and `drop_dangling` treat a reference
  as a pointer, so a type only ever reached through one is still a handle.
- **A link failure no longer claims to be a compiler bug.** `DZ0803` said "this is a bug in
  the Dazzle compiler" for *any* C++ failure, which is right for a compile error
  (invariant 7) and wrong for a link error — where the code was accepted and what is
  missing is a definition. `linker_failure` tells them apart, and the link message points
  at the recipe: a single-header library needs one source that defines its
  `*_IMPLEMENTATION` macro, since a library's `defines` build its own sources and nothing
  else (r22).

### r27 — C++: namespaces, classes, overloads, template instances (M7b)

The C++ half of §15. `tests/run/cppinterop` is the fixture: a small C++ library vendored as
source (`vendor/shapes`), built by `dz`, and called from Dazzle — verified on clang++ and
g++.

- **Compound-literal macros.** `type_macros` finished: a macro clang can type but not fold
  keeps its type (`ConstValue::Typed`), so raylib's `RAYWHITE` is a `Color`. The bug was
  that an `auto` variable's *declared* type is the placeholder, whose spelling shows the
  deduced type while its kind does not — `clang_getCanonicalType` first.
- **The second translation unit never found the header.** Both extra passes wrote
  `#include "<path>"` using the path clang reported, which is *relative* whenever the `-I`
  was; a quoted include is searched from the including file's directory first, and that is
  `build/cpp`. So the macro pass had been silently importing nothing outside tests, where
  the path happened to be absolute. `Walker::header_include` absolutises it.
- **Every declaration carries its C++ spelling** (`Decl::cpp`) and the namespaces around it
  (`Decl::scope`). `CType::Named` and `DeclKind::Alias` now hold that path rather than a
  bare name — two namespaces may each declare a `Body`. Codegen writes `Decl::cpp` verbatim
  instead of reconstructing it, and mangles imported types by it.
- **Namespaces** are `SymbolKind::Namespace` symbols; `Resolution::inner` holds what each
  one contains, and the resolver's `check_members`, the checker's `path_head` and
  `lower_path_type` all walk it one segment at a time.
- **Classes** fill exactly the places a Dazzle `impl` block fills — `StructSig::methods`,
  `method_keys`, `overloads`, `ctors` — so **overload resolution by argument type, receiver
  mutability and visibility all work on C++ methods with no second mechanism**; §18.27's
  method keys already expressed what C++ overloading needs. `::dz::obj(recv).name(args)` was
  already the right emission.
- **A C++ destructor stays C++'s.** `StructSig::cpp_owned` makes such a type move-only
  (DZ0403, with its own message) without Dazzle emitting a `drop` — a public copy
  constructor makes it an ordinary value again.
- **Private and protected members are no longer imported.** They were, which meant Dazzle
  accepted `buf.data` and C++ rejected it — an invariant-7 break.
- **`[cpp.instantiate]`** (§15.4), which needed quoted keys in `dazzle.toml`. libclang
  exposes *no children* for an implicit specialization, so the members are read from the
  template pattern with its parameters bound to the instantiation's arguments
  (`Walker::template_args`, consulted by `map_type`). A member whose type does not follow
  from that binding maps to nothing and is skipped, as anything unmappable is.
- **`static` member functions are skipped with their reason**: they need associated
  functions, which Dazzle does not have (§7).

### r26 — C callbacks, and function values that compile (M7b)

- The importer maps a pointer to a function type onto Dazzle's own `fn(…) -> T`. A variadic
  one, or a parameter that cannot be mapped, leaves the declaration out as before.
- **Function values did not compile at all** — in any Dazzle program, not just an importing
  one. `let g = twice` reached C++ as `int32_t(*)(int32_t) g = …`, and a
  `fn(i32) -> i32` parameter the same way: C++ writes a function pointer with the name
  *inside* the parentheses, so the type cannot go where a type goes. The shim gained
  `template <class R, class... A> using fn = R (*)(A...)`, and `cpp_ty` now spells a
  function type `::dz::fn<R, A…>`. §5.4 has had the type since r1 and nothing could use it.
- `tests/run/cinterop` hands `triple` to C through `mini_apply`; a run test pins the
  Dazzle-only case; an RGFW probe registers `RGFW_setEventCallback(RGFW_windowResized,
  on_event)`, compiles and runs.

### r25 — C array members and bit-fields (M7b)

- A struct's `float m[16]` imports as `[16]f32` and `unsigned on : 1` as an ordinary
  member. Both were hidden before, which took whole structs out of reach — raylib's
  `Matrix` and most device structs are built from them.
- **Codegen: `::dz::obj` is now emitted only where it does something**, which is reaching
  through a pointer. A C array member handed to a template decays to a pointer and takes
  its length with it, so `obj(T*)` won by partial ordering and indexing broke; adding
  array overloads only made the call ambiguous. A value or a reference never needed the
  wrapper at all. The shim gained raw-array `index`, `len` and `data`.
- **Invariant 7 nearly slipped through**: `let copy = s.items` emitted
  `::dz::Array<int32_t, 4> copy = b.items;`, which C++ rejects — a raw array cannot be
  copied or assigned. The checker now tracks whether a member access is *reached into* —
  indexed, iterated, `.len`, `.ptr` — and refuses a whole C array anywhere else
  (**`DZ0707`**), instead of leaving it to the C++ compiler to complain about generated
  code. `&` on a bit-field is **`DZ0706`** for the same reason: C++ has no reference to one.
- `mini.h` gained `MiniFlags` — two bit-fields and an `int[4]` — which
  `tests/run/cinterop` reads, indexes, iterates and passes back to C; run tests pin
  `DZ0706` and `DZ0707`, and an importer test pins the field shapes.

### r24 — C unions, and two silent mistakes (M7b)

- RGFW's event loop is built on `RGFW_event`, a named C union filled in through a pointer.
  A C union is not Dazzle's: §7.5's carries a tag and switches exhaustively over it, which
  a C one cannot promise. It imports as a type whose members are the same memory, so
  `e.type` reads exactly what C reads and a literal may set one member (**`DZ0705`**).
  `map_type` no longer refuses a union, and `record` imports one with `is_union` set.
- **The loop runs**: `RGFW_init`, `RGFW_createWindow`, a `RGFW_event` by value,
  `RGFW_window_checkEvent(win, &mut event)` polled until it returns 0, `event.type`
  compared against an enumerator, then `RGFW_window_close` and `RGFW_deinit`.
- A diagnostic that was missing entirely: `.Variant` where the expected type is neither an
  enum nor a union reported **nothing** — `.NotAThing` against a `u32` checked clean, since
  `dot_infer` returned an error node without saying anything. It now names the type that
  was wanted. (The user's own `rgfw::RGFW_windowCenter::RGFW_windowCenter` was already
  `DZ0305`: the flag is an untyped integer, so the constant alone, `0`, and `a | b` all
  pass a `u32` parameter.)
- `mini.h` gained `MiniValue`, a union of an `int` and a `float`: `tests/run/cinterop`
  makes one through the library, reads it back, writes a literal, and prints both; a run
  test pins `DZ0705`, and a checker test pins the `.Variant` message.

### r23 — handles: a C type its header only declares (M7b)

- Found by the user: RGFW's `RGFW_createWindow` did not exist as far as Dazzle was
  concerned. RGFW keeps `struct RGFW_window`'s definition behind `RGFW_NATIVE` /
  `RGFW_IMPLEMENTATION`, so a consumer sees a forward declaration and nothing else — and
  M7a skipped opaque structs, stranding every function that mentions one.
- The importer imports a record it never sees defined as `Struct { opaque: true }`, and
  `adopt_handles` does the same for a type the header never mentions at all — one declared
  in a system header it includes, `FILE` or `HWND` — **when every use of it is behind a
  pointer**. Used by value, the declaration that does so is still dropped: its size really
  is unknown.
- The checker makes such a type a handle: not copyable, and refused wherever a value of one
  would have to exist. `lower_type` gained a wrapper that walks the lowered type and reports
  the new **`DZ0704`** for a handle that is not behind a pointer — one hook covering locals,
  fields, parameters, returns, generic arguments and containers — and `p^` on one is the
  same error, since there is nothing to read out.
- `mini.h` gained `MiniCounter`, defined only in `mini.c`: `tests/run/cinterop` bumps it
  through its pointer and prints the count, and a run test pins `DZ0704` for holding one by
  value. RGFW's window API resolves with no workaround.

### r22 — a vendored library's defines are its own

- Found by the user, vendoring **RGFW**: the project would not link. Two mistakes in M7a's
  recipe model, one on each side of the boundary.
- `Workspace::cpp_flags` handed a library's `defines` to everything that included its
  header, so the generated C++ was compiled with `RGFW_IMPLEMENTATION` too and every symbol
  RGFW defines was defined twice (`LNK1169`). A library's defines now build its own sources
  and nothing else; `[cpp]`'s are the shared ones, as §15.1's `JPH_PROFILE_ENABLED` always
  was.
- The other way round, a module's `[cpp]` defines never reached its libraries' sources, so
  the `RGFWDEF=` that gives a single-header library's declarations external linkage could
  not be written anywhere that worked: with it, the header's declarations are `inline` with
  no definition in that translation unit (`LNK1120`). `native::build_libraries` now passes
  the module's `[cpp]` include dirs and defines before the library's own, which win where
  the two disagree.
- `tests/run/cinterop_header` is the shape both bugs live in: a single-header library whose
  implementation is behind `SHL_IMPLEMENTATION` and whose `SHLDEF` is `inline` unless the
  build says otherwise. Leaking the library's define makes it fail to link with duplicate
  symbols; not propagating the module's makes it fail with an unresolved one.

### r21 — making a `cstr`

- The user asked how to create one and found there was no way: a literal did not fit a
  `cstr` parameter (`DZ0501`), `"x" as cstr` type-checked and then produced C++ that did
  not compile (`DZ0803` — invariant 7), and `cstr("x")` said nothing of its own, so the
  only message was `` `{unknown}` cannot be formatted `` from the `println` that consumed it.
- §5.3's `[PROPOSED]` "conversion to `cstr` is explicit and may allocate" contradicted
  §15.1's own `InitWindow(…, "Dazzle")`. Settled as 18.39 / r21: **a string literal is a
  `cstr` wherever one is wanted**, typed by its context as an untyped number is, and emitted
  as the C literal itself rather than as `::dz::str(s, n)`. `"x" as cstr` hands the same
  expected type to its operand. `c as str` works through an explicit `str(const char*)` in
  the shim, which measures it. A runtime `str` is refused, with a help naming M12's `String`
  as the owning copy it would need.
- Two diagnostics fixed alongside: a builtin type name in value position — `cstr("x")`,
  `i32(3)` — is `DZ0503` at the name, with the conversion in its help; and `check_format`
  no longer reports an argument whose type is already an error, which is what turned one
  mistake into two.

---

## 3. Deferral register

Everything knowingly postponed. Each row has a comment in the code naming the same
milestone. **This is the list to check before claiming a milestone is complete.**

| Owed by | What is deferred | Where |
|---|---|---|
| **M9** | Code actions (fill `switch` arms, insert `move()`, add `impl Drop`) and inlay hints for implicit `drop`s — §17.4 items 6 and 7 | `dz_lsp/src/lib.rs`, `capabilities` |
| **M9** | No incremental analysis: every settle re-runs the whole project. Fine at today's sizes; §17.2's per-module cache is the answer | `dz_lsp/src/lib.rs`, `analyse` |
| **M9** | `impl` methods and inherited trait defaults are tied to their type by name, so two same-named types in different modules could cross. The checker knows each implementation's real target; it should say | `dz_lsp/src/ide.rs`, `methods_of` |
| **M12** | **A module named after a runtime-shim identifier does not compile.** The shim declares about forty names directly in `namespace dz` — `obj`, `len`, `mv`, `str`, `index`, `Array`… — and a module’s namespace is `dz::<name>`, so a module called `obj` emits `namespace dz::obj` beside the existing `dz::obj()` and g++ rejects it as "redeclared as different kind of entity". Found by `a_module_named_after_a_build_directory_does_not_collide`, which now skips that one name. Listing the shim’s names in the namespace sanitiser would work but drifts as the shim grows; moving the shim into `dz::rt` and leaving `dz` to module namespaces alone is the fix that cannot drift | `dz_cpp/src/lib.rs`, `qualified_namespace` |
| **M12** | **A generic base.** `struct Button : Panel<i32>` is refused (`DZ0541`): inheriting an instantiation means laying out one base per instance, which monomorphization does not do. A generic *derived* type over a concrete base works | `dz_check/src/lib.rs`, `collect_base` |
| **M12** | **Several constructors are not resolved by argument.** A constructor call, and a base initialiser, take the first one declared — the same limitation both had before inheritance, now visible in two places | `dz_check/src/check.rs`, `call` |
| **M7a** | **Hover on an imported name** shows nothing: the editor queries build their text from Dazzle declarations. Go-to-definition should already land in the header, since the symbol's span points there — untested | `dz_lsp/src/ide.rs` |
| **M7a** | The import cache is **in-process only**. §15.1 wants it on disk too, so `dz check` does not re-parse an unchanged header; the LSP, which matters most, is covered | `dz_cppimport`, `cache` |
| **M7b** | Left out of an import, with a reason in `Header::skipped`: variadic functions and function pointers; a `wchar_t` pointer; an rvalue reference `T&&`; a `static` member function (it needs associated functions, §7); a member whose type does not follow from a template instantiation's argument binding | `dz_cppimport`, `map_type` |
| **M7b** | **An overloaded free function imports only its first declaration** — `claim` takes the name and the rest are passed over. Overloaded *methods* do resolve by argument type (§15.3). The `@(...)` disambiguation suffix is `[PROPOSED]` and unparsed | `dz_cppimport`, `function` |
| **M7b** | An imported overloaded method is not cast at the boundary the way a free function is: `hir::Callee::Method` carries the name, not the overload key, so `dz_cpp` applies `c_cast`/`ret_cast` only where the name names one method | `dz_cpp/src/lib.rs`, `call` |
| **M7b** | Inheritance between imported C++ classes: a base's members are not imported into a derived class, and `&Derived` does not convert to `&Base`. **No longer blocked** — struct inheritance landed at r30, so the Dazzle side (a `Symbol::base`, a flattened field list, the conversions) exists and what remains is the importer filling those in from libclang, plus letting `: Base` name an imported class, which `collect_base` refuses today with `DZ0541` | `dz_cppimport`, `collect_methods`; `dz_check/src/lib.rs`, `collect_base` |
| **M7b** | A whole C array member is refused rather than copied (`DZ0707`), and cannot be passed as a `[]T`. Copying one needs a helper that fills a `dz::Array` element by element; the slice needs a `Slice{ptr, len}` built at the use site. Indexing, `for`, `.len` and `.ptr` all work | `dz_check/src/check.rs`, `is_c_array_member` |
| **M7b** | libclang parses with its own default target (MSVC here), not the C++ compiler's: a header needing the C library's own headers may find MSVC's while g++ builds against mingw's | `dz_cppimport`, `arguments` |
| **M7b** | An untyped imported integer is not range-checked against its context: `let b: u8 = rl::SOME_BIG` reaches C++ as a narrowing `static_cast`. The importer knows the value; the checker should compare it, as it does a literal's | `dz_check/src/lib.rs`, `collect_foreign` |
| **M12** | **`indexed()` exists only in a `for` head.** It is recognised there by the checker, not declared: a real adapter is a generic struct implementing `Iterate` over another, whose `Item` needs associated-type projection (the row below). `move(xs).indexed()` is refused for the same reason | `dz_check/src/check.rs`, `for_stmt` |
| **M12** | **Memory tracking is single-threaded and prints to stdout.** `TRACKER` is one static with no lock, so two threads allocating from `Global` under `--track-memory` race — concurrency is v2 (18.11). The leak report goes through `println`, because there is no way to write to stderr from Dazzle yet | `dz_project/src/std/memory/tracker.dz` |
| **M12** | **A trait method's non-constant default argument** is rendered on the handle's declaration, before the module's structs are defined, so a default like `Vec3{}` on a trait method would not compile there | `dz_cpp`, `trait_object_decls` |
| **M12** | **`for` over a generic `C: Iterable`** is refused (`DZ0801`): the loop's element is the projection of `C::Iter`'s `Item`, the same associated-type projection a generic `T: Iterate` loop waits on. Concrete containers loop (r47) | `dz_check/src/check.rs`, `container_iterator` |
| **M12** | **A union variant carrying a struct with a constructor cannot be built**: the variant literal is the struct's literal, which a type with a constructor refuses (DZ0543), and there is no spelling for building the variant through the constructor. The union itself compiles and drops correctly (r47) — how to spell `.Sound(5)` is a question for the user | `dz_check/src/check.rs`, `union_variant_literal` |
| **M12** | `List::insert` and `remove` move elements one at a time, even for a `T` whose bytes could be moved in one `memmove`; there is no `memmove` builtin yet | `dz_project/src/std/collections/list.dz` |
| **M12** | **A `HashMap<String, V>` is looked up with a `&String`**, so finding a key you hold as a `str` builds a `String` first. Rust answers this with `Borrow<Q>`; Dazzle has no such trait yet | `dz_project/src/std/collections/hash_map.dz` |
| **M12** | **`hash_combine` relies on `u64` arithmetic wrapping**, which C++ guarantees and the spec does not yet say (§5 leaves overflow of plain arithmetic unstated, and signed overflow is not checked in debug as §5.1 proposes). `wrapping_add` exists now, and since r50 is in the same module. Deciding what plain `+` does on overflow settles both | `dz_project/src/std/core/hash.dz` |
| **M12** | **Methods on an untyped literal**: `2.0.sqrt()` has no type to find `Float` on until the literal meets a context; write `(2.0 as f32).sqrt()` or bind it first | `dz_check/src/check.rs`, `call` |
| **M12** | **`Mat4` and raylib's `Matrix`** hold the same numbers, but raylib's struct declares its fields row by row (`m0, m4, m8, m12, …`), so converting is a field-by-field copy rather than a cast | `dz_project/src/std/math/mat4.dz` |
| **M9** | **`std` is type-checked in every analysis.** Its trees are parsed once per process (`dz_driver::std_tree`), which took a debug-build analysis of `tests/run/hello` from ~75 ms to ~61 ms (release: ~22 ms before). What remains is checking its ~2 200 lines each time, ~40 ms of the debug figure: symbols, the type table and HIR are whole-program, so reusing `std`'s needs the per-module analysis cache §17.2 already asks of M9 | `dz_driver`, `analyse_for` |
| **M9** | **An unused-dependency warning can be wrong for a dependency used only through `.`**: a trait method it adds, called on a value whose type comes from elsewhere, is resolved by the checker, which does not mark the dependency used. Name the trait, or ignore the warning | `dz_resolve`, `report_unused_dependencies` |
| **M9** | **A dependency's `pub` dependency inside a `[names]` value**: `[names]` takes a key of this module, so choosing `rl::shapes`'s `Rectangle` needs a `via` key first | `dz_resolve`, `module_names` |
| **M12** | **An intrinsic default uses the C++ compiler's builtins**, so its column is 0 and its function is the C++ name (`bump`, not `Counter::bump`) | `dz_cpp`, `caller_intrinsic` |
| **M10** | Macro declarations parse shallowly; bodies are kept as raw source and reproduced verbatim by the formatter | `dz_ast/src/lib.rs`, `dz_fmt/src/lib.rs` |
| **M10** | The intrinsics `#module`, `#location` and `#expr_text` are typed but not emitted (`#file`, `#line`, `#column` and `#function` are) | `dz_cpp`, `intrinsic` |
| **M10** | **`T::fields`**, compile-time reflection (§13.4), needs macros to iterate it; the runtime table is done | `dz_check`, `path_type_query` |
| **M12** | **An imported C or C++ enum's variants are not in the type table**: its constants are the header's to spell, and `offsetof`-style reading of their values was not needed by anything yet. Its size, backing type and name are | `dz_cpp/src/reflect.rs`, `describe_named` |
| **M12** | **Generic methods** — a method with type parameters of its own — report `DZ0801`. A generic *type*'s methods are instantiated; a method generic over its own arguments needs a second instance key and has no user yet | `dz_cpp/src/lib.rs`, `struct_decl` |
| **M12** | **Trait objects that are not object-safe** — of a generic trait, one with associated types, or one whose methods mention `Self` — report `DZ0801`, and so does a trait object of a type whose `impl` is generic, which would need a vtable per instance. | `dz_cpp`, `object_safe`; `dz_check`, `refuse_generic_impl_object` |
| **M12** | **Associated-type projection** (`T::Item`, `T::Output`). An associated type resolves through an implementer but cannot be named off a bounded parameter, so `for` over a generic `T: Iterate` is refused by the checker (`DZ0801`), and generic arithmetic cannot name its result type | `dz_check/src/check.rs`, `iterate_item` |
| **M12** | An open-ended range (`lo..`) held in a variable rather than written in a `for` head | `dz_cpp`, `expr` |
| **M12** | A trait default method is exercised by `tests/run/generics` only for bodies that call the trait's own methods; one that builds or returns a `Self` value has no test | `dz_mono`, `inherited_defaults` |
| **M12** | `error(…)` and `format(…)` write into a static ring of four 256-byte buffers, so the fifth live message overwrites the first. §9.2 requires no allocation *and* a message that outlives the call; a real answer needs the stdlib's `String` | `dz_cpp/src/runtime.rs` |
| **M12** | `or unreachable` panics in release as well as debug. §9.4 says release may assume the success path, and §18.20 has not settled whether that means `__builtin_unreachable` or merely skipping the check | `dz_cpp/src/lib.rs`, `clause` |
| **M12** | **`println`/`print`/`format` are still builtins**, and a `String` is printed through `s.as_str()`: `Format` has no implementations and `Formatter` is still empty, now that `String` exists to give it a buffer. `HashMap` is still to come | `dz_cpp/src/runtime.rs`, `dz_project/src/std/core/format.dz` |

### Not deferred — deliberately out of scope forever

- Lifetimes. `&T` cannot be stored in a struct field (§5.2, §18.3) and that is the whole
  mechanism; a dangling `^T` is the programmer's problem (§1.1).
- Function overloading, until a later language version (§18.5).

---

## 3a. M11 — reflection (r51), done 2026-09-14

Taken up by the user after the import redesign, with three decisions (18.59): the table
describes **every kind of type**, not only structs; field metadata is an **Odin-style tag**, a
raw string ending the field (`` hp: i32 = 100 `json:"health"` ``), not language-defined
attributes; and M9 stays open.

- **Syntax**: `dz_ast::Field::tag`, parsed after the default; the formatter and dump print it.
- **std**: `std/reflect/` — `TypeId` (a struct around `u32`, `Eq`/`Hash`), `TypeKind`,
  `Visibility`, `TypeInfo`, `FieldInfo` (with `tag_value(key)`, written in Dazzle over bytes)
  and `VariantInfo`. `module`, `own` and `type` are keywords or begin members, so the fields
  are `module_name`, `owning` and `type_id`.
- **Checker**: `reflect_call` types `typeinfo_of<T>()` / `typeid_of<T>()` as the new HIR node
  `ExprKind::Reflect { ty, info }` (substituted and visited by mono like `TypeQuery`) and
  `typeinfo(id)` as a builtin; `type_info = "none"` is DZ0555.
- **Codegen** (`dz_cpp/src/reflect.rs`): every emitter interns the types it reflects on into a
  shared registry, keyed by their C++ spelling from `std`'s namespace, and `dz_typeinfo.cpp` is
  written last: the closure over fields, variants and element types, filled once by a
  function-local static's constructor (generated structs have constructors, and §16.4 forbids
  global ones), names as `uint8_t` arrays so they stay constant, offsets by `offsetof` with
  `-Winvalid-offsetof` off for the file. `type_info = "all"` seeds it with every declared type; there is no `#[reflect]` (the user's call: a program keeps the ids it wants listed). The shim
  declares `::dz::reflect_entry`, which bounds-checks the id.
- **Found**: a trait reached through a pointer field (`List`'s `^mut Allocator`) has no size —
  described as a size-less `TraitObject`.
- Tests: `tests/run/reflect` (the criterion; warning-clean set), `typed/` and `typeerrors/`,
  `type_info_none_refuses_reflection_and_all_describes_everything`, and an imported C struct
  reflected on by hand.

### r52 — attributes are checked (18.60)

Asked for by the user once `#[reflect]` was removed and would have been silently accepted.
`Walker::attributes` in `dz_resolve` checks every item's and member's attributes: an
unknown name is DZ0314 (with `did you mean`, and `#if` for `cfg`); `#[test]`/`#[bench]` off a
module-level function, `#[thread_local]` off a `static`, arguments, or a repeat are DZ0315;
the specified-but-unbuilt `inline`, `align`, `packed`, `deprecated`, `export`, `derive`,
`persist` and `raw` are DZ0316. The checker's own "`#[test]` takes no arguments" moved there.
Tests: nine `tests/errors` programs, one per mistake and one valid.

## 4. Next: M12, the stdlib

Chosen by the user on 2026-09-13 over M10 and M11. M9's editor rows stay open in §3. The
milestone's criterion is a small game written without touching C++.

1. ✅ **Language groundwork (r39)** — `static`, associated functions, trait objects.
2. ✅ **`#if` and `#location` at the caller (r40).**
3. ✅ **Associated constants (r41).**
4. ✅ **Allocators and tracking (r41)**, and trait objects finished for them.
5. ✅ **`List<T>` (named `Buffer<T>` until r43) and `String` (r42).**

What remains, in the order the user set on 2026-09-13:

6. ✅ **Left-to-right argument evaluation** in codegen (r41). Before, a call went to C++ as
   written, and g++ and clang++ on Windows both evaluated `three(say(1), say(2), say(3))`
   right to left; a hoisted argument (`or`, a block value) also ran ahead of the arguments
   to its left. Now `dz_cpp` renders each argument on its own (`capture_args`), then writes
   them out in order (`emit_args`): its hoisted statements, and a temporary only where an
   argument with effects meets one whose value could change — so a call of plain reads
   emits exactly what it did. A value temporary is handed on with `::dz::mv`; a `&T`, and
   the places `construct`/`destroy`/`move_out` work on, are bound by `auto&&` and
   forwarded. A receiver is bound only when an argument has effects, since C++17 already
   sequences the object expression first. Builtins (`println`, `error`…) take the same
   path. `tests/run/argorder` pins calls, a builtin, an `or` clause, a local changed by a
   later argument, a move-only argument and a trait-method receiver; it is in the
   warning-clean set, and g++ was checked by hand too.

   **r44 extended it to every operand**, at the user's request: binary operators
   (`ordered_operands`; `&&`/`||` keep their own short-circuiting lowering), index and
   slice operands (the container held by reference as a place), range bounds and tuple and
   array elements (`sequenced_elements` — C++ braced initialisers are already ordered, so
   only hoisted work needs a temporary), and struct and union literal fields in *written*
   order (`literal_fields` recovers it from the value spans, since the HIR keeps fields in
   declaration order). Assignment evaluates the value before the place — the user's call,
   matching C++17 and Rust — so only hoisted statements needed reordering there. The
   `argorder` fixture grew a case for each. Not touched: a `for` loop's range bounds, which
   are written into the C++ `for` head.

   **The editor: going to a prelude definition.** Ctrl+click on `String`, `List`,
   `Allocators` or `Allocators::Temp` did nothing — the server dropped every location in the
   prelude, whose `<prelude>/dz.dz` path is not a file. `dz lsp` now writes the prelude's
   exact text, read-only, to `%TEMP%/dazzle/prelude-<hash>/dz.dz` and reports locations
   there; a query from inside that copy is answered by any open project's analysis. Two bugs
   found on the way: an associated constant (`Temp`) had no definition at all, and the
   source map holds the prelude under two file ids, only one of them parsed —
   `ide::prelude_file` finds the right one.
7. ✅ **`HashMap<K, V>` and `Hash` (r45).** Open addressing with linear probing over three
   parallel arrays (hashes, keys, values; a zero hash marks an empty slot, so a key whose
   hash is zero is stored as one), doubling at 3/4 full, backward-shift deletion. The
   compiler side: `primitive_implements` gives integers, `bool`, `char` and `str` `Hash`, and
   `str` `Eq`; a prelude trait method called on a primitive (`n.hash()`, `a.cmp(&b)`)
   resolves in `call` and is lowered by `native_trait_call` — `hash` to the shim's
   `::dz::hash`, SplitMix64's finaliser for integers and FNV-1a then that for `str`. Found on
   the way and fixed: `&` of a non-place (`get(&5)`) emitted `::dz::addr(5)`, which C++
   refuses — it now holds the value in a temporary — and its literal ignored the context
   (`&7` wanted as `&u8` was `&i32`); the `&&` lowering wrapped its condition in doubled
   parentheses, which clang warns about; and the unsatisfied-bound help still suggested
   `struct T : Trait`, the spelling r13 replaced. `insert` returns nothing and `replace`
   returns the old value, because a discarded `V?` warns (DZ0510) and most inserts discard
   it. `tests/run/hashmap` (warning-clean, and leak-free under `--track-memory` in
   `a_tracked_build_reports_leaks_and_bad_frees`) checks removal against a reference list
   over 500 keys, every key kind, move-only values dropped exactly once, copying and
   `Temp`; `typed/` and `typeerrors/` have a case each.
8. ✅ **`assert` as a function (r46).** Removed from `BUILTIN_FNS`; the prelude has
   `assert(cond, msg = "", loc = #location)`. For it to report its caller's line, `panic`
   takes a `SourceLocation` as its last argument — the user's call, "pass the location, by
   default `#location`": `check_format` leaves a final `SourceLocation` out of the format
   arguments and codegen passes it as the panic's location. `tests/run/assert` (a `#panic`
   fixture) checks the report names `src/main.dz:13`; `typed/` has a `panic(…, loc)` helper
   and `assert` calls, `typeerrors/` a location standing in for a placeholder's argument and
   a non-`bool` condition.
9. ✅ **Math (r48).** Design chosen by the user on 2026-09-13 (18.55): `std::math`,
   `f32` types plus `Vec2i`/`Vec3i`, methods everywhere, column-major.
   - **Standard-library modules.** `dz_project::STD_MODULES` lists compiler-carried modules
     with pseudo-paths (`<std>/math.dz`); `dz_driver::import_std` adds one to the workspace,
     parses it under the file id `Workspace::add_std` gave it, and makes it an implicit
     dependency of each importing module — only when some file writes `import std::…`, so a
     program that does not pays nothing. A manifest named `std` or `std::…` is an error; an
     unknown `std::…` import lists the modules that exist; `import std::math` is never
     "unused", since `x.sqrt()` uses it through `.`. `std` is sanitized to `std_` in C++, or
     `dz::std` would capture every `std::` inside `namespace dz`.
   - **Methods on numbers.** `std::math` declares `Float` and `Integer`; the checker counts
     them among the traits the machine's types implement (`prelude_names`,
     `primitive_implements`), and `native_trait_call` lowers each call to `::dz::num::…` in
     the shim — `<math.h>` for floats; wrapping through the same-width unsigned type,
     `__builtin_*_overflow` for checked and saturating. One trait per kind instead of an
     `impl` per type: ten integer types would have been ~150 identical methods. A method of
     the other kind of number (`n.sqrt()`) says which kind has it.
   - **The rest is Dazzle** (`crates/dz_project/src/std/math.dz`). `Mat4::inverse` was first
     transcribed wrong; the replacement was checked against 50 random matrices in Python and
     generated from the verified formula.
   - **Found by writing it**: unary `-` never reached `Neg`; `x == Vec3{…}` failed because
     `Eq::eq` wants `&Self` and a temporary is no place; `-128: i8` was rejected; the
     operator-trait help still spelled `struct T : Trait`. Fixed, with `typed/`/`typeerrors/`
     cases. The editor's and debugger's copies of embedded sources now cover every one, so
     go-to-definition and stepping reach `std/math.dz` (`embedded_copies`,
     `with_embedded_lines`).
   - Tests: `tests/run/math` (every value checked by hand; warning-clean on both compilers),
     three `tests/errors` cases, `a_module_cannot_be_named_under_std`,
     `definition_of_a_std_math_name_is_in_std_math`.
10. ✅ **Variadic parameters (r49).** Design chosen by the user on 2026-09-14 (18.56).
    - **Syntax**: `dz_ast::Param::variadic`, set by the parser on `name: ..T`; the formatter
      and dump print it back. `Ctx::param_sigs` lowers it to a read-only `[]T`, sets
      `FnSig::variadic`, and reports a variadic that is not last or has a default
      (DZ0554, new) — once: the default is not also checked against the slice.
    - **Calls**: `check_variadic_args` checks the fixed arguments as usual, then either
      passes a lone spread `..xs` (parsed as a start-less range) as the slice, or gathers
      the rest into one `ArrayLit` typed `[N]T` — an empty slice when N is 0, emitted as
      `Slice<T>()` — so codegen, flow and monomorphization see an array passed where a
      slice is wanted and needed nothing new. `variadic_element` borrows a value given where
      a read-only trait reference or pointer is wanted. A generic `T` is inferred through
      the existing array-to-slice unification.
    - **Editor**: signature help keeps the variadic parameter active past its first argument.
    - **Found**: `..^Trait` makes every call through an element `unsafe`; `..&Trait` is
      allowed (a slice of references) and safe, so the spec shows that spelling.
    - Tests: `tests/run/variadic` (order, drops, spread, generic, method, constructor,
      mixed types; warning-clean), `typed/`/`typeerrors/`, the parse and fmt corpora,
      `signature_help_stays_on_a_variadic_parameter`.
    - **Next, not done**: `println`/`format` as library functions need `Format` implemented
      for the built-in types and a real `Formatter`, plus a way to mark a parameter as a
      format string the compiler checks against the variadic arguments.
11. ✅ **Imports in the manifest; the prelude becomes `std` (r50).** Designed with the user
    on 2026-09-14 (18.57, 18.58), after a first pass at file-level `import m::*` was
    written, tested, and dropped in favour of this.
    - **`std` is one module, one declaration to a file.** `prelude.dz` and `std/math.dz` were
      split by a script (checked: the same non-comment lines, the same doc comments) into
      `crates/dz_project/src/std/{core,collections,memory,text,math}/*.dz`, listed in
      `STD_SOURCES` under `<std>/…` pseudo-paths; `tests/std_sources.rs` holds the list to
      the directory. It is module 0 as the prelude was, always loaded — `STD_MODULES`,
      `add_std` and the driver's `import_std` are gone, as is the name `dz`. The checker's
      and codegen's `Float`/`Integer` test is now "a trait of module 0".
    - **The manifest** (`dz_project`): a dependency is `path`, `cpp` (a recipe in this
      manifest or a dependency's) or `via`, with `open` and `pub`; the key must be an
      identifier; `std = {…}` takes no source; `[names]` picks. `CppDep`s become
      `Dependency`s when the driver has read the header (`import_headers` now walks
      `Module::cpp_deps`, and a recipe's `import = [...]` names the header, one for now).
      A module that does not list `std` still depends on it, `implicit`, so its generated
      C++ includes it and `for`/operators work.
    - **Resolution**: `ModuleNames` per module (keys, opened, picks, public), built after
      pass 1 so `[names]` can name declarations; `via` walks public dependencies. A bare
      name: locals → own declarations → file `{ … }` → `[names]` → builtins → opened, where
      two different symbols are DZ0310 (no order-based precedence, by the user's choice).
      `Resolution::module_prefix`/`path_item` walk `key::pubdep::Item::ns…`, and the checker
      now uses `path_item` everywhere it used to special-case two segments. File imports:
      `{ … }` and `as` only; `import m` alone and `import cpp` are DZ0312 with the manifest
      lines as help. Not-found help now says which dependency has the name and whether it
      is `std` not being listed. **DZ0311**: a library's `pub` signature naming a type from
      a module that is not itself, `std`, or publicly reachable; executables are exempt.
    - **Codegen** includes a foreign dependency's header by the recipe's `import` name, and
      no longer emits `#include "<recipe>.h"` for one.
    - Tests: `tests/workspace/opened` (open, `[names]`, `via`, a file pick — must check
      clean through the checker) and `opened_errors` (DZ0310, DZ0303 into a private
      dependency, DZ0301 for a private name, DZ0311) in `dz_driver/tests/opened.rs`, plus a
      module with no `std`; `tests/errors` for DZ0312 twice and a module declaration
      shadowing `std`. Every fixture manifest gained `std = { open = true }`, every
      `import cpp` moved into its recipe.
    - **Afterwards, the same day**: a recipe's `import` takes several headers, parsed as one
      unit (`Request::headers`; each header's path from the inclusions the synthetic main
      file makes directly, and the macro and instantiation passes include them all);
      **DZ0313** on a dependency nothing names (`Resolution::used_deps`, filled by key
      paths, imports, opened names, `[names]` and `via`; public dependencies and `std` are
      exempt); `std`'s trees parsed once per process; and hover and go-to-definition on a
      dependency key — the header of a `cpp` one, the manifest of a project, through public
      dependencies too (`ide::module_info`).
12. The criterion: a small raylib game with no C++ of its own.

**`error(…)` keeps static storage** — decided by the user on 2026-09-13. Moving it onto
`Allocators::Temp` was considered and refused: an allocator is itself a place that reports
errors, and one whose message needed an allocator would be circular.

**Design decisions 18.51–18.54 (r47), chosen by the user on 2026-09-13, all implemented.**

- **Enums are `Eq`, `Hash` and `Ord`** (18.54). `TypeTable::builtin_implements` answers for
  enums as `primitive_implements` does for numbers; `n.hash()`/`a.cmp(&b)` resolve on an
  enum receiver, and `native_trait_call` lowers them. An enum key in `tests/run/hashmap`;
  `typed/` and `typeerrors/` (an enum is not `Add`).
- **`T` converts to `T?` wherever one is expected** (18.53). `assignable` accepts it and
  `join` joins a `T` arm with a `T?` arm. A bare `none` arm with nothing to type it gets
  a placeholder (`ty::pending_none`, an optional of `Never`) that `settle_pending_nones`
  replaces once the `if`/`switch` has joined its arms — or passes to an enclosing arm, or
  falls back to the function's `T?` return, or reports DZ0521 once per `none`.
  `.Variant` where a `T?` is wanted now infers from `T`. Found on the way: `t or …` on a
  move-only optional local copied the optional in C++; it is moved. `tests/run/optionals`.
- **A union owns its active payload** (18.52). A union whose payloads are not all plain
  data gets a destructor, copy (or deleted copy) and move members that switch on the tag,
  and `become(kind)`, which drops the old payload and builds the new one; a variant literal
  uses it. Plain-data unions are emitted as before. Found on the way: `copyable`,
  `needs_drop` and `trivially_copyable` read a variant's `fields` only, so a *carried*
  struct's `drop` was invisible — `payload_types` includes it. `tests/run/unions` (every
  drop exactly once; leak-free under `--track-memory`; warning-clean), and flow cases for a
  move-only union copied and for copyable and move-only unions used correctly.
- **`Iterable` and `IterableMut`** (18.51). Prelude traits, with `SliceIter`/`SliceIterMut`
  and `HashMapIter`/`HashMapIterMut` (whose `Item` is `(&K, &V)` or `(&K, &mut V)`), and
  implementations for `List` and `HashMap`. The checker's `container_iterator` replaces a
  container subject with a call to `iter()` — `iter_mut()` when a binding is `&mut` — so
  codegen and flow see an ordinary `Iterate` loop. Diagnostics say it as the loop:
  "`Readings` can be looped over, but not written through", "cannot refer to these elements
  with `&mut`". A `const Slice<T>` now indexes to `T&` (the shim had tied a view's
  writability to the view object, which `SliceIterMut::get(&self)` exposed).
  `tests/run/iterable`. A generic `C: Iterable` loop is refused (deferral register).

**Editor work alongside, asked for by the user on 2026-09-13.**

- **Keywords added since the grammar was written** — `static`, `virtual`, `override`,
  `protected` — were uncoloured and missing from completion. `static` is now coloured as
  `let` is; the other three as modifiers, with `pub`.
- **Debugging, step one ✅.** No Dazzle debugger: the `#line` directives already make a C++
  compiler's debug information name `.dz` files, so the VS Code extension contributes a
  `dazzle` debug type (and breakpoints in `.dz` files, and a Debug Project button) that runs
  `dz build` and hands the executable to a native debugger — the Visual Studio debugger when
  the build left a fresh PDB (clang++ on Windows), gdb for g++'s DWARF, CodeLLDB otherwise.
  `dazzle.natvis` shows `str`, slices, arrays, `T?` and ranges readably under the Visual
  Studio debugger. On the compiler side, the prelude's `#line` directives named
  `<prelude>/dz.dz`, which no debugger can open: `emit_cpp` now writes the prelude to
  `build/cpp/prelude/dz.dz` and points the directives there, indented ones inside lambdas
  included (`line_directives_name_files_a_debugger_can_open`); panic locations keep the
  short name. Verified with gdb from the command line (a breakpoint on a `main.dz` line,
  locals by their Dazzle names, stepping into `List::push` in the prelude copy) and with
  `llvm-pdbutil` for clang's PDB; F5 in VS Code confirmed working by the user on 2026-09-13,
  including `str` values shown as text with their length.
  **Next steps**, not started: gdb pretty-printers to match the natvis; showing a
  monomorphized `List__i32` as its elements (natvis cannot match a name pattern that is not
  a template, so this needs either a common template base in the shim or generated natvis);
  and keeping `_dz_*` temporaries out of the locals list, which would mean scoping them in
  blocks of their own.

The earlier plan (CI, then fetched packages) follows; CI is running.

### r53 — handles: `Rc`, `Weak`, `Box`; `if let` bindings (18.61, 18.62)

Designed with the user on 2026-09-14, over several rounds (see §4c).

- **Syntax**: `dz_ast::IfBinding::mode` — `if let &v` and `if let &mut v`, parsed with the
  `for`/`case` `parse_bind_mode`; the formatter and dump print it.
- **Checker**: `if_let_binding` types the binding as `for_binding` and `case_binding_type`
  do, for `T?`, `&T?`, `^T` and handles; `or` reads through a `&T?` (`MOVE_OUT_OF_BORROW`
  for a move-only one). Handles are anything implementing the prelude trait `Nullable`
  (`nullable`): `null` takes the handle's type, `h == null` becomes `h.is_null()`, and
  `guarded_handle` rewrites the subject of `or`/`if let` to the new HIR node
  `ExprKind::Guarded { guard, init: h.guard(), value: guard.ptr() }` — a pointer, so the
  rest of `or` and `if let` is the `^T` path unchanged.
- **Codegen**: `Guarded` emits the guard as a line of the current C++ block; an `if let` on
  one opens a block of its own so the guard ends with it. `null` of a handle is `T{}`.
  `if let` bindings: `auto&&` to the optional for a reference, a copy otherwise.
- **Flow**: `Guarded` initialises its local like a `let`; a reference `if let` binding reads
  its operand in place instead of taking it.
- **std**: `memory/{nullable,rc_block,rc,weak,box,unguarded}.dz`. One allocation per `Rc`
  (`RcBlock`: counts, allocator, value); the value dropped with the last `Rc`, the block
  freed when the `Weak`s are gone too.
- **Found on the way**: `copyable` answered `true` for every `T?` (a catch-all), so a
  move-only optional counted as copyable; and `or` on a `&u32?` — what `^u32? or return`
  gives — was refused as "cannot fail". Both fixed.
- **Migrated**: `if let p = ptr` that wrote through `p` in `tests/run/dynamic`,
  `tests/run/iterator` and `typed/`.
- Tests: `tests/run/handles` (every form, the guard outliving `self.texture = null`, a `Weak`
  going null, `Box`; leak-free under `--track-memory`; warning-clean), and `tests/errors`:
  `if_let_mut_on_a_read_only_handle`, `if_let_copy_of_a_move_only_target`,
  `if_let_mut_on_a_read_only_pointer`, `or_moves_out_of_a_reference_to_an_optional`,
  `handles_in_use`.

## 4c. To do: what a game still needs (readiness review, 2026-09-14)

Asked for by the user after a review of whether Dazzle can carry a real game today. The
deferral register (§3) lists what the *code* postpones; this is the list of what is
**missing** for a game, grouped by area and roughly in the order a game would hit it. Rows
marked **design** need the user's decision before implementation. Tick or delete a row
when it lands, with a pointer to the revision.

### Ownership and smart pointers

Raw pointers are the escape hatch, not the everyday tool (spec §5.2). Game code that shares
an object — a texture used by several sprites, a scene node with children, an entity
referenced by systems — should hold a smart pointer, never a `^T` read through `unsafe`.
Only containers and the allocators should need `unsafe` inside `std`.

- [x] **`Rc<T>`, `Weak<T>`, `Box<T>`** — r53, see "r53 — handles" below. Decided with the
  user (18.61, 18.62): each may be `null`; reached only through `== null`, `or` and `if let`
  (so no `Deref`); `if let` takes a `Weak` directly (no `upgrade`); `or`/`if let` hold a
  strong `Rc` while the value is used; `&mut` needs the handle in a writable place.
- [ ] **`Arc<T>`** — atomically counted `Rc`. Waits on atomics, which wait on concurrency
  (18.11, v2); listed so the name is reserved and `Rc`'s API is shaped to match.
- [ ] **design — `Rc<Trait>` / `Rc<Base>`.** Whether an `Rc<Button>` converts to an
  `Rc<UiElement>` or an `Rc` of a trait object, as `&Derived` → `&Base` does. Needs either
  a conversion the checker knows about or trait objects that are not only `^Trait`/`&Trait`.
- [ ] **Skip the guard's count when nothing can drop the owner.** An `if let` body with no
  call the compiler cannot see into and no assignment to the handle cannot free the value,
  so the count up and down could be left out. A compiler detail, not in the spec.
- [ ] **A handle's `or` with a value fallback.** `rc or 7` is refused, as `ptr or 7` is: the
  success side is a `&T`. `if let v = rc { v } else { 7 }` works. Worth deciding for both
  pointers and handles together.
- [ ] **Handles** — a generational `Pool<T>` / `Handle<T>` (index + generation) for
  entities and resources, the other everyday alternative to pointers in game code.

### I/O and the operating system

`std` has no way to talk to the machine except `print`/`println` to stdout. A game can reach
these through raylib or C today; a stdlib should not require it.

- [ ] **Files**: read a whole file into a `List<u8>`/`String`, write one, open/read/write/
  seek/close as a move-only `File` whose `drop` closes it; all fallible (`!->`).
- [ ] **Paths and directories**: join, extension, exists, list a directory, create one.
- [ ] **stderr**: `eprint`/`eprintln`. The memory tracker's leak report goes to stdout for
  want of it (§3).
- [ ] **Time**: a monotonic clock (`Instant`, `Duration`) — the shim already has
  `now_nanos` for `dz bench` — plus wall-clock time and `sleep`.
- [ ] **Random**: a seedable, fast, non-cryptographic generator (PCG or xoshiro) with
  ranges for integers and floats, and `Vec2`/`Vec3` helpers.
- [ ] **Process**: command-line arguments, environment variables, exit code.

### Formatting and text

- [ ] **`println`/`print`/`format` as library functions** (§3, last row): `Format`
  implemented for the built-in types and `std` types, a real `Formatter` writing into a
  `String` or a stream, and a way to mark a parameter as a format string the compiler
  checks against its variadic arguments (18.56 made the arguments possible).
- [ ] **`format` into a `String`** — `error`/`format` today share a ring of four 256-byte
  buffers, so a fifth live message overwrites the first. `error` keeps static storage by
  the user's decision (§4); `format` does not have to.
- [ ] **Format specifiers**: width, precision, padding, hex (`{:.2}`, `{:08x}`).
- [ ] **`String` operations**: split, trim, find, replace, starts/ends with, parse to
  integer and float (fallible), and `HashMap<String, V>` looked up by `str` (§3,
  `Borrow<Q>`).
- [ ] **A `cstr` from a runtime `str`**: an owned, zero-terminated copy (18.39 left it for
  `String`) — every C API taking a file name or a label needs it.

### Generics

- [ ] **Generic methods** — `fn map<U>(&self, …)` on a type (§3, `DZ0801`).
- [ ] **Associated-type projection** — `T::Item`, `T::Output` off a bound. Unblocks `for`
  over a generic `C: Iterable`, generic arithmetic, and real iterator adapters (§3).
- [ ] **Iterator adapters** — `indexed()` as a real adapter rather than a `for`-head
  special case, then `filter`, `map`, `rev`, `zip` (§3).
- [ ] **Constructor chosen by argument types** — today the first declared constructor
  wins (§3).
- [ ] **A generic base** — `struct Button : Panel<i32>` (§3, `DZ0541`).
- [ ] **Trait objects that are not object-safe today** — of a generic trait, or of a type
  whose `impl` is generic (§3).

### C++ interop for the libraries a game uses

- [ ] **Overloaded free functions** (§3, M7b). **Dear ImGui's C++ API is built on them** —
  `ImGui::Checkbox`, `Combo`, `BeginChild`, `TreeNode`, `PushID` and more have several
  overloads, and today only the first declaration of each imports. Until this lands ImGui is
  usable only through cimgui, its C wrapper. Needs the `@(...)` disambiguation suffix
  (`[PROPOSED]`) or resolution by argument type, as methods already do.
- [ ] **Variadic C functions** — `ImGui::Text(const char* fmt, ...)`, `printf`-style logging;
  skipped by the importer today (§3).
- [ ] **Inheritance between imported C++ classes** (§3, M7b) — Jolt's shapes, bodies and
  listeners are class hierarchies.
- [ ] **A Dazzle struct implementing an imported C++ abstract class** — Jolt's
  `ContactListener`, `BroadPhaseLayerInterface` and ImGui backends are subclassed by the user.
- [ ] **Proof on real libraries**: a `tests/run` fixture for Dear ImGui (over raylib or GLFW)
  and one for Jolt, each built from vendored source, as raylib is.

### Semantics still open

- [ ] **design — integer overflow**: what plain `+` does (wrap, or panic in debug as §5.1
  proposes). `hash_combine` relies on wrapping today (§3).
- [ ] **design — 18.20**: `or unreachable` in release.

### Concurrency (v2 by 18.11 — listed so it is not forgotten)

- [ ] Threads, a mutex, atomics, then a job system; `Arc<T>` above; a thread-safe memory
  tracker (§3).

### Build and tooling

- [ ] **Measure compile time at scale**: a generated 20k–50k-line project, cold and warm,
  through `dz build`. Principle 6 is "compile fast" and nothing measures it yet.
- [ ] **Incremental analysis** in the LSP, and `std` not re-checked on every keystroke (§3,
  M9).
- [ ] **Code actions and inlay hints** (§3, M9).
- [ ] **A release workflow** shipping `dz` with libclang beside it (§4a).
- [ ] **Fetched packages** (M16, §4a).
- [ ] **Macros** (M10) and **hot reload** (M13), per the roadmap.

### The criterion

- [ ] **A small raylib game with no C++ of its own** (M12, item 12 above) — written as
  soon as `Rc`, files, time, random and library `format` exist, and used to reorder this list.

## 4a. Earlier: get CI green, then fetched packages

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

The one thing still missing is `assert`, which is typed but not emitted (a §3 row, M12);
until then a test says what went wrong with `panic`, which is what the runner reports.


1. `#[test]` on a `fn` with no parameters and no return, checked as one: a test that takes
   an argument or returns a value is an error at the attribute, not at the call.
2. Tests are **excluded from a normal build** — `dz build` must not link them — and are the
   only entry points `dz test` generates a `main` for. That means a second codegen
   configuration, which is also what M15 needs.
3. A failure is a `panic` (§9.8), which already aborts with a location, so the first
   version needs no assertion machinery beyond `assert` becoming real (a §3 row, M12).
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

### Shipping a `dz` that needs nothing installed

The goal: **a released `dz` depends on nothing on the machine except a C++ compiler**, and
the compiler is the user's choice rather than a fixed requirement (r33 made that
configurable at four levels). What stands in the way is **libclang**, which `import cpp`
loads at run time to read C and C++ headers.

**Done now:** `dz` looks for libclang beside its own executable, or in a `lib/` next to it,
before falling back to an installed LLVM. So a release that unpacks as

```text
dz/
  bin/dz
  lib/libclang.so
```

reads headers on a machine with no LLVM. Nothing is bundled by a `cargo build`, so a
development build behaves exactly as before.

**What is left is packaging, not code.** The options, and why the shipped-beside-`dz` one
is the one to take:

| | | |
|---|---|---|
| **Ship the binary beside `dz`** | ~50–130 MB per platform, built once per release. Apache-2.0 with the LLVM exception, so redistributable with attribution | **the plan.** Fetch it in the release workflow rather than checking it into git — a 300 MB repository is its own problem |
| Link libclang statically | One binary, nothing to install | Needs LLVM built with the right flags, which every contributor building from source would then need too. Hours per build |
| Parse headers ourselves | No dependency at all | A C preprocessor plus enough C++ to read Jolt's templates is a multi-year project, and every mistake is a miscompile of somebody's engine |
| Download on first use | Small release | Still a dependency, just a later one, and it needs the network on a machine that may not have it |

Two things make this less pressing than it looks. libclang is needed **only** for
`import cpp`: a Dazzle program with no C interop never loads it. And it is needed at
*compile* time only — nothing links into the program `dz` produces.

**Owed:** a release workflow that builds `dz` for the three platforms, fetches the matching
libclang, and produces an archive in the layout above. That is where the bundling belongs,
and it is the natural next step after CI is green.

### M16 — dependencies that are fetched

`load_dependencies` accepts `{ path = … }` and says so in its own diagnostic: *"only path
dependencies are supported so far"*. A git source is a real feature, not a manifest tweak:

1. `{ git = "…", rev = "…" }`, and **`rev` is required** — a branch name is not a version,
   and resolving `master` at build time means two machines build different code. A tag is
   resolved to its commit and recorded, not trusted.
2. A **lockfile, checked in**, holding the exact commit and a content hash for every
   transitive dependency. Without it the pinning is advisory.
3. A local cache keyed on the commit, and a build that is otherwise identical to a path
   dependency's — fetched source is source.
4. Fetching and building code from a URL is the one place this toolchain would execute
   something the user did not write. Worth deciding explicitly, before implementing, what
   `dz` will and will not run from a fetched package (today: nothing — there are no build
   scripts, and `[cpp.libs]` recipes are declarative, which is a property worth keeping).

### CI — GitHub Actions across Linux, macOS and Windows ✅ running

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

- **libclang parses with its own default target** (a §3 row): on Windows that is MSVC while
  `g++` builds against mingw. CI running both compilers is what should surface it.
- **Lints run on a pinned toolchain.** `-D warnings` against a floating `stable` means any
  new lint Rust ships turns CI red through no change of ours, which is what happened on the
  very first run: 1.98 added `question_mark`. `LINT_TOOLCHAIN` is bumped deliberately.

### M9's remaining editor work, resized

- **Size and alignment on hover** is *not* a small LSP feature, because **Dazzle has no
  layout model**: `T::size` and `T::align` are emitted as C++'s `sizeof` and `alignof` and
  the answer is the C++ compiler's. That is deliberate — it is what makes a Dazzle struct
  ABI-identical to the C++ it is compiled to, and what lets an imported C++ class keep its
  header's layout. So the editor cannot just look it up. Three ways, in preference order:
  **ask libclang** (already a dependency, already loaded in-process, `clang_Type_getSizeOf`
  and `getOffsetOf` over the generated header — accurate by construction and gives field
  offsets, which is the genuinely useful part for a game programmer laying out data);
  compile a probe translation unit and read it back (accurate, slower, another process); or
  reimplement the platform ABI in Dazzle (fast, and wrong the first time the ABI disagrees).
  The first is the one to build.
- **Doxygen comment generation in the VS Code extension** — typing `///` above a
  declaration expands to a stub with one `@param` per parameter and an `@return` when the
  signature has one. Entirely extension-side: the client already runs `dz lsp`, and the
  outline query already knows every declaration's parameters, so the information is there.
  Self-contained, and the right size for a first contribution to `editors/vscode`.
- **Quick fixes** are already a §3 row (M9, code actions). The ones worth having are the
  ones the compiler already computes the fix for: insert `move()` where a value was
  implicitly copied, add the missing `override`, add `: Base(…)` to a constructor that
  needs one, fill a `switch`'s missing arms.

---

## 4b. Struct inheritance (spec 18.32) — done at r30

### Struct inheritance (spec 18.32)

Designed with the user and recorded as 18.32; written into §7.3 by the revision that
implements it. `: Base` already parses and is reserved. The work, roughly in order:

1. **Layout and conversions.** The base's fields come first, so `&Derived` → `&Base` and
   `^Derived` → `^Base` are free; lower to real C++ single inheritance. A by-value
   `Derived` → `Base` is an error (no slicing). Fields and methods of the base are in
   scope in the derived type, as its own are (§7.3).
2. **Constructors.** A base with a constructor must be constructed through it:
   `Button(…) : UiElement(r) { … }`, an error when left out; a base without one is
   default-initialised (§6.2). The same rule makes a struct literal of a type with a
   constructor an error — agreed alongside, and part of the same revision.
3. **`virtual` / `override`.** Both checked in both directions; a body-less `virtual fn`
   makes the base abstract and not constructible. `drop` is virtual automatically once a
   type or a base has a `virtual` method. `UiElement::draw(self)` calls the base's version.
   Lowers to C++ `virtual`, which is also what lets a Dazzle struct inherit an imported C++
   class at M7 (Jolt's listeners) — and what M13's hot reload will have to re-point.
4. **`protected`**, a fourth visibility: the type's own methods and its derived types'.
5. **Implementations are inherited** — a `Button` is `Inspect` if `UiElement` is — and a
   more specific `impl Inspect for Button` overrides the inherited one. Static, so
   dispatch through a `&UiElement` goes to `UiElement`'s implementation; runtime dispatch
   is a `virtual` method called from it. This needs `impl_applies` to consider bases, and
   overlap to allow exactly this one shape.

**Four places in codegen a base or a `virtual` will collide with**, found while surveying
before the work started. None is hard; all four are silent if missed:

- `trivially_copyable` (`dz_check/src/lib.rs`) tests for a `drop`, a `copy` and its
  fields. A C++ class with a virtual function is **never** trivially copyable — the
  standard requires no virtual functions and no virtual bases — so a polymorphic type
  would take `Buffer<T>`'s `memcpy` path and `T::is_trivially_copyable` would fold the
  wrong way. It has to answer `false` for a type that has, or inherits, a `virtual`.
- The aggregate test in `struct_literal` (`dz_cpp`) checks for constructors and special
  members. A type with a base is not an aggregate in the sense the C++20 designated-
  initialiser path assumes, so "has a base" belongs in that test too.
- `~T()` becomes `virtual ~T()` once the type or a base has a `virtual` method, and the
  explicit `this->~T()` calls in the move and assignment paths (`dz_cpp`) want a second
  look once it does.
- `field_list` must walk bases before the type's own fields, which is also what makes the
  layout claim in step 1 true. The LSP's `methods_of` ties methods to a type by name
  (already a §3 row) and gets one more reason to carry the real target.

---

## 5. Test map

| Corpus | Runner | Covers |
|---|---|---|
| `tests/lex/*.dz` | `dz_lex/tests/lex.rs` | tokens + lexer diagnostics, snapshotted |
| `tests/parse/*.dz` | `dz_parse/tests/parse.rs` | AST + parser diagnostics, incl. `errors.dz` for recovery |
| `tests/fmt/*.dz`, `tests/parse/*.dz`, `tests/run/*/src/main.dz` | `dz_fmt/tests/fmt.rs` | idempotence, tree preservation, re-parsing, and every comment kept in order — over the whole-program corpus too, which is what catches a formatter that changes meaning (a dropped `!->`, a comment moved out of its block); plus targeted cases for comments in expression blocks, empty blocks holding a comment, and blank lines after a header comment |
| `tests/workspace/good` | `dz_resolve/tests/resolve.rs` | three-module workspace, resolves clean |
| `tests/workspace/private` | " | one violation per §3/§4 rule |
| `tests/workspace/cycle` | " | circular module dependencies |
| `tests/workspace/badmanifest` | " | manifest validation |
| `tests/workspace/typed` | `dz_check/tests/check.rs` | **must produce zero diagnostics** |
| `tests/workspace/typeerrors` | " | one deliberate failure per type rule |
| `tests/flow/*.dz` | `dz_flow/tests/flow.rs` | one file per ownership rule, each naming the code it must produce in an `// expect:` line, plus `clean.dz` which must produce nothing. The count is asserted too, so a rule that fires twice for one mistake is a regression |
| `tests/run/*/` | `dz/tests/run.rs` | whole programs compiled by a real C++ compiler and executed, stdout compared against `expected.txt`. A first line of `#panic` marks a program that must abort. `features` covers M4's constructs plus array literals and `.Variant`, `ownership` §6's special members and drop ordering, `failure` §9's `or` clauses plus unions and tuples, `generics` §8's `Buffer<T>` for two element types, traits and default methods, a user `Iterate`, format specs and folded constants. `cinterop` covers §15's C mapping end to end and `cinterop_header` the single-header library both define-scoping bugs lived in; `cinterop_rgfw` (no `expected.txt` — it opens a window, so it is built rather than run) is the RGFW probe. `run.rs` also pins that runaway instantiation is one `DZ0536` and that a trait object is refused |
| `tests/run/cppinterop` | " | §15's **C++** mapping against a vendored C++ library: a namespace and a nested one, a scoped enum, `const T&` and `T&` parameters, a class with a constructor and `const`/mutating methods, an overload set resolved by argument type (§15.3), a class C++ destroys itself (move-only, no Dazzle `drop`), and a `[cpp.instantiate]` template instance (§15.4) |
| `tests/run/raylib` | `dz/tests/run.rs`, `the_windowed_interop_programs_build` | **M7b's criterion**: raylib 6.0 compiled from its own vendored source by `dz` and a window opened from Dazzle. It opens a window and waits, so it carries no `expected.txt` and its stdout is not compared; instead every project under `tests/run/` that has *no* `expected.txt` — this and `cinterop_rgfw` — is **built**, which exercises the import, the recipe, the library's own sources and the link, and stops short of the window. Run it by hand to see the window |
| `tests/bench` | `dz/tests/run.rs`, `the_benchmark_suite_agrees_with_its_cpp_baseline` | **the C++ comparison** (spec §19 M15): each `#[bench]` is one of a pair, the other calling hand-written C++ vendored in `vendor/baseline`, built at the same optimisation level into the same binary. The *numbers* are not asserted — one machine's timing is not a fact about the language — but the pairs' **results** are: `main` prints each Dazzle answer beside its C++ one and the test requires them equal, because two benchmarks computing different things measure nothing. Run it with `dz bench tests/bench` |
| `tests/run/tests` | `dz/tests/run.rs`, `dz_test_runs_the_projects_tests` | **`#[test]` and `dz test`** (spec §19 M14): three passing tests, run by name in their own processes. Its `expected.txt` is the other half of the claim — running the *program* prints only what `main` prints, because a normal build leaves the tests out, which the test also checks by grepping the generated C++. `a_failing_test_is_reported_and_does_not_stop_the_rest` covers a panicking test: named, its own message shown, and the tests after it still run |
| `tests/run/inherit` | " | **struct inheritance end to end** (spec §7.3, 18.32): a chain of three, inherited fields and methods, `&Derived` → `&Base` and `^Derived` → `^Base`, writing through a base reference, defaults coming from whichever type declared the field, a constructor building its base, `virtual`/`override` dispatching at runtime through a base reference, a base method calling a virtual one and landing on the override, `Base::method(self)` calling the version it replaced, `protected` reached from a derived type, and a trait implementation inherited from a base and replaced by a more specific one. Also in the `-Wall -Wextra` set |
| `dz_driver/tests/feature_matrix/mod.rs` | `dz_driver/tests/matrix.rs`, `dz/tests/matrix.rs` | **the feature matrix**: every construct and every mistake in every context (function, method, generic function, loop and `while` bodies, `switch` arm, `or` block, `if let`, `unsafe`, nested blocks, constructor, trait method, generic struct method), with the shared types in the same module and in another. Constructs are compiled, run and value-checked, warning-clean under every compiler present; mistakes must each give exactly one code. Extend it by adding a row to any of the three lists |
| `tests/errors/*.dz` | `dz_driver/tests/errors.rs` | **one program per mistake**, whole pipeline, each naming its exact diagnostics in `// expect:` lines (or `(none)`); exact match, so a mistake reported twice fails. The home for rules in combination: bindings inside `switch` inside `for`, moves inside loops and arms, generic unions inferred inside generics, constants built by statements |
| `tests/run/loops` | `dz/tests/run.rs` | **r37**: `for` bindings by copy, `&` and `&mut` over arrays, slices through `&mut`, structs and tuples; `indexed()` over an array, an iterator and a range with `continue`; an ECS-shaped iterator yielding `(usize, &P, &mut i32)` written through; `case (&mut c)` inside a loop over `&mut` elements; `or break` in an arm of a generic loop; inferred generic unions; move-only elements referred to and then consumed. Also in the `-Wall -Wextra` set |
| `tests/run/switching` | " | **M8**: `switch` on integers, `char`, `bool` and `str` against literals, constants and constant arithmetic (§10.3); `break` and `continue` in an arm acting on the loop; a temporary subject and a payload written through `&mut`; a generic union built three ways and matched inside a generic function; and a `const`, a field default and a default argument built by a `switch` or `if`, with the constants written after their users. Also in the `-Wall -Wextra` set |
| `tests/run/keywords` | " | every declaration named after a C++ keyword — an enum, a struct, a union, a field, a method, a variant, a variant's field, a `const` and a free function all called `class`, `double`, `operator` and the like. The generator has to rename each one and every reference to it; it used to rename only locals, so `fn double` reached C++ as `int32_t double(…)` and clang's complaint was mapped back onto the user's own line. Also in the `-Wall -Wextra` set |
| `tests/run/cinterop*`, `tests/run/cppinterop` | `dz_cppimport/tests/import.rs` | the importer against those same headers, one test per §15.2 rule: functions/structs/enums/macros, handles, unions, array members and bit-fields, callbacks, namespaces and C++ paths, public members and overloads, the move-only rule, and that a template imports only where an instantiation is named |

| `tests/run/generics` | `dz_lsp/tests/ide.rs` | the editor queries against a real program: hover and definition for every kind of name, including beside a syntax error; references counted against every token of the name, so a missed use fails; rename and what it refuses; completion in scope, on a typed receiver (`largest(…).`), by name from an older analysis, after `::`, and silence in comments and strings; signature help; inlay hints; the outline; workspace symbols; formatting refusing broken source |
| `tests/run/*/` | `dz_driver/tests/tolerant.rs` | editor mode on about 1,200 damaged programs (a token deleted, the file cut off, a stray `.`, `(` or `{`): never a panic, never a later phase's errors while an earlier one failed, and diagnostics identical to build mode's |

`examples/*.dz` are **not** tests. They are the user's pre-spec design sketches and are not
expected to parse.
