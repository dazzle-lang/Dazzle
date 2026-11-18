# M22: placement finished

What §14.6 still said was "not yet" when 0.3 was planned: owner arenas (T-0120), frame
regions (T-0139), result placement for `virtual` methods (T-0141), more skipped drops
(T-0147), by-value parameters across modules (T-0142), a list in a region growing in place
(T-0201), and a memory budget or no heap (T-0143). The user moved them into 0.3 on
2026-11-18, as M22 (spec §19).

## 2026-11-18: `frame`, the program owner arenas need

T-0120 had waited for a program with a container of owning values emptied every frame:
everything in `examples/` and the benchmarks held plain data. `tests/bench_programs/frame`
is one, as a whole-program benchmark pair with a C++ half: a renderer's private
`List<Command>` emptied by `begin` and refilled with about 5,000 commands a frame, each
owning a formatted `String` and a `List<Point>`. Writing it needed a tie on `Renderer`
(`tie @commands = m_commands@items, m_commands[*]`) for its `kills` clauses, as the
diagnostic's help said.

`dz placements` puts both on the heap, "stored in `renderer` on every pass of a loop". Its
timings, and those of the same program patched by hand to build them in an arena the
renderer owns, were left to the user's Linux machine; T-0120 has the method.

## 2026-11-18: what an owner arena would gain, measured

On the Linux machine, `frame` is 1.04× its C++ half on clang++ and 1.19× on g++ as
generated. Patched by hand so the label and trail are built in one `ScopeArena`, dropped and
made again when the renderer's list is cleared, it is 0.74× and 0.95×: 0.71× and 0.80× the
time of the heap build, and 0.76× and 0.77× its instructions, at the same peak memory.
Skipping the drops too (T-0118's rule) is a further 4–6%. So the allocator is the gain and
skipped drops the smaller step after it. The table, and how the patched builds were made
with `dz build --release`'s own commands, are in T-0120.

## 2026-11-18: owner arenas, built for `frame`'s case (T-0120)

The user described what they expected the compiler to do: place the label and trail in the
loop's region, then, seeing they move into the renderer, directly in the commands' region,
which lives where the renderer does; on any path, since an `if` may move a value on only
one branch. The general questions (many small owners, sizing, a region shared by several
containers) wait until a program needs them.

Built that way. `dz_flow/src/arenas.rs` finds the fields that keep an arena: a private
`List` of owning values that its struct's own bodies only read, `push` and `clear`. The
proposer places a value stored into such an owner, a local of the caller's, in its arena,
unless the owner is used between building the value and storing it, or lent `&mut`, either
of which could reset the arena under it; the region check is told each arena site's
container and lets the value go nowhere else. `std`'s new `OwnerArena` lives in memory from
the owner's allocator and keeps its chunks on reset. In C++ it is a hidden `ArenaSlot`
member before the field, made on first use, reset after `clear()`, moved with the field.

`frame` went from 1.04× and 1.19× its C++ half to **0.76× and 0.96×** (clang++, g++), at
the same peak memory: within 2–3% of the hand patch, so reading the arena at run time costs
little. A caller in another module, other containers, locals and skipped drops on reset are
left for when a program needs them (T-0120 lists them).

One thing seen on the way, not changed: a struct local whose function returns one of its
plain fields (`return r.drawn`) keeps the heap, "outlives `…`", at HEAD as now, where the
same struct in `main` gets the region.
