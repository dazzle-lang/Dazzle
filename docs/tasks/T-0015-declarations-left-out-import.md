---
id: T-0015
title: Declarations left out of an import
milestone: M7b
status: open
kind: deferral
area: interop
where: `dz_cppimport`, `Header::skipped`
---

**What this row is.** The catalogue of what `import cpp` does *not* bring across. Each is
recorded in `Header::skipped` with a reason, so nothing is silently dropped — but nothing
surfaces the list to the user either, which is the first thing to fix.

| Skipped | Why | Owed by |
|---|---|---|
| **Variadic functions** (`...`) | Dazzle has no varargs, by design (§12.9) | [T-0008](T-0008-variadic-c-functions.md) |
| **Function pointers** as parameters or members | the Dazzle `fn` type and the C convention need matching up | this row |
| A **`wchar_t` pointer** | no Dazzle type for it; its width is platform-dependent | this row |
| An **rvalue reference** `T&&` | Dazzle's `move` is not C++'s, and there is nothing to map it to | probably never — document it |
| A **`static` member function** | needs associated functions on a type (§7), which Dazzle does not have | §7 work, not interop |
| A **member whose type does not follow from a template instantiation's argument binding** | the importer resolves template members by substituting the instantiation's arguments, and gives up where that does not determine the type | this row |

**Why it matters.** Function pointers are the one that bites: a C library's callback is a
function pointer, so every C API that calls you back — raylib's `SetTraceLogCallback`, a
sort comparator, an allocator hook — is unreachable. That is a large share of what makes a C
library configurable. The `wchar_t` case bites on Windows-specific headers. The rest are
genuinely rare.

**What it would take.**

1. **Surface the list.** `Header::skipped` already holds everything needed; nothing reads it.
   A warning naming the count per header, and a full listing in
   `dz api <module>` ([T-0061](T-0061-api-dump-dz-api-module.md)), turns "that function does
   not exist" into "that function was skipped because it is variadic". This is small and
   worth doing before any of the rest.
2. **Function pointers**, the one real feature here: map a C function-pointer parameter to a
   Dazzle `fn` type, with §15.2's pointer conventions in between, and decide what a Dazzle
   function passed as one must satisfy (no captures, since a C function pointer has no
   environment, and an `extern` calling convention on the emitted side).
3. The rest get a line each in the spec saying they are not imported and what to do instead,
   rather than being left to be discovered.
