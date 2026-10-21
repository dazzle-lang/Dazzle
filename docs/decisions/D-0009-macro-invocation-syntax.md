---
id: D-0009
title: Macro invocation syntax
status: decided
---

**The macro's kind decides its spelling** (r65, by the user; r63 had made it `#name` everywhere, and before that it was `#name …` for declarations and `name!(…)` for expressions). An `expr` or `stmt` macro reads like the call it stands for: `max!(a, b)`, `log!("warn", "…", hp)`, one bracketed group — an `expr` macro wherever an expression may be, a `stmt` macro as a whole statement. A `decl` macro reads like the declaration it writes: `#machine Foo(…) { … }`, running to the end of the line. A dependency's is `dep::max!(…)` or `#dep::machine …`. The formatting macros are `println!(…)`; the other intrinsics, which stand for a value, stay `#file`, `#location`, `#expr_text(e)`. Each macro has one spelling, and the other is an error saying which to write (DZ0610, and DZ0217 for an intrinsic). The user's reason: `#println()` and `machine!`/`location!()` both read wrong — `!` is for what looks like a function, and a `log` looks like one. `!` after a name cannot be read as "not" or as `!->` (§12.5, §12.7, §12.8)
