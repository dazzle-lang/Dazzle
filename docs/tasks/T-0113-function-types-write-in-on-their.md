---
id: T-0113
title: Function types write in on their result
milestone: none
status: open
kind: feature
area: ownership
---

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
| A function type's clause | `TypeKind::Fn` (`dz_ast`); `dz_parse/src/ty.rs`; `FnKills` in `dz_hir/src/ty.rs` |
| Read as a signature | `fn_type_sig`, `check_fn_types` in `dz_check/src/ties.rs` |
| Fitting a function to a type | `assignable` (`dz_hir/src/ty.rs`), `DZ0568` in `expect` (`dz_check/src/check.rs`) |
