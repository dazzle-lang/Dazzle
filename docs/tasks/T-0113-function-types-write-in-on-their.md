---
id: T-0113
title: Function types write in on their result
milestone: none
status: done
done: r91, 2026-10-30
kind: feature
area: ownership
---

**Done, r91 (2026-10-30).** A function type writes `in`, is held to `DZ0570` and `DZ0571`
as a declaration is, and a stored function must fit its `in` (`DZ0572`). Built as below;
`docs/log/r81-ties.md` has the detail. Next is
[T-0111](T-0111-view-structs-struct-that-holds-view.md).

Step 1 of [D-0093](../decisions/D-0093-views-are-borrowed-no-bare-str.md), by r90's order;
the steps are listed in [T-0109](T-0109-text-only-borrowed-str-const-str.md). Small, and
independent of the others: it could land at any point, so it goes first.

**What it is.** A function type names its parameters when its clause needs them (r81), but
cannot say where its result points. With two view inputs its result is untied:

```dz
pub on_pick: fn(a: &Enemy, b: &Enemy) -> &Enemy               // today: untied
pub on_pick: fn(a: &Enemy, b: &Enemy) -> &Enemy in (a, b)     // after
```

**Why it matters.** It is the last case r88's `DZ0570` cannot reach: a function value
returning a view with several inputs leaves its callers unchecked.

**Why it is this way.** r88 settled `in` for declarations; function types were left out.

**What it would take.** A language change (§6.7, §20, a §21 row). The parser for a function
type (`dz_parse/src/ty.rs`), `TypeKind::Fn`, `FnKills` in `Ty::Fn` (as written, not part of
the type's identity, like the clause), `fn_type_sig` filling `FnSig::ret_ties`, `DZ0570` for
a function type, and `assignable` holding a stored function's `in` to the type's (a function
fits a type whose `in` covers its own). `dz_flow` already reads `fn_type_sig` at calls
through a function value.

| Piece | Where |
|---|---|
| A function type's `in` | `TypeKind::Fn::ret_ties` (`dz_ast`), printed by `type_str` and the formatter's `ty`; parsed in `dz_parse/src/ty.rs` with `parse_in_list`; `FnKills::Known::results` in `dz_hir/src/ty.rs` |
| Read as a signature | `fn_type_sig` (fills `ret_ties`), `check_fn_types` (its paths, and `DZ0570`/`DZ0571` through `untied_fn_type`, at the span of the type holding it) in `dz_check/src/ties.rs` |
| Fitting a function to a type | `FnKills::kills_within` and `results_within` (elision by `result_places`), both in `within`, used by `assignable` (`dz_hir/src/ty.rs`); `DZ0568` and `DZ0572` in `expect` (`dz_check/src/check.rs`) |
| Tests | `tests/parse/ties.dz`, `tests/fmt/ties.dz`, `a_function_types_result_takes_in` (`dz_parse/tests/parse.rs`), `typed/` (`pick_with`), `typeerrors/` (`pick_either`, `named_by`, `picks_wide`), `a_call_through_a_function_value_ties_its_result_as_its_type_says` (`dz_flow/tests/ties.rs`), `dz explain DZ0572` |
