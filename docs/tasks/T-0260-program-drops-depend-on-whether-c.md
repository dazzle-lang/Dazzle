---
id: T-0260
title: A program's drops depend on whether the C++ compiler elides a copy
milestone: M21
status: open
kind: feature
area: codegen
where: `crates/dz_cpp/src/lib.rs` (struct literals built in a lambda, `[&] { T x{}; x.f = …; return x; }()`); the runtime's move constructors, which leave a value at its defaults for its `drop` to run on (spec §6.4)
---

Found by T-0254 on 2026-11-19, building every `tests/run` program with `cl /Od`. Five of them
(`optionals`, `loops`, `moves`, `generic_methods`, `statics`) printed one `drop` more than
`expected.txt`, always of a value at its defaults: `drop 0`, `drop '' 0`.

The generated C++ makes objects the Dazzle program does not have. `let t: Token? =
Token{ id: 3 }` becomes `[&] { Token _dz_lit27{}; _dz_lit27.id = 3; return _dz_lit27; }()`.
Returning a named local is elided only by NRVO, which C++17 allows but does not require.
clang++ and g++ always apply it, even at `-O0`, so the expected outputs were written against
them. `cl.exe` applies it only at `/O2`, or with `/Zc:nrvo` (MSVC 17.4 and later). Without
it, `_dz_lit27` is moved from, and its destructor runs the program's `drop` on the
reset value.

T-0253 passes `/Zc:nrvo`, and with it all five match. But the output still rests on a
choice C++ leaves to the compiler, and a `drop` that prints, counts or frees makes that
choice visible. Two ways to stop relying on it:

- emit no named local where a prvalue would do: a struct literal built by position (T-0222
  did this where it could), or built into its destination;
- or have a move made by the generated C++, as opposed to one the program wrote, leave
  nothing for `drop` to run on.

The first keeps the generated code closer to the program. Either should come with a test
whose expected drops come from the program, not from a compiler's elision.
