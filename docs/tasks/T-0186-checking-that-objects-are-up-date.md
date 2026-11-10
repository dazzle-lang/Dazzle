---
id: T-0186
title: Checking that objects are up to date hashes every header each one includes, on every build
milestone: M15
status: open
kind: feature
area: tooling
where: `crates/dz/src/native.rs`: `up_to_date`, `write_stamp`; `Pch::fresh` in `crates/dz/src/build.rs`
---

**What is wrong.** An object is up to date when every file its depfile names still has the
content it had (T-0102). That is checked by reading and hashing every one of them, on every
build. T-0012's generated project shows the cost: with nothing changed, the C++ compiler
phase of `dz build` is 78 ms at 10 modules and 3.2 s at 100, all of it hashing, since each of
the 100 objects names every header beneath it (T-0185) and those are read again for each.

**What it would take.** Record each input's size and modification time in the stamp beside
its hash, and hash only a file whose size or time differs, as the header cache does
(`dz_cppimport/src/disk.rs`, T-0101). A file read once per build would do too, since the
same header appears in many stamps. Compiler work, for `docs/log/`.

