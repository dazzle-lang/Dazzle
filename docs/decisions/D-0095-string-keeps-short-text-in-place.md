---
id: D-0095
title: A String keeps short text in place, and a view inside an element dies with the array
status: decided
---

**Decided** by the user on 2026-11-04: "We also can have short string inside the String",
answering the question [T-0156](../tasks/T-0156-string-always-allocates-short-text-kept.md)
put. Built in r114.

**The question.** A `String` was a `List<u8>` and its closing zero, so every non-empty string
allocated. `std::string` keeps up to 15 bytes inside the object. In
`tests/bench_programs/words`, nearly every word is that short, so the Dazzle half made
millions of allocations its C++ half does not. Keeping short text in place changes what a
`String`'s tie means. A view into a short string points into the `String` itself, so
moving the `String` ends it, and so does moving the list it is an element of: growing,
sorting or swapping it. Until then §6.7 said a view into one element's text survived
pushing another. The alternatives were a separate `SmallString`, or leaving `String` as it
was and relying on regions.

**The resolution.**

- **`String` keeps text of up to 15 bytes in place** (§14.2), in two machine words that
  hold either the text and its zero or where longer text is and how much room it has. It
  stays 32 bytes: those words, the length, and the allocator.
- **A view inside an element dies with the array** (§6.7). What a path reaches from an
  element through no pointer's storage is in the element's own bytes, which move when the
  array does: `list[i].as_str()` for a short string, and a method's reference into an
  element, `list[i].left_of()`. A reference read through the element, `&list[i].name`,
  already died this way; a view through a method did not, which was a hole for any type,
  not only `String`. What an element owns behind a pointer stays where it is.
- **A tie naming plain data names those bytes** (§6.7). `tie @text = m_store`, an array of
  numbers, names the bytes inside the value, so a clause can kill them though they own
  nothing. Before, a tie on plain data named nothing a clause could compare.

**What it changes for a program.** A view into a list element's text, kept across a push,
`sort`, `sort_by`, `swap` or `reverse` of that list, is now `DZ0413`. Every program in the
repository already got its views again after such a change.
