---
id: T-0056
title: A formatting macro's arguments go through `&Format`
milestone: M12
status: open
kind: deferral
area: stdlib
where: `dz_check/src/check.rs`, `format_macro`; `std/core/formatter.dz`, `write_args`
---

**What happens now.** A formatting invocation lowers to **one call** handing the pieces, the
specs and the arguments to `Formatter::write_args`, which takes `args: ..&Format` — so each
argument is passed as a **trait reference** and written with **one indirect call**:

```dz
fn write_args(&mut self, pieces: []str, specs: []str, args: ..&Format) {
    for i in 0..args.len {
        self.write_str(pieces[i])
        self.set_spec(specs[i])
        args[i].format(&mut self)        // indirect, through the vtable
    }
    …
}
```

**What §12.9 originally described** was the other shape: a **statically dispatched
`Format::format` call per placeholder**, emitted as a sequence of statements, with no vtable
and no indirection.

**Why the direct form was built first and dropped.** As a sequence of statements it ended
each argument's temporaries **at the end of its own statement**, before the line had been
written. §12.9 promises the opposite — *"a temporary one of them makes lives until the text
is written"* — which is what lets `println!("{}", format_name(e))` work when
`format_name` returns something that borrows. A single call keeps every argument's temporary
alive for the whole call, which is why the `&Format` form is correct and the sequence-of-
statements form was not (D-0080).

**What it costs.** One indirect call per placeholder, and a vtable requirement that has
knock-on effects: a **generic `impl` has no vtable**, which is exactly why a pointer cannot
be formatted ([T-0055](T-0055-pointer-has-no-format.md)) — `impl<T> Format for ^T` is
unusable here. So this is not only a few nanoseconds per placeholder; it is the reason one
category of type cannot be an argument at all.

**What it would take.** Either of two, and both are real work in the same place:

1. **Keep one statement's temporaries alive across several statements** in codegen — a
   scope that spans the emitted sequence — so the direct form becomes correct. Then
   `Format::format` is called statically per placeholder, generic `impl`s work, and
   [T-0055](T-0055-pointer-has-no-format.md) falls out for free. This is the version that
   pays for itself.
2. **A `write_args` instantiated per argument list** — a monomorphized variadic taking the
   arguments by their own types rather than as `&Format`. Keeps the single call and so keeps
   the temporaries, and dispatches statically. Costs one instance per distinct argument-type
   list, which in a program with many `println!`s is a lot of near-identical functions, and
   it is unclear whether Dazzle's generics can express it without a tuple type.

Option 1 is the one to aim for, and it is worth noting that it also removes the whole reason
[T-0055](T-0055-pointer-has-no-format.md) exists — so the two should be considered one piece
of work.
