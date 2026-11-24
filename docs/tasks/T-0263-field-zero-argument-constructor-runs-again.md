---
id: T-0263
title: A field's zero-argument constructor runs again when its struct is built or moved
milestone: M19
status: done
kind: bug
area: codegen
where: `crates/dz_cpp/src/lib.rs` (field declarations, emitted as `T field{};`, and the synthesised move constructor)
---

**What happens.** The emitted C++ gives every field a default member initialiser, `Sealed
inner{};`. A Dazzle constructor and a synthesised move constructor assign in their bodies, so
C++ first builds the field through its zero-argument constructor and then overwrites it:

```dz
struct Counted {
    pub n: i32 = 0
    pub Counted() {
        println!("zero-argument constructor ran")
        n = 1
    }
}

struct Holder {
    pub inner: Counted = Counted()
    pub id: i32 = 0
}

impl Drop for Holder {
    fn drop(&mut self) {}
}

fn main() {
    let a = Holder{ inner: Counted(), id: 1 }
    let b = a                   // prints the line twice more
}
```

Checked on 2026-11-23: building `a` prints the line twice, and the move twice more.

**Why it matters.** A constructor with an effect, such as counting instances or taking a
resource, runs when nothing asked for it, and a move costs a construction per field. It also
weakens D-0115: a private zero-argument constructor cannot be *reached* from outside its type,
but a struct holding that type still runs it before assigning the field. Nobody can read the
value it made.

**The way out, to be worked out.** Initialise in the constructor's member-initialiser list: a
move constructor from `other`'s field, and a Dazzle constructor's opening assignments from
their values. A moved-from value still has to go back to its defaults (§6.4), and `clone` runs
on a value at its defaults (§6.6), so those must not start calling a constructor either. Check
the cost with the benchmarks, since moves are hot.

**Done when** the program above prints the line once for `Counted()` and not at all for the
move, with a `tests/run` program to pin it.

**Done** (2026-11-23, `c9874f2`, r149). A field is built once, by a literal, a constructor or
the move constructor, and a move runs none of the program's code, which took a decision of its
own about what a moved-from field holds ([D-0116](../decisions/D-0116-move-runs-no-program-code.md)).
`tests/run/field_init` pins it; the build log is in `docs/log/tooling.md`.
