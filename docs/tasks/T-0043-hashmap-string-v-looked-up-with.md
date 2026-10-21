---
id: T-0043
title: A `HashMap<String, V>` is looked up with a `&String`
milestone: M12
status: open
kind: deferral
area: stdlib
where: `dz_project/src/std/collections/hash_map.dz`
---

**A `HashMap<String, V>` is looked up with a `&String`**, so finding a key you hold as a `str` builds a `String` first. Rust answers this with `Borrow<Q>`; Dazzle has no such trait yet

**Why a game needs it** (from the readiness review, 2026-09-14):

**`HashMap<String, V>` looked up by `str`** (`Borrow<Q>`).
