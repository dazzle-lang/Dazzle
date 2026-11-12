---
id: T-0202
title: "Homes through traits and function types: a result `in self@home` read through either reaches freed memory"
milestone: none
status: open
kind: feature
area: ownership
---

Left from [T-0200](T-0200-homes-built-sends-clauses-home-tie.md) stages 1 and 2, where
it was noted in passing; [D-0097](../decisions/D-0097-homes-ownership-that-travels-with-its.md)'s
homes.

**Where the code is**

| Piece | Where |
|---|---|
| An implementation's `kills` and `in` against its trait's (DZ0573, `IMPL_CLAUSE_WIDER`) | `clauses_within_trait` in `dz_check/src/check.rs` |
| A function type's clauses; a function fitting a function type | `FnKills` (`within`, `kills_within`, `results_within`) and the `Ty::Fn` case of fitting in `dz_hir/src/ty.rs` |
| A function type's clauses parsed | `dz_parse/src/ty.rs` (`fn(world: &mut World) kills world`); the AST type in `dz_ast/src/lib.rs` |
| A function type's clauses resolved into a signature | `fn_type_sig` in `dz_check/src/ties.rs` |
| `FnSig::sends`, `FnSig::ret_home` | `fn_sig_inner` in `dz_check/src/lib.rs` |
| A body's sends and returns against its clauses, and the fix that writes one | `report_homes` in `dz_flow/src/ties/report.rs` (DZ0421, DZ0422) |
| A call read by its callee's clauses in placement | `lend_keeps`, `sends_to_self`, `region_clauses` in `dz_flow/src/placement.rs` |

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

**What it would take.**

- Turn the probes into a regression program, `tests/run/transfers` or one of its own:
  T7 (trait object), T9 (generic bound) and F3 (function value), each with a churn
  between building and reading, checked under the sanitizer job (T-0132) as well.
- For 1: compare `sends` and `ret_home` in `clauses_within_trait`, each implementation's
  within its trait's, by parameter position as `kills` is; a new code or DZ0573 with its
  own message. An `in self@home` on an implementation is wider than a trait method's
  untied owned result, so T7 is refused there and the trait must say `in self@home`.
- For 2: function-type syntax for `sends` and `in … @home` (a language change: a §21 row,
  spec §6.7 and §20), carried in `FnKills`, compared in `within`, and read by placement at
  a call through a function value. Until then a function with `sends` or `in … @home`
  could be refused where a function type is expected.
- For 3: `report_homes`' fix rewrites the existing `in` with the new path added.
