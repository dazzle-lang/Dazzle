---
id: D-0071
title: Text operations
status: decided
---

**One `Text` trait, implemented for `str` and `String`**, whose operations are default methods over `text()`: both get every one, a literal included, with no `Deref` and no duplicate APIs. Pieces are `str` views; only `replace` and the case changes allocate. Parsing takes the whole text and returns an optional, refusing surrounding spaces rather than skipping them, since `trim()` says that explicitly. Case changes are ASCII only — Unicode case mapping needs tables `std` does not carry (§14.4)
