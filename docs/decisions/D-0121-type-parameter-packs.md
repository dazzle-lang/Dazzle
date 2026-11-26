---
id: D-0121
title: "Type parameter packs: <..T>, as C++ has them, and no spawn! macro"
status: decided
---

**Decided, 2026-11-26, r155 and r156: spec §8.7**, with §5.4, §7.2, §22.3 and §22.11. The user's
choice: asked for the next task, which was the macro `spawn!`, the user asked why a macro was
needed at all, and answered that what is needed is "`..T` the same way C++ … already
support[s]".

**The question.** D-0117 built `spawn(entry, value)`, one moved value, because `spawn!(f(a,
b))` needed a function of `a` and `b` and somewhere to keep them, which a macro in expression
position cannot declare, "nor can a generic count its arguments (there are no variadic type
parameters)". D-0096 met the same wall from the other side: `Query<A>` to `Query<A, B, C, D,
E>` written five times. D-0056 had turned compile-time packs down in favour of `..T` slices
because a pack "cannot be walked without macros (M10)"; M10 has since been built.

**What it is.** C++'s parameter packs, spelled with the `..` Dazzle already uses for "a run
of" (D-0056), and checked at the definition as every generic is (§8.1):

| C++ | Dazzle |
|---|---|
| `template <class... Ts>` | `<..T>`, `<..T: Send>`: a **pack**, standing for any number of types, each of which meets the bounds |
| `Ts... args` | `args: ..T`: a **pack parameter**, given the rest of the positional arguments, each of its own type |
| `std::tuple<Ts...>` | `(..T)`, and `..T` in any list of types: `(i32, ..T)`, `fn(..T) -> R`, `Foo<..T>` |
| `f(args...)`, `std::apply(f, t)` | `f(..args)`: a tuple **spread** into the arguments it holds |

- **A pack is declared last** in its list, and there is at most one, as C++ requires of a
  class template's. Written type arguments past the fixed ones are the pack's,
  `spawn<i32, f32>(…)`, and none at all is an empty pack.
- **`T` alone is not a type** when `T` is a pack: it is written `..T`, inside a list.
- **In the body a pack parameter is a tuple**, `args: ..T` is a `(..T)`, as a variadic slice
  parameter is a `[]T` (§7.2). So it is moved, stored in a field and passed on as any value is,
  and `..args` spreads it into a call. A pack parameter is the function's variadic parameter:
  there is at most one of either kind, and the parameters after it are passed by name. Whether
  `xs: ..T` is a pack or a slice is read off `T`: a pack's name makes it a pack, any other type
  a slice.
- **A spread is of a tuple**, any tuple, not only a pack's: `f(..t)` passes `t.0`, `t.1`, …
  as that many arguments, moving the elements that are not plain data. It may stand anywhere
  among a call's positional arguments and in a tuple literal, `(..t, 3)`, more than once, and
  its operand is a place, a local, parameter or field, since each element is read from it in
  turn. Spreading a slice into a variadic slice parameter is unchanged (§7.2).
- **Inference** reads a pack off the arguments a pack parameter is given, or off a function
  type: `fn(..A)` against `fn(i32, f32)` makes `A` the pack `i32, f32`.
- **Bounds** hold of every element: `<..A: Send>` given `i32, String` checks `i32: Send` and
  `String: Send`, and an error names the element that fails.
- **Each pack is its own instance**, as each type argument is (§8.4): `spawn` given two
  arguments and given three are two functions.

**Why C++'s shape.** The user asked for it, and it is the one model that exists: C# has no
variadic generics (`Func<…>`, `Action<…>` and `ValueTuple<…>` are each written out sixteen
times), and Rust has none either. Dazzle's difference from C++ is not chosen here but follows
from §8.1: C++ checks a pack's body once per instantiation, so its errors are reported inside
the template, while Dazzle checks the body once, against the bounds, so a body does with a
pack's elements only what every element's bounds allow, and errors stay at the user's line.

**Why the body sees a tuple.** It is the least new language: a value of a pack is then an
ordinary value, and the only new expression is the spread. It is also what C++ ends up writing
whenever a pack has to be kept (`std::tuple<Ts...>` and `std::apply`), which `spawn` must,
since its arguments cross to another thread.

**What it replaces.** `spawn!` is not built: `spawn(entry, ..args)` is a plain generic
function, `spawn(physics_thread, inbox, outbox)`, which is what §22.3's `spawn!(f(a, b))`
was for. D-0117's question of the macro's spelling is closed by this.

**Completed, 2026-11-26, r156: the rest, the user's choices again** (T-0269).

- **An expression repeated over each element is `..pattern`**, C++'s `f(g(args)...)`. Asked
  for a spelling, the user preferred C++'s compile-time placing to a loop, which is run-time
  work, and asked "why not simply `..params`?". So `..` is the one marker: `..e` of a place
  is a spread, as before, and `..e` of anything else that names a pack's tuple is `e` once per
  element, the tuple's name standing for the element. The two cannot be confused, since a
  spread's operand is a place, and a spread is the pattern of the element alone. The same
  reading holds for types: `..Option<A>` is `Option` of each of `A`'s types, C++'s
  `Option<Ts>...`. A fold needs no syntax of its own: a pattern into a variadic slice
  parameter, `total(..args.size())`, is one. A pattern is checked once, against the bounds,
  with the element as a type of its own that has the pack's bounds; monomorphization copies
  it once per element, so it holds no block, branch, `or` or formatting macro.
- **A pack's length is `countof(A)`**, the user's choice beside `sizeof` and `alignof`, as C++
  writes `sizeof...`. It is not `args.len`, nor a trait with a `len` method on the tuple: the
  user pointed out that inside a pattern `args` is each element, which may well have a
  `len()` of its own, a `List`'s.
- **A pack on a struct, a union, a trait and an implementation** works as on a function: a
  type's arguments are written flat, `Query<Position, Velocity>`, and its pack is the tuple
  of them.
- **A function with a pack parameter is a value**: C++ takes one parameter per element, so
  `call<i32, f64>` is a `fn(fn(i32, f64), i32, f64)`.
- **Any tuple may be spread**, a pack with elements beside it included, `(i32, ..A, bool)`,
  and **a tuple has any number of elements**: the sixteen-element limit (DZ0904) was retired
  before any release.
