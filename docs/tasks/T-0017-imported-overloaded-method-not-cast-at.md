---
id: T-0017
title: An imported overloaded method is not cast at the boundary
milestone: M7b
status: done
kind: bug
area: codegen
where: `dz_check/src/check.rs`: `method_by_args`, which returns the key it picked, and the two calls that record it (`mark_method_key`); `dz_hir`: `Body::method_keys`, `method_key`; `dz_cpp/src/lib.rs`: `call`, the imported method's casts; `tests/run/cppinterop`: `Summer`
---

**What the casts are for.** At the C boundary Dazzle's types and the header's are not always
the same spelling of the same thing — a `bool` that is an `int`, an enum that is its backing
integer, a reference where the header wants a pointer. `dz_cpp` bridges each with a
`c_cast` on the way in and a `ret_cast` on the way out, chosen from the imported
declaration's own types.

**What goes wrong.** For a **method**, `hir::Callee::Method` carries only the method's
**name**, not the key identifying *which* overload the checker picked. So `dz_cpp` can look
the declaration up — and therefore apply the casts — only when the name names exactly one
method. Where the header overloads it, the casts are skipped.

**Why it matters.** Imported overloaded methods *do* resolve correctly by argument type
(§15.3) — the checker gets the right one. The defect is downstream of that: the call is
emitted without the conversion the boundary needs, so whatever the C++ compiler does
implicitly is what happens. Most of the time that is the right thing, which is why nothing
has caught fire; where it is not, the symptom is a wrong value rather than a compile error,
and by invariant 7 a C++ error in generated code is a compiler bug — a *silently* wrong
value is worse.

Overloaded methods are common in C++ libraries, so this will matter as soon as
[T-0010](T-0010-proof-on-real-libraries-dear-imgui.md)'s fixtures exist.

**What it would take.** The fix is small and local: have the checker record the chosen
overload's key on `hir::Callee::Method` — it already computed it to resolve the call — and
have `dz_cpp` look the declaration up by key rather than by name. The work is in the HIR
shape (one more field, and every construction site) rather than in any logic.

What is harder is *testing* it: the cases where the cast matters and C++'s implicit
conversion does not do the same thing are narrow, so a test wants to be built around a
deliberate fixture header with an overload set whose parameters differ in exactly a way the
boundary must convert (a `bool`-as-`int`, an enum). Worth writing that fixture first, so the
fix is known to fix something.

Related: [T-0016](T-0016-overloaded-free-function-imports-only-its.md), which is the free
function's version of the same area — there, the overloads are not even imported.

**Done** (2026-11-17), filed as a bug, which it was: the generated C++ did not compile, or
could take a wrong value. The fixture came first, as this task advised: `Summer::total` in
`tests/run/cppinterop`, an overload set whose one member takes `const int xs[4]`, which
Dazzle passes cast to a pointer. Without the cast, clang++ found no overload to call.

The checker now records the key of the overload a call reached, where a name has several, in
a per-body side table next to `clones` (`Body::method_keys`, pruned by `rollback` like
`clones`). Codegen looks the declaration up by that key. A side table rather than a field on
`Callee::Method`, which every construction site would have had to fill for the rare call
that needs it.
