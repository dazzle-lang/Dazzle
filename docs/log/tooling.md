# Tooling — build modes, `dz test` (M14), `dz bench` (M15), CI

Part of the build log in `docs/log/` — what was built and decided, kept as the record. Open work is in `docs/tasks/`; nothing here needs reading to decide what to do next.

## Releases, the installers and `dz new` (T-0076; 2026-11-04)

The user asked what would make Dazzle easier to take up, and chose to start with being able to
install it: until now `dz` was built from source, with Rust, libclang and a `cargo build`
before a first program.

- **`.github/workflows/release.yml`** builds `dz` for x86-64 and ARM64 Linux (on Ubuntu
  22.04, so glibc 2.34 or newer will do), Apple-silicon macOS and x86-64 Windows. A tag
  `v0.1.N` publishes a release (the first, `v0.1.0`, on 2026-11-04); run by hand, it builds and tests the archives and keeps
  them as the run's artifacts, publishing nothing. `dz --version` names a release build by
  its tag (`DZ_RELEASE`); any other build is still `0.1.0`.
- **libclang** comes from LLVM's own release archives, pinned in `tools/release.py`
  (23.1.5). They build libclang with LLVM linked in statically, so on Linux it needs only
  libc, libstdc++, libgcc and zlib. The archives are 1–2 GB, so the script streams one and
  keeps only libclang and clang's 327 builtin headers (`stddef.h` and the rest, which glibc's
  headers include): about a minute. Stripped and gzipped, the x86-64 Linux release is 63 MB.
  `dz` already looked for libclang beside itself before anywhere else, and finds the headers by
  LLVM's layout rule, so no code changed for it: traced with `LD_DEBUG`, the archive's own
  copy is the one loaded.
- **macOS carries no libclang.** A Mac's C++ compiler comes from Xcode's command line tools,
  which ship libclang too, and clang-sys already looks there; it is the path CI tests. No
  Intel-Mac build: LLVM no longer publishes one to take libclang from.
- **Every archive is smoke-tested** before it is kept: unpacked, `LIBCLANG_PATH` unset, and a
  program built and run with it. `println!` reaches `std`, which imports `stdlib.h`, so the
  one program proves libclang loads and finds its headers.
- **`install/install.sh`** (`curl -fsSL https://dazzle-lang.github.io/install.sh | sh`) and
  **`install/install.ps1`** (`irm … | iex`) download the latest release, or the tag in
  `DAZZLE_VERSION`. They check it against `SHA256SUMS` and unpack it into `~/.dazzle`, then
  put its `bin/` on the PATH (the shell's startup file, or the user's PATH on Windows). They
  also say so when no C++ compiler is found. The site copies both to its root
  (`scripts/sync.mjs`). Tested here under `dash` against a local archive, twice: the second
  run replaces the first and adds no second PATH line.
- **The VS Code extension** is packed by the same workflow and attached as `dazzle.vsix`. It
  already ran the `dz` on PATH when it found none in the workspace.
- **`dz new <dir>`** (spec §17.3 listed it; T-0064 asked for it) writes a manifest, a
  `.gitignore` for `build/`, and `src/main.dz`: a function, a call of it, and a `#[test]` of
  it, so `dz run` and `dz test` both have something to show. It names the module after the folder (`-` read as `_`; `dz new .` uses the
  current one). It refuses a name that cannot be a module, and a folder that is not empty
  (`crates/dz/tests/new.rs`).

**The first dry run found that `dz` could not build anything on Xcode 15.** Linux, Windows
and the extension packed and passed, but macOS (GitHub's `macos-14` image, Xcode 15.4) failed
its smoke test: `module libc has no malloc`. A probe on a throwaway branch ruled out
libclang's choice: every combination failed, debug and release, command line tools' libclang
and Xcode's. Tracing the import showed why. Apple's libclang 15 reports an `extern "C" { … }`
block as an `UnexposedDecl` rather than a `LinkageSpec`, and the walk only looked inside the
latter, so every C function behind `__BEGIN_DECLS` was invisible: `malloc` and `free` for
`std`, and every function of a C library such as raylib. CI never saw it because its macOS job
runs on `macos-latest`, whose newer libclang reports a `LinkageSpec`. `flatten` in
`crates/dz_cppimport/src/clang.rs` now looks inside an `UnexposedDecl` too. The release job
builds on `macos-14`, so each release's smoke test covers the older libclang. Separately, on
macOS `dz` now loads the libclang beside the `clang` that `xcrun` picks, so the parser, the
SDK and the compiler are one toolchain.

Also: the README, the spec's examples and the site's pages write `String("ada")` rather
than `String::from("ada")`, which is the same constructor with a longer name. The user
looked at letting a literal convert where a `String` is wanted, and put it off (T-0170).

## What Dazzle's lead in the benchmarks is made of (2026-11-04)

The user asked whether `astar` beat C++ because its C++ half used `std::priority_queue`
rather than the same heap. Each C++ half uses its own standard library, so `astar` measured
Dazzle's SwissTable `HashMap` against `std::unordered_map`'s node per entry as well. To see
how much of each ratio the libraries make, Dazzle's map and its hashes, `List::sort_by`'s
introsort through a function pointer, and `astar`'s binary heap were ported line for line
into C++ and every C++ half run on them, with every pair still agreeing on its checksum.
Dazzle / C++, clang++ and g++: `astar` 1.00× and 1.01× (0.77× and 0.88× against `std`),
`expr` 1.19× and 1.13×, `words` 1.13× and 1.29×; peak memory `astar` 9.4 against 9.3 MiB,
`words` 37.6 against 34.0. The machine was busier than for the `std` run, every time
12–25% longer, so only the ratios compare. (The port had a bug of its own:
`values_[place(…)]` read `values_` before `place` grew the table, a use after free that
AddressSanitizer caught, and the kind Dazzle refuses to compile.)

The user then chose to keep `std` on the C++ side, as benchmarks between languages usually
do: each half uses its own library, and a report says where a library's choice decides a
ratio. `tests/bench_programs/README.md` and the website say that `astar`'s lead is mostly
the hash table, with these numbers; the port is not kept.

What placement is worth, from `tools/bench_regions.py`, each Dazzle half heap-only against
regions on the same day: `astar` the same, `expr` 3–7% slower heap-only, `words` 30% slower
heap-only on clang++ but 5% faster on g++ (T-0165).

## `HashMap` is a SwissTable (T-0164; 2026-11-04)

Library work; no program behaves differently, and §14.2's description of the map is
updated. The user chose a SwissTable over Robin Hood probing, the smaller change, once it
was the faster and smaller of the two.

- **The layout** (`std/collections/hash_map.dz`): one control byte per slot, `0xFF` for
  empty, `0x80` for a removed entry's marker, or seven bits of the hash. The bits are taken
  from the top of the hash times a large odd constant, so a weak hash still spreads them.
  The first group's eight bytes are repeated after the last slot, so a group read from any
  slot stays in the array. Keys and values are in their own arrays, as before. The table
  doubles at seven eighths full: `m_growth_left` counts the empty slots that may still be
  filled.
- **A lookup** reads eight control bytes as one word and matches the tag in all of them at
  once with integer arithmetic, since std has no SIMD. The first match's byte comes from a
  multiply, since Dazzle has no count-trailing-zeros. It compares keys only at matches and
  stops at a group holding an empty slot. Probing steps by growing multiples of a group,
  which visits every group of a power-of-two table.
- **Removal** marks the slot empty again when fewer than a group's worth of full slots
  surround it, so no probe can have passed it; otherwise it leaves a marker. When markers
  take the last room, the map is rebuilt at the same size if at most half of it holds
  entries, and doubles otherwise, so a map whose keys churn does not grow without bound.
  A rebuild hashes every key again, since a slot keeps only seven bits.
- **The group is loaded with a `memcpy`.** Assembled byte by byte, clang++ made it one load
  and g++ eight; that alone cost g++ most of `astar`'s gain. Byte order is checked by a test
  both compilers fold away, with a swap for a big-endian target.

Measured against the previous map, the same programs' C++ built by each, eleven alternating
runs at a load average of about 2:

| Program | clang++ | g++ | Peak memory |
|---|---|---|---|
| `astar` | 371 → 321 ms | 410 → 354 ms | 17.4 → 11.2 MiB (the C++ half: 12.6) |
| `expr` | 164 → 160 ms | 165 → 159 ms | about the same |
| `words` | 144 → 143 ms | 165 → 174 ms | 37.3 → 36.6 MiB |

`words` on g++ is the one loss. Its lookups go through `get_mut_str`, which compares keys
through a function pointer, and a 7-bit tag lets through more false matches than the
32-bit hash the old map stored. `RESULTS.md` is not re-measured: the machine was running
other work, and a run at a load average of 17 to 32 put the C++ half of `astar` at 2.5
seconds.

**Found on the way: the formatter broke a chained comparison.** It dropped the parentheses in
`(i % 2 == 0) == found`, and `a == b == c` does not parse, since comparison does not chain
(§11.1). A comparison inside a comparison now keeps them (`dz_fmt`, the `Binary` arm), and
`tests/fmt/expressions.dz` has both sides.

Tests: `tests/run/hash_map_churn` covers:
- half the entries removed and added back;
- a window of 100 keys slid along 100,000 times, whose slot count must stay small;
- 300 keys under one hash;
- `retain`, `clear`, a copy, `get_or_insert` counting, and `String` keys removed by `str`.

It is clean under ASan and UBSan with regions on and off. Every run program prints what it
did, every benchmark the C++ half's checksum, and the sanitizer suite is clean.

## A smaller `Box` moved once, and a smaller map slot (T-0097, T-0164; 2026-11-04)

Compiler and library work; no program behaves differently.

- **`Box` is 24 bytes, from 40** (`std/memory/box.dz`). It kept the size, alignment and
  drop function of what it was made as, so a `Box<UiElement>` holding a `Button` could drop
  and free a whole `Button` through any allocator. One function made for that type,
  `drop_boxed_as<T>`, now does both, so the box keeps only the pointer, its allocator and
  that function. `dz_cpp`'s upcast constructor copies three fields where it copied five.
- **A constructor's parameter that it always moves out takes its argument by reference**
  (`consumed_params` in `dz_cpp/src/lib.rs`). It qualifies when it owns memory and cannot
  be copied, is named once, by a `move` that every path reaches before any `return`.
  `Box(value: T)` moved its value three times (into the parameter, into the body's binding,
  into the allocation) and now moves it once. Only constructors: a function value or a
  trait method fixes its C++ signature. Measured by hand-edited C++ first: 5% on `expr`
  with clang++, 1.5% with g++.
- **Not done: building a union literal directly in its variant.** A constructor per
  variant, from its payload, measured within noise against the default, `become` and
  field-by-field assignment the literal does now: the C++ compilers already see through it.
- **A `HashMap` slot keeps a 32-bit hash, from 64** (`key_hash` folds the halves). It picks
  the home slot, tells empty from full and skips most key comparisons, and 32 bits do all
  three, so a slot is 4 bytes smaller.

**T-0164, measured.** Its guess, that a region keeps a growing map's old arrays, was wrong:
`tools/bench_regions.py` gives `astar` the same peak on the heap as in regions (19.3 and
19.4 MiB), and `words` a higher one on the heap (44 against 41). Instrumenting `astar` found
the cause in `HashMap`'s layout. Its largest search holds 110,069 entries in each map, and
both maps grow to 262,144 slots, 42% full, since a map doubles past three quarters. While
`came_from` grows, its old arrays are alive beside the new ones. With 20- and 24-byte slots
that came to about 14.6 MB of the 19.4 MiB peak, the runtime's baseline being 3.5. The
32-bit hash brings `astar` to 17.4 MiB and `words` to 40. What is left is the open-
addressing layout itself, and for `words` likely each short `String`'s own allocation,
which `std::string` keeps inline (T-0156); both are left in T-0164.

Against the C++ half, `bench_programs.py`, nine runs (`RESULTS.md`): `expr` 1.17× on
clang++ and 1.10× on g++, from 1.33× and 1.20× after T-0146; `astar` 0.81× and 0.95×;
`words` 0.78× and 0.87×. The machine was loaded, so the ratio within the run is the figure.

Tests: the crates' tests; the run suite on clang++ and g++ (all but `every_example_builds`,
whose graphical examples need X11, GL and ALSA headers this machine lacks); every run
program under ASan and UBSan. The HIR snapshot of `std` changed only in the hash's type.

## `sort_by` an introsort, and `String::push` inlined (T-0096; 2026-11-04)

Library work, no language change. `List::sort_by` was a heapsort, chosen for allocating
nothing and having no bad inputs, but it makes about twice quicksort's comparisons and
jumps around memory: about 100 ms of `words`, against 19 ms for `std::sort`. It is now an
introsort, as `std::sort` is: quicksort around a median of three, insertion sort at 16
elements or fewer, and the heapsort kept as the fallback once quicksort has gone
2·log2(n) splits deep, so the old guarantee holds. About 20 ms now. `exchange` is `swap`
without its checks, and returns at once for one index, since moving out of a slot and back
into it would read the slot after emptying it. `tests/run/sorting` gained larger inputs, and
gave the same output with the sort forced down each path alone.

A profile then had `String::push` called 14 million times on clang++, its path past ASCII
making it too large to inline; that path is `push_encoded` now. `bench_programs.py`, nine
alternating runs, Dazzle / C++: `words` 0.77× on clang++ and 0.97× on g++; `astar` 0.92× and
1.08×; `expr` 1.47× and 1.22×. Between runs on this machine a ratio moves by about a tenth.

## A one-word allocator handle (r132; D-0103; T-0228; 2026-11-14)

The user chose, of the three ways T-0228 laid out, to make every `^mut Allocator` one word,
not only the containers' field. Each implementation of `Allocator` keeps its vtable as a
hidden first member, `_dz_vt`, and `Allocator__dyn` is `ThinDyn`, the address alone, which
reads the vtable from the object. `std`'s source did not change: `String`, `List`,
`HashMap`, `Deque`, `Box` and `RcBlock` still hold a `^mut Allocator`, and every placed call
still passes one, each now a word.

- **The vtable is a `constexpr` variable**, `Heap__Allocator__vt`, and `dz_vtable` for a
  thin trait a `constexpr` function returning its address, declared before the struct so the
  member's default can name it. Statics are now emitted after the vtables: with the function
  defined above it, `inline Heap HEAP = Heap{}` is constant-initialised, as it was with no
  member to set, instead of falling to dynamic initialisation in whatever order C++ starts a
  program's statics, which a static built from the heap could have run before.
- **The handle is made from `&p->_dz_vt`**, not `p`, so through a derived type it is still
  the implementation's own address, and a type that implements no `Allocator` has no such
  member and does not compile.
- **The region's direct call (T-0151) compares `vt()`**, the allocator's first word, which
  the call touches next anyway.
- **`DZ0587`**: only a struct of the implementation's own module, not generic, with no base
  and no `virtual` method, may implement `Allocator`; an imported header's struct is
  refused too. Nothing in the repository implemented it otherwise.

Measured on every benchmark, the run before and after on the same machine in one sitting
(`RESULTS.md` is the after):

| Program | Compiler | Before | After | Peak memory before → after |
|---|---|---:|---:|---:|
| json | clang++ | 259 ms (1.15×) | 239 ms (1.07×) | 235.9 → 208.3 MiB |
| json | g++ | 278 ms (1.32×) | 262 ms (1.24×) | 234.0 → 209.9 MiB |
| words | clang++ | 139 ms (0.90×) | 136 ms (0.88×) | 37.8 → 33.5 MiB |
| words | g++ | 146 ms (0.88×) | 148 ms (0.89×) | 38.0 → 34.0 MiB |
| astar, expr, raytrace | both | | within 2 ms | unchanged |

`json`'s types are now the size of its C++ half's, `String` and `List` 32 bytes, `Json` 40
and a member 72 (`sizeof` in the generated C++), so the 27 MiB it still holds more is not
their size; T-0225 has the other suspect, what a region keeps of a list that grew, not yet
measured on its own.

## Moves that leave nothing to reset, and small `std` functions inlined (T-0225; 2026-11-14)

Compiler and library work, no language change. On `json`, from 1.25× its C++ half on
clang++ to 1.16×, and 1.42× to 1.31× on g++:

- **`move(x)` of a copyable local is a plain C++ move** when the reset §6.4 promises cannot
  be seen. The flow analysis refuses any use of a moved-from local and ties any reference or
  pointer to it, so only its drop meets it again, and the C++ move leaves a value harmless to
  destroy. `take` had built a default and assigned it: for a union, a switch and a drop on
  every `push(move(v))`. Guarded by `drops_silently` (no `drop` of the program's own
  anywhere in the value, which could print, and no base) and by the function making no
  pointer with an explicit cast, which ties do not follow. Parse 138 → 123 ms.
- **`let v = f() or return` names the value in the result** it was read out of, a
  temporary of the same block declared just before `v`, rather than moving it out: one move
  of a union fewer per element. Also only where `drops_silently`, since otherwise the
  moved-from value left behind would no longer be dropped. Parse 125 → 120 ms.
- **`let _ = move(x)` drops `x` there.** It was emitted as `(void)(mv(x))`, a cast that moves
  nothing, so a move-only `x` lived to the end of its scope; it now builds the value and
  lets it go. `tests/run/discarded_move`, which the old compiler got wrong.
- **`cut_bytes` inline**, its panic moved to `cut_bytes_failed`; `slice_bytes` and
  `is_utf8_boundary` `#[inline]`; `Formatter::write_char`'s non-ASCII path a call of its own
  so the ASCII one inlines; `String`'s `drop` `#[inline]`. About 10 ms together.

Tried and not kept: returning `f()`'s result whole from `return f() or return`, which C++'s
own elision had already covered. One cost, kept knowingly: `words` on g++ went from 141 to
148 ms (0.85× its C++ half to 0.89×). The plain moves shrank `std`'s `HashMap` and `List`
code enough that g++ now inlines the map's lookup, `get_mut_str`, into `words`'s loop, and
the loop is slower for it; nothing does more work, and clang++ did not move. What is left of `json`'s gap is mostly memory, its
containers' two-word allocator handle (T-0228, a design question).

## Floats written in full, and `loop` (r130, r131; D-0101, D-0102; 2026-11-14)

Two language changes the `json` benchmark led to, both the user's choice.

**`{}` on a float** (r130, D-0101, T-0223) writes the shortest digits that read back to the
value, in plain decimal, as Rust's `{}` does; it was `snprintf("%g")`, six significant
digits. The runtime's `format_float` gets the digits from `std::to_chars`'s scientific form
and lays them out itself: its fixed form writes a large whole number's exact digits
(`f64::MAX` as 309 of them), where Rust and D-0101 write the shortest digits and zeros, and
the standard breaks that tie the other way. Where the C++ library has no floating
`to_chars` (libc++ before it had `from_chars` too, as on macOS), `%.*e` at one more digit
each time until `strtod` reads it back gives the same digits; `DZ_FLOAT_FALLBACK` forces
that path, and `float_text_is_the_same_without_to_chars` holds it to
`tests/run/float_text`'s expected output. Checked beyond the corpus: the two paths agreed
on 200,000 random `f64` and `f32` bit patterns, and every `f64` matched Python's `repr`.
`Format for f32` passes the `f32` rather than widening it, `std`'s buffer is 336 bytes (an
`f64` in plain decimal can need 327), and `error(...)`'s messages format floats the same way.
`tests/run/formatting` and `math` print more digits, and so does one test in `run.rs`, where
an `f32` that is 1.9999999 had been rounded to `2`. `json` went from 1.40× its C++ half
to 1.25× on clang++, and from 1.56× to 1.42× on g++.

**`loop { … }`** (r131, D-0102, T-0224). The parser makes it a `while` whose condition is a
`true` spanning the keyword, with `is_loop` for the formatter and the dumps, so every walker
of a `while` walks it unchanged. The checker marks each loop a `break` leaves, labelled or
not (`LoopFrame::left`), and a `loop` none leaves is `forever` in the HIR, which definite
return treats as never falling through. `tests/flow/loop_never_ends.dz` and
`loop_left_by_break.dz` are the pair; `tests/run/looping` runs it; the lexer, parser and
formatter corpora have it, and the language server's keywords and the VS Code grammar know
it. Left for later: the warning on `while true` that suggests `loop` (T-0226, 0.3), and a
move on a path that ends in `return` counted when a loop body runs again (T-0227).

## A JSON reader and writer against C++ (T-0173; 2026-11-14)

A benchmark, with a compiler fix and two library changes it found; no language change.
`tests/bench_programs/json` generates 100,000 records as a JSON document, parses it into a
recursive union of `String`s and `List`s, turns the tree into a `List<Record>` by hand (a
`String` cannot be built at a raw address in safe code, so reflection does not read), and
writes the records back through a serializer written once against reflection's type table.
The C++ half has a `std::variant` tree and hand-written `from_json` and `to_json`. Both
agree on every byte written; its numbers have at most six digits, since `{}` on a float is
`%g` (T-0223).

- **The checker overflowed its stack** on a union holding a `List` of itself: `copyable`
  asked about `List<Json>`, whose `impl<T: Copy> Copy` asked about `Json` through
  `impl_applies` and `implements`, from an empty stack each time. The stack is now kept in
  a thread-local while an `impl Copy`'s bounds are checked, so the cycle is seen and
  answered as `copyable_guarded` answers cycles. `tests/run/recursive_union`.
- **`List`'s first allocation** was 8 elements whatever their size; it is now 8 bytes, 4
  elements up to a KiB, 1 beyond, as Rust's `Vec` does. `json` 376 → 329 ms and 316 → 237
  MiB; the other programs unchanged.
- **`parse_f64`** goes through `std::from_chars` where the C++ library has it for floats,
  falling back to `strtod` for anything it does not take, so no result changes. The parse
  160 → 142 ms.

Tried and not kept, since neither measured: `List::push` marked `#[inline]`, and
`Formatter::write_char`'s encoding moved out of its inline body. `json` is 1.40× its C++
half on clang++ and 1.56× on g++, from 1.70× and 1.91×; T-0225 has the profile of the rest,
most of it float text and a union moved through every `!->` return.

## A literal on the left takes the right's type, and a union's `switch` ends in `else` (T-0222; 2026-11-14)

Compiler work, no language change. With `perf` allowed, `raytrace`'s remaining 10% was
profiled, and then bounded: the generated `intersect`, `closest`, `sky` and `trace`
rewritten by hand as clean C++ on Dazzle's own types and operations ran 475 / 625 ms
(clang++ / g++) against 501 / 688 generated and 455 / 624 for the C++ half. So most of the
gap was the shape of the generated code, not the types, and T-0222 records which shapes
were tried one at a time; most interact, and two changes are clear:

- **A number on the left of an operator** stayed untyped while the operator became the
  right operand's type, then settled on `f64`: `0.5 * x` for an `f32` `x` reached C++ as
  `0.5 * x`, computed in `double` and narrowed, against spec §2.5. The checker now checks
  the right operand first when the left is a written-out number (`literal_on_left`, decided
  from the syntax so a long sum is not checked twice per level), for arithmetic, bitwise
  operators and comparisons, and lends its type only when it is a number. `std`'s
  `Random::unit_vec3` took its square root in `double` because of it; `hir_typed` shows 13
  literals retyped, all the right way. `tests/run/literal_left` pins it with values that
  round differently the two ways.
- **A union's `switch` with no `default` ends in `else`**: the checker proved the arms
  exhaustive and only building a union sets its tag, so testing the last variant gave the
  C++ compiler a branch that cannot be taken. Not an enum's, which C can give a value no
  variant names. `a_union_switch_ends_in_else` pins both. `raytrace` 498 → 486 ms on
  clang++ and 688 → 668 on g++.

`raytrace` is now 1.07× its C++ half on both compilers, from 1.10×; the other programs did
not move beyond their noise.

## A module's private functions are `static` (T-0221; 2026-11-14)

Compiler work, no language change. Every free function was declared in its module's header,
so had external linkage, `pub` or not. clang++ inlines a function with one caller when it
has internal linkage, as a C++ one in an anonymous namespace does, and otherwise judges it
by size alone; LTO did not make up for it. Now a free function only its module's `.cpp` can
call is declared at the top of that `.cpp` and defined there `[[maybe_unused]] static`.
`Emitter::file_local_fns` decides it from the module's own code alone, since an incremental
build re-emits only the modules an edit can reach (T-0197): not `pub`, generic, `extern`,
`#[inline]` or attributed, not `main`, and named only by bodies emitted into that `.cpp`,
which leaves out generics' bodies (instantiated where used), field defaults and `#[inline]`
bodies (in the header), default arguments (written at each call), and constants and
statics. A module declaring a macro is left alone, since an expansion elsewhere might name
one of its functions, and so is `std`, whose C++ every program shares. Methods cannot be
covered: a member of a class declared in the header has the class's linkage.

`raytrace` went from 1.25× its C++ half on clang++ to 1.10× (565 ms to 500, C++ 454); g++
was unmoved at 1.10×, since it keeps `intersect` out of line in both halves, and the other
three programs moved within their noise. One test searched the `.cpp` for `void grow(` and
now found the declaration first; it looks for the definition. What is left of the gap is
T-0222, where four more experiments are recorded, none of which explained it.

## A ray tracer against C++ (T-0174; 2026-11-14)

A benchmark, no language change. `tests/bench_programs/raytrace` is the tracer of
`examples/stb` without the PNG, twice: the Dazzle half on `std::math`'s `Vec3`, the C++ half
on a `Vec3` struct with operators, `std::variant` and `std::optional`. Its SplitMix64 replaces
`std`'s `Random`, and the checksum is over the image's bytes. The first program here that is
mostly floating point, and the first where both halves must compute the same floats: they
agree on every byte, with each expression written in the same order on both sides and both
built with `-ffp-contract=off`. That flag comes from the program's new `cxx_flags.txt`, which
`tools/bench_programs.py` and `whole_program_benchmarks_agree_with_their_cpp_halves` give both
halves, the Dazzle one through `DZ_CXXFLAGS`. The script also builds a Dazzle half once
uncounted before timing clean builds, so a program's own flags do not time a `std` compile.

It is 1.25× its C++ half on clang++ and 1.11× on g++. `perf` was not allowed on the machine,
so the gap was found by editing the generated C++, or the C++ half towards it, and timing
each. A switch case copying its payload, `Material` and all, cost 22 ms; that is the
language's rule (D-0043), so the benchmark borrows with `case Sphere(&s)`, as the C++ half's
`get_if` does. Most of the rest on clang++ is `intersect` not being inlined: the C++ half's
functions are in an anonymous namespace, and Dazzle's private ones have external linkage,
which LTO did not make up for (`static` in the generated code, 575 ms to 531). An empty
`Opt<Hit>` building a whole `Hit` costs g++ about 27 ms. Both are T-0221.

## A tree of `Box`es in a region skips its drop (T-0218; 2026-11-14)

Compiler work, no language change. `expr` put each statement's tree in the region of its
pass but dropped it node by node anyway: a union of `Box`es was never releasable, a type
reaching itself was answered "no" while being worked out, the parser's mutually recursive
functions never counted as building their results whole (a least fixpoint), and the tree
arrived through a tuple `let` with `or`, which was not followed. Now the memo and
`whole_functions` take the largest answer, unions and `Box`es of releasable types are
releasable, one walk (`Whole`) follows sites, literals, `or` and locals grown from
themselves, and a `Box(…)` is whole only when what it holds is. A value holding a union or a
`Box` skips its drop only in a local that is not `mut`. `expr` went from 1.10× its C++ half
to 1.05× on clang++ and 0.96× to 0.92× on g++; the rest of the gap is moved-from
destructors (T-0097).

## `Formatter` appends in place, and decimal digits divide by a constant (T-0217; 2026-11-14)

Library work, no language change. Timing `expr`'s phases against its C++ half showed its
generator at 41 ms against 29–31, the rest close. `Formatter::write_str` was one
out-of-line call per piece, `write_char` went through it with a 4-byte buffer, and
`write_digits` divided by a parameter, two hardware divisions a digit. `write_str` is now an
`#[inline]` append to the `String` when nothing else is asked for, `write_char` pushes an
ASCII byte, and base 10 has its own loop. The generator went to 33 ms on both compilers;
`expr` to 1.10× its C++ half on clang++ and 0.96× on g++, from 1.15× and 1.03×. The same
timing found what is most of the rest: each statement's tree is dropped node by node
although its region frees it (T-0218), worth 13 ms of the 72-ms parse-and-evaluate loop.

## A `for` range's end is evaluated once (T-0216, r128, D-0099; 2026-11-14)

A language change, the user's choice. `for i in a..b` was emitted as a C++ counting loop
whose condition re-evaluated `b` before every pass; now `a` and `b` are evaluated once,
before the first, as Rust's `for` does, the end held in a variable declared with the
counter. A literal or constant end is still written in place.

## A loop that kills nothing indexes its `List`s through a slice (T-0215; 2026-11-14)

Compiler work, no language change. After T-0214, writing through a `&mut List` in a loop was
still 7× a slice on clang++: after each store C++ could not prove the list's length and
array unchanged. The tie analysis can, so `dz_flow::views` picks, per loop, the lists it
indexes that nothing inside it may kill, and codegen builds a slice of each before the loop
and indexes that, every index still checked. `dst[i] = src[i] * 3` went from 41 µs to 6 µs
per 100 000 elements on clang++; g++, which vectorizes neither, is unchanged. A failing
index now panics as a slice's does, at the user's line. On the way: g++ stores a bounds
check's `SourceLocation` on every pass, and `for i in a..b` evaluates `b` on every pass,
which the spec does not settle; T-0215 records both.

## `#[inline]` reaches another module's generics, and `List` loops vectorize (T-0214; 2026-11-14)

Compiler and library work, no language change. The question was whether bounds checks cost
in loops; they do not. On clang++, a sum over a `List<i32>` took 22.7 µs per 100 000
elements indexed and 20.3 µs with `for &x`, against 2.6 µs over a slice whether checked or
`unsafe`. The emitter collected `#[inline]` from the module being emitted only, so `std`'s
never reached a `List<i32>` the program instantiates (T-0185 puts instance bodies in the
program's `.inst.cpp`), and link-time inlining came too late for the vectorizer. Now the set
is every module's; `List`'s accessors and the slice iterators are `#[inline]`; and a
constructor that only stores its parameters goes in the header, since `SliceIter`'s, out
of line, kept the iterator in memory. Both `List` loops now run at 2.6–3.2 µs. g++ 14
vectorizes none of them and did not change. The whole-program benchmarks did not move, none
walking a `List` in order in its hot path. Writing through `&mut List` in a loop is still 5×
slices on clang++; T-0214 says why and what would fix it.

## `List`'s index checks inlined (T-0157; 2026-11-04)

Library work, no language change. A gprof profile of `astar`'s release build showed that
link-time optimisation already inlined `Hash` and `Ord`, but not `List::index_mut` (100
million calls, 15%) or `List::swap` (32 million, 5%), called from the binary heap's sift
loops. Each formatted its out-of-bounds panic in its own body, so a one-compare accessor
was too large to inline. The panics moved into `list_index_out_of_bounds` and
`list_swap_out_of_bounds`, and the three accessors now inline. `bench_programs.py`, nine
alternating runs, Dazzle / C++: `astar` 0.84× on clang++ and 0.95× on g++, faster than its
C++ half for the first time; `words` 1.11× and 1.07×; `expr` 1.40× and 1.27×. The machine
was as noisy as the day before. `HashMap`'s regrowth was left: the C++ half regrows too, and
it is now about 2% of `astar`. Peak memory, 50% over C++ in `astar`, is T-0164.

## Fewer moves through `or return`, and a union's move without a reset (T-0097; 2026-11-04)

Compiler work, no language change. A gprof profile of `expr` had `Expr::become`, a union's
reset, at 27 million calls for 1.6 million tree nodes: each level of the recursive parser
moved its `Expr` four times, and every move reset its source.

- **`or` read in place** (`or_in_place` in `dz_cpp`): `x or return`, and any `or` whose
  clause is `return`, `break`, `continue`, a panic or `unreachable`, no longer builds a
  default value for the clause to fill. The value is moved straight out of the `Res` or
  `Opt`. `tests/run/optionals` prints two fewer `drop 0`s, the drops of those defaults.
- **`Res` by reference**: its constructor and `good` took the value by value, one move more
  per `return`.
- **A union's move leaves its source's variant** (`union_decl`): the payload's fields are
  each moved from, and so at their own defaults, and a union has no `drop` of its own
  (§7.5), so dropping the source later drops only defaults. A source a program can still
  reach is reset as §6.4 says, by `take` rather than a plain move (`moved_out`): a field, an
  element, anything through a pointer, a static, and the optional `or` and `if let`
  consume. A local is never read after its move (`DZ0401`), and a temporary never at all.
  `tests/run/unions` gained a union moved out of a field and out of a struct moved whole,
  each sound dropped once.

`become` fell to 14.7 million calls with the first two. Measured Dazzle against itself
(the generated C++ with and without the reset, eleven alternating runs), the third is 11%
on clang++ and 1% on g++. Against the C++ half, `bench_programs.py`, before and after all
three: `expr` 2.10× → 1.50× on clang++ and 1.66× → 1.31× on g++. The machine was noisy, the
C++ half itself ranging from 134 to 181 ms between runs, so the ratio within one run is the
figure to read.

What is left of T-0097: `Box(value: T)` moves its value twice, `Box` is 40 bytes (a
library decision), and a union literal is built as a default, then switched to its variant,
then filled field by field. A region for each statement's tree (T-0146) measured 5–7% on
`expr` by hand-edited C++.

## Inlining across modules: `#[inline]` (r106, T-0153) and LTO (T-0154; 2026-11-03)

A `gprof` profile of the three benchmarks, regions on, found allocation a small cost and
missed inlining a large one: each module is its own translation unit, so `std`'s small
functions were calls from the program however often they ran (`String::push` 14 million
times in `words`, `hash_combine` 11 million in `astar`), and `Allocators::Global()` ran 32
million times in `words`, the default a move resets a `String`'s allocator to. Two fixes,
each for what the other misses:

- **`#[inline]`** (spec §12.8, r106), described but refused until now: the body goes in the
  module's header, after everything else in it, and every caller can inline it. An
  associated constant's function always goes there. `std` marks `String`'s small methods,
  `hash_combine`, `string_is`, the text iterators' steps and `write_char`.
- **`-flto=auto`** on Dazzle's code in `--release` and `--shipping`, with one retry without
  it, and a note, when a toolchain cannot link it.

Measured against the compiler before both, fastest of twenty alternating runs: clang++
`astar` and `words` 18% faster and `expr` 8%; g++ `words` 13%, `astar` and `expr` 4–5%.
On g++ `#[inline]` gives most of it; on clang++, LTO. Debug builds (`-O0`) are unchanged.

## A build compiles only the C++ it emitted (T-0087; 2026-10-26)

`compile` used to pass every `.cpp` in `build/cpp/` to the C++ compiler. So a module that
had been renamed or removed left a stale `.cpp` behind, and that file failed with `DZ0803`
against a runtime it was not generated for. The error named source that no longer existed.
`examples/ownership` hit this after its `books` module went.

`BuildOutput` now carries `files`, every file `emit_cpp` emitted whether or not it was
rewritten, and `compile` takes its sources from that list. Nothing in the folder is deleted,
since a user may have put something there. `a_build_ignores_cpp_left_behind_by_a_module_that_is_gone`
plants a `.cpp` holding an `#error`, builds `hello`, and checks that the build succeeds and
the file is still there. Without the fix, the test fails with the original `DZ0803`.

## `--timings`, and where a build's time goes (2026-10-25)

The user asked for the benchmarks to show compile times, broken down by phase. Nothing in
`dz` measured a phase, so `check`, `cpp`, `build` and `run` now take `--timings`, which
prints each phase and its time on standard error once the command is done (spec §17.3).

- `dz_driver::Timings` is a list of named durations, filled by `analyse_prepared`:
  manifests, C and C++ headers, CMake builds (only when there was one), lex and parse, `#if`
  and macros, resolve names, type check, ownership, and instantiate generics. `dz` adds
  emit C++, C and C++ libraries, and the C++ compiler. Nothing reads a duration back, so
  every phase is still a pure function of its input (invariant 3).
- Header import and CMake happen inside `dz_project::load_for`, which cannot depend on the
  driver, so the workspace carries `LoadTimes`, and the driver subtracts them from loading.
- A test (`timings_name_every_phase_on_standard_error`) pins the phase names, since
  `tools/bench_programs.py` reads them. The runner now builds each half three times from
  clean, takes the medians, and reports Dazzle's own phases, the header import and the C++
  compiler separately.

**What it showed**, on the three benchmark programs at `-O2` with g++ 16: Dazzle's own
phases take about 35 ms, and importing `stdlib.h` with libclang takes about 380 ms. The
standard library imports it for `Allocators::Global`, so every program pays that. The C++
compiler on the generated code takes about 1.4–2.4 s, against 0.3–0.5 s for the same
program written in C++, because the standard library's C++ is compiled again every time.
Both are filed: T-0101 (cache imported headers) and T-0102 (stop recompiling the standard
library).

## Whole-program benchmarks against C++ (T-0090; 2026-10-24)

`tests/bench` compares one construct at a time. The website wanted a number for a whole
program, so `tests/bench_programs` has three, each written idiomatically twice: `astar` (A*
over a grid with a generic heap and struct-keyed maps), `expr` (generate, tokenize, parse into
`Box`ed unions, evaluate, with errors through `!->`) and `words` (a word count over generated
text, then a sort). `tests/bench_programs/README.md` has the rules.

- **Same compiler, same flags, same result.** For each compiler, the Dazzle half is `dz build
  --release --cxx X`, and the C++ half is `X -std=c++17 -O2 -fno-exceptions -fno-rtti`.
  Dazzle's code also gets `-fwrapv`, the cost of its wrapping arithmetic, which the C++ does
  not get. Input comes from SplitMix64, written out on both sides. Every program prints a
  checksum over its whole result, and any order that could tie is made total, since the two
  sides' heaps and sorts are different algorithms.
- **`tools/bench_programs.py`** builds from clean, times `dz cpp` separately as the front end,
  runs the halves alternately, takes the median of ten, and refuses to report a pair whose
  checksums differ. It writes Markdown (`RESULTS.md`) and JSON, with the CPU, the compilers'
  versions, the commit and the load average before and after. Runs go through
  `measure.cpp`, a fork/exec/`wait4` helper, because a process started from Python reports
  the interpreter's ~18 MiB as its own peak until it calls exec.
- **A test**, `whole_program_benchmarks_agree_with_their_cpp_halves`, builds every pair at
  `-O0`/`-O1` on the input in `quick_args.txt` and requires the same checksum.

**The temporary arena.** The Dazzle halves first allocated everything from the global heap.
The user pointed out that a Dazzle program would put scratch memory in `Allocators::Temp`,
so each program was measured both ways, interleaved in one sitting. `expr`, with each
statement's tree in the arena and a reset before the next, was 5–11% faster. `words`, with
the whole count in it, was 6–7% faster, but peaked at 71 MiB instead of 44, because what a
growing text or table leaves behind is not reused. `astar`, with each search's heap and
maps in it and a reset after the search, was 4–5% *slower* and 30% larger. Its maps grow
past the arena's 64 KiB chunk, so every resize became a `malloc` of its own (T-0099). It stays on the
global heap, with a comment saying why. The C++ halves use the default allocator;
`std::pmr::monotonic_buffer_resource` would be the matching strategy, and is not used.

**The published run:** Dazzle at 1.3–1.45× C++ with g++ and 1.55–2.1× with clang. Build time
is 1.8–3 s against 0.3–0.5 s, of which the front end is about 0.5 s. Phase timings, taken with
`Instant` in throwaway copies, located most of the gap. Neither `perf` nor valgrind works
on this machine; valgrind cannot decode its glibc's AVX-512.

- `words`: `List::sort_by` takes 100 ms where `std::sort` takes 19 (T-0096).
- `expr`: parsing takes 120 ms where C++ parses and evaluates in about 80. Each `!->` step
  moves a union twice through a default-constructed temporary (T-0097).
- `astar`: not broken down yet.

**Three codegen bugs the programs found, fixed** (each with a case in `tests/run`):

- A `const` array built from casts, `const OPS: [2]u8 = ['+' as u8, '-' as u8]`, failed to
  compile. The C++ is `constexpr`, and `dz::cast` was not. It is now (`tests/run/statics`).
- `let (name, tree) = parser.statement() or { … }` copied each element out of the tuple
  temporary, and `Expr` is move-only, so the C++ did not compile. An element that owns
  anything is now moved out (`tests/run/failure`).
- `let tokens = tokenize(text) or { … }` **copied the whole list twice**: out of the result,
  then into `tokens`. `List<Token>` is copyable because `Token` is, and both lowerings moved
  only what could not be copied. Both sources are compiler temporaries that are never read
  again, so anything that owns something is now moved out of them (`copy_is_free` in
  `dz_cpp`). `expr`'s peak memory went from 298 MiB to 140, below C++'s 151, and its g++ time
  fell from 402 ms to 310.

Writing them also turned up some friction, not filed as tasks: `while true { … return }`
does not count as returning on every path (DZ0405); a moved-out local must be returned as
`move(x)` rather than `x`; and `if let (a, b) = opt` does not parse. A `str` cannot be cut
safely at all, which is T-0098.

## `--no-line-directives`, and `dz` as WebAssembly for the website's playground (2026-10-24)

The website gained a playground: Dazzle on one side, the C++ it compiles to on the other,
regenerated as you type. It needed two things from the compiler.

- **`--no-line-directives`**, which spec §16.3 had promised and nothing implemented. `dz
  run`, `build`, `cpp`, `test` and `bench` take it; `dz_cpp::emit` now takes an `Options`
  (the `Mode` plus `line_directives`). Every place a directive would go is still tracked, so
  the output is the default output with its `#line`s removed and nothing else — a test
  (`no_line_directives_leaves_out_the_directives_and_nothing_else`) holds every generated
  file, `std`'s included, to exactly that. `#location` and panic locations are unaffected:
  they are rendered from spans, never from C++'s `__LINE__`. Without directives nothing
  names the copies of the standard library's sources written beside the C++, so they are
  not written.
- **`dz` builds for `wasm32-wasip1`.** The browser runs the real CLI over an in-memory
  filesystem (a WASI shim), so the manifest, the analysis and the emitter are exactly
  `dz cpp`'s. Two things stood in the way. libclang cannot be loaded in WebAssembly, so the
  libclang half of `dz_cppimport` moved to `clang.rs`, built only off-wasm, and `clang-sys`
  became a target-specific dependency. But `std` itself imports `stdlib.h` for
  `Allocators::Global` (spec §14.2), so a WebAssembly build carries that one header in
  `builtin.rs`: `malloc` and `free`, the declarations `std` calls, under a pseudo-path whose
  text `dz_cppimport::read_file` serves. The generated C++ is the same either way, since it
  includes the real `<stdlib.h>`. Any other `import cpp` reports that this build has no
  libclang. A test holds the carried declarations to what libclang reads from the real
  header, parameter names aside (`__size`, `_Size`), so the two cannot drift. The other
  obstacle was `std::env::temp_dir()`, which panics on WASI; `dz_project` used it only as
  the directory `std`'s header is imported from, which the carried header never reads.
- CI builds `dz` for `wasm32-wasip1` on Linux, so a dependency that cannot target it fails
  there rather than in the website's deploy.

Under Node's WASI on Windows, a glob in `sources` matched nothing (directory listing failed)
while literal paths worked. The playground names `src/main.dz` literally, so it does not
depend on listing directories.

The shared C++ cache (T-0182) used process IDs to name its temporary files. WASI builds
now use randomly seeded names for the same atomic writes. The website runs
`dz cpp --no-line-directives` under WASI during its checks to exercise the same code
generation path as the playground.

## Raised on 2026-09-12

Struct inheritance is done (r30). What follows is the tooling the language needs before it
can be worked in seriously, raised by the user on 2026-09-12 and sized here against what
the compiler actually does today.

### Already there, and what was added

- **Build modes were already in the manifest** — `[build] optimize = "debug" | "release" |
  "shipping"`, reaching the C++ compiler as `-O0 -g` / `-O2` / `-O3 -DNDEBUG`. What was
  missing was a way to choose one without editing the manifest, so `dz run`, `dz build` and
  `dz cpp` now take `--debug` / `--release` / `--shipping`. The override applies to **every
  module**, not just the root: one command builds one program, and a dependency left at a
  different optimisation level is the classic way to get a binary that matches neither
  setting. **Done.**
- **Submodules already work, in exactly the shape asked about.** A package with a `libs/`
  folder of Dazzle modules is `[dependencies] engine = { path = "libs/engine" }`, with the
  dependency graph, cycle detection and per-module visibility already built on it —
  `tests/workspace/good` *is* a `game` with `libs/engine` and `libs/math`. Include dirs,
  defines and `[cpp.libs]` recipes are inherited through those edges too (r19), so a Dazzle
  module wrapping a C++ library passes its build settings to whatever depends on it.
  **Nothing to do.**

### M14 — `dz test` ✅ done

`#[test] fn name() { … }`, written beside the code it tests. It passes by returning and
fails by panicking, which needed no new machinery: `panic` already aborts with a location
(§9.8).

- **One process per test.** A panic aborts and there are no exceptions to catch (§16.5), so
  a runner that called every test in one process would stop at the first failure and report
  nothing about the rest. The generated entry point therefore runs *one* test per
  invocation, named on the command line, and lists them with `--list`; `dz test` lists,
  then spawns once per name. It costs a process per test and buys a complete report — and
  isolates a test that corrupts memory from the ones after it.
- **A normal build leaves the tests out entirely**, which is the half that is easy to lose:
  `dz_cpp::Mode::Program` skips a `#[test]` item, `Mode::Tests` emits it and generates the
  runner instead of calling the program's `main`. Both halves are asserted — the fixture's
  `expected.txt` covers the program side and `dz_test_runs_the_projects_tests` greps the
  generated C++ for a test's name.
- The test binary builds into `build/test/`, so `dz test` never overwrites the program's
  objects and a later `dz run` does not relink a test entry point.
- `dz test [path] [filter]` filters by substring; the build-mode flags work here too, so
  `dz test . --release` is how you check that a release build still passes.
- **Where tests live was a decision, and a deliberately reversible one.** Beside the code
  needs no manifest change — `sources` already globs `src/**` — and adding a `tests/`
  source root later is purely additive rather than a reversal.

The one thing still missing is `assert`, which is typed but not emitted (a deferral-register row, M12);
until then a test says what went wrong with `panic`, which is what the runner reports.


1. `#[test]` on a `fn` with no parameters and no return, checked as one: a test that takes
   an argument or returns a value is an error at the attribute, not at the call.
2. Tests are **excluded from a normal build** — `dz build` must not link them — and are the
   only entry points `dz test` generates a `main` for. That means a second codegen
   configuration, which is also what M15 needs.
3. A failure is a `panic` (§9.8), which already aborts with a location, so the first
   version needs no assertion machinery beyond `assert` becoming real (a deferral-register row, M12).
4. Reporting: name, pass/fail, counts, non-zero exit. Running each test in its own process
   is what makes a panic recoverable; doing it in-process is faster and cannot survive the
   first abort. Start with one process per binary and a fail-fast report, since that is
   what a compiler's own test suite needs first.

**Open question for the user:** where do tests live? Beside the code in the same file is
simplest and keeps a test next to what it tests; a `tests/` directory per module matches
the way this repo's own corpora are organised. It changes what `sources` means, so it is a
manifest decision rather than a compiler one.

### M15 — `dz bench`, and what the guarantees cost ✅ done

`#[bench] fn name() { … }`, timed by `dz bench`. The same shape as `#[test]` and sharing
its machinery: `dz_cpp::Mode` picks which attribute a build keeps, and a program keeps
neither.

**What makes a measurement trustworthy, and what was needed for each:**

- **`--release` by default.** Timing `-O0` measures a build nobody ships. `dz bench
  --debug` is still there for anyone who wants to know what the debug build costs.
- **A calibrated iteration count.** Timing a cheap body once measures the clock, so the
  count doubles until a batch lasts 50ms.
- **The fastest of five batches**, not the mean. A machine running other work perturbs a
  batch upward and never downward, so the minimum is the closest thing to the body's own
  cost that a shared machine can show. This was not a theoretical worry: before it, one
  pair reported 550ns and 1171ns for the same work on consecutive runs.
- **`blackbox`, a new builtin** (§21 r31). A benchmark has two ways to measure nothing —
  its result is never read, so the work computing it may be deleted; its input is usually
  a literal, so the answer may be folded at compile time. A *void* barrier only fixes the
  first, and the first version of this was exactly that: it reported 5.40ns and 5.39ns for
  loops of 1000 and 10 iterations. Returning the value fixes both.
- **A monotonic clock** in the shim, `now_nanos`, defined in the `.cpp` so `<chrono>`
  reaches one translation unit rather than every build.

### The comparison suite, and what it says

`tests/bench` is the point of the milestone. Each benchmark is a **pair** — one Dazzle,
one calling hand-written C++ vendored in `vendor/baseline` — compiled by `dz` at the same
optimisation level into the same binary and timed by the same clock. `main` prints each
pair's two results side by side, and `the_benchmark_suite_agrees_with_its_cpp_baseline`
asserts they match: two benchmarks computing different things measure nothing.

Getting this honest took two corrections, both worth recording because both produced
confident wrong answers first:

1. **The baseline needed the same barrier.** With `blackbox` inside the Dazzle loop and
   nothing inside the C++ one, the compiler closed the C++ loop into a formula and reported
   3.38ns for 1024 iterations — physically impossible, and it would have been written up as
   "RAII is 166× slower in Dazzle". `barrier()` in `baseline.cpp` matches `blackbox`.
2. **The length had to be opaque on both sides.** The Dazzle half read a `const`, so it
   could specialise on a length the C++ half received at run time. Both now take it through
   `blackbox`.

With those fixed, the three pairs land **within a few percent of each other**: a `drop`
costs what a C++ destructor costs, a `virtual` call costs what a C++ virtual call costs,
and bounds-checked indexing costs no more than unchecked C++ on this machine — the check
*is* emitted (`::dz::index` is there in the generated code), the compiler simply makes it
free in this loop. Those are results from one machine and one compiler; the suite exists so
the claim can be re-checked rather than believed.

**Still owed:** a generic-instantiation pair, and somewhere to track the numbers over time
so a regression is visible. A single run is a snapshot, not a baseline.

## CI — GitHub Actions across Linux, macOS and Windows ✅ running

`.github/workflows/ci.yml`: build, clippy with `-D warnings`, the test suite, and `dz test`
and `dz bench` driven as a user would meet them, on `ubuntu-latest`, `macos-latest` and
`windows-latest`. `fail-fast` is off, because one platform failing is a fact worth having
about the other two.

**It has found eight bugs across its first runs, three of them in the compiler rather
than in the workflow** — every one of them invisible on Windows, which is what the matrix was for.

| Where | What | Why it only showed up there |
|---|---|---|
| shim | **`println("{}", xs.len)` did not compile on macOS.** `write_value` was overloaded on `int32_t`, `uint64_t` and the rest, which are *aliases*: on macOS `size_t` is `unsigned long` while `uint64_t` is `unsigned long long`, so a `usize` matched no overload exactly and the call was ambiguous | On Windows and Linux x86-64 `size_t` happens to alias one of the fixed-width names, so the set covered it by luck |
| importer | **No `import cpp` worked on macOS**: "'assert.h' file not found". The C and C++ standard headers live inside the active SDK there, and libclang does not run `xcrun` to find it the way the `clang` driver does | Only macOS puts the standard headers behind an SDK path |
| tests | **`#panic` fixtures failed on Windows.** The marker was stripped with `strip_prefix("#panic\n")` *before* normalising, and git checks `expected.txt` out with CRLF on Windows — so the marker stayed in the expected text and the test looked for the literal "#panic" in the program's own stderr | `.gitattributes` forced LF for `.dz` and `.snap` but not `expected.txt` |
| tests | **A snapshot pinned the operating system's wording.** `cannot read …: The system cannot find the path specified. (os error 3)` on Windows is `No such file or directory (os error 2)` elsewhere, so the snapshot failed everywhere it had not been recorded | The message comes from the OS, not from Dazzle |
| tests | **The compiler-precedence test passed vacuously.** `DZ_CXX` sits above the manifest in the order it checks, and CI sets it for the whole job — so the manifest levels were unobservable | It passed locally only because that machine had no `DZ_CXX` set |
| recipe | **raylib did not build on Linux or macOS.** On Linux GLFW refuses to guess a backend and stops at `#error "Cannot disable Wayland and X11 at the same time"` — it needed `_GLFW_X11` in `defines`. On macOS `rglfw.c` includes GLFW's Cocoa backend, five Objective-C `.m` files, so the translation unit has to be compiled as Objective-C despite its `.c` name | Windows selects its backend from the platform macros and has no Objective-C |
| layout | **`dz bench` could not run on Linux or macOS**: "cannot create `build/bench/cpp`: Not a directory". The built program sat in `build/`, so a module named `bench` *was* the file `build/bench` — the very directory the harness needed. The executable moved to `build/bin/` (§21 r35), leaving `build/` to directories alone | Windows names it `bench.exe`, so the two never met |
| workflow | **libclang was never found on Linux**, so every interop test reported `DZ0703` and *skipped*. `llvm-config --libdir` produced an empty string: Ubuntu installs it versioned (`llvm-config-18`) and the bare name is absent | — |

raylib needed a new recipe key to be buildable at all on macOS: **`flags`** (§21 r34),
raw compiler flags for a library's own sources. Nothing a `define` can say makes a `.c`
file compile as Objective-C, and raylib's own Makefile passes the same `-x objective-c`.

The other fixes: `write_value` and `write_hex` are now overloaded on C++'s ten **fundamental**
integer types, which are always ten distinct types and which every `intN_t`/`size_t`/
`ptrdiff_t` aliases exactly one of — so the set covers all of them on every platform, and
needs no `PRId32`-style macros to do it. The importer passes `-isysroot` from `xcrun` on
macOS. The run-test normalises before stripping, and `expected.txt` is `eol=lf` in
`.gitattributes`. The precedence test clears `DZ_CXX`, and the resolve snapshot redacts the OS's own
message — which stays in the *diagnostic*, where "permission denied" against "not
found" is something a user needs to be told. CI finds libclang by locating the
library rather than asking `llvm-config`, and **fails the job** when it cannot — a skipped
interop suite reads as a pass, which is the worst way for this to go wrong.

What writing it turned up, all of it fixed here rather than left for CI to discover:

- **`tests/run/cinterop_rgfw` could only ever link on Windows.** Its recipe listed
  `opengl32` and `gdi32` as plain `link_libs`, which apply on every platform; nothing
  else has those libraries. They are now in a `target.windows` section, with the X11 and
  Cocoa equivalents beside them.
- **macOS could not have linked either fixture**, because a macOS system library is a
  *framework* and `link_libs` becomes `-lNAME`, which finds nothing. Hence **`frameworks`**
  in a recipe (§15.1), linked as `-framework NAME`. `raylib`'s and `rgfw`'s macOS sections
  now name the ones GLFW's Cocoa backend needs; without them the link fails with undefined
  Objective-C symbols, which name nothing that looks like a missing framework.
- **No `cargo fmt --check` step**, deliberately. This repository is hand-formatted to a
  wider style than rustfmt's default: running it rewrites nearly every file, including ones
  untouched for months. Adopting rustfmt needs a `rustfmt.toml` and a one-time reformat,
  which is a decision rather than a CI step.
- The suite was run against **both** `clang++` and `g++` locally and passes on each, which
  is invariant 7's "more than one compiler" checked rather than assumed. The Linux job
  repeats the suite under `clang++` for the same reason.

Still open:

- **libclang parses with its own default target** (a deferral-register row): on Windows that is MSVC while
  `g++` builds against mingw. CI running both compilers is what should surface it.
- **Lints run on a pinned toolchain.** `-D warnings` against a floating `stable` means any
  new lint Rust ships turns CI red through no change of ours, which is what happened on the
  very first run: 1.98 added `question_mark`. `LINT_TOOLCHAIN` is bumped deliberately.

## The suite runs on every core

`cargo test` parallelises **across** test functions, never inside one, so a test that is a
loop over thousands of cases pins a single core however many the machine has. Four tests
were that shape and were three quarters of the wall time: `tolerant.rs` (~7,200 analyses),
`dz/tests/run.rs` (every project built and run, then 25 fixtures checked warning-clean),
`dz_driver/tests/matrix.rs` (232 placements) and `dz/tests/matrix.rs`. They now spread
their own work over the cores through `dz_driver/tests/parallel/mod.rs`, a work-stealing
`thread::scope` shared by `#[path]` as `feature_matrix` is. A full `cargo test --workspace`
went from **324 s to 77 s** on a 20-core machine.

This is sound because every phase is a pure function of its input (invariant 3) and the two
process-wide caches — `std_tree`'s parsed trees and `dz_cppimport`'s header cache — were
already behind a `OnceLock` and a `Mutex`.

It also exposed a race that had been latent: `hello_world_runs`, `generated_cpp_snapshot`
and `generated_cpp_carries_line_directives_back_to_dazzle` all drove `tests/run/hello/build/`
directly, which `every_program_produces_its_expected_output` builds too — and those already
ran on separate harness threads. Serial timing just made the window rare. They take scratch
copies now, for the reason `copy_fixture` had documented all along. **A test that builds a
fixture must copy it first**; the shared directory is the failure mode, not the parallelism.

## Diagnostics a program can read, and act on (T-0060, T-0062; 2026-10-22)

The first two `llm-tooling` rows, chosen by the user with the r70–r72 batch.

- **`dz check --format=json`** (`dz_diag::render_json`): one JSON document on stdout, a
  diagnostic to a line inside it, with code, severity, message, the primary file/line/column
  flat, every label, notes and helps split, and the fix. Each span is given as byte offsets
  into the file as it is on disk, which is what an edit is applied with, and as 1-based line
  and column in characters. Written by hand — `dz_diag` has no serde — and checked by parsing
  it with `serde_json` in `crates/dz/tests/fix.rs`.
- **Certain fixes and suggestions.** `Fix` gained `certain`. A certain fix resolves the
  diagnostic and nothing else a reader would need to check; a suggestion is a guess an editor
  offers by its title. `Diagnostic::did_you_mean` turns every "did you mean `x`?" in the
  resolver and checker into a suggestion replacing the misspelt name (twelve sites); certain
  fixes were added for an unused variable (`_` in front), `mut fn` and `#println(…)`, beside
  the two that existed (missing switch arms, missing trait members). The parser attaches a
  fix only to tokens of its own file, and `dz_macro::annotate` drops any fix whose edit
  touches a macro invocation's name — the span every token a macro wrote carries.
- **`dz fix [--dry-run]`** applies the certain fixes: per file, last edit first, a fix
  whose edits clash with one already taken skipped whole, never a file not on disk (the
  standard library). It says to run `dz check` again, since a fix can let a later phase run.
- **`dz explain <code>`** (`dz_diag::explain`): the rule is the code's doc comment in
  `codes.rs`, read from the file as compiled; the why and the examples are in
  `crates/dz_diag/explain/*.md`, a `## DZNNNN` section per code with a wrong and a right
  program. `dz_driver/tests/explain.rs` requires a section for every code and compiles every
  example — the wrong one must report exactly its code, the right one nothing — so the text
  cannot drift. Codes no single-file program can produce (manifests, encodings, C++ libraries,
  compiler bugs) carry a `No example:` line instead. Writing them found four compiler bugs,
  listed under r70–r72 in `m12.md`.

## Getting the repository ready to be public (2026-10-22)

The repository moved to the `dazzle-lang` organisation, ahead of being made public.

- **raylib and RGFW are git submodules** in `examples/vendor/`, no longer 527 files of
  raylib copied into `examples/raylib/vendor`. raylib is pinned to its `6.0` tag: the copy
  matched 6.0 except for a newer RGFW backend, which the GLFW build never compiles. RGFW is
  pinned to `d7eb573`, the upstream commit whose `RGFW.h` was byte-identical to the copy.
  `examples/raylib` and `examples/breakout` share the one raylib. CI checks out submodules,
  and `every_example_builds` names `git submodule update --init` when they are missing,
  rather than failing deep inside the recipe. The copies remain in git history. A recipe
  that fetches its own source, which would retire the submodules, is now part of T-0077.
- **The examples were cleaned up.** `raylib` had leftover `List` pushes and an unused
  static. `rgfw` called `RGFW_window_close`, which frees the window, inside the event loop
  and then again after it, so pressing a key was a use-after-free. It now asks the window to
  close and frees it once with `defer`. `books` reimplemented `Buffer` and `List` over
  movies, and is now `ownership`, the README's companion: a growable `Shelf<T>` with `own`,
  a constructor, `drop`, `copy`, growth through `move_out`, a copy and a move, and no leaks
  under `--track-memory`. Breakout's rules moved from `tests/run/breakout_sim/game` to
  `examples/breakout/game`, so an example no longer depends on a test fixture;
  `breakout_sim` points there instead. Every example is now `dz fmt`-clean, and CI checks
  that. Formatting them found T-0081 and T-0082.
- **README**: new sections on modules and imports, and on `own` and the three special
  members with the C++ they become, taken from `dz cpp`'s real output. The stale revision
  number and the "planned" `dz fix`, `dz explain` and `--format=json`, all shipped, are
  gone. Added `LICENSE` (MIT, previously declared only in `Cargo.toml`) and `CONTRIBUTING.md`.

## Field alignment counts the visibility (T-0081; 2026-10-23)

`dz fmt` aligns a run of fields on the width of the longest name, but measured the name
without its `pub`, `protected` or `pub(module)`, so a struct mixing visibilities came out
ragged. It now measures and pads everything before the `:`: visibility, `own` and the name.
`examples/ownership` and `tests/run/ownership` re-format. Four fixtures that had been aligned
by hand the new way were already right. The standard library is not `dz fmt`-clean for other
reasons and was not re-formatted here.

## CI runs once per change (2026-10-23)

CI ran on every pull request and again on the merge into master, and pull requests here are
merged as soon as they open, so every change paid for two runs of up to half an hour and the
PR run was usually cancelled. It now runs only on a push to master, plus by hand
(`gh workflow run CI --ref <branch>`) to try a branch first. A push that touches only the
root Markdown files, `docs/`, `editors/`, `tools/`, `.claude/` or the logo skips it (not
every `.md`: `dz explain` compiles in `crates/dz_diag/explain/*.md`), and a newer push to the
same ref cancels the run still going.

## CI on the pull request again, and linear history (2026-10-23)

The repository now merges pull requests only by rebase, deletes the branch once merged, and
commits carry no per-commit AI credit (the README discloses it once). With rebase-merge, a
PR's commits land on master as new commits whose committer is GitHub, so CI runs on the pull
request again and the job skips a push to master that GitHub committed: each change is still
tested once, on the PR (as merged into master), or on master for a commit pushed there
directly. Both triggers share the same paths-ignore.

## Trailing comments stay where they were written (T-0082; 2026-10-23)

`dz fmt` placed a trailing comment only after the nodes that asked for one — a statement, a
field, a variant — so a comment after a block's `{` went to the next line inside the block,
and one after a `}` to a line of its own. Now any line comment written after code goes at
the end of the last line printed, which is that code: the head of a `fn`, `struct`, `impl`,
`while`, `for` or `#if`, any closing `}`. Blocks inside expressions (`if`, `else`, `case`,
`unsafe`, an `or { e -> }`) are printed by a sub-printer that now starts on the `{`, so the
same rule covers them; `switch x {` takes its comment explicitly, or the first `case` would.
A `fn` whose body is only a comment no longer prints as `{}` with the comment after it.

Trailing comments also keep their column instead of collapsing to one space. Each is
written as a placeholder carrying the column it was written at, and a last pass over the
finished text places it: at that column, counted from its line's indentation so it moves
with a re-indented line, never closer than one space to the code, and with adjacent lines
whose comments were written at one column kept aligned at the column the widest needs. The
placeholder is a private-use character absent from the source, so it can never be mistaken
for one written there. The README's samples, aligned by hand, format unchanged; nothing in
`examples/`, `tests/` or the standard library formats differently.

## CI in a third of the time, and the standard library checked once (2026-10-23)

The Linux job took 29 minutes. Per-step timings put it in three places: `apt-get install`,
7 of them on a day the Ubuntu mirror served `libclang-dev`'s 29 MB at 70 kB/s; the test
suite at 10; and the whole suite again against clang++, another 10. Within the suite,
`dz_driver/tests/tolerant.rs` was six and a half minutes of each pass. Now:

- **Tests build at opt-level 1 in CI** (`CARGO_PROFILE_{DEV,TEST}_OPT_LEVEL`, in the
  workflow and not `Cargo.toml`, so local rebuilds stay fast). Debug assertions and overflow
  checks are separate settings and stay on. `tolerant.rs` went from 293 s to 96 s on an
  8-core machine for 36 s more of build.
- **The clang++ pass runs only `dz/tests/run.rs`**, the one test that reads `DZ_CXX`;
  `matrix.rs` already tries every compiler present, and nothing else compiles C++.
- **`libclang-dev` is installed only when the runner has no libclang**: clang-sys loads
  `libclang.so.1`, which the image's LLVM ships, and the headers and static archives on top
  are what made the download slow.

**The standard library is checked once per thread rather than once per analysis.** With
its trees already cached, checking its bodies was still about 20 of the 44 ms an analysis of
`hello` took — and the editor analyses on every pause in typing. The checker now works in two
layers: the prelude's (`std` and the headers it imports), then everything else, each pass
running over a whole layer before the next starts. Nothing in the prelude's layer can see
past it, so its checker state — the type table, its HIR, the parameter counter, the `impl`
bookkeeping — is a function of the standard library alone, and `dz_check::PreludeLayer`
captures it. `dz_driver` keeps one per thread (the types share names through `Rc`) keyed on
what can change it: the `#if` flags on for `std` (none when a parse error left every `#if`
in place), the `Arc` of each header it imports, and the root's `type_info`, the one root
setting the checker reads. An analysis that matches starts from a copy and checks only the
rest.

That needed the layer's ids to be the same in every workspace, and they were not:
- `libc`, the module `std` imports `stdlib.h` into, was created after every project module,
  so its module id and symbol ids depended on the program. `dz_project::load` now imports
  the prelude's headers straight after adding the prelude, so they are modules 1.. and their
  files follow `std`'s. **`std`'s own `stdlib.h` is therefore read with C++17**, the oldest
  standard the generated code compiles as, rather than the root manifest's `cpp_standard`:
  anything declared there is declared under any later standard too.
- `std`'s 75 files are added to the source map first, before any manifest
  (`Module::embedded_files`), instead of after the manifests of however many modules the
  project has.
- The checker numbered a method's generic parameters after every module's type parameters;
  checking by layer numbers `std`'s first.

`flatten_bases`, `attach_special_impls` and `collect_foreign` each walked the whole
workspace; each now does only its layer, since flattening a type twice gives it its base's
fields twice. Impl overlap is checked only against earlier impls, and `std`'s come first, so
layering changes nothing there. A layer is used only if its symbol count matches the
resolution it is given.

`dz_driver/tests/prelude_cache.rs` holds reuse to being invisible: a program analysed after a
different one must give exactly the diagnostics and HIR dump it gives checked afresh, in both
modes, and a broken program and a clean one never share a layer. It caught a race on its
first run: two threads importing one header for the first time both parsed it, and the
second replaced the first's cache entry, so the first thread's `Arc` never matched again.
`dz_cppimport::import` now keeps an entry another thread made meanwhile, so an unchanged
header is one `Arc` per process — which `Workspace::add_foreign`'s sharing relied on already.

`hello` now analyses in about 29 ms steady, from 44; copying the layer is 3.5 ms of that.
`tolerant.rs` takes 132 s in a debug build (from 293) and 55 s at opt-level 1 (from 96).

## Each generated file its own cached object (T-0102; 2026-11-09)

`dz build` compiled every generated `.cpp` in one command, so `std.cpp` (0.4 MB for
`words`) was compiled again after any edit. Now each file becomes its own object in
`build/obj/dz/`. The objects compile in parallel, are skipped while their inputs are
unchanged, and a separate command links them. The up-to-date check is the vendored
libraries' own (`native::up_to_date`, `write_stamp`): the content of the source and of
every header the depfile names, with the compiler and every flag in the object's name. That
is why the release build's `-flto=auto` and its fallback without it get separate objects.

On `words` (clang++ 21 on Windows, debug), the C++ side of a one-line change went from
1.9 s to 0.26 s of compiling and 0.1 s of linking. A clean build went from 1.9 s to 0.66 s,
since the files now compile at once. `--timings` reports `link` as a phase of its own, and
`tools/bench_programs.py` adds it back into its C++ compiler column. What is left of the
0.2 targets is T-0180, split into T-0181 to T-0184.

## A precompiled `std.h` (T-0181; 2026-11-09)

After T-0102 a one-line change recompiles only the program's own `.cpp`, but that file
still parses `std.h` and the runtime again, about 0.5 MB of C++ for `words`. Now `std.h` is
precompiled once per compiler and flags, into `build/obj/dz/pch-<key>/`, and the program's
files start from it. The folder holds a one-line `std.h` that includes the generated one,
and `std.h.gch` beside it. A file is compiled with `-include <that header>`. g++ then looks
for the `.gch` itself, and clang++'s driver looks for it too and passes it on as
`-include-pch`, so both compilers take the same flags and `dz` never asks which one it has.
Where neither can use it, `-include` still includes the real header, which is why the
one-line header exists.

Only a file whose first include is a generated header that itself starts with `std.h` uses
it: each module's `.cpp` and `dz_main.cpp`. Including `std.h` before anything else changes
nothing for those. `std.cpp` gains little from it, and the runtime's files do not include
`std.h` at all. On a build from clean the header is built while those three compile, and
the program's files start as soon as it is done. Compiles now run as a pool of up to one
per core rather than in batches, so they need not wait for `std.cpp`.

The header is judged as the objects are, by the content of everything its depfile names,
and also by time: clang++ refuses a precompiled header if any input is newer than it, even
with the same contents, so `dz` rebuilds it then. A header the compiler cannot build means
the files compile without it, with a note. A file compiled from it that fails is compiled
again without it, whatever the compiler said, since clang++'s error for a damaged header
does not always say so. Both paths were tried by truncating the `.gch`, on clang++ 21 and
g++ 14. A note from a build attempt that is then thrown away is not printed, such as the
LTO attempt clang++ cannot link on Windows.

On `words` (Windows, release `dz`, debug build, three runs each): with clang++ 21 a
one-line change spends 157 ms in the C++ compiler, from 225 ms, and the whole `dz build`
takes 390 ms, from 460 ms. With g++ 14 the same change takes 400 ms, from 530 ms. A build
from clean is unchanged on clang++ (0.50 s, waiting on `dz_native.cpp` and `windows.h`) and
went from 1.62 s to 1.50 s on g++. Of the 157 ms, `words.cpp` compiles in about 100 ms. The
rest is checking which objects are up to date, which hashes every header each object's
depfile names, hundreds of them for `dz_native.cpp`'s `windows.h`.

**A module's library headers too.** In `examples/sdl3` the `std.h` header did little: a
recompile of `scene.cpp` went from 500 ms to 430 ms on clang++. The module's header goes on
to include `<SDL3/SDL.h>`, glad and ImGui, and an empty module including only that header
takes 300 ms to compile with `std.h` precompiled, but 85 ms with those headers precompiled
as well. So `start_of` now reads a module header's whole opening run of includes: the
runtime and `std.h`, then any `<…>` headers, ending at the first line that is anything else,
such as another module's header (`breakout.h` includes `game.h` first, so it stays with
`std.h`'s). Each distinct run is precompiled once, keyed by its text with the compiler and
flags, so modules importing the same libraries share it. Each file gets only its own
module's headers, never a union of them all: raylib defines `PI` and `RED` as macros, which
would break a module that never imported it.

That header takes longer to build than a file takes to compile from `std.h`'s: 0.48 s for
SDL3 and ImGui on clang++, 1.5 s on g++. Built first, it would slow down a build from clean
and the first edit. So it is built *alongside*: a file whose set is not built yet compiles
from `std.h`'s header while its set's is built, and uses it from the next build on. A file
whose object from `std.h`'s header is still up to date keeps it, so the build after the
header appears recompiles nothing. A set is built only when one of its files needs
compiling, so a module never edited never pays for it.

On `examples/sdl3` (clang++ 21, debug, release `dz`) a one-line change spends 265 ms in the
C++ compiler, from 500 ms before T-0181. The whole `dz build` is still about 1.2 s. Most of
the rest is not compiling: 420 ms importing the C headers through libclang (T-0101), 210 ms
checking SDL3's CMake build, and 90–270 ms linking. On `examples/raylib` an edit compiles in
95 ms on clang++ and 155 ms on g++. A corrupted library header was refused, the build went on
without it, and the next build rebuilt it, on both compilers.

## One compiled `std` for every project (T-0182; 2026-11-09)

After T-0102 and T-0181 a one-line change was quick, but a build from clean, a new project or
a fresh `dz test` harness still compiled `std.cpp` and precompiled `std.h`: about 0.5 s of a
0.75 s debug build of `words`. The runtime's five files and `std`'s two come out of
`dz_cpp` the same for every program but one in the run corpus, once the `#line` paths are
left out, since an instance of a generic in `std` lives in the module that uses it (spec
§8.4). The `#line` paths were the only difference: they named copies of `std`'s sources in
each project's `build/cpp/std/`.

Now `dz_cpp` marks those seven files `shared`, and `emit_cpp` writes them to a folder of a
per-user cache, `~/.cache/dazzle/std-<hash>/` on Linux (`dz_project::user_cache_dir`;
`DZ_CACHE_DIR` moves it). The hash is of their contents, and of `std`'s sources when
`#line`s name them, so the copies a debugger opens are the ones compiled. The sources go in
its `src/`, and the `#line`s name them there, so every project's files are the same bytes.
`compile` compiles the shared files from the folder, and precompiles `std.h` there, into
its `obj/`. The program's own files and the module headers' precompiled sets stay in
`build/obj/dz/`. `build/cpp` still gets a copy of every file: a module header includes
`std.h` from beside it, and the folder stays the whole program's C++. A file compiled from
the shared precompiled `std.h` then reaches the project's own copy through its module
header, and the include guard, `DZ_STD_H`, skips it.

Keying by contents means nothing had to be listed. The one program in the corpus that
differs, `elements_guard` with `check_trusted = true`, simply gets a folder of its own, as
do `--track-memory`, `DZ_REGIONS=0`, a harness and a build without `#line`s. That turned out
to matter for pruning: a full `cargo test` uses about ten folders at once. So the cache
keeps the 8 most recently used and never removes one used within the hour, judged by a
`used` file each build rewrites.

Several `dz` processes now write one folder, which the test suite does constantly. So
nothing in it is written in place: an object and its depfile are compiled to a name with
the process id appended and renamed onto theirs, then the stamp is written the same way.
A precompiled header is built the same way, which also retires the old rule of deleting
its stamp before rebuilding it. Where Windows refuses the rename because another process
has the old file open, that file was made from the same inputs, so it is used, or this
build links its own copy and deletes it after.

On Linux (Ryzen 7 H 255, release `dz`, median of three, `master` and this measured back to
back), a build from clean of `words` went from 0.76 s to 0.37 s in debug and from 1.21 s to
0.61 s in release with clang++ 19, against 0.28 s and 0.42 s for the C++ version. With g++
14, 0.88 s to 0.39 s and 1.11 s to 0.75 s. `astar` and `expr` moved the same way. A build
from clean now costs what a one-line change does, which T-0180's second target needed.

## The LTO fallback remembered (T-0183; 2026-11-09)

A release build that cannot link with `-flto=auto` (clang++ targeting MSVC without `lld`) is
built again without it. That compiled everything twice on every build from clean. Now, when
the second build succeeds, a `no-lto-<key>` file in the shared folder's `obj/` says so, keyed
by the compiler and every flag. Later builds skip the attempt, and the note names the file
to delete to try again. Only a success is remembered: a link missing a definition fails
without LTO too, and must not turn it off for good. The test stands in for the compiler
with a script that fails any link given `-flto=auto`, so it runs only on Unix.

## Imported headers kept on disk (T-0101; 2026-11-09)

Every `dz` process loaded libclang and parsed `stdlib.h` again for `std`, about 85–115 ms of
a build whose own phases take 90, and 300 ms for `examples/sdl3`'s SDL3 and ImGui. The
importer's in-process cache now has a second level in `headers/` of the per-user cache: one
JSON file per import (serde, already a workspace dependency, now derived on `Header` and
everything in it). An entry holds every file the parse read, by size, time and hash. It is
used while each file holds the same contents, checking size and time first and hashing only
a file whose time changed. On a hit libclang is not loaded at all.

The key is the in-process one plus the `dz` executable's path, size and time, because the
importer's own logic decides what a header becomes, and a compiler being worked on changes
it, and `LIBCLANG_PATH`. It cannot name the libclang the search would find without loading
it. A different libclang's own headers are among the files read, so replacing one changes
them; an entry from another libclang whose files are all still in place is used, giving
what that one read from the same headers, which for C declarations is the same.

The phase went from 115 ms to 2 ms on `words` and from 298 ms to 18 ms on `examples/sdl3`.

## Build time at scale (T-0012; 2026-11-09)

Every build-time number so far came from programs of one file, which is C++'s best case.
`tools/bench_scale.py` generates a project of N modules, about 510 lines of Dazzle each
(structs with methods, `Eq` and `Ord`, a generic container over each struct, an enum with a
`switch`, a `HashMap`, calls into the module before it and the one at half its index), and
the same program in C++, one header and one source per module. The C++ half is built as
`make -j` builds a plain project: each source compiled on its own when it or a header it
includes is newer than its object, with no precompiled headers. Both halves print a
checksum and must agree. Four debug builds of each: from clean, nothing changed, the leaf
module edited, the root module edited. `tests/bench_programs/SCALE.md` has the results.

Dazzle does not scale yet. At 10 modules it is 2.5× C++ from clean and close after an edit;
at 100 modules (51 000 lines, clang++) a build from clean is 60 s against 5.3 s, nothing
changed 8.8 s against 0.05 s, the leaf edited 15 s against 1.1 s. Three causes, filed as
T-0185 to T-0187. The largest: a module's header holds every method of every generic
instance it needs, 1 100 lines for each `List<T>`, so a 500-line module gets a 10 600-line
header, and each module's `.cpp` parses the headers of every module beneath it. Then the
up-to-date check hashes every one of those headers again for each object on every build,
3.2 s at 100 modules with nothing changed. And emitting, ownership, placement and
instantiation nearly triple each time the project doubles.

It also found that `#line` directives named sources as the project's path was spelled on
the command line, so `dz build game` after `dz build /full/path/game` changed every
generated file and compiled each one again. `emit_cpp`'s `#line` rewriting, which already
renamed `std`'s pseudo-paths, now also makes every relative source path absolute, through
one lookup per directive. Panic locations have the same trouble, but what they print is the
program's output, so that is T-0188, a question for the user.

The 0.2 work, measured on it (clang++, 10 modules): before T-0102 every build took 9.7 s,
edit or not, since all the generated C++ was compiled in one command. After T-0102 and
T-0181, 0.6 s with nothing changed and 0.8–1.9 s otherwise. T-0182, T-0183 and T-0101 move
it by 0.1–0.3 s, since at this size `std` is a small part of the build.

## Up to date by size and time, and no link when nothing changed (T-0186; 2026-11-09)

T-0102's check read and hashed every file an object's depfile names, on every build. In
T-0012's 100-module project each object names every header beneath it, so a build with
nothing changed spent 3.2 s hashing. A stamp line now holds the file's size and time beside
its hash. A file with the same size and time is taken as unchanged when that time is at
least 2 s older than the stamp; any other is hashed, as before. The margin is git's answer to
"racy" index entries: on a file system whose clock ticks every 2 s, a file written twice in
one tick keeps its time. The generated files are written just before they are compiled, so
their first stamp often cannot trust them; a stamp that needed hashing and still holds is
written again, and the next build goes by the times. A stamp in the old format means
compiling again, never a guess.

The link is skipped too when nothing it names changed: the command, and the size and time of
each object and library, kept in `build/obj/dz/link-<exe>.stamp`, which is removed before a
link and written after one succeeds.

At 100 modules with nothing changed, the C++ compiler phase went from 3.2 s to 28 ms and the
link from 0.38 s to 0.2 ms. The 4.5 s left is Dazzle's own phases (T-0187).

## Instance bodies out of headers (T-0185; 2026-11-09)

T-0012 found that a 500-line module got a 10 600-line header: every method of every generic
instance it needed, `inline`, 1 100 lines for each `List<T>`. Every module above it includes
that header, so the C++ compile grew with the square of the project, 60 s from clean at 100
modules against 5.3 s for C++.

Now an instance's type and its methods' declarations stay in each needing module's header,
behind its guard, and the bodies are defined once in the program, as ordinary functions, in
the file of the first module emitted that needs them. The prelude is module 0, so `std`'s own
instances stay in `std.cpp`, which T-0182 needs to be the same for every program. One helper,
`place_instance_body`, does it for a type's members, a generic function, a generic impl's
methods and a generic method. It emits the body as a plain module's would be: `emit_body`
already sends a `#[inline]` body to the header, and associated constants always go there, so
what lands in the header is wrapped in an `_INLINE` guard in every needing module's header,
and the rest goes to the owner's file, in the generic's namespace (`cpp_chunks`). A set of
`_DEF` guards shared by every module's emitter says which file owns what.

At 100 modules (clang++, debug), from clean went from 60 s to 12.2 s, an edit to the leaf
module from 15.2 s to 6.0 s, and with T-0186 nothing changed from 8.8 s to 4.8 s. The C++ side
of an edit is now 1.0–1.3 s, against C++'s 1.0–1.1 s; the rest is Dazzle's own phases.

What it cost: an ordinary function is compiled even when nothing calls it, where an unused
`inline` one was not, so a small program's debug rebuild got 0.03–0.06 s slower. And g++
inlines less: `astar` with g++ went from 352 ms to 396 ms. g++ at -O2 inlines a function not
declared `inline` up to 15 instructions against 70, and counts a body that must stay as more
growth. Two tries failed. Marking the bodies `inline __attribute__((used))` changed nothing for
g++, and clang++ then emitted only one of a destructor's two symbols, failing the link.
Raising g++'s budget for functions not declared inline (`--param max-inline-insns-auto=70`)
made `astar` faster than ever, 329 ms, but made `words` print a wrong checksum, and crash
with `DZ_REGIONS=0` — and so did the code from before T-0185 with the same flag, so that is a
bug of its own, T-0189, found rather than caused. Doing what C++ does, each file defining
`inline` exactly the instance methods it calls, would win both back: T-0190.

One `cargo test` run after the change saw `timings_name_every_phase_on_standard_error` fail
once, on a cold cache, while dozens of builds made the new shared `std` at once; it did not
recur in three runs from an empty cache. The test prints `dz`'s whole output when it fails.

## `destroy` of a word ended its lifetime (T-0189; 2026-11-09)

While measuring T-0185, giving g++ a larger inline budget made `words` print a wrong checksum,
and crash with `DZ_REGIONS=0`; the code from before T-0185 did the same. Every sanitizer hid
it. glibc reported "corrupted top size", so something wrote past the newest heap block. With
10 words there were four allocations; a hardware watchpoint on the eight bytes past the last
one caught `String::set_len` writing the text's zero at offset 41 of a 32-byte buffer.

`make_room` had stored the new capacity, 32 then more, into `m_store[1]`, and `set_len` then
`destroy`s that word, a no-op kept as a signal to the ties check (§6.7). The runtime's
`destroy` was `x.~T()`, a pseudo-destructor on a `size_t`, which ends the object's lifetime;
g++ deletes a store followed by the end of its object's lifetime as dead. Inlined together,
the store vanished and the string kept a stale capacity. Reduced to a dozen lines, g++ -O2
prints 0 for a word just set to 41, and warns it is used uninitialised; clang++ prints 41.

`destroy` now runs a destructor only for a class or union (`__is_class`, `__is_union`, which
GCC, Clang and MSVC share), and `move_out` goes through it. The reduced program is the test,
compiled against the runtime with each compiler present.

## g++'s inline budget for Dazzle's code (T-0190, the first half; 2026-11-09)

T-0185 made a generic instance's methods ordinary functions in one file, where they had been
`inline` in headers, and `astar` with g++ ran 12% slower: at -O2 g++ inlines a function not
declared `inline` only up to 15 instructions, against 70 for one that is. An optimised build
with g++ now passes `--param max-inline-insns-auto=70` for Dazzle's own code, the budget of a
declared-`inline` function. All three benchmark programs then run faster than before T-0185
or after it: `astar` 330 ms (352 before, 399 after), `expr` 151 ms (160), `words` 155 ms (165).
A release build takes 0.1–0.17 s longer. clang++ was not slower, and warns at the flag, so g++
is told apart by what `--version` says, which also keeps macOS's `g++`, which is clang, out.

The other half, writing only the instance methods each file calls, is still open. The first
attempt at this flag found T-0189 rather than a speed-up; with that fixed it is safe. What
makes the other half hard is in T-0190: the C++ uses of a member the emitter never writes,
a destructor of a field of a struct held by value in another module among them.

## Dazzle's own phases grow with the program (T-0187; 2026-11-11)

T-0012 found emitting, ownership, placement and instantiation nearly tripling each time the
generated project doubled: 4.5 s of every build at 100 modules, an edit or not. With `perf`
not allowed on the machine, a few hundred stacks sampled with gdb from a release `dz` with
line tables found three causes.

`TypeTable::impls_for`, under every question of whether a type implements a trait, tried
every `impl` in the program. Ownership's `elements_in`, placement, instantiation and emit
all ask it about every type they meet, and every module adds implementations, so the cost
grew with the square of the program: 59% of the time at 100 modules. The implementations are
now also kept by the head of their type, a declaration or a shape such as a pointer or
`i32` (`impls_by_head`), and only those with the type's head, and any for a bare
parameter, are tried. `match_ty` never matches a type against a pattern with another head,
so every answer, and its order, is what it was.

`place_instance_body` (T-0185) emitted an instance's bodies for every module that needs the
instance and kept them only in the first, since the others need just the `#[inline]` part in
their header. `emit_body` now skips the rest there (`inline_only`). The generated C++ is
the same except for the numbers in temporaries' names, which changes every file once.

And `dz build` spent 0.1 s at 100 modules freeing the analysis just before exiting. It is
now left to the operating system, as rustc does.

Dazzle's phases now double when the project doubles: 0.24, 0.50, 0.96 and 1.89 s at 10, 25,
50 and 100 modules, where they were 0.31, 0.75, 1.71 and 4.5 s. At 100 modules (clang++,
debug), nothing changed went from 4.8 s to 2.1 s, an edit to the leaf module from 6.0 s to
3.3 s and from clean from 12.2 s to 9.5 s, against 0.05 s, 1.1 s and 5.3 s for C++
(`tests/bench_programs/SCALE.md`). Small programs gain too: the phases of `hello` went from
75 to 65 ms, and those of the benchmark programs from 90-101 to 74-80 ms. What is left at
scale is one copy of each instance's body in `dz_mono` and emitting it, both linear, and
re-analysing what an edit did not touch, which is T-0023.

A second round the same day took emit's constant factors, profiled this time with the real
release settings (thin LTO, one codegen unit) plus line tables, since the first round's
build without LTO had made clones look three times as costly as they are. Declaring an
instance's methods in a header walked each method's whole body to name its locals, where
only the parameters' names are needed and nothing after them changes those (`param_names`).
The instances a call reaches were found by scanning every module's (`Mono::instance` and
its two siblings now look them up by declaration). And a struct's signature and each node
`Namer` visits were cloned rather than borrowed. Emit at 100 modules went from 0.79 s to
0.69 s, all of Dazzle's phases from 1.82 s to 1.72 s, and a small program's by 3-8%; the
C++ is byte for byte the same. What is left is spread thin: a quarter of the time is emitting
each instance's body once, which is output the program needs, and the rest has no
single place. The next large step is not a constant factor: running phases on several
threads needs `Ty`'s `Rc` to become `Arc`, and skipping the modules an edit did not touch
is T-0023.

## A build with nothing changed skips analysis (T-0192; 2026-11-11)

Stage 1 of the plan to make a build cost what changed rather than how big the program is.
Every `dz build` analysed the whole program before it could learn that nothing needed
compiling: 2 s at T-0012's 100 modules, against 0.05 s for `make`.

A successful `dz build` or `dz run` now writes a build stamp (`crates/dz/src/fresh.rs`),
and the next build with the same key checks it before anything else. The key is the `dz`
executable (path, size, time and version), the entry as written and the folder `dz` ran
from, the options, and the environment variables that change a build. A rebuilt `dz` is
another key, since it may generate different C++ from the same source; no version has to be
bumped by hand.

The inputs are:
- the C++ compiler, resolved on the `PATH` with links followed, by size and time;
- the executable;
- each source pattern, expanded again, so an added or removed file counts;
- each CMake source tree, by its files' sizes and times;
- every file read, by size, time and hash, under T-0186's rule that a time within 2 s of the
  stamp is not trusted.

The files read are taken from three places:
- the source map: manifests and `.dz` sources, hashed as they were read, so an edit made
  while the build ran is seen;
- `dz_cppimport::files_read`: every file a header import read;
- the link stamp's objects and each object's own stamp: generated C++, library sources, C
  and system headers. Those carry the hashes from when the object was compiled.

The warnings the build printed are kept, and printed again on a skip, as Cargo does.

Two things the tests found:
- A file restored to a size and a time that were already settled cannot be seen by any
  time-based check, `make`'s included. The test pins the case the 2 s rule exists for
  instead: a time too recent to trust.
- Building from beside the project (`dz build hello`) makes a different executable than
  building inside it, 24 bytes apart, because panic locations name sources as the command
  line did (T-0188). So the key holds the entry as written and the folder, and the two are
  different builds.

Measured with a release `dz` and clang++:
- nothing changed in the 100-module project: 2.0 s to 5 ms, the check 0.6–2.3 ms over 910
  files and 100 patterns;
- `tests/run/hello`: 7 ms;
- writing the stamp adds 20–40 ms to a real build.

Not covered yet:
- `dz test`'s binary, whose harness builds through another path;
- `dz cpp` and `dz check`, which analyse because diagnostics are what they are for.

`--timings` shows `up to date` on a skip and `build stamp` on a build;
`tools/bench_programs.py` counts the latter as Dazzle's own work.

**Fixed the same day: on Windows the stamp never matched**, and every `stamp.rs` test
failed in CI. A `glob` line hashed the source list as loading found it, joined to the
current folder with `\`, while the check hashed the list the pattern expands to, spelled
with `/`. The two compare equal as `PathBuf`s on Windows, which is why the write's own
comparison passed, but their text differs. The stamp now hashes the expansion, which is
what the check computes. macOS and Linux spell both alike, so only the Windows job saw it.

## A module's instance bodies in a file of their own (T-0191; 2026-11-11)

Stage 3a of making a build cost what an edit changed. Since T-0185 an instance's bodies are
defined once, in the `.cpp` of the first module that needs them. In `bench_scale.py`'s
100-module project that is 7 000 of the leaf `m099.cpp`'s 7 929 lines, and any edit to one of
the module's own bodies compiled them all again. They are now `<module>.inst.cpp`, an object
of its own; the module's `.cpp` keeps its own 891 lines. The instances' temporaries are
numbered from zero rather than on from the module's code, or a `for` added to `main` would
rename `_dz_i1` in every instance after it and the file would change anyway (the new test
fails without that). A module that is first to need no instance has no file at all: an empty
one would still be a compile and an object to link. `std`'s own instances stay with `std`,
in its shared `std.inst.cpp`. A file starts with its module's header, so it is compiled from
the same precompiled header as the module's `.cpp`, and its object's name (`m099.inst-…`)
cannot meet a module's, since a module's name has no `.`.

Measured on the 100-module project, a leaf edit (`bench_scale.py`'s `edit`, five in a row,
medians), release `dz`, on a machine another session was loading, so the walls are noisy and
the C++ phase is the steadier figure:

| leaf edit | before | after |
|---|---:|---:|
| clang++: C++ compiler | 1.06 s | 0.61 s |
| clang++: whole build | 3.45 s | 2.89 s |
| g++: C++ compiler | 1.84 s | 0.53 s |
| g++: whole build | 6.62 s | 2.94 s |

Only `m099.cpp` is compiled again; `m099.inst.cpp` and `m099.h` are unchanged.

The price is a build from clean. Every one of the 101 modules has instances here, so a build
from clean compiles 101 more files, each parsing its module's header again (0.47 s for the
leaf's): about 105 CPU-seconds before and 152 after on clang++, +45%. On 16 cores that is a
few seconds of wall time; on a laptop with four it is more. A build from clean is rarer than
an edit, and stage 3b, the edited module's header precompiled, does nothing for it, so if it
matters the alternative is to keep a module's instances in its `.cpp` until the module is
first edited, the rule 3b uses for its header.

## An incremental build is checked against a clean one (T-0193; 2026-11-11)

Before any build reuses earlier work for the modules an edit cannot reach, there is a test
that would catch it reusing too much: `crates/dz/tests/incremental.rs`. Each scenario builds
a project, applies one scripted edit, builds again in place, then copies the edited sources
to a folder of their own and builds them from clean. `build/cpp` must be the same byte for
byte, once each folder's own path is replaced (a `#line` names its source by its full path),
and the two programs must print the same thing.

The edits are the ones where one module's change shows up in another's generated code: a
leaf and a root body, the root's signature, a module added and one removed, and a constant,
default argument, field default, `Drop` body and reflected type declared in the root and
used from the module above it. They run on `bench_scale.py generate 10`, whose text is known
exactly, so each edit is a replacement that must match once; and a generic body, a field
default and a reflected field in three `tests/run` projects. A scenario whose edit changes no
generated C++ fails, since it would pass however stale a build was. The scenarios run in
parallel, about 20 s in all, debug, with the per-user `std` cache shared. Every build sets
`DZ_INCREMENTAL` itself (`verify` for the incremental one, `0` for the clean one), which the
compiler does not read yet.

Today's compiler re-analyses everything, so the test passes trivially, as it was meant to.
It found one difference anyway: a removed module's `.h`, `.cpp` and `.inst.cpp` stay in
`build/cpp`. Nothing includes or compiles them, so the program is right, but they are stale
output, and per-module reuse will have to tell a module removed from one left alone. The
test tolerates exactly those files, for a module its edit removed, until T-0194 deletes them.

## The module being edited compiles from its own header (T-0195; 2026-11-11)

Stage 3b. After T-0191 a leaf edit at 100 modules still spent 0.9 s compiling the module's
`.cpp`. Taking the file apart showed where:
- an empty file with the same flags: 0.04 s;
- `#include "m099.h"` alone: 0.47 s, 129 000 lines of module headers beneath it;
- with the module's own 890 lines: 0.52 s.

So the module's own header is now precompiled once the module is being edited, and the next
edit starts from it. This is T-0181's header sets with one more set per file, the module's
header. It is built alongside a compile of a file whose object exists but is stale, and used
from the next build on while it is fresh. A build from clean has no stale objects, so it
builds none and waits for none.

`Pch::prepare` used to write each header's folder when it was asked about. Asking about a
module's header for every module left 101 empty folders, so the folder is written by
`spawn` now. The object compiled from the header before is reused while the file has not
changed, as it already was for a set's.

Leaf edit, the C++ compiler phase, from the second edit on:
- 100 modules, clang++: 0.91 s to 0.24 s;
- 10 modules, clang++: 0.32 s to 0.20 s;
- 10 modules, g++: 0.43 s to 0.14 s.

At 100 modules the whole edit went from 3.3 s to 2.45 s. What is left is Dazzle's own
phases, 1.7 s (stage 2 of the plan), and the link, 0.44 s with GNU `ld` (stage 3c).

## A debug build links with mold or lld (T-0196; 2026-11-11)

Stage 3c. With T-0195, a leaf edit at 100 modules compiled its file in 0.24 s and then spent
0.44 s in GNU `ld`. A debug build on Linux now links with `mold`, else `lld`, when one
is on the `PATH` and the compiler links an empty program with it.

That check runs once per compiler and linker and is cached in the shared cache, keyed by both
files' path, size and time. An upgrade asks again, and an uninstalled linker is not found, so
no build fails because of it. Some cases keep the system's linker:
- optimised builds, whose LTO `lld` cannot link for g++;
- macOS and Windows;
- a `-fuse-ld` the user gave in `DZ_CXXFLAGS`.

Neither linker is on this machine, and fetching `mold`'s release binary to measure it was
refused, so there is no number here. The test stands in a `ld.lld` that records its uses
and runs `ld`, and a `ld.mold` that always fails. It checks the failing one is passed over
and the other is tried once and then used, with clang++ and with g++.

On runners with a bundled LLD, Clang can resolve `-fuse-ld=lld` to its own linker before
looking on `PATH`. Dazzle also passes `--ld-path` to Clang so the probe and the final link
use the executable identified in the cache key.

## An edit finishes only the modules it can affect (T-0197; 2026-11-11)

Stage 2a. After the stamp and the C++-side stages, a leaf edit at 100 modules was still 1.7 s
of Dazzle's own work. Ownership, instantiation and emission were 1.2 s of that, done again
for every module.

**Which modules are skipped.** Each build now records every module it wrote, in
`build/obj/dz/modules-<key>/`, under the stamp's key:
- its sources' hash;
- its files' sizes and times;
- the instance bodies it placed and the ones it defines;
- whether it read the type table;
- its ownership warnings.

The next build reads and checks the whole program as before, then hands the driver a set of
modules to finish (`analyse_selecting`). These are the ones whose record is missing or
stale, or that depend on such a module. Ownership (`check_files`), instantiation
(`monomorphize_only`) and emission (`emit_selected`) run for those and for `std`,
whose files key the shared cache. The others' files are compiled as they are, and their
warnings are printed from their records.

**What could not be skipped.** Placement runs over the whole program: its whole-program pass
`release::whole_functions` reads the placement of every callee, and an edited module calls
into the modules beneath it. That is the 0.29 s left.

**Instance bodies.** Which module defines one depended on emitting every module in order:
the first to need it. The emitter is now told which bodies the kept modules define
(`Selection::claimed`), and an emitted module defers to a kept one before it. It reports
every body it placed. Owners are then worked out again from all modules' needs, and a kept
module whose bodies changed hands is instantiated and emitted too. Anything the rule cannot
explain, or any use of the type table (whose ids follow emission order), emits the whole
program.

**Checks.**
- `DZ_INCREMENTAL=verify` also emits the whole program and fails on any file that differs.
- The edit corpus (T-0193) runs every incremental build with verify on, and passed
  unchanged.
- `crates/dz/tests/reuse.rs` changes a kept file by hand, keeping its size and time, to
  show it really is kept, and that verify refuses it.

**Measured** (`SCALE.md`), a leaf edit at 100 modules: 1.07 s, against 1.10 s for
`make`.
- Dazzle's own phases: 0.56 s. Placement (0.29 s) and the front end are what remain.
- Nothing changed: 0.01 s, against 0.05 s.
- An edit to the root module: 2.32 s. Every module depends on it, so every module is redone,
  even for a body-only edit. That is stage 2b.

The link varied between 0.08 s and 0.44 s across the day with GNU `ld`, for the same
link, so compare its column with care.

## A body-only edit leaves the modules above it alone (T-0198; 2026-11-11)

Stage 2b, the last of the plan. T-0197 redid every module depending on an edited one, so a
body edit to the root of the 100-module project redid all of them: 2.32 s, against 1.0 s
for `make`, which recompiles the one `.cpp`.

**The interface.** A module's record now also holds a hash of what other modules' C++ can be
made from of it, written from its AST:
- declarations and signatures, constants, statics, macros and imports, as text;
- field defaults and default arguments, as text with their line;
- generic items and traits whole, with their line.

The body of a plain function, method or constructor is left out. A module is redone when its
own sources changed, or the interface of anything beneath it did.

Lines are kept only where a position reaches another module, and the corpus showed it does.
`root_body_grows_above_a_generic` grows a body above a generic that only `m001`
instantiates. With lines left out entirely it failed: verify reported `m001.inst.cpp`
differs, its `#line`s naming the moved generic. The first version of that scenario used
`Shelf0<T>`, whose instances the root itself defines, and passed either way. So it now adds
a generic of its own.

**Placement** can carry a body across modules too: whether a callee keeps an argument, or
builds its result whole, comes from the callee's body. So each record also holds a
fingerprint of what placement decided for the module's own bodies
(`BodyPlacement::fingerprint`, which leaves the file's id out), and one for every generic
body. Placement moved before the choice of modules, so `--timings` lists it before
ownership. No scenario in the corpus moves a decision across modules this way, so that part
is held up by verify alone.

**Measured** (`SCALE.md`), at 100 modules:
- a root edit: 2.32 s to 1.08 s, against 1.00 s for `make`;
- a leaf edit: 1.08 s, against 1.10 s;
- nothing changed: 0.01 s, against 0.05 s.

From clean is 13.2 s, against 5.2 s for C++, mostly T-0191's extra translation units.
What remains of an edit at scale is placement, 0.29 s, and the front end, 0.17 s, over the
whole program.

## A build from clean compiles a module's two files as one (T-0199; 2026-11-11)

**Profiled first.** At 100 modules a build from clean was 13.2 s against 5.2 s for C++,
11.3 s of it the C++ compiler. Timing files one at a time, and clang's `-ftime-trace`,
showed it is parsing, not code generation:
- every module's file includes its header, which includes every module header beneath it,
  49 KB each against C++'s 4 KB: `m050.h` alone parses in 0.24 s, `m099.h` in 0.46 s;
- most of that is the `List` instances, each a class with ~45 member declarations whatever
  the program calls: 110 ms of `m050.cpp`'s 167 ms of `ParseClass`, where C++ parses
  `std::vector` once, in 4 ms;
- since T-0191 each module is two files, and both parse it all: `m050` 0.30 s + 0.44 s
  apart, 0.48 s as one file.
- `#line` is 56% of an instance file's bytes and costs nothing measurable.

**The fix here** is the cheapest of the three T-0199 lists. When both of a module's files
need compiling and there are more files to compile than cores, `combine` replaces the pair
with `<module>.both.cpp` in the object folder, which includes both, and links its one
object. With cores to spare two files at once finish sooner than one holding both, so a
small build is as it was. A combined object still up to date stands in for the pair, which
is every module after a clean build: so the count of files to compile leaves those pairs
out, or every later build looks crowded and compiles each edited module combined, as the
first version did (each leaf edit 1.58 s). A body edit then compiles the two apart, the
instance file once. That first edit also precompiles the module's header (T-0195), now told
the module was compiled before by its combined object.

Generated C++ is unchanged, so the edit corpus holds as it was; its 10-module first build
combines on a 16-core machine. Forcing every pair together, the run corpus passed on
clang++ and g++, so a module's two files share no name at file scope. One test fails on g++
before and after: `a_module_being_edited_compiles_from_its_own_header_precompiled`, whose
depfile from g++ does not name the precompiled header.

**Measured** (`SCALE.md`): from clean at 100 modules 13.19 s to 8.99 s (C++: 5.22 s), the
C++ compiler 11.28 s to 7.11 s; at 50, 5.10 s to 3.83 s. Edits as they were. Next is
declaring only the instance methods a program calls, T-0199's second fix.

## An instance has only the methods the program calls (T-0199; 2026-11-11)

T-0199's second fix. Every instance of a generic struct declared every method of the
generic in each header that held it, and its owner's file defined them all: about 45 for a
`List`, against the handful a program calls.

**Which methods.** `dz_mono::called_methods` walks the whole program once: every body but a
generic struct's plain methods is a root, and such a method's body joins once something
counted calls it, by `Method`, `BaseMethod` or `Assoc`. It is per generic and name, not per
instance: a call anywhere, `std` included, keeps the method on every instance. Per instance
would keep about 5 of a `List`'s 36 where this keeps 18, but needs the type arguments
followed through every call, which instantiation only does for the modules a build redoes.

**Why it must be whole-program.** An instance's class is written, under one include guard,
into the header of every module that uses it, and a file gets the first one it includes. So
every header must say the same, and a module the incremental build keeps has the header it
had. The set is folded into the program-wide `templates` fingerprint, which redoes every
module when it changes. Two edit scenarios, `first_call_of_a_method` and
`last_call_of_a_method`, both fail with the fingerprint left out.

**What stays.** `virtual` and `override` methods, since a table of them names each;
constructors, `drop`, `copy` and constants; methods with type parameters of their own,
instantiated per call already; and every method of an instance `std` itself holds, since
`std.h` is shared by every program on the machine (T-0182) and is included before any
module's header, so its full copy is the one every file gets.

The emitter writes no method calls of its own: a `for` loop, an index or an operator goes
through a trait's implementation, which is a root. So a method missed would be a C++
compile error in every build, not a link error in some, and the run corpus passes on
clang++ and g++.

**Measured** (`SCALE.md`): from clean at 100 modules 8.99 s to 7.22 s (C++: 5.24 s); the
C++ compiler 7.11 s to 5.64 s, and Dazzle's own phases 1.81 s to 1.51 s, as fewer instances
are made. `m050`'s two files as one 0.48 s to 0.38 s, against 0.34 s for its C++. What is
left of a clean build's gap is mostly Dazzle's own phases over the whole program.

## GCC records dependencies from precompiled headers (2026-11-12)

Without `-fpch-deps`, GCC used the edited module's precompiled header but left it and
its inputs out of the object's depfile. The object cache uses that file to decide when
to rebuild, and the module header test failed on Linux when it looked for the header
there. `dz` now passes `-fpch-deps` to GCC both when creating and when using the header.
Clang already records these inputs without the flag.

## A removed module's generated files are deleted (T-0103, T-0194; 2026-11-14)

Since T-0087 a build compiles only the files it emitted, so a renamed or removed module no
longer broke it, but its `.cpp`, `.h` and `.inst.cpp` stayed in `build/cpp`, and its objects
in `build/obj/dz`. Now `emit_cpp` ends by deleting every `.cpp` and `.h` in `build/cpp` that
this build's program is not made of (the files emitted, and the kept modules' under T-0197)
and that starts with `// Generated by the Dazzle compiler`, which every generated file does.
A file a user put there has no such line and stays, which is what kept T-0087 from deleting
anything. Each deleted `.cpp`'s objects go with it, found by `object_path`'s
`<stem>-<16 hex>.o` and taking their `.d` and `.stamp` along, and so does the module's
`.both.cpp` with its object (T-0199's two files compiled as one). The build stamp,
`build-<key>.stamp`, has no object, so a module named `build` cannot take it.

T-0193's `module_removed` scenario tolerated exactly those three files; the allowance is
gone, and the scenario fails with the sweep turned off. No `dz clean`: the build itself now
leaves `build/cpp` as a build from clean would.

## Ownership is not run over `std` (T-0025; 2026-11-14)

`dz_flow::check_files` checked every body, `std`'s included, though `std` is the same in
every program and a body is checked against other functions' signatures alone, so nothing a
program declares can change what `std`'s bodies say. Its `sends` and `in x@home` were
already left to the test suite (`check_std_homes`, T-0200); now all of ownership is.
`check_files` skips `std`'s bodies, and `check_std` (replacing `check_std_homes`) runs every
check but unread bindings over them alone. Two tests run it: `dz_flow`'s
`std_is_ownership_clean`, with every `#if` branch in place, and `dz_driver`'s
`std_is_ownership_clean_with_each_branch`, through the real pipeline with `TRACK_MEMORY`
off and on, the one flag `std` branches on. A use after `move` planted in `std/text/join.dz`
fails both.

**Measured**, release `dz check`, median of five: `words` 57.6 ms to 40.0 ms, `astar` 63.1 ms
to 46.4 ms, `expr` 58.0 ms to 41.7 ms; ownership 22–26 ms to 5–9 ms. What is left of
`std`'s cost is its type check (~14 ms) and placement (~6 ms), T-0025's remainder.

## What a small program's rebuild spent besides compiling (T-0025, T-0190; 2026-11-14)

A debug rebuild of `words` after an edit that left its C++ unchanged took 106 ms, of which
the C++ compiler compiled nothing. Five costs, two of them bugs:

- **The C++ compiler was asked `--version` up to three times a build**, 15 ms each: by
  `find_compiler` when none was named, by `is_gcc` for g++'s flags, and again by
  `find_compiler` for the build stamp. `version_of` now keeps the answer in the process and
  in the per-user cache (`compilers/`), under the compiler's resolved file, size and time,
  as the fast-linker probe is (T-0196): an upgrade, or another compiler first on the `PATH`,
  is asked again. The build stamp went from 18 ms to 2 ms.
- **A refreshed stamp was never written** (`native::refreshed`). An object whose inputs
  were written within `SETTLED` of its stamp has them hashed, and the stamp is written again
  so the next build can go by their times. It was written through `shared::write`, which
  skips a file that already holds the text, and a refreshed stamp's text is usually the same,
  so its time never moved and every build hashed again. The shared `std`'s objects are
  always in that state, since one build writes their sources and their stamps: about 3 MB of
  system headers hashed on every build, 20 ms. `shared::replace` writes it regardless, and
  only the inputs that needed hashing are read again, not every one.
- **`std`'s C++ was emitted every build**, 14 of emitting's 20 ms: T-0197's reuse forced
  `std` into every selection, since its files are what name the shared folder (T-0182). Its
  record now names the folder, `Shared::kept` builds from it, and `std` is kept like any
  module when its sources, the root's manifest (whose settings decide `std`'s `#if`s), its
  placement and the generic bodies are as recorded and the folder still holds its files.
  `DZ_INCREMENTAL=verify` builds and `crates/dz/tests/incremental.rs` hold it to a clean
  build. A one-module program is now selective too: it used to emit everything, since its
  one module was all the modules there were.
- **`std`'s checked layer was copied out of the per-thread cache** (`dz_check::check_from`),
  5 ms, in a process that analyses once. `dz build`, `run` and `cpp` call
  `dz_driver::analyse_once`, and the layer is handed over (`check_with`); the editor and the
  tests keep the cache.
- **`std`'s 85 files were parsed one after another**, and their trees copied the same way:
  both now on up to eight threads (`on_every_core`), 10 ms to 5.5 ms.

**Measured**, release `dz`, debug builds, median of seven, `master` and this back to back:

| | `words` | `astar` | `expr` |
|---|---:|---:|---:|
| edit, C++ unchanged (a comment) | 136 → 56 ms | 142 → 62 ms | 136 → 55 ms |
| edit adding a function | 335 → 261 ms | 346 → 270 ms | 337 → 260 ms |
| nothing changed | 6 → 2 ms | 7 → 2 ms | 7 → 2 ms |

What is left of Dazzle's own phases in a small rebuild, about 45 ms: the type check (~11 ms)
and placement (~7 ms), nearly all `std`, which T-0025 now says would need the checked layer
kept on disk.

**A regression at scale, found on the way**: re-running `tools/bench_scale.py` for the
website, a debug build of the 100-module project from clean took 12.0 s, against 7.2 s on
2026-11-11. Bisected, with the project as today's generator writes it (D-0097's `sends`
since r119): 7.7 s at r119, 8.1 s through T-0177, 12.0 s from T-0214. T-0214 let `std`'s
`#[inline]` bodies reach other modules' instances, and made `List`'s accessors `#[inline]`,
so each module's header held `len`, `as_slice` and `as_mut_slice` for each of its `List`
instances, and every module above it parsed them again: `m050.h` 41 KB to 65 KB. At `-O0`
nothing is inlined, so in a debug build they bought nothing. `dz_cpp::Options::
inline_instances` is now off for a debug build (`build::for_build`): an instance's
`#[inline]` bodies, and its constructors that only store their parameters, are defined once,
as its other bodies are (T-0185). Release builds keep them in headers, which is what
vectorises a `List<i32>` loop on clang++. From clean, 12.0 s to 8.1 s. Since the root's
manifest decides `optimize`, every module's record hash now includes it (`reuse::
source_hash`), so changing a build setting there emits every module again rather than keep
files written for the other one.

**T-0190 is closed** with nothing more built: its first cost, unused instance methods
compiled, went with T-0199, and its second, g++'s inlining, with the inline budget. Its
estimate of 0.03–0.06 s left predates T-0199; a body edit now recompiles only the module's
`.cpp`, never its `.inst.cpp`.

## A one-word `Box` (r133; D-0104; T-0229; 2026-11-14)

The user chose, of the four layouts T-0229 laid out, to keep a box's allocator in its
allocation and reach a derived value's drop and size through the vtable, refusing a `Box`
conversion to a base with none, and to fix in the same change a crash found while weighing
them. `Box<T>` is `m_ptr` alone, 8 bytes where it was 24, and `expr`'s `Expr` 32 where it
was 64 (`T::size`, as `sizeof` in the generated C++ says).

- **The header.** `Box(value)` allocates `max(8, alignof(T))` bytes of header and then the
  value, at `max(8, alignof(T))`, and writes a `BoxHeader`, the allocator, in the word just
  before the value, so the drop finds it from `m_ptr` alone whatever the alignment.
- **The size.** The drop asks `dyn_sizeof(m_ptr)` and `dyn_alignof(m_ptr)`, builtins only
  `std` can name (`STD_BUILTIN_FNS`, accepted by resolution only in the prelude). In C++ they
  are `::dz::rt::dyn_size` and `dyn_align`: `p->_dz_size()` where the type has one, else
  `sizeof(T)`, so a box of a plain struct costs nothing at run time. Every polymorphic struct
  now declares `_dz_size` and `_dz_align`, `virtual` in the first polymorphic struct of a
  chain and `override` below it. `destroy` through the box is the virtual destructor.
- **The virtual destructor** was declared only by a polymorphic struct with no base, so a
  struct that added the first `virtual` below a base with none, and had no `drop` of its own,
  had none, and a value deriving from it was destroyed as it. The first polymorphic struct
  of a chain now declares it.
- **`DZ0588`**: a `Box<Derived>` where a `Box<Base>` is wanted, `Base` with no `virtual`
  method. Explained, with `tests/errors`, `typed/` and `typeerrors/` cases. `typed/` had a
  `Box<Panel>` used as a `Box<Widget>`, which has no `virtual`: it moved to a new pair.
- **The crash.** An `Rc<Plain>` or `Box<Plain>` of a `Fancy` adding the first `virtual`
  below `Plain` failed when dropped (`free(): invalid pointer`): `Plain` sits a word into a
  `Fancy`, after its vtable, and the drop function was handed `Plain`'s address as a
  `Fancy`'s. `Rc`'s drop now passes the block, and `drop_value_as<T>` finds the value in
  `RcBlock<T>`; the `Box` conversion is `DZ0588`'s. `tests/run/handle_upcast` has both,
  and `a_tracked_build_reports_leaks_and_bad_frees` runs it tracked.
- **What reads a box.** `release.rs` proved a box's drop only frees by reading `m_drop`'s
  type (T-0218); it now asks of `destroy` whether the type only frees and, when it has a
  vtable, nothing derives from it (`dyn_only_frees`). So a `Box<Shape>` of a `Shape` with no
  `virtual`, which can no longer hold a `Circle`, has its drop skipped in a region; the
  placement test that pinned the opposite gives its `Shape` a `virtual` method. Placement
  found a container's constructor by a field keeping an allocator, which `Box` no longer
  has: `holds_allocator` counts a `Box` as keeping one (`Keeps`).

Measured on every benchmark, a baseline `dz` built from `d49cfef` and this one, back to back
on a quiet machine (`RESULTS.md` is the after):

| Program | Compiler | Before | After | Peak memory before → after |
|---|---|---:|---:|---:|
| expr | clang++ | 137 ms (1.02×) | 140 ms (1.04×) | 140.7 → 141.0 MiB |
| expr | g++ | 134 ms (0.92×) | 127 ms (0.87×) | 141.1 → 140.6 MiB |
| astar, json, raytrace, words | both | | within 2 ms | unchanged |

A first pair agreed (136 → 140 and 133 → 127 ms), its baseline flagged as run on a busy
machine. `expr`'s peak is not its trees: each statement's tree is dropped before the next is
parsed, so what the smaller nodes save is never resident at once. Its g++ release build
went from 0.80 to 0.65 s.

**Why the time barely moved.** The tree work is about a quarter of the run, and most of it is
moving a node up through each parser level and dropping the moved-from, null, box behind.
The old drop reached the value's through a function pointer, so `~Box` was a leaf and both
compilers inlined it everywhere: the old binary has no call to it. The new one destroys the
value directly, which drops its boxes, and a compiler inlines nothing into a recursive call:
clang++'s binary has 43 calls to `~Box`, 4.7% of the run, which is what the smaller nodes
saved. Keeping only the null test in the drop and the rest in a function of its own made
clang++ 136 -> 129.5 ms and g++ 133 -> 134.5 ms, g++ then keeping `Expr`'s drop out of line
instead, so it was not kept. What would fix both is not calling the drop of a value flow
knows was just moved from.

## A move may copy the bytes (r134; D-0105; T-0225; 2026-11-14)

`json` still held 28 MiB more than its C++ half after T-0228. Taken apart with `massif` on
both halves, regions on and off: about 10 MiB is what regions add (chunk slack, not yet
measured further), and most of the rest is how big a new `List` started, room for 4
elements, where libstdc++'s `vector` starts at 1. A first allocation of 2 saved 13 MiB but
cost 9 ms, because a growing `List<Json>` moved each element through a switch on its tag
that resets the source. The user chose to let a move copy the bytes, revising D-0081 and the
line T-0133 drew, and then a first allocation of 2.

- **`T::is_relocatable`** (`TypeQuery::IsRelocatable`, `TypeTable::relocatable`): every type
  but a type parameter, an imported C++ class with a destructor (`cpp_owned`) and an opaque
  handle. Answered where every query is: the checker's table, `dz_cpp`'s folding and
  emission, and `release.rs`'s folding.
- **`List`** grows (`move_to`, through `realloc`), shifts (`shift_up`, `shift_down`) and
  appends relocatable elements as bytes. A copy, `insert_slice` and `extend`, stays on
  `T::is_trivially_copyable`. `grow_to` starts with room for 2 elements up to a KiB.
- **`memcpy` and `memmove` are emitted through `void*`**, so g++'s `-Wclass-memaccess` does
  not warn of a relocation, a type C++ does not call trivially copyable.
- **Tests.** `tests/run/relocate`: short and long `String`s, a `drop` type, one with a vtable,
  a union of nested lists, through growth, `insert`, `remove` and `append`, tracked and in
  the warning-clean set. `tests/run/cppinterop` gains `Anchored`, a C++ class keeping its own
  address, which a `List` moves through its move constructor: a byte move would make it
  report -1. `typed/` and `typeerrors/` have the query. `docs/testing.md`'s audit records the
  one reliance on purpose outside C++'s object model.

Tried and not kept: a region reusing its freed small blocks by size class, which changed
nothing on any benchmark, since none grows small containers inside a region. And `List<T>`
of an imported C++ class needs a default constructor, since `resize` is emitted for the
instance though nothing calls it; `Anchored` has one for that.

Measured on every benchmark, a baseline `dz` built from `e37c032` and this one on a quiet
machine (`RESULTS.md` is the after):

| Program | Compiler | Before | After | Peak memory before → after |
|---|---|---:|---:|---:|
| json | clang++ | 239 ms (1.07×) | 242 ms (1.08×) | 206.6 → 196.4 MiB |
| json | g++ | 258 ms (1.22×) | 257 ms (1.21×) | 206.7 → 195.9 MiB |
| astar, expr, raytrace, words | both | | within 2 ms | unchanged |

With the byte moves and the old first allocation of 4, `json` was 236 and 248 ms and kept
its memory: the user chose the memory.

**Then `Deque` and `HashMap`.** `Deque` grows a relocatable element type by copying its
ring as two runs, from the head to the end of the old array and then from its start;
`HashMap`'s rehash moves each key and value into its new slot with one `memcpy`.
`tests/run/relocate` gained a deque that wraps around before it grows and a map of
`Noisy` values that rehashes, the same on clang++ and g++ and as the compiler before.
Measured against `8a126fa`, no benchmark moved: none grows a deque or a map of
non-trivial elements far enough to show. Filed on the way: T-0230, a `List` of an imported
C++ class with no default constructor does not compile, since an instance has `resize`
whenever any `List` calls it (T-0199).

## The gap to C++ is not safety; a parameter used in place, g++'s unit-growth limit (T-0234; 2026-11-15)

The user asked whether `json` and `raytrace` behind their C++ halves is the price of being
safe. Measured first, then two fixes. Compiler work; no program behaves differently.

**It is not safety.** With `panic`, `panic_bounds` and `panic_len` made
`__builtin_unreachable()` in the generated C++, so every bounds, division and assert test
is deleted, and `-fwrapv -fno-finite-loops -fno-strict-aliasing` reversed, neither program
ran faster on either compiler (`json` 1.07× and 1.24×, `raytrace` 1.07× and 1.07×, as
before). The C++ halves given the three flags did not slow down, and with libstdc++'s
bounds checks on, `json`'s ran 5% faster, which is how much a build moves for no reason.
T-0234 has the tables, `json` by phase, and each cause with what it is worth.

**What it is**: the generated C++ is lowered a statement at a time and the C++ optimiser
clears most of it up, not all. Two of the causes are fixed:

- **A by-value parameter the body moves on, on every path, is used where it is**
  (`moved_out_params` in `dz_cpp/src/lib.rs`). `take_ownership_of_params` moves each owning
  parameter into a binding of the body's own, so it drops at the body's end under every
  ABI. One the body always moves out is empty by then, so where its drop is silent
  (`drops_silently`) the body binds a reference to the parameter. `List::push` moved its
  element into the parameter, into the binding and into the list; now into the parameter
  (built there, when the argument is a temporary) and into the list. The signature is
  unchanged, so trait and `virtual` methods and copyable values qualify. A value with a
  `drop` of the program's own keeps the binding: the first version left it out, and
  `tests/run/discarded_move` failed with one `drop 0` fewer after `push(move(c))`, which is
  what that test is for. `a_parameter_moved_on_is_used_in_place` in `dz/tests/run.rs` holds
  the four cases. Taking the caller's value by `&&`, as a constructor does (T-0097), was 1–2
  ms more on `json` by hand and is not built: a copyable argument that no copy was placed
  for is passed as an lvalue.
- **g++ gets `--param inline-unit-growth=100` in an optimised build** (`dz/src/build.rs`).
  Asked why it left `String::push_str` a call, g++ named `early-inlining-insns`, which
  changed nothing, as `max-inline-insns-single` at 120, 200 and 400 and `-O3` did not. The
  limit that binds is the unit's growth, 40%, which applies to a unit over 10,000
  instructions: under LTO the unit is the program, and with `std` in it a Dazzle program is
  always over, a small C++ program never. At 100, 300 and 1000 the binary is the same one.
  `always_inline` on the nine text functions was 12–14 ms on `json`; this is 17, and
  leaves the choice with the compiler, as §12.8 says.

`RESULTS.md` is the run after both: `json` 257 → 233 ms on g++ (1.21× → 1.13×) and 242 →
234 ms on clang++ (1.08× → 1.06×); the other four programs within a percent, and build
times as they were (`json`'s release build on g++ 1.17 → 1.12 s).

The experiments were whole runs of the generated C++ patched by hand, as T-0222's were.
`perf` was not available (`perf_event_paranoid` is 3 again), so instruction counts came
from `valgrind --tool=callgrind`, which reads the `#line` directives and so reports by
Dazzle source line, and the phases from a timer added to both halves' `main`.

## Values in the shape a C++ programmer writes, and an optional that is not rebuilt (T-0222; 2026-11-15)

Compiler work, no language change: no program behaves differently, and
`tests/run/value_shapes` prints what it printed before, on clang++ and g++. `raytrace` is
0.97× its C++ half on g++ and 1.06× on clang++, from 1.07× on both.

**Three shapes in the generator** (`dz_cpp/src/lib.rs`), the ones T-0222 named after its
hand-written bound:

- **A struct literal by position.** `Vec3{x, y, z}` for a type C++ calls an aggregate,
  where the fields written are the first ones (`struct_literal`). The lambda that zeroed
  a value and assigned each field stays for a field left out in the middle, an imported
  struct, an allocator and a union's variant. Spec §16's sentence on C++20 says so now.
- **A `let` assigned by its branches.** `let x = if …`, `if let`, `switch`, a block or
  an `or` with a value declares `x` and assigns it in each branch (`fills`, `fill`), and
  a block whose value is such a construct writes where its own value goes (`assign_into`).
  The `_dz_val` temporary is left for a `return`, an argument and an operand. The local is
  in scope in its own branches, so `Namer::stmt` claims its name before theirs.
- **A value named where an optional holds it.** `if let h = f()` and `let h = f() or
  return` bind a reference to the payload of the temporary optional, for plain data
  always, and for a value that owns something where `drops_silently` allows, in a block of
  its own so it dies where the `if let` ends.

Measured as one change, 486 → 481 ms on clang++ and nothing on g++. Patching the generated
C++ by hand again found g++'s 40 ms in the runtime (`dz_cpp/src/runtime.rs`):

- **`Opt` is built and assigned from its value by reference**: `Opt(const T&)` and
  `Opt(T&&)` where there was `Opt(T)`, and an assignment from exactly a `T` that writes
  the payload in place. `best = hit` had copied 48 bytes three times.
- **An empty optional of plain data builds no payload** (`OptStore`): a union of the
  payload and an empty struct, so `Opt()` is still `constexpr` in C++17 and writes nothing.

Neither moves g++ alone (668 and 647 ms); together 601, on the old generator's output as
on the new, against 623 for the C++ half. T-0222 has the table, and two things to know
before patching generated C++ again: `dz cpp` writes debug-mode C++, and g++ under LTO
has two layouts of this program that a small edit can flip.

`cpp20_uses_designated_initialisers_and_cpp17_does_not` now pins the positional form and
the lambda where a middle field is left out. The whole suite passes on clang++, `dz`'s own
tests on g++, and every program under AddressSanitizer and UBSan.

`RESULTS.md` is the run after it: `raytrace` 670 → 602 ms on g++ (C++: 623) and 487 →
479 on clang++ (C++: 454). Nothing else moved, measured against the compiler before the
change on the same day. The file's last run was at `7875aab`, before values moved by
default (T-0235), and `json` had gone from 1.06× and 1.13× to 1.01× and 1.06× with that,
and from 195 MiB to 185; this run records it as 0.99× and 1.07×.

## A location is a constant, a float's text is to_chars', a union is built in place (T-0220, T-0225, T-0231; 2026-11-16)

Compiler and runtime work, no language change: every program prints what it printed. Three
items of the run-time list the user set on 2026-11-15, measured mostly by counting
instructions with callgrind, since the machine was busy with other work for much of the day
(load averages up to 29): a count does not change with load, and every timing below is
interleaved runs of both builds in one sitting.

- **A location is a constant of its function (T-0220).** Each `SourceLocation` a body passes
  is one `static constexpr` constant, declared at the body's top once it is known which the
  body uses (`source_location`, `site_consts`), and the runtime's checks (`index`, `div`,
  `rem`, the panics) take it by `const&`: a check stores nothing until it fails, where it
  built the location on the stack every time, on g++ ahead of the test. A Dazzle function's
  `loc: SourceLocation` stays a value: by `const&` too it stopped g++ inlining
  `String::push_str` into `json`'s writer, 3% more instructions.
- **A float's text (T-0225).** For a value from 1e-3 below 2^24 (`f32`) or 1e15 (`f64`),
  `std::to_chars`' own shortest text, when it has no exponent, is written as it is, rather
  than its scientific form taken apart with `atoi` and laid out again. Above 2^24 an `f32`'s
  fixed text is its exact digits, which D-0101 does not write. The same bytes as before on
  226 million values on both compilers; `tests/run/float_text` has each side of each bound.
- **A union built in place, and left plain when moved (T-0231).** `expr` spent 7.5% of its
  instructions on clang++ calling the drop of empty boxes. A non-trivial union's variant
  literal now builds its payload in place by position (`no_payload`), with no temporary
  `Box`es to drop, and a move leaves the source in its first variant, built over the
  moved-from payload without dropping it, where that variant is plain data and the union's
  drops are silent. T-0097 had stopped resetting a moved-from union because the reset was a
  switch, a drop and a construct; this one is two stores.

Instructions against the compiler before (`ac6e719`), clang++ / g++:

| Program | Instructions |
|---|---:|
| `astar` | -2.0% / +0.1% |
| `expr` | -14.2% / -4.6% |
| `json` | -6.4% / -4.6% |
| `raytrace` | level / level |
| `words` | -6.0% / -2.0% |

`RESULTS.md` is the run after, on a quiet machine (load 1.1): `expr` 140 → 128 ms on clang++
(1.04× → 0.96× its C++ half) and level on g++; `json` 221 → 213 ms on clang++ (0.99× ->
0.96×) and 223 → 220 on g++ (1.06×); `words` 0.87× and 0.88× → 0.86× on both; `astar` and
`raytrace` as they were. One cost: `expr`'s release build on g++ from clean is 0.66 → 0.77 s
(C++: 0.51), all of it in the link, where g++'s whole-program pass now inlines more of the
parser; the other programs' builds did not move.

Tried and not kept, each measured: `Box`'s drop split into an inline null test (clang++
-6.8%, g++ +4.1%, as T-0229 found); a constructor taking every field, for a struct C++ does
not call an aggregate (0.4% of `json`'s instructions); and location parameters by `const&`
(above). Found on the way: `dz fmt` dropped a union payload field's default (T-0236, fixed,
and `tests/fmt/items.dz` now has one), and such a default is accepted, never checked and
never applied (T-0237, a question for the user, since §7.5 does not say payload fields take
defaults).

New tests: `a_checks_location_is_a_constant_of_its_function` and
`a_moved_union_is_left_plain_and_a_variant_is_built_in_place` in `dz/tests/run.rs`, and a
union section in `tests/run/value_shapes`, the same output before and after on both
compilers.

## `json`'s reflective writer works out each type once (T-0225; 2026-11-16)

Benchmark work, no compiler or language change. Asked whether reflection could be faster,
the user chose to keep `json`'s writer reflective rather than write `to_json` by hand or
generate it (§13.4). It was redoing, for every value, work whose answer depends only on the
type: comparing the type's name with `"List<"`, reading each field's tag again (and checking
it is UTF-8), and searching a list's fields for `m_data` and `m_len`. `Writer` now prepares
a `Plan` per `TypeId` the first time it meets the type, keys quoted and tags read, and
writing a value only reads it.

Its own instructions halve on both compilers. `json` is 0.93× its C++ half on clang++ (from
0.96×, 2.2% faster) and 1.06× on g++, level: there g++ then kept `String::push_str` out of
line in `Formatter::write_str`, after inlining `make_room` into it, which costs about what
the writer saved. A hand-made `cold, noinline` on `make_room` moved that luck around rather
than removing it (better with this writer, worse with the old). The writer is more code:
the release build from clean is 0.05 s longer on clang++ and 0.15 s on g++ (1.23 s against
C++'s 0.70, inside T-0180's twice). T-0225 has the table.

## A union payload field's default (r136; T-0237), and `main`'s region kept (T-0233; 2026-11-16)

**A language change, r136, the user's choice.** A union payload field may have a default,
as a struct field may. It was accepted before and never checked or applied: a field left out
read zero, and a default of the wrong type compiled. Now:

- **The checker** gives each payload default a body, `BodyOwner::PayloadDefault { owner,
  variant, index }`, checked against the field's type as a struct field's default is
  (`DZ0501` for the wrong type). The new owner joins `FieldDefault` wherever a body's owner
  is matched: monomorphization, placement, ties, the HIR dump.
- **Codegen** writes the default as the payload field's member initialiser
  (`default_value`, shared with struct fields), so a literal that leaves the field out, by
  either of `union_literal`'s paths, and `let u: U` hold it; a function a payload default
  calls is declared first (`field_default_prototypes`). A plain-data union's literal now
  builds its payload before setting the fields written: it set only those, so a field left
  out was whatever the storage held, which a default made reachable.
- **Tests:** `a_payload_default_is_checked_against_its_field` in `dz_check`, the matched
  pair (`Pane` in `typed/`, `Margin` in `typeerrors/`, and the `hir_typed` snapshot's new
  body), and `tests/run/value_shapes`, the same on both compilers and warning-clean.

**`main`'s region, measured again (T-0233).** A value whose region would be `main`'s
outermost block, on the heap instead, built behind a switch: `json` no smaller (187.2 MiB
against 187.7 on clang++, 186.7 against 184.7 on g++) and about 1% slower; the other four
level. The 10 MiB and 4 ms of 2026-11-14 went with r134 and r135. Kept; the switch removed.
