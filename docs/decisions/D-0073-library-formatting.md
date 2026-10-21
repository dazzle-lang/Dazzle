---
id: D-0073
title: Library formatting
status: decided
---

**No global `format`: `String::format(fmt, args: ..&Format, allocator = …)` builds a `String`**, chosen by the user over a global function and over a macro, which waits for M10. **`Format` is a real trait**: `std` implements it for the numbers, `bool`, `char`, `str`, `cstr` and `String`, and a type of one's own implements it to be formatted; a `Formatter` appends to a `String` and reports the placeholder's spec, which a type's own implementation interprets. **No attribute marks a format string**: an `#[format]` was built and removed by the user, who wants formatting functions of one's own to be **macros, as in Rust** — `String::format` is the one function whose format string the compiler checks, until M10. `println` and friends stay builtins, taking only the built-in kinds, until macros (M10); `error(…)` keeps its static storage (§9.2). *Revised by 18.80:* M10 has come, and `String::format` and the builtins are the macros `#format` and `#println`
