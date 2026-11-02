---
id: T-0149
title: Stored values left on the heap: a map filled in a loop, a container lent as &mut, format! text in a loop
milestone: none
status: open
kind: feature
area: ownership
---

Left by [T-0145](T-0145-value-stored-into-container-built-where.md), which builds a value
stored into a container where the container lives, but only where it can follow the value
there and back out.

**What it is.** Four cases where a value stored into a container in a region still keeps the
heap:

| The case | Why it keeps the heap | What would place it |
|---|---|---|
| a map filled in a loop, `names.insert(String(name), v)` (`expr`, and `words` once its keys are not copies) | `insert` may drop the value it replaces, so on every pass the region could grow past what the map holds (the loop rule) | follow the guard most such code has, an `insert` in the `else` of a failed lookup, which never replaces; or a `std` method that only adds, whose summary keeps |
| a container lent to a function, `fn tag(names: &mut List<String>)` | the callee cannot see where its caller's container is placed | build the value with the container's own allocator, which it stores as `#placement` (the prototype's `contents_allocator`, T-0105) |
| `format!`'s text pushed in a loop, `names.push(format!(…))` | the text is built inside the macro's lambda, so it is not stored "directly" | treat the macro's value as the site its text is, as `formatted_text` does for a `let` |
| a program's own container whose `kills` clause names a tie other than `@items` or `@text` | `keeps` cannot tell such a tie from one over the elements | read the tie's declaration: one that does not reach `[*]` keeps |

**Why it matters.** The first is how most maps are filled, and where `words` and `expr` spend
their allocations: one key per distinct name.

**Where the code is.**

| Piece | Where |
|---|---|
| Which methods keep a parameter, and which containers keep everything | `storing_params`, `keeps`, `keeps_through` in `dz_flow/src/storing.rs` |
| The proposer's target and its loop rule | `stored_target`, `lets_go`, `stored_by` in `dz_flow/src/placement.rs` |
| The check's loop rule | `kept_each_pass` in `dz_flow/src/ties/body.rs` |

**What it would take.** Compiler work for each row but the first's second answer, which
adds to `std` and is the user's call.
