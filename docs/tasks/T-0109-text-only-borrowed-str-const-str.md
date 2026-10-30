---
id: T-0109
title: Text is only borrowed: &str and const &str, no bare str
milestone: none
status: open
kind: design
area: language
---

**Next, by the user's choice (2026-10-30), after r88.** Written down, not started: nothing
here is decided beyond the direction, and nothing is in the spec.

**What it is.** Today `str` is a value type: a `{ ptr, len }` view that can be passed,
returned, stored in a field and kept in a container, as `List<str>` is. The user's
direction is that bare `str` should not exist, as in Rust: text is borrowed as `&str`, and
text in static storage is `const &str` (today's `const str`, r87).

```dz
// today
fn name(&self) -> str in self@text
let names = List<str>()             // a list of views, each tied to something else
fn label() -> const str

// proposed
fn name(&self) -> &str in self@text
// List<&str> cannot exist: a reference is never stored in a field (D-0003)
fn label() -> const &str
```

**Why it matters.** It removes a class of problems the ties work keeps running into. A `str`
is a reference that does not look like one, so every rule about references has had to
special-case it.

- **A type parameter could be a view.** `List<str>` makes `T` a view, so a generic body's
  `T` values and results such as `pop() -> T?` cannot be tied. r88 had to leave them
  unchecked (`untied_result`, `Signature::effects`); the census in `docs/log/r81-ties.md`
  counts about 80 such sites (a `T` bound, or built into an aggregate). With `&str` under
  D-0003, no `T` is ever a view, except a `const &str`, which needs no tie.
- **Structs holding text.** A struct with a `str` field is a struct holding a view, which r88
  ties as a whole. With `&str` it cannot exist; the field is a `String` or a `const &str`.
- **Which inputs count.** T-0107's elision question was whether a `str` parameter is a view
  input; a `&str` plainly is.
- **What reads clearly.** `-> str` returning a literal (DZ0571 under r88) reads like an
  owned value. `-> &str` says it borrows, which is what an LLM trained on Rust expects.

**Why it is this way.** `str` was designed as a value view early (D-0050, D-0092), before
ties existed, when there was no checker for what it points into.

**What it would take.** A language change: a §21 row, a decision superseding parts of D-0050
and D-0092, revising §5.3, §6.7 and §20, and every `str` in std, the examples and the corpora
(about 150 mentions in std, 260 in `tests/`, 17 containers of `str`). Questions for the user
first:

1. **Slices.** Do `[]T` and `[]mut T` become `&[]T` and `&mut []T` for the same reasons? If
   so, iterator structs that hold a slice (`SliceIter`, `Split`, `chars`) cannot hold one
   under D-0003: they need another shape, or an exception.
2. **`cstr`.** Does it follow, `&cstr`, or stay a C pointer type?
3. **`const &str` in fields and containers.** Stays allowed, since it views static storage
   (D-0092)? Then `List<const &str>` replaces `List<str>` where the text is literals.
4. **Spelling.** `const &str` or `&const str`; and whether `&str` in a parameter can be
   written by passing a `String` directly (today `ImplicitAs` and `as_str()`).
5. **C++ interop.** The emitted type can stay `::dz::str`; only the Dazzle spelling and
   rules change, but imported C++ functions taking `std::string_view` need a mapping.
