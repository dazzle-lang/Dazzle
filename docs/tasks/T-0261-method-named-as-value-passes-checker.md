---
id: T-0261
title: A method named as a value passes the checker and fails in C++ generation
milestone: M19
status: done
kind: bug
area: checker
where: `crates/dz_check/src/check.rs` (`assoc_item`, its last arm); `crates/dz_cpp/src/lib.rs` (the `hir::ExprKind::Error` arm that reports DZ0804)
---

**What happens.** A method named by its type's path, as a function value, checks clean and
then stops the build in C++ generation with DZ0804, "the compiler lost track of this
expression", which says it is a compiler bug. A plain method is enough:

```dz
struct Counter {
    pub n: i32 = 0
    pub fn add(&mut self, by: i32) { n += by }
}

fn main() {
    let mut c = Counter{}
    let f: fn(c: &mut Counter, by: i32) = Counter::add   // `dz check`: clean; `dz run`: DZ0804
    f(&mut c, 2)
}
```

`Pair<i32>::bump`, a method of a generic struct, and `World::remove<T>`, a generic method,
do the same. Found while reworking T-0246, whose `World` keeps per-type removers; it uses
free functions instead, with a comment pointing here.

**Why.** `assoc_item` knows enum variants, unions and associated constants. Anything else
on a type's path, a method included, falls to its last arm, `(Ty::Error,
hir::ExprKind::Error)`, without a diagnostic. `Ty::Error` quietens every check after it, so
nothing is reported until the emitter meets the `Error` node.

**What to decide.** The spec says a generic *function* may be taken as a value (§8.1); it
says nothing of methods. Either a method is a value whose first parameter is the receiver,
which is what the program above expects (the user's call, and a §21 row), or naming one is
an error with its own code and a `help:` to wrap it in a free function. The second fixes
the bug whatever is decided about the first.

**Done when** the program above either runs or is refused by `dz check` with a diagnostic
that is not DZ0804, with a `tests/errors` case or a `tests/run` program to match.

**Done, 2026-11-28 (r162).** The user's call: a method is a value, its receiver first. The checker makes `hir::ExprKind::MethodItem` (`assoc_item`, `method_value`), typed as the function type with the receiver prepended and the method's clauses. `dz_mono` counts it as a call (`called_methods`, and `want_method` for a generic method). The emitter writes a captureless lambda that calls the method, as `result_adapter` does for a function (`Emitter::method_value`). On the way, a function type's parameters were found to compare one way only, so a `fn(&mut T)` passed as a `fn(&T)`. They match exactly now (`assignable` in `dz_hir::ty`).

Tests: `tests/run/method_values`, `tests/errors/method_value_mutability.dz` (DZ0501), `tests/errors/method_value_kills.dz` (DZ0568), and a case in each of `typed` and `typeerrors`.
