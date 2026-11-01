---
id: T-0121
title: A local named like a free function hides it in the generated C++
milestone: M12
status: open
kind: bug
area: codegen
---

Found 2026-11-01 while extending `tests/run/regions` (T-0117). A program that checks
clean is rejected by the C++ compiler, which is a compiler bug (invariant 7):

```dz
fn total(xs: &List<i32>) -> i32 { … }

fn main() {
    let mut total = 0
    …
    println!("{}", total(&xs))     // Dazzle: the function. C++: the local.
}
```

clang++ says "called object type 'int32_t' is not a function". Dazzle resolves the call to
the function, but the C++ writes the bare name `total`, and the `int32_t total` declared
earlier in the same C++ scope hides it.

**Where the code is.**

| Piece | Where |
|---|---|
| A free function's name at a call: `hir::Callee::Fn` → `self.qualify(*sym)` | `call`, `dz_cpp/src/lib.rs` |
| Local names, made unique only against other locals | `local_names`, `dz_cpp/src/lib.rs` |

**What it would take.** Either qualify a free function at a call whenever a local of the
body has its name (`::ns::total(…)`), or have `local_names` rename a local that shares a
name with a function the body calls. The first keeps the locals' names as written, which
the generated code is meant to show (spec §16.2). A `tests/run` case, and the warning-clean
list.
