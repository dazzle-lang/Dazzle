---
id: T-0278
title: Rc counts atomic per type or per field, not for the whole program
milestone: M19
status: open
kind: design
area: concurrency
---

**Asked by the user on 2026-11-27**, after T-0266 made `Rc` and `Weak` counts atomic in any
program with a `parallel` block, a `parallel for` or `spawn`. Then every `Rc` in such a
program pays an atomic increment, about 20 cycles where a plain one is 1, and the C++
compiler can no longer cancel the count up and down every `or` and `if let` does, even for
an `Rc` only ever used on one thread. The user asked for it to be decided per usage, not for
the whole program. Not decided yet; the two options discussed:

1. **Per type.** A compile-time constant per type, a builtin such as `concurrent(T)`, true when
   a `parallel` block or loop reaches a value of `T`, which `std`'s code (or `unsafe` code)
   reads to choose atomics: `if concurrent(Rc<T>) { … } else { … }`, the branch not taken never
   generated. General, not special to `Rc`. An `Rc<Texture>` that reaches parallel code
   somewhere is atomic everywhere, `Rc<Button>` and `Rc<UiElement>` alike (they share a count),
   and `Weak<T>` with `Rc<T>`. The spelling is the user's.
2. **Per field and per local**, the user's own idea (`Rc` and an atomic `Arc` underneath,
   chosen by the compiler). Sound, since a plain count is only touched while no block or loop
   runs: `Sprite.texture` atomic because sprites reach parallel code, `Cache.textures` and a
   local never given to it plain, converting at assignment. A field belongs to its struct,
   so it is atomic for every value of it; a container is two instances; a function given
   both kinds is compiled twice. Much more work, and invisible in the language.

Per call site is not possible: a handle's drop is a C++ destructor, one per type, so the
same `Rc<Texture>` cannot count plainly in one function and atomically in another without
two C++ types or a check at run time. `spawn` needs neither, since it takes no `Rc`.

The recommendation was per type first, per field built on it once a benchmark shows what it
saves. A spec change to §22.4 either way.

## Where the code is

| Piece | Location |
|---|---|
| The whole-program switch | `runs_in_parallel` in `crates/dz_check/src/threads.rs`; `native_h` in `crates/dz_cpp/src/lib.rs`; `DZ_PARALLEL` in `NATIVE_H` (`crates/dz_cpp/src/runtime.rs`); the fingerprint in `Facts::of` (`crates/dz/src/reuse.rs`) |
| The counts | `crates/dz_project/src/std/memory/rc_block.dz` (`rc_count_*`), `rc.dz`, `weak.dz` |
| What reaches parallel code, by type | `handles_reached` in `crates/dz_check/src/threads.rs`; `record_parallel` in `crates/dz_flow/src/ties/body.rs` |
