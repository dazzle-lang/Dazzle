---
id: T-0241
title: A trait's default body copies self into a Self value, though Self may not be plain
milestone: M12
status: open
kind: bug
area: checker
where: `dz_check/src/check.rs`, `subst` (`Self` in a trait body is the trait's own type, `self_type_of`); `dz_check/src/impls.rs`, `implements` ("a trait satisfies itself"); the plain-data test `TypeTable::plain`
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
