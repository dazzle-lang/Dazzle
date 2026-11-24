---
id: D-0116
title: A move runs no code of the program's
status: decided
---

**Decided by the user on 2026-11-23**, when fixing a field's zero-argument constructor running
for nothing showed that resetting a moved-from value evaluated the field's declared default.
In the spec as §6.4, r149.

**The rule.** A moved-from value is reset without running any code the program wrote:

- a field whose default is a constant goes back to it;
- a field whose default runs code, `= Counted()` or `= String("text")`, or whose type has a
  zero-argument constructor, is left **empty**: numbers zero, pointers null, an optional
  `none`, and a struct's fields by this same rule;
- a union whose default runs code keeps its variant, with its payload emptied by this rule.

**Why.** §6.4 already promised that a move "costs a few stores", is "a pointer steal however
much `a` owns", and that dropping a moved-from value is "a no-op for the resource". Evaluating
the default broke all three: one move of a struct with `name: String = String("long text")`
made as many allocations as building a second value, and a default such as `= open_log()`
would acquire a resource on every move. A type with a `drop` of its own already reset its
fields without calling its constructor, so its moved-from state could already differ from a
freshly built value.

**The cost.** A `drop` that reads a moved-from field sees it empty rather than at a default
that runs code, so a `drop` assuming such a field always holds its default must handle the
empty case. Nothing else can see it, since using a moved-from value is `DZ0401`.

**Rejected.** Keeping §6.4 as it was and correcting its cost claim: a move would go on
allocating and running constructors. Evaluating a default once and copying it: it changes how
often the default's effects run, and a value that owns something cannot be shared. Skipping
the reset where `drop` does not read the field: whether a constructor runs on a move would
then depend on the body of an unrelated `drop`.
