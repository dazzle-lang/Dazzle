---
id: T-0221
title: A module's private functions have external linkage, so clang++ does not inline them: raytrace's gap
milestone: M15
status: done
kind: feature
area: codegen
where: `crates/dz_cpp/src/lib.rs`: `Emitter::file_local_fns`, `stays_in_cpp`, `free_fn`, the `.cpp`'s assembly (`local_protos`); `crates/dz/tests/run.rs`: `a_viewed_list_is_still_bounds_checked`'s search for a definition
---

Found by the `raytrace` benchmark (T-0174, 2026-11-14), which runs 1.25× its C++ half on
clang++ and 1.11× on g++ (`RESULTS.md`, 2026-11-14). No profiler was available (`perf_event_paranoid` is 3 on the
machine), so each cause was found by editing the generated C++ in a copy, or the C++ half
toward it, and timing that. Numbers are clang++ `-O2 -flto=auto`, 960×540 at 16 rays a
pixel, before the switch cases borrowed (575 ms, C++ 455 ms).

**1. Private functions are not inlined (most of the clang++ gap).** `intersect`, called
from one place (`Scene::closest`), stays an out-of-line call in the Dazzle build, where the
C++ half inlines it. The C++ half's functions are in an anonymous namespace; Dazzle's are
declared in the module's header with external linkage, even when they are not `pub`, and
LTO does not make up for it. Making the generated `intersect` `static` (its declaration
removed from `raytrace.h`) took the run from 575 ms to 531, the same as
`__attribute__((always_inline))`; marking the C++ half's `intersect` `noinline` took it from
455 ms to 573, Dazzle's time. What to build: emit a function nothing outside its own file
can call with internal linkage (an anonymous namespace, or `static`), or otherwise let the
optimiser see it has one caller. What can call it is the question: a module's `.inst.cpp`,
the other half of a module compiled as one (T-0199), and the debug build's per-file
objects. g++ is unmoved by this (711 ms either way): it keeps `intersect` out of line in the
C++ half too.

Items 2 and 3, and what the switch copy was, are below as they were found; 2 and 3 are
T-0222, where item 2's union was tried in the generated code and did not pay.

**2. `Opt<T>()` constructs its payload.** `return none` from a `Hit?` function builds a
whole default `Hit` (`Opt() : value(), has(false)`), where `std::optional` constructs
nothing. Removing `value()` alone changed nothing, since `Hit`'s fields have default member
initialisers; the C++ half with an optional shaped like Dazzle's (`T value{}; bool has`)
ran 27 ms slower on g++ (623 to 650) and no slower on clang++. Holding the payload in a
union, constructed only on `some`, is the fix if it measures.

**3. The rest of the g++ gap is not found.** After the cases borrowed, about 40 ms of the
67 between the halves on g++ (690 ms against 623) is neither of the above.

A switch case's payload copy (`case Sphere(s)`) cost 22 ms on clang++; that one is the
program's choice, not the compiler's (D-0043: `s` copies, `&s` borrows), and the benchmark
borrows, as its C++ half does.

**Done 2026-11-14 (item 1).** A free function that only its own module's `.cpp` can call is
declared and defined `[[maybe_unused]] static` there, its declaration at the top of the
`.cpp` instead of in the header (`Emitter::file_local_fns`). It qualifies when it is not
`pub`, not generic, not `extern`, not `#[inline]`, has no attribute, is not `main`, and
every body naming it is a non-generic function or a method, constructor, `copy` or `drop`
of a non-generic type, not `#[inline]`, with no default arguments. The decision reads only
the module's own code, since an incremental build re-emits only the modules an edit can
reach (T-0197); so a module declaring a macro, whose expansions elsewhere might name its
functions, is left as it was, and so is `std`, whose C++ every program shares (T-0182).
`[[maybe_unused]]` keeps an uncalled private function from becoming a C++ warning
(invariant 7); checked with clang++ and g++, debug and release, on a program with an unused
function and two that only call each other.

`raytrace` on clang++ went from 565 ms to 500 (C++ 454), 1.25× to 1.10×; on g++ it stayed
at 687 (623), 1.10×, since g++ keeps `intersect` out of line in both halves. The other three
programs moved within their noise. Methods are not covered: a member cannot have internal
linkage while its class is declared in the header, and the C++ half's `Scene` methods
are internal only because the whole class is in an anonymous namespace.
