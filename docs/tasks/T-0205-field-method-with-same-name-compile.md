---
id: T-0205
title: A field and a method with the same name compile to invalid C++
milestone: none
status: open
kind: bug
area: codegen
---

Found on 2026-11-12 while writing T-0178's test. The checker accepts a struct with a field
and a method of the same name, and the C++ it emits does not compile, which invariant 7
counts as a compiler bug:

```dz
struct Holder {
    pub shape: Shape = Shape::Circle{ radius: 1 }

    pub fn shape(&self) -> &Shape in self {   // accepted by `dz check`
        return &shape
    }
}
```

```
error: redefinition of 'shape' as different kind of symbol
    const Shape& shape() const;
```

**What it would take.** Either answer is the user's call, since nothing in the spec or
`docs/decisions/` says whether the two may share a name:

1. **Refuse it** in the checker, with a new diagnostic pointing at the second declaration
   (a language change: a §21 row, and a case in both `typed/` and `typeerrors/`). A bare
   `shape` inside a method is then never ambiguous.
2. **Allow it** and mangle one of the two in C++ (a method `shape` as `shape_m`, say),
   which also needs a rule for what a bare `shape` means in a method body.
