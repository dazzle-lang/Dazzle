# 3. Projects, modules, and imports

**A module is a project. A project is a module.** Folders are organisation only; the
compiler never infers anything from directory layout. `[DECIDED]`

This is the C#/CMake model, not the Odin/Go one. A project is declared by a `dazzle.toml`
at its root, which names the module and lists the files belonging to it.

## 3.1 The project file

```toml
[module]
name    = "engine"
version = "0.3.0"

# Globs, relative to this file. Order is irrelevant: the compiler builds the whole
# module's symbol table before checking any of it.
sources = ["src/**/*.dz"]
exclude = ["src/platform/ps5/**"]

[dependencies]                     # what every file of this module can name (§3.2)
std    = { open = true }                                     # String, Vec2 — bare
math   = { path = "../libs/math" }                           # math::pow
raylib = { path = "../libs/raylib", flags = ["NO_AUDIO"] }   # its flags this turns on (§12.10)
rl     = { cpp = "raylib" }                                  # a C library's headers (§15.1)

[names]                            # a bare name two opened dependencies both export (§3.2)
Vec2 = "std"

[flags]                            # this package's build flags, read by `#if` (§12.10)
PROFILE = { default = false, doc = "time every frame" }
EDITOR  = false

[build]
cpp_standard = "c++17"
output       = "static-lib"   # "executable" | "static-lib" | "shared-lib"
optimize     = "debug"        # "debug" | "release" | "shipping"
type_info    = "used"         # "none" | "used" | "all"   (§13)
track_memory = false          # the allocators' tracking (§14.2); the root's counts

[cpp.libs.raylib]                  # a C or C++ library, vendored as source (§15.1)
root         = "vendor/raylib/src"
sources      = ["rcore.c", "rshapes.c"]
include_dirs = ["."]
import       = ["raylib.h"]        # the headers Dazzle code sees of it
```

A workspace has a root project depending on sibling projects, conventionally in `libs/`:

```
game/
  dazzle.toml            module "game", executable
  src/
    main.dz
    gameplay/            <- just a folder. No meaning to the compiler.
      player.dz
      enemy.dz
    render/
      sprite_batch.dz
  libs/
    engine/dazzle.toml   module "engine"
    math/dazzle.toml     module "math"
```

Every file in `game/src/**` is part of module `game` and can see every other file's
module-visible declarations without importing anything. Moving `player.dz` from
`gameplay/` to `entities/` changes nothing anywhere. That is the entire point.

## 3.2 Imports

**What a module can name is declared once, in its `dazzle.toml`, and holds in every one of
its files.** There is no `import` line to repeat at the top of each file, and no way for one
file to call a library `rl` while the next calls it `raylib`. `[DECIDED — §18.57]`

```toml
[dependencies]
std    = { open = true }                        # String, List, Vec2 — and std::Vec2
engine = { path = "libs/engine", open = true }  # Transform, Mesh — and engine::Transform
rl     = { path = "libs/myraylib" }             # rl::draw_line
GL     = { cpp = "opengl" }                     # GL::DrawArrays
GLES   = { cpp = "gles" }                       # GLES::DrawArrays
```

- **The key is the name.** Every file of the module reaches a dependency's exports through
  it: `rl::draw_line`, `GL::DrawArrays`. Renaming a library, or swapping one for another, is
  one line.
- **`open = true` also puts its exports at the root**, so they are written bare. The key
  still works beside them. A game opens what it uses on every line — the standard library,
  its engine — and keeps a C API or two platform backends behind a name.
- **`std` is a dependency like any other** (§14.4). It needs no `path`, but it must be
  listed: a module that does not list it cannot name `String` or `Vec2` at all, and one that
  lists it without `open` writes `std::String`. The language's own constructs — `for`, `or`,
  operators, `println!` — work either way, because they reach the standard library without
  naming it. `dz new` writes `std = { open = true }`.
- **Only a dependency's `pub` declarations are reachable**, and never what *it* depends on,
  unless it says so (below).

**A bare name** is looked for in this order, and the first place that has it wins:

1. locals and generic parameters;
2. the module's own declarations, in any of its files;
3. a `{ … }` import in this file (below);
4. the module's `[names]` choices;
5. the opened dependencies — and here, **two different declarations of the name are an
   error** (`DZ0310`), reported on the line that writes it, not at the manifest. There is no
   precedence among dependencies, by order or otherwise: `let a = Vec2{}` with two opened
   dependencies exporting `Vec2` does not compile until it is written `std::Vec2`, or the
   module chooses one:

```toml
[names]
Vec2 = "std"      # a bare `Vec2` is std's, in every file of this module
```

A dependency that adds a name the module already declares changes nothing, since its own
declarations come first; one that adds a name another opened dependency exports breaks only
the lines that write that name bare. Two names for one declaration — a C `typedef` — are not
a conflict. `[DECIDED — §18.57]`

A dependency nothing in the module names — no path through its key, no import, no opened
name, no `[names]` choice or `via` — is a warning on its manifest line (`DZ0313`). A public
one is part of the module's interface whether or not the module uses it, and is not.

**Public dependencies.** A dependency is private: the module can use it, and modules that
depend on *this* one cannot see it. `pub = true` makes it part of the module's interface:

```toml
# libs/myraylib/dazzle.toml
[dependencies]
std    = { open = true }
native = { cpp = "raylib" }                 # private: only this module writes native::
shapes = { path = "../shapes", pub = true } # public: dependents reach it as rl::shapes
```

A public dependency stays under its own name, one level inside the module that exports it —
`rl::shapes::Rectangle` in a module that calls `myraylib` `rl` — rather than merging into its
names: a wrapper usually reuses the names of what it wraps, and merging would collide them.
A dependent may give it a name of its own, and open it, like any dependency:

```toml
rlshapes = { via = "rl::shapes", open = true }
```

**A private dependency cannot appear in a library's `pub` signature** (`DZ0311`) — a `pub`
function's parameters and return, a `pub` struct's `pub` fields and methods, a `pub`
constant's or static's type. `pub fn bounds() -> native::Rectangle` in `myraylib` would hand
its users a value whose type they cannot write; the fix is to mark `native` `pub`, or to
return a type of the module's own. An executable has no dependents, so the rule does not
apply to one. `[DECIDED — §18.58]`

**`import` in a file** is the exception, for when one file wants something the manifest
does not give it. Two forms, each visible only in the file that writes it:

```dz
import phys::{ Vec3, Body }     // a bare Vec3 is phys's in this file, even if std opens one
import engine::render as gfx    // a shorter name for a path, in this file
```

The path starts at a name the module already has — a dependency key, or a public
dependency inside one (`rl::shapes`). An import cannot reach a module the manifest does not
list. `[DECIDED — §18.57]`

**`::` is the only scope separator, everywhere.** A dependency, a public dependency inside
it, an enum member, and an associated function all use it, so `rl::shapes::Rectangle::new`
reads uniformly from the outside in and there is never a question of which separator
applies at which level.

A module's `name` in `[module]` identifies the project — in error messages, in `via`, in
the generated C++ — and is not what code writes; code writes the key a dependent chose.

Circular dependencies between modules are an error. Circular references *within* a module
are fine — declaration order never matters inside a module. `[DECIDED]`

## 3.3 `::` versus `.`

`::` is the compile-time operator: its left side is always a name known at compile time — a
module, a type, an enum, a union. `.` is the runtime operator; its left side is always a
value.

```dz
math::PI                        // module member
Sprites::Selected               // enum member
Allocators::Global              // associated constant of a type
T::is_trivially_copyable        // compile-time type query
Buffer<f32>::with_capacity(8)   // associated function of an instantiation

buf.length()                    // method on a value
p.field                         // auto-derefs through ^ and & as deep as needed
```

There is no `->`. `.` dereferences pointers automatically. `[DECIDED]`

**There is no turbofish.** Generic arguments are written the same way in expressions as in
types — `Buffer<f32>::with_capacity(8)`, not `Buffer::<f32>::…`. Rust needs the extra `::`
because `a < b > (c)` is a valid expression there; in Dazzle comparison is
**non-associative** (§11.1), so that is already a syntax error and the ambiguity does not
exist. `[DECIDED]` The same goes for a method's own arguments after `.`:
`list.map<f32>(to_float)` (§8.1), taken as type arguments only when a call follows the `>`,
so `n.len < max` is still a comparison.

The one residual case is an argument list: `f(a<b, c>(d))` could be two comparisons or one
generic call. The parser prefers the **generic reading**, and the comparison meaning is
written `f((a < b), (c > (d)))`. `[PROPOSED]`
