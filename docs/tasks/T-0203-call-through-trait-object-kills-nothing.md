---
id: T-0203
title: A call through a trait object kills nothing, so a view survives its container's growth
milestone: none
status: done
kind: feature
area: ownership
---

Found 2026-11-12 while probing [T-0202](T-0202-homes-through-traits-function-types-sends.md).
A safety hole in `kills` (spec §6.7), older than homes.

**Where the code is**

| Piece | Where |
|---|---|
| The ties a killed place names; a trait object named whole (the fix) | `collect_ties` in `dz_flow/src/ties.rs` (the `ItemSig::Trait` arm) |
| A call's signature, a trait method's resolved to its implementation when the receiver's type is known | `call_signature` in `dz_flow/src/ties/body.rs` (the `Callee::Method` arm: `resolve_impl`, then the trait's own `methods_sig`) |
| A body's kills checked against its clause | `dz_flow/src/ties/report.rs` (DZ0415, DZ0416) |
| Trait objects | `^Trait`, `&Trait`, `&mut Trait`: spec §7.6; `tests/run/dynamic` |
| The tests | `tests/flow/trait_object_kills.dz`; the passing pair in `tests/flow/clean.dz` |

**Done 2026-11-12.** The kill was not lost at the call: `call_signature` read the trait's
`kills self` and bound it to the receiver. But a trait object's type is `Ty::Named` with the
trait's symbol, and `collect_ties` named no ties for a trait, so the kill named nothing.
A trait object may be anything, so it is now named whole, as a type parameter is. Every
example, `tests/run` program and bench program still checks.

**What it is.** A method called through `&mut Trait` does not kill what its trait
declaration's `kills self` names. The body that makes the call is then said to kill nothing,
so its own signature may leave `kills` out, and a caller's view into the object survives the
call:

```
trait Sink {
    fn put(&mut self, s: String) kills self
}

impl Sink for List<String> {
    fn put(&mut self, s: String) kills self@items sends s to self {
        self.push(move(s))
    }
}

fn put_many(sink: &mut Sink) {   // accepted with no `kills`
    for _ in 0..100 {
        sink.put(String::from("pushed through a trait, long enough to need memory"))
    }
}

static XS: List<String>? = none

fn main() {
    XS = List<String>()
    if let &mut xs = XS {
        xs.push(String::from("first, long enough to need memory"))
        let r = &xs[0]
        put_many(&mut xs)   // grows `xs` and frees its old buffer
        println!("{}", r)   // prints an empty line: `r` is in the freed buffer
    }
}
```

`put_many(sink: &mut Sink) kills sink` is refused instead, with DZ0416 "never kills
`sink`". The same body with `sink: &mut List<String>` is refused with DZ0415 (needs `kills
sink@items`), and with a generic bound `<S: Sink>(sink: &mut S)` with DZ0415 (needs
`kills sink`): both correct. Only the trait object loses the kill.

In a region the hole is hidden: the old buffer stays until the region ends, so the same
program with `xs` a local prints the right text. The static puts the list on the heap.

**What it would take.** A call through a trait object kills, at its receiver, what the trait
method's clause says (`kills self` kills everything reachable through the object). The
program above, cut down, is in `tests/flow/` (DZ0415 at `put_many`, DZ0413 at the use of
`r`), with a passing case in `tests/flow/clean.dz` that writes the clause.
