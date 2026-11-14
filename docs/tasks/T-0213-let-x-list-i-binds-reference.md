---
id: T-0213
title: let x = list[i] binds a reference to the element, where let x = arr[i] copies it
milestone: M12
status: open
kind: design
area: language
---

Found writing `BitSet` (T-0177), 2026-11-14. **A decision for the user.**

**What happens.** On a type with `Index`/`IndexMut` (`List`, `Deque`), `xs[i]` has the
type `index_mut` returns, `&mut T` (or
`index`'s `&T`), as §5.2 and §11.2 say. So `let was = xs[i]` binds a *reference* to the
element, not a copy of it, even for an `i32`:

```dz
let mut xs = List<u64>()
xs.push(1)
let was = xs[0]
xs[0] = 5
println!("{}", was)        // 5
was = 3                    // writes xs[0]
```

An array behaves the other way: `let a = arr[0]` copies, and a later `arr[0] = 5` does not
show through `a`. Written with a type, `let was: u64 = xs[0]` copies too. Nothing is
unsound — the reference is to a slot that is still there, and a call that could move the
elements kills it as usual (§6.7) — but the same line means two things depending on
whether the container is an array or a `List`, and the reference reading is the surprising
one for anyone coming from C++ (`auto x = v[0]` copies) or Rust (`let x = v[0]` copies a
`Copy` element and refuses otherwise).

`BitSet::insert` hit it: `let was = m_words[word]` then `m_words[word] = was | bit` made
`was` the new word, so `insert` said every new integer was already there. It now writes
`let was: u64 = m_words[word]`.

**Options.**

1. **A `let` without a type copies a copyable element**, as an array's index does: the
   binding's type is `T`, and a reference is asked for with `let r = &mut xs[i]` (already
   spelled so in §6.7's examples). Move-only elements would need `&` or `&mut` to bind at
   all, or be an error as a move out of a borrowed place. Recommended: one meaning for one
   spelling, and the one both C++ and Rust users expect.
2. **Keep it**, and say so in §5.2 with the example above, perhaps with a warning when a
   binding from an index is written to the container before it is read.
3. **Make arrays match**, binding a reference: consistent, but surprising to everyone.

## Where the code is

| What | Where |
|---|---|
| the type of `xs[i]` on a user type | `crates/dz_check/src/check.rs`, the index expression (`index_mut` / `index`) |
| a `let` taking its initialiser's type | `crates/dz_check/src/check.rs`, `let` statements |
| the C++ (`uint64_t& was = …index_mut(…)`) | `crates/dz_cpp/src/lib.rs`, `StmtKind::Let` |
| the repro | the example above, as a `tests/run` program |
