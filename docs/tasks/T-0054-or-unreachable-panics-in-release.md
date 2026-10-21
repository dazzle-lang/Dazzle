---
id: T-0054
title: `or unreachable` panics in release
milestone: M12
status: done
done: r59, 2026-09-15
kind: deferral
area: codegen
where: `dz_cpp/src/lib.rs`, `OrClause::Unreachable`
---

**Settled by D-0020 and built at r59.** The open question was whether a release build's
`or unreachable` should emit `__builtin_unreachable` or merely skip the check. D-0020 chose
`__builtin_unreachable` — the compiler assumes the success path — on the grounds that
`or unreachable` is already the loud spelling of "this cannot fail", and §1.1 does not
promise safety.

`OrClause::Unreachable` in `dz_cpp/src/lib.rs` now emits `::dz::unreachable(loc)`, which
panics with the location, when the root module's `optimize` is `Debug`, and
`__builtin_unreachable()` for release and shipping; the function-end safety net keeps its
panic either way. `overflow_wraps_and_unreachable_is_assumed_in_release` runs a program in
all three modes. Spec §9.4 says the rule, and `docs/log/m12.md` records the work under r59.
