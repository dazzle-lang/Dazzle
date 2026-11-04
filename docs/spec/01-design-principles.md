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
6. **Compile fast.** Someone working in a large native codebase recompiles hundreds of
   times a day. Any design decision that trades compile time for cleverness loses.

## 1.1 Non-goals

- **Memory safety by refusing aliasing.** Dazzle is not Rust, but on one thread it makes
  Rust's promise: **a program with no `unsafe` has no undefined behaviour.** No read or
  write of freed memory, no payload used under another variant, nothing C++ leaves
  undefined. It keeps the promise without Rust's rule that memory is shared or changed,
  never both. In Dazzle it may be both, which single-threaded code needs; what it may not
  be is freed, or given another variant, while something else can still see it. Each part
  of the promise is a rule of the language:
  - a value used after a move is an error, and one is never dropped twice (§6.4);
  - a reference used after a call killed the memory it points into is an error, and a
    call reaches what it kills through one argument only (ties, §6.7);
  - reading or writing through a raw pointer, and turning an untied one into a reference,
    happen only inside `unsafe` (§5.2);
  - every value is initialised (§6.2), every index is checked in every build (§5.3),
    arithmetic is defined (§5.1, §10.6), `or unreachable` panics (§9.4), and a loop runs
    as written, even one with no side effects that never ends (§16.5);
  - a static is checked as a parameter is, and dropped at exit in an order a drop can rely
    on (§7.7);
  - a C or C++ function that can be handed a pointer is vouched for in its library's
    recipe, and one that keeps the pointer is `unsafe` to call (§15.1, §15.2).

  The promise ends where these rules do, and that is where to look for a bug:
  - **`unsafe`**, a block or an `unsafe fn`'s body, which vouches for what it does;
  - **`std`'s trusted bodies**: its containers' `kills` clauses are written by hand over
    `unsafe` code, and checked at run time in debug builds (§6.7, §14.2);
  - **the C and C++ a program calls**, and the `keeps` list a recipe vouches for (§15.1);
  - **threads**: Dazzle has none of its own yet, and how they share memory is undecided
    (§18.11);
  - **running out of stack**, which C++ does not define; on every platform Dazzle targets
    it ends the program.

  `[DECIDED — D-0094; revises D-0091's wording]`
- **A garbage collector, green threads, or any implicit runtime.**
- **Source compatibility with C++.** We *call* C++; we are not a superset of it.
- **Being small.** Odin is small, and that is the problem being solved here.
