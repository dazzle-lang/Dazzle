---
id: T-0112
title: cstr behind a pointer, and text at the C and C++ boundary
milestone: none
status: done
done: r100, 2026-10-31
kind: feature
area: interop
---

**Done, r100 (2026-10-31).** `Ty::CStr` is `&cstr`, a view; the new `Ty::ConstCStr` and `Ty::RawCStr` are `const &cstr` and `^cstr`, all three `const char*` in C++. `foreign_ty` maps `const char*` to `^cstr`, which takes the other two by assignability and a `String` or `Path` by `ImplicitAs<&cstr>` (`implicit_as` asks for `&cstr` when `^cstr` is wanted). `or` on a `^cstr` gives a `&cstr` (`or_expr`, and its C++), and measuring one as `&str` is `unsafe`. `untied_result` now asks a `&cstr` result for a tie and no longer exempts an `extern fn`; a `^str` or `^[]T` result is a pointer and may stay untied. std: `dir_next` returns `^str`, `null` at the end, and its runtime `Opt<str>`; `process_arg` is `const &str`; the memory tracker's files are `const &cstr`. **Not done:** a C++ `std::string_view` parameter or result (`&str`, `^str`), since the importer has no `string_view` mapping yet.

Step 5 of [D-0093](../decisions/D-0093-views-are-borrowed-no-bare-str.md); the steps are
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
pass text to raylib, SDL and sokol everywhere. **Settled in r90:** `process_env` returns
`^str` rather than eliding to its argument, as point 4 says for text an `extern fn` does not
own (it is valid until the next call); `Process::env`, its one caller, converts it in
`unsafe` and copies it. The memory tracker's `Allocation` (`memory/allocation.dz`) keeps
source files in `cstr` fields, which become `const &cstr`.

| Piece | Where |
|---|---|
| `Ty::CStr` | `dz_hir/src/ty.rs` |
| Imported C and C++ types | `foreign_ty` in `dz_check/src/lib.rs`; `dz_cppimport` |
| `as_cstr`, `ImplicitAs<cstr>` | `dz_project/src/std/text/string.dz`, `io/path.dz` |
| The exemptions to remove | `untied_result` in `dz_check/src/ties.rs` |
| The runtime's text functions (`dir_next`, `process_arg`, `process_env`) | `dz_cpp/src/runtime.rs`; `std/io/native.dz`, `io/process.dz` |
