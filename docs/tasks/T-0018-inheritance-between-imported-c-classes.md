---
id: T-0018
title: Inheritance between imported C++ classes
milestone: M7b
status: open
kind: deferral
area: interop
where: `dz_cppimport`, `collect_methods`; `dz_check/src/lib.rs`, `collect_base`
---

Inheritance between imported C++ classes: a base's members are not imported into a derived class, and `&Derived` does not convert to `&Base`. **No longer blocked** — struct inheritance landed at r30, so the Dazzle side (a `Symbol::base`, a flattened field list, the conversions) exists and what remains is the importer filling those in from libclang, plus letting `: Base` name an imported class, which `collect_base` refuses today with `DZ0541`

**Why a game needs it** (from the readiness review, 2026-09-14):

**Inheritance between imported C++ classes** (M7b) — Jolt's shapes, bodies and
listeners are class hierarchies.
