---
id: D-0044
title: Assigning to a `&mut T`
status: decided
---

**Writes the `T` it refers to**, as in C++. A reference cannot be pointed elsewhere, so there was no other meaning to give it, and `for &mut n in counts { n += 1 }` needs it. Through a `&T` it is an error (§5.2)
