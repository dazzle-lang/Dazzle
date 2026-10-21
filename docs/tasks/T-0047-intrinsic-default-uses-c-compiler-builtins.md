---
id: T-0047
title: An intrinsic default uses the C++ compiler's builtins
milestone: M12
status: open
kind: deferral
area: codegen
where: `dz_cpp/src/runtime.rs`, `caller_location`
---

**What goes wrong.** A parameter defaulted to `#location` (spec §12.7) must be filled in
with the **caller's** position, not the callee's. Where the call is written in Dazzle, codegen
knows the span and emits a `dz::SourceLocation` literal with all four fields. But a default
has to work when the call is *not* rewritten — so the shim provides `caller_location`, whose
own defaults are the C++ compiler's builtins:

```cpp
constexpr SourceLocation caller_location(const char* file = __builtin_FILE(),
                                         const char* function = __builtin_FUNCTION(),
                                         uint32_t line = __builtin_LINE()) {
    return SourceLocation{file, function, line, 0};
}
```

Two fields come out wrong wherever that path is taken:

- **the column is 0**, because C++ has no `__builtin_COLUMN` that g++ and clang++ agree on;
- **the function is the C++ name**, so a method reports `bump` rather than `Counter::bump` —
  `__builtin_FUNCTION()` gives the unqualified name, and for a monomorphized instance it
  would be the mangled one.

**Why it matters, and why it was accepted.** This is what a leaked allocation's recorded
site looks like under `--track-memory`, and what a panic from inside `std` points at. A
missing column means an editor cannot jump to the exact position; the unqualified function
name is ambiguous the moment two types have a `bump`. It was accepted because the Dazzle-side
path — where the call is written out and codegen supplies the literal — covers the common
case, and because the alternative is work in the emitter rather than a fix in the shim.

**What it would take.** Stop relying on the builtins: make every call site that passes a
`#location` default emit the literal explicitly, so `caller_location` is never reached. That
means codegen filling in defaulted `SourceLocation` arguments at the call rather than leaving
them to C++ — which it already does where it rewrites the call, so the work is extending that
to the cases it currently leaves alone (a call through a function value, a default on a trait
method — see [T-0039](T-0039-trait-method-non-constant-default-argument.md) — and a default
that reaches another default). Once no C++ default is involved, the column and the qualified
name are both just the span, and `caller_location` can go.

A test would pin a reported location's column and qualified function name from inside a
method in `std`, which nothing asserts today.
