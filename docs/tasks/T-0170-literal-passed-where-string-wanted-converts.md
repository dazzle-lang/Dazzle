---
id: T-0170
title: A literal passed where a String is wanted converts, and the converted copy is placed like a written one
milestone: M12
status: open
kind: feature
area: language
---

Asked for by the user on 2026-11-04, and put off by their choice: until this lands, code and
docs write `String("ada")`.

**What is missing.** `names.push("ada")` for a `List<String>`, `greet("x")` for a `fn
greet(name: String)` and `Player("x")` for a constructor taking a `String` are refused with
DZ0501 (expected `String`, found `&str`). Argument conversions exist (spec §7.2, D-0089),
but `std` declares them only from a `String` to text: `ImplicitAs<&str>` and
`ImplicitAs<&cstr>` for `String`, and `ImplicitAs<&cstr>` for `Path`. The other direction is
one implementation in `std`, under the same rule:

```dz
impl ImplicitAs<String> for &str {
    fn convert(&self) -> String {
        return String(self)
    }
}
```

**What was tried (2026-11-04).** With that `impl` added, `push`, a free function, a
constructor and a method all accepted a literal. A converted string kept in a list returned
to the caller, in a struct built and renamed through `String` parameters, and in a map
filled in a loop ran correctly. A runtime `&str` (`shout(owner.as_str())`) also converted, by
copying. There were no leaks under `--track-memory`, and nothing from AddressSanitizer or UBSan
with regions on or off. It is safe: `convert` returns an owned copy and borrows nothing, so
there is no tie and nothing can dangle, and the parameter takes ownership as of any `String`.
A `let` and a field still refuse a literal, since D-0089 converts arguments only.

**Why it waits: the converted copy is placed worse than a written one.** `String("ada")`
written at the call is built where the compiler places it, for a value stored into a local
container in that container's region (T-0145), with its drop skipped. A converted copy is
built inside `std`'s `convert`, which the placement pass sees as an opaque call, and
`dz placements` did not list it: as far as can be told it goes on the heap, so the short
spelling would be the slower one. The fix is to handle `ImplicitAs` conversions robustly:
treat a `convert` whose body only builds a value as that construction at the call site, or
inline it, so a converted argument is placed exactly as the value written out would be.
Confirm first where the copy actually goes today.

**Also, at the same time.**

- **`map.get("key")`** stays refused even with the conversion: `HashMap<String, V>::get`
  takes `&K`, a `&String`, and a conversion makes a `String`, not a reference to one. A
  reader who sees `names.push("ada")` work will expect `get("ada")` to work too. A lookup
  should take the key's borrowed form (`&str` for a `String` key), as Rust's `Borrow` lets
  it.
- **A copy you do not see.** `set_label("x")` called every frame allocates every frame, as
  C++'s `std::string` does. The parameter's type is the only sign; an inlay hint where a
  conversion allocates would make it visible (T-0089).
- **The language change** is a §21 row: which programs are valid changes.

**Where the code is**

| Piece | Where |
|---|---|
| An argument converted through `ImplicitAs` | `argument` and `implicit_as` in `crates/dz_check/src/check.rs`; the call is a `hir::Callee::Method` named `convert` |
| The help when a type does not convert | `implicit_as_help` in `crates/dz_check/src/check.rs` |
| Where an `impl ImplicitAs<T>` may be written | `crates/dz_check/src/impls.rs` |
| The trait, and `String`'s conversions | `crates/dz_project/src/std/core/implicit_as.dz`, `crates/dz_project/src/std/text/string.dz` |
| Placement: a value built where its container lives (T-0145), a result where the caller keeps it | `crates/dz_flow/src/storing.rs`, `crates/dz_flow/src/placement.rs` |
| `HashMap::get` | `crates/dz_project/src/std/collections/hash_map.dz` |
| The rule | spec §7.2, D-0089 |
