---
id: T-0173
title: A json benchmark: parsing, and writing structs through reflection
milestone: M15
status: open
kind: feature
area: bench
---

Planned for 0.2 by the user on 2026-11-09, one of two new whole-program benchmarks.

**What it is.** `tests/bench_programs/json`, a Dazzle half and a C++ half under the rules in
`tests/bench_programs/README.md`. It generates a JSON document of records (about 20 MB, from
the shared SplitMix64), each like:

```dz
struct Record {
    id:     u64
    name:   String
    tags:   List<String>
    pos:    Vec3
    active: bool
    score:  f64?
}
```

and then:

1. parses the text into a tree, a tagged union of null, bool, number, string, array and
   object;
2. turns the tree into a `List<Record>`;
3. writes the records back out as JSON, through reflection (`typeinfo`, field names or
   `json:"…"` tags), with one serializer written once for every type, as `tests/run/reflect`
   does.

The C++ half uses `std::variant` for the tree and writes `to_json` and `from_json` by hand
for each struct, since C++17 has no reflection.

**Why it matters.** The three programs there now are all hashing, text and allocation.
None times reflection (M11), a recursive union tree being built and freed, or number
parsing and formatting, and JSON is something nearly every program reads.

**Things to settle while building it.**

- **Reading back through reflection.** `tests/run/reflect` only writes. Filling a struct
  from the tree by field name needs to write a field found at run time, which may need
  `unsafe` or [T-0029](T-0029-t-fields-type-parameter.md). If it cannot be done in safe
  code, step 2 is written by hand on both sides, reflection is used only for writing, and
  the README says so.
- **Number text.** Dazzle's `f64` formatting and C++'s (`std::to_chars` or `snprintf`) may
  print different but equally right digits. Take the checksum over the parsed values and
  the written text's length and structure, not its bytes, or generate numbers that print
  exactly (integers, and decimals with a few digits).

**What it would take.** `src/main.dz`, `main.cpp`, `dazzle.toml`, `quick_args.txt`, a row in
the README's table and `RESULTS.md`. A benchmark; no language change. What `std` lacks on
the way becomes a task or an addition.
