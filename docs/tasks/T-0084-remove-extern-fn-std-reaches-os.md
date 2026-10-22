---
id: T-0084
title: Remove extern fn: std reaches the OS through header imports
milestone: M12
status: open
kind: feature
area: interop
---

**What is wrong.** `extern fn` (§7.2, D-0065) declares a function whose body is C++ the
compiler ships, `dz_native.cpp`. Nothing on the Dazzle side checks that the function exists
on the target, that its signature matches, or which header declares it: the declaration
is a promise, and only the C++ compile of the generated code finds out if it is broken.
The user wants it gone (2026-10-22). std should reach the operating system the way a
program reaches a C library: a header import, whose declarations come from the header and
whose `#include` the generated C++ writes (§15.1).

r74 did this for the C heap (D-0084): `Allocators::Global` calls `malloc` and `free` from
std's own import of `stdlib.h`. About 30 `extern fn`s remain, in `std/io/native.dz`,
`std/time/native.dz`, `std/core/format.dz` and `std/core/formatter.dz`.

**Prerequisites.** What those bodies use and the importer cannot bring across yet:

| Prerequisite | What needs it |
|---|---|
| [T-0008](T-0008-variadic-c-functions.md), variadic C functions | `format_float` is `snprintf("%g")` / `snprintf("%.*f")` |
| [T-0015](T-0015-declarations-left-out-import.md), `wchar_t` | Windows paths: `_wfsopen`, `DeleteFileW`, `GetFileAttributesW`, `CommandLineToArgvW` take UTF-16 |
| [T-0014](T-0014-import-cache-in-process-only.md), the import cache on disk | `windows.h` takes 0.6–1.9 s to read on every build without it |

**What it would take, once those are in.**

1. Per-target code moves from C++ `#if defined(_WIN32)` into Dazzle `#if OS_WINDOWS`
   (§12.10), each branch importing its own headers: `windows.h` on Windows,
   `unistd.h`/`dirent.h`/`sys/stat.h` elsewhere. A recipe per target, under
   `[cpp.libs.….target.<os>]` as a program's would be (§15.1).
2. The UTF-8 to UTF-16 conversion for Windows paths (`OsPath` in `dz_native.cpp`) becomes
   Dazzle over `MultiByteToWideChar`.
3. The clocks: `std::chrono` is a C++ template library; `clock_gettime` and
   `QueryPerformanceCounter` are C and import.
4. Then `extern fn`, `DZ0317` and `dz_native.cpp` go, and §7.2 and D-0065 are revised.

Moving one area at a time is fine: each `native.dz` can switch on its own, as the heap did.
