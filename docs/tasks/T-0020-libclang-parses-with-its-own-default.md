---
id: T-0020
title: Libclang parses with its own default target
milestone: M7b
status: open
kind: deferral
area: interop
where: `dz_cppimport`, the libclang invocation
---

**What goes wrong.** `import cpp` reads a header with libclang, and the **C++ compiler that
builds the program is a separate choice** (`DZ_CXX`, configurable at four levels since r33).
Nothing makes the two agree on a target triple. libclang parses with its own default — on
this machine the MSVC target — so a header that includes the C library's own headers finds
**MSVC's** `stdio.h`, `stdint.h` and friends, while **g++** then compiles the generated code
against **mingw's**.

**Why that is a real problem and not a tidiness one.** The two C libraries do not agree on
everything that matters to an importer: the width and signedness of `wchar_t`, which types
`size_t` and `time_t` alias, struct layout and padding in a few places, and which macros are
defined — and a macro-conditional declaration is the worst case, because libclang may import
a *different declaration* than the one g++ compiles. The result is a mismatch that shows up
as either a C++ error in generated code (a compiler bug by invariant 7) or, worse, a silently
different struct layout.

It has not bitten yet because the headers imported so far — raylib's — are self-contained
enough not to depend on the difference.

**What it would take.** Pass the target explicitly, which means knowing it:

1. **Ask the C++ compiler.** `g++ -dumpmachine` prints its triple, `clang++ --print-target-triple`
   the same, and MSVC needs its own handling. Cache it per compiler path, as the import cache
   caches headers ([T-0014](T-0014-import-cache-in-process-only.md)).
2. Pass `-target <triple>` to libclang, plus — on mingw especially — the compiler's own
   **system include paths**, which `g++ -E -v -x c++ /dev/null` reports and which are the
   thing actually being got wrong here. Without the include paths the triple alone may not be
   enough.
3. Fail loudly where the triple cannot be determined, rather than falling back to the default
   silently. A diagnostic saying "could not determine the target of `<compiler>`; headers
   will be read for `<default>`" is better than a mismatch nobody can see.
4. A test is awkward — it needs two toolchains — but a cheap one is possible: assert that the
   triple passed to libclang matches `DZ_CXX`'s `-dumpmachine` output, which catches the
   plumbing breaking even on a single-toolchain machine. Remember invariant: a test must not
   depend on the host, so it clears and sets `DZ_CXX` itself.
