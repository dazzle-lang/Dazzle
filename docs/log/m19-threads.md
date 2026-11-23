# M19: Threads

The 0.3 milestone for threads (spec §19, §22). Newest first.

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
