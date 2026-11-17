---
id: T-0241
title: A trait's default body copies self into a Self value, though Self may not be plain
milestone: M12
status: done
kind: bug
area: checker
where: `dz_flow/src/lib.rs`: `return` of a whole local, which asks `denotes` whether the local is owned, and `type_name`; `dz_check/src/lib.rs`: `clones_itself` (a trait's own type is never plain); `crates/dz_project/src/std/memory/rc.dz`: `guard`
---

Found with T-0037 (2026-11-17). A default method may read `&self` into a value of type
`Self`, and the checker accepts it for every implementer:

```dz
trait Twice {
    fn me(&self) -> Self {
        return self          // accepted
    }
}

struct W {
    pub s: String
}

impl Twice for W {}

fn main() {
    let w = W{ s: String("a") }
    let v = w.me()           // a copy of a `String`, written nowhere
}
```

That is a copy of a value that is not plain data, which D-0106 allows only as `.clone()`.
The same body in a generic function, `fn dup<T>(x: &T) -> T { return x }`, is refused with
`DZ0402`, since a type parameter is never plain. A default body is checked once, with `Self`
read as the trait's own type, and that type is treated as plain. The C++ it emits copies
through `String`'s copy constructor, so nothing is freed twice, but the language rule is
broken silently.

**The fix to look at:** in a default body, treat `Self` as a type parameter is treated: not
plain, so a by-value read of `self` is `DZ0402`, with the same help. Check std's default
methods still compile (`Iterate`, `Iterable`, `Hash`, `Ord`'s and the formatting traits').
T-0037 avoided this: `Iterate` has no `indexed()`, because its body would have been exactly
this copy; an iterator is numbered with `Indexed(it)` instead.

**Done** (2026-11-17, r140). Wider than found: `return self` of a value that is not plain
copied it in any method, not only a default body. The ownership pass treated `return x` of a
whole local as a move without asking whether the local was owned, and `self` is a borrowed
receiver. It now asks (`Denotes::Owned`), so `return self` is `DZ0402` with the `.clone()` fix.
A trait's own type, which is `Self` in its default bodies, is never plain (`clones_itself`),
and messages say `Self` for it. One `std` site relied on the copy: `Rc`'s `Nullable::guard`,
now `self.clone()`. Test: `tests/flow/return_self_is_borrowed.dz`, and `self.clone()` in a
default body runs in `tests/run/default_bodies`.
