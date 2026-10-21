# 15. C++ interoperability

This is a headline feature, not an afterthought. Because Dazzle *emits* C++, calling C++ is
a direct call in the generated source — no wrappers, no shims, no ABI marshalling, no
overhead. The importer's only job is to teach the type checker what the header contains.

## 15.1 Importing a header

A C or C++ library is **vendored as source and built by `dz`** — no CMake, no Makefiles,
no binaries to find for each platform. Its recipe says which files to compile and how, and
lives in the `dazzle.toml` of the module that uses it:

```toml
[cpp.libs.raylib]
root         = "vendor/raylib"          # the paths below are relative to this
sources      = ["src/rcore.c", "src/rshapes.c", "src/rtextures.c", "src/rtext.c",
                "src/rmodels.c", "src/raudio.c", "src/utils.c"]
include_dirs = ["src"]
defines      = ["GRAPHICS_API_OPENGL_33"]
import       = ["raylib.h"]              # what Dazzle code sees: see below

[cpp.libs.raylib.target.windows]        # added to the above when building for Windows
sources   = ["src/rglfw.c"]
defines   = ["PLATFORM_DESKTOP"]
link_libs = ["opengl32", "gdi32", "winmm"]

[cpp.libs.raylib.target.macos]          # a macOS system library is a framework
frameworks = ["Cocoa", "IOKit", "CoreVideo", "OpenGL"]

[cpp.libs.rgfw]                         # a single-header library: no source to write
root         = "vendor/rgfw"
header       = "RGFW.h"
include_dirs = ["."]
defines      = ["RGFW_IMPLEMENTATION"]

[cpp.libs.jolt]                         # a library that ships binaries instead
include_dirs = ["vendor/jolt/include"]
lib_dirs     = ["vendor/jolt/lib"]
link_libs    = ["Jolt"]
import       = ["Jolt/Physics/Body/Body.h"]

[cpp]                                   # for everything, not tied to one library
defines = ["JPH_PROFILE_ENABLED=0"]
```

- **`sources`** are paths or globs. A glob skips what is not `.c`, `.cc`, `.cpp` or `.cxx`;
  a path written out must be one. `.c` files are compiled as C. Each source becomes one
  object under `build/obj/`, rebuilt only when the source, a header it includes, or its
  flags change — judged by content, as generated C++ is (§16.6).
- A library is compiled with **its own** `include_dirs` and `defines`, and with its
  module's `[cpp]` ones, at the build's optimisation level, with warnings off: its warnings
  are not the project's to fix.
- **Every path is relative to the manifest**, or to the library's `root` when it has one.
- **`[cpp.target.<os>]`** and **`[cpp.libs.<name>.target.<os>]`** add to their base table
  when building for that target — `windows`, `linux`, `macos` or `web`. Every section is
  checked on every platform, so a recipe that is wrong for another platform says so here.
  A system library belongs in one of these and not in the base table: `opengl32` exists
  only on Windows, and naming it at the top makes a recipe that cannot link anywhere else.
- **`link_libs`** are passed as `-l<name>`. **`frameworks`** is the macOS counterpart,
  passed as `-framework <name>`, because a macOS system library is a framework and `-l`
  does not find one. It belongs in a `target.macos` section, and is ignored elsewhere.
- **`flags`** are passed to the compiler for this library's sources, after the ones `dz`
  chooses — so a recipe can override one, which is the point. It is the escape hatch for
  what a define cannot express: raylib's `rglfw.c` includes GLFW's Cocoa backend, so on
  macOS that translation unit is Objective-C despite its `.c` name and needs
  `flags = ["-x", "objective-c"]`. A **library's** sources only: Dazzle's own output must
  compile under the fixed set above, and a manifest does not get to weaken that.
- **A module's include dirs** — its `[cpp]`'s and every vendored library's — and **the
  defines of its `[cpp]`** apply to its own generated C++, to the headers it imports, and to
  every module that depends on it. A header means what the shared defines say, so everything
  that includes it has to agree; it is also what lets a Dazzle module that wraps a library
  carry the recipe for its users. Two recipes for one library are an error.
- **A library's own `defines` build its sources and nothing else.** A single-header library
  puts its implementation behind one — `RGFW_IMPLEMENTATION`, `STB_IMAGE_IMPLEMENTATION` —
  and it must reach exactly the source that compiles it, or every symbol would be defined
  again in each translation unit that includes the header. A define that has to hold on both
  sides, such as the `SHLDEF=` that gives a header-only library's declarations external
  linkage, goes in `[cpp]`.
- **`header`** names a single-header library, and replaces `sources` for one. Such a library
  needs exactly one translation unit that includes its header with the implementation define
  in force, and nothing else about that file is interesting, so `dz` writes it rather than
  making you keep a one-line `.c` beside the header. It is compiled as C for a `.h` header
  and as C++ otherwise. Forgetting to write that file by hand produces an unresolved symbol
  at link time, which says nothing about what is actually missing — this is the same recipe
  without that trap.
- A library that needs a configure step — a generated `config.h` — cannot be described this
  way. Ship its binaries through `lib_dirs` and `link_libs`, or generate the file once and
  vendor it. `[DECIDED]`

- **`import`** lists the headers Dazzle code sees of the library: their declarations become
  a module of their own, which the manifest names like any dependency, `rl = { cpp =
  "raylib" }` (§3.2). A header is found as `#include <raylib.h>` would find it, through the
  include dirs, and the generated C++ includes it by that same name. A library whose API
  is spread over several headers lists them all — `import = ["raylib.h", "raymath.h"]` —
  and they are read together, as one module under one key. A library with no `import` is
  built and linked and nothing more. A system library needs no sources, only its link names
  and the header: `[cpp.libs.opengl]` with `import = ["GL/gl.h"]` under `target.linux`.
  There is no `import cpp` in a source file: a header is named once, beside the recipe
  that builds it, so renaming or replacing a library touches one table. `[DECIDED —
  §18.57]`

```toml
[dependencies]
rl   = { cpp = "raylib" }
jolt = { cpp = "jolt" }
```

```dz
fn main() {
    rl::InitWindow(1280, 720, "Dazzle")
    defer rl::CloseWindow()

    while !rl::WindowShouldClose() {
        rl::BeginDrawing()
        rl::ClearBackground(rl::RAYWHITE)
        rl::DrawText("hello", 16, 16, 20, rl::DARKGRAY)
        rl::EndDrawing()
    }
}
```

The compiler runs **libclang** over the header with the configured include paths and
defines, walks the resulting AST, and materialises Dazzle-visible declarations. The
generated C++ contains `#include "raylib.h"` and calls `InitWindow(1280, 720, "Dazzle")`
directly.

A header's module opens like any dependency (§3.2): `rl = { cpp = "raylib", open = true }`
lets the same program write `InitWindow(…)` and `RAYWHITE` bare, with `rl::` still available
where a name needs telling apart. A C library's names usually carry their own prefix, which
is what makes this reasonable; a header like `windows.h`, with its thousands of macros, is
better left behind a name.

Import results are cached, keyed on the header's content hash plus the flags, so the
libclang pass runs once and not on every build.

## 15.2 What maps, and how

| C / C++ | Dazzle |
|---|---|
| `int`, `unsigned`, `float`, … | `i32`, `u32`, `f32`, … (exact width resolved by libclang) |
| `size_t`, `uintptr_t` / `ptrdiff_t`, `intptr_t` | `usize` / `isize` |
| `T*` | `^mut T` |
| `const T*` | `^T` — read-only, as in C (§18.37) |
| `T&` | `&mut T` |
| `const T&` | `&T` |
| `T&&` | not imported: `&&` is C++'s spelling of a move, and §6.4 has its own |
| `const char*` | `cstr` |
| `struct` / `class` | `struct`, public fields mapped, private and protected C++ members become opaque padding — they stay in the type, since Dazzle never redeclares it, but cannot be named. A field whose type Dazzle cannot name yet is left out the same way. Every field defaults to zero, as `{}` makes it, so a literal names only the fields it sets |
| `typedef` of a struct or enum | a second name for the same type |
| a `struct` the header only declares | a **handle**: used behind `^T` and `^mut T`, never by value (§18.40) |
| C `union` | a type whose members are the same memory, as in C; a literal sets one of them (§18.41) |
| an array member, `float m[16]` | `[16]f32` — indexed, iterated, `.len`, `.ptr`, bounds-checked. A whole one is not a value: C cannot copy an array either (`DZ0707`) |
| a bit-field, `unsigned on : 1` | an ordinary member to read and assign. It has no address, so `&` on one is refused (`DZ0706`) |
| a function pointer, `void (*)(const RGFW_event *)` | `fn(^RGFW_event)` (§5.4): a Dazzle function is handed to C as a callback |
| member function | method — `const` takes `&self`, any other `&mut self`. A `static` one needs associated functions, which Dazzle does not have yet, and is left out with that reason |
| constructor | a constructor call, `Counter(10)` |
| destructor | **C++ runs it.** Dazzle emits no `drop` for an imported type; the type is move-only instead, so it is never destroyed twice |
| C++ copy constructor | what makes such a type copyable again: a class with a destructor *and* a public copy constructor is an ordinary value |
| `namespace A::B` | nested scope: `ns::A::B::thing`. An `inline` namespace adds no segment, as in C++ |
| `enum` | `enum`, and each enumerator also an untyped integer constant at the header's scope (§18.38) |
| `enum class` | `enum`, its enumerators reached only through it |
| overload set | see §15.3 |
| `virtual` method | callable normally; Dazzle types cannot inherit from it |
| template | not imported unless explicitly instantiated (§15.4) |
| macro `#define X 1` | constant, emitted by its own name: an integer is untyped, a float `f32` when written with `f` and untyped otherwise (§18.38). A macro whose value is another expression clang can type keeps that type — raylib's `RAYWHITE` is a `Color` |
| function-like macro | not imported |
| variadic `...` | not imported; use a fixed-arity overload |

The importer only teaches the compiler what exists. **A call into a header is ordinary safe
code**, emitted as the call it names (`::InitWindow(1280, 720, "Dazzle")`): what the callee
does with a pointer is the header's promise, which Dazzle can neither check nor improve on
by making every call `unsafe`. Forming the pointer is safe as always (§5.2); a nicer API is a
Dazzle module wrapping the header. What cannot be named in Dazzle yet — a variadic function,
a union, a function pointer — is left out of the import rather than guessed at.

An imported type with a non-trivial C++ destructor is **move-only in Dazzle** unless it also
has a copy constructor, which makes C++ RAII types obey Dazzle's rules automatically.

Imported types can be marked owning at the use site:

```dz
struct Texture {
    handle: own rl::Texture2D
}

impl Drop for Texture {
    fn drop(&mut self) { rl::UnloadTexture(handle) }
}
```

## 15.3 Overloads

Dazzle has no function overloading (§7.2), but C++ headers are full of it. Imported
overload sets are resolved by argument types at the call site, using exact matches only —
no implicit conversions, no promotion, no SFINAE.

An overloaded **member function** resolves this way today: `counter.add(5)` and
`counter.add(point)` reach the two `add`s of one C++ class. An overloaded **free function**
does not yet — the header's second declaration of a name is passed over, and the first is
the one imported.

When two overloads both match exactly, or none does, the error lists the candidates with
their parameter types and asks you to disambiguate:

```dz
rl::DrawTextureEx@(Texture2D, Vector2, f32, f32, Color)(tex, pos, 0, 1, tint)
```

The `@(...)` suffix names a specific signature. It is rarely needed and exists so that no
header is ever un-callable. `[PROPOSED]`

## 15.4 Templates

Templates are not mapped onto Dazzle generics. Instead, you name the instantiations you
want, and they are imported as ordinary concrete types:

```toml
[cpp.instantiate]
"std::vector<float>"           = "StdVecF32"
"JPH::Array<JPH::BodyID>"      = "BodyIdArray"
```

```toml
[cpp.libs.stdvector]
import = ["vector"]

[dependencies]
cppstd = { cpp = "stdvector" }
```

```dz
let v: cppstd::StdVecF32
```

The Dazzle name is a name of the header's module, whatever namespaces the C++ type sits in;
the generated C++ writes the instantiation itself. The type's members are the template's,
with its parameters bound to the arguments named here — so `Pair<int>`'s `sum()` returns
`i32`. A member whose type does not follow from that binding (`typename T::value_type`) is
left out with its reason, like any other unmappable declaration.

This covers the real cases (a handful of instantiations per project) without the enormous
complexity of unifying two different generic systems. `[DECIDED]`

## 15.5 Exporting Dazzle to C++

```dz
#[export]
pub fn dazzle_update(dt: f32) { ... }

#[export(c)]
pub fn dz_init() -> i32 { ... }     // extern "C", no mangling
```

`#[export]` emits the declaration into the module's generated header with a stable,
documented name so an existing C++ codebase can call into Dazzle — the path for adopting
Dazzle inside an engine you already have.
