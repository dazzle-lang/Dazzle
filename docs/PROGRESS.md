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
| Milestones done | M0, M1, M2, M3, M4, **M5 — programs own, move and drop their resources** |
| Next | **M6 — generics, traits, monomorphization** |
| Tests | 88, all green |
| Clippy | clean (`--workspace --all-targets`) |
| Compiler crates | 11 |
| Language spec | 0.1 **unreleased**, revision 7; four open questions (§18.2) |
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
| `dz_project` | `dazzle.toml`, globs, dependency graph | §3.1 |
| `dz_resolve` | symbol tables, name resolution, visibility | §3, §4 |
| `dz_check` | type checker | §5, §7, §9, §10, §11 |
| `dz_flow` | ownership, moves, definite return | §6 |
| `dz_cpp` | C++17 emission + the runtime shim | §16 |

Crates still to come are listed in spec §17.1: `dz_mono`, `dz_cppimport`, `dz_macro`,
`dz_lsp`.

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

---

## 3. Deferral register

Everything knowingly postponed. Each row has a comment in the code naming the same
milestone. **This is the list to check before claiming a milestone is complete.**

| Owed by | What is deferred | Where |
|---|---|---|
| **M6** | A slice parameter is immutable through an immutable binding, so `&mut xs[i]` on an `xs: []i32` parameter is rejected. A slice is a pointer and a length, and indexing one arguably ought to be mutable the way `^T` is — but that is a language question (§5.3) for you, not a bug to fix quietly | `dz_check/src/check.rs`, `place_mutability` |
| **M6** | A range that escapes a `for` loop reports `DZ0801`; `dz::Range<T>` exists in the shim but nothing produces one | `dz_cpp`, `expr` |
| **M6** | `break` with a value reports `DZ0801` | `dz_cpp`, `expr` |
| **M6** | Generic bodies are skipped, not checked; generic items are not emitted | `dz_check/src/check.rs`, `dz_cpp` |
| **M6** | Trait conformance: `struct Arena : Allocator` resolves the name but nothing verifies the methods | `dz_check/src/lib.rs` |
| **M6** | **§12.9 format-string checking.** Builtin calls are variadic and untyped: the first argument is not required to be a string literal, `{}` count is not compared against the argument count, `{:spec}` is not parsed, and an argument whose type cannot be formatted is not rejected — so `println("{}", some_struct)` reaches C++ and fails there as `DZ0803` (an invariant-7 violation). The check wants the `Format` trait to say what "formattable" means, so it waits for traits | `dz_check/src/check.rs`, `builtin_call_type` |
| **M6** | `for` over user types needs `Iterate` | `dz_check/src/check.rs` |
| **M6** | Constant folding; array lengths accept only literals; `const` without an annotation infers from literals only | `dz_check/src/lib.rs` |
| **M6** | `sizeof`/`alignof` and the reflection builtins are not emitted | `dz_cpp`, reports `DZ0801` |
| **M7** | `import cpp "…"` is accepted and the alias is opaque; no header is read | `dz_resolve/src/walk.rs` |
| **M10** | Macro declarations parse shallowly; bodies are kept as raw source and reproduced verbatim by the formatter | `dz_ast/src/lib.rs`, `dz_fmt/src/lib.rs` |
| **M12** | A union variant whose payload owns a resource is refused rather than leaked. A C++ union with a non-trivial member needs special members that switch on the tag; §7.5 marks variant drops `[PROPOSED]`, so this waits for that decision | `dz_cpp/src/lib.rs`, `union_decl` |
| **M12** | `error(…)` and `format(…)` write into a static ring of four 256-byte buffers, so the fifth live message overwrites the first. §9.2 requires no allocation *and* a message that outlives the call; a real answer needs the stdlib's `String` | `dz_cpp/src/runtime.rs` |
| **M12** | `or unreachable` panics in release as well as debug. §9.4 says release may assume the success path, and §18.20 has not settled whether that means `__builtin_unreachable` or merely skipping the check | `dz_cpp/src/lib.rs`, `clause` |
| **M12** | No stdlib. `dz_prelude.h` stands in for §12.9's `Format` trait so `println` works at all. `println`/`print`/`format` are temporary builtins and stop being builtins here: they become ordinary stdlib functions over `Format`. The operator traits of §11.2 (`Ord`, `Add`, …), `Iterate`, `Allocator`, `String`, `Buffer` do not exist, so naming one is correctly an unresolved-name error | `dz_cpp/src/runtime.rs` |

### Not deferred — deliberately out of scope forever

- Lifetimes. `&T` cannot be stored in a struct field (§5.2, §18.3) and that is the whole
  mechanism; a dangling `^T` is the programmer's problem (§1.1).
- Function overloading, until a later language version (§18.5).

---

## 4. Next: M6 — generics, traits, monomorphization

Ends when `Buffer<T>`, `Allocator` and `Arena` work as written in spec §6 and §8 — which is
also what M5's stated criterion was waiting for, since `Buffer<T>` and `String` are generic.
Nothing about them is blocked on ownership any more.

Planned order:

1. **Definition-site checking** (§8.1). A generic body is checked **once**, against its
   bounds, not per instantiation. This is the whole point of §8.6 and the reason error
   messages can point at user code — get it wrong and every later diagnostic names a
   generated instantiation instead.
2. **Traits** (§7.6): declaration, conformance checking (`struct Arena : Allocator` today
   resolves the name and verifies nothing), associated types (§8.5), and `^Trait` as the
   fat pointer §18.6 settled on.
3. **`dz_mono`**: instantiate generic items, fold compile-time branches (§8.3, §8.4). Names
   are mangled Dazzle-side so the C++ stays readable and `#line` keeps working.
4. **The operator and iteration traits** the checker currently hard-codes: `Ord`/`Add`/…
   (§11.2) and `Iterate` (§10.4), which is what lets `for` run over a user type.
5. **§12.9 format-string checking**, which has been waiting for `Format` to exist — see the
   deferral register. Until then `println("{}", some_struct)` fails in C++ rather than in
   Dazzle, which is the one standing violation of invariant 7.
6. **Constant folding** (§8.3), which array lengths and un-annotated `const` both need.

Two things to settle early, because they shape everything after:

- **How `Iterate::next` spells one signature for both item kinds** (§18.21). The decision is
  made — a pointer iterator returns `^T`, a value iterator `Item?` — but the trait
  declaration still has to say so in one signature.
- **Whether a generic parameter is a real `Ty` or a substitution.** Today an unresolved type
  name inside a generic body becomes `Ty::Error`, and `Ty::Error` is compatible with
  everything, so generic bodies are skipped rather than checked. M5 found three real bugs
  hiding behind exactly that behaviour; a `Ty::Param` that is compatible with *nothing* it
  has not been proved compatible with is the shape that avoids repeating it.

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
| `tests/run/*/` | `dz/tests/run.rs` | whole programs compiled by a real C++ compiler and executed, stdout compared against `expected.txt`. A first line of `#panic` marks a program that must abort. `features` covers M4's constructs, `ownership` §6's special members and drop ordering, `failure` §9's `or` clauses plus unions and tuples |

`examples/*.dz` are **not** tests. They are the user's pre-spec design sketches and are not
expected to parse.
