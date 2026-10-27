---
id: D-0010
title: Allocator passing
status: decided
---

**Explicit parameter with a default argument.** It is the user's and the library's concern, not the language's (§14)

**Superseded by D-0091 (r81, 2026-10-27, by the user):** ordinary code no longer names an allocator; the compiler places memory (§14.6), and ties make every placement safe (§6.7). The explicit parameter stays only until that is implemented (T-0107), and the `Allocator` trait stays for `std`.
