---
id: T-0186
title: Checking that objects are up to date hashes every header each one includes, on every build
milestone: M15
status: done
kind: feature
area: tooling
where: `crates/dz/src/native.rs`: `up_to_date`, `write_stamp`, `Input`, `SETTLED`; the link stamp in `compile_and_link`, `crates/dz/src/build.rs`
---

**Done** (2026-11-09). A stamp line is now `hash size time path`. A file whose size and time
match is taken as unchanged when that time was at least 2 s older than the stamp
(`SETTLED`); any other is read and hashed, and a stamp that needed hashing and still holds
is written again so the next build can go by the times. That is git's rule for "racy"
index entries: a file written twice within one tick of a coarse file-system clock keeps its
time. A stamp in the old format means compiling again. Then the link: it is skipped when
the command and the size and time of every file it names match `link-<exe>.stamp` in
`build/obj/dz/`, as `make` skips it. On T-0012's 100-module project with nothing changed,
the C++ compiler phase went from 3.2 s to 28 ms and the link from 0.38 s to 0.2 ms; the build
is now 4.5 s, all of it Dazzle's own phases (T-0187).

| Piece | Where |
|---|---|
| The stamp and the check | `Input`, `up_to_date`, `write_stamp`, `stamp_paths` in `crates/dz/src/native.rs` |
| The link skipped | `link_stamp`, `link_key` in `compile_and_link`, `crates/dz/src/build.rs` |
| Test | `an_object_is_judged_by_content_until_its_inputs_times_settle` in `native.rs` |

**What is wrong.** An object is up to date when every file its depfile names still has the
content it had (T-0102). That is checked by reading and hashing every one of them, on every
build. T-0012's generated project shows the cost: with nothing changed, the C++ compiler
phase of `dz build` is 78 ms at 10 modules and 3.2 s at 100, all of it hashing, since each of
the 100 objects names every header beneath it (T-0185) and those are read again for each.

**What it would take.** Record each input's size and modification time in the stamp beside
its hash, and hash only a file whose size or time differs, as the header cache does
(`dz_cppimport/src/disk.rs`, T-0101). A file read once per build would do too, since the
same header appears in many stamps. Compiler work, for `docs/log/`.

