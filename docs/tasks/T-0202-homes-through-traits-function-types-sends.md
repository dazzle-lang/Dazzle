---
id: T-0202
title: "Homes through traits and function types: a result `in self@home` read through either reaches freed memory"
milestone: none
status: done
kind: feature
area: ownership
---

Left from [T-0200](T-0200-homes-built-sends-clauses-home-tie.md) stages 1 and 2, where
it was noted in passing; [D-0097](../decisions/D-0097-homes-ownership-that-travels-with-its.md)'s
homes.

**Where the code is**

| Piece | Where |
|---|---|
| An implementation's clauses against its trait's, `sends` and `in … @home` included (DZ0573) | `clauses_within_trait` in `dz_check/src/check.rs` |
| A function type's clauses, `sends` and `homes` included; a function fitting a type | `FnKills` (`within`, `kills_within`, `results_within`, `sends_within`, `homes_within`, `without_home`) in `dz_hir/src/ty.rs` |
| The report when a function does not fit a type (DZ0568, DZ0572, DZ0584) | the `Ty::Fn` case of a type mismatch in `dz_check/src/check.rs` |
| A function item's value carries its own clauses | the `SymbolKind::Fn` value in `dz_check/src/check.rs` |
| A function type parsed, `sends` with it; `sends` parenthesised in a type | `dz_parse/src/ty.rs`; `parse_sends(in_type)` in `dz_parse/src/item.rs`; `TypeKind::Fn::sends` in `dz_ast/src/lib.rs`; `dump::sends_in` |
| A function type lowered, its `in` split into views and homes | `TypeKind::Fn` in `lower_type_inner`, `dz_check/src/lib.rs` |
| A function type's clauses resolved and checked | `fn_type_sig`, `fn_types` and `homes_and_sends` in `dz_check/src/ties.rs` |
| `FnSig::sends`, `FnSig::ret_home` | `fn_sig_inner` in `dz_check/src/lib.rs` |
| The region check reading a call through a function value | `signature_at` in `dz_flow/src/ties/body.rs` |
| The region check following a local reference to its container | `containers_of`, `regions_held`, `region_clauses` in `dz_flow/src/ties/body.rs` |
| The proposer doing the same | `reference_to` and the function-value case of `built_and_left` in `dz_flow/src/placement.rs` |
| The tests | `tests/run/homes_through_calls`; `tests/errors/impl_*_trait.dz`, `fn_*_its_type.dz`; the `Tray` section of `tests/workspace/typed`; `tests/parse/ties.dz`, `tests/fmt/ties.dz` |

**What it is.** Three gaps in where `sends` and `in x@home` are read:

1. **A trait method's clauses against its implementations'.** `clauses_within_trait`
   compares `kills` and a view result's `in`, but not `sends` or an owned result's
   `in … @home`. An implementation can send what its trait method says it keeps, and a
   call through the trait reads only the trait's clause.
2. **A function type's `sends` and `in … @home`.** `FnKills` carries `kills` and a view
   result's `in` only, so a function type cannot say where it sends what it is given, and a
   function with `sends` fits a function type that says nothing.
3. **`dz fix` extending an `in` already written.** DZ0421's fix writes a missing clause,
   but where the result already has `in a@home` and the body also returns `b`'s, it does
   not add `b@home` to it.

**Why it matters: 1 and 2 are a safety hole** (found 2026-11-12). Safe code with no
diagnostic reads freed memory. The result direction is the hole: an implementation whose
signature says its result is `in self@home`, reached through a trait method or function type
that says nothing, hands back a value built in its receiver's home, and the caller, reading
the trait's or the type's clause, takes it as built where it asked and keeps it longer than
that home lives:

```
trait Source {
    fn make(&mut self) -> String? kills self
}

impl Source for Bag {   // Bag holds `m_items: List<String>`
    fn make(&mut self) -> String? in self@home kills self@items, self@items@items {
        m_items.push(String::from("built in the receiver's home"))   // built where `self` lives
        return m_items.pop()
    }
}

fn make_through(store: &mut List<String>) -> i32 kills store@items {
    let mut bag = Bag()            // in `make_through`'s region
    let s: &mut Source = &mut bag
    let v = s.make() or return 0   // placed "home: where `store` lives"
    store.push(move(v))            // `store` outlives the region `v` is in
    return churn()
}
```

`main` then prints `################################ ng enou` for that string: a later
call's region reused its memory. The same happens through a generic bound (`fn
make_one<S: Source>(src: &mut S, …)`, which reads the trait's clause), and through a
function value (`f: fn(xs: &mut List<String>) -> String kills xs`, given a `made` that pushes
then pops, `-> String in xs@home`). The same call made directly is sound: the caller reads
the implementation's `in self@home` and puts `bag` (or the list) on the heap.

The other direction is not a hole today. An argument handed through a trait method or a
function value goes on the heap ("passed to `sink.put`", "passed to `a function value`"),
and a container lent to either keeps the heap for what it holds ("lent as `&mut` with
`names`"), so an implementation that sends its argument into `self` stores a heap value.
That is conservative only because placement does not yet read a trait's (empty) `sends`
as "keeps nothing", so it must hold when the trait's clauses are trusted.

The probes (nine cases, with the direct-call controls) were run from the scratchpad, not
committed. The third gap, `dz fix`, is a convenience.

A function type's list of `kills` paths, as a parameter's type, is parenthesised:
`kills (xs@items, xs[*])`, since a comma after a bare path begins the next parameter (settled
in r81's syntax; the probe used `kills xs`, which covers both).

A second hole was found on the way, in `kills` rather than homes, and is closed:
[T-0203](T-0203-call-through-trait-object-kills-nothing.md).

**Done 2026-11-12 (r120)**, for 1 and 2; 3 is
[T-0204](T-0204-dz-fix-adds-home-owned-result.md). The user chose to write the clauses on
function types over refusing to store such functions.

- **Traits.** `clauses_within_trait` holds an implementation's `sends` and `in … @home`
  within its trait's (DZ0573), so a trait whose implementations keep or hand back memory
  says so, and a call through the trait or a bound places by it.
- **Function types** take `sends` and `in x@home` as a declaration does, carried in
  `FnKills` and compared in `within`: a function fits only a type that covers its clauses
  (DZ0584 for `sends`, new; DZ0572 for `in`). A call through a function value now reads the
  type's signature in the region check (`signature_at`), where `call_signature` had nothing
  for a value that is not a function item.
- **A local reference.** Fixing the trait object showed a hole with no trait in it at all:
  `let s = &mut bag; s.make()` left `bag` in a region while its result went to a longer-lived
  list. The region check stopped at a reference local (`local_root`); it now follows it to
  what it views (`containers_of`), and the proposer follows a `let s = &mut bag` to `bag`
  (`reference_to`), so they agree.

The probe's nine cases all print what they built; `tests/run/homes_through_calls` keeps
seven of them, each placing its container on the heap where it was a region before.
