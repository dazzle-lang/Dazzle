---
id: T-0256
title: MSVC in CI, warning-clean at /W4
milestone: M21
status: open
kind: feature
area: tooling
where: `.github/workflows/ci.yml` (the test matrix ~80–93: `cxx` per OS, passed as `DZ_CXX`)
---

M21's criterion. Invariant 7 says generated C++ compiles warning-clean on more than one
compiler; once `cl.exe` is supported it is one of them, and only CI keeps it so.

**What it would take.** A Windows row in the matrix with `DZ_CXX: cl`, after T-0253 and
T-0254, running the whole suite: every `tests/run` program and every example built and run,
with warnings as errors at `/W4`. MSVC warns about things the others do not (C4100 unused
parameters, C4127 constant conditions, C4244 narrowing); each is fixed in the emitter where
the generated code is at fault, and disabled with a comment saying why where it is not.

Imported headers: libclang already parses with the MSVC target on Windows (T-0020), so
`import cpp` reads the same headers `cl.exe` compiles against; check that nothing in the
import cache assumes the GCC family.
