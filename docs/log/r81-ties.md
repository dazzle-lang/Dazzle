# r81: ties, `kills` clauses and compiler-placed memory

The language change is spec §6.7 and §14.6, decided as D-0091; the plan, in order, is
T-0107. How it came about: the regions prototype (T-0104, T-0105, on the branch
`t-0104-regions-prototype`) tried to choose allocators without a checker, and each stage's
safety rested on rules a test had found. The user's own "ties" idea supplied the checker, and
D-0091 was decided on 2026-10-27.

## r93: an item outlives its iterator (2026-10-31)

T-0111's second part. r88 tied a method's result on a local view struct to the local and to
what it views, so `let it = xs.iter(); return it.get()` was a view of a local. A view
struct's views are one tie (D-0093, point 3), so a tie it declares on a view field can only
mean what the field views.

- **`through_view_field`** (`dz_flow/src/ties/body.rs`), asked by `invoke` for each `in`
  path of a call's result: a path whose root is a view struct and whose first step is a field
  holding a view gives the result what the input holds, in place of the bound place. A local
  input's views are its state's; a temporary's, the views it carries. Where they are not
  known, or the input is a parameter or a field, the bound place stands, as before.
- **std.** `SliceIter` and `SliceIterMut` declare `tie @items = m_slice` and `get` is `in
  self@items`; `Split`, `Lines` and `SplitWhitespace` declare `tie @text = m_text`, and their
  `get` is `in self@text`. The trait's `Iterate::get` still elides to `self`; an
  implementation's narrower `in` is safe, and nothing yet checks one against the trait's
  (T-0111's list).
- **Storage.** `check_view_storage` refuses a view in a union's payload, a carried struct
  included, and in a static.

Tests: `tests/flow/clean.dz` (an item returned past its local iterator, a piece of a split),
`use_after_kill.dz` (the item still dies when the list grows), `typeerrors/` (the union
payload and the static).

## r92: view structs and `view` parameters (2026-10-31)

T-0111's first part, D-0093's points 3 and 8 as r90 refined them. Its placement rule only;
the ties through a view struct's fields come next.

- **Syntax.** `view` is contextual, like `type`: a keyword only before `struct` (an item,
  `pub view struct`), before `type` among a trait's members, and before a type parameter's
  name. The lexer is unchanged, and a field, a local, a parameter or a generic parameter may
  still be named `view` (the parse corpus has each). The syntax tree carries it as
  `StructDecl::is_view`, `GenericParam::is_view` and `MemberKind::TypeAlias::is_view`; the
  dump and the formatter print it back.
- **What is a view, for placement.** `TypeTable::placed_as_view`: a `view struct` (known from
  pass 0, so any declaration may name one), a `view` parameter (`ty::Param::view`, set where
  pass 0 allocates it, a trait's associated types included), a projection of a `view type`,
  or a tuple, optional or array of one. References, slices and `str` are left to T-0109 and
  T-0110.
- **Where one may not go**, each `DZ0517`: a plain struct's field (`collect_struct`); a type
  argument written for a plain parameter (`check_view_arg`, from `type_args`) or inferred
  for one (`check_bounds`); a choice for an associated type the trait does not declare
  `view`. Three rules keep a plain parameter from ever receiving a view: a `view` parameter
  is refused on a plain struct, a union and a trait (pass 0); an implementation's parameter
  that reaches its type's `view` parameter must be `view` itself (`impl_header`); and `view`
  written on an implementation's choice is an error (`DZ0533`), since the trait decides.
- **std.** `Iterate::Item`, `Iterable::Iter` and `IterableMut::IterMut` are `view type`s,
  and the thirteen iterators are view structs: the slice, map and slot-map ones, `Chars`,
  `Split`, `Lines`, `SplitWhitespace`. Nothing in std, the examples or the run corpus stored
  one or gave one to a plain parameter; the projection tests (`tests/run/projection`,
  `typed/`, the feature matrix) and the DZ0560 explanation took `view`, and their adapters
  became view structs.

Tests: `typed/` (a user's view struct over a `view` parameter, and a trait's `view type`
chosen as an iterator), `typeerrors/` (one case per rule), and the parse and fmt corpora.

## r91: a function type says where its result points (2026-10-30)

T-0113, the first of D-0093's steps in r90's order. A function type could name its
parameters for a `kills` clause (r81) but not say where its result points, so a function
value returning a view from two inputs left every call through it unchecked.

- **Syntax.** `TypeKind::Fn::ret_ties`, parsed after the result with the declaration's
  `parse_in_list`: one path, or several in parentheses. The type parser takes it, so an `in`
  after a function type's result is the type's, never the enclosing parameter's tie. The
  dump and the formatter print it with `in_str`, between the result and `kills`.
- **The type.** `FnKills::Known` gains `results`, as written, beside `paths`. A function
  item's value carries its signature's `ret_ties`; `fn_type_sig` hands them to whoever reads
  the type as a signature, so `dz_flow`'s `call_effects` ties a call's result to the
  arguments the `in` names with no change of its own.
- **The checks.** `check_fn_types` now takes the span of the type it walks (a parameter's,
  the result's, a field's): it resolves the `in` paths, reports an `in` on a result holding
  no view, and holds the result to r88's rule through `untied_fn_type`, so several view
  inputs and no `in` is `DZ0570` and a view from none `DZ0571`, with the same exemptions and
  no fix (the type's parameters may be unnamed). Fitting a function to a type is
  `FnKills::within`, now split into `kills_within` and `results_within`, which compares the
  places by parameter position after elision (`result_places`: the `in` paths, or the one
  view input whole). `expect` tells the two apart: `DZ0568` for kills, the new `DZ0572` for
  a result that may point into more than the type allows.

Nothing in std, the examples or the corpora had a function type whose result holds a view,
so nothing else changed. Tests: the parse and fmt corpora, `a_function_types_result_takes_in`,
`typed/`'s `pick_with` (a narrower function into a wider type, and elision on both sides),
`typeerrors/`'s three cases, `a_call_through_a_function_value_ties_its_result_as_its_type_says`
in `dz_flow`, and `dz explain DZ0572`.

## r90: the open questions closed, and the prototype removed (2026-10-30)

The user asked for every open question on ties and regions to be settled, and accepted the
answers proposed. Three were places where D-0093 (r89) did not fit the rest of the
language, now recorded in D-0093 and r90's row: point 8 refused `T = SliceIter<i32>`, which
§8.5's projections and two corpora rely on, so a type parameter or associated type may be
marked `view`; `-> &T?` meant a reference to an optional, so an optional view is a pointer
(`^T`, and `^str` for `str?`); and view structs hold references, which D-0003 said nothing
may. The rest went into their tasks: `view` is contextual (T-0111), the untied-pointer check
is `dz_flow`'s and a pointer parameter is untied (T-0114), `process_env` returns `^str`
(T-0112), a trusted body is checked at its edges in debug builds (T-0116, which takes over
T-0115), a drop that only frees is proven, not marked (T-0118), and placement is shown by
inlay hints and `dz placements` (T-0117). T-0085 and T-0100 are done, T-0099 and T-0115
dropped. D-0093's order changed: view structs before `&str`, since `Chars`, `Split`, `Lines`
and `SplitWhitespace` hold a `str` field.

**The prototype is removed** (compiler work; T-0117's question). All three stages: `dz_cpp`'s
`regions.rs` and its hooks in the emitter (`Options::regions`, the block's `_dz_region`, the
hidden result parameter, the copies into a container's contents), `DZ_REGIONS` in `dz build`,
the `#allocator` intrinsic in `dz_ast`, `dz_check` and `dz_hir`, and in std `ScopeArena`,
`ScopePool`, `ScopeShared`, `Allocator::scoped` and `contents`, `contents_allocator` on
`List`, `HashMap` and `HashSet`, `String::copy_in`, and the copy guard that sent a copy of
a scoped container to the heap. std's constructors default to `Allocators::Global` again, as
before the prototype. `tests/run/regions` and `regions_change_nothing` went with it. Kept: a
codegen fix stage 3 found, which declares each trait's `__dyn` handle ahead of its vtable, so
a trait method may name the trait's own object type. The code is at the git tag
`regions-prototype` (`548b771`) for T-0117 to mine; the measurements stay in T-0104 and
T-0105. The HIR snapshot lost std's prototype bodies and nothing else.

## r88: every returned reference says where it points (2026-10-30)

The user's direction from earlier the same day, references keep their tie, designed and
built. The user settled four questions: several view inputs and no `in` is an error, not
Rust's rule that `self` wins; `in` takes several paths in parentheses; a reference, slice or
`str` result from no view input is an error, a raw pointer's is not; and a struct, tuple or
optional holding a view follows the same rules.

- **Syntax.** `FnDecl::ret_ties` (was `ret_tie`) is a list, parsed by `parse_in_list` as one
  path or several in parentheses, printed by `dump::in_str` (dump, formatter, completion).
- **The checker.** `ties::untied_result` holds every signature to it, bodiless ones
  included, from `check_ties`: `DZ0570` with several inputs, offering `in (a, b, …)` (every
  input, never wrong, for the author to narrow, so only offered); `DZ0571` with none,
  offering `const str` for a `str` result. `ties::holds_view` says whether a type holds a
  view, with a flag for what a type parameter counts as; `dz_flow`'s `contains_views` is it
  with `T` counting. Left alone, each for a reason: a view only through a type parameter
  (`pop() -> T?` is the element, which points wherever it did), a raw pointer or `cstr` from
  nothing (it may own what it points at: `c_malloc`), an `extern fn` with no input (C's
  memory: `dir_next`'s text lives until the next call on a `usize` handle, which no tie can
  name), and a result with a parameter that holds a view by value (no rule for it yet). An
  `in` is allowed on any result that may hold a view.
- **The analysis.** `ReturnTie::Explicit` holds every place `in` names, and a call's result
  views each; the return check accepts any. A result holding a view elides like a reference
  (`call_effects`), except one that was a view only through `T` before substitution, which
  `Signature::effects` leaves unknown (`generic_result`). Then four places that dropped a
  known aggregate view keep it: `bind` keeps a struct local's views (`let it = xs.iter()`),
  a tuple a call returned is no longer marked unknown when its whole tie is known, and
  `t.0` and `let (a, b) =` on it give each part the whole's views. Assigning over a whole
  struct or tuple local rebinds it, as a pointer local was, rather than reading the old,
  possibly dead, views.
- **A gap this closed that predates it.** A call on a local view (`v.trim()`, `v.split_once`
  where `v = s.as_str()`) tied its result to the local `v`, not to the text `v` views, so
  `s.push_str` never ended it. `invoke` now adds what such a local views.
- **What it changed.** In std, `strip_prefix`, `strip_suffix`, `split_once`, `split` and
  `tag_value` are `in self`; `Allocator::realloc` and the arenas' are `in (self, p)`, since
  `ScopeArena` and `TempArena` hand `p` back when it shrinks in place (the first try, `in
  self`, was a `DZ0414` in each); `Heap`'s is `in self`; `contents_allocator` is `in (self,
  fallback)`. Eleven test programs, and the `#record` macro in `tests/expand/shapes.dz`
  (reported at each invocation, with no fix, since the `str` is the macro's), returned
  literals as `str` and now return `const str`;
  `find_movie` is `in list`; `typed/`'s `Counter.label` is a `const str`. Over std's 692
  bodies, the 13 returns "with no unambiguous declared tie" are now 0.

Tests: `dz_flow/tests/ties.rs` (`every_returned_view_says_where_it_points`, both codes and
what stays allowed; `a_returned_aggregate_keeps_its_inputs_tie`, an iterator, a split, a
two-path result and `pop`); `tests/flow/use_after_kill.dz` and `clean.dz` end to end;
`typed/` and `typeerrors/`; the parse corpus (and an unclosed list in `errors.dz`) and the
fmt corpus; `dz explain` for both codes.

## 2c, sixth part: a tuple keeps its parts' ties (2026-10-30)

Compiler work: §6.7 says every reference carries its tie, and its unchecked boundary is a
pointer made in `unsafe` or stored in a struct. A tuple was neither, yet a view put into one
lost its tie: `let t = (xs.first(), 1)`, then `xs.push(1)`, then `t.0` or `let (p, n) = t`
compiled, and read freed memory.

- **A value has parts.** `Value::parts` holds, for a tuple, each part's own views, so `t.1`
  after a kill that ended the view in `t.0` is fine, and `(p.a[0], p.b[0])` keeps the two
  lists apart. A tuple literal holds each part while the next is evaluated, as a call holds
  its arguments, so `(xs.first(), grow(xs))` ends the first part.
- **Everything that handles a value reaches into its parts**: `t.0` (`tuple_part`, which
  reads only that part), `let (p, n) = t` (each name takes its part), `bind`, an optional
  holding a tuple through `or` and `if let`, a `move`, branch and loop merges, kills,
  reads, scope-exit drops and `return`, which now sees a view of a local in a returned tuple.
  Assigning `t.0 = o.first()` repoints that part, as assigning a pointer local does.
- **Where the parts are not known** (a tuple a call returns, a parameter, a field), the
  tuple is unknown, so a merge with a known one cannot lose that; its uses stay unchecked as
  before. A call returning a tuple of views still has no elision rule: that is the open
  question in T-0107, and the user's direction that returned references keep their tie.
- A destructuring with no views in it (`let (c, width) = decode_utf8(…)`) is no longer
  counted as unchecked.

Over std's 715 bodies, the nine tuple sites left unchecked (five literals, four
destructurings) are all followed now. One of them reappears where it is returned
(`-> (^T, …)` has no tie to hold it to), which is the elision question again. Every example
and run program checks with no tie error; `examples/sdl3` and `tests/run/cmake` need Ninja,
which the machine this was written on lacks, so CI checks those two.

Tests: `dz_flow/tests/ties.rs` (`a_tuple_keeps_each_parts_ties`: a part, a destructuring,
a nested part, a kill in a later part, an optional through `or`, a returned local, and
the plain part, the other list and a repointed part staying clean), and the fixture that
pinned tuples as unknown now expects them followed: a reference carried in one is exact
enough for its clause to be too broad. `tests/flow/use_after_kill.dz` has the case end to end.

## 2c, fifth part: loops over containers (2026-10-30)

Compiler work: §6.7 already listed `for &mut e in enemies { enemies.push(…) }` as an error,
and it compiled. The checker writes `for e in xs` as a loop over `xs.iter()` (or
`iter_mut()`), whose result is a struct holding a view, which no elision rule reaches; so the
elements had no origin and their uses went unchecked.

- **The elements.** `iterated_container` sees through that call (only the prelude's
  `Iterable::iter` and `IterableMut::iter_mut`) to the container, and `iterated_origins`
  gives each element what `&xs[i]` would have through the type's `Index`, slot and owned
  memory apart. A type that can only be iterated (`HashMap`) has its elements named whole
  at `[*]`, as §6.7 says, so any kill under the container ends them. Tuple bindings
  (`for k, &mut v in m`) take the same origins, since every part points into the container.
- **The loop.** The iterator dies with the elements' storage, and `loop_body` already read
  it each pass, so a kill in the body that the loop goes on past is a `DZ0413` at the
  subject, with its own message ("the loop goes on over memory its body killed"), told
  apart by `InvalidUse::next_pass`. A `break` after the call leaves no next pass.
- **A value argument is a copy.** With elements followed, `counts.insert(n, 0)` inside a
  loop over `counts` was reported: the call kills where `n` points, and `n` was an
  argument. But a value parameter is copied before the callee runs (C++ passes it by
  value), so only an argument to a parameter that points is checked against the call's kills.

A census over every body of std, the examples and the run corpus (1,100-odd) counted
`unchecked` sites by reason: loops were 23 of the 97 call sites whose returned view had no
input tie, and 8 of the 12 tuple loop bindings. What remains is listed in T-0107. No
example or run program had a loop that kills its container.

Tests: `tests/flow/loop_kills_its_container.dz` (a `List` and a `HashMap` that grow while
walked, a `break` after the push, and a write through an element); `dz_flow/tests/ties.rs`
pins the same, the next-pass flag, and a value copied out of a container into a call that
grows it.

## 2e, second part: the editor, and the tie a fix declares (2026-10-30)

Compiler and editor work; nothing about the language changed.

- **A call's clause, wherever a signature is shown.** Hover and signature help quote the
  declaration from its source, so they had it already, and now a test says so; completion's
  detail is rebuilt from `FnSig`, and now writes `in` ties and the `kills` clause too
  (`fn push(v: T, loc: SourceLocation) kills self@items`).
- **The clause a body needs, as an inlay hint.** A function that writes no clause, over a
  body that kills, shows `kills …` before its `{`. It is read off `DZ0415`'s fix rather
  than worked out again, so it cannot disagree with `dz fix`, and the server attaches the
  insertion so accepting the hint writes it. A clause that is written but wrong gets no hint:
  its own errors say what it should be. Like every flow error, it appears only while the
  program type-checks.
- **The tie a `pub` method needs.** A clause through a field the function's callers cannot
  see was offered as it stood, which named the private field and so was itself an error.
  The fix now declares a tie for the field on its type and names that: `tie @things =
  m_things@items` and `kills self@things`, as `box2d` was fixed by hand. The tie is named
  after the field without its `m_`; every path through one field becomes one tie (`tie
  @names = m_names@items, m_names[*]`, r83); it goes after the type's ties, or after its
  fields with a blank line, indented like them. Still only offered, since the name is the
  author's. With no tie to declare (an inherited field, a name the type already uses, a
  type on one line) the old offer stands. Laying the line out needs the source, so
  `dz_flow::check` takes the `SourceMap`, as `dz_check::check` does.
- **`examples/sdl3` and `tests/run/cmake`**, whose clauses 2e's first part wrote by hand
  without Ninja, check clean on a machine with it.

Tests: `dz/tests/fix.rs` takes each offered fix in turn on a type with two private lists
and ends with no diagnostics; `dz_lsp/tests/ide.rs` has a call's clause in hover, signature
help and completion, and the hint (once per function, none for a clause already written).

## r87: `const str` (2026-10-29)

D-0092, the user's spelling of the fix for 2e's open gap: `describe(&self) -> str`
returning literals was tied to `self` by elision, so returning it from a function that took
the value by value was `DZ0413`, and no signature could say otherwise.

- **The type.** `Ty::ConstStr`, emitted as `::dz::str`; `assignable(Str, ConstStr)` lets
  it be a `str`, never the reverse. `const` before a type parses as `TypeKind::Const`;
  lowering accepts only `str` (`DZ0569`, explained).
- **Where it is one.** A literal is a `const str` where one is wanted, beside the `cstr`
  rule. A `const str` value reads as a `str` everywhere else: `expr` retypes the node
  (the C++ is the same), except where a place is written — an assignment's target and a
  `&mut`'s operand use `expr_inner` — and an untyped `let`, which keeps it. `impl_applies`
  matches `str`'s implementations for it (`Format`, `Hash`, `Eq`), `field_type` gives it
  `.len`, and the builtin `Eq`/`Hash` and formatting checks list it.
- **Ties.** It is not a view (`points` leaves it out), so returning one needs no `in`, a
  field of it is no aggregate view, and nothing kills it.
- **The message** for a `str` where a `const str` is wanted says why: the text may view a
  `String`.

`typed/`'s `Describe`, `Label` and `Loud` return `const str` now, and the five functions
that returned `String` in 2e return the text again.

Tests: `typed/` holds one in a field and a `List` and reads it as a `str`; `typeerrors/`
writes a `str` to a `const str` field, returns one, and writes `const i32`; the parse and
fmt corpora have the type; `tests/run/const_str` keeps a state's name across a change of
state, lists names, returns one from by-value arguments, and keys a `HashMap` by them.

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
