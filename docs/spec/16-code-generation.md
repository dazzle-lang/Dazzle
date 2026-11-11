# 16. Code generation

## 16.1 Output layout

```
build/
  cpp/
    dz_runtime.h        the shim (§16.4), identical in every project
    dz_runtime.cpp
    engine.h            module "engine": all types, in dependency order
    engine.cpp          module "engine": all function bodies
    engine.inst.cpp     the bodies of the generic instances "engine" is first to need (§8.4)
    game.h
    game.cpp
    dz_typeinfo.cpp     the static type table (§13)
  obj/                  one object per vendored library source (§15.1), and in obj/dz/
                        one per generated file of the program's own (§16.6)
  bin/
    game.exe            the program
  test/                 `dz test`'s own build, the same shape again (§19 M14)
  bench/                and `dz bench`'s (§19 M15)
```

**`build/` holds only directories `dz` names itself**, which is why the executable is in
`bin/` rather than beside them. A module named `bench` would otherwise produce the *file*
`build/bench`, which is exactly the directory `dz bench` needs — and the build fails with
"Not a directory" on every platform whose executables have no extension.

One header and one translation unit per module. A generic instantiation's type and the
declarations of its methods go in the header of each module that *uses* it, behind an
include guard, so a `Buffer<f32>` used by three modules is one type. Its method bodies are
defined once in the whole program, in the translation unit of the first module that needs
it, the standard library first; only those written `#[inline]` (§12.8) are in every user's
header, so they can be inlined. A header then holds declarations, which every module above
it parses, rather than every body of every instance it needs.

## 16.2 Readability of the output

The generated C++ is meant to be read. Field and function names survive; only generic
instantiation and module scoping are mangled.

```dz
pub struct Transform {
    position: Vec3
    pub fn matrix(&self) -> Mat4 { ... }
}
```

```cpp
namespace dz { namespace engine {

struct Transform {
    ::dz::math::Vec3 position;
    ::dz::math::Quat rotation;
    ::dz::math::Vec3 scale;

    Transform() noexcept;
    Mat4 matrix() const;
};

}}
```

Mangling is `dz::<module>::<Name>` for plain types and
`dz::<module>::<Name>__<argmangle>` for instantiations, where `argmangle` is the
underscore-joined mangled names of the type arguments. It is deterministic, reversible, and
readable in a profiler.

## 16.3 Source mapping

Every emitted statement carries a `#line` directive pointing at the `.dz` source:

```cpp
#line 42 "/home/ada/game/src/gameplay/player.dz"
    this->health -= damage;
```

Consequences, all of which matter more than they sound: breakpoints land in `.dz` files,
call stacks show `.dz` line numbers, a C++ compiler error that slips through Dazzle's
checks points at your Dazzle source, and profilers attribute samples to Dazzle lines.
`[DECIDED]`

`dz build --no-line-directives` turns this off when you want to debug the generated C++
itself.

## 16.4 The runtime shim

`dz_runtime.h` / `.cpp` is the entire Dazzle "runtime": a few hundred lines, depending on
nothing but the C standard library.

It contains: `dz_panic` (message, `SourceLocation`, stack trace in debug), the bounds-check
helper, the arithmetic helpers (division by zero, shifts, float-to-integer conversion, and
the checked and saturating integer methods), `SourceLocation`, the `TypeInfo` structs, slice
and `&str` layouts, and the `T?` and fallible-return tag helpers.

There is no allocator in it, no I/O beyond writing a panic message to stderr, no threading,
and no global constructors. `[DECIDED]`

The standard library's own C++ — the bodies of its `extern fn`s, which read the
clocks and, later, the file system (§7.2, 18.65) — is emitted beside the shim as
`dz_native.h` / `.cpp`, and is the one file that includes the operating system's headers.
What is left of the formatting in `dz_prelude.h` is `error(…)`'s, which writes into static
storage (§9.2); everything else formats through the standard library's `Format` (§12.9).

## 16.5 Compilation flags

The generated code is required to build cleanly with:

```
-std=c++17 -fno-exceptions -fno-rtti -fwrapv -fno-finite-loops -fno-strict-aliasing -Wall -Wextra
```

Nothing Dazzle emits may need exceptions or RTTI, because console and mobile targets
routinely disable both. The other three flags define what C++ leaves undefined and safe
Dazzle could otherwise reach (§1.1):

- `-fwrapv` makes signed arithmetic wrap, as §5.1 requires; an 8- or 16-bit result is also
  cast back to its own type, since C++ computes it as an `int`.
- `-fno-finite-loops` runs a loop as written. C++ lets an optimiser assume that a loop with
  no side effects ends, and both clang++ and g++ deleted `while x < 10 {}` with `x`
  unchanged, carrying on past it.
- `-fno-strict-aliasing` reads memory reached through a pointer cast (`unsafe`, §10.6) as
  the type the pointer names, rather than letting the optimiser assume two types never
  share memory.

The flags need clang 12 or GCC 10. A vendored C or C++ library is built without them, with
the semantics it was written for. What else the emitted C++ relies on, and how each case
C++ leaves undefined is ruled out, is in the audit in `docs/testing.md`.

An optimised build (`--release`, `--shipping`) adds `-flto=auto` to Dazzle's own code, so
a call from one module into another can be inlined at link time; a vendored library keeps
its recipe's flags. A toolchain whose linker cannot do it (LLVM's clang targeting MSVC
without `lld`, for one) gets the build without it and a note saying so. `#[inline]` (§12.8)
is what inlines across modules there.

**Which compiler builds it is the project's choice, and the command's.** `dz` drives an
ordinary C++ compiler, and any of them that takes those flags will do — a console SDK's
compiler as much as `clang++` or `g++`. Four places may name one, and the nearest to the
person running the build wins: `--cxx <compiler>` on the command line, `DZ_CXX` in the
environment, `cxx` in the manifest's `[build]` table, and `cxx` in
`[build.target.<os>]` for a single platform. With none of them `dz` uses the first of
`clang++`, `g++`, `c++` on the path. A compiler named explicitly is **not** probed first:
if it is not there the build fails saying so, rather than quietly using another one.
`[DECIDED]`

```toml
[build]
cxx = "clang++"                 # what this project is known to build with

[build.target.linux]
cxx = "g++"                     # except here
```

**C++20 is supported and opt-in**, via `cpp_standard = "c++20"` in `dazzle.toml`. The
only thing it changes is readability: struct literals become designated initialisers,
`Vec3{ .x = 1, .y = 2 }`, instead of the immediately-invoked lambda C++17 forces. Runtime
behaviour and performance are identical, so C++17 stays the default for the widest
toolchain reach. `[DECIDED]`

**The access helpers are zero-overhead by construction.** `dz::obj`, `dz::addr`,
`dz::cast` and `dz::index` are marked always-inline, not merely `inline`, so they vanish
at `-O0` as well as `-O2` — a game has to stay playable in a debug build. The only call
that survives is `panic_bounds`, which is the cold failure path and should stay one. A
test compiles an indexing loop and asserts no helper call reaches the object code.

## 16.6 Incremental builds

Dazzle writes a generated file only when its contents change, and compiles each generated
`.cpp` to an object of its own, compiled again only when the file or a header it includes
has changed, judged by content rather than by time. A one-line change in one function
rewrites one `.cpp`, and the C++ compiler rebuilds one translation unit. Every file starts
from `std.h` precompiled, so that translation unit does not parse the standard library again.
The bodies of the generic instances a module is the first to need are a file of their own,
`<module>.inst.cpp`, with temporaries numbered apart from the module's own code, so an edit
to one of the module's bodies leaves that file as it was and it is not compiled again.
A module being edited, one whose file is compiled again, gets its own header precompiled
alongside that compile. That header holds every module header beneath it, and the module's
next edit compiles from it, so an edit costs about what the module's own code does.
A debug build links with `mold` or `lld` where one is installed and the compiler can use
it, since the link is then the largest part of an edit; otherwise with the system's linker.

**A build with nothing changed does nothing.** A successful `dz build` or `dz run` records
what it read, in `build/obj/dz/build-<key>.stamp`. The key is what the build was asked for:
the `dz` executable itself, the options, the project as the command line named it, and the
environment variables that change a build. A different `dz` is a different build, since it
may generate different C++ from the same source. The inputs are:
- every manifest and source file, and each source pattern expanded again, so a file added
  or removed counts;
- every file a header import read, and every file each object was compiled from, the system
  headers included;
- every CMake library's source tree;
- the C++ compiler the name resolves to;
- the executable.

The next build with the same key checks these first, by size and time, and by content
where a time is too recent to trust. When all of them hold, it prints the warnings that
build printed and stops, without analysing anything; `dz run` runs the program.

**The runtime and `std` are compiled once for every project.** Their generated files are the
same in every program, so they, their objects and the precompiled `std.h` live in a per-user
cache rather than in `build/`: `$XDG_CACHE_HOME/dazzle` or `~/.cache/dazzle` on Linux,
`~/Library/Caches/dazzle` on macOS, `%LOCALAPPDATA%\dazzle` on Windows, or wherever
`DZ_CACHE_DIR` names. A new project, or a build from clean, starts with them already built,
as a C++ program starts with its standard library already built. Each folder in it is named
for the contents of those files, so a setting that changes them (`--track-memory`,
`check_trusted`) gets a folder of its own and a stale one is never used. The folders a
build has not used for a while are removed. `build/cpp` still holds a copy of every file,
so it is the whole program's C++, and the `#line` directives that name `std`'s sources
(§16.3) name copies in the cache.

The same cache keeps what each import of a C or C++ header produced (§15.1), so a build does
not parse `stdlib.h`, or a library's headers, again while none of the files they read has
changed.
