---
id: T-0163
title: A static's drop at exit can read a static C++ already destroyed
milestone: none
status: done
kind: bug
area: ownership
---

Found on 2026-11-03 while checking the holes [T-0160](T-0160-1-1-rewritten-safe-dazzle-has.md)'s
promise rests on. Decided by the user the same day (r113): every static's drop runs at exit,
in a fixed order, and a dropped static is left at its default.

**Where the code is**

| Piece | Where |
|---|---|
| A static kept in a cell C++ never destroys; which ones (`in_cell`) | `static_decl` and `in_cell` in `dz_cpp/src/lib.rs` |
| A use of a celled static reads `.value` | `hir::ExprKind::Static` in `Emitter::expr` |
| Each module's `dz_drop_statics__()`, in reverse declaration order | `drop_statics_fn` in `dz_cpp/src/lib.rs` |
| The exit hook: modules before their dependencies, `std` last | `drop_all_statics` in `dz_cpp/src/lib.rs` |
| `StaticCell<T>` and `drop_static` | the runtime shim, `dz_cpp/src/runtime.rs` |
| The test | `tests/run/statics` (`Closer`, `Ledger`) |
| The rule | §7.7, the bullet "A static that owns resources" |

**What was wrong.** A static was a C++ `inline` variable, and C++ destroys those in reverse
order of construction, which across files it does not promise. A static's `drop` that read
another static could find it already destroyed. This safe program read freed memory at exit
and printed a garbage sum:

```dz
impl Drop for Closer {
    fn drop(&mut self) {
        let mut sum = 0
        for v in LOG.lines { sum += v }
        println!("closer saw {} lines, sum {}", LOG.lines.len(), sum)
    }
}
static CLOSER: Closer = Closer{}
static LOG: Log = Log{}    // declared second, so C++ destroyed it first
```

A `thread_local` static was worse: C++ destroys those before any other static, so a static's
drop that built a `String` in a region reached the destroyed `SCOPE_POOL`.

**What it is now.** A static with something to drop lives in a `StaticCell` that C++ never
destroys. At exit the program drops them itself: a module's statics before those of the
modules it uses, `std` last, and within a module in reverse order of declaration. Each is
moved out and left at its default, as a moved-from value is, so a drop that runs later and
reads it sees an empty value: defined, and free at run time. The hook is registered with
`atexit` by a static initialiser in the root module's file, so it runs after `main` returns,
on `exit`, and for a library linked into a C++ program.

**Not covered.** A `thread_local` static on a thread C started (an audio callback) is never
dropped: Dazzle has no threads until T-0011, which has to decide when a thread's statics
drop. A static of an imported C++ class is still destroyed by C++.
