---
id: T-0258
title: & is deep: nothing reachable through a & is written, and Iterate::get takes &mut self
milestone: M19
status: done
kind: feature
area: checker
where: `crates/dz_check/src/check.rs` (`read_only_view`, `reached_through_shared`, `read_only_view_help`; the field binding in `function` and the `ExprKind::Field` arm of `expr`); `crates/dz_project/src/std/core/iterate.dz` and every `impl … Iterate` in `std`; `tests/workspace/typed` and `typeerrors` (`ViewTally`)
---

Decided by the user on 2026-11-18 (D-0096, "Agreed, 2026-11-18"), from hole 1 of T-0245.

**What is wrong.** `&` is shallow today, as C++'s `const` is: through `&self`, a view
struct's `&mut` field can be written. Checked with the compiler:

```dz
view struct Holder {
    pub(module) c: &mut Counter
    tie @c = c
    pub fn bump(&self) { c.n += 1 }     // accepted: writes through `&self`
}
fn reader(h: &Holder) { h.bump() }      // a `&` parameter that writes
```

So a signature's `&` does not say the call writes nothing reachable from it, which a reader
(or an LLM) assumes, and which D-0096's parallel calls need: two calls sharing a `&` must
not be able to write.

**The rule.** Nothing reachable through a `&` is written: a `&mut` field reached through
`&` reads as `&`, and a `^mut` field likewise, outside `unsafe`. `Iterate::get` takes `&mut
self`, as Rust's `next` does, so a mutable iterator still hands out `&mut` items; a `for`
loop already calls `get` on its own local iterator. Game code is unchanged; what changes is
`std`'s 14 `Iterate` implementations (one word each), `ecs`'s two queries, and the spec's
`fn first<view T: Iterate>(it: &T)` examples, which take `&mut T`.

**With it:** a `tests/errors` case for `bump` above (a new code), the matching
`tests/workspace/typed` case for the same through `&mut self`, and a §21 row, since it
changes what a valid program is.

**Done 2026-11-18** (r144): see `docs/log/m19-threads.md`. Built as a type rule: a view's `&mut` or `[]mut` field reached through a `&` is typed `&` or `[]`, so assignment, receivers and `&mut` follow; the error is `DZ0507` with a help naming the rule, not a new code. Left for the user: an iterator taken by value as a parameter must move into a `let mut` before it can step, since Dazzle has no `mut` parameters.
