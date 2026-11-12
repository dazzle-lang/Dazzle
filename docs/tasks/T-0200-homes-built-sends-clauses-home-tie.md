---
id: T-0200
title: Homes built: sends clauses and the home tie (D-0097)
milestone: none
status: done
kind: feature
area: ownership
---

**What it is.** Build [D-0097](../decisions/D-0097-homes-ownership-that-travels-with-its.md),
decided on 2026-11-11 (r119). Every value that owns allocator memory has a home, `x@home`,
the region or heap it was placed in. Ownership moves only to an owner whose home dies no
later. A function that hands what one input owns to another says so with
`sends from[*] to store`, and an owned result taken out of a parameter says where it lives
with `-> T in xs@home`. Placement then reads what a function stores and what leaves a
container from signatures, instead of inferring it from bodies within one module. That
closes [T-0150](T-0150-value-stored-into-container-lent-function.md), and lets values given
to another module's containers leave the heap.

**Why it matters.** Today a value put into a container a function was lent, or into a
container of another module, keeps the heap wherever the container lives. On D-0097's
frame loop that is every value a system puts into the frame's event list, draw list and
paths: 35 `malloc`s per frame that the frame's region could serve, where the same work
written in one body makes none.

**Where the code is**

| Piece | Where |
|---|---|
| `kills` clause: parsed, in the AST | `parse_kills` and the clause after the return type in `dz_parse/src/item.rs`; `kills`, `kills_span` in `dz_ast/src/lib.rs` |
| Tie declarations (where `tie @home` is refused) | `dz_check/src/ties.rs` (`DZ0567`, `TIE_DECLARED_TWICE`, is the pattern) |
| A body's kills checked against its clause, both ways, and `dz fix` | `dz_flow/src/ties/report.rs` (`DZ0415`, `DZ0416`, `DZ0417`) |
| The ties check that follows values (where homes are compared) | `dz_flow/src/ties/body.rs` |
| What a method stores into `self`, inferred today | `storing_params` in `dz_flow/src/storing.rs`; `Staying::stores` in `dz_flow/src/placement.rs` (own module and `std` only) |
| Whether an element leaves a container, inferred today | `lets_go` in `dz_flow/src/placement.rs` |
| The fifteen `std` signatures | `std/collections/list.dz`, `hash_map.dz`, `hash_set.dz`, `slot_map.dz`, `std/memory/box.dz` |
| Diagnostic codes | `dz_diag/src/codes.rs` (DZ0220, DZ0581, DZ0582, DZ0583), explained in `dz_diag/explain/parser.md` and `check.md` |
| `sends` parsed; `FnDecl::sends` | `parse_sends` in `dz_parse/src/item.rs`; `Send` in `dz_ast/src/lib.rs`; `dump::sends_str` |
| `FnSig::sends`, `FnSig::ret_home`, and the split of an `in` | `fn_sig_inner` in `dz_check/src/lib.rs`; `ends_at_home` in `dz_check/src/ties.rs` |
| `@home` resolved, and its misuse refused | `Walker::walk` (`Step::Home`), `check`, `sendable`, `home_misplaced`, `owns_memory` in `dz_check/src/ties.rs` |
| The programs D-0097 was tried on | `tests/run/transfers` (with its module `movers`) |

**Stages**, each its own commit:

1. **Syntax. Done 2026-11-11.** `sends` is a keyword (`to` is read as a word, so it stays
   a name); `sends a to b, c to d` comes before or after `kills`, and the formatter writes
   it after. `FnSig::sends` holds the pairs and `FnSig::ret_home` an owned result's
   `in … @home`, kept apart from `ret_ties`, which `dz_flow` reads as where a view points.
   `ties::check` resolves every path (a static's included) and refuses `@home` anywhere
   but the end of an owned result's `in` (DZ0582), a `sends` from what the function
   neither owns nor can change or to what it cannot change (DZ0583), and `tie @home`
   (DZ0581); a pair with no `to` is DZ0220. Nothing yet checks a clause against its
   body. **Left for stage 2:** a function *type*'s `sends` (a function value's callers
   need it as a direct call's do), and a trait method's `sends` against its implementations'
   `sends`, as `clauses_within_trait` in `dz_check/src/check.rs` compares their `kills`.
2. **The check. Done 2026-11-11.** `analyse_homes` (`dz_flow/src/ties/body.rs`) walks a
   body a third way, beside the ties check and the region check, with each input's home as
   a fact carried in `Value::owned`: a by-value parameter's value is in its own, and an
   element taken out of a `&mut` input is in that input's. Where the region check refuses
   a placement, it records a send instead: an assignment under another input, a call
   whose `sends` (or, for `std` until stage 3, whose inferred storing) puts an argument
   under one, and a `return`. `report_homes` (`ties/report.rs`) compares both ways:
   DZ0421 for a send or an owned return the signature does not declare, DZ0422 for one
   the body never does, each with the clause it needs as a fix. A by-value parameter
   returned needs `in x@home` too (decided 2026-11-11, D-0097). `std` is skipped until
   stage 3. Refusing a move into an owner that may outlive a value is this check: inside
   one body nothing is refused (D-0097, Tried), so a refusal is always a clause missing
   at a signature. Still open: a function type's `sends`, and a trait method's against
   its implementations' (both left from stage 1), and `dz fix` extending an `in` already
   written. The check is a whole extra walk per body, about 5% of `dz check` on the
   examples.
3. **`std`'s signatures. Done 2026-11-11.** Seventeen, by hand: `sends` on `List::push`,
   `insert` and `append`, `HashMap::insert`, `replace`, `get_or_insert` and the private
   `place`, `HashSet::insert` and `SlotMap::insert`; `in self@home` on `List::pop`,
   `remove` and `swap_remove`, `HashMap::replace`, `remove` and the private
   `remove_slot`, `SlotMap::remove`, and `StrKeys::remove_str` (declaration and
   implementation). `Box(…)` needs none: a constructor's value holds what it is given.
   The walk follows `construct(dst, v)` as a store under `dst` and `move_out(src)` as
   taking out of `src`, which is how `std`'s containers move elements, and no longer
   counts the views walk's trust of a body over a pointer's elements (T-0116) as a gap in
   homes. With the clauses written, the stage-2 stand-ins are gone: a call is read by
   its callee's clauses alone, and `Staying` is no longer computed for the check. A build
   leaves `std`'s bodies out, since `std` is the same for every program;
   `dz_flow::check_std_homes` checks them, run by `std_says_where_it_sends_what_it_is_given`
   in `dz_flow/tests/flow.rs`. The check now costs about 3–5% of `dz check`.
4. **Placement reads clauses.** *First part done 2026-11-11:*
   - **A function builds in a lent container's home** (T-0150). A value handed by a call
     whose `sends` says `v to self` to a receiver under a parameter or `self` gets
     `Decision::Home`. The site has to be direct, or surely stored after a `let`, and the
     receiver's type has to keep an allocator (`placement::home_path`, the fields down to
     `m_allocator`). Codegen passes `::dz::obj(…).m_allocator` from the receiver
     (`Placed::Home`).
   - **The region check accepts such a site**, as a by-value parameter's value: no block
     of the body releases it, a `return` cannot take it, and it may go only into its
     container's input (`home_input`, `home_sent_roots`).
   - **Callers stay sound.** A local container lent `&mut` to a function of the program's
     own may have been built into (`built_into`). What a callee's `sends` hands on from a
     lent container goes with that container's regions (`region_clauses`), and so does a
     result its callee says is `in x@home`. The proposer refuses the same containers
     (`built_and_left`), so the two agree.

   `dz placements` counts homes, and `tests/run/regions` builds four strings in a lent
   container's home. D-0097's frame loop makes no `malloc` per frame and runs 9% faster
   than on the heap.

   *Second part done 2026-11-11:*
   - **Lending keeps what was stored.** A container lent `&mut` to a call with clauses
     keeps what was stored into it (`lend_keeps`, `lend_kept`). Nothing else can leave it:
     what the `sends` hands on and an `in x@home` result are followed at the call. A
     function value or a C function still loses it. The proposer refuses only where the
     clauses hand something on ("handed on by `f` from").
   - **Other modules' and local containers.** These are read through their `sends`
     wherever `Staying::stores` has no answer (`sends_to_self`), though as not always kept,
     so the loop rule stays strict for them.
   - **An outer block's region instead of the heap.** A container a callee may build into,
     whose elements a `sends` puts only into locals of enclosing blocks with no loop
     between, is in the outermost such block's region (`Left::Into`).

   `tests/run/transfers` gained K5, a `Shelf` of another module that keeps its strings in
   the region, and K6, a list lent to another module's sort that keeps them too.
   `tests/run/regions`' `lent` moved from the heap to `main`'s region.

   *Left:*
   - **In-place growth.** A container's buffer interleaved with the values built into its
     home cannot grow in place (`resize_in_place`), so the frame loop is 3% slower than
     with those values on the heap, though still faster than all-heap: T-0201.
5. **The spec. Done 2026-11-11.** §6.4 says a move hands on ownership, not memory; §6.7
   has homes, `sends` and `in x@home`, and no longer calls `pop`'s result untied; §14.2
   says a container's elements live in its home, and what `resize_in_place` is for; §14.6
   says a function lent a container builds into its home, and drops T-0150 from what is not
   implemented; §0.1 has a row for homes. The r119 row no longer says "being built".

**Done.** In-place growth beside what is built into a list's home, the last item of stage 4,
is [T-0201](T-0201-list-in-region-grows-in-place.md). What stages 1 and 2 left open (a
function type's `sends`, a trait method's against its implementations', and `dz fix`
extending an `in`) is [T-0202](T-0202-homes-through-traits-function-types-sends.md).
