---
id: T-0172
title: An ECS benchmark against Rust and C++: a query inside a query
milestone: M15
status: open
kind: feature
area: bench
---

Planned for 0.2 by the user on 2026-11-09. The ECS is a benchmark, not a library: simple, a
few hundred lines in each language, not a robust engine.

**What it is.** A whole-program benchmark in `tests/bench_programs/ecs`, written three times,
in Dazzle, Rust and C++, around the same minimal entity-component-system, plus Rust and C++
reference points on real ECS libraries. The program is a small battle simulation: a few
thousand units with `Position`, `Velocity`, `Team` and `Health`, stepped for a fixed number
of frames, where each frame:

1. moves every unit (one query);
2. pushes apart units that overlap, changing **both** units' `Velocity` (a query inside a
   query over the same storage);
3. lets each unit damage the nearest enemy in range (an outer query reading `Position`
   and `Team`, writing **another** entity's `Health`);
4. despawns the dead and spawns reinforcements, after the loops.

Steps 2 and 3 are the point. In Dazzle they are written directly, because spec §6.7 allows
aliasing that moves nothing: `&mut` to one element beside `&` to another is fine, and only a
call that can move the storage kills a view.

A sketch; the query API is this task's to design:

```dz
for x in world.query2<Position, Velocity>() {
    for y in world.query2<Position, Velocity>() {
        if x.entity != y.entity && overlap(x.first, y.first) {
            let p = push(x.first, y.first)
            x.second.v += p                   // writes one entity's Velocity
            y.second.v -= p                   // and another's, inside the same loops
        }
    }
}
world.spawn(...)                              // fine here; inside the loops it kills the
                                              // storage both queries read: refused
```

In Rust, a query borrows the world (or its storage) and the inner query conflicts with it,
so the usual code collects the changes into a temporary list and applies them after.

**Why it matters.** It shows the claim behind §6.7 on the kind of code games are made of:
Dazzle refuses what is unsafe (a structural change while iterating) and allows what is safe
single-threaded, where Rust refuses both. C++ allows both, including the unsafe one.

**The rules, so the comparison stands up** (they extend `tests/bench_programs/README.md`):

- **The same design in all three.** Entities are an index and a generation; each component
  type has a sparse set (a dense array plus an index from entity to slot), found by type id.
  Queries walk the smallest set and look up the others. The three halves differ only where
  the language makes them differ.
- **Rust gets its best idioms, not only `collect`.** The Rust half is timed in its variants:
  collect-then-apply; index loops with `get_disjoint_mut` or `split_at_mut`; and interior
  mutability (`Cell` or `RefCell` per storage). If one of these is as clear and as fast as
  Dazzle, the report says so. A comparison against a strawman convinces nobody who writes
  Rust.
- **Reference points, reported apart.** `bevy_ecs` (with `iter_combinations_mut`,
  `ParamSet` and `Commands`) and `hecs` (`query_one`, `CommandBuffer`) for Rust, EnTT for
  C++. Their numbers mostly measure their own designs (archetypes against sparse sets), so
  they sit beside the main table rather than in it.
- **What is reported**: run time, peak memory, allocations, and the code of steps 2 and 3
  from each half side by side. Collecting may cost little time; the report states what was
  measured, whichever way it goes, and the code shape is the main exhibit.
- **Same work, same checksum**: input from the shared SplitMix64, and a `checksum` over every
  surviving unit's state, which `tools/bench_programs.py` holds all halves to. Iteration
  order is fixed by entity index so that floating-point sums agree.
- **Comparable builds.** Rust in release with `opt-level = 2`, `lto`, `panic = "abort"`
  (C++ has `-fno-exceptions`), and wrapping arithmetic, which release builds already have.
  Rust and Dazzle both check bounds; C++ does not, and the report says so.

**What it would take.**

1. The Dazzle ECS in `tests/bench_programs/ecs/src/`. Type-erased storage needs a trait
   object per storage and `std::reflect`'s type id; a query of two components is
   `query2<A, B>`, since generics have no variadic parameters. What `std` lacks on the way
   ([T-0177](T-0177-bitset-set-small-integers-in-std.md) for component masks, perhaps)
   goes into `std` or a task.
2. `main.cpp` with the same ECS; `rust/` a Cargo crate with no dependencies, and its
   `Cargo.lock` checked in.
3. `reference/`: a Cargo crate on `bevy_ecs` and `hecs`, and a C++ file on EnTT's single
   header, vendored (MIT). Check that EnTT builds with `-fno-exceptions -fno-rtti`. The
   reference crates fetch from crates.io, so they stay off the test suite's quick check.
4. `tools/bench_programs.py` learns a third half: build with `cargo build --release`, time
   it the same way, include it in the checksum check and the Markdown table.
   `--quick` runs the Rust half only if `cargo` is on the path.
5. A section in `tests/bench_programs/README.md` and the results in `RESULTS.md`.

Compiler work and a benchmark; no language change. If the Dazzle half needs something the
language refuses, that is a finding to bring to the user, not something to work around.
