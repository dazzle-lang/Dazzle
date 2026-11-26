---
id: T-0271
title: An impl generic only through its trait's arguments generates broken C++
milestone: none
status: open
kind: bug
area: codegen
---

Found on 2026-11-26 while testing packs on implementations (T-0269), and there before it.
An implementation whose type parameter appears only in the trait's arguments, not in the
type it is for, checks clean and then fails in C++ (invariant 7):

```dz
trait Describe<T> {
    fn describe(&self) -> usize
}

struct Widget {
    pub n: i32 = 0
}

impl<T> Describe<T> for Widget {
    fn describe(&self) -> usize {
        return sizeof(T)
    }
}

fn main() {
    let w = Widget{}
    println!("{}", Describe<i64>::describe(&w))
}
```

The implementation's function is emitted once, as if not generic, `Describe_x__…`, with `T`
left a `void`, and the call names `Describe_T__…`: neither side solved `T` from the trait's
arguments. An implementation generic through its type (`impl<T> Describe for Box<T>`) works.

Either such an implementation is instantiated per trait argument, which `resolve_impl` and
monomorphization would have to solve from the trait reference as well as the receiver, or the
checker refuses it with a diagnostic until it is.

## Where the code is

| Piece | Location |
|---|---|
| Naming and emitting an implementation's functions | `crates/dz_cpp/src/lib.rs` (`impl_fn_name`, the loop that emits non-generic implementations with an empty map) |
| Which implementation a call reaches | `TypeTable::resolve_impl` in `crates/dz_check/src/impls.rs` (`match_ty`) |
| Implementation instances | `crates/dz_mono/src/lib.rs` (`impl_instance`) |
