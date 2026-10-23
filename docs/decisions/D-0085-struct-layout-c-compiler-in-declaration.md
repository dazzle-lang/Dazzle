---
id: D-0085
title: Struct layout is the C++ compiler's, in declaration order
status: decided
---

**A struct's fields are laid out in the order they are declared, a base's first, with the
padding and alignment the target's C++ compiler gives that struct; the compiler never
reorders them. Chosen by the user on 2026-10-23.**

The spec had never said what a struct looks like in memory. The compiler already emitted
fields in declaration order and `T::size` was already `sizeof` (§5.5), and T-0071 leaned on
that as if it were a rule. It came up over T-0046, which needs to know whether two structs
share a layout, and the user chose between the two possible answers:

- **The C++ layout (chosen).** A Dazzle struct is layout-identical to the C++ struct that
  declares the same fields in the same order, which is the struct Dazzle emits. So it can
  be handed to C or C++ code by pointer with nothing in between, an imported C++ struct
  (§15) follows the same rule as one written in Dazzle, and `T::size`, `T::align` and a
  field's offset are whatever the C++ compiler says, on every target.
- **Reordering for the smallest size (rejected)**, as Rust does by default. It would have
  made a Dazzle struct differ from an imported one that has the same fields; made the
  generated C++ list fields in an order the programmer did not write (§16.2); and taken
  away the order that game and embedded code often chooses on purpose: hot fields
  together, or a layout that must match a GPU buffer, a file format or a hardware register
  block. It would also have needed an opt-out for all of those, and in a language whose
  every struct is a C++ struct, the opt-out would be the usual case.

The cost is that a struct written carelessly carries padding: `a: u8, b: u64, c: u8` takes
24 bytes where `b, a, c` takes 16. The programmer orders the fields, largest alignment
first, when size matters. A warning that points out avoidable padding would help, and
would be tooling, not a language rule.

Spec: §7.3, §5.5.
