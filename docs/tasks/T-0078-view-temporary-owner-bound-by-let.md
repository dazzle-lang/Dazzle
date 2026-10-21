---
id: T-0078
title: A view of a temporary owner bound by `let` dangles undiagnosed
milestone: M12
status: done
done: r68, 2026-10-21
kind: feature
area: ownership
where: `dz_flow`; `dz_cpp/src/lib.rs`, `declare_then_assign`
---

**What is accepted.** A view taken from a temporary that owns what it views, and bound by
`let`, outlives its owner:

```dz
let c = String::from(name).as_cstr()    // the String is dropped at the end of the statement
rl::LoadFileText(c)                      // reads freed memory
let s = make_string().as_str()           // the same, with a `str`
```

Both compile with no diagnostic, and a quick test usually passes, because the freed bytes are
still there. Found while closing [T-0006](T-0006-cstr-from-runtime-str.md) (2026-10-21):
`let dangling = String::from(name).as_cstr()` printed the right length on Windows.

**Why it is this way.** The CI fix of 2026-09-15 (`docs/log/m12.md`) made codegen scope a
temporary owner to the end of its statement, which is what a temporary is promised. What
nothing does yet is notice that the statement *keeps* a view of it past that point. Passing
the view straight to a call is fine — the owner lives for the whole statement — so the rule
wanted is only about the binding.

**What it would take.** A flow rule: a `let` (or an assignment) whose value `borrows` (the
same test `emit_args` uses) and whose path to the view passes through an
`owning_temporaries` node is an error, at the temporary, saying it is dropped at the end of
the statement and to bind it first (`let owned = String::from(name)`). That is a new
diagnostic for an existing rule, so a `tests/flow` case naming the code and a `tests/errors`
case for each form above, not a language change.

**Done in r68.** `Flow::kept_view` in `dz_flow`: a `let`, plain assignment or `return` whose value borrows (and does not itself need a drop) and which reaches, through receivers, fields, indexes, slices, derefs and casts, a call or literal that needs a drop, is `DZ0407`. Call arguments are not followed, so `first_of(make().as_str())` is not caught — a function may return a view of anything. The whole corpus, `std` and the examples check clean; `tests/flow/view_of_temporary.dz` and two `tests/errors` files pin it. The first try counted `Path("a") / "b"` as a view, since every `List` keeps an allocator pointer; a value that needs a drop owns what it holds, so it is no view.
