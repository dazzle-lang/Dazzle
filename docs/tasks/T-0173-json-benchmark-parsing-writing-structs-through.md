---
id: T-0173
title: A json benchmark: parsing, and writing structs through reflection
milestone: M15
status: done
kind: feature
area: bench
where: `tests/bench_programs/json/` (`src/main.dz`, `main.cpp`, `quick_args.txt`); `crates/dz_check/src/lib.rs`: `COPY_STACK`; `crates/dz_project/src/std/collections/list.dz`: `grow_to`; `crates/dz_cpp/src/runtime.rs`: `text_parse_f64`
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

**Done 2026-11-14.** 100,000 records, 17 MB in and 13 MB out; both halves agree on every
byte written, with clang++ and g++. As the task foresaw, **reading back is by hand** on
both sides: filling a `String` or a `List<String>` found at run time means building one at
a raw address, which safe code cannot do. Writing goes through reflection, one serializer
for every type (`String` recognised by its type id, `List<T>` by its name and read through
its own `m_data` and `m_len`), against the C++ half's hand-written `to_json`. **Numbers**
have at most six significant digits, because `{}` on a float is C's `%g` (T-0223); on
those, the Dazzle text and `std::to_chars`'s shortest text agree, and the checksum is over
the written bytes.

Building it found and fixed three things:

- **The compiler overflowed its stack** checking a union that holds a `List` of itself:
  whether it copies asks whether `List<Json>` does, and `List`'s `impl<T: Copy> Copy`
  asked about `Json` again from an empty stack, forever. A question asked from inside the
  answer now inherits its stack (`COPY_STACK`). `tests/run/recursive_union` pins it.
- **`List`'s first allocation was 8 elements of any size**, so a three-member object kept
  room for eight 88-byte members. It is now sized by the element, as Rust's `Vec` is: 8
  bytes, 4 elements up to a KiB, 1 beyond. `json` 376 → 329 ms and 316 → 237 MiB; the
  other programs did not move.
- **`parse_f64` uses `std::from_chars`** where the C++ library has it for floats, with
  `strtod` for whatever it does not take (a `+`, a value out of range), so every result
  is the one it was: parsing 160 → 142 ms.

First run: 1.40× its C++ half on clang++ and 1.56× on g++, from 1.70× and
1.91×. What is left is T-0225, with T-0223 (float text) and T-0224 (`while true` and
definite return, met writing the parser) for the user to decide.

The same day the user decided both: `{}` writes a float in full (r130, D-0101), and `loop`
is a keyword (r131, D-0102). With floats found by `std::to_chars` on both sides, `json` is
1.25× on clang++ and 1.42× on g++ (`RESULTS.md`); its numbers no longer need to be short.
