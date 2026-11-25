# M19: Threads

The 0.3 milestone for threads (spec §19, §22). Newest first.

## `lends p` (T-0265, D-0119, r152; 2026-11-25)

What replaced r151's runtime lock (below). The user asked what the lock was and whether it
was the best approach; three ways to tell the function that places a container that a block
further down will grow it were put: a clause in the signature, inference across functions,
or the lock. `&mut` alone cannot say it, since nearly every `&mut` is single-threaded. The
user chose the clause, spelled `lends w`.

- **Syntax.** `lends` is a keyword; the clause is a list of names after `kills` and `sends`,
  in any order, parsed as `kills` is (`parse_kills`). `FnDecl::lends`, `FnSig::lends`,
  `dump::lends_str`. A test function named `lends` in `tests/flow/static_kills.dz` was renamed.
- **The checker.** `ties::lendable`: a whole `&mut`, `[]mut` or `^mut` parameter, `&mut self`,
  or a view by value (DZ0427). A function with a `lends` used as a value is DZ0428, since a
  function type has none; an implementation or override lending more than the trait's or the
  base's method is DZ0429 (`lends_within_ceiling`), one diagnostic per clause.
- **What a body lends** (`dz_flow/src/ties/body.rs`). `record_parallel` marks every place a
  block's call writes, and `record_lends` every argument to a callee's `lends` parameter, as
  lent (`BodyAnalysis::lent`), by its root local after the tie walk's references are followed.
  Only what may own memory counts (`may_grow`): the first version asked `both` for `lends c`
  on a `&mut i64`. `report_lends` holds the clause to that, both ways, with the fix `dz fix`
  applies, as `sends` is held (DZ0425, DZ0426).
- **Placement.** For a body with a block or a call to a lending function (`lends_anything`),
  the tie walk is run once more and every lent local leaves (`escape_of_all`): on the heap. It
  follows references, which the direct-use rule of r151 did not: `let r = &mut xs` and
  `parallel { fill(r) … }` left `xs` in the region.
- **Removed**: the guard in every `Allocator` handle method, `ArenaGuard`, `parallel_active`,
  `parallel_depth`, the arena locks, each arena's `m_lock`, `Hir::runs_parallel` and the build
  fact it fed. `std`'s C++ is the same in every program again.

`tests/errors/lends_*.dz` and `lending_function_as_a_value.dz` have one mistake each; `typed/`
has `lends` on a function, a caller passing it on and `&mut self`; `typeerrors/` the checker's
three; a placement test pins what goes to the heap. `tests/run/parallel`'s world lent only to
`frame`, which had been in `main`'s region under the lock, is on the heap.

## `parallel { … }` (T-0011, D-0118, r151; 2026-11-24)

The user asked why a program needs `join!` beside `spawn`: `spawn` takes only values, since
nothing waits for its thread, and `join!` waits, so its calls may borrow. Asked how to build
it, with no closures to carry a call's arguments to a thread, the user chose a block keyword
over a compiler-owned macro, and over `run`, which 43 `.dz` files name. Calls return nothing,
each runs on a thread of its own, and `Rc` is refused, all for now (T-0265 to T-0267).

- **Syntax.** `parallel` is a keyword; `StmtKind::Parallel(Block)` in the AST, so the parser
  takes any statements and the checker says what is wrong with one. Formatted like `while`.
- **The checker** (`dz_check/src/check/parallel.rs`). Each statement must be a call that
  returns nothing (DZ0597). A callee that needs the main thread is caught where
  `needs_main_thread` sees a need at the call's own span (DZ0598), inside `unsafe` too, and
  not charged to the enclosing function. Each argument and receiver is held to `Send`/`Share`
  by how it crosses (DZ0599), with §22.4's help when the type reaches an `Rc` (`holds_rc`).
  The HIR is `StmtKind::Parallel { calls }`.
- **Calls against each other** (`dz_flow/src/ties/body.rs`). The tie walk takes the calls in
  order, as every other pass does; while it walks one, `call_inner` records what its arguments
  reach and whether they write: `&mut`, a `&mut self` receiver, a view by value that is not
  `Share`, and a value moved in (its source kept in `moved_from`, since a `Move` node's places
  are cleared). Pairs that meet on a place, one writing, are DZ0424; `Place::covers` keeps two
  fields, and two elements with known different indices, apart. Since every argument is
  evaluated before any call, a move beside a read is a conflict, not a use after move.
- **Placement.** A local lent `&mut` or given by value to a block's call, and a site made for
  one, go to the heap (`parallel_call`, `lent_shared_to`). A by-value argument read as a copy
  leaves its source where it is; `is_copy` alone said "copy" for `&mut xs` too, since `List`
  can be cloned, and the first version kept such lists in the region.
- **A lock per arena** (§22.6). A container that reaches a block through a parameter was
  placed by a caller that cannot see the block, possibly in its region, and two calls may grow
  two lists of that region at once. Every method of an `Allocator` handle that writes it now
  starts with an `ArenaGuard`, held while `parallel_depth` is above zero. The first version
  took one lock for every allocator but the C heap, which serialised each call's own regions
  too; the user chose a lock per arena instead (`allocator_lock`): a region's or an owner
  arena's own `m_lock` word, none for the heap, and one shared word, recursive, for any other
  allocator. An owner arena takes its parent's lock only while holding its own, and a region
  calls no other allocator, so the order is always child before parent and nothing deadlocks.
  Whether a signature should make the lock unnecessary is T-0265.
- **What the check costs.** Measured against the compiler before it, release, twenty
  alternating runs on this machine: a guard object in each handle method made `frame` 9%
  slower on clang++ (owner arenas, through the vtable); a second copy of the body under the
  check, still 9%; the locked call out of line, `DZ_NOINLINE`, 6.5%; passing it the object's
  address rather than the handle, which then had an address and left a register, 3–4%. On g++
  that version was still 5% on `astar` and 8% on `words`, a load and a branch on every
  allocation. So the check is emitted only for a program with a `parallel` block
  (`Hir::runs_parallel`), which the build folds into its whole-program `templates` fact, so the
  first block added or the last removed emits `std` again; `std`'s compiled copy is keyed by
  its contents. Without a block, every program is within noise of before, `frame` within 1.5%
  (the lock word each arena now carries).
- **C++.** Every argument, receiver and default of each call is bound to a temporary first
  (`hold_args`, taken by `call` for the outer call alone, which forces `emit_args`' binding);
  each call is then a `[&]` lambda, and `::dz::rt::native::parallel_run` runs the first on this
  thread and the others through `thread_start`, joining them, or running a call here when its
  thread did not start. The statement is a C++ block, so the temporaries die after the join.

`tests/run/parallel`, `tests/errors/parallel_*`, `tests/flow/parallel_conflict.dz`, the typed
and typeerrors workspaces, and a placement test cover it. The run test grows a region two calls
share, a call's own region and an owner arena in one block, and gave the same output on fifteen
runs, under `--track-memory` and with regions off. The generated C++ was compiled
warning-clean with g++ and clang++ by hand.

## `Send`, `Share` and `#[main_thread]` (T-0264, r150; 2026-11-24)

What makes `spawn` safe. It was first built as an `unsafe fn`; the user asked that spawning not
be `unsafe` and that the promise of §1.1 hold, so the checks of §22.2 and §22.5 came with it.

- **`Send` and `Share`** (`dz_check/src/threads.rs`). `TypeTable::sendable` and `shareable`
  walk the fields as `copyable` does, with Rust's mapping for references (`&T` is `T` shared,
  `&mut T` moves as `T` and is never shared). `implements` answers both from the walk, so a
  bound `T: Send` needs no new machinery. `std`'s `impl Send` and `impl Share` replace the walk
  for that type and count only in `std` (DZ0596 refuses one elsewhere): the containers, whose
  `^mut Allocator` is placed for the thread, `Mutex` and `Atomic`. A refused bound names the
  field that stops it (`crossing_blocker`).
- **`#[main_thread]`** (`dz_check/src/check.rs`, "the main thread"). A body records what needs
  the main thread outside `unsafe`: a static that is not `#[thread_local]` and whose type does
  not synchronise (`note_static`), every call whose signature is `#[main_thread]` (hooked in
  `check_args`, which every call a program writes passes through), a call through a
  `#[main_thread] fn` value, and making or taking by value a value whose drop is the main
  thread's. `settle_main_thread` holds that to the signature when the body ends: DZ0592 with a
  certain fix that writes the attribute above the declaration at its indentation, DZ0593 with
  one that removes it, and DZ0595 where a trait's method or a base's `virtual` one does not
  allow it. `FnSig` carries `main_thread` and `entry_point`; `Ty::Fn` carries `MainThread`,
  outside its identity as `kills` is, and `assignable` refuses a main-thread function where a
  plain one is wanted (DZ0594).
- **Statics.** "A static whose type is `Share` needs nothing" would let `FRAME_COUNT += 1` race,
  since a `u64` is `Share`. Read instead as a type that synchronises, which `std` says with a
  private trait, `Synchronises`, on `Mutex` and `Atomic`.
- **Drops.** `examples/sdl3`'s `Renderer` frees GPU resources in its `drop`. The user chose that a
  `drop` may be `#[main_thread]`, its type then neither `Send` nor `Share`; making or taking one
  by value needs the main thread too (`TypeTable::dies_on_main_thread`), or a worker could build
  one from its defaults and drop it.
- **C.** A recipe's `thread_safe` (§15.1) is read: `true`, or the functions listed, checked
  against the header as `keeps` is. Everything else imported is `#[main_thread]`, and so is a C
  function pointer. `std`'s `libc` is thread-safe.
- **The type `#[main_thread] fn(…)`** parses, prints and formats.

The migration was `dz fix`: `examples/breakout`, `sokol`, `stb`, `box2d` and `sdl3` (every function
calling raylib, sokol, SDL, OpenGL or ImGui, and `Renderer`'s `drop`), and the run tests that
touch statics. `examples/ownership` and `tests/run/cinterop` say `thread_safe` instead. Three
tests build statics at start-up through constructors that read other statics, from a field's
default, which may run on any thread; they read them inside `unsafe`, vouching for start-up.

`tests/errors/main_thread_*`, `spawn_*`, `send_*` and `attribute_on_a_function_type` have one
mistake each; `typed/` and `typeerrors/` have the rules side by side; `dz_flow`'s placement
tests pin that what a thread is given is built on the heap.

## `spawn` and `Thread` (T-0011, r150; 2026-11-24)

The third of T-0011's pieces. §22.3 decides `spawn!(f(a, b))` and nothing about what it
expands to, so the function underneath is a proposal (D-0117, §22.11): `spawn(entry, value)`
and a `Thread` with `join`. The macro waits, since an expression macro cannot declare the
struct a call's arguments need.

- **`std`** (`std/sync/thread.dz`). `spawn<A>` builds a `Start<A>` of the function and the
  value in a `Global` allocation, and starts the generic `start_thread<A>`, passed as a plain
  `fn(^mut u8)` value, on it. The thread moves the value out and frees the block before it calls
  `entry`, so a thread that never returns holds nothing. A `Thread` is one word, zero once
  joined, and its drop detaches it if it was not.
- **The runtime** (`NATIVE_CPP`). `_beginthreadex` and `WaitForSingleObject` on Windows,
  `pthread_create` elsewhere, with a malloc'd record for the `pthread_t`, since it may be zero or
  a pointer. Not `std::thread`: `<thread>` is missing from MinGW's win32 thread model. The link
  line gains `-pthread` off Windows (`toolchain::link`), for glibc before 2.34.
- **Safe**, with T-0264's checks, above.

`tests/run/threads` joins one thread, eight racing on an atomic and eight on a mutex, a struct
carrying two values, a handle dropped unjoined, and a value with a destructor that runs once,
on the thread. Clean under `--track-memory`.

## Constructor visibility (D-0115, r148; 2026-11-23)

Found while building `Mutex<T>`: its guard's `pub(module)` constructor could be called from
any module, because the constructor call never checked visibility. The user chose private by
default, as for every other member (§4).

- **The checker.** A constructor call and a `: Base(…)` initialiser both go through
  `check_member_visible`, so the error is the existing `DZ0506`, naming the constructor.
- **The migration.** Every constructor with no keyword became `pub`, so no program lost access:
  128 in `std`, `examples` and the test corpora, the programs embedded in Rust tests and
  `dz explain`, the spec's examples, and the project `tools/bench_scale.py` generates. The
  lexer, parser, formatter and expansion corpora are untouched, since they do not check types.

`tests/errors/constructor_*` cover a private, a `protected` and a `pub(module)` constructor
reached from outside, and a private base. A two-module matrix, run by hand, covered overloads,
generics, factories, trait implementations and field defaults.

That matrix also found that default initialisation built a value with no call at all. The user
chose C++'s and Rust's answer (T-0262): `missing_default` decides whether a type can be built at
its default from a given place, and a `let`, a constructor's fields and each generic instance
ask it, all as `DZ0590`. `tests/errors/default_*` and `constructor_leaves_*` pin it.

It left one bug visible, T-0263: the emitted C++ default-initialised every field before a
constructor's or a move's body, so a field's zero-argument constructor ran when it should not.
That was fixed on its own (r149, D-0116; `docs/log/tooling.md`).

## `Mutex<T>` (T-0011, r147; 2026-11-23)

The second of T-0011's pieces. D-0096 gave it no API, so the shape is a proposal (D-0114,
§22.10): `lock()` returns a view struct, `MutexGuard<T>`, that unlocks when dropped.

- **`std`** (`std/sync/mutex.dz`, `mutex_guard.dz`). `get` and `get_mut` are `in self@held`, a
  tie on the guard's own `bool`. A tie on its `&Mutex` field would name what the view views, and
  so let a reference outlive the guard (§6.7). The result really points into the mutex, so a
  pointer cast under `unsafe` vouches for the shorter tie. The same `bool` makes a moved-from guard
  safe: a move resets the source to its defaults and still runs its drop (§6.4), which on the
  first version unlocked through a null reference.
- **The runtime** (`NATIVE_CPP`). One word, zero when unlocked, so a `Mutex` needs no OS setup
  or teardown and can start in a static: a three-state futex on Linux, `SRWLOCK` on Windows,
  `os_unfair_lock` on macOS, and a yielding spin lock elsewhere.
- **The checker.** `is_static_initialiser` accepts `Mutex<T>(v)`, for a `v` a static may start
  as.

`tests/run/mutex` has eight native threads on a counter, a two-field invariant and a growing
`List`. `tests/errors/mutex_*` pin the ways a reference could outlive the lock. Checked by hand
on g++ and clang++: warning-clean at `-O2`, five clean runs under ThreadSanitizer, and 60
contended runs with no hang.

## `Atomic<T>` (T-0011, r146; 2026-11-23)

The first of T-0011's four pieces. The user chose one generic type and sequentially
consistent operations only (D-0113, §22.9).

- **`std`** (`std/sync/atomic.dz`). The bounds, `AtomicValue` and `AtomicInteger`, are private
  traits, so a program can neither name nor implement them. `add` and `sub` come from an
  `impl` bounded on `AtomicInteger`, so `Atomic<bool>` has neither. `impl Clone` is what makes
  the type not plain data.
- **The runtime** (`NATIVE_CPP`). Six functions take their operands by address and a width,
  so one set serves every `T`. C++17 has no `atomic_ref`, so GCC and Clang use the `__atomic`
  builtins and `cl.exe` casts to `std::atomic<T>`, with assertions that it has `T`'s layout.
  They live in `dz_native.cpp`, so each operation is a real call for now; moving them into the
  header is the obvious step if a benchmark shows it.
- **The checker.** `is_static_initialiser` accepts `Atomic<T>(constant)`. C++ emission needed
  nothing: such a static is a `BuiltStatic`, built from `main` before any thread starts.

`tests/run/atomics` races eight native threads on every operation and checks each width's
wrapping. Checked by hand on g++ and clang++: warning-clean at `-O2` and five clean runs under
ThreadSanitizer (the CI job is T-0248). `Send` and `Share` are not built, so nothing yet
refuses a plain value shared between threads.

## Checked type-keyed storage (T-0246)

`TypeMap` holds one owned value per type. Each entry is boxed behind a private
abstract base, whose virtual method supplies the stored type's `TypeId`. Lookups
check that identity before downcasting. A build with `type_info = "none"` leaves
the map out of `std`.

Its API is `len`, `get<T>`, `get_mut<T>` and `insert<T>`, and nothing more. The first
version also handed out insertion slots, with `get_mut_at` and a callback-taking
`update_at`, which existed for the ECS's despawn alone; they were taken out the same
day. The ECS now keeps one remover per component type, a generic function pushed
with the storage, that finds its storage by type, so nothing ties the remover list's
order to the map's. Despawn is the one path that pays a lookup per storage.

What the remover kills needs one indirection. A kill through `T`'s storage would be
`self@stored@items`, a tie the map's private entry type does not have, so the remover
takes the entity out through a function value declared `kills storage`, which the
checker reads as all of `self@stored`. A tie per type (§22.8) makes it direct.
The remover is a free function because a method named as a value checks clean and
then fails in C++ generation, DZ0804 (T-0261).

`tests/run/type_map` checks missing and mismatched lookups, values kept while the
map grows, and replacement dropping the old value without adding an entry.
`tests/errors/type_map_values.dz` checks that replacement ends a prior borrow.

## Synchronized memory tracking (T-0038)

One native lock covers records, frees, table growth, trusted element checks, and
the final leak report (§14.2). A free can find its record even when a different
thread made the allocation. Untracked builds bypass the lock. A separate exit
static closes and reports the table under the lock, so teardown never moves the
shared table out of its static storage.

The `thread_statics` fixture now has four workers allocate concurrently, grow the
table, and free one another's blocks. The previous tracker panicked on a valid free.

## Thread-local drops on workers (T-0247)

The first use of a thread-local static that needs initialization or a drop installs
a worker exit hook and initializes that thread's managed statics in dependency and
declaration order. The generated hook walks the same order backwards at thread exit.
It drops only thread-local values and resets each to its default before running its
drop, so later drops can still read it. The primary thread's process exit hook keeps
the existing order for both kinds of static (spec §7.7, §22.5).

The cells use trivially destructible storage so C++ does not end their lifetimes
before the generated hooks run. MinGW's POSIX thread model uses a pthread cleanup
callback because its C++ TLS destructors can run after the emulated TLS is freed.
The `thread_statics` run fixture checks the drop order across two modules, on two
native workers and the main thread, including a worker allocation that must be
released before the main thread's memory tracker reports.

## `mut` parameters (r145; 2026-11-18)

What deep `&` cost (below): an iterator taken by value could not step itself, since a
parameter was never writable. The user chose Rust's answer, `mut it: T`.

- **Parser** (`parse_params` in `dz_parse/src/item.rs`): an optional `mut` before a
  parameter's name, kept as `Param::is_mut`. `mut self` is read and refused with the message
  `self` by value always had. The AST dump and the formatter print it.
- **Checker** (the parameter loop of `function` in `dz_check/src/check.rs`): the parameter
  binds writable when `mut`; on a reference (`Ty::Ref`) it is `DZ0591`, new, since assigning
  to a reference writes through it with or without `mut`. A `&str`, a slice, a raw pointer
  and a view struct by value are re-pointed or changed in place, so they take `mut`.
- **The help for writing a parameter that is not `mut`** now points at the signature
  (`param_write_help`), for a parameter taken by value only: a reference is made writable by
  `&mut` in its type and `self` by `&mut self`, so neither is told to add `mut`.
- **C++**: nothing new. An owning by-value parameter was already moved into a local at the
  top of the body (M5), and a plain one is a C++ value parameter, which C++ lets the body
  write. Checked on clang++ and g++ with an iterator, an `i32`, a `&str` and a `String`.

Tests: `tests/errors/mut_on_a_reference_parameter.dz`; `typeerrors` gains a by-value
iterator stepped without `mut` (`DZ0507`) and `mut` on a `&mut i32` (`DZ0591`), `typed` the
four kinds accepted; the parse and fmt corpora a `mut` parameter, the fmt one written with
two spaces, which the formatter makes one; `dz explain DZ0591`. `tests/flow/clean.dz`'s
`taken` loses its `let mut walk = it`.

## Deep `&`, and `Iterate::get` takes `&mut self` (T-0258, r144; 2026-11-18)

The first piece of M19 that is code. Trying to break D-0096 (T-0245) found that `&` was
shallow, as C++'s `const` is: through `&self` a view struct's `&mut` field could be written,
so `fn reader(h: &Holder) { h.bump() }` wrote, and two threads given one `&` could race. The
user decided `&` is deep: nothing reachable through a `&` is written (spec §5.2).

**The checker.** Two places give a view's writable reference its read-only type, `&mut T` as
`&T` and `[]mut T` as `[]T` (`read_only_view` in `dz_check/src/check.rs`):

- a method taking `&self` binds its struct's fields unqualified (`bind_self_field`), and now
  binds such a field at the read-only type;
- a field access `base.f` whose base is reached through a `&` (`reached_through_shared`: a
  `&` or `[]` in the base's own type, or a value held by a `&self` field, followed down
  through fields and indexes) gets the read-only type.

Everything else follows from the types: assignment (`place_mutability`), a `&mut self`
method's receiver (`require_mutable_receiver`), `&mut` of a place, and passing the field on
as `&mut` all read the expression's type. A raw `^mut` field is left alone, since writing
through any raw pointer already needs `unsafe`, which vouches for it. The error is the
existing `DZ0507`, with a help that names the field and the rule
(`read_only_view_help`); the expressions that were downgraded are remembered by span
(`read_only_views`) and the downgraded fields by name (`read_only_fields`) only for that help.

**`Iterate::get` takes `&mut self`**, as Rust's `next` does, so an iterator may still hand
out `&mut` items. Every implementation changes, since a receiver must match its trait's
(`DZ0533`): the trait and 16 in `std`, the two `ecs` queries (`tests/bench_programs/ecs` and
`examples/ecs`) and 17 in the test corpora. A `for` loop calls `get` on its own local, so no
loop changed.

**What it costs a program.** Code that stepped an iterator through a `&` now takes it as
`&mut`, and since Dazzle cannot take `&mut` of a temporary, binds it first:
`first(&xs.iter())` became `let mut it = xs.iter(); first(&mut it)` in the spec's examples
(§6.7, §8.5), `dz explain DZ0560`, the feature matrix and four corpus programs. **An
iterator taken by value as a parameter cannot step itself**, since a parameter is not
writable and Dazzle has no `mut` parameters: `fn taken<view T: Iterate>(it: T)` moves it
into a `let mut walk = it` first; r145 answers it with `mut` parameters (above).

**Tests.** `tests/workspace/typeerrors` has the three mistakes (writing through the field in
a `&self` method, calling a `&mut self` method on it, writing through it from a `&` parameter),
each `DZ0507`; `tests/workspace/typed` has the same through `&mut self` and `&mut`, accepted,
and a read through `&self`. `hir_typed` shows the change where it applies: `std`'s
iterators' `valid(&self)` read their `&mut` fields as `&`.
