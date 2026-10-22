---
id: D-0084
title: The C heap through an import of stdlib.h, and no alloc or free builtins
status: decided
---

**`std` reaches `malloc` and `free` by importing `stdlib.h` like any C library, and the
language's `alloc` and `free` builtins are gone; chosen by the user on 2026-10-22.**

r11 had made `alloc(size, align)` and `free(p)` builtins "until the stdlib's allocators
replace them". The allocators came in M12 but the builtins stayed, and any program could
call them: the ownership example, two run tests and §8.3's `Buffer` all did. The user asked
for them to go, and for the allocator to call the real C functions rather than a Dazzle
primitive.

- **The C heap is an import, not an `extern fn`.** The user rejected a hand-written
  `extern fn malloc(…)`: nothing checks that such a declaration matches a function that
  exists on the target, nor says which header declares it. A header import has both
  answers from the header itself, and the generated C++ includes it.
- **`std` carries its own recipe**: `[cpp.libs.libc]` with `import = ["stdlib.h"]`, as
  the dependency key `libc`. It is `std`'s, so a program cannot name it; a program that
  wants `malloc` imports `stdlib.h` itself.
- **A requested system header imports its declarations** (§15.1). Before, the importer
  passed over every declaration in a system header, so `import = ["stdlib.h"]` produced
  nothing. Now a system header named in `import` brings its own declarations and those of
  the system headers it includes; one reached only through a library's header still does
  not, so importing raylib does not import the C library.
- **Every build reads `stdlib.h` through libclang**, about 50–80 ms, once per process for
  every project (its cache key does not depend on the project). libclang becomes required
  for every program rather than only those importing C or C++; the installer bundles it
  (T-0076) and CI already had it. A cache on disk (T-0014) would remove the cost.
- **`extern fn` stays for now**, for the rest of what `std` does natively — files, clocks,
  the console, float formatting. Removing it is T-0084, which needs variadic C functions
  (T-0008), `wchar_t` (T-0015) and the on-disk import cache (T-0014) first.

Spec: §5.2, §6.3, §8.3, §9.8, §12.10, §14, §14.3, §15.1.
