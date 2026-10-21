---
id: D-0081
title: The collections pass
status: decided
---

**`List`, `HashMap` and `String` rounded out together, a `HashSet` added, and `memmove` beside `memcpy`; chosen by the user on 2026-10-21.**

- **A method that needs more of the element than the container does is a trait's**, as `Sort` already was (18.70), since a method cannot bound its type's parameter: `Search<T>` (`contains`, `index_of`) for a `List` of `Eq` elements, `Extend<T>` (`extend`, `insert_slice`, `fill`) for `Copy` ones, and `Eq` and `Hash` on the `List` itself. A missing bound says which trait and which bound.
- **Absence is a pointer, as `HashMap::get`'s already was** (§9.1): `first()`/`last()` return `^T`, `null` on an empty list, so `xs.last() or return` reads as a lookup does.
- **Looking up a `String` key by `str` is a trait implemented for `HashMap<String, V>` alone** — `StrKeys<V>`: `get_str`, `get_mut_str`, `contains_str`, `remove_str` — and `StrSet` for `HashSet<String>`. Chosen over a `Borrow` trait, which would be a promise nothing checks that two types hash alike, and over overloading, which 18.5 rules out (T-0043).
- **`HashSet<T: Hash + Eq>`** is a `HashMap<T, bool>` underneath: the same table, one byte a slot for values that say nothing.
- **A runtime `str` reaches C through a `String`**: `let owned = String::from(name)` then `owned.as_cstr()`. No `CString` type; the `String` already keeps a zero after its bytes. Passing `String::from(name).as_cstr()` straight to a call is fine, since the `String` lives to the end of the statement; binding that `cstr` with `let` is not (T-0006).
- **`memmove(dst, src, bytes)`** joins the placement primitives (§14.3): an overlapping copy, `unsafe` as `memcpy` is. `List` shifts elements with it when `T::is_trivially_copyable` and one at a time otherwise (T-0042).
- **`hash_combine` relies on plain `+` wrapping**, which 18.63 promises; it is not rewritten with `wrapping_add`, which would imply plain `+` might not wrap (T-0044).

**Two follow-ups, the same day (r68)**, from what writing this exposed, fixed at the user's word ("fix the issues you found"):

- **An operator reads through a reference** to a number, `bool`, `char`, `str` or enum (§5.2), as an argument already did: `x > 2` on a `&i32`. A `retain` predicate takes a `&T` and compares it on nearly every line, so without this the new API was awkward to use (T-0079).
- **Keeping a view of a temporary is an error, `DZ0407`** (§6.5): `let c = String::from(name).as_cstr()` compiled and read freed memory. The view is followed back through receivers, fields, indexes and slices only, never into a call's arguments, and a value that needs a drop is no view, so `Path("a") / "b"` is not one (T-0078).

Spec: §5.2, §5.3, §6.5, §14.2, §14.3, §14.4.
