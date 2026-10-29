# r81: ties, `kills` clauses and compiler-placed memory

The language change is spec §6.7 and §14.6, decided as D-0091; the plan, in order, is
T-0107. How it came about: the regions prototype (T-0104, T-0105, on the branch
`t-0104-regions-prototype`) tried to choose allocators without a checker, and each stage's
safety rested on rules a test had found. The user's own "ties" idea supplied the checker, and
D-0091 was decided on 2026-10-27.

## 2e, first part: the findings are errors, and `dz fix` writes clauses (2026-10-29)

Compiler work: §6.7 already said all of this is an error. `dz_flow::check` now runs
`ties::analyse_body` on every body, and `ties::report` (`dz_flow/src/ties/report.rs`)
turns what it finds into diagnostics:

- **`DZ0413`**, a use after a kill: the use, labelled with the tie it points into, and the
  call, write, move or drop that killed it. A `return` of a view of a local or a by-value
  parameter is the same code with its own message ("dies as the function returns").
- **`DZ0414`**, a returned view outside the signature's `in`. Both sides are now compared
  spelt as their ties, so `self.m_books[*]` is `self[*]` when the type says so.
- **`DZ0415`–`DZ0417`**, a kill the clause leaves out (at the kill), a path never killed,
  and a path wider than what dies (at the path). Each carries the clause the body needs.
- **The clause the body needs** is what it kills that a caller sees, without what names
  no memory, rounded up to the ties types declare and spelt as a tie path, `@items` rather
  than `.m_data` (`tie_path`). A field a cast reached that its type does not have ends the
  path, which then names everything under what holds it: `kills data` for a `^mut void`
  read as a struct (and `ties` compares it that way, a `void` pointee named whole).
- **The fix** replaces the clause, through its new span (`FnDecl::kills_span`, kept as
  `FnSig::kills_span`), or inserts one before the body's `{`. It is certain, so `dz fix`
  applies it, except when a path goes through a field the function's callers cannot see
  (checked with `ties::resolve`): then it is only offered, and the help says to declare a
  tie. A function a macro wrote gets no fix, only a note that its clause belongs in the
  macro, since every token of it is at the invocation.

The repository got its clauses in the same change, mostly by the fixes themselves:
`box2d` (with `tie @things = m_things@items` on `Demo`, whose `pub` methods could not name
the private list), `breakout`, `sdl3`'s mesh (by hand: `dz check` needs Ninja for its
CMake recipe, which this machine lacks), the run corpus (`button`, `generic_base`,
`machine`'s macro template, `reflect`, `regions`, `union_methods`), `typed/`, and the
`astar` benchmark (with `@items` and `@values` on its heap). `typed/` had five functions
returning a view of a by-value parameter (`fn heaviest<T: Weigh>(a: T, b: T) -> str {
return a.describe() }`); they return a `String` now, and the language gap behind it, a
view that points into nothing, is open in T-0107. Explain examples whose clauses named a
whole list where `push` moves only its array now say `@items`.

Found on the way: `dz fix`'s `DZ0406` rename breaks a variable that is assigned but never
read (T-0108), so the corpora were fixed with the clause fixes alone.

Tests: `tests/flow/` gains one file per code (`use_after_kill`, `return_outside_tie`,
`kill_not_declared`, `kill_never_happens`, `kill_too_broad`), each with its valid twin;
every code has its `dz explain` entry in `flow.md`.

## r86: a function type's clause is checked (2026-10-29)

2b parsed a function type's clause and left it out of `Ty::Fn`, so a call through a function
value fell back to the opaque rule and left its body's summary incomplete, and storing a
function never compared clauses.

- **The type.** `Ty::Fn` gains `kills: FnKills` (`dz_hir/src/ty.rs`): `Known` keeps the
  paths as written and the parameters' names, like a signature's clause, since the paths
  need every type's fields; `Unknown` is a pointer from C. It is not part of the type's
  identity (its `PartialEq` is always true), so C++ emission, instances and inference are
  untouched; `assignable` is where it counts, through `FnKills::within`: each path of the
  value's clause is one of the type's, or under one, matched by parameter position. A
  written type gets its clause in `lower_type_inner`, a function item's value its own
  signature's, a C function pointer `Unknown`, which fits only a type with no `&mut` or
  `^mut` parameter.
- **Errors.** `expect` reports a function that would fit but for its kills as `DZ0568`,
  naming both clauses. `ties::check_fn_types` resolves a function type's paths through a
  signature `fn_type_sig` builds from the type, so a path naming no parameter is `DZ0562`
  and a kill through `&` is `DZ0564`; it runs over every signature's parameters and
  result, and every field.
- **Flow.** A call through a function value builds the same `fn_type_sig` and goes through
  `invoke`, as a direct call does: its kills bind to the arguments, its result elides as
  a signature's would, and a C pointer's `kills_unknown` gives the opaque rule. Bodies
  calling a function value are now exact: `SlotMap::retain` and `HashMap::retain_with`.

Nothing in std or the examples stored a function that kills into a type that says
otherwise; all of it checks.

Tests: `typed/` stores a function that kills `r@scores` and one that kills nothing into a
type promising `r@scores`; `typeerrors/` has the three mistakes. `dz_flow/tests/ties.rs`:
a call through a value of a type that promises a kill ends a view, one through a type
promising nothing does not, and the containers test holds `SlotMap::retain` exact too.

## r85: taking an element out kills `[*]` (2026-10-29)

The user's call on 2d's open question, once it was clear the issue is a view into the
element taken out, which outlives nothing the checker can see. `List::pop`, `remove`,
`swap_remove`; `HashMap::insert`, `replace`, `remove` and `remove_slot`; `HashSet::insert`
and `remove`; `SlotMap::remove`; and `StrKeys` and `StrSet`'s `remove_str` (whose trait
promise is now `kills self`) kill `self@items, self[*]`. Every std body still matches its
clause. For a container of plain data `[*]` names nothing, so its callers' clauses are
unchanged; a `HashMap`'s `[*]` is named whole, so a caller inserting into one names
`m[*]` now, as the `counting` scenario does.

`List`'s bodies over `m_data` stay trusted: reading `pop` exactly now agrees with its
clause, but `swap`, the shifts and growing move elements within the array, which the
analysis reads as killing `[*]`.

Tests: `dz_flow/tests/ties.rs`, 38: a view into an element's text ended by `swap_remove`
(the value dropped in a block), by `pop`, and by a map's `insert` over an existing key,
and a list of `i32` whose `pop` needs no `[*]` in its caller's clause.

## r84: a struct declares its ties itself (2026-10-29)

The user's preference, settling the first open point of T-0107 and 2d's block on containers
built from containers: rather than reaching into a field's ties, a struct exports its own.
`tie @items = m_data` and `tie [*] = m_data[*]` are members; `in @name` on a field is gone.
The syntax is provisional by the user's choice; what mattered was that it works.

- **Front end.** `tie` is a keyword (lexer, completion, the VS Code grammar). A struct
  member `MemberKind::Tie(TieDecl)` holds `@name` or `[*]` and a list of tie paths,
  parsed by `parse_kills`, so a parenthesised list is accepted too. The formatter writes
  `tie @x = a, b`. `in @x` after a field's type is read past and reported once as
  `DZ0219`, with the `tie` to write. An `impl` holding a `tie` is `DZ0533`, as a field there is.
- **Checker.** `StructSig::ties` keeps the declarations as written; `ties::export_path`
  resolves one from the struct's own type, where its private fields are visible and no
  visibility rank applies, and `ties::check_struct` reports a tie declared twice
  (`DZ0567`), `tie [*]` on a type with no elements, a path from `self`, and a path through
  another type's private field (`DZ0563`, telling that type to declare a tie). A tie
  standing for whole fields still fills `FieldSig::tie`, so everything that read r83's
  tied fields reads the same. Resolution was split from the function signature: a
  `Walker` knows who names the path and what they may see. `[*]` on a pointer field names
  the array behind it. A base's ties are its derived types'.
- **Flow.** A tie normalizes to its first path's canonical place. `canonicalize`
  rewrites every other path a tie lists to that one, and every path `[*]` lists to `[*]`;
  normalized clauses, the ties compared for exactness, and places the body builds from
  fields and pointer indexing all go through it. So inside `List`, `&m_data[i]` has its
  element at `self[*]` and its address in `self@items`; growing ends the address and
  leaves what the element owns. Inside `SlotMap`, a kill of `m_slot_of`'s array is a kill
  of `self@items`.
- **Still trusted.** A body indexing a tied pointer's elements is still not held to its
  clause, as 2d decided. Its uses are followed now; its summary is not compared. Reading
  them exactly, the analysis says `pop`, `swap` and the shifts kill `self[*]` (they move
  elements), which is the open question of taking an element out.
- **std.** `List`, `String`, `HashMap` and `Formatter` declare the ties they had.
  `HashSet` (`@items = m_map@items`, `[*] = m_map[*]`) and `SlotMap` (`@items` over its
  three lists, `[*] = m_values[*]`) gain ties and clauses: `insert`, `remove`, `reserve`
  kill `self@items`; `clear` and `retain` kill `self@items, self[*]`; `SlotMap::get`,
  `get_mut` and `values` are `in self@items`. Every body is exact but `SlotMap::retain`,
  which calls a function value.

Not adopted: making a declared tie opaque, so no path goes past it. r83's rule stands.

Tests: the parse, fmt and error corpora have the new member, its mistakes (`DZ0219`, a
missing `@`, a missing `=`) and the layout; `typed/` has a wrapper whose ties stand for a
list's and a type with `tie [*]` over a pointer, `typeerrors/` one case per new mistake;
`dz_flow/tests/ties.rs` has 37, new: `HashSet` and `SlotMap` held to their clauses, a view
from `SlotMap::get` ended by `insert`, and inside a type, a pointer element's address ended
by growth while what it owns survives growth and dies with `[*]`. `dz explain` has DZ0219
and DZ0567.

## 2d, first part: `List`, `String` and `HashMap` declare their ties (2026-10-29)

std's first clauses, following §6.7's table. Nothing is enforced yet, so a missing clause in
a caller is still only a finding.

- **`List`**: `m_data: own ^mut T in @items`. Growing, inserting, taking out and moving
  elements (`push`, `insert`, `pop`, `remove`, `swap_remove`, `swap`, `reverse`,
  `sort_by`, `reserve`) kill `self@items`; dropping elements (`clear`, `truncate`,
  `remove_range`, `retain`, `resize`) kills `self@items, self[*]`; `append` kills
  `self@items, other@items`, since the elements leave `other` with their own memory.
  Indexing, `first`, `last` and the slices are `in self@items`. `Sort::sort` promises
  `kills self` and `List`'s implementation kills `self@items`.
- **`String`**: `m_bytes: List<u8> in @text`; every change kills `self@text`, and `as_str`,
  `as_cstr` and the `cstr` conversion are `in self@text`.
- **`HashMap`**: all three arrays are `in @items` (r83), with `List`'s pattern; `StrKeys`
  promises `in self` and `kills self`, its implementation names `@items`.
- **`Formatter`**: `m_width` and `m_precision` are `in @spec`, which `set_spec`,
  `begin_arg` and `finish` kill: a `u32?` changing variant is a payload change (r82).
- **Trusted bodies.** §6.7 says std's clauses are written by hand over `unsafe` code and
  trusted, and its own table makes that necessary: `pop` kills only `self@items`, while the
  checker, which cannot follow a view from a place to where it moved, kills everything
  under a place moved out. A body indexing a tied pointer (`m_data[i]`) stays incomplete,
  so its clause is not compared with it; step 3's debug checks are its test. Bodies built
  on those types (`String` over `List`, `clear` over `truncate`) are compared exactly.
- **A pointer made by a cast is unchecked** (§6.7), now as the spec says it: any explicit
  `as` to a pointer, which is always `unsafe`, rather than only one to another pointee.
  `HashMap::grow_to` keeps `m_hashes as ^mut u64` while it moves the entries out, and the
  write that replaced `m_hashes` had made that read look like a use after a kill. HIR's
  `Cast` gains `explicit`, so the checker's own `mut`-dropping conversions keep their tie.

**Clauses compare by the ties they name** (§6.7: a path that ends without a tie means every
tie under it). `push_str kills self@text` was "wider than what dies" against a body that
kills `self@text@items`, though a `List<u8>` owns nothing else. `dz_flow::ties::ties`
expands a place to its ties: the memory behind tied pointers, payloads, the elements of a
type with `[*]` (behind a pointer they are not among the fields), an `Iterable`'s entries
when they are references into it (a `String`'s `char`s are not), what a type parameter
might own, and a value whose `drop` frees an untied `own` pointer, named whole.
`compare_ties` compares those, so `kills n` equals `kills n.text` when `n`'s other fields
are plain, and a clause naming only plain data names nothing (unused). `Drop` bodies are no
longer held to a clause: the value is dying, and `Drop::drop` declares none.

After this, across std and the examples: no use after a kill. The examples' mismatches are
the clauses 2e writes (breakout's wall and bricks, box2d's `reset` and `spawn`, sdl3's mesh).
std's are `HashSet`, `StrSet` and `SlotMap`, which are blocked; see below.

**Open, for the user:**
- *A container built from containers.* `SlotMap` is three `List`s and `HashSet` a
  `HashMap`. §6.7 gives them `self@items` and `self[*]` like `List`, but their array is a
  field's `@items` and their elements a field's `[*]`, and no declaration says so: tying
  the field gives a tie for the whole field, which is wider than `@items` and fails the
  exact clause. Either a type declares its ties as paths into its fields, or such
  containers name their fields' ties (`kills self@values@items`), or their bodies are
  trusted too.
- *Taking an element out.* `pop`, `remove`, `swap_remove` and `HashMap::replace` kill only
  `@items` (§6.7's table), so a view into the element's own memory survives. That is right
  while the taken value lives, but nothing ends the view when the caller drops it: `let s =
  xs[0].as_str(); let x = xs.swap_remove(0); drop(x)`, then `s` is freed text. Killing
  `self[*]` as well closes it, at the cost of every other element's views.

Tests: `dz_flow/tests/ties.rs` has 34. New: §6.7's scenarios against std's real clauses
(`get_or_insert` then `insert`, `as_str` then `push_str`, a slot or an element's text across
`push` and `clear`, two lists apart), and clauses compared over the ties they name.

## r83: fields tied together (2026-10-29)

`HashMap` keeps hashes, keys and values in three arrays that grow as one, and 2b's
`DZ0566` allowed a tie on only one field, so it could not have its `@items`. Asked how std's
container bodies should meet their clauses, the user answered that fields can be tied
together. Nothing in §6.7 or D-0091 had said either way; `DZ0566` was 2b's own choice.

- **The rule** (§6.7, r83): several fields may declare one tie, a path naming it names all
  of them, and they live and die together. `DZ0566` is retired, not reused.
- **Resolving** (`dz_check::ties`): a tie now resolves to every field declaring it. A path
  stops there if any of them is a pointer, as one pointer tie already did, and also if their
  types differ, since there is no one type to go on into (`DZ0562`).
- **The analysis** (`dz_flow::ties`): every field tied together stands for the first one
  declaring the tie, in paths and in a body's places alike (`canonical_field`), so a kill
  through `keys` is a kill of the tie and ends a view into `values`.

Tests: `typed/` gains two lists tied together, cleared through both and walked through one
past the tie; `typeerrors/`' duplicate tie becomes a path going on past fields of different
types; `dz_flow/tests/ties.rs` has 32, with a shared tie killed through either field and an
unrelated tie left alone.

## 2c, fourth part: which gaps can hide a kill (2026-10-29)

After the third part, 337 of the 881 bodies in std and `examples/` were still incomplete,
and an incomplete body's clause is never checked. Most of those reasons could not hide a
kill, so they were keeping clauses unchecked for no gain. Now **848 are complete**, still
with no use after a kill and the one mismatch, `set_spec`. Nothing is enforced yet.

- **Two lists instead of one.** `incomplete` now means only that a kill may have been
  missed or widened, so the summary is not exact and the clause is not compared with it.
  `unchecked` is where a view could not be followed, so a use or a return went unchecked:
  views in tuples, optionals and structs, returns of unknown provenance (an `unsafe` cast,
  a `str` field), multi-input elision. A lost view cannot hide a kill, because every way a
  kill reaches unknown memory has its own `incomplete` reason: a call whose argument's
  origin is unknown, a write, `destroy` or `move` through unknown storage (the last one new).
- **Initialisers** (`const`, `static`, field defaults) have no parameters and no clause, so
  they are walked but never incomplete: 193 bodies.
- **Indirect calls.** A function type with no `&mut` or `^mut` parameter can kill nothing,
  whatever its clause, since a clause names only those (DZ0564); 81 of the 95 such calls.
  For the rest, the opaque rule now kills what each argument refers to rather than also the
  array an element is in.
- **Default arguments** are the callee's own expressions and cannot name the caller's
  places, so they kill nothing at the call. Their provenance is unknown, which matters only
  where a clause or tie names that parameter.
- **Statics** are reached only in `unsafe` and no clause can name one, so they are
  unchecked, as a pointer made there is.
- **Found on the way, a real hole:** a method called through a pointer field
  (`unsafe { c.p.add() }`) recorded its kill on the local `c`, so it was dropped from the
  summary as a local's, and the body looked complete. A pointer receiver now stands for
  its pointee, here unknown, which makes the body incomplete.

The 33 still incomplete: `destroy` through std's untied pointers (27, which 2d's ties and
the `self[*]` mapping address), calls with arguments of unknown provenance (14), indirect
calls through `&mut` parameters, which need function types to carry clauses (14), and one
assignment through unknown storage.

Tests: `dz_flow/tests/ties.rs` has 31: read-only indirect calls with and without a clause,
initialisers and omitted defaults, the pointer-field receiver, and a view lost in a tuple
leaving its use unchecked while the clause is still held to the body.

## 2c, third part: real code, and what a write kills (2026-10-29)

The analysis was run over every body in std and `examples/` (881), with a scratch probe
tallying findings and the reasons a body was left incomplete. It reported a use after a kill
in **62 bodies, every one wrong**, and clause mismatches in 20. Nothing is enforced yet, so
none of this reached a user; it would have made 2e unshippable.

- **A result returned by value is not a view.** `return m` held the value across the
  scope's drops and read it afterwards, so dropping `m` made its own return "a use after a
  kill". Only the result's views are held now: 57 of the 62.
- **An `own` pointer denotes its memory.** Indexing one (`m_data[i]`) took the field as an
  inline array, so `move_out(m_data[i])` killed `self.m_data[*]`, and `take`'s result,
  elided to `self`, died with it. An untied `own` pointer is now unchecked, as §6.7 says of
  a pointer stored in a struct.
- **Elision does not choose between a reference and a pointer.** §6.7 elides with exactly
  one reference input; `contents_allocator(&self, fallback: ^mut Allocator)` was elided to
  `self` and then faulted for returning `fallback`. Whether a pointer, slice or `str` input
  counts is not settled, so one beside another input now leaves the result unknown (the
  body incomplete) rather than picking one: still T-0107's open question on elision.
- **A cast to another pointee is unchecked** (§6.7): `p as ^mut T` from a `^mut u8` was
  following `p`. Dropping `mut`, or reading a `str` as its bytes, still keeps the tie.
- **A struct, tuple or optional holding a view, returned from a call,** was silently taken
  to point nowhere; it is unknown now. So is a view taken out of an optional by `or` or
  `if let v`, until optionals carry their views.

**What a write kills: r82, the user's call.** r81 said writing a place kills every tie
under it, inline storage included, so `ball = Vec2{…}` killed `self.ball`, and breakout's
`rest_ball`, sokol's `spawn` and `event`, box2d's `event` and `Formatter::set_spec` would
each have needed a clause although nothing moved. Asked, the user chose: moving or dropping
still kills everything under the place, but writing kills only what the old value owned and
a union's or optional's payload. In the analysis:

- **A kill's reach follows its kind.** Drop and move reach every view under the place;
  a write and a call's clause reach only views whose tie passes through a tied pointer's
  storage below it, broad views (`in w`, elided results) under it, and payload references.
  So `kills w` from a call leaves `&w.at.x` alone, and replacing `xs[0]` ends what the old
  element owned but not `&xs[0]` or `&xs[1]`, which no longer need `xs@items` in the clause.
- **A write kills only when it can move something**: the type has a `drop`, a tied pointer
  field, a union variant with a payload, an optional, or a type parameter, inline anywhere.
  Plain data kills nothing, so it needs no clause.
- **Payload references are followed.** `case Circle(&c)` and `if let &w = opt` make a view
  remembering the union or optional it is in, which any kill of that place ends; a payload's
  fields are read through the variant. "Union payload tie paths are not settled" is no longer
  a reason to give up: a clause names the union's place, which is all a body needs. How a
  *path* would name one payload apart from the others is still open.
- **One mistake, one finding**: a read inside an argument (`c` in `observe(&c.r)`) and the
  argument itself no longer both report the same kill (invariant 6).

After this, across std and the examples: no use after a kill, 544 bodies complete, and one
mismatch, `Formatter::set_spec`, which writes two `usize?` fields: under r82 that is a payload
change, so 2d gives it a clause (the fields are private, so a declared tie). The leading
reasons a body is incomplete: no callable signature (193, mostly generated bodies), function
types without clauses (95), unknown returned provenance (40–43 each for elision and pointer
results), and destruction through an untied pointer (27, which is std's trusted code).

Tests: `dz_flow/tests/ties.rs` has 29. New: plain writes against payload, optional and
owned-memory writes, a coarse clause leaving a plain field alone, casts, and an element
replacement that leaves slots alive. Two changed with r82 (a replaced slot and a replaced
array no longer end references to the slots). The every-body test now also requires std
to produce no invalid use or return; with the first fix undone, it fails.

## 2c, second part: following references through bodies (2026-10-28)

`dz_flow::ties::analyse_body` now walks a checked body and returns its kills, invalid uses,
wrong returned ties, caller-visible effect summary and clause disagreements. It remains
separate from `dz_flow::check`: this is analysis that can be exercised and reviewed before
std's clauses and the diagnostics/fixes turn enforcement on. No language rule changed.

- **Addresses and owned memory stay separate.** An origin remembers its logical target
  as well as the tie keeping its address valid. `&xs[i]` targets `xs[*]` but lives in
  `xs@items`. A view into an element's owned field follows the former, so it survives a
  reallocation of the array. Index/IndexMut calls, including their lowering to HIR method
  calls, preserve this distinction, as do safe pointer returns into indexed storage.
  Replacing an element ends both its slot's references and its owned memory.
- **Aliases and generations.** Local reference, pointer, slice and string views carry
  origins through bindings, copies and pointer retargeting. A kill marks the existing
  views; a fresh view starts live. Moving a reference ends its binding, not the referent.
  Scalar writes that move nothing keep mutable aliases legal. An input's `in` annotation
  tells the body which owner's kills can end that reference without claiming that writing
  the reference replaces the owner whole.
- **Effects.** Assignment, move, scope exit, destruction intrinsics and calls record
  kills at spans in the body. Calls read signatures, not bodies, with generic type
  substitution and concrete trait implementations where known. Receiver and argument
  values remain live while later arguments are evaluated. A call also checks independent
  argument/receiver aliases against what its clause kills. Indirect calls conservatively
  kill through mutable arguments and remain incomplete until function types keep clauses.
- **Control flow.** Branches union possible origins and kills, excluding paths that do
  not reach the join. Loops reach a fixed point, including `continue` edges and kills from
  earlier iterations; fresh iteration bindings are refreshed. Native array/slice loops
  retain an iteration view. `break`, `continue` and return apply exited scopes' drops and
  defers before recording the edge. Scope metadata and temporary argument/return views
  follow the same branch edges, so a returning branch cannot poison a surviving one.
  Provenance growth and replay are bounded for editor responsiveness; hitting a bound
  explicitly marks the analysis incomplete.
- **Returns and summaries.** A view cannot escape a local or a by-value parameter that
  is about to die, nor survive a deferred kill before return. Known returned origins must
  fit the declared/elided tie; elision through a helper retains the input's slot-address
  dependency. Only kills through mutable inputs become the caller-visible summary;
  local drops, constructor destinations and copy destinations do not. Complete summaries
  use the first part's missing/unused/broad-clause comparison.

**Still open in 2c:** aggregate view storage and destructuring (tuples, optionals and
structs containing views), union payload naming, general iterator provenance, mapping
private pointer indexing to the declaring container's `[*]`, default-argument effects and
function-type clauses. An unknown/unsupported case is recorded in `incomplete`; such a
body produces no clause-removal or narrowing suggestion. Raw pointer fields/casts retain
§6.7's unchecked boundary. New diagnostic codes, rendering and fixes are not enabled yet.

Tests: `dz_flow/tests/ties.rs` now has 27 tests, including valid/invalid body pairs for
calls, indexing and owned element views, branch joins, fresh references, moves and
replacement, scope exit, argument evaluation, returns, defers, breaks, continues and
native `for` loops. Other cases pin clause summaries, indirect/default/aggregate and
provenance-growth boundaries, and run every fixture/std HIR body without panicking.

## 2c, first part: the call-effect model (2026-10-28)

`dz_flow::ties` now reads what a signature can invalidate at a call. This is the shared
model for the body analysis, not enforcement: `dz_flow::check` still reports the same
diagnostics, and 2c remains open. No language rule changed.

- **One representation for each place.** A path is rooted in a caller's `LocalId`, with
  fields, `[*]`, and the storage behind a pointer field as distinct steps. A public tie
  on a non-pointer normalizes to its private field; one on a pointer normalizes to that
  field's storage. Thus `w@bag@items` and `w.bag@items` compare equal, while
  `xs@items` and `xs[*]@text` remain separate. Element types come from
  `dz_check::ties::elements`, shared with the resolver, including generic `Index` and
  Iterable-only containers.
- **Signatures at arguments.** `call_effects` takes the origins of the actual receiver and
  arguments, translates every kill, returned tie and parameter's `in`, and retains each
  kill's clause span for later diagnostics. `Path::bind` also permits normalization once
  and substitution at multiple calls. It reads no callee body. Parameter contracts do not
  replace independently known argument origins.
- **Returned views.** An explicit result keeps its named path; elision with exactly one
  reference input (including `self`) keeps that input whole. A kill beneath a whole-input
  tie ends the view, while a kill of list storage leaves an element's separately owned
  text alone. Ambiguous inputs and origins not known to the caller stay unknown; this
  makes no new decision about elision with multiple inputs.
- **Unknown effects stay unknown.** Imported functions kill through `&mut` and `^mut`
  arguments, and a mutable imported receiver. An argument whose origin is unknown sets
  `incomplete`, rather than silently becoming a function that kills nothing. Unknown
  return or parameter ties are represented separately from unknown kills.
- **Exact clauses.** `minimal` removes repeated and subsumed effects without merging
  siblings. `compare` distinguishes a missing kill, an unused declaration and a broad
  declaration, keeping indices into the original paths for labels and a replacement
  clause. A too-narrow declaration over a broader actual kill produces only the missing
  kill, rather than also claiming the declared memory never dies. This comparison is
  for complete body summaries, not trait/extern/function-type promises.

Tests: nine `dz_flow/tests/ties.rs` cases use real parsed and checked signatures, with
valid/invalid comparison pairs, public aliases, generic `Index`, arrays, slices,
Iterable-only containers, receiver and parameter substitution, returned ties and elision,
foreign effects, unknown origins, and malformed incremental signatures. The checker and
existing ownership suites pass too.

The body walk follows in the second part above. Diagnostics and fixes become enforced
with 2d/2e's std and corpus updates.

## 2b: resolving tie paths (2026-10-28)

Every tie path in a signature now has to name something, and something every caller can
see. Nothing yet checks a body against its clause; that is 2c.

- **Where the clause lives.** A signature is collected before every type's fields are, so
  `FnSig` keeps `kills` and `ret_tie`, and `ParamSig` its `tie`, as written. The new module
  `dz_check::ties` resolves one into a `Tie` — a root (`self` or a parameter's index) and
  steps (field, `[*]`, tie) — and `ties::check` lists what is wrong with a signature.
  `dz_flow` will call `resolve` for the paths themselves. This kept the change out of the
  dozen places a signature is built: the checker runs it once per function, from
  `Body::function`, and for a trait's required methods, which have no body.
- **`FieldSig::tie`** carries a field's `in @name`, and **`FnSig::kills_unknown`** marks an
  imported C or C++ function, whose header says nothing of what it kills.
- **What a path may name.** A root is a parameter, or `self` in a method. `.field` goes
  through a struct's fields; `@name` through a tie a field declares, and stops there when the
  field is a pointer, since the tie then names that memory whole. `[*]` is settled: the
  elements of an array, a slice, or an `Index<usize>` type, whose `index` gives the element
  type; an `Iterable`-only type such as `HashMap` is named whole at `[*]`.
- **Visibility** (§4): a field in a clause must be reachable from the function and at least
  as visible as it, ranked private < `protected` < `pub(module)` < `pub`. A private free
  function counts as `pub(module)`, since the whole module may call it. A tie is a public
  name, which is how a `pub` method names private storage.
- **Codes**: `DZ0562` a path that names nothing, `DZ0563` a field some caller cannot see,
  `DZ0564` a kill through `&`, `&self` or a by-value parameter, `DZ0565` an `in` on what
  points into nothing (or into itself), `DZ0566` a tie declared twice. Each has its `dz
  explain` entry, wrong and right.
- **Left for later**: a function type's clause is parsed but not resolved, nor part of
  `Ty::Fn`. `List` declares no `@items` until 2d, so `kills xs@items` on a list is `DZ0562`
  today, and the bare `kills xs` stands in for it.

Tests: `typed/` gains every valid form and `typeerrors/` a case for each new code (invariant
8), and two targeted checker tests pin generic types, implementations, `protected` through
inheritance, and `pub(module)` against the function's own visibility.

## 2a: the syntax (2026-10-27)

Parsed and kept, with no meaning yet: nothing past the parser reads a clause.

- **`kills` is a keyword** (`dz_lex`), and `@`, already a token nothing used, starts a tie.
- **The tree** (`dz_ast`): a `TiePath` is a root (a parameter or `self`) and steps — `.field`,
  `[*]`, `@tie`. `FnDecl` gained `ret_tie` (`-> ^T in self@items`) and `kills`; `Param` and
  `Field` gained `tie`; a function type gained optional parameter names and its own `kills`.
  The dump prints them only when present, so no earlier snapshot changed.
- **The parser** (`dz_parse`): a clause sits after the return type, a reference's `in`
  after its type and before a default, a field's `in @name` after its type. A path that
  breaks off is one `DZ0218` and is kept as far as it got: `xs[1]` drops the index and keeps
  `[*]`, and `in items` reads as `in @items`, so the default, the body and everything after
  still parse (invariants 1 and 6).
- **One grammar point settled here:** in a function type, a comma after a bare path begins
  the next parameter (`fn each(f: fn(xs: &mut List<i32>) kills xs@items, n: i32)`), so a
  list of paths there is parenthesised. A declaration's list never needs to be, and a
  parenthesised one is accepted anywhere; the formatter writes the parentheses only where
  they are needed.
- **The formatter** (`dz_fmt`) writes all of it through the dump's own spellings, so a clause
  survives exactly; `tests/fmt/ties.dz` shows the layout it settles on.
- **Hygiene** (`dz_macro`): a tie path's root is renamed with the parameter it names, through
  a new `tie_root` hook, so a clause a macro writes still names its own parameter. A function
  type's clause is not walked: its parameter names are not binders.

- **The editor**: `kills` is coloured as a keyword by the VS Code grammar and offered by
  completion (`dz_lsp`'s `KEYWORDS`), the two places a new keyword was once missed.
- **Unrelated, found on the way:** `dz_resolve`'s workspace snapshots counted std's files
  before the prototype branch added three (`scope_arena`, `scope_pool`, `scope_shared`);
  only those counts changed.

Tests: `tests/parse/ties.dz` (every form), four mistakes in `tests/parse/errors.dz`,
`tests/fmt/ties.dz`, `kills` in `tests/lex/keywords.dz`, and two targeted parser tests (the
comma rule, and `kills` no longer being a name). `dz explain DZ0218` has its wrong and right
programs.
