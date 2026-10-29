---
id: T-0107
title: Design ties, kills clauses and compiler-placed memory
milestone: none
status: open
kind: design
area: ownership
---

The full design behind [D-0091](../decisions/D-0091-ties-checked-reference-invalidation-memory-placed.md),
as worked out with the user on 2026-10-27 after the regions prototype
([T-0104](T-0104-prototype-compiler-chooses-where-memory-comes.md),
[T-0105](T-0105-placement-follows-value-destination-not-function.md)). A draft: nothing here
is decided, nothing is in the spec, and the syntax is a sketch to be settled later.

**What it is.** Two things that depend on each other: a check that no reference is used
after the memory it points into has moved or been freed, and memory placed by the compiler
instead of by an allocator the program names.

The notation below is the one proposed under **Syntax** further down (`@` for a tie, `in`
to say where something points); nothing about it is settled.

*Ties.* A tie is a span during which some memory is valid. A type declares a tie by writing
`in @name` on a field. On a pointer field it names the memory the pointer owns; on any
other field it is a public name for that private field, so a signature can mention it
without breaking encapsulation (§4):

```dz
pub struct List<T> {
    m_data: own ^mut T in @items = null     // the array of elements: moves when it grows
    ...
}

pub struct String {
    m_bytes: List<u8> in @text
}
```

A tie is part of the type, not a generic parameter: every `List` has its own, and nobody
using the type picks it.

*Tie paths.* A clause or a tied return names memory by a path: a parameter, then public
fields (or fields visible wherever the function is), declared ties (`@items`), and `[*]` for
any element. A path that ends without a tie means every tie under it.

- `list@items` is the array: what `&list[i]` points into.
- `list[*]@text` is the text of any element of a `List<String>`: what `list[i].as_str()`
  points into. It is **not** under `list@items`, so `push`, which moves the array but not
  each string's bytes, leaves such a view alone.
- `list[*]` is everything every element owns, and `list` everything the list owns.

*References carry ties.* Every reference points into some tie, and the code that makes it
knows which: `&mut world.enemies[i]` is in `world.enemies@items`. A function returning a
reference says where it points: `pub fn first(&self) -> ^T in self@items`. With exactly one
reference input and no `in` written, the result is in that input as a whole, so any kill on it
ends the result. A reference parameter may declare where it points too, to be checked inside
the function: `e: &mut Enemy in world`.

*`kills` clauses.* Every function says in its signature which ties it may end:

```dz
pub fn push(&mut self, v: T) kills self@items
pub fn clear(&mut self) kills self@items, self[*]
pub fn len(&self) -> usize                        // no clause: kills nothing

pub fn spawn(world: &mut World, e: Enemy) kills world.spawned@items
fn think(e: &mut Enemy, world: &mut World) kills world.enemies@items
fn rebuild(world: &mut World) kills world          // coarse: anything reachable from it
```

- No clause means the function kills nothing, and **a clause says exactly what the body
  kills**, as the user required on 2026-10-27. The compiler works out what every body
  kills and holds the clause to it both ways: a kill left out is an error where it happens; a
  path the body never kills (`kills xs@items` over a body that only reads `xs`) and a path
  wider than what dies (`kills w` where only `w.enemies@items` does) are errors on the clause.
  Each comes with the clause the body needs, which `dz fix` writes, the editor offers as a
  quick fix, and shows while the body is being written. Where there is no body (a trait
  method with no default, an `extern fn`, a function type), the clause is a promise, the most
  an implementation may kill. The cost of exactness: a `pub` function cannot declare a kill
  it does not do yet, to keep room for a later version.
- A trait method's clause is the most any implementation may kill; a function type's
  likewise. A call with no clause to read (C or C++, an opaque pointer) is taken to kill
  everything reachable through its `&mut` arguments.
- Explicit everywhere, private functions included, by the user's choice: a reader, or an
  LLM, should see from one line what a function can invalidate. A body change can then
  never break a caller in another module, and each module is checked on its own (invariant 3).

*What kills what.*

- Writing, moving or dropping a place kills every tie under it, inline storage included:
  assigning a tagged union a different variant kills references into its old payload.
- `xs[i]` and `xs[j]` share one tie: a kill through either kills both, since `i` may equal `j`.
- `std` writes its containers' clauses by hand, over `unsafe` code. An element whose own
  memory survives (a string moved to a new array keeps its bytes) is a design point, not a
  checker rule: the checker kills everything under a place a *user* moves, since it cannot
  follow a view from `s` to `t` after `let t = move(s)`.

  | | `self@items` | `self[*]` |
  |---|---|---|
  | `push`, `insert`, `reserve` | kills | — |
  | `pop`, `remove`, `swap_remove` | kills | — (the element leaves; the others stay) |
  | `clear`, `truncate`, reassigning or dropping the container | kills | kills |
  | `sort_by`, `swap`, `reverse` | kills (elements move) | — |
  | `len`, `get`, `first`, indexing | — | — |

  `String::push_str` and `push` kill `self@text`. `HashMap` follows `List`, with `[*]` for
  its entries.
- A raw pointer a safe call returns (`HashMap::get` gives a `^V`) is tied like a reference.
  One made in `unsafe` by a cast or arithmetic is untied and unchecked, as today. So is a
  `^mut World` stored in a context struct (§5.2): the checker follows `ctx.world.enemies` as a
  path, but cannot know that `ctx.world` and a separate `&mut world` are one `World`.

*What is still allowed.* Aliasing: `&mut xs[i]` beside `&xs[j]`, a write through a `&mut`
that moves nothing (`p.x += 1`), a borrow of one field while another changes. A call is
rejected only when it can move memory that a live reference points into.

*Placement.* No allocator appears in ordinary code: no parameter, no `#allocator`, no
`Allocators::Temp`. The compiler puts each value in the most efficient place it can prove
safe, the heap last:

| The value | Its memory |
|---|---|
| never leaves its scope | that scope's region (T-0104 stage 1) |
| returned | wherever the caller wants the result (stage 2's hidden parameter) |
| the contents of a container cleared only all at once | an arena the container owns, reset when it is cleared: **cut from the first version** |
| anything else | the heap |

- A value's owned parts live where the value lives. A constructor and `copy` build into
  wherever `self` is placed, so a copy needs no allocator either (`String::copy_in` was the
  one-off version of this).
- Every function that returns something owning memory takes a hidden result placement:
  free functions, methods, trait methods and function values alike, so stage 2's exceptions
  go. A function called from C or C++ returns on the heap.
- Where a by-value parameter ends up (`fn add(&mut self, name: String)` storing it in
  `self`) is inferred within a module, since a wrong answer costs speed, never correctness.
  Across modules the default is the heap.
- Anything passing through code the compiler cannot see goes to the heap from the start,
  as do `Rc`, `Weak` and statics. A `Box`'s contents go where the `Box` goes.
- The `Allocator` trait, `ScopeArena` and `TempArena` stay as `std` internals, under `unsafe`.

**The scenarios** it was checked against on paper, with the verdict the rules give:

| Scenario | Verdict |
|---|---|
| `let n = counts.get_or_insert(k, 0)`; `counts.insert(o, 1)`; `n += 1` | ❌ `insert` kills `counts@items`, `n` points into it |
| `let s = name.as_str()`; `name.push_str("x")`; use `s` | ❌ `push_str` kills `name@text` |
| `for &mut e in enemies { enemies.push(…) }` | ❌ the loop's iterator is tied to `enemies@items` |
| ECS: `let p = &mut world.positions[i]` beside `let v = &world.velocities[i]`, `p.x += v.dx` | ✅ different ties, and the write kills nothing |
| ECS: `&mut positions[i]` beside `&positions[j]` in a collision loop | ✅ one tie, but nothing kills it (Rust rejects this) |
| ECS: hold `&world.positions[0]`, push to `world.positions`, use it | ❌ correctly; undefined behaviour in C++ |
| ECS: hold `&world.enemies[0]`, push to `world.spawned` | ✅ different ties |
| ECS: `think(&mut world.enemies[i], &mut world)`, where `think` pushes to `world.enemies` and then writes `e` | ❌ at the call: `e` points into what `think`'s clause kills |
| the same `think` with a local `boss` | ✅ `boss` is not in `world` |
| `think` declaring `e: &mut Enemy in world` | ❌ inside `think`, at the write after the push |
| ECS: a scratch `List` built and dropped in `update` every frame | ✅ the scope's region, from the per-thread pool, no `malloc` per frame |
| ECS: component lists that despawn with `swap_remove` | heap: one element at a time, so no arena; today's performance |
| a string popped from a region list and kept in a long-lived store (T-0105's hole) | heap: the region's tie does not reach the store, so that placement fails the check |
| `words`: keys copied into a `HashMap` local to `main` | the map is in `main`'s region and nothing takes a key out, so the copies go there too |
| a `String` returned through a trait method | the result placement, through the trait's calling convention |
| frame data in a private `self.scratch` cleared every frame | an owner arena, **not checkable yet**: nothing in the repository has such a container |

**Why it matters.** The first holes are the common memory bugs of game code, and Dazzle
neither prevents nor marks them. Rust prevents them, but by also refusing aliasing that is
harmless single-threaded, which is what makes ECS-style code painful there; this keeps the
aliasing and refuses only what can actually move memory. Taking allocators out of ordinary
code removes the other class of bug (a value outliving its arena) along with the choice that
is easy to get wrong (T-0099: `Temp` was slower than the heap for a growing container).

**Why it is this way.** The regions prototype placed memory without a checker, so each
stage needed its own safety argument, and each hole was found by a test rather than by a
rule: copies through borrows (stage 1), and elements popped from a region container (stage
3). With a checker, a wrong placement fails the check and falls back to the heap. The
user's own "ties" idea, single-threaded and per-field, supplied the checker. The design
moved through these steps: references carry ties (instead of assuming any `&mut T` might
alias any container of `T`), kill summaries are explicit (instead of inferred, which made a
body change ripple into other modules), and the placement intent mechanisms were dropped.

*What was cut, and why:*

- **Owner arenas, from the first version.** Every container in `examples/` and the
  benchmarks holds plain data (`Brick`, `Thing`, `Vertex`, `Object`, `Cell`, `u8`), so they
  have no contents to place, and the rule that decides when an arena is safe for memory (cleared
  only all at once, a private field or a local) cannot be tested here. They return when a
  real program has a container of owning values cleared each frame, measured against the heap.
- **Intent: a placement assertion (`#region`) and a backing policy per module (budgets, no
  heap).** The user's call: the policy is always the most efficient placement first. The cost is
  that a small edit can quietly move a hot value to the heap; inlay hints show it.
- **Threads**, deferred: until there is a design, nothing in a region may be sent to another thread.
- **The prototype's `STORES` and `TAKES` tables and `#allocator`**, which the checker and the
  result convention replace.

*Evidence so far:*

- About 13 of the 148 functions in `examples/` and the benchmarks would need a `kills`
  clause: a method changing its own list (`build_wall`, `hit_bricks`, box2d's `reset`,
  sdl3's `triangle`, `astar`'s queue) and the functions that call them. That is a text-based
  count, not an analysis.
- Std has five trait methods returning something owning memory, all text helpers returning
  `String` (`join`, `repeat`, `replace`, `to_ascii_lowercase`, `to_ascii_uppercase`), and the
  examples declare no traits. So the result convention through traits is cheap.
- From the prototype: scope regions and result placement made `astar` 12–17% faster than the
  heap. Stage 3 reached `words`' keys but gained 2%, with hand-placed `Temp` 6–7% ahead.

**Syntax, accepted by the user on 2026-10-27** as proposed here (step 1 below; needed under
D-0091's options A and B alike). It reaches the spec with D-0091's decision and its §21 row,
not before. Five choices, and the notation above uses them:

1. **The sigil for a tie: `@items`, not `'items`.** An apostrophe already starts a char
   literal (`'a'`, §2.5), so `'a` against `'a'` needs lookahead in a lexer that must never
   bail (invariant 1). Rust does it, but that is the second reason against it: an LLM that
   sees `'a` writes Rust lifetimes, which mean "lives as long as", where a tie means "points
   into". `@` is unused (§2.6), lexes on its own, and reads as "at".
2. **`in` says where something points**, in all three places: a field declaring a tie
   (`m_data: own ^mut T in @items = null`), a tied return (`-> &T in self@items`), and a
   parameter declaring one (`e: &mut Enemy in world`). `in` is already a keyword (`for x in`),
   and after a type it cannot be confused with a loop. The alternative, a prefix on the
   reference (`&@items T`, as Rust writes `&'a T`), cannot name the parameter the tie
   belongs to without a second sigil.
3. **`kills` becomes a reserved keyword** (§2.7), written after the return type and before
   the body: `fn load(p: &Path) !-> Mesh kills cache@entries { … }`. A keyword only in that
   position would avoid breaking an identifier named `kills`, but the language is
   pre-release, and a real keyword is easier for a reader and an LLM to spot.
4. **A clause may name only what is visible wherever the function is.** A private method can
   write `kills self.m_things@items`; a `pub` method on a private field declares a tie on
   the field (`m_things: List<Thing> in @things`) and writes `kills self@things`. A `pub`
   field is named by its path: `kills self.bricks@items`.
5. **Function types gain optional parameter names**, needed when their clause names one:
   `fn(world: &mut World) kills world`. Today a function type has types only (§20).

In the grammar sketch (§20), roughly:

```ebnf
fn_decl   = … "(" [ params ] ")" [ ( "->" | "!->" ) type [ "in" tie_path ] ]
            [ "kills" tie_path { "," tie_path } ] block ;
param     = … | ident ":" type [ "in" tie_path ] [ "=" expr ] ;
field     = … ident ":" type [ "in" "@" ident ] [ "=" expr ] … ;
tie_path  = ( ident | "self" ) { "." ident | "[" "*" "]" | "@" ident } ;
```

**What it would take.** The user decided D-0091 on 2026-10-27: option A, all of it (r81;
spec §6.7 and §14.6). The work happens on the branch `t-0104-regions-prototype`, by the
user's choice. In order, each step standing on its own:

1. Settle the syntax of ties, `kills` clauses and tied returns. A language change: a §21
   row, superseding D-0010 and D-0047, revising §1.1, §6.4 and §14.
2. The checker: references with ties, `kills` clauses checked against bodies, `std`'s clauses
   written by hand, diagnostics in terms of user code ("`push` kills `world.enemies@items`;
   `e` points into it and is used at line 14"), and `dz fix` for a missing clause. Per
   function, so the LSP stays fast. In pieces, each landing on its own:
   - **2a. Syntax only.** `kills` a keyword; the parser, the syntax tree and the formatter
     for `in @tie` on fields, `in path` on parameters and returns, `kills` clauses and named
     parameters in function types; the lex, parse and fmt corpora. Parsed and kept, with no
     meaning yet. Invariants 1, 6 and 10. **Done 2026-10-27** (`docs/log/r81-ties.md`).
   - **2b. Resolving tie paths**, in the checker since they go through `.` (invariant 9):
     each path's parameters, fields, declared ties and `[*]` exist and are visible wherever
     the function is (§6.7); the clause is part of the function's signature in the HIR; new
     codes for a bad path; `typed/` and `typeerrors/` cases (invariant 8). **Done
     2026-10-28** (`docs/log/r81-ties.md`): the signature keeps the clause as written, and
     `dz_check::ties::resolve` names what it means, for the checker and later `dz_flow`.
   - **2c. The analysis**, in `dz_flow`: the tie of every reference and view; kills at
     calls (the callee's clause read at its arguments), at assignment, move and drop; what
     each body kills in all, as the narrowest set of paths. Errors for a use after a kill, and
     three that hold a clause to its body: a kill it leaves out, a path the body never kills,
     a path wider than what dies. Each carries the clause the body needs, so 2e can offer it.
     Calls with no clause kill everything reachable through `&mut` and `^mut` arguments.
     **Started 2026-10-28** (`docs/log/r81-ties.md`): `dz_flow::ties` normalizes public
     ties and private field paths to the same places, substitutes a call's argument
     origins into its kill, return and parameter ties, keeps container storage separate
     from element-owned memory, and compares complete effect sets for missing, unused
     and broad clauses. `analyse_body` now follows local references and views through
     calls, assignment, move, scope exit, branches and loop backedges, preserves separate
     slot-address and element-owned ties, checks returned ties, and produces structured
     invalid-use and clause-mismatch findings. Unknown cases remain explicit and cannot
     produce clause-narrowing/removal suggestions. **2026-10-29:** run over all 881 bodies
     of std and `examples/`, it had false uses after a kill in 62 and now has none (std is
     held to that by a test); writing plain data kills nothing (r82, the user's call), and
     payload references through `case X(&c)` and `if let &v` are followed. **Still open:**
     aggregate view storage and destructuring (an optional, struct or tuple holding a view
     is unknown), general iterators, private pointer indexing to `[*]`, how a path names
     one payload, default-argument effects, function-type clauses, and diagnostic/fix
     integration. Nothing new is enforced yet.
   - **2d. `std`'s clauses** on `List`, `HashMap`, `HashSet`, `SlotMap` and `String`, with
     `std` itself checking clean.
   - **2e. The fixes and the editor.** `dz fix` writes the clause a body needs, adding,
     removing or narrowing paths; the editor offers the same as a quick fix on each of 2c's
     errors, shows the clause a body needs while it is being written, and the clause in a
     call's hover. `dz explain` for every new code. Then the examples and corpora get their clauses (about 13
     functions in `examples/`), in the same change that makes a missing one an error, so
     nothing in the repository stops compiling.
3. A debug-build check of `std`'s trusted clauses: generation counters on arena memory, so a
   wrong declaration fails a test instead of shipping.
4. Placement on top of the checker: stages 1 and 2 re-based on ties, the result convention
   for every function, constructors and `copy` building into `self`'s placement.
5. Skipping drops that only free memory (`String`, not `File`) when everything a value owns
   is in a region being released.
6. Removing allocators from the language: std's parameters, the examples, the benchmarks.
   Measured against hand-placed `Temp` as in T-0104.
7. Owner arenas, once a program needs them.

Still open, to settle on the way: how 2c ties `self[*]` to the memory behind it inside the
type that declares it (2b settled which types have `[*]` — those with `Index` or `Iterable`,
§6.7 — but in `List`'s own body the elements are `m_data[i]`, behind a pointer tied
`@items`, which the analysis has to read as `self[*]`); the clause of a function type,
which 2b leaves unresolved and outside `Ty::Fn`, so storing a function in one does not yet
compare their clauses; the elision rules for returned references (whether a pointer, slice
or `str` input counts as one: until settled, one beside another leaves the result unknown),
how a tagged union's payload is named in a tie, what `dz` shows for each placement (inlay
hints and a listing), and whether stage 3 on the prototype branch is reverted now or left
until step 4.
