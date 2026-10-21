---
id: T-0044
title: `hash_combine` relies on `u64` arithmetic wrapping
milestone: M12
status: open
kind: deferral
area: stdlib
where: `dz_project/src/std/core/hash.dz`, `hash_combine`
---

**What it relies on.** `hash_combine(seed, h)` is how a struct mixes its fields' hashes
(§14.2), and like every hash mixer it is built out of arithmetic that **overflows on
purpose** — multiply, add, shift, and let the high bits fall off. It is written with plain
`+` and `*` on `u64`.

**Why that is a loose end.** C++ guarantees it: unsigned arithmetic wraps, so the generated
code does the right thing today. **The spec does not say it** — §5 leaves the overflow of
plain arithmetic unstated, and §5.1's `[PROPOSED]` signed-overflow check in debug builds
would, if adopted as written, make this depend on a rule nobody had decided. So the function
is correct by the backend's accident rather than by the language's promise.

Two things have changed since this was written: **`wrapping_add` exists**, and since r50 it
is **in the same module**, so the explicit spelling is available with no import. And 18.63
settled that **overflow wraps** — `dz build` passes `-fwrapv` for generated code (r59,
`docs/log/m12.md`) — so the behaviour is now a decision rather than an accident.

**What is actually left.** Less than the row's title suggests, and it is worth being precise
about it:

1. **Say it in §5.** The decision exists (18.63) and the compiler implements it; §5 should
   state what plain `+` does on overflow for both signed and unsigned, so a reader of the
   spec can tell whether `hash_combine` is relying on a promise. That is a spec edit and a
   §21 revision row, not code.
2. **Then decide whether `hash_combine` should say it anyway.** Even with wrapping
   guaranteed, writing `wrapping_add` and `wrapping_mul` in a mixer documents the intent at
   the site — a reader can see the overflow is wanted rather than tolerated. Against that:
   if plain `+` wraps, the explicit form is noise, and `std` using it would imply plain `+`
   is unsafe.

Either way the function's *behaviour* does not change, which is why this sat: there is no bug
here, only an unstated dependency. Settling item 1 closes it, and item 2 is a style call.

A test pinning `hash_combine`'s output for known inputs would be worth having regardless —
it does not exist, and a mixer whose result nothing asserts is free to change silently.
