# M19: Threads

The 0.3 milestone for threads (spec §19, §22). Newest first.

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
into a `let mut walk = it` first, which is the user's to weigh.

**Tests.** `tests/workspace/typeerrors` has the three mistakes (writing through the field in
a `&self` method, calling a `&mut self` method on it, writing through it from a `&` parameter),
each `DZ0507`; `tests/workspace/typed` has the same through `&mut self` and `&mut`, accepted,
and a read through `&self`. `hir_typed` shows the change where it applies: `std`'s
iterators' `valid(&self)` read their `&mut` fields as `&`.
