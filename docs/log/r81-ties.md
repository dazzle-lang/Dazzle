# r81: ties, `kills` clauses and compiler-placed memory

The language change is spec §6.7 and §14.6, decided as D-0091; the plan, in order, is
T-0107. How it came about: the regions prototype (T-0104, T-0105, on the branch
`t-0104-regions-prototype`) tried to choose allocators without a checker, and each stage's
safety rested on rules a test had found. The user's own "ties" idea supplied the checker, and
D-0091 was decided on 2026-10-27.

## T-0206, T-0207: two ways safe code escaped the ties check, closed (2026-11-13)

Compiler work; no language change: both are §6.7 as written, which the checker now holds.

- **A kill through a call missed a multi-place tie's later place** (T-0207). A tie standing
  for several places is spelt by its first (`canonicalize`), but `Cx::invoke` bound a call's
  kills and result origins without making them canonical in the caller's frame, so `grow_b(w)`
  killing `w.b@items` (spelt `w.a…` in the callee) never met a reference bound as `w.b@items`
  through `List::index_mut`. The result's origins are now made canonical there, and each kill
  ends what its canonical spelling reaches as well as its own (`kill_spelt`); it is recorded
  and reported as written, so `DZ0415`/`DZ0416` and `DZ0420` compare what they did. The other
  direction, that places of one tie "live and die together", now holds too, and it caught
  `std`'s `SlotMap::insert`, which read a slot's version through a reference into `m_slots`
  after pushing onto `m_values`, both under `@items`; it now reads it first.
- **A `for` over a user-written iterator checked nothing** (T-0206). A subject that was not a
  container's `iter`/`iter_mut` took its elements from `index_origins`, which knows no `Index`
  for a view struct and gave none. Now a view struct's loop walks what it views
  (`iterated_view`, from the local holding it or from what a temporary carries): elements
  named whole at `[*]` of that memory, broad, as an iterable type's are, so any kill under it
  ends a binding and the next pass. Following `Iterate::get`'s `in self@items` through the
  view field would be finer, but a temporary `one(xs)` has no place to bind as its receiver;
  every example and `tests/run` program still checks clean with the coarser rule. A tuple
  `Item` is covered the same way, and its `unchecked` note is gone.

**What it cost, and the cache that paid it back.** Making every call's places canonical
slowed `dz check` by 9–27% (`examples/ecs` 1.07 s to 1.35 s, almost all of it the ownership
phase): `canonicalize` worked out a type's tie rewrites (`rewrites`, through `export_path`) on
every call, and a `List`'s are asked for at nearly every one. They are now kept per type
instance for the body's walk (`ties::Rewrites`, in `Analysis::rewrites`), which the field reads
that already canonicalised share. Against the compiler before T-0208, best of seven, debug
builds: `ecs` 1066 to 931 ms, `breakout` 481 to 381, `tests/run/regions` 817 to 791,
`bench_programs/expr` 469 to 397, `tests/bench` 381 to 304. The generated C++ of every
benchmark is unchanged apart from paths, so their run times are too.

Tests: `tests/flow/kill_through_a_later_tie_place.dz` (through a field-named and a tie-named
clause, a method, the reverse direction, the first place, a direct push, and a reference
taken after the kill, which stays clean) and `tests/flow/loop_over_a_view_struct.dz` (a call, a
local and a tuple iterator, each refused twice; a `break` after the kill and another list
grown stay clean).

## T-0209: an element's `&mut` followed through a local (2026-11-13)

Compiler work; no language change. T-0208 followed the `&mut` element `index_mut` lends as far
as the expression it is used in, and kept the drop once a local held it. Now `let r = xs[i]`,
`let m = &mut xs[i]` and a `for &mut s in xs` binding are followed through every use
(`Contents::local_keeps`): a reference cannot be pointed elsewhere, so each use is the
element, judged as `lent_to_keep` judges the expression, and `r = v` writes the element (§5.2)
and keeps the drop. A `for` is accepted when its subject is `iter_mut` itself, whose iterator
nothing else sees (`walked_by`). Tests: four cases in the indexed-container placement test,
and `tests/run/regions`' `indexed` reads through a local and a loop, the same tracked, with
regions on and off.

## T-0208: an indexed container skips its drop, and a leak no test could see (2026-11-13)

Compiler work; no language change. A `List<String>` filled in its region kept its drop as
soon as it was indexed, even to read: §11.2 makes `xs[i]` on a writable place `index_mut`,
and `whole_inside` refused any `&mut self` call returning something writable. The task
proposed choosing `Index` for reads in the checker, but that is §11.2's `[DECIDED]` rule
(D-0050) and would change what `let r = xs[i]` is, so the checker is unchanged and `dz_flow`
follows the `&mut` instead (`Contents::lent_to_keep`): through the place it roots
(`xs[i].name`), it may be read, borrowed `&`, formatted, given to an operator or a cast,
assigned where the part owns nothing, or the receiver of a method that keeps (`keeps`): the
element's own, or `index_mut` again for `nest[i][j]`. Anything that hands on something
writable, `&mut` of an owning part, `construct`, `destroy`, `move_out` and `move` keep the
drop. A `let` holding the `&mut` is not followed yet (T-0209).

Found on the way, by surveying which drops `examples/` and `tests/run` still keep:
- **A leak in safe code.** `adds_foreign` took every `move_out` for an element the container
  already had, so `List::append`, which moves another list's elements in, was not an adder:
  `names.append(&mut other)` skipped `names`' drop while it held `other`'s heap strings. Now
  only a `move_out` of `self`'s own memory counts, directly or through a `let` that is not
  `mut` (`swap`'s `held`, `HashMap::rehash`'s `old_keys`, through the `unsafe { }` it is in).
- **`names.sort()`** kept a drop: `Sort::sort` is a trait method, which `keeps` refused, and
  `swap` was an adder by the rule above. A trait method is now found in the type's own
  implementation (`runs`), accepted when it is given nothing that owns memory, since
  `storing` works out stored parameters only for a type's own methods.
- **`xs[0] as usize` was refused** (`DZ0413`, "returns a view into `xs`"), since a cast passed
  its operand's origins on whatever it made. A cast to a number, `bool` or `char` is a copy
  (`ties/body.rs`); the read through the reference is still checked.

**Why none of it was seen.** A drop kept for nothing changes no output, and the skip test
pinned only what was kept, with no indexed container in it. A drop skipped wrongly is a leak,
and no automatic test looked for one: the sanitizer job runs with `detect_leaks=0`, and
`a_tracked_build_reports_leaks_and_bad_frees` compared stdout alone while the leak report goes
to stderr with the program succeeding, so its "clean" fixtures were never checked for leaks.
It now fails on a report, and runs `regions`, `homes_through_calls`, `short_strings`,
`element_indices` and `containers` too; LeakSanitizer in CI is T-0211. `transfers` cannot join
it yet: under `--track-memory` a static that owns memory is reported as leaked, then freed into
a tracker already dropped (T-0210, older than this).

Tests: `an_indexed_container_skips_its_drop_unless_an_element_is_replaced` in
`dz_flow/tests/placement.rs` (reads, growth, formatting, parts, nested lists and `sort` skip;
assignment, `&mut` handed on, a `let` of the `&mut` and `append` keep);
`tests/run/regions` gains `appended` and `indexed`, the first leaking 61 bytes tracked under
the old compiler; `element_as_number` in `tests/flow/clean.dz` and `cast_after_kill` in
`tests/flow/use_after_kill.dz`.

## T-0178: a temporary a loop head looks into lives as long as the loop (2026-11-12)

`for c in format!(…).chars()` read freed memory in safe code. The loop was emitted as
`for (auto it = ::dz::obj(<format!>).chars(); …)`, and C++ destroys the `String` at the end
of that init statement, before the first pass. The same held for a range-for over
`numbers().as_slice()` (`auto&& seq = …` extends the slice's life, not the list's), an `if
let &x = numbers().first()`, and a `switch holder().shape()` whose payload binding referred
into the destroyed `Holder` (its `drop` printed before the arm ran). A `while` or `if`
condition is a `bool`, which keeps nothing. Compiler work, not a language change: the
checker already took the temporary to live as long as the construct that uses it.

Hoisting each temporary into a local before the construct, as `emit_args` does for a view
argument, would have moved its evaluation ahead of whatever precedes it:
`second(step(1), listed(2).as_slice())` would call `listed` first, against §7.2. So the
temporary is built in place instead. A new runtime type, `::dz::Kept<T>`, is an empty slot
declared at the top of the construct's own scope (`for_loop`'s block, a block `if let`
opens when it keeps something, the one `switch` already had, which the subject's evaluation
now moves inside). Its call operator move-constructs the value in place and returns it, and
its destructor drops it when that scope ends. `Emitter::expr` writes a kept expression as
`_dz_heldN(<expression>)` where it stands.

What is kept is `collect_owners`' answer for the subject, when its type looks into memory
at all (`borrows`). Three changes to it, which `emit_args` shares:
- A `format!` (`Formatted`) is a temporary that owns, as a call is.
- An argument that owns something is not followed. It is moved into the callee, so the
  result cannot look into it, and holding it would have copied a move-only value.
- `&make()` is not followed: its value already sits in a local to the end of the block.

`is_place` is not used to rule a subject out, because it takes any reference for a place,
and `holder().shape()` is a reference into a temporary.

Tests: `tests/run/loop_temporaries` covers the four head shapes. Its `Holder` prints its
drop, and a step-numbered case pins the order, so the output shows the fix even where
nothing checks the reads; CI's `sanitizers` job runs it under ASan and UBSan. The generated
C++ is warning-clean under g++ and clang++. Writing the test turned up T-0205: a field and a
method with the same name compile to invalid C++.

## r120, T-0202: `sends` and `in x@home` through traits and function types (2026-11-12)

Probes run against r119 found safe code reading freed memory. An implementation whose result
was `in self@home`, reached through a trait method, a bound or a function type that said
nothing of it, handed back a value built in its receiver's home; the caller, reading only the
trait's or the type's clause, kept it in a list that outlived that home's region. Direct calls
were sound. The other direction, an argument handed in, was safe only because placement
put such arguments on the heap.

- **Traits** (`clauses_within_trait`): an implementation's `sends` and `in … @home` must be
  within its trait's, as its `kills` and `in` already were (DZ0573), each pair or home
  compared by parameter position.
- **Function types**, decided by the user over refusing such functions: `TypeKind::Fn`
  gained `sends`, parsed after the result in either order with `kills`, more than one pair
  parenthesised as `kills` paths are (`parse_sends(in_type)`, `dump::sends_in`). `FnKills`
  gained `sends` and `homes`, the `in` split as a declaration's is, and `within` compares
  them (`sends_within`, `homes_within`); a function that does not fit is DZ0584 (new) or
  DZ0572. `fn_type_sig` hands them to callers, and `fn_types` checks their paths with the
  declaration's own loops (`homes_and_sends`).
- **The region check** read no clauses at a call through a function value, since
  `call_signature` answers only for a function item: `signature_at` falls back to the
  type's signature, for `region_clauses` and the homes walk.
- **A local reference**, found on the way, with no trait involved: `let s = &mut bag;
  s.make()` kept `bag` in a region. `local_root` stops at a reference local, so neither
  `built_into` nor `regions_held` saw `bag`; `containers_of` follows a reference local to
  what the walk says it views. The proposer's `built_and_left` follows `let s = &mut bag`
  (`reference_to`) and reads a function value's type, so the two still agree.

Tests: `tests/run/homes_through_calls` (seven shapes, each read back after a churn),
four programs in `tests/errors`, the `Tray` section of the typed workspace (and its HIR
snapshot), and the new syntax in `tests/parse/ties.dz` and `tests/fmt/ties.dz`. Every
example, run program and bench program still checks. `dz fix` extending an `in` already
written is T-0204.

## T-0203: a call through a trait object kills the object (2026-11-12)

A hole found while probing T-0202: `put_many(sink: &mut Sink)` called `sink.put(…)`, whose
trait declaration says `kills self`, and was accepted with no clause; writing `kills sink`
was refused as never killed (DZ0416). A caller's `&xs[0]` then survived `put_many(&mut xs)`
growing the list, and read its freed buffer once the list was on the heap (in a region the
old buffer stays, which hid it). The same body with `&mut List<String>`, or through a bound
`<S: Sink>`, was refused as it should be.

The cause was in `collect_ties` (`dz_flow/src/ties.rs`), which lists the ties a killed place
names: a trait object's type is `Ty::Named` with the trait's symbol, and that arm named
nothing for a trait, so `kills self` at the object was a kill of nothing. A trait object may
be anything, so it is now named whole, as a type parameter is.

Every example, `tests/run` program and bench program still checks with the stricter rule.
Tests: `tests/flow/trait_object_kills.dz` (the missing clause, DZ0415, and the view used
after the call, DZ0413) and a passing pair in `tests/flow/clean.dz`; without the fix the
first gives DZ0416 instead and the second DZ0416.

## r119, T-0200 stage 4, second part: a lent container is placed by its callees' clauses (2026-11-11)

Since the first part, the caller follows everything a callee's clauses can do with a
container it lends: what its `sends` hands on, and what it returns `in` the container's
home. A `&mut` cannot be kept. Three of the old inferences gave way to that.

**Lending keeps what was stored.** A local container lent `&mut` used to lose what had been
stored into it, since the walk could not see an element taken out. Lent to a call with
clauses, it now keeps it (`lend_keeps`). A function value or a C function still loses it,
having none. The proposer refuses only where the clauses do hand something on, and says
so: "handed on by `give` from `names`".

**Other modules' and local containers.** A method's `sends v to self` now counts where
`Staying::stores` has no answer (`sends_to_self`), in both the proposer and the check. A
`Shelf` from another module keeps the strings it is given in its caller's region. A `sends`
does not say the value is kept on every call, so the loop rule stays strict for these.

**An outer block's region.** A container a callee may build into, whose elements are all
sent by a `sends` into locals of enclosing blocks with no loop between, goes in the
outermost such block's region instead of on the heap (`Left::Into`). `tests/run/regions`'
`lent`, which `take_last` empties into `given`, is now in `main`'s region with `given`; the
region check accepts it unchanged, since a send into a local declared in that block dies
with that block.

Tests: `a_lent_container_is_placed_by_its_callees_clauses` in `dz_flow/tests/placement.rs`,
and K5 and K6 in `tests/run/transfers`. Every program checks out with regions on and off,
under the sanitizers, under the ties fuzzer and in the headless examples. The frame loop and
the benchmarks are unchanged.

## r119, T-0200 stage 4, first part: a function builds in a lent container's home (2026-11-11)

Placement reads `sends`. A value handed to a receiver under a parameter or `self`, by a call
whose clause says `sends v to self`, is built in that container's home. That is T-0150: a
function filling a list its caller lent it builds with the list's own allocator, wherever
the caller put the list. The new pieces:

- **`Decision::Home`**, and `Placed::Home(receiver)` for codegen. The home is read at run
  time from the receiver through its fields to the `m_allocator` every `std` container
  keeps (`placement::home_path`). Codegen writes `::dz::obj(::dz::obj(events).m_list).m_allocator`,
  and private fields are public in the generated C++.
- **The proposer** (`Escape::IntoHome`, `home_receiver`, `home_target`) proposes a home for
  a direct argument, or one surely stored after a `let`, and only where the receiver's type
  keeps an allocator.
- **The region check** treats a home site as a by-value parameter's region, not released by
  the body and not returnable, which may go only into its own container's input
  (`home_input`, `home_sent_roots`). A copy made there counts too.

**Callers stay sound.** A function building into a container its caller lent it puts memory
in the caller's region that the caller never saw made. So the caller's region check now
treats three things as carrying that region:

- a local container lent `&mut` to a function of the program's own (`built_into`); `std`'s
  methods only move what they are given;
- what a callee's `sends` hands on from a lent container (`region_clauses`);
- a result its callee says is `in x@home`.

When such a value leaves for somewhere longer-lived, the container goes to the heap. The
proposer makes the same judgement (`built_and_left`), so the two agree on the whole
corpus. In `tests/run/regions` this is exactly `lent`, whose test says "what `tag` pushes
here must not be in it". The ties fuzzer and every program under the sanitizers pass.

**Results.** `dz placements` counts homes: `tests/run/regions` builds four strings in a lent
container's home. On D-0097's frame loop the engine's `emit` and `text` declare
`sends … to self`, which the check demanded. Every value the systems make then goes into the
frame's lists in the frame's region: **no `malloc` per frame** (35 before), and 0.56 s
against 0.61 s on the heap. That is 3% slower than with those values on the heap and the
lists alone in the region, because a list's buffer, interleaved with the strings built into
its home, can no longer grow in place. `words`, `expr` and `astar` are unchanged.

What is left of stage 4 is in T-0200.

## r119, T-0200 stage 3: `std` says what it keeps and hands back (2026-11-11)

`std` has its clauses: seventeen signatures, listed in T-0200. `List::push(&mut self, v: T)
sends v to self`, `List::pop(&mut self) -> T? in self@home`, and so on. Its containers move
elements with intrinsics, so the homes walk now follows them in `std`'s bodies:

- `construct(dst, v)` stores `v` under `dst`, as an assignment does;
- `move_out(src)` takes out of `src`.

The views walk trusts a body over a pointer's elements (T-0116) and calls that
incomplete. That says nothing about homes, so the homes walk no longer stops there. Checking
`std` then found what the hand count had missed: `HashMap`'s private `place` and
`remove_slot`, and `StrKeys::remove_str`. A trait's `self` in a declaration may have a home,
as an implementation may give it memory.

With `std` declared, the stage-2 stand-ins went, and a call is read by its callee's clauses
alone. The stand-ins were:

- `Staying::stores` for what a `std` method stores;
- the guess that a `&mut self` method of `std` returning an owned value takes it out of its
  receiver.

The check no longer computes `Staying`. `std` is the same for every program, so a build
leaves its bodies out, and `check_std_homes` checks them once in the suite
(`std_says_where_it_sends_what_it_is_given`). Removing `push`'s clause makes that test fail,
naming `push` and also `insert`, which relies on it. Checking `std` in every build had cost
`breakout` 70% of its `dz check`; as built, the homes check costs 3–5%.

The `construct` handling also found three program containers written with raw memory whose
`push` stores into `self`, in `tests/run/generics`, `tests/run/iterator` and
`examples/ownership`; they got `sends v to self`. The editor's hover, signature help and
completion detail now show `std`'s clauses.

## r119, T-0200 stage 2: a body checked against its `sends` and `in x@home` (2026-11-11)

**The walk.** `analyse_homes` is the ties walk a third time, after the ties check and the
region check, and it reuses the region check's machinery. A home is a fact carried in
`Value::owned`, as a region is: a by-value parameter that owns memory starts in its own
home; an element taken out of a `&mut` input, or out of `&mut self`, is in that input's
(`homes_call`, from the callee's `in x@home` or, for `std` until stage 3, from any
`&mut self` method of `std` whose result owns memory); a constructor's or `Box`'s value
holds what it is given by value. A home is followed through `let`s, `or`, struct
literals, local containers and loops. Where the region check would refuse a placement,
this walk records a send instead:

- an assignment to a place under another input (`homes_assign`);
- a call whose `sends` names an argument and where it goes;
- a method of `std` that stores an argument into a receiver under an input
  (`Staying::stores`, until stage 3);
- a `return`.

A copy carries no home.

**The report.** `report_homes` compares the sends and returns with the signature both
ways, as `kills` are. DZ0421 is a send or an owned return the signature leaves out; its
fix writes the whole `sends` clause, or the result's `in` when none is written. DZ0422 is a
pair, or an `in x@home`, the body never does. Nothing is reported where the walk could not
follow a value, and `std` is skipped until its clauses are written.

**What the user decided on the way.** A by-value parameter returned is an owned result
in that parameter's home, so `largest<T>(a: T, b: T) -> T in (a@home, b@home)`. Placing
every argument whose type the result could hold where the result goes was the other
answer. It needs no clause, but it is a hidden guess, and it costs a longer home for
arguments that are never returned.

**Two fixes the walk needed.** `bind` kept `owned` only in the region check, and
`unwrapped` dropped it with an optional of a generic type whose views it cannot follow;
a home depends on neither.

**What it found in the repository.** What D-0097's trial predicted: the six functions of
`tests/run/transfers`, `take_last` in `tests/run/regions`, the `push` and `pop` of
user-written containers in `tests/run/iterator` and the `astar` benchmark, and
`Shelf::put` in `tools/bench_scale.py`'s generated program. It also found about a dozen
generic functions returning a parameter (`largest`, `swap`, `keep`, `unwrap_or`,
`or_else`, the feature matrix's helpers). `dz fix` wrote their clauses; `typed.dz` was done
by hand, since `dz fix` also applies the fixes of unrelated warnings there. Placement is
unchanged: it reads no clause until stage 4.

Tests: `tests/flow/send_not_declared.dz` and `send_never_happens.dz`, which also hold the
legal cases (a value built in the body, a copy, an element put back, plain data), and
`dz explain` entries for both codes. The walk adds about 5% to `dz check` on `examples/sdl3`
and `breakout`.

## r119, T-0200 stage 1: `sends` and `@home` parsed and resolved (2026-11-11)

D-0097 (homes) was decided as variant A. Its first stage is the syntax, with nothing yet
checked against a body.

- **`sends` is a keyword**, as `kills` is. `to` is read as a word in the clause and stays a
  name everywhere else (a parameter may still be called `to`). A signature takes
  `kills …` and `sends a to b, c to d` in either order, at most once each, and the formatter
  writes `kills` first. That is layout only, since `FnDecl` keeps the two apart.
- **`FnSig::sends`** holds the pairs. **`FnSig::ret_home`** holds an owned result's
  `in xs@home`, split from `ret_ties` by `ties::ends_at_home` when the signature is built.
  `dz_flow` reads `ret_ties` as where a view points, so an owned result's home must never
  reach it.
- **`@home` resolves to `Step::Home`**, which no struct declares, on any value that may own
  memory (`ties::owns_memory`: a type parameter may, and a reference or optional is asked of
  what it holds). It ends its path. `ties::check` refuses it in a `kills` clause, on a view
  result's or a parameter's `in`, in a `sends` path, in a type's own `tie`, after a result
  that owns nothing, and on a value that owns nothing: all DZ0582, each with its own reason.
  `tie @home` is DZ0581.
- **A `sends` path** resolves like a `kills` path, a static's included. Its source must be
  the function's own (a by-value parameter) or reached through `&mut`; its destination must
  be changeable (DZ0583). A pair with no `to` is DZ0220, and the clause keeps its other pairs.
- **The editor** completes `sends` and highlights it, and hover shows both clauses. The macro
  hygiene walk renames `sends` paths' roots as it does `kills` paths'. §2.7 lists the keyword
  and §20's grammar the clause.

Tests: `tests/errors/{sends_without_to,home_tie_declared,home_killed,sends_to_shared_reference}`,
a `dz explain` entry for each code, `typed/` (a container whose `post` sends into it and
whose `collect` returns `in self@home`, a generic mover, a send to a static) against
`typeerrors/` (`tie @home`, a killed home, a send to a `&`), and the parser and formatter
corpora (`tests/parse/ties.dz`, `tests/fmt/ties.dz`), with a `sends` written before `kills`
and a function named `to`.

## A value a `let` builds and then stores goes where the container lives (2026-11-11)

Compiler work; no language change. The loop rule (T-0145) lets a value made on every pass
into a region the loop does not end only when it is stored *directly*, by a method that
keeps it, into a container that keeps everything. Otherwise a pass could drop the value
while the region kept its memory, and the region would grow past what the container holds.
"Directly" meant the site was the argument itself, so a list built over several statements
and then pushed kept the heap:

```dz
let mut path = List<Vec2>()
for step in 0..12 { path.push(…) }
paths.push(move(path))
```

`storing::stored_after_let` finds the call a `let`'s local is moved into, `move(x)` and not
a copy, in a later statement of the `let`'s own block, with no `break`, `continue` or
assignment to the local in between. A `return` ends every pass, so it does not count. Such
a store is as sure as a direct one. The proposer (`Cx::surely_stored`, from `kept_in`) and the
region check (`kept_each_pass`) both use it, so they agree. Every other condition stands:
the method keeps the value, and the container keeps everything.

Found by D-0097's frame measurement. Written as one body, the frame made 16 `malloc`s per
pass, all path buffers; it now makes none, and runs 10% faster than on the heap. No
placement in the corpus, benchmarks or examples changed. `dz_flow/tests/placement.rs` pins
the rule (`a_value_built_by_a_let_and_then_stored_goes_where_the_container_lives`): the
direct case, and a `continue` between, a push under an `if`, and the local replaced before
the push, which keep the heap. `tests/run/regions` builds paths so, churns the region and
reads them back.

## A list grows where it is in a region: `Allocator::resize_in_place` (2026-11-11)

This is library work, not a language change: a defaulted trait method, like `scoped`. It
was found by measuring D-0097 on a game's frame loop. Each pass of the loop builds an event
list, a draw list and a list of paths, and systems fill them. With regions on, the loop ran
12% *slower* than with every value on the heap (200,000 frames: 0.68 s against 0.61 s,
release, clang++).

A sampling profiler (an `LD_PRELOAD` shim on a POSIX timer, since `perf` was not
available) showed destructors and moves around the draw list. A/B runs then settled it.
With the frame's lists given their capacity up front, regions were 4% *ahead*; with only the
draw list reserved, the gap closed. `List::move_to` grows a list of elements that are not
trivially copyable (`DrawCmd` holds a `String`) by allocating a new block, moving each
element and freeing the old one. On the heap the freed block is reused at once. In a region
it stays until the region ends, since only the last allocation can be given back. So every
doubling cost a full move and left the old block behind.

`Allocator` now has `resize_in_place(p, old_size, new_size, align) -> bool`. It resizes
without moving when the allocator can, and its default says it cannot. `ScopeArena` and
`TempArena` can when `p` is their last allocation and the chunk has room (`ScopeArena` also
requires the block to stay small, as its `realloc` did). Both `realloc`s now use it.
`List::move_to` tries it before the fresh-block path, so such a list in a region grows and
shrinks where it is with no move. The `Heap` keeps the default: glibc has no portable way
to try.

The frame loop is now 11% *faster* with regions than on the heap (0.54 s against 0.61 s),
and 20% faster than before the change (0.68 s). `words`, `expr` and `astar` are unchanged,
within a millisecond (best of 7, regions on, release, before and after built from the same
tree).
Entering and leaving a region was measured on its own: 10 million calls of a function that
builds a four-element list cost 0.10 s with a region and 0.14 s on the heap, since the
per-thread pool hands back the chunk with no `malloc`. `tests/run/region_arena` checks the
method directly (the last allocation resizes; an earlier one, or one grown into a large
block, does not), and grows and shrinks a `List<String>` in a region.

## D-0097: homes tried before deciding (2026-11-11)

D-0097 (homes, open) asked for its programs to be written before it is decided. They are
`tests/run/transfers`, with a second module, `movers`, for the cross-module cases. Each
probe moves a value out of a container whose memory could be a region, into something that
outlives it, then reuses the region with `churn` and prints the value. The test runs with
regions on and off, and under the sanitizers with the rest of `tests/run`.

No hole turned up. Every probe's value was placed on the heap with a reason that names it,
including `List::append`, whose transfer goes through a `&mut` parameter that the storing
inference (by-value parameters only) never looks at. The proposer already sends a local
"lent as `&mut`" to the heap. A result function that returns a value it did not build is
dropped by its caller and does not leak.

Two numbers matter for the decision, and both are in D-0097. Only four of the 118 heap sites
across `tests/run` are the shape homes would move, with none in the benchmarks or the
examples. And `Staying::stores` trusts a method's stores only within its own module and
`std`, so a container from another module gets the heap for everything it is given.

## r117, T-0168: elements known apart by their indices (2026-11-04)

The user asked, after the site's comparison with Rust, for the refusals both languages make
that could soundly be accepted. This is the first: `names[i].push_str(names[i +
1].as_str())`. The second, one list passed as both arguments of a function that keeps
nothing across its kills, turned out to depend on the callee's body rather than its
signature, which the check never reads at a call; the user chose to keep it refused (T-0169,
dropped).

A place's element step may now carry an index, `Step::Element(Index)`, where `Index` is a
literal or a local that is not `mut` plus a constant. `index_key` reads it off `xs[e]`, both
the `Index` expression an array uses and the `Index::index` call a `List` lowers to (the
latter was the one that mattered: no keyed place reached a `List` until it was covered).
`Place::covers` compares step by step instead of by prefix, and two element steps are apart
only when both indices are literals that differ or offsets of one local by different
constants. A keyed step anywhere else stands for `[*]`: a declared tie's rewrite pattern
matches it and drops the index, `owns_memory` and the type walks treat it as `[*]`, and the
report unkeys a place before writing it as a clause or a message.

Soundness rests on the local. One that is not `mut` cannot be assigned or lent as `&mut`,
so it changes only when its binding runs again: the next pass of a `for`, or a `let` inside
a loop. `bind` then calls `forget_index`, which turns every index keyed by that local into
`[*]`, in the state, in held values and in the break and continue states of enclosing loops.
`tests/flow/element_indices.dz` holds the cases that must stay refused, the view taken with
`i + 1` that the next pass kills as `names[i]` among them, and the ones now accepted;
`tests/run/element_indices` runs the accepted ones, clean under the sanitizers on clang++ and
g++.

## r115, T-0133 and T-0134: the generated C++ audited, and the ties check fuzzed (2026-11-04)

The two checks D-0094 asked for besides the sanitizer job, done together at the user's
request.

**T-0133, the audit.** Every operation `dz_cpp` emits and the runtime shim performs was
listed against the inputs C++ leaves undefined, with what rules each out. The table is in
`docs/testing.md`, "What the generated C++ relies on". Most rows were already covered
(T-0128, T-0129, T-0132). It found:

- **A loop with no side effects was deleted.** `while x < 10 {}` with `x` never changing, in
  a `--release` build, printed the line after it on clang++ 19 and g++ 14: C++ lets an
  optimiser assume such a loop ends. Every build now passes `-fno-finite-loops`, and §1.1
  says a loop runs as written. That is r115, with the next item.
- **Strict aliasing.** No safe code casts a pointer, but `std`'s trusted bodies read memory
  as another type: the memory tracker scans a value's bytes as `usize` words, and `TieGuard`
  reads a `T*` field as a `const void*`. Every build now passes `-fno-strict-aliasing`, which
  also defines what a pointer cast in `unsafe` reads (§10.6). Two runs of the whole-program
  benchmarks with and without both flags, on a busy machine, differed by less than their
  noise.
- **A union Dazzle calls trivially copyable that C++ did not.** One carrying a struct with a
  constructor took the path with hand-written copy and move members, which made it
  non-trivial to C++, while §5.5 calls it trivially copyable and `List` moves it with
  `memmove`. Such a union now gets C++'s own copy, move and destructor
  (`union_decl`). And every type Dazzle calls trivially copyable now gets
  `static_assert(__is_trivially_copyable(T))` after its definition
  (`assert_trivially_copyable`), so the two cannot drift apart unseen; the whole run corpus
  and the feature matrix pass it. `tests/run/plain_unions` is the regression program.
- **Null with a count of 0.** C leaves a null buffer undefined even when nothing is copied,
  and an empty `String`'s text is null. `std`'s own copies already return early; the shim's
  `panic_len`, `file_read`, `text_parse_f64`, `process_env` and `write_stream` now do too.
- **A static's field default runs code before `main`.** DZ0553 checks a static's
  initialiser as written, but not the defaults of the fields a struct literal leaves out,
  and a default may call a constructor that reads another static C++ has not built. It needs
  the user's choice between two fixes: T-0166. A field default that calls a free function of
  its own module does not compile at all: T-0167.

**T-0134, the fuzzer.** `tools/fuzz_ties.py` writes programs from fragments: a view into one of
a list of ints, of long strings or of short ones (whose text lives in the `String`, D-0095),
a `String`, a hash map, a `Box`, two `Rc`s to one list, a union's payload, two fields of a
struct, a list of lists and a map of lists; taken directly, through a call returning one, by
`if let`, `switch` or a `for` loop; then zero to three kills, of the same place or another,
directly, through a helper, through two arguments, or by a call that also reads a view; then
each view used once. Cases are batched one function each into one program, sorted by where
`dz check`'s diagnostics land, re-checked after dropping any the generator got wrong (the
ties check runs only on a program that type-checks), and run one process each under
AddressSanitizer and UBSan with `DZ_REGIONS=0`, since a region keeps freed memory mapped.
Every kill grows its container far enough to move it.

A refused program is built anyway through `DZ_FUZZ_ALLOW`, which only a debug `dz` reads
(`allow_for_fuzzing` in `dz/src/main.rs`): it demotes the listed codes to warnings, so a
release `dz` has no way to switch a check off. CI runs seed 1 at 200 cases in the
`sanitizers` job (`ties_fuzzer_finds_no_hole_under_sanitizers`, about 35 seconds).

Run with seed 4 at 1,000 cases on g++ and seed 5 at 2,000 on clang++, it found **no hole**:
all 1,889 accepted programs ran clean or panicked by design. Of 1,111 refused, 679 reached
freed memory when built anyway. Most of the rest read memory that was never freed (after
`clear()`, a union's change of variant, a long `String`'s text after the `String` moved),
which the sanitizers cannot see; what is left is the cost of two rules, one kill named by
two arguments and `nest[*]` for every element of a list of lists. T-0134 has the table.

## r116, T-0166: statics that take code are built from `main` (2026-11-04)

The user chose option B. A static is a `BuiltStatic` when its initialiser, with the defaults
of the fields its literals leave out, contains anything but literals, constants, and
aggregates and arithmetic on numbers (`static_runs_code`, conservative where unsure). Its
storage is a union with a placeholder `char`, which C++ constant-initialises with no code,
beside a state byte; `get()` builds the value in place from an `inline` function returning
it (C++17 elides the move), and panics naming the static if called while it is being built.
Each module's `dz_build_statics__` builds its own in declaration order; the root's
`dz_build_all_statics__` calls them `std` first and every module after its dependencies,
and the generated `main` and the test and bench harness call it after `process_start`, so a
value may read the program's arguments. Drops are unchanged: a `BuiltStatic` with a `drop`
joins the celled ones, and `drop_static` skips one never built. A static of constants alone
is emitted as before, which keeps std's allocators, the tracker and `RANDOM` as cheap to
read as they were. `tests/run/static_order` and `static_cycle` are the regression programs;
the C++ snapshot gained the two hooks.

## r114, T-0156: a `String` keeps short text in place (2026-11-04)

D-0095, decided by the user. The language change is in §6.7 and §14.2; what was built:

- **`String`** (`std/text/string.dz`) is two machine words, `m_store`, the length and the
  allocator: 32 bytes, as before. Text of up to 15 bytes and its zero are in `m_store`;
  longer text is on the heap or in a region, with `m_store` holding where and how much room,
  and the length's top bit saying which. It no longer wraps a `List<u8>`, so it has its own
  `Drop`, which frees only long text and still only frees (T-0118's skipped drops hold),
  and its own `Copy`, which builds where the copy is placed. Every edit ends in `set_len` or
  grows through `make_room`, and `unsafe { destroy(m_store[1]) }` there, a no-op on a word,
  is the kill of `@text` the ties check reads.
- **A tie naming plain data names those bytes** (`tied_bytes`, `plain_bytes` in
  `dz_flow/src/ties.rs`). Clauses are compared over the memory each path names, and an
  array of numbers named none, so `tie @text = m_store` could be neither killed nor
  declared. Now a tied field of plain data is memory named whole, and a kill under it is a
  kill of it. A tied reference field, as the iterators have, is unchanged.
- **A view inside an element dies with the array** (`ties/body.rs`, where a call's
  returned view gets its tie). A result tied through no pointer's storage to an input that
  lives in another value's storage, an element, also holds that storage's tie, as
  `Origin::field` already did for a field read through the element. Growing, sorting or
  swapping the list now ends `names[0].as_str()`, and a method's `in self` reference into
  an element too, which was a hole for any type.

Nothing in the repository was refused by the new rule. `words` against the same program
built before, eleven alternating runs: 162 → 160 ms on clang++ and 179 → 180 ms on g++,
the same, since in a region an allocation was already a pointer bump. Peak memory 40 →
37.9 MiB, against the C++ half's 32.3.

Tests: `tests/run/short_strings` (both forms and the line between them, every edit, copies,
a move, equality and hash across the forms, `reserve`, a list and a map of both kinds),
clean under ASan and UBSan with regions on and off; `tests/flow/view_inside_an_element.dz`
(four views refused, two fine). Changed: `element_text` in `dz_flow/tests/ties.rs`, which
pinned the old rule, is now refused; `tests/run/elements_guard` uses names longer than 15
bytes, so each owns a heap block for its guard to watch (and its first line, which had
literal `` `n `` where line breaks were, is fixed); the reflection test reaches `List<u8>`
through its own `Probe` rather than through `String`; the editor's cross-file definition
test follows `Allocator` from `string.dz`, which no longer names `List`.

## T-0146: a value that holds the one it replaces keeps its region (2026-11-04)

Compiler work; no program behaves differently, and §14.6 gains a bullet beside T-0145's.
`expr`'s parser built every node on the heap, for two reasons: an assignment's value was
never placed, and its statement loop's `let (name, tree) = parser.statement()` was a tuple
`let`, which placement did not follow. Now every allocation in the parser is placed, and
each statement's tree is built in the region of the statement loop's pass.

- **The proposer** (`dz_flow/src/placement.rs`) follows a value assigned to a whole local
  the body declares as it follows a `let`'s: a new `Escape::Held(local)` says where it
  lives, and the region is that local's block, or the `let`'s when it is further out
  (`held_target`). A tuple `let` is followed as a `let` of each part (`escape_of_all`). A
  use of a local inside the value assigned to that same local stays where it is
  (`held_by_itself`), so `lhs = Binary{ lhs: Box(move(lhs)), … }` does not count as leaving.
  Struct, union, tuple and `Box` literals hold their parts the same way for both steps
  (`holds_part`, moved out of the two copies).
- **The loop rule's second exception** (`grown_by` in `dz_flow/src/storing.rs`, beside
  T-0145's `keeps`): a value made on every pass may go into a region the loop outlives when
  it is assigned to a local that grows from itself, or is a `let` moved into such an
  assignment later in the same block with no `break` or `continue` between. A local grows
  from itself when every assignment to it holds its old value through `move`, and nothing
  else drops it or gives it away: no `&mut` lent, no `&mut self` method, no part assigned or
  moved out, no `or`, by-value `if let` or consuming `for`, and a `move` of it only into its
  own next value or a `return`. Like `keeps`, it bounds memory, not safety.
- **The region check** (`grown_each_pass` in `ties/body.rs`) accepts the same sites, and
  also asks that the local be declared outside every loop the region outlives. A tree built
  in an inner loop and returned from inside an outer one is refused, since each outer pass
  that does not return drops it.
- **What else moved**: `s = String(…)` outside a loop is now in `s`'s region, and a list
  moved to a local of an outer block is in that block's region. Both went to the heap before.

What it bought, `expr` built by the compiler before and after, fifteen alternating runs:
221 → 211 ms on clang++ and 216 → 204 ms on g++, about 5%, which is what hand-edited C++
predicted. Against the C++ half (`RESULTS.md`): 1.33× on clang++ and 1.20× on g++, from
1.40–1.50× and 1.22–1.33×. Peak memory is unchanged, 139–140 MiB: it is the program's text and tokens.

Tests: `a_value_that_holds_the_one_it_replaces_keeps_its_region` in
`dz_flow/tests/placement.rs` covers:
- a tree returned and one kept;
- one the loop also resets, and a pass that may skip the move in;
- a tree each outer pass drops, and one returned from inside an outer loop;
- a tuple `let`.

`outer` moved from the wrong placements to the placed ones, and the skipped-drop test counts
14 regions where it counted 13. `expr` gives the C++ half's checksum under ASan and UBSan.
The crates' tests and the run suite on clang++ and g++ pass, except `every_example_builds`,
whose graphical examples cannot build here (no X11, GL or ALSA headers). Those examples
check and generate their C++. A garbled doc comment on `ExprKind::Break` (backspace
characters where the backticks were) is fixed.

## T-0132: every program under the sanitizers in CI (2026-11-03)

Tooling, no language change. The evidence for §1.1's promise is every program that runs,
built under AddressSanitizer and UBSan.

- **`DZ_CXXFLAGS`** (`compile` in `dz/src/build.rs`): extra flags, split on whitespace,
  added last to the command that compiles Dazzle's code and links. Vendored libraries keep
  their recipe's flags.
- **Tests** (`dz/tests/run.rs`, both `#[ignore]`): `every_program_is_clean_under_sanitizers`
  builds every `tests/run` program with clang++ and `-fsanitize=address,undefined
  -fno-sanitize-recover=all -g -O1`, regions on and off, each a copy so it races nothing,
  and checks its output; `headless_examples_are_clean_under_sanitizers` runs
  `examples/ownership` and `examples/stb` in place. `sanitized_run` fails on any report.
  Leak checking is off. About 70 seconds on 16 cores.
- **CI**: a `sanitizers` job on Linux, fetching only the `stb` submodule.
- **What it found**: the shim's `subslice` returned `{&c[0] + lo, …}`, binding a reference
  to an element that an empty slice does not have; `String::as_str()` on an empty string
  reached it with a null pointer. It now uses `first_of(c)`, the start without a read.

## r113, T-0160 and T-0163: §1.1's promise, and statics dropped in order (2026-11-03)

D-0094 point 8. Before writing the promise, the holes it rests on were checked once more,
and one was still open: a static's `drop` at exit reading another static. Each static was a
C++ `inline` variable, which C++ destroys in reverse order of construction, an order it
does not promise across files. A safe program whose drop summed a static `List` declared
after it printed a garbage sum. `thread_local` statics were worse, since C++ destroys them
before every other static. The user chose that every static's drop runs, in a fixed order,
and that a dropped static is left at its default (asked first whether statics should never
be dropped, as in Rust, they said a drop that never runs would be a bug).

- **Storage** (`dz_cpp`): a static whose type needs a drop (`TypeTable::needs_drop`), and
  is not an imported C++ class, is a `::dz::StaticCell<T>`, a union C++ never destroys
  (`in_cell`). Its value is built in place from a lambda, since C++17 must elide that: a
  temporary moved in ran its own `drop` before `main`. A use reads `NAME.value`; the
  `ExprKind::Static` arm is the only place a static is spelt.
- **Dropping** (`drop_statics_fn`, `drop_all_statics`): each module gets
  `dz_drop_statics__()`, which calls the shim's `drop_static` on its cells in reverse order
  of declaration; `drop_static` moves the value out with `take`, so the static is at its
  default while its old value's drop runs. The root module's file defines a function calling
  every module's, in reverse post-order from the root (each module before those it uses),
  `std` last, and registers it with `atexit` from a static initialiser, so it runs after
  `main`, on `exit`, and in a library.
- **Memory tracking** keeps working: `std` is dropped last, so a program's statics free
  their memory before `TRACKER`'s drop reports leaks.
- **Spec**: §1.1 rewritten, §0.1 states the promise, §5.2 and §6.7 no longer call a stale
  `&T` the programmer's problem, §7.7's exit rule. D-0016, D-0046 and D-0094 carry notes;
  T-0011 and T-0001 now ask whether threads extend the promise.
- **Tests**: `tests/run/statics` gained `REPORT`, whose drop reads `LOG` (declared before,
  still whole) and `NOTES` (declared after, already empty). Checked warning-clean with g++
  and clang++.
- **Not covered**: a `thread_local` static on a thread C started is never dropped (T-0011).

## r112, T-0136: a `kills` clause may name a static (2026-11-03)

D-0094 point 7. The user settled three points before it was built:
- A write that kills nothing (`FRAMES += 1`) is safe with no clause.
- A static's kill ripples up callers' clauses and stops at entry points: `main`, `#[test]`
  and `#[bench]`. The user suggested a later `#[entry_point]` for functions Dazzle never
  calls (T-0162).
- A function whose clause names a static fits a function type or trait method only if its
  clause names the static too.

The program in the task, which read freed memory, is now refused at the `println!`
(`DZ0413`).

- **Syntax** (`dz_ast`, `dz_parse`): `TiePath::module` holds the modules before a static,
  so `world::SCORES@items` parses. `root_spelling` is how the dump, and so the formatter,
  writes it.
- **Resolution** (`dz_check`): a root that is not `self` or a parameter is resolved once,
  where the function is declared, by `Resolution::path_item` (`tie_statics`). It is kept on
  the signature as `FnSig::statics` and on a function type as `FnKills::Known::statics`,
  because a caller in another file reads the same clause and cannot resolve it from there.
  `Root::Static(sym)` is a new root. `changeable` accepts it, and `static_visible` refuses a
  static less visible than the function (`DZ0563`), since every caller names it.
  Comparing a function's clause with a function type's (`covered`) matches static roots by
  symbol.
- **The checker no longer asks for `unsafe` to change a static.** The seven
  `require_unsafe_static_write` sites are gone. Whether a change is allowed is now the
  ties check's question.
- **The ties walk** (`dz_flow`): a static is a pseudo-local, `LocalId(u32::MAX - sym)`,
  which no body's own locals reach. It is never declared or dropped, so it outlives every
  scope. `hir::ExprKind::Static` gives that place instead of "unknown", so a reference into a
  static is followed like one into a parameter. `Bindings::get` binds a callee's static root
  to the same place at every call.
  - Every lookup that could see the place goes through `local_ty`, `root_ty` or `static_of`.
    Two panics found in testing (`what_it_views`, the reporter's root type) were such lookups.
  - Kills of statics count against the clause unless the body is an entry point
    (`entry_point`, which reads `TypeTable::tests` and `benches`).
  - A kill inside `unsafe` still ends the body's references but is not recorded, which keeps
    std's arena and the tracker, written in `unsafe`, as they were.
  - A view of a static returned from a function is not a dropped local. It is held to the
    result's `in` like any other, so `-> &i32 in SCORES.list@items` is how one is returned.
  - The reporter spells a static as the body's file names it: `SCORES`, or
    `world::SCORES` through `Resolution::spelling_in_file`. `dz fix`'s clause compiles in
    another module.

Tests:
- `tests/flow/static_kills.dz`: undeclared kills through a push, a call and a `&mut`
  argument, and a clause that never happens. A counter and an `unsafe` kill are accepted.
- The static cases in `use_after_kill.dz` and `clean.dz`.
- `typed/`: safe writes, a clause, a ripple and an `in`. `typeerrors/`: a `pub` function
  naming a private static. `SHARED = 1` there, and
  `tests/errors/static_changed_outside_unsafe.dz`, tested the rule that is gone.
- `tests/run/statics`: its writes are out of `unsafe`, and a static `List` is grown through
  a clause.
- `a_clause_names_a_static_in_another_module`: `world::SCORES` across modules, the fix
  `dz fix` applies, and a clean check after.
- The parse and fmt corpora have module-qualified roots.

## r111, T-0135: a C or C++ call that can be handed a pointer is vouched for (2026-11-03)

D-0094 point 6, with its spelling settled by the user on the day: one recipe key, `keeps`,
whose presence is the vouching. The user asked whether the issue was not specific functions
rather than a library. It is. The library-wide part exists only so the hundreds of
pointer-taking functions that keep nothing need no list of their own.

- **Which parameters are values** (`dz_cppimport`): `Header::takes_pointer`. Numbers, enums,
  `const char*`, callbacks and handles are values, and a struct by value is one if every
  field is, checked recursively. `CType::Named` and an alias's target hold the C++ path
  (`::MiniVec2`), not the Dazzle name. The first version looked them up by name, found
  nothing, and refused `ClearBackground(Color)`, so the lookup is by `Decl::cpp`.
  `Header::function_names` is what `keeps` entries are checked against: `Decl::qualified`
  for a function, and `Type::method` for a method, which makes a constructor
  `Counter::Counter`.
- **The manifest** (`dz_project`): `keeps` is a recipe key and not a target key. `keeps =
  []` is `Some(vec![])`. `unknown_keeps` refuses a name the headers do not declare
  (`DZ0004`), once per recipe. The module carries a `Trust`. When two recipes share a header
  module, either one vouching is enough, and both `keeps` lists apply. std's libc recipe
  vouches, since its bodies are trusted already.
- **The checker** (`dz_check`): `foreign_unsafe` sets `is_unsafe` on an imported function or
  method, and `FnSig::foreign_unsafe` records why. A C++ receiver or constructor counts as a
  pointer, as the user chose. `require_unsafe_call` replaces the six `calling unsafe fn`
  sites, and is now also at the constructor call, which checked nothing before. It keeps
  `DZ0528` but writes a help that names the recipe and the key to add.

What it found: raylib and Breakout needed nothing, since every call takes values. The
examples whose recipes now have a `keeps` line are box2d, sokol, stb, sdl3 (and its glad),
miniaudio and rgfw, plus `tests/run/cppinterop`. Two functions keep a pointer and are now
called in `unsafe`, each with a comment saying why the call is sound. One is
`ma_device_init`, whose audio thread keeps the device. The other is
`RGFW_window_createSurface`, which draws from the pixels it was given; its sound teardown
leans on r110's `defer` order. sokol's and Box2D's `user_data` needs no entry, since it is
a `void*` and the example already casts to it in `unsafe`.

Tests: `a_function_taking_only_values_hands_c_no_pointer` (which of `mini.h`'s functions
take a pointer, and the names `keeps` uses), and `keeps_vouches_for_the_calls_that_take_a_pointer`
(no `keeps`, `keeps = []`, a listed value-only function, a misspelt name). `tests/run/cinterop`
gained `mini_watch`, a C function that keeps its pointer, listed in `keeps` and called in
`unsafe`. Not built: an editor quick fix offering `keeps = []`.

## r110, T-0130: a `defer` runs among its scope's drops (2026-11-03)

§6.5 and §10.5 said a `defer` ran after all of its scope's drops. Codegen never did that: its
C++ scope guard is declared where the `defer` is written, so C++ destroys it among the
locals, in reverse order of declaration. The spec now says what the compiler does. The ties
walk followed the old sentence. It replayed a scope's defers after killing its locals, and
it had removed their state first, so a pointer read in a `defer` was not checked at all. Its
scope is now one list of `Exit`s, `Drop(local)` and `Defer(stmt)`, in the order they were
declared. `block_scope` and `unwind` (a `break`, `continue` or `return`) run that list in
reverse. A jump out of a region's block now ends the region after the defers too, since the
region is declared first in its block and so ends last. The move checker only had its
comment changed: it never modelled when a `defer` runs.

So a `defer` that reads, through a pointer, a local declared after it is now `DZ0413`, and
one that reads a local declared before it is checked and accepted.

Also fixed: CI had been red since T-0126 on all three platforms. `typed/`'s new `unsafe { p }`
made the `if let` operand a block value in `check__hir_typed.snap`, and that snapshot was
not updated.

Tests: `dz_flow/tests/ties.rs`'s `a_defer_runs_among_the_drops_in_reverse_declaration_order`
covers the block's end and a `break`, reading an earlier and a later local; it fails against
the old walk. `tests/flow/use_after_kill.dz` refuses the later local and `clean.dz` accepts
the earlier one. `tests/run/ownership` now declares a local before its `defer` and prints
`drop c`, `deferred: early live=1`, `drop early`, which pins the run-time order.

## T-0126: `if let` on an untied pointer needs `unsafe` (2026-11-03)

D-0094's hole that needed no decision: r101 (T-0114) checked `or` only, so `if let &mut n =
slot.target` wrote through a pointer field with no `unsafe`. The `IfLet` arm of `dz_flow`'s
ties walk now asks what the `Or` arm asks — a pointer type `or_pointer` accepts, outside
`unsafe`, an operand not written `unsafe { … }`, and not `tied_pointer` — for every binding
form, `&v`, `&mut v` and `v`, which reads through the pointer. It reports `DZ0418`, and the
offered fix wraps the operand alone, `if let &mut n = unsafe { slot.target }`, so the
branches stay checked; wrapping the whole `if let`, as `or` is wrapped, would have vouched
for them too.

Tests: `tests/flow/untied_pointer.dz` gained a field and a parameter refused, the copy form
among them, and a vouched and a tied `if let` accepted. Two places relied on the gap and now
write `unsafe`: `tests/run/dynamic`'s `grow_through`, and `typed/`'s `if_let_forms`, whose
`p` is a pointer parameter. No example or std source used it.

## r109, T-0129: arithmetic is defined (2026-11-03)

D-0094 point 5, extending D-0063. The shim's `num` namespace gained `div`, `rem`,
`div_assign`, `rem_assign`, `shl`, `shr` and `float_to_int`, all `constexpr` so a `const`
can be built from them, and `max_of`, `min_of` and `all_ones` became `constexpr` for
`float_to_int`. `defined_op` (`dz_cpp`) decides when an integer operator needs one: a
divisor that is not a literal other than 0 and -1, a shift count that is not a literal below
the width (`isize` and `usize` counted as 32 bits), and any signed `<<`. The rest keep C++'s
own operator, so `x / 2` and `x >> 3` read as before. `/=` and `%=` pass the place by
reference, so it is evaluated once. A float `as` an integer goes through `float_to_int`,
whose bounds are powers of two and so exact in either float type.

The checker reports a constant zero divisor, folded by `const_int`, so a constant's name
counts: `DZ0580`, for `/`, `%`, `/=` and `%=` on integers only.

Tests: `arithmetic_cpp_leaves_undefined_is_defined` (`dz/tests/run.rs`) runs every row of
§5.1's table in debug and release, on clang++ and g++, with operands from calls so nothing
folds; `typed/` divides by a variable and a float by zero, `typeerrors/` by a constant zero.

**Found on the way:** the parser binds `as` tighter than prefix `-`, so `-x as u8` is
`-(x as u8)`, where §11.1 puts prefix operators above `as`.

## r108, T-0128: `or unreachable` panics in every build (2026-11-03)

D-0094 point 4, revising D-0020. The emitter (`dz_cpp`) counts the `unsafe` blocks around
what it writes, `unsafe_depth`: `block_into` adds one for a block in the body's
`unsafe_blocks`, and `in_body` starts at one for an `unsafe fn`, whose root block the HIR
leaves out since the signature says it (`owner_is_unsafe`). `or unreachable` lowers to
`::dz::unreachable(loc)` unless the depth is above zero and the build is optimised, where it
is still `__builtin_unreachable()`. The checker needed nothing: `unsafe { x or unreachable }`
was already accepted.

Tests: `overflow_wraps_and_unreachable_is_assumed_only_in_unsafe` (`dz/tests/run.rs`)
counts the assumptions in the C++ of all three builds, a block's and an `unsafe fn`'s;
`unreachable_reached_panics_in_release` is T-0128's failed search, which a release build
used to run past the end of its list and now stops at the panic.

**Unrelated, found on the way:** T-0154's fallback to a link without `-flto=auto` never
fired for LLVM's clang targeting MSVC, the case its comment names, since clang's driver says
"LTO requires -fuse-ld=lld" before any linker runs. `linker_failure` now knows that message,
so an optimised build with that clang links without LTO and says so, rather than failing.

## T-0152: regions on by default (2026-11-03)

Compiler work; no language change. §14.6 already said where each value goes; until now a
build did it only with `DZ_REGIONS` set, and the gate's verdict made that the default.
`dz_cpp::Options::regions` is `true`, and `emit_cpp` turns it off only for `DZ_REGIONS=0`,
which builds the same program with every value on the heap — the A/B side for
`tools/bench_regions.py`, which now sets the variable to `0` or `1` either way, and the
check to make when a result looks wrong. The run corpus runs on the default build;
`regions_change_nothing` runs it again on the heap, and both builds' C++ is compiled under
`-Wall -Wextra` on clang++ and g++. The editor's placement hint no longer says a build
ignores it.

## T-0151: a region's allocation called directly (2026-11-03)

Compiler work, no language change. A container reaches its allocator through the
`Allocator` trait object, so a region's pointer bump sat behind an indirect call. With
regions on, `Allocator__dyn`'s methods now check for `ScopeArena`'s vtable and call its
implementation directly, and `ScopeArena::alloc` is cut to the bump (the rest is
`alloc_slow`), so the C++ compiler inlines it within `std`. Measured against the compiler
before it, fastest of twenty alternating runs: 2–6% faster on clang++, `expr` 8% faster
on g++, `astar` and `words` about 1% slower on g++. The indirect call was a small part of
the cost; the rest is listed in T-0151.

## T-0117: the gate's verdict, regions stay (2026-11-02)

Measurement only; no code changed. `tools/bench_regions.py` with r105 in, release, ten
alternating runs, clang 18 and g++ 13 on a 4-thread cloud VM, with the pre-r104 hand
build from `39680ee`. `astar` and `words` are faster than the heap on both compilers, and
`words` peaks 9% under the heap and 43% under hand-placed `Temp`; `expr` is even, its
parser nodes kept on the heap by the loop rule. Peak memory is within 1% elsewhere, the
loop is flat, and the 18 tests in `dz_flow/tests/placement.rs` pass. A run before r105
had speed even with the heap on every row; the copies and maps r105 placed are what made
`words` faster. The user kept regions. The tables are in T-0117.

## r105, T-0140 and T-0149: copies placed, and maps filled in a loop (2026-11-02)

Asked for by the user before T-0117's gate, since both change what it measures: `words`
spends its allocations on copies of a buffer inserted as keys, and `expr` on names inserted
into a map in a loop. Behind `DZ_REGIONS`, as all placement is. One language change: r105,
`#placement` in a `copy` is where the copy goes, rather than the heap (noted in D-0091).

- **T-0149, a map filled under a failed lookup.** `HashMap::insert` may drop what it
  replaces, so in a loop it kept its values on the heap (the loop rule). An insert in the
  `else` of an `if let` on `get_mut_str(key)`, or under `!contains`, with the same map and
  a key read from the same local, replaces nothing: `adds_new` in `dz_flow/src/storing.rs`
  says so to the proposer, to `keeps` and to the check alike. `format!`'s text pushed in a
  loop is stored directly now (`formatted_by`, shared by both sides), and a program's own
  container whose clause kills a tie other than `@items` keeps its elements when the tie's
  declaration, made canonical, reaches none (`tie_spares_elements` in `dz_flow/src/ties.rs`).
  None of it is a safety rule; the check still follows every value.
- **Found while building it**: the check's "this site was stored into a container" was one
  flag per site, so on a loop's second pass `format!`'s `Formatter` borrowing `&mut text`
  looked like lending a container with stored values in it. It is now per container.
- **T-0149's second row is not built.** Building a value with the container's own
  allocator in a function it is lent to is right only while no element leaves the container
  for somewhere older, and a valid `give(from: &mut List<String>, store)` that pops and
  stores elsewhere breaks it, out of the check's sight. It is T-0150, a design question for
  the user: a language rule on what `pop` returns, or a whole-module summary.
- **T-0140, a copy is a site.** Where a copy of a type that can be copied into a placement
  is taken by value (a `let`, an argument, a literal's part, a non-local `return`, a field of
  `self` in a `copy`), it is decided like any other site, following the copy's own value.
  `placed_copy_kind` says which structs: a `copy` of their own (`List`, `HashMap`), which
  becomes a result function reading `#placement`, or none and no `drop`, copied field by
  field (`String`, `Path`, a program's struct of them). C++ gets `T(::dz::copy_tag, const T&,
  Allocator__dyn)`; the plain copy constructor delegates to it with the heap, so copy
  assignment and every copy C++ makes on its own stay on the heap, as before. `List::copy`
  and `HashMap::copy` build from `#placement` instead of the original's allocator, which
  retires the `Allocator::scoped` guard they used.
- **The check** gives a copy site its own region in `passed`, refuses it in `sink`, and
  refuses one the walk never reached, so a copy is never placed unchecked. A copy is not
  "built whole" for T-0118, since a copied `List<String>`'s elements are still copied by C++
  on the heap: its drop is kept.
- **Found on the way**: a struct with a `copy` and no `drop` never compiled. Its header
  defaulted the move members that `struct_bodies` defined; the header now declares them,
  as §6.4's reset needs. Nothing had one until `tests/run/regions`' `Named`.
- **Where it lands**: `words` places 7 of its 8 allocations (both copies, the key and the
  word kept in each entry, in `main`'s region), `expr` 5 of 21 (its names). Both print the
  same with regions on, leak-free under `--track-memory`.

Tests: `a_map_filled_under_a_failed_lookup_keeps_what_it_is_given`,
`a_copy_is_built_where_it_is_placed` and `which_copies_can_be_placed` in
`dz_flow/tests/placement.rs`, and the copies in the existing cases' expectations; in
`tests/run/regions`, `tallied`, `labelled`, `lend_copy`, `named_of` and `named_copy`.

## T-0145: a value stored into a container lives where the container lives (2026-11-02)

Compiler work; no language change. Spec §14.6 now says what it does. Asked for by the user
right after T-0118, for the case it could not reach: a `List<String>` whose strings are in its
region, so freeing the region frees them and the drop need not walk them.

- **What a method does with what it is given** (`dz_flow/src/storing.rs`), inferred from
  bodies to a fixed point: a by-value parameter is *stored* when every use borrows, copies,
  drops or moves it into `self` (`construct` into memory reached from a field, a field
  assignment, or a stored parameter of a method on `self` or a field), and *kept* when that
  move runs on every call. `List::push` and `insert` and `SlotMap::insert` keep;
  `HashMap::insert` and `get_or_insert` and `HashSet::insert` store but may drop. std's
  summaries hold from any module, a program's own within its module.
- **The proposer** places a value passed to a stored parameter where the receiver lives (a
  local's block, or the result placement when the container is returned), unless something
  taken back out of the container goes where the region cannot follow, or the container is
  lent as `&mut`. In a loop, only a value stored at once by a keeping method into a container
  that nothing takes from, drops from or lends (`keeps`): the loop rule bounds memory, and the
  region then grows no faster than the container.
- **The region check** follows the value into the container's local, and back out: a `&mut
  self` method returning something that owns memory carries what was stored, `&mut` of the
  container loses it. The corpus test caught the proposer placing a string later popped
  into a longer-lived store; `lets_go` now follows what is taken out.
- **Skipped drops for a container of owning elements** (T-0118's second half): a
  `List<String>` or `HashMap<i32, String>` skips its drop when every `&mut self` call on it
  adds only what it was given (`resize` and `Extend::extend` add their own), hands back
  nothing that writes an element (`xs[i] =`, `iter_mut`, `get_or_insert`), and is given only
  sites built whole in its region. The leak check over `tests/run` found one wrong skip while
  this was built: a list passed without `move` is a copy, on the heap, and a map holding it
  as a key skipped its drop. A bare local now counts as a copy.
- **Not yet**: `words`' keys are copies (T-0140); `expr` and most programs fill maps in a
  loop, where `insert` may drop what it replaces; a container lent to a function; `format!`
  text pushed in a loop. All in T-0149. The user's idea of a checked marker on a type whose
  drop only frees, so a reader can see it, is T-0148, for later.

Tests: `std_containers_store_what_they_are_given`,
`a_value_stored_into_a_container_is_built_where_it_lives` and the skip test's new cases in
`dz_flow/tests/placement.rs`; `tests/run/regions` gains `filled`, `popped`, `drained` and
`counted`, the same with regions on and off and warning-clean on both compilers. Every
`tests/run` program ran with regions on and `--track-memory`: no leaks. The crates' tests,
clippy and fmt pass; `dz_check`'s typed-tree snapshot of std was stale since T-0118's
`HashMap` drop and is accepted here.

## T-0118: drops a region makes unnecessary (2026-11-02)

Compiler work; no language change, since a drop that only frees does nothing a program can
see. Step 5 of D-0091's plan. Spec §14.6 now says what it does, and its "not yet" note
lists what it leaves (T-0147).

- **A drop that only frees**, proven per type instance from the drop bodies (r90): every
  `drop` in the type, its bases' and its parts', with the instance's types put in, does
  nothing but read, compute, write a local or a field of the value being dropped, count with
  `for i in 0..n`, call `Allocator::free` and `destroy` parts whose drops only free. An `if`
  of type questions is answered for the instance. `Box`'s `m_drop` is followed as the drop of
  `T` and of any type deriving from it. r90's examples all hold: `List<String>`, `Box<String>`,
  `String` do; `File`, `Rc`, `Formatter` and `List<File>` do not.
- **`HashMap`'s drop** called `clear`, which walks every slot writing its hash before the
  arrays are freed. It now destroys entries only when `K::has_drop || V::has_drop`, so a map
  of plain values is freed without a pass over its slots, with regions on or off.
- **Everything the value owns is in the region**, the half r90 did not settle. Placement says
  where a value was built, not where its parts came from: a region's `List<String>` holds
  strings built on the heap, and a region's `String` can be replaced by one built elsewhere,
  so skipping either drop would leak. A local skips only when it was built whole in its
  block's region (by a function that builds all it owns in its placement, worked out as the
  smallest set closed under the rules in `whole_functions`), owns nothing from elsewhere (a
  container whose elements own nothing, or a struct of such containers whose own methods
  never replace them), and is never replaced (no assignment to it or an owning part, no
  `&mut` handed on but to its own methods and a `for`). The proof answers "no" when unsure:
  a wrong "no" keeps a drop, a wrong "yes" would leak.
- **Codegen** puts such a local in storage C++ never destroys, `alignas(T) unsigned char
  _dz_keptN[sizeof(T)]`, built with placement `new`, and binds the name as a `T&` to it, so
  nothing else in the emitter changes. The block's region is declared first, so it is
  released after the block's last use of the value.
- **Shown**: the hint and `dz placements` append "; drop skipped" (`Site::label`), and the JSON
  has `drop_skipped`.
- **Reach**: 15 of the 38 sites in a region in `tests/run/regions`; `astar`'s queue, maps and
  path, and `words`' text and buffer. Left for T-0147: a value lent as `&mut`, one built by a
  trait method, a function value or `format!`, and a local in a generic body. A container of
  owning elements waits for T-0145.

Tests: `a_drop_only_frees_when_its_body_does_nothing_but_free` (r90's examples) and
`a_drop_is_skipped_only_when_its_region_frees_everything` (six skipped, seven kept, each kept
one in a region all the same) in `dz_flow/tests/placement.rs`; the placement hint and
`dz placements` tests read the new label. Every `tests/run` program was also run with regions
on and `--track-memory`: no leaks, the same output as with regions off. Not timed: the
machine was busy.

## r104: no allocator in ordinary code, and `#placement` (2026-11-02)

The language change is r104 (T-0119, step 6 of D-0091's plan), with one question settled by
the user on the way: a container learns where it was placed from an intrinsic,
`#placement`, readable in any `unsafe` code rather than in `std` alone, so a program's own
container is placed as `std`'s are. Recorded in D-0091.

- **std's constructors take no allocator.** `List`, `HashMap`, `HashSet`, `SlotMap`,
  `String` (and `String::from`, `String::from_utf8`), `Path`, `Box` and `Rc` lost their
  `allocator` parameter. `List`, `HashMap` and `Box` store `unsafe { #placement }`; the
  rest build through them, and `Rc` allocates from `Allocators::Global`, since a `Weak` can
  outlive any scope. Each constructor whose struct keeps an allocator already took a hidden
  placement by T-0117's round-3 rule, so `std`'s containers joined result placement
  unchanged: a site is now a call to a function that takes a placement (`result_functions`),
  plus `Rc(…)`, shown as heap, and a call to the program's own function that takes an
  allocator parameter left at its default, also heap.
- **`#placement`** (`dz_check`): a `^mut Allocator`, `DZ0528` outside `unsafe`, and
  `DZ0579`, new, as a parameter's default, where each caller would fill in its own. Codegen
  writes `_dz_result` in a body that takes a placement and `Allocators::Global()` anywhere
  else, so with regions off every container is on the heap, as before. `placement` joins
  `INTRINSICS`, which reserves the name and offers it after `#` in the editor.
- **`format!`** takes nothing by name: its `allocator =` and the lowering that passed it to
  `String`'s constructor are gone, and passing one says the compiler places the text.
- **What changed in the corpora.** The benchmarks name no allocator: `astar`'s `MinHeap`,
  `words`' text, map, buffer and list, and `expr`'s `Parser`, its `Box`es and the
  `Temp.reset()` between statements. `tests/run/containers`, `hashmap` and `formatting`
  build their scratch without `Temp` (`formatting`'s `#location` lines moved up one);
  `collections` lost the counting allocator that proved a lookup by `str` builds no
  `String`, since nothing can be given one now; `generics` and `iterator` have their own
  `Buffer` read `#placement`; `region_arena` lets the compiler put its containers in a
  region, so the copy guard is exercised by `regions_change_nothing`, with regions on.
- **`tools/bench_regions.py`** builds each benchmark as written, and with `--hand-src` and
  `--hand-dz` a third build from an older checkout, with its scratch in `Temp`.
- **What it cost**, measured against that hand-placed build as T-0104 measured (the table
  is in T-0119): `astar` nothing; `words` 9–15% and `expr` 5–7% slower than with `Temp`,
  regions even with the heap on both, since placement does not reach their hot allocations
  yet (T-0145, T-0146). `words` peaks at 46 MiB where `Temp` took 70. The user chose to take
  allocators out first and record the loss.

Tests: `typed/` builds a container of its own with `#placement`; `typeerrors/` reads one
outside `unsafe` (`DZ0528`), takes one as a default (`DZ0579`) and passes `format!` an
`allocator` (`DZ0558`); `dz explain DZ0579` has its wrong and right programs; the
placement tests count the hidden parameter where they counted the allocator's index. The
crates' tests, the run suite but `every_example_builds` (no vendor submodules here), clippy
and fmt pass.

## Frame regions not needed yet; two placement gaps closed instead (2026-11-02)

Compiler work; no language change. Before designing frame regions (T-0139), a frame loop
was run through `dz placements`. Its own motivating case, `let visible = self.cull(…)` in
the function called each frame, was already in that function's region: scope regions
open one per call, and result placement carries it down to the callees. So a frame region
adds no new place for values to go. What kept per-frame values on the heap was three
narrower gaps. Two are closed here, and the third, a field replaced every frame, joins
T-0120.

- **`format!`'s text** goes where the macro's value goes. The `String` is built inside the
  macro's body, which C++ gets as a lambda and where no region can be declared, so it was
  always heap: every HUD line of every frame. The proposer now places it as the
  `Formatted` expression itself (`formatted_text`); the lambda captures by reference, so it
  reaches the region or `_dz_result` outside it. The check needed nothing: it already
  followed a block's value out.
- **A value passed by value to a parameter that stays in the call** keeps its caller's
  region (`Staying`, `staying_params`). Such a parameter is one the callee only reads,
  borrows, copies or drops, or hands to another such parameter. Every by-value parameter
  that keeps an allocator is claimed. A claim is withdrawn when the proposer sees the
  parameter leave, or when the check refuses it, checking the parameter as a region of the
  caller's that outlives the body but cannot be returned, stored or passed on. This repeats
  until nothing changes, so recursion and chains are covered. Only direct calls within one
  module rely on it (T-0142 is the rest), and nothing changes in how a call is made: the
  callee frees into the caller's region when it drops the value.

Tests: `a_formatted_string_goes_where_the_macro_value_goes` and
`a_parameter_that_stays_in_the_call_takes_the_callers_region`, each also with everything
proposed, so the check alone refuses what it must; `give` moved to the placed cases and
`handed` (a parameter that is kept) replaced it among the wrong ones. `tests/run/regions`
gains both, 63 sites, 38 in a region, and prints the same both ways and under
`--track-memory`.

## T-0131: memory tracking overwrites what is freed (2026-11-02)

Compiler and std work; no language change. Under `--track-memory`, every block the heap,
`Allocators::Temp` or a region frees is filled with `0xDD`, and so is everything a region
held when it ends, so a read through a dangling reference sees garbage, not a plausible old
value. Regions needed it most: a small block freed in one stays in its chunk, holding its
old contents, until the region ends, and a wrong placement is a read of released memory.
A region's large block keeps its size in its header again (32 bytes) to be overwritten at
the region's end.

Tests: `a_tracked_build_poisons_what_it_frees`. `tests/run/region_arena` lost its two
checks that a freed large block comes back at the same address: since the block became the
heap's, they held only by chance of glibc, and would not on every host.

## r103: three holes closed, and a drop counted as a use (2026-11-02)

The language change is r103: D-0094 points 1–3, decided as proposed that day (point 2 as
option A). T-0127 enforces an existing rule. Every program D-0094 lists for these four now
fails to compile, where it read freed memory when it ran.

- **A call may reach what it kills through one argument only** (T-0123, `DZ0420`).
  `invoke`'s kill loop collects, per kill, the inputs the clause names it through and the
  other reference inputs whose target covers it. One naming and another reaching it, or two
  naming it, is refused when the killed place owns memory (`owns_memory`), so
  `rebuild(&mut world, &world.score)` stays fine. An argument pointing *into* the memory
  stays `DZ0413`, as before.
- **Every `Rc<T>` is one place** (T-0124). A guard of an `Rc<T>` has its target re-rooted
  at the body's first guard local of that type. The walk never drops a guard, so the place
  lasts the whole body. A guard local stands for the value it guards (`root_ty`), and a
  diagnostic shows it as its type, with a note.
- **A `Box`'s value is in the box** (T-0125). `Box` declares `tie @value = m_ptr[*]`. D-0094
  wrote `m_ptr`, which names the same memory, but a tie ending at a pointer cannot be
  followed further, so the clause a function changing the boxed value needs,
  `kills h.node@value.name@text`, could not have been written. A guard of a `Box` named by a
  place gets a view built as `&xs[i]`'s is: target `@value`, tie the allocation.
- **A drop is a use of a value's views** (T-0127): at a scope's end, on a jump out, and
  when assigned over, when the value's type needs a drop and holds views. A moved-out local
  no longer counts as dropped.
- **The criterion programs** `tests/run/button` (M8) and `tests/run/machine` (M10) handed
  the button to its own state's `step`, which kills that state: `DZ0420`. Safe as written,
  since no state touches `btn.state`, but `step` is checked as if `btn` and the state were
  apart, and a body that did would dangle. Each now steps a copy of the state and writes it
  back, which prints the same.

Tests: one `tests/flow` file per rule (`aliased_kill`, `rc_shared_value`,
`box_value_tie`, `view_struct_drop_reads`), and `DZ0420` in `dz explain`. All examples,
`tests/run` and the bench programs check clean.

## T-0117, stage 1's peak-memory bug: large blocks are the heap's (2026-11-02)

Compiler work; no language change. Behind `DZ_REGIONS`.

- **The cause**: `ScopePool` kept every large block a region freed, by power-of-two size
  class, with no limit. A growing map left every class it grew through in the pool, and
  each block was rounded up to its class. That came to a third more peak memory than the
  heap on `astar`, and 8 MiB more on `expr`. With large blocks straight from the heap,
  peak memory matched the heap's in all three programs.
- **The fix**: a region's large block (16 KiB or more) is the heap's, at its exact size,
  freed the moment it is freed. The pool keeps chunks only. A byte budget on the pool was
  tried at four sizes: it kept part of the memory and none of the speed.
- **What it cost**: `astar`'s 3–7% edge over the heap. It had been the pool's, paid for in
  memory, so the stage 1 checkpoint's speed row now fails for `astar` (even with the heap).
  The numbers are in T-0117.
- **Found on the way**: a constructor's heap overload, and `result_adapter`, cast a
  by-reference parameter as `static_cast<T&&&>`, so `astar` did not compile with regions
  on. Both now pass references on as they are (`forward`).

Tests: `tests/run/regions` gains a constructor taking `&List<i32>`; the typed HIR snapshot
loses the pool's size classes.

## T-0117, stage 2, round 3: owned parts live where their value lives (2026-11-01)

Compiler work; no language change. Behind `DZ_REGIONS`.

- **The proposer follows a value through what holds it**: `move`, `or`, struct and union
  literals, tuples, arrays and a `Box`'s contents, and into the local a `let` moves it to.
  One function, `fate`, now answers "stays, returned, or leaves" for every use, where the
  old one answered only "leaves" and the return case was special-cased beside it. A
  `move` is never a copy: treating `move(text)` of a copyable `String` as one sent
  `expr`'s `generate` back to the heap until it was fixed.
- **The check carries a `Box`'s contents** into the box's value.
- **Constructors** take a placement for the value they make and build their fields
  there; the check lets a result site into a field of `self` and holds it in `self`. Each
  also gets a heap overload that delegates, so constructions that pass no placement — the
  default constructor, a base initialiser — still compile.
- **`copy` is left**: C++'s copy constructor cannot take a placement, so it would need a
  placed copy per type. Copies stay on the heap, correct as before.
- Found T-0122: a constructor cannot declare a `kills` clause.

Tests: `owned_parts_live_where_their_value_lives`, `a_constructor_builds_where_its_value_is_placed`;
`tests/run/regions` gains constructors, a literal's fields and a box's contents.

## T-0117, stage 2, round 2: trait methods, function values, and results through `or` (2026-11-01)

Compiler work; no language change. Behind `DZ_REGIONS`.

- **Through `or`**: `let x = make() or …` and `return make() or …` are placed as the
  `or`'s value is used; the check already followed values through `or`.
- **Trait methods** take a placement by the trait's own signature, so every
  implementation, the defaults copied into them, the vtable slot and a call through a
  bound or an object agree. A trait method returning `Self` takes none.
- **Function types** with a result that keeps an allocator pass one, so a function value is
  called like a function. A function named as a value that takes none itself (a generic
  `-> T`, one with its own allocator parameter) is wrapped in a capture-less adapter. This
  removes round 1's "never used as a value" rule; only `virtual` methods are left out.
- Found on the way: T-0121, a local hiding a free function of the same name in the C++,
  with regions off too. And `dz_placements_prints_each_site` had been failing since round
  1's commit, whose last fixture edit moved its lines.

Tests: `tests/run/regions` gains each path (a trait's method directly, through a bound and
through an object, a default method, a function value passed and called, the adapter);
`a_returned_value_is_built_where_the_caller_keeps_it` covers `or`, trait implementations
and function values.

## T-0117, stage 2, round 1: a returned value built where the caller keeps it (2026-11-01)

Compiler work; no language change (§14.6 already says it). Behind `DZ_REGIONS`, as stage 1.

- **A result function** takes a hidden last parameter, `_dz_result`, its caller's choice of
  where the result is built. It is a free function or a non-virtual struct or union method
  whose result keeps an allocator, that takes no allocator of its own and is never used as
  a function value. That is decided from signatures alone (`result_functions`), so the
  declaration and every call agree, and codegen reads it from `Placements::takes_result`.
  The first version keyed on `needs_drop`, which gave every `println!` a placement through
  `Formatter`; "keeps an allocator, through its fields" is the property that matters. A
  bare `T` result does not count, since `pop` and `get` build nothing.
- **Inside one**, a site whose value is returned gets `Decision::Result` and is built in
  `_dz_result`. The region check models the placement as a region of the outermost block
  that its end does not release and that `return` may take, so every other escape is still
  refused, including a value made on every pass of a loop.
- **At a call**, the call is a site like stage 1's: the caller's region when it keeps the
  result and only borrows it, its own placement when it returns it, the heap otherwise.
- The split of `local_names`' doc comment by `region_name`, from stage 1's round 2, is
  repaired.

Tests: `a_returned_value_is_built_where_the_caller_keeps_it` in `dz_flow/tests/placement.rs`;
`regions_change_nothing` with the hidden parameter on every std and program result
function; `tests/run/regions` now shows it (`evens`, `squares`, `more_squares`).

## T-0117, stage 1, round 2: codegen behind `DZ_REGIONS`, and each placement shown (2026-11-01)

Compiler work; no language change. With `DZ_REGIONS` unset, every program builds exactly as
before.

- **Codegen reads the placements** when `dz_cpp::Options::regions` is on, which `dz` sets
  from `DZ_REGIONS` (any value but `0`) for every command that emits C++, so
  `tools/bench_regions.py` builds one source both ways. `emit` is handed
  `Analysis::placements`; a body's are found by its owner, which an instance from `dz_mono`
  shares with its generic body. A block with a region declares `ScopeArena _dz_regionN{}`
  first, so C++ releases it after the block's own locals, and a placed call passes
  `::dz::addr(_dz_regionN)` where its default allocator went.
- **Round 1's two codegen hazards do not arise**: `List`'s C++ copy constructor and copy
  assignment both go through `List::copy` (and `String`, `Path`, `HashSet`, `SlotMap` copy
  through `List` or `HashMap`), and codegen turns no last use into a C++ move.
- **Each placement is shown**, from one wording, `Decision::label`: an inlay hint after
  each site (`HintKind::Placement`), and `dz placements <dir>`, a line per site in the
  project's own files and a count, or `--format=json`. `serde_json` became a normal
  dependency of `dz`, which already built it for the language server.

Tests: `regions_change_nothing` runs every `tests/run` program with regions on against its
`expected.txt`, and compiles the region C++ warning-clean on clang++ and g++;
`tests/run/regions` is the prototype's case list less its stage 2, and places 13 of its 24
sites; `inlay_hints_show_where_each_allocation_goes` and `dz_placements_prints_each_site`.
The run fixtures' warning-clean list is one constant, `WARNING_CLEAN`, shared by both tests.

## T-0117, stage 1, round 1: the region arena, and where each allocation goes (2026-10-31)

Compiler and std work; no language change, and no program runs differently yet. The user
set the order: scope regions alone, behind a go/no-go gate (T-0117), and the work stopped
after this round by the user's choice.

- **The arena** is back from the tag `regions-prototype`: `ScopeArena` and `ScopePool`
  (`std/memory/`), brought up to the ties rules. A large block that is outgrown no longer
  grows in place, which the prototype allowed by checking only the new size.
- **Copies go to the heap.** `Allocator::scoped` is new, and `List::copy` and
  `HashMap::copy` read it; everything else in std that copies goes through those two.
- **Placement is decided in analysis**, per body, so it can never disagree between the
  editor, `dz placements` and codegen: `dz_flow::placement` proposes by the prototype's
  stage-1 rule, and the tie walk, run again with each region as a tie that dies at the end
  of its block, refuses what it cannot follow. The two share no rule; over the corpus the
  check refuses nothing the proposer places, and with every site proposed it alone refuses
  each wrong case in the tests. `Analysis::placements` holds the answer, and `--timings`
  gained a `placement` phase.
- **The gate's stopwatch** is `tools/bench_regions.py`, ready for when codegen reads
  `DZ_REGIONS`.

Tests: `dz_flow/tests/placement.rs` and `tests/run/region_arena`. What is left, and the
question the gate raises, are in T-0117.

## r102: `[*]` checked too, with `check_trusted` (2026-10-31)

T-0116's second half, opt-in by the user's choice: walking every element on each call makes
a loop of `push` quadratic, which only a test run can afford.

- **The option.** `[build] check_trusted = true` (`BuildConfig::check_trusted`), which
  turns `TRACK_MEMORY` on (`dz_driver/src/cfg.rs`).
- **Numbers.** The tracker numbers each allocation (`Tracker::m_seq`, `Allocation::seq`).
- **Which blocks.** No per-type walk was needed, nor new instances from `dz_mono`, which
  enumerating elements through `Iterable` would have taken: `Tracker::reach` reads the
  input's bytes and every block found as words, and a word equal to a live block's start is
  a block reached. The container's own arrays, the targets of its tied pointer fields, are
  left out, since `@items` is the pointer guard's.
- **What is checked.** Physically: every block found on entry must still be the same live
  allocation on the way out. An element moved within the array, or out to another owner as
  `append` does, keeps its blocks; one dropped, or replaced, frees them. So `append` needs no
  `kills other[*]`, which a reachability check would have asked for.
- **Codegen.** `tie_guards` emits an `ElementsGuard` per `&mut` input with tied pointer
  fields whose clause kills neither `[*]` nor the input whole, passing it the tracker's two
  functions by their qualified names, which only the emitter knows.

Conservative: a word that happens to equal a block's start, or a raw pointer into an
unrelated block freed meanwhile, could trap where nothing is wrong. Nothing in the eleven
programs run with it does. Tests: `tests/run/elements_guard`, and
`std_clauses_hold_under_check_trusted`.

## T-0116, first part: std's trusted clauses checked in debug builds (2026-10-31)

Compiler and runtime work; no language change. r90 settled the shape: a view carries
nothing, and a body is checked at its edges. This does the pointer-field half.

- **Which ties.** `tie_guards` (`dz_cpp/src/lib.rs`), called at the top of every emitted
  body in a debug build: for each `&mut` input, `&mut self` included, whose struct has a
  pointer field standing for a tie (`FieldSig::tie`), and for each such tie the `kills`
  clause leaves out (a path rooted at the input that is the input whole, the tie, or the
  field, counts; `[*]` does not), one guard. A function whose clause is unknown gets none.
- **The guard.** `::dz::TieGuard` remembers the field's value and compares it in its
  destructor, which C++ runs on every way out, after the result is built; a change calls
  `tie_killed`, which prints the function and the tie and aborts, as a panic does.
- **Where.** Every such function, not only the bodies `dz_flow` marks trusted: the trusted
  marker is the analysis's, which codegen does not see, and a compare per call is nothing in
  a debug build. Release builds emit none.

Nothing in std tripped one over the whole corpus, so its clauses over `m_data`, `m_hashes`
and the like hold where they are exercised. Not done: `[*]`, the blocks elements own, and
an allocation sequence number so a block freed and reused at the same address is not
taken for unmoved.

Tests: `tests/run/tie_guard`, a container whose `push_wrong` grows its array without a
clause, expected to trap.

## r101: an untied pointer becomes a reference only in `unsafe` (2026-10-31)

T-0114, D-0093's last step. The check is `dz_flow`'s, as r90 settled, since only the ties
walk knows where a pointer came from.

- **What `unsafe` covers.** The HIR had no trace of `unsafe { … }`: the checker counted its
  depth and lowered it to a plain block. `Body::unsafe_blocks` now lists them
  (`BodyBuilder::mark_unsafe`), and the walk keeps an `unsafe_depth`, starting at one in an
  `unsafe fn`. An expression that *is* such a block is `vouched`.
- **Tied.** `tied_pointer`: the value came from a safe call under a declared or elided tie
  (`Value::vouched`, set at the end of `invoke` from the signature alone, since the callee's
  body is held to the tie whatever was known of the arguments; not for an `extern fn` or an
  `unsafe fn`), or it views known places, none a pointer parameter. Otherwise `p or …` on an
  `or_pointer` type outside `unsafe` is `DZ0418`.
- **The laundering hole** found at the start of this session: a `return` of a pointer whose
  provenance is unknown, under the signature's tie, outside `unsafe`, is `DZ0419`. It was
  only an `unchecked` note before, which let `world_of(&ctx) or return` look tied.
- **Both** carry `unsafe { … }` as a suggestion: an editor offers it, `dz fix` never adds
  it, since vouching is the author's call.
- **On the way:** `str::from_utf8` now views its bytes in the walk, so `str::from_utf8(b) or
  …` is tied.

std needed 27 conversions in `unsafe`, every one over its own allocator, header or value
pointer fields (`List`, `HashMap`, `Box`, `Rc`, `Weak`, `TempArena`, `Unguarded`, `Heap`);
the corpora 19, mostly C callbacks' user data, `examples/ownership`'s `m_allocator` (the two
r90 counted) and test allocators. They were applied from the suggestions by a scratch
script, std first with a rebuild between rounds.

Tests: `tests/flow/untied_pointer.dz` (both codes, and the tied and vouched forms that pass),
`dz explain DZ0418` and `DZ0419`.

## r100: a C string is borrowed (2026-10-31)

T-0112, D-0093's fifth step. `cstr` was a view with no tie, kept anywhere; it is now three
types, one C++ `const char*`. `Ty::CStr` is `&cstr`, a view (`placed_as_view`); the new
`Ty::ConstCStr` is a literal's `const &cstr`, read as a `&cstr` where none is wanted; the
new `Ty::RawCStr` is C's `^cstr`, which takes either by assignability and is what
`foreign_ty` gives a C `const char*`. `implicit_as` converts a `String` or `Path` to
`&cstr` where a `^cstr` is wanted. `or` on a `^cstr` is a null check giving the `&cstr`
(`or_expr`, and a C++ arm that does not dereference); `null` empties one; `^cstr as &str`
is `unsafe`. `DZ0578` respells the old forms (`cstr_spelling`).

`untied_result` ties a `&cstr` result and no longer exempts an `extern fn` with no input,
so std's `dir_next` returns `^str` (its runtime `Opt<str>`, `null` at the end, read in
`unsafe` by `Directory::list`) and `process_arg` `const &str`, valid for the whole run. A
`^str` or `^[]T` result stays exempt: it is a pointer. `SourceLocation`'s `file` and
`function` are `const &cstr`, as are the memory tracker's.

The migration was r99's script with `DZ0578`, now skipping `typeerrors/` so its deliberate
mistakes stay; its older `cstr` spellings were respelled apart. `examples/sdl3` and
`tests/run/cinterop` read C's error and name text in `unsafe`. Not done: a C++
`std::string_view`, which the importer does not map yet.

Tests: `typed/`'s `c_kinds`, `typeerrors/`' C string block, `dz explain DZ0578`.

## r99: slices are borrowed (2026-10-31)

T-0110, D-0093's fourth step, built as r98 was. `lower_type_inner` reads `&[]T` and `&mut
[]T` as `Ty::Slice`, `^[]T` as an optional slice and `const &[]T` as the new
`Ty::ConstSlice`; the old forms are `DZ0577` (`slice_spelling`, whose edit is offered only
where the type is written out in the file). `ConstSlice` is a read-only slice to C++, is
assignable to `&[]T` and read as one wherever it is not wanted (`expr`, `field_type`), and
is no view. `placed_as_view` counts a slice. `view_pointer` lets `null` empty a `^str` or a
`^[]T` and compare with one. A `&mut []T` parameter is a mutable input to `kills`
(`dz_check::ties`, `dz_flow::ties::mutable_input`, `FnKills::kills_within`): what was a
`[]mut T` by value is now a borrow, and may kill through its elements.

The migration was r98's script with `DZ0577` added: 69 edits in std, 58 in the corpora.
Reflection's three tables became `const &[]`, and reflect's `to_json` takes a `view T`,
since the test serialises a slice. Rust snippets, `explain` and the spec were respelled by
pattern, leaving macro syntax (`$xs: []K`) alone. One slip, caught by the snapshot: the
script also "fixed" `typeerrors/`' deliberate `DZ0575` cases, which were put back.

Tests: `typed/`'s `slice_kinds`, `typeerrors/`' bare and stored slices, `dz explain DZ0577`.

## r98: text is borrowed (2026-10-31)

T-0109, D-0093's third step. The language change is a spelling and a placement; the
representation did not move.

- **Lowering** (`lower_type_inner`): `&str` is `Ty::Str`, `const &str` `Ty::ConstStr`,
  `^str` `Ty::Optional(Str)`. The old spellings and `&mut str` are `DZ0575` with a certain
  fix, `(&T)?` (a one-element tuple to the parser, read through `grouped`) is `DZ0576`
  respelled `^T`. `written_fix` attaches an edit only where the source says what the
  message does, so a macro's expansion, whose tokens sit at its invocation, gets none.
- **Display** (`type_name_at`): `&str`, `const &str`, `^str`; every message that said `const
  str` says `const &str`.
- **`null` and `^str`**: `null` where a `^str` is wanted is its empty case, `x == null`
  compares as `x == none` did, and `none` for one is `DZ0575`.
- **Placement**: `placed_as_view(Ty::Str)`, so `DZ0517` covers a text field, element, type
  argument, static and union payload; `check_view_arg` exempts a trait's arguments.
- **std**: `impl ImplicitAs<&str> for String`; `find_by<view Q: Copy>` takes the key itself;
  `key_hash<view K>`; `Join for List<const &str>`; reflection's names `const &str`.
- **Ties**: `generic_result` and its `Signature` field are gone, and `untied_result` no
  longer lets a plain `T` parameter excuse an untied result.

The migration ran the compiler over itself: a scratch script applied every certain `DZ0575`
and `DZ0576` fix to std (rebuilding `dz` between rounds, since std is compiled in), then to
every project in `tests/` and `examples/` and to the single-file corpora; a second pass
turned each stored `&str` (`DZ0517`) into `const &str`, and what still failed was changed by
hand, mostly to `String` where runtime text had been stored. Rust test snippets, `explain`
and the spec's examples were respelled by pattern. `tests/bench_programs/expr` was the one
real rewrite: its tokens and names viewed the program, so they now keep spans, the parser is
a view struct holding `&String`, and names are `String` keys looked up with `get_str`.

Tests: `typed/`'s `text_kinds`, `typeerrors/`'s spellings, `(&i32)?`, a text field and
`List<&str>`, `dz explain DZ0575` and `DZ0576`.

## r97: an optional reference item is a pointer (2026-10-31)

T-0111's second item, T-0085's option 3. `last<view T: Iterate>(it: T) -> T::Item?` over a
list's iterator emitted `::dz::Opt<const int32_t&>`, which C++ refuses (a reference member
has no default and no assignment), so the user got `DZ0803`.

- **The call.** `ty::optional_reference_as_pointer` turns a result written as an optional
  of a projection, once solved to an optional of a reference, into `^T` or `^mut T`. It is
  applied where a generic function's signature is solved (`check.rs`) and in
  `instantiate_sig`, for a method of a generic type; not in `trait_method`, which the
  conformance check also reads, where an implementation's `(&T)?` would then mismatch.
- **The body.** Unchanged and still an optional: one body serves every instance. The
  runtime's `Opt<T&>` holds a `T*` and `has`, converts to `T*` for the caller and from one
  for an argument, and `opt_get` reads either kind's payload, which codegen now uses in
  `if let` and `or` where it read `.value`.
- **Ties.** Nothing new: the `^i32` is tied by elision as the reference was, so
  `last(xs.iter())` followed by `xs.push(5)` ends it.

Tests: `tests/run/projection` (`last` over a list, an empty one and `Countdown`'s owned
tokens), `typed/`'s `final_number`, and `typeerrors/`'s `i32?` given a `^i32`.

## r96: a view struct holds a reference (2026-10-31)

T-0111's first item. A reference has no zero value, which §6.2's "every value starts at its
defaults" did not allow for, so the user chose how one is set: a constructor binds it in
its opening statements, over a literal-only rule that would have cost the seven map
iterators their constructors.

- **Checker.** `collect_struct` accepts a `&T` field in a view struct and refuses a
  default on one. `function` collects a view struct constructor's reference fields into
  `Body::unbound`; `block_valued` hands each statement the `binding_prefix`, and
  `bind_ref_field` turns `m_map = map` (or `self.m_map = map`) into an `Assign` with `bind:
  true`, until a statement that is not one ends the prefix. A late binding is reported once,
  at the constructor, not again as a write through a `&T`. `TypeTable::holds_reference`
  refuses `let x: T` with no value. `place_mutability` takes a reference field's writability
  from the reference, so writing through a `&T` field is `DZ0507` as for a local; that also
  makes `let mut r = &x; r = y` an error, which §5.2 already said it was.
- **HIR.** `StmtKind::Assign` gains `bind`; `dz_flow`'s ownership pass treats it as an
  assignment, and the ties walk as setting nothing to kill.
- **C++.** A reference field is a pointer member (`member_ty`), since a C++ reference member
  cannot be reassigned by the move a `for` makes; a read is `(*obj(b).f)` (`ref_member` in
  the `Field` arm), a binding and a literal's value are `&::dz::as_ref(v)`, which takes both a
  C++ reference and the `Addr` proxy `&x` makes. Warning-clean under clang++ and g++.
- **Ties.** A view struct's constructor call now returns the struct in `call_signature`, so
  `Peek(&b)` is elided to `b` and an item through `tie @bag = m_bag` outlives the iterator.
  Before, a constructor's result held no views, so `let p = Peek(xs[..]); let r = p.n();
  move(p)` reported `r` as killed even for a slice field.

std's seven map and slot-map iterators hold `&`/`&mut` and `tie @items = m_map`, with
`get` `in self@items`, and lose their `or return` guards. Tests: `tests/run/ref_fields`
(also in the `-Wall -Wextra` set), `typed/`'s `Teller` and `Glance`, `typeerrors/`'s five
`DZ0574`s and a plain struct's `DZ0517`, and `dz explain DZ0574`.

## r95: a view struct stands for what it views (2026-10-31)

The user's choice between two answers to where a result through a view struct points:
option A, what it views, over B, the struct. Under B, `let a = first(&xs.iter())` was a
view of a temporary iterator; and since a projection did not count as a view, `first`'s
`T::Item` result was in fact untied, so nothing was refused and nothing was checked.

- **Inputs.** `dz_check::ties::view_input` is what elision counts: `points`, or a view
  taken by value (`placed_as_view`). `untied_result`, `untied_fn_type`, `dz_flow`'s
  `call_effects` and `result_roots` all use it, and `invoke` binds such an argument by the
  views it holds, as it binds a reference by what it refers to.
- **What holds a view.** `holds` counts a `view` parameter and a projection of a `view
  type`, so `-> T::Item` is tied by elision. The DZ0571 test does not, since what a `view`
  parameter is given need not be a view. `generic_result` is unchanged and still leaves a
  plain `T` result untied.
- **What a result is in.** `what_it_views`: an origin that is a view-struct local itself,
  whole, stands for the views its state holds. `invoke` uses it for an elided result and
  for an `in` naming an input whole, in place of the input's place, beside r93's
  `through_view_field` for a path through a view field. A reference to a temporary view
  struct (`&xs.iter()`) carries the temporary's views, since it has no place.
- **The callee's side.** A by-value view parameter is seeded as a view input, caller-owned
  in the return check, and not dropped as a place at the end. `own_storage` keeps a result
  elided to a view struct, or `in` it whole, from being its own storage: a field holding no
  view, or the struct behind a reference. `in p.at` names the field and ties to the struct.

Nothing in std or the examples needed a change. One program in `tests/errors` did:
`swap_in<T: Iterate, U: Iterate>(a: &T, b: &U) -> T::Item` returns a view now, from two
view inputs, so it says `in a` (the program is about the projections' types, `DZ0501`).
Tests: `tests/flow/clean.dz`
(an item of a temporary iterator and of one taken by value), `use_after_kill.dz` (that item
dies with the list), and the new `view_struct_own_storage.dz` (`DZ0414` for a field of the
struct's own, and the `in` that allows it).

## r94: an implementation promises no more than its trait (2026-10-31)

Found while giving `SliceIter::get` an `in` its trait's method does not have: §6.7 said an
implementation's clause may name no more than the trait's, and nothing checked it, for
`kills` or for `in`. A call through the trait reads only the trait's, so an implementation
naming more would end or keep references nobody was told about.

`clauses_within_trait` (`dz_check/src/check.rs`), called from the conformance check once a
method's types match, builds both sides' clauses as `FnKills`, names rooted at `self` and
then the parameters, and asks `kills_within` and `results_within` (r91's, from
`dz_hir/src/ty.rs`), so elision is the same as for a function type's. `DZ0573` for either,
with `dz explain`. std's three trait methods with clauses (`Sort::sort`,
`StrKeys::remove_str`, `StrSet::remove_str`) already promised enough, and no
implementation in std, the examples or the corpora named more. Tests: `typeerrors/`
(`Tally`: a kill and a result tie the trait does not allow).

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
