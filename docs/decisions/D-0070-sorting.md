---
id: D-0070
title: Sorting
status: decided
---

**`List::sort` (by `Ord`, through a `Sort` trait implemented for `List<T: Ord>`) and `List::sort_by(fn(&T, &T) -> bool)`; a heapsort.** A trait rather than a method, because a method cannot require more of `T` than its type does. Heapsort over quicksort or merge sort: no allocation and no quadratic input, which a frame budget cares about more than the constant factor; stability is given up, and a stable sort can come later as its own name. `String` and `Path` gain byte-order `Ord`, so `Directory::list` can return a sorted, platform-independent order (§14.4)

**2026-11-04, T-0096:** the heapsort is now an introsort, quicksort with the heapsort as its
fallback, which keeps both reasons above (no allocation, no quadratic input) at about a fifth
of the time. Nothing a program can rely on changed: the sort was not stable, and is not.
