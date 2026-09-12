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
| Milestones done | M0–M5, **M6 — generics, traits, monomorphization**, **M7a — C interop from vendored source**; most of **M9 — language server and VS Code extension**, pulled forward (what remains is in §3) |
| Next | **M7b — the rest of C, then C++.** Handles are in (r23); still to come: `char *` and `long *`, arrays and bit-fields in struct fields, unions, function pointers, compound-literal macros (`RAYWHITE`), raylib from its own source, then C++ classes, namespaces and overloads. Struct inheritance (spec 18.32) follows. See §4 |
| Tests | 161, all green |
| Clippy | clean (`--workspace --all-targets`) |
| Compiler crates | 16, plus the `dz` CLI |
| Language spec | 0.1 **unreleased**, revision 18; four open questions (§18.2), one of them inheritance, which is designed and follows M7 |
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
| `dz_project` | `dazzle.toml`, globs, dependency graph, **the prelude** (`src/prelude.dz`) | §3.1, §3.2 |
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
| **next** | **Struct inheritance.** `struct Button : UiElement` parses, and the checker reports it with `DZ0801`; the design is settled as spec 18.32 | `dz_check/src/check.rs`, `item` |
| **M7a** | **Hover on an imported name** shows nothing: the editor queries build their text from Dazzle declarations. Go-to-definition should already land in the header, since the symbol's span points there — untested | `dz_lsp/src/ide.rs` |
| **M7a** | The import cache is **in-process only**. §15.1 wants it on disk too, so `dz check` does not re-parse an unchanged header; the LSP, which matters most, is covered | `dz_cppimport`, `cache` |
| **M7b** | Left out of an import, with a reason in `Header::skipped`: variadic functions and function pointers; a `wchar_t` pointer; an rvalue reference `T&&`; a `static` member function (it needs associated functions, §7); a member whose type does not follow from a template instantiation's argument binding | `dz_cppimport`, `map_type` |
| **M7b** | **An overloaded free function imports only its first declaration** — `claim` takes the name and the rest are passed over. Overloaded *methods* do resolve by argument type (§15.3). The `@(...)` disambiguation suffix is `[PROPOSED]` and unparsed | `dz_cppimport`, `function` |
| **M7b** | An imported overloaded method is not cast at the boundary the way a free function is: `hir::Callee::Method` carries the name, not the overload key, so `dz_cpp` applies `c_cast`/`ret_cast` only where the name names one method | `dz_cpp/src/lib.rs`, `call` |
| **M7b** | Inheritance between imported C++ classes: a base's members are not imported into a derived class, and `&Derived` does not convert to `&Base`. A Dazzle struct inheriting a C++ class (Jolt's listeners) waits for struct inheritance | `dz_cppimport`, `collect_methods` |
| **M7b** | A whole C array member is refused rather than copied (`DZ0707`), and cannot be passed as a `[]T`. Copying one needs a helper that fills a `dz::Array` element by element; the slice needs a `Slice{ptr, len}` built at the use site. Indexing, `for`, `.len` and `.ptr` all work | `dz_check/src/check.rs`, `is_c_array_member` |
| **M7b** | libclang parses with its own default target (MSVC here), not the C++ compiler's: a header needing the C library's own headers may find MSVC's while g++ builds against mingw's | `dz_cppimport`, `arguments` |
| **M7b** | An untyped imported integer is not range-checked against its context: `let b: u8 = rl::SOME_BIG` reaches C++ as a narrowing `static_cast`. The importer knows the value; the checker should compare it, as it does a literal's | `dz_check/src/lib.rs`, `collect_foreign` |
| **M8** | **A generic union** reports `DZ0801`. Unions are M8's, and the tagged-union emission has no instance path yet | `dz_cpp/src/lib.rs`, `module` |
| **M8** | **Hoisting in initialiser position.** A `const`, a field default or a default argument whose value needs statements — an `or` clause, a block-valued `if` or `switch` — reports `DZ0801`, because each is emitted where C++ allows only an expression | `dz_cpp`, `const_decl`, `field_default`, `decl_params` |
| **M8** | `switch` on a subject that is neither an enum nor a union (an integer, a `str`) | `dz_cpp`, `switch` |
| **M8** | `for` with three or more bindings, i.e. destructuring a tuple `Item` (§10.4's `for btn, state in …` with an index) | `dz_cpp`, `for_loop` |
| **M10** | Macro declarations parse shallowly; bodies are kept as raw source and reproduced verbatim by the formatter | `dz_ast/src/lib.rs`, `dz_fmt/src/lib.rs` |
| **M10** | The intrinsics `#module`, `#location` and `#expr_text` are typed but not emitted (`#file`, `#line`, `#column` and `#function` are) | `dz_cpp`, `intrinsic` |
| **M11** | `typeinfo_of`, `typeid_of` and `typeinfo` are typed but not emitted | `dz_cpp`, `builtin` |
| **M12** | **Generic methods** — a method with type parameters of its own — report `DZ0801`. A generic *type*'s methods are instantiated; a method generic over its own arguments needs a second instance key and has no user yet | `dz_cpp/src/lib.rs`, `struct_decl` |
| **M12** | **`^Trait`**, and so a stored `^Allocator`: static dispatch only, per the M6 plan (§7.6, §18.6). Refused with `DZ0801` wherever one appears | `dz_cpp`, `reject_trait_objects` |
| **M12** | **Associated-type projection** (`T::Item`, `T::Output`). An associated type resolves through an implementer but cannot be named off a bounded parameter, so `for` over a generic `T: Iterate` is refused by the checker (`DZ0801`), and generic arithmetic cannot name its result type | `dz_check/src/check.rs`, `iterate_item` |
| **M12** | An open-ended range (`lo..`) held in a variable rather than written in a `for` head | `dz_cpp`, `expr` |
| **M12** | `assert` is typed but not emitted; it becomes an ordinary stdlib function | `dz_cpp`, `builtin` |
| **M12** | `alloc` panics on an alignment stricter than `malloc` guarantees, rather than honouring it | `dz_cpp/src/runtime.rs`, `alloc` |
| **M12** | A trait default method is exercised by `tests/run/generics` only for bodies that call the trait's own methods; one that builds or returns a `Self` value has no test | `dz_mono`, `inherited_defaults` |
| **M12** | A union variant whose payload owns a resource is refused rather than leaked. A C++ union with a non-trivial member needs special members that switch on the tag; §7.5 marks variant drops `[PROPOSED]`, so this waits for that decision | `dz_cpp/src/lib.rs`, `union_decl` |
| **M12** | `error(…)` and `format(…)` write into a static ring of four 256-byte buffers, so the fifth live message overwrites the first. §9.2 requires no allocation *and* a message that outlives the call; a real answer needs the stdlib's `String` | `dz_cpp/src/runtime.rs` |
| **M12** | `or unreachable` panics in release as well as debug. §9.4 says release may assume the success path, and §18.20 has not settled whether that means `__builtin_unreachable` or merely skipping the check | `dz_cpp/src/lib.rs`, `clause` |
| **M12** | **`str` has no `.len`.** §5.3 gives it `{ ptr, len }` and a slice answers `.len`, but member lookup knows only arrays and slices, so `name.len` on a `str` is `DZ0505`. Found while writing r21's tests | `dz_check/src/check.rs`, `field_type` |
| **M12** | **C++ keyword escaping covers locals only** (`sanitize`), so a *declaration* named `double`, `class` or `template` reaches C++ as itself and does not compile. Found while testing function values, with a Dazzle `fn double(…)` | `dz_cpp/src/lib.rs`, `sanitize` |
| **M12** | No stdlib. `println`/`print`/`format` are still builtins and stop being so here: they become ordinary stdlib functions over `Format`. The prelude declares the traits (§18.23) but nothing implements them — `String`, `Buffer`, `HashMap`, `Arena` and `Allocators::Global` are M12 — and `Formatter` is an empty struct until `String` exists to give it a buffer | `dz_cpp/src/runtime.rs`, `dz_project/src/prelude.dz` |

### Not deferred — deliberately out of scope forever

- Lifetimes. `&T` cannot be stored in a struct field (§5.2, §18.3) and that is the whole
  mechanism; a dangling `^T` is the programmer's problem (§1.1).
- Function overloading, until a later language version (§18.5).

---

## 4. Next: struct inheritance, then M7

What M9 still owes — semantic tokens, code actions, implicit-drop hints, incremental
analysis — is in §3. None of it blocks the language work, and inheritance will be the
first real test of the editor queries: a base's fields and methods must show up in hover,
definition and `.` completion of the derived type.

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
| `tests/run/*/` | `dz/tests/run.rs` | whole programs compiled by a real C++ compiler and executed, stdout compared against `expected.txt`. A first line of `#panic` marks a program that must abort. `features` covers M4's constructs plus array literals and `.Variant`, `ownership` §6's special members and drop ordering, `failure` §9's `or` clauses plus unions and tuples, `generics` §8's `Buffer<T>` for two element types, traits and default methods, a user `Iterate`, format specs and folded constants. `cinterop` covers §15's C mapping end to end and `cinterop_header` the single-header library both define-scoping bugs lived in; `cinterop_rgfw` (no `expected.txt`, so not run — it opens a window) is the manual RGFW probe. `run.rs` also pins that runaway instantiation is one `DZ0536` and that a trait object is refused |
| `tests/run/cppinterop` | " | §15's **C++** mapping against a vendored C++ library: a namespace and a nested one, a scoped enum, `const T&` and `T&` parameters, a class with a constructor and `const`/mutating methods, an overload set resolved by argument type (§15.3), a class C++ destroys itself (move-only, no Dazzle `drop`), and a `[cpp.instantiate]` template instance (§15.4) |
| `tests/run/raylib` | — (no `expected.txt`) | **M7b's criterion**: raylib 6.0 compiled from its own vendored source by `dz` and a window opened from Dazzle. Not run by the suite — it opens a window and waits, as `cinterop_rgfw` does. Check it with `dz build tests/run/raylib`, run it to see the window |
| `tests/run/cinterop*`, `tests/run/cppinterop` | `dz_cppimport/tests/import.rs` | the importer against those same headers, one test per §15.2 rule: functions/structs/enums/macros, handles, unions, array members and bit-fields, callbacks, namespaces and C++ paths, public members and overloads, the move-only rule, and that a template imports only where an instantiation is named |

| `tests/run/generics` | `dz_lsp/tests/ide.rs` | the editor queries against a real program: hover and definition for every kind of name, including beside a syntax error; references counted against every token of the name, so a missed use fails; rename and what it refuses; completion in scope, on a typed receiver (`largest(…).`), by name from an older analysis, after `::`, and silence in comments and strings; signature help; inlay hints; the outline; workspace symbols; formatting refusing broken source |
| `tests/run/*/` | `dz_driver/tests/tolerant.rs` | editor mode on about 1,200 damaged programs (a token deleted, the file cut off, a stray `.`, `(` or `{`): never a panic, never a later phase's errors while an earlier one failed, and diagnostics identical to build mode's |

`examples/*.dz` are **not** tests. They are the user's pre-spec design sketches and are not
expected to parse.
