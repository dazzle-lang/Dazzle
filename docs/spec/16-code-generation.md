# 16. Code generation

## 16.1 Output layout

```
build/
  cpp/
    dz_runtime.h        the shim (§16.4), identical in every project
    dz_runtime.cpp
    engine.h            module "engine": all types, in dependency order
    engine.cpp          module "engine": all function bodies
    game.h
    game.cpp
    dz_typeinfo.cpp     the static type table (§13)
  obj/                  one object per vendored library source (§15.1)
  bin/
    game.exe            the program
  test/                 `dz test`'s own build, the same shape again (§19 M14)
  bench/                and `dz bench`'s (§19 M15)
```

**`build/` holds only directories `dz` names itself**, which is why the executable is in
`bin/` rather than beside them. A module named `bench` would otherwise produce the *file*
`build/bench`, which is exactly the directory `dz bench` needs — and the build fails with
"Not a directory" on every platform whose executables have no extension.

One header and one translation unit per module. Generic instantiations are emitted into the
module that *uses* them, deduplicated by mangled name with include guards, so a
`Buffer<f32>` used by three modules is defined once.

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
#line 42 "D:/OdinProjects/Dazzle/game/src/gameplay/player.dz"
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
helper, the overflow-check helpers, `SourceLocation`, the `TypeInfo` structs, slice and
`str` layouts, and the `T?` and fallible-return tag helpers.

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
-std=c++17 -fno-exceptions -fno-rtti -fwrapv -Wall -Wextra
```

Nothing Dazzle emits may need exceptions or RTTI, because console and mobile targets
routinely disable both. `-fwrapv` makes signed arithmetic wrap, as §5.1 requires; an 8- or
16-bit result is also cast back to its own type, since C++ computes it as an `int`. A
vendored C or C++ library is built without `-fwrapv`, with the semantics it was written for.

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

Dazzle writes a generated file only when its content hash changes. That way the C++
compiler's own incremental build does the right thing without Dazzle having to model object
files. A one-line change in one function rewrites one `.cpp`, and the C++ compiler rebuilds
one translation unit.
