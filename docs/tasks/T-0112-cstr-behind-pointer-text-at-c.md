---
id: T-0112
title: cstr behind a pointer, and text at the C and C++ boundary
milestone: none
status: open
kind: feature
area: interop
---

Step 4 of [D-0093](../decisions/D-0093-views-are-borrowed-no-bare-str.md); the steps are
listed in [T-0109](T-0109-text-only-borrowed-str-const-str.md).

**What it is.** A `cstr` is today a view with no tie at all, stored anywhere:
`String::as_cstr()` returns one into the string's bytes. It becomes usable only behind a
pointer: `&cstr` is a tied view, `const &cstr` a literal, and `^cstr` C's pointer, untied.

```dz
pub fn as_cstr(&self) -> &cstr in self@text
rl::DrawText("score", …)             // a `const &cstr`
let name: ^cstr = c::getenv("HOME")  // C's memory: untied
```

At the boundary: a C `const char*` parameter takes any of the three; a C function's result
is `^cstr`. A C++ `std::string_view` parameter is `&str`, and a result `^str`. An `extern
fn` declared in Dazzle returning text it does not own (`dir_next`) returns `^str`.

**Why it matters.** It is the largest text hole left: a `cstr` kept past a `String`'s growth
reads freed memory, and nothing reports it. It also removes r88's exemptions for a `cstr`
and an `extern fn` result with no input: the only untied results left are raw pointers.

**Why it is this way.** D-0039 made `cstr` a value view before ties existed.

**What it would take.** A language change (§5.3, §15, §16, a §21 row). The C importer's
mapping of `const char*` (`foreign_ty` in `dz_check/src/lib.rs`, `dz_cppimport`), the
literal-to-`cstr` rule, `ImplicitAs<&cstr>` for `String` and `Path`, and the examples, which
pass text to raylib, SDL and sokol everywhere. `process_env` returns a copy or a `^str`
rather than eliding to its argument.

| Piece | Where |
|---|---|
| `Ty::CStr` | `dz_hir/src/ty.rs` |
| Imported C and C++ types | `foreign_ty` in `dz_check/src/lib.rs`; `dz_cppimport` |
| `as_cstr`, `ImplicitAs<cstr>` | `dz_project/src/std/text/string.dz`, `io/path.dz` |
| The exemptions to remove | `untied_result` in `dz_check/src/ties.rs` |
| The runtime's text functions (`dir_next`, `process_arg`, `process_env`) | `dz_cpp/src/runtime.rs`; `std/io/native.dz`, `io/process.dz` |
