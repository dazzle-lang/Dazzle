# r81: ties, `kills` clauses and compiler-placed memory

The language change is spec §6.7 and §14.6, decided as D-0091; the plan, in order, is
T-0107. How it came about: the regions prototype (T-0104, T-0105, on the branch
`t-0104-regions-prototype`) tried to choose allocators without a checker, and each stage's
safety rested on rules a test had found. The user's own "ties" idea supplied the checker, and
D-0091 was decided on 2026-10-27.

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
