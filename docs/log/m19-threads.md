# M19: Threads

The 0.3 milestone for threads (spec §19, §22). Newest first.

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
