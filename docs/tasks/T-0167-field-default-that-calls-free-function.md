---
id: T-0167
title: A field default that calls a free function of its own module does not compile
milestone: none
status: done
kind: bug
area: codegen
---

**Done 2026-11-14.** `field_default_prototypes` declares, before any struct is defined,
the module's free functions a field default calls; `free_fn` declares each again later,
which C++ allows. A declaration needs no type complete, and default arguments are not in
C++ declarations, so nothing else had to move. Test: `tests/run/names` (`Holder`).

Found on the way to [T-0166](T-0166-static-field-defaults-run-code-before.md), 2026-11-04.

**What is wrong.** A field's default may call a function (spec §6.2), and the checker accepts
one that calls a free function of the same module:

```dz
struct Holder {
    pub n: usize = peek()
}

fn peek() -> usize {
    return 3
}

fn main() {
    let h = Holder{}
    println!("{}", h.n)
}
```

The generated header defines `struct Holder` with `size_t n = peek();` before `peek` is
declared, so C++ refuses it: "'peek' was not declared in this scope" (DZ0803, a compiler bug
by invariant 7). A constructor call works, `pub p: Probe = Probe()`, since a constructor is
declared inside its struct.

**What it would take.** Declare the module's free functions before the struct definitions
that use them in a default, or emit such a default through the constructor's body in the
`.cpp`, where every declaration is visible. A function whose signature names the struct
needs the struct declared first, which a forward declaration gives.
