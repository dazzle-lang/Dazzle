---
id: D-0058
title: Whether a dependency's names reach the modules that depend on this one
status: decided
---

**Private by default; `pub = true` re-exports it, nested under the exporting module** (`rl::shapes::Rectangle`), and a dependent may name it itself with `via = "rl::shapes"`. A private dependency's type in a `pub` signature is an error (DZ0311). Nested rather than merged into the exporter's names, because a wrapper usually reuses the names of what it wraps. Follows Java's `requires` / `requires transitive` and Swift 6's `internal import` / `public import`, which also rejects a public API naming an internally-imported type; Rust merely warns, and leaves users holding values whose type they cannot write (§3.2)
