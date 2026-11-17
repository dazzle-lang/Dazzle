---
id: T-0048
title: An imported C or C++ enum's variants are not in the type table
milestone: M12
status: done
kind: deferral
area: codegen
where: the variants are imported by `dz_cppimport` (`DeclKind::Enum`) and typed in `crates/dz_check/src/lib.rs` (`ItemSig::Enum`, with `values`); reflection's row is `describe_named` in `crates/dz_cpp/src/reflect.rs`; tested by `tests/run/cinterop`
---

**Done 2026-11-17 (D-0111, r143).** What this said was missing had mostly arrived since:
an imported enum's variants were named (`shp::shapes::Kind::Round`) and a `switch` over one
was exhaustive. What was still missing was the type table: `typeinfo_of` an imported enum
listed no variants, since reflection named each value `Type::Variant` in C++ and a C enum
need not have that spelling. It now writes the value libclang read from the header, so a
`typedef enum { … } MiniShape` lists `MINI_CIRCLE = 1` and `MINI_SQUARE = 4`
(`tests/run/cinterop`). The question below, whether a `switch` over an imported enum must
have a `default` arm, the user answered: they stay closed, as today. A value outside the
list ends a value-producing `switch` in a panic and runs no arm of a statement one.

What follows is the task as it was written.

**What is and is not imported.** When `import cpp` reads a C or C++ enum, the **type**
arrives properly — its name, its size and its backing integer type are all recorded, so a
parameter or field of that type works, and so does passing a value through. What does not
arrive is the **list of variants and their values**: the constants stay the header's to
spell.

```dz
rl::SetWindowState(rl::FLAG_WINDOW_RESIZABLE)   // the constant, spelled as the header has it
```

In practice that is usually enough, because C libraries expose their enumerators as
top-level constants anyway and those *are* imported. What is missing is anything that needs
the enum's own variant list.

**Why it was accepted.** Nothing needed it yet. Reading the values out of libclang is
straightforward (`clang_getEnumConstantDeclValue` per child cursor); recording them means
deciding what an imported enum *is* on the Dazzle side, and that is the real question.

**What it would take, and the decision inside it.** A Dazzle `enum` is a closed set: a
`switch` over one is checked for exhaustiveness, and a value outside the set is not
representable. A C enum is **not** closed — C code routinely stores flags, combinations and
out-of-range values in an enum-typed variable, which is what `FLAG_WINDOW_RESIZABLE |
FLAG_VSYNC_HINT` is. So importing a C enum as a Dazzle `enum` would promise exhaustiveness
the header cannot keep, and an imported `switch` would be wrong rather than merely
incomplete.

The honest shape is probably two things: the variants recorded for **reflection and tooling**
— so `dz api rl` ([T-0061](T-0061-api-dump-dz-api-module.md)) can list them, which is the
single most useful thing this unblocks — without making the type a closed Dazzle `enum`; and
a `switch` over an imported enum requiring a default arm. That is a language-visible rule, so
it wants a decision file before the importer work.
