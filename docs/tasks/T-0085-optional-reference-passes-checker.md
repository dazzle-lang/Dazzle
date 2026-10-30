---
id: T-0085
title: An optional of a reference passes the checker
milestone: M12
status: done
done: r90, 2026-10-30
kind: design
area: checker
---

**Done: decided in r90 (2026-10-30), by the user, with D-0093.** Each case below has an
answer and a task that builds it:

- **`(&i32)?` written out** is an error that offers `^i32`
  ([T-0109](T-0109-text-only-borrowed-str-const-str.md)).
- **`wrap(&n)`, where `T` would be `&i32`**, is refused at the call, since no view is a type
  argument (D-0093, point 8; T-0109).
- **`T::Item?` whose instance is a reference** is that pointer: option 3 below, for
  projections only, since a plain `T` is never a reference
  ([T-0111](T-0111-view-structs-struct-that-holds-view.md)).

**What is wrong.** Spec §5.6 says `?` applies to value types only: there is no `(&T)?` and
no `(^T)?` (`[DECIDED]`). The checker does not enforce it, in three places:

```dz
fn pick(xs: &List<i32>) -> (&i32)? { … }   // written out: accepted

fn wrap<T>(x: T) -> T? { return move(x) }
let r = wrap(&n)                           // T is &i32, so T? is (&i32)?: accepted

fn last<T: Iterate>(it: T) -> T::Item? { … }
last(xs.iter())                            // Item is &i32: accepted
```

The first two are accepted silently. The third is new with associated-type projection (r75),
since `SliceIter<T>` chose `type Item = &T`. In all three, the generated C++ is a
`::dz::Opt<const int32_t&>`, which C++ rejects: a reference member cannot be
default-constructed or reassigned. So the user gets `DZ0803`, a compiler bug by invariant 7,
instead of a diagnostic.

**Why it matters.** Generic code over iterators reaches for `T::Item?` naturally: "the last
element", "the first that matches", "the largest". It is natural to write, and today it breaks in
C++ as soon as the iterator yields references, which every container's does.

**Decision for the user.** The concrete `(&i32)?` is plainly an error and only needs a
diagnostic. The generic case needs a rule for what `T?` means when `T` turns out to be a
reference:

1. **Refuse at the instance.** A type argument that would make `T?` an optional of a
   reference is an error at the call. Simple, but it is exactly the per-instance error §8.6
   exists to avoid, and it makes `last` unusable over every container.
2. **Refuse in the body.** `T?` on an unbounded `T`, or on a `T::Item` whose trait does not
   say it is a value, is an error where it is written. Errors stay at the definition, but
   `last` cannot be written at all.
3. **An optional of a reference is a pointer.** `(&T)?` is spelled and emitted as `^T`, as
   §5.6 says a pointer already is the optional reference. `T?` then means "a `T`, or
   nothing" for every `T`. This is a language change (§5.6, a §21 row) and the most useful
   of the three.

**Where.** The concrete check belongs where an optional type is lowered
(`Ctx::lower_type_inner`, `TypeKind::Optional`). Options 1 and 3 work on the instance's
types, in `dz_mono` or codegen's `cpp_ty`.
