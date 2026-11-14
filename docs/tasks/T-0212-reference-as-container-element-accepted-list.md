---
id: T-0212
title: A reference as a container's element is accepted: List<&i32> checks clean, dangles, and emits invalid C++
milestone: none
status: done
kind: bug
area: checker
---

**Done 2026-11-14.** A reference is refused wherever a view is (`DZ0517`): as a type
argument to a parameter that is not `view` (`List<&i32>`, `Box<&i32>`, `keep(&a)`), in a
static, a union's payload, and inside a tuple, optional or array in a plain struct's field.
`TypeTable::stored_as_view` is `placed_as_view` plus `&T`, asked by the four storage checks;
the ties check still asks `placed_as_view`, which it follows references apart from. The
message says "is a reference" or "holds a reference" rather than calling `&i32` a view. Tests:
`a_reference_cannot_be_a_plain_type_argument`, `stores_references` in `typeerrors/`, and
`a_reference_passed_on` in `typed/` (a `view` parameter given one).

Reported by the user on 2026-11-13. `dz check` accepts a `List<&i32>`, even holding a
reference to a block-local value read after the block, and the build then fails in C++,
which invariant 7 makes a compiler bug:

```dz
fn main() {
    let mut refs = List<&i32>()
    {
        let local = 42
        refs.push(&local)
    }
    println!("{}", refs[0])     // `local` is gone: a read of freed stack memory
}
```

```
error[DZ0803]: the C++ compiler rejected the generated code
  refs.h:30:19: error: 'm_data' declared as a pointer to a reference of type 'const int32_t &'
  dz_runtime.h:144:6: error: 'ptr' declared as a pointer to a reference of type 'const int &'
```

The C++ fails without the block too (`let a = 1`, `refs.push(&a)`): any `List<&T>` does.

**Why it happens (likely).** §5.2 says a reference "cannot be stored in a struct field, cannot
be heap-allocated" (`[DECIDED — §18.3]`), and a view given to a type parameter that is not
`view` is already refused with `DZ0517` (`check_view_arg`: "a view lives only where a reference
does, never in a container"). But that check asks `placed_as_view`, which looks to answer for
view structs and the like and not for a plain `&T`, so a reference passes as an element. With
the type accepted, the ties check has nothing to follow into the list (its element is not a
place it models), so the dangling read is not `DZ0413` either.

**What it would take.** Most likely: refuse a `&T` or `&mut T` as the argument of a type
parameter that is not `view`, with `DZ0517`, where `check_view_arg` refuses a view, and check
where else a reference can reach a field or the heap through a type argument (`Box<&T>`, a
generic struct's field, a tuple in a container). That is the existing rule applied, not a
language change, unless `view` parameters are meant to take plain references; `std`'s own
`view` parameters (iterators, `Search`) should be checked to still accept what they take.
Tests: a `typeerrors/` case for `List<&i32>` and a `typed/` one for a `view` parameter given
a reference (invariant 8).

**Where the code is.**

| Piece | Where |
|---|---|
| A view as a type argument refused, `DZ0517`: `check_view_arg`, `check_view_storage` | `crates/dz_check/src/lib.rs` |
| What counts as a view: `placed_as_view` | `crates/dz_check/src/lib.rs` |
| The code and its rule | `REF_IN_FIELD` in `crates/dz_diag/src/codes.rs` |
| The rule | spec §5.2 (the `&T` row), §6.7, D-0003, D-0093 |
