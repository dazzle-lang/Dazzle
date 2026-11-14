---
id: D-0098
title: BinaryHeap: which element comes out first
status: decided
---

**`BinaryHeap<T: Ord>` gives the largest element first, and `Reverse<T>` gives the
smallest; chosen by the user on 2026-11-14 (T-0175).**

The three options T-0175 set out:

1. **The largest, as C++'s `priority_queue` and Rust's `BinaryHeap`**, with a `Reverse<T>`
   wrapper whose `Ord` is reversed for the smallest. Chosen: it is what both audiences
   already expect, so neither reads a heap's `pop` wrongly.
2. **The smallest**, which is what path finding and event queues want, but surprises anyone
   coming from either language.
3. **Given by a comparison at construction**, as `sort_by` takes one. The most flexible,
   but every heap stores the comparison, and the common case pays for the rare one.

What follows from it:

- **`Reverse<T: Ord>` is a struct with one public field, `value`**, beside `Ordering` in
  `std`: `Reverse<Node>{ value: n }` to build, `.value` to read. Its `Eq` is `value`'s and
  its `Ord` is `value`'s with the operands swapped. A field of its own and no constructor,
  since a constructor call would need its type argument written just as the literal does.
  It is copyable when `T` is, by §6.3's rule for a struct with no `Copy` or `Drop`.
- **Absence follows `List`** (§9.1, D-0081): `pop()` is a `T?` and `peek()` a `^T`,
  `null` when empty, so `let top = heap.peek() or return`.
- **A heap is built from a `List<T>` by a second constructor**, `BinaryHeap<T>(items)`, in
  `n` steps. This needed §7.2's "whose parameters the arguments fit" to be held to for a
  generic parameter: `items: List<T>` fits only a `List`, so `BinaryHeap<T>(16)` is the
  capacity constructor and not ambiguous.

Spec: §14.4, §7.2.
