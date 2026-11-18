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
renderer owns, are left to the user's Linux machine; T-0120 has the method. The decision on
the rule and on where the arena lives comes next, then the implementation.
