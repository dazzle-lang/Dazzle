---
id: T-0269
title: Type parameter packs, <..T>, as C++ has them
milestone: M19
status: done
kind: feature
area: generics
---

**Asked for by the user on 2026-11-26** in place of the macro `spawn!` (D-0121, spec §8.7):
"`..T` the same way C++ … already support[s]". Done the same day, in two revisions.

**r155.** A function or a method declares a pack, `<..A>` or `<view ..A: Send>`, last; `..A`
stands for its types in a tuple or a function type's parameters; a pack parameter, `args:
..A`, takes the rest of the arguments and is the tuple `(..A)` in the body; `..t` spreads a
tuple into a call or a tuple literal. A pack is inferred from the arguments or a function
type, or written after the fixed type arguments, and its bounds hold of each type. `std`'s
`spawn` takes any number of arguments.

**r156**, the user's choices of spelling: `..pattern` repeats an expression per element
(`..e` of a place is a spread, `..e` of anything else naming a pack's tuple a pattern); `..R`
in a type, `(..A?)`; `countof(A)`; packs on structs, unions, traits and implementations; a
pack function as a value; a tuple with a pack and more spread; tuples of any length.

**Not done**, each for when a program needs it: a pattern naming two packs (C++ expands them
in step); a pattern with a block, branch, `or` or formatting macro in it (DZ0903); `..R` in an
implementation's own pattern (`match_list` in `impls.rs` matches `..A`, not `..R`). Found on
the way and recorded apart: T-0270 (a long `+` chain overflows the checker's stack) and
T-0271 (an implementation generic only through its trait's arguments).

## Where the code is

| Piece | Location |
|---|---|
| Syntax: `<..T>`, `..T` and `..R` in a type list | `crates/dz_parse/src/item.rs` (`parse_generics`), `crates/dz_parse/src/ty.rs` (`parse_type`, `TokenKind::DotDot`); `GenericParam::is_pack`, `TypeKind::Expand` in `dz_ast`; printed by `dz_ast::dump` and `dz_fmt` |
| A pack in the type system | `crates/dz_hir/src/ty.rs`: `Param::pack`, `Param::element` (the `ELEMENT` bit), `Ty::Each` and `each`, `substitute_list` (splices `..A` and `..R`), `pack_in`, `pack_entry` |
| Declaring and writing packs (DZ0901, DZ0902) | `crates/dz_check/src/lib.rs`: `check_packs`, `ParamDef::as_arg`, `lower_type_list`, `lower_expansion` (`..R` with the element bound), `names_in_type`, `pack_named`, `type_args` and `trait_args` (a pack's arguments grouped), `param_sigs`, `FnSig::pack_param`; `Symbol::packed` in `dz_resolve` prints a type's pack flat |
| A call: inference, gathering, spreads, patterns (DZ0903) | `crates/dz_check/src/check/packs.rs` (`Slot`, `slots`, `expansion`, `names_in_expr`, `expand_node`, `expand_into_slice`, `refuse_uncopyable`, `unify_args`, `bind_written`, `check_pack_bounds`, `check_pack_args`, `check_spread_args`, `fn_value_args`, `tuple_with_spread`, `count_query`); from `check_args`, `solve_call`, `check_bounds`, `fn_value_type_args`, `variadic_positional` in `check.rs`; `unify_list` and `value_params` there |
| Implementations over packs | `match_list`, `lists_might_overlap` in `crates/dz_check/src/impls.rs` |
| HIR nodes | `Spread`, `SpreadRest`, `TupleEnd`, `Expand`, `Expanded`, `Body::pack_param`, `TypeQuery::Count` in `crates/dz_hir/src/lib.rs`; `expand_patterns` and `copy_tree` (one copy of a pattern per element, in `map_body_types`), `remap_children` |
| Ownership | `crates/dz_check/src/moves.rs` (a pattern takes its tuple where it takes the element); `crates/dz_flow/src/lib.rs` (`read`), `ties/body.rs`, `placement.rs`, `release.rs`, `storing.rs` |
| C++ | `crates/dz_cpp/src/lib.rs`: `spread_text`, `Expanded` and `SpreadRest` in `expr_node`, `emit_args` (`SPREAD_SEP`), `tuple_member` and `tuple_braces` (past sixteen), `pack_arg` and `decl_params`/`take_ownership_of_params` (a pack parameter as one C++ parameter per element), `type_query` (`Count`); `Tuple` and `leave_empty` in `crates/dz_cpp/src/runtime.rs` |
| `spawn` over a pack | `crates/dz_project/src/std/sync/thread.dz` |
| Tests | `tests/run/packs`, `tests/run/pack_types`, `tests/run/pack_patterns`, `tests/run/threads`; `tests/workspace/typed` and `typeerrors`; `tests/errors/pack_*.dz`; the feature matrix; `tests/parse/items.dz`, `tests/fmt/items.dz`; `dz explain DZ0901`…`DZ0903` |
