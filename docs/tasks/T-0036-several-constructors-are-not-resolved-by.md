---
id: T-0036
title: Several constructors are not resolved by argument
milestone: M12
status: open
kind: deferral
area: checker
where: `dz_check/src/check.rs`, `call`
---

**Several constructors are not resolved by argument.** A constructor call, and a base initialiser, take the first one declared — the same limitation both had before inheritance, now visible in two places

**Why a game needs it** (from the readiness review, 2026-09-14):

**Constructor chosen by argument types** — today the first declared constructor
wins.
