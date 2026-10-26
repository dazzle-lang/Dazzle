---
id: T-0086
title: A bound at two arguments answers a method call with the first
milestone: M12
status: done
done: 2026-10-26
kind: bug
area: checker
where: `dz_check/src/check.rs`, `param_method`
---

**What is wrong.** A type may implement one trait at several arguments, and a call picks the
implementation by its first argument (spec §11.2, §18.27). A concrete `Vec2` that is
`Mul<f32>` and `Mul<Vec2>` does this. A generic parameter bounded the same way does not:

```dz
fn scale<T: Mul<f32> + Mul<i32>>(a: &T) {
    let b = a.mul(2.0)        // DZ0501: expected `i32`, found `{float}`
}
```

`param_method` walks the bounds' closure and returns the first trait whose method has the
name. That is `Mul<i32>` here, because the closure comes off a stack, so the call is checked
against the wrong `rhs`. Found while writing associated-type projection (r75).

**Why it matters.** It is rare: a generic body bounded by one trait at two arguments is
unusual. But when it happens, a correct program is refused with a type mismatch that names
the wrong type.

**What it would take.** Collect every bound that provides the method, as `trait_method_call`
does for a concrete receiver, and let `pick_overload` choose by the first argument. A
`tests/run` case calling both, and a `typed/` case. Compiler work only, for `docs/log/`.
