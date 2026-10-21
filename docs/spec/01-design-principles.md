# 1. Design principles

Dazzle exists because of a specific complaint: C++ gives you the control and the
abstraction but drowns you in forty years of accumulated syntax and undefined behaviour;
Odin gives you the clarity but takes away encapsulation, RAII, and the ability to build
layered abstractions that hold up in a large codebase.

The principles, in priority order. When two conflict, the earlier one wins.

1. **You can always see the machine.** No hidden allocations, no hidden control flow, no
   garbage collector, no runtime you did not ask for. If a line of Dazzle allocates, it
   says so.
2. **Destruction is deterministic and automatic.** RAII, but with exactly three special
   member functions instead of C++'s five-and-a-half: a constructor, `copy`, and `drop`.
3. **Encapsulation is enforced, not conventional.** Private is the default. The compiler,
   not a naming convention, is what stops layer N+1 from reaching into layer N's guts.
4. **Errors point at your code.** Definition-site generic checking, monomorphization under
   our control rather than C++'s, and `#line`-mapped output all exist so that a mistake in
   a generic function is reported in *that function*, not in a 400-line instantiation
   trace.
5. **The escape hatch is a door, not a hole.** C++ interop is a first-class feature with a
   real header importer, not an `extern "C"` afterthought. You should be able to use
   raylib, Jolt, and Dear ImGui on day one.
6. **Compile fast.** A game programmer recompiles hundreds of times a day. Any design
   decision that trades compile time for cleverness loses.

## 1.1 Non-goals

- **Memory safety as a guarantee.** Dazzle is not Rust. It prevents use-after-move and
  double-free; it does not prevent dangling references. What it does instead is *mark* the
  places a dangling pointer can bite: every read or write through a raw pointer sits inside
  `unsafe` (§5.2), where a reader — or `grep` — can find it. `[DECIDED]`
- **A garbage collector, green threads, or any implicit runtime.**
- **Source compatibility with C++.** We *call* C++; we are not a superset of it.
- **Being small.** Odin is small, and that is the problem being solved here.
