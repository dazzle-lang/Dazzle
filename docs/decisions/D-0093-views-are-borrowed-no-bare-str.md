---
id: D-0093
title: Views are borrowed: no bare str, slices or cstr, and no view outside a reference's places
status: decided
---

**Decided: every point below, as proposed** (r89, 2026-10-30, by the user). Not yet
implemented: [T-0109](../tasks/T-0109-text-only-borrowed-str-const-str.md) lists the tasks,
in the order r90 set. Refines D-0091 and D-0003; supersedes the value-view parts of D-0039,
D-0050 and D-0092.

**Settled on the way, 2026-10-30 (r90, by the user):** three places where the points below
did not fit the rest of the language.

- **A type parameter or an associated type may be marked `view`** (point 8 refined). Point 8
  alone refused `T = SliceIter<i32>`, which §8.5's projections rely on: `first(&list.iter())`,
  `count(xs.iter())`, an adapter `Numbered<SliceIter<Monster>>`. A parameter written `view T`,
  or an associated type a trait declares `view type Item`, may be a view, and the generic
  body checks a value of it as one: it lives only where a reference does, carries a tie,
  and passes only to another `view` parameter. A struct with a `view` parameter is a `view
  struct`. A plain `T` is still never a view, so generic bodies stay fully checked.
  `Iterate::Item`, `Iterable::Iter` and `IterableMut::IterMut` are `view type`s.
- **An optional view is a pointer** (point 3's spelling corrected). `&T?` is a reference to
  an optional `T` (§5.6), so a method handing back an element or nothing is `-> ^T in
  self@items`, and text or a slice that may be absent is `^str` or `^[]T` where it was
  `str?`: tied when a safe call returns it with `in`, so `or` turns it into a reference
  safely (point 11). `^str` is then the text pointer itself, a `{ ptr, len }` that may be
  null, no longer a pointer to a `str` value. `(&T)?` written out is an error that offers
  `^T`, and a `T::Item?` whose instance is a reference is that pointer, which settles
  [T-0085](../tasks/T-0085-optional-reference-passes-checker.md).
- **A view struct holds references** (D-0003 refined). D-0091 kept D-0003; point 3 relaxes it
  for view structs only, which are placed as a reference is, so a reference still lives only
  where references live.

Also settled, each in its task: `view` is a keyword only before `struct`, `type` and a type
parameter, so `let view = …` stays valid ([T-0111](../tasks/T-0111-view-structs-struct-that-holds-view.md));
a pointer parameter is untied, so `p or return` on one needs `unsafe`, checked in `dz_flow`
([T-0114](../tasks/T-0114-untied-pointer-becomes-reference-only-in.md)); `process_env`
returns `^str` ([T-0112](../tasks/T-0112-cstr-behind-pointer-text-at-c.md)). The order is
now T-0113, T-0111, T-0109, T-0110, T-0112, T-0114: view structs come first, since `Chars`,
`Split`, `Lines` and `SplitWhitespace` hold a `str` field and `SliceIter` a slice, which
only a view struct may once views are borrowed.

**The question.** `str`, `[]T` and `cstr` are views that look like values: they can be
stored in fields and containers (`List<str>`), so a type parameter may be a view and a struct
may hold one. r88 had to leave each such case untied. How do the ties become complete, with
no hole other than what is marked `unsafe`?

**The rule.** A *view* is anything that points into memory something else owns. Every view
has a tie, known where it is made. Views live only where references live — locals,
parameters, results, temporaries — never in a field, a container or a type argument. The one
exception is `const &`, which points at static memory, needs no tie and may go anywhere.
Anything untied is a raw `^` pointer, and turning a raw pointer into a tied reference is
`unsafe`.

1. **Text is `&str`**, and static text `const &str` (r87's `const str`). Bare `str` is no
   longer a type a value can have.
2. **Slices are `&[]T` and `&mut []T`**, and static tables `const &[]T` (the open `const []T`
   question). Fixed arrays `[N]T` stay values.
3. **View structs.** `view struct SliceIter<T> { m_slice: &[]T }` may hold references and is
   itself a view: placed as a reference is, with one tie for everything its fields view, and
   elision and `in` as for a reference. A method returns through a tie on the field,
   `tie @items = m_slice` and `-> ^T in self@items` (r90; first written `-> &T?`, which
   §5.6 makes a reference to an optional), so what it returns outlives the iterator. The marker is explicit, so a struct never becomes unstorable because a field was
   added. An optional, tuple or array of views is a view by the same rule, with no marker.
4. **`cstr` too.** `&cstr` is a tied view (`String::as_cstr() -> &cstr in self@text`),
   `const &cstr` a literal, and `^cstr` C's pointer, untied. A C `const char*` parameter takes
   any of the three; a C result is `^cstr`. A C++ `std::string_view` parameter is `&str`, a
   result `^str`.
5. **`const &` may be stored**: `List<const &str>` and a `const &str` field are fine. It
   applies to `str`, `cstr` and `[]T`.
6. **Spelt `const &str`**: the constness is the reference's (what it points at never dies),
   in r87's word order. `&const str` would say the text cannot change, which every `&str`
   already says.
7. **A `String` passes as `&str`** through `ImplicitAs<&str>` and `ImplicitAs<&cstr>`: at
   arguments only, borrowed, tied to the `String`. No general deref coercion.
8. **No view is a type argument** (DZ0517 extended): `List<&str>` and `T = SliceIter<i32>`
   are errors, `List<const &str>` is not. A `T` is then never a view, so generic bodies are
   fully checked. (r90: except a parameter or associated type marked `view`, above.)
9. **Function types write `in`**: `fn(a: &T, b: &T) -> &T in (a, b)`. (Built in r91,
   T-0113.)
10. **A tagged union's payload needs no tie syntax**: a write that may change the variant
    kills every payload (r82), so `in u` and `kills u` are already exact. Closes T-0107's
    question.
11. **`p or return` on an untied `^T` is `unsafe`.** A pointer with a known tie, one a safe
    call returned with `in`, still converts to a tied `&T` safely; one from a field, a cast
    or C converts only in `unsafe`. A context struct's `^mut World` (§5.2) therefore needs
    `unsafe` to use, which is honest: that aliasing is exactly what §6.7 cannot check.
    Rejected: keeping it safe as the one documented hole.

**What stays unchecked**, by design and marked: raw pointers and `unsafe` blocks; `std`'s
trusted bodies, which [T-0116](../tasks/T-0116-debug-build-check-std-trusted-kills.md)
tests in debug builds; and threads (D-0011, deferred). Every exemption r88 left
goes: a result that is a view only through `T` (point 8), `cstr` from nothing and `extern`
results (point 4), by-value view parameters (point 3), and function types (point 9).

**Spec:** §5.3, §6.7, §15, §20 are revised as each task lands; r89 records the decision.
