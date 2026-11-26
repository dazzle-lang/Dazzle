---
id: T-0269
title: Type parameter packs, <..T>, as C++ has them
milestone: M19
status: open
kind: feature
area: generics
---

**Asked for by the user on 2026-11-26** in place of the macro `spawn!` (D-0121, spec §8.7,
r155): "`..T` the same way C++ … already support[s]".

**Built (r155).** A function or a method declares a pack, `<..A>` or `<view ..A: Send>`,
last; `..A` stands for its types in a tuple or a function type's parameters; a pack
parameter, `args: ..A`, takes the rest of the arguments and is the tuple `(..A)` in the body;
`..t` spreads a tuple, a pack's or one of known types, into a call or a tuple literal. A pack
is inferred from the arguments or a function type, or written after the fixed type
arguments, and its bounds hold of each type. `std`'s `spawn` takes any number of
arguments. Tuples hold up to sixteen elements (the runtime's `Tuple` spells each length out).

**Left, each for when a program needs it:**

- **A pack on a struct, union, trait or implementation**, `Query<..C>` (D-0096 writes five
  `Query`s without it). Refused now with DZ0901. The representation is ready: a `Named`'s
  argument for the pack would be the `Ty::Tuple` of its types, which `substitute` splices.
- **A pack's length**, C++'s `sizeof...(T)`.
- **An expression repeated over each element**, C++'s `g(args)...` and its folds: needs a
  body checked once per element against the bounds (spec §8.1), so it is a design question.
- **A function with a pack parameter taken as a value** (DZ0902 now): its C++ takes one
  tuple, so a value of it would need a wrapper taking the elements.
- **A tuple with a pack and more spread**, `..t` with `t: (i32, ..A)` (DZ0903 now).
- **Tuples longer than sixteen** (DZ0904): a generated `Tuple` specialization per length
  used would lift the limit.

## Where the code is

| Piece | Location |
|---|---|
| Syntax: `<..T>`, `..T` in a type list | `crates/dz_parse/src/item.rs` (`parse_generics`), `crates/dz_parse/src/ty.rs` (`parse_type`, `TokenKind::DotDot`); `GenericParam::is_pack`, `TypeKind::Expand` in `dz_ast`; printed by `dz_ast::dump` and `dz_fmt` |
| A pack in the type system | `ty::Param::pack`, `substitute_list` and `pack_in` in `crates/dz_hir/src/ty.rs`: a pack is solved to a `Ty::Tuple`, spliced into the list it stands in |
| Where a pack may be declared and written (DZ0901, DZ0902) | `crates/dz_check/src/lib.rs`: `check_packs` (from `declare_params`), `lower_type_list`, `lower_expansion`, `pack_named`, the bare-pack check in `lower_path_type`, `param_sigs` (a pack parameter's type is `(..T)`), `FnSig::pack_param` |
| A call: inference, gathering, spreads (DZ0903, DZ0904) | `crates/dz_check/src/check/packs.rs` (`Slot`, `slots`, `unify_args`, `bind_written`, `check_pack_bounds`, `check_pack_args`, `check_spread_args`, `fn_value_args`, `tuple_with_spread`); called from `check_args`, `solve_call`, `check_bounds`, `fn_value_type_args` in `check.rs`; `unify_list` there; element by position in `field_type` |
| The spread node | `hir::ExprKind::Spread` in `crates/dz_hir/src/lib.rs`; moves in `crates/dz_check/src/moves.rs`; flow in `crates/dz_flow/src/lib.rs` (`read`), `ties/body.rs`, `placement.rs`, `release.rs`, `storing.rs` |
| C++ | `crates/dz_cpp/src/lib.rs`: `expr_node` (`Spread` as its elements), `capture_each` and `emit_args` (never held, an empty one left out); `Tuple` up to sixteen in `crates/dz_cpp/src/runtime.rs` |
| `spawn` over a pack | `crates/dz_project/src/std/sync/thread.dz` (`spawn`, `Start<F, P>`, `start_thread`) |
| Tests | `tests/run/packs`, `tests/run/threads`; `tests/workspace/typed` and `typeerrors`; `tests/errors/pack_*.dz`; `tests/parse/items.dz`, `tests/fmt/items.dz`; `dz explain DZ0901`…`DZ0904` in `crates/dz_diag/explain/generics.md` |
