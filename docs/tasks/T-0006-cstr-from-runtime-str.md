---
id: T-0006
title: A `cstr` from a runtime `str`
milestone: M12
status: done
done: r67, 2026-10-21
kind: feature
area: text
---

**The two string types, and why they do not convert.** A `str` is a borrowed slice — a
pointer and a length, with **no terminating zero**. A `cstr` is what C wants — a pointer to
bytes that **end in a zero**, with no length. So `str -> cstr` cannot be a cast: there is
nowhere to put the zero without copying the bytes. The other direction is free (`c as str`
measures the C string), and a *literal* is already a `cstr` wherever one is wanted, because
a literal lives in static storage and the emitted C literal is zero-terminated
(D-0039).

**What is missing** is the runtime case: you hold a `str` — read from a file, sliced out of
a line, handed to you by a caller — and a C function wants a `cstr`.

```dz
fn load_level(name: str) {
    let data = rl::LoadFileText(name)   // refused: `str` is not a `cstr`
}
```

D-0039 settled that this needs an **owned, zero-terminated copy, freed by its drop**, and
left the spelling for when `String` existed. `String` exists now, and already keeps a zero
after its bytes for exactly this purpose: `String::as_cstr` hands one out without a copy
(`std/text/string.dz`). So the pieces are there and what is missing is the spelling and the
lifetime.

**Why the obvious one-liner is not the answer.** `String::from(name).as_cstr()` builds the
copy and then borrows a `cstr` out of a temporary that dies at the end of the statement, so
the pointer C receives is dangling by the time it is read. Whatever this is spelled as has
to make the owner outlive the call, which means either a named binding:

```dz
let owned = String::from(name)          // or a dedicated `CString`
rl::LoadFileText(owned.as_cstr())
```

or a form the compiler knows keeps its temporary until the full statement ends — the same
problem [T-0056](T-0056-formatting-macro-arguments-go-through-format.md) describes for a
formatting macro's arguments, and worth deciding once for both.

**What it would take.**

1. **Decide the spelling** — the user's call. Candidates: a `str` method
   (`name.to_cstr(allocator)`) returning an owned type; `String::from(name)` plus
   `as_cstr()` documented as *the* way, with a diagnostic on the dangling one-liner; or a
   distinct `CString` type, which would say in its name that it owns the zero.
2. Whichever it is, the allocator is a by-name argument as everywhere else (§7.2), and
   `Allocators::Temp` is the right default for the common case — a name handed straight to
   a C call and never kept.
3. A `tests/run` case passing a runtime `str` to a C function that reads past the length,
   so a missing zero shows up as a failure rather than as luck about what follows in
   memory, and a `tests/errors` case for the dangling form once it is diagnosed.

**Why it matters.** Every C API that takes a file name, a window title, a shader uniform
name or a label wants a `cstr`. Literals cover the demos; the moment a name comes from a
level file, a directory listing or a config, there is no way to call that API at all. It is
the most likely thing to stop a real game using a C library.

**Done in r67** (D-0081), as the user chose: no new type. `let owned = String::from(name)` then `owned.as_cstr()` is the documented way (§5.3, §14.4), and `String::from(name).as_cstr()` passed straight to a call is fine, since the temporary lives to the end of the statement. Not diagnosed: a `cstr` bound by `let` from a temporary `String` dangles. That is one case of a general hole — any view taken from a temporary owner and bound by `let` — and is its own task, [T-0078](T-0078-view-temporary-owner-bound-by-let.md).
