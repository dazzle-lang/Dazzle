---
id: T-0118
title: Skip drops that only free region memory
milestone: none
status: open
kind: feature
area: ownership
---

Step 5 of D-0091's plan, split out of [T-0107](T-0107-design-ties-kills-clauses-compiler-placed.md);
after [T-0117](T-0117-memory-placed-by-compiler-regions-result.md).

**What it is.** When everything a value owns is in a region that is about to be released, its
`drop` need not run if all it does is free memory: a `String` or a `List<i32>` in a scope
region costs nothing at scope exit. A drop with another effect (`File` closing a handle)
still runs.

**Why it matters.** Most of a region's speed comes from releasing it at once; walking every
value to free memory the region frees anyway gives part of it back.

**Why it is this way.** It needs placement first, and a way to know a drop only frees memory.

**What it would take.** Compiler work only (`docs/log/`).

**Settled in r90: no syntax; the compiler proves it.** A drop only frees when its body does
nothing but free memory the value owns, through `Allocator::free`, and drop parts whose own
drops only free. It is worked out per type instance, with monomorphization, since `List<T>`
depends on `T`: `List<String>` and `Box<String>` qualify; `File` (it closes a handle), `Rc`
(a count other owners see) and any drop that calls C, a function value or another trait
object do not. A marker a type wrote would be trusted, and wrong the day its drop gains an
effect. The placement hint says when a drop is skipped.
