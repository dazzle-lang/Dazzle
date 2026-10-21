---
id: T-0061
title: An API dump: `dz api <module>`
milestone: none
status: open
kind: feature
area: llm-tooling
---

**An API dump: `dz api <module>`.** Prints a module's public surface as compact
Dazzle signatures with their `///` docs — `dz api std`, and for an `import cpp` module,
`dz api rl`. Guessed stdlib names are where models invent the most, and for C/C++ the
model knows the C API but not its Dazzle shape (`^T` versus `&T`, handles, callbacks,
§15.2). Showing it `rl` *as Dazzle* closes that gap. Close to the `dz doc` of §17.3.
