---
id: T-0149
title: Stored values left on the heap: a map filled in a loop, a container lent as &mut, format! text in a loop
milestone: none
status: done
kind: feature
area: ownership
---

Left by [T-0145](T-0145-value-stored-into-container-built-where.md), which builds a value
stored into a container where the container lives, but only where it can follow the value
there and back out.

**Where the code is** (as built, 2026-11-02)

| Piece | Where |
|---|---|
| An insert under a lookup that found nothing: `adds_new` (`looks_up`, `first_touch`, `place_path`, `key_local`) | `dz_flow/src/storing.rs` |
| Which containers keep everything stored into them, now through `adds_new` and a program's own ties: `keeps`, `keeps_through` | `dz_flow/src/storing.rs` |
| Whether killing a tie spares the elements, from the type's declarations: `tie_spares_elements` | `dz_flow/src/ties.rs` |
| `format!`'s text as the macro's value, for both sides: `formatted_by` | `dz_flow/src/placement.rs` |
| The proposer: `stored_by` (takes the call), `temporary` (no longer told the text is indirect) | `dz_flow/src/placement.rs` |
| The check: `kept_each_pass`; `Regions::stored`, now by container | `dz_flow/src/ties/body.rs` |
| A container lent to a function, not built | [T-0150](T-0150-value-stored-into-container-lent-function.md) |
| Tests | `a_map_filled_under_a_failed_lookup_keeps_what_it_is_given`, and `filled`'s `format!` in `a_value_stored_into_a_container_is_built_where_it_lives`, in `dz_flow/tests/placement.rs`; `tallied` and `labelled` in `tests/run/regions` |

**What it was.** Four cases where a value stored into a container in a region still kept
the heap:

| The case | Why it kept the heap | Now |
|---|---|---|
| a map filled in a loop, `names.insert(String(name), v)` (`expr`, and `words` once its keys are not copies) | `insert` may drop the value it replaces, so on every pass the region could grow past what the map holds (the loop rule) | placed when the insert is guarded by a lookup of the same key that found nothing: it never replaces |
| a container lent to a function, `fn tag(names: &mut List<String>)` | the callee cannot see where its caller's container is placed | **not built**: the proposed answer is not sound alone, [T-0150](T-0150-value-stored-into-container-lent-function.md) |
| `format!`'s text pushed in a loop, `names.push(format!(…))` | the text is built inside the macro's lambda, so it was not stored "directly" | placed: the macro's value is the text |
| a program's own container whose `kills` clause names a tie other than `@items` or `@text` | `keeps` could not tell such a tie from one over the elements | placed when the tie's declaration spares the elements |

**Built 2026-11-02.** Behind `DZ_REGIONS`, as all placement is. None of it is a safety rule:
the loop rule bounds memory, and the check follows every value it places whatever the rule
says.

- **The guard (row 1).** `adds_new` recognises `std`'s `HashMap::insert` and
  `HashSet::insert` in the `else` of an `if let` on `get`, `get_mut`, `get_str` or
  `get_mut_str`, or under an `if` on `contains` or `contains_str` (the `else`, or the `then`
  of `!`). The lookup is on the same place, the two keys are read from the same local
  (through `&`, `move`, `as_str()`, `String(…)`, `String::from(…)`), and nothing in the block
  before the insert mentions the map or the key; plain blocks are looked through. Such an
  insert keeps what it is given, for the proposer (`stored_by`), for `keeps` (it does not
  count against the map) and for the check (`kept_each_pass`). The std method that only adds,
  the task's other answer, was not needed.
- **`format!` (row 3).** `formatted_by` finds the macro whose value a site is, and both
  sides start from it. Building it found a bug in the check: whether a site had been stored
  was one flag per site, so on a loop's second pass the `Formatter` borrowing `&mut text`
  looked like lending a container with stored values in it. `Regions::stored` is now by
  container.
- **A program's own ties (row 4).** A body may kill only what its clause covers, in canonical
  form (§6.7), so a tie spares the elements when each of its paths, made canonical, reaches
  none: a pointer's storage whose elements canonicalise elsewhere (`tie @names =
  m_names@items` of a `List` field), or a field that owns nothing. A tie over a whole owning
  field (`tie @all = m_names`) does not: a method that kills it may replace the field.
  `std`'s containers still go by name.
- **What it reaches**: `expr`'s names (`names.insert(String(name), value)` under
  `get_mut_str`) are in `main`'s region; with T-0140, so are `words`' keys.
