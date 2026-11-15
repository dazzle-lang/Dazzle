---
id: T-0230
title: A List of an imported C++ class with no default constructor does not compile
milestone: M12
status: open
kind: bug
area: codegen
where: `crates/dz_mono/src/lib.rs`: `Called`, `called_methods` (T-0199); `crates/dz_cpp/src/lib.rs`: `has_method`; `crates/dz_project/src/std/collections/list.dz`: `resize`
---

Found 2026-11-14 while writing `tests/run/cppinterop`'s `Anchored` for D-0105. A
`List<shapes::Anchored>` the program only pushes to, inserts into and reads failed in C++:
`no matching constructor for initialization of 'shapes::Anchored'`, from `List::resize`'s
`construct(m_data[m_len])`, which default-constructs. The program never calls `resize`.

**Why it is there.** T-0199 gave an instance only the methods the program calls, but decided
per generic and name, not per instance: `BitSet` calls `resize` on its `List<u64>`, and
`File::read` on a `List<u8>`, so every `List<T>` has `resize`, and `sort_by` and its helpers
the same way. The log entry for T-0199 says why: per instance needs the type arguments
followed through every call, which instantiation only does for the modules a build redoes,
and the set must be the same in every header that carries an instance. A C++ error in
generated code is a compiler bug (invariant 7), so this is one, though only a C++ class with
no default constructor reaches it: every Dazzle type has a default.

**Ways out.** Per-instance method sets, T-0199's road not taken, which would also shrink
every header (about 5 of a `List`'s 36 methods against 18). Or the emitter writing such a
method so that C++ instantiates its body only when called, a member template or an
`if constexpr` on `std::is_default_constructible`, with the checker refusing a call to
`resize` on a `List` of a type that has no default. `Anchored` has a default constructor in
the meantime.

## Where the code is

| What | Where |
|---|---|
| Which methods an instance keeps | `crates/dz_mono/src/lib.rs`: `Called::keeps`, `called_methods` |
| Where codegen asks | `crates/dz_cpp/src/lib.rs`: `has_method` |
| The method that default-constructs | `crates/dz_project/src/std/collections/list.dz`: `resize` |
| The class that met it | `tests/run/cppinterop/vendor/shapes/include/shapes.hpp`: `Anchored` |
