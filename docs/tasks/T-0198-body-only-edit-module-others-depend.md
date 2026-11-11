---
id: T-0198
title: A body-only edit to a module others depend on leaves them as they were
milestone: M15
status: done
kind: feature
area: tooling
where: `Facts` and `Interface` in `crates/dz/src/reuse.rs` (`select` compares them); `BodyPlacement::fingerprint` in `crates/dz_flow/src/placement.rs`; `Checked` and placement before `select` in `crates/dz_driver/src/lib.rs`; scenarios `root_constructor_body` and `root_body_grows_above_a_generic` in `crates/dz/tests/incremental.rs`
---

**Done** (2026-11-11), stage 2b of the incremental-build plan. **What was wrong:** after
T-0197 an edit redid every module depending on the edited one, so a body edit to the root
module of the 100-module project redid all 100: 2.32 s, against 1.0 s for `make`, which
recompiles only the edited `.cpp`.

**Now** a module's record also holds a hash of its **interface**, what of it other modules'
C++ can be made from:
- its declarations and signatures, constants, statics, macros and imports;
- field defaults and default arguments, with their lines;
- every generic item and trait whole, with its line: their bodies are instantiated or
  inherited in other modules, and their `#line`s name its lines.

The body of a plain function, method or constructor is left out. A module is redone when
its own sources changed, or the interface of anything beneath it did.

**Placement** can also carry a body's change across modules: whether a callee keeps its
argument, or builds its result whole, is read from the callee's body. So the record also
holds a fingerprint of what placement decided for the module's own bodies, and one for
every generic body in the program, and a difference in either redoes the module. Placement
now runs before the modules are chosen; `--timings` lists it before ownership.

**Checked by** the edit corpus (T-0193), with two scenarios added:
- `root_constructor_body`: the modules above are kept, and the result is still the clean
  build's.
- `root_body_grows_above_a_generic`: a body grows a line above a generic only `m001`
  instantiates. With lines left out of the interface it fails, as verify reports
  `m001.inst.cpp` differs; it passes with them.

**Measured** at 100 modules (clang++): a body edit to the root module went from 2.32 s to
0.88 s, against 1.00 s for `make`.

**Left:** placement and the front end still cover the whole program (0.29 s and 0.17 s at
100 modules). And a body that grows a line still redoes the modules using a generic further
down its file, since the generic's lines moved.
