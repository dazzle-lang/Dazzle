---
id: T-0230
title: A List of an imported C++ class with no default constructor does not compile
milestone: M12
status: done
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

**Done** (2026-11-17, r140): the user's choice, "template and refusal".

- **The C++ compiles.** `List::resize` builds elements with the runtime's
  `construct(T&)`, a function template. It now builds `T()` under `if constexpr
  (__is_constructible(T))`, so for a class with no default the construction is discarded
  rather than compiled. This is the same effect as making each such method a member
  template, and simpler: the runtime is already a template there. `__is_constructible` is a
  compiler built-in in GCC, Clang and MSVC, as `__is_class` is, so the runtime still needs no
  `<type_traits>`.
- **A call is refused.** An imported class records `no_default` when it declares
  constructors and none can be called with no arguments (`collect_foreign`). Monomorphization,
  which sees every call with its concrete type arguments, reports `DZ0590` at a call to a
  method whose body builds a type parameter at its default (`construct(x)` with no value)
  where that parameter is such a class (`refuse_no_default`). That covers a call reached
  through another generic body's instance too.

`tests/run/cppinterop`'s `Anchored` lost the default constructor added as a workaround, so
the fixture now builds a `List` of a class with none (its C++ failed before this, checked).
`resize_of_a_class_with_no_default_is_refused` in `crates/dz/tests/run.rs` pins `DZ0590`.
