---
id: T-0127
title: A view struct's `drop` is not counted as a use of its views
milestone: none
status: open
kind: bug
area: ownership
---

Listed in [D-0094](../decisions/D-0094-safe-code-has-no-undefined-behaviour.md) as needing
no decision. It enforces §6.7's first rule: a reference is never used after the memory it
points into has moved or been freed.

**Where the code is**

| Piece | Where |
|---|---|
| A scope's end: each local killed, its state dropped | `block_scope` in `dz_flow/src/ties/body.rs` (~1648) |
| Other drops: assignment, moves into a place, temporaries | `KillKind::Drop` sites in `dz_flow/src/ties/body.rs` (~450, ~1875, ~2331) |

**What it is.** Rust calls this "drop check". A view struct may have a `drop`, and the
`drop` may read its views. The implicit drop at the end of a scope is a use, but the ties
walk does not count it as one. This is accepted, and the drop reads freed text:

```dz
view struct Peek {
    m_text: &str

    Peek(text: &str) {
        m_text = text
    }
}

impl Drop for Peek {
    fn drop(&mut self) {
        println!("{}", m_text)      // reads the view
    }
}

let mut s = String::from("the original text")
let p = Peek(s.as_str())
s.push_str("-grow")                 // kills `s@text`: `p`'s view is dead
                                    // accepted: at the scope's end `p`'s drop prints the
                                    // original text, from freed memory
```

**Why it matters.** A guard that does something on drop (unlock, end a profiler zone,
write a summary) is a natural view struct, and its drop runs where no line of code shows a
use.

**Why it is this way.** View structs that hold references are recent (r96). Before them, a
type with a `drop` could hold no view.

**What it would take.** Wherever a value is dropped, a value whose type has a `drop` and
holds views uses those views first. A view that is already dead is then `DZ0413`, at the
kill, as for any other use. A type without a `drop` loses nothing, which covers most
iterators. If a type later needs a `drop` that reads none of its views, Rust's answer is an
`unsafe` marker on the drop (`#[may_dangle]`); it is not needed now. Compiler work only.
