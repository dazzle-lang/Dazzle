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
| Milestones done | M0, M1, M2, M3, M4, M5, **M6 — generics, traits, monomorphization** |
| Next | **Struct inheritance (spec 18.32)**, then **M7 — C++ interop via libclang.** See §4 |
| Tests | 94, all green |
| Clippy | clean (`--workspace --all-targets`) |
| Compiler crates | 13, plus the `dz` CLI |
| Language spec | 0.1 **unreleased**, revision 13; four open questions (§18.2), one of them inheritance, which is designed and next |
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

Crates still to come are listed in spec §17.1: `dz_cppimport`, `dz_macro`, `dz_lsp`.

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

---

## 3. Deferral register

Everything knowingly postponed. Each row has a comment in the code naming the same
milestone. **This is the list to check before claiming a milestone is complete.**

| Owed by | What is deferred | Where |
|---|---|---|
| **next** | **Struct inheritance.** `struct Button : UiElement` parses, and the checker reports it with `DZ0801`; the design is settled as spec 18.32 | `dz_check/src/check.rs`, `item` |
| **M7** | `import cpp "…"` is accepted and the alias is opaque; no header is read | `dz_resolve/src/walk.rs` |
| **M7** | **No read-only pointer.** `.ptr` refuses a `[]T`, and `&T` cannot widen, because the only pointer is the writable `^T`. A `^const T` belongs with the `const T*` mapping M7 has to decide (spec §15.2) | `dz_check/src/check.rs`, `Field` |
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
| **M12** | No stdlib. `println`/`print`/`format` are still builtins and stop being so here: they become ordinary stdlib functions over `Format`. The prelude declares the traits (§18.23) but nothing implements them — `String`, `Buffer`, `HashMap`, `Arena` and `Allocators::Global` are M12 — and `Formatter` is an empty struct until `String` exists to give it a buffer | `dz_cpp/src/runtime.rs`, `dz_project/src/prelude.dz` |

### Not deferred — deliberately out of scope forever

- Lifetimes. `&T` cannot be stored in a struct field (§5.2, §18.3) and that is the whole
  mechanism; a dangling `^T` is the programmer's problem (§1.1).
- Function overloading, until a later language version (§18.5).

---

## 4. Next: struct inheritance, then M7

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

### M7 — C++ interop via libclang

Ends when **a raylib window opens from Dazzle** (spec §19). This is the milestone that
turns Dazzle from a toy that prints into something that can draw, and it is the first of
0.1's three release conditions (the others are M9's LSP and M12's stdlib).

What exists: `import cpp "…"` parses, resolves to an opaque alias, and reads nothing
(`dz_resolve/src/walk.rs`). Everything else is new.

1. **Read spec §15 first, all of it** — importing (§15.1), the mapping table (§15.2),
   overloads (§15.3), templates (§15.4) and exporting (§15.5). Anything it leaves open is a
   question for the user before code, as the four were before M6.
2. **`dz_cppimport`** (spec §17.1): a libclang reader that turns a header into Dazzle-side
   declarations the resolver and checker can treat like any module's. The binding is a
   choice to make deliberately — `clang-sys` needs `libclang.dll` at run time on Windows,
   which is the same environment problem ASan already has (§1).
3. **Map the subset raylib needs, and nothing more**: C functions, plain structs, enums,
   `#define` integer constants, pointers. raylib is C, so overloads and templates can wait
   for a C++ header that needs them.
4. **Emission and linking.** The generated `.cpp` includes the header, and `dazzle.toml`
   needs somewhere to say which library to link. `dz run` must still work on both
   compilers.

**Decide during M7:** whether there is a `^const T`. raylib's `const Vector2*` parameters
are the real cases to judge it against, and it is what `.ptr` on a `[]T` is waiting for.
`impl` blocks are what an imported type gets traits through — raylib's `Color` can only
ever be `Inspect` that way — so M7 is also their first real test.

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
| `tests/flow/*.dz` | `dz_flow/tests/flow.rs` | one file per ownership rule, each naming the code it must produce in an `// expect:` line, plus `clean.dz` which must produce nothing. The count is asserted too, so a rule that fires twice for one mistake is a regression |
| `tests/run/*/` | `dz/tests/run.rs` | whole programs compiled by a real C++ compiler and executed, stdout compared against `expected.txt`. A first line of `#panic` marks a program that must abort. `features` covers M4's constructs plus array literals and `.Variant`, `ownership` §6's special members and drop ordering, `failure` §9's `or` clauses plus unions and tuples, `generics` §8's `Buffer<T>` for two element types, traits and default methods, a user `Iterate`, format specs and folded constants. `run.rs` also pins that runaway instantiation is one `DZ0536` and that a trait object is refused |

`examples/*.dz` are **not** tests. They are the user's pre-spec design sketches and are not
expected to parse.
