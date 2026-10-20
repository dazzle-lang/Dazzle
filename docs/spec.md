# The Dazzle Language Specification

**Version:** 0.1 — **unreleased**, revision 62
**Status:** M0–M8, M11, M14 and M15 implemented, and most of M9 and M12 — Dazzle programs
compile and run, own, move and drop their resources deterministically, have generics,
traits, unions and reflection, call C and C++ libraries that `dz` builds from vendored
source, and are edited with a language server. Not yet 0.1: the editor still lacks code
actions and incremental analysis (M9), and the standard library, though it has carried a
game written without C++ (M12's criterion), still has `println` as a builtin until macros.
See `docs/PROGRESS.md`.

**The version number stays 0.1 until 0.1 is actually finished.** 0.1 means: an LSP good
enough to work in (§19 M9), a stdlib you can write a game against (M12), and C++ interop
(M7). Until all three land, this document is revised, not versioned — the revision numbers
below track *changes to the language*, and none of them is a release.
**Target:** a systems language for game development that compiles to readable C++17.

---

## 0. How to read this document

This is a *design* document, not a description of a working compiler. It is meant to be
argued with. Three markers appear throughout:

- **`[DECIDED]`** — settled. Implementation may proceed against it.
- **`[PROPOSED]`** — my recommendation, written as if decided so you can judge it in
  context. Reject freely; nothing depends on it yet.
- **`[OPEN]`** — a genuine fork that needs your call. All of them are collected in §18.

Every code sample here is intended to be a valid Dazzle program (or a deliberately invalid
one, marked as such). They double as the compiler's first test corpus.

### 0.1 The whole language on one page

If you read nothing else, read this. Everything after it is detail.

```dz
// no imports: dazzle.toml names what every file can use — `std`, `engine`, `rl` (§3.2)

pub struct Buffer<T> {                // private by default; pub / pub(module) / (nothing)
    m_allocator: ^Allocator = null    // ^T  nullable pointer
    m_data:      own ^mut T           // own ^mut T  this struct frees it -> Buffer is move-only
    m_len:       u32                  // fields default-initialise; no uninitialised memory

    Buffer(cap: u32, a: ^Allocator = Allocators::Global) { ... }   // constructor

    pub fn length(&self)      -> u32 { return m_len }       // &self     = reads
    pub fn resize(&mut self, n: u32) { ... }                // &mut self = mutates
}

impl<T> Drop for Buffer<T> {                    // destructor -> deterministic, C++ handles it
    fn drop(&mut self) { ... }
}

impl<T: Copy> Copy for Buffer<T> {              // copy constructor -> copyable when T is
    fn copy(&mut self, other: &Self) { ... }
}
```

| Concept | Syntax | One-line rule |
|---|---|---|
| Refer to a value | `&T` `&mut T` `^T` `^mut T` `own ^mut T` | `&`/`&mut` are non-null and **stack-only**; `^` is a C pointer, read-only unless `^mut`; `own ^` means you free it; `^T` → `&T` needs an `or` clause, `&T` → `^T` is free, and reading or writing *through* a pointer is `unsafe` |
| Copy vs move | `let b = a` / `let b = move(a)` | if the type owns anything, copying needs an `impl Copy`; otherwise `move()` |
| Absence | `T?` | postfix, binds to the type name. `^T?` is a pointer to an optional; an optional pointer is just `^T` |
| Failure | `fn f(…) !-> T` | `!` marks the *signature*, never a type. There is no `T!` to store, annotate, or point at |
| Handle anything fallible | `let v = f() or { e -> … }` | one clause for `T!`, `T?`, and `^T` alike |
| Propagate | `f() or return` | only where the enclosing signature can carry the failure |
| Fall back | `f() or Textures::Missing` | "on failure use this instead" — the common case in game code |
| Sum type | `union S { A{…} B{} C }` + `switch` | tagged, exhaustive, no fallthrough; bare `C` carries `struct C` |
| Interface | `trait X` + `impl X for Y` | bound `<T: X>` = static dispatch; `^X` = fat pointer, object stays the same size; an `impl` lives with the trait or with the type, so a module can add a trait to a type it did not write |
| Generics | `fn f<T: Ord>(…)` | checked once at the definition, then monomorphized by Dazzle |
| Module | one `dazzle.toml` = one module | folders mean nothing; `name = "engine::render"` for nesting |
| Metaprogramming | `macro m -> declaration { … }` | declarative pattern → expand, so the LSP can complete inside it |
| Reflection | `typeinfo_of<T>()` | one static table; your structs carry zero extra bytes |
| Compiles to | C++17, `-fno-exceptions -fno-rtti` | with `#line` back to `.dz`, so debuggers and profilers show your source |

**The five rules that shape everything else:** private by default · no implicit numeric
conversions · every value is initialised · moved-from values reset to defaults · `&`
references can never be stored in a field (this is why there are no lifetimes).

---

## 1. Design principles

Dazzle exists because of a specific complaint: C++ gives you the control and the
abstraction but drowns you in forty years of accumulated syntax and undefined behaviour;
Odin gives you the clarity but takes away encapsulation, RAII, and the ability to build
layered abstractions that hold up in a large codebase.

The principles, in priority order. When two conflict, the earlier one wins.

1. **You can always see the machine.** No hidden allocations, no hidden control flow, no
   garbage collector, no runtime you did not ask for. If a line of Dazzle allocates, it
   says so.
2. **Destruction is deterministic and automatic.** RAII, but with exactly three special
   member functions instead of C++'s five-and-a-half: a constructor, `copy`, and `drop`.
3. **Encapsulation is enforced, not conventional.** Private is the default. The compiler,
   not a naming convention, is what stops layer N+1 from reaching into layer N's guts.
4. **Errors point at your code.** Definition-site generic checking, monomorphization under
   our control rather than C++'s, and `#line`-mapped output all exist so that a mistake in
   a generic function is reported in *that function*, not in a 400-line instantiation
   trace.
5. **The escape hatch is a door, not a hole.** C++ interop is a first-class feature with a
   real header importer, not an `extern "C"` afterthought. You should be able to use
   raylib, Jolt, and Dear ImGui on day one.
6. **Compile fast.** A game programmer recompiles hundreds of times a day. Any design
   decision that trades compile time for cleverness loses.

### 1.1 Non-goals

- **Memory safety as a guarantee.** Dazzle is not Rust. It prevents use-after-move and
  double-free; it does not prevent dangling references. What it does instead is *mark* the
  places a dangling pointer can bite: every read or write through a raw pointer sits inside
  `unsafe` (§5.2), where a reader — or `grep` — can find it. `[DECIDED]`
- **A garbage collector, green threads, or any implicit runtime.**
- **Source compatibility with C++.** We *call* C++; we are not a superset of it.
- **Being small.** Odin is small, and that is the problem being solved here.

---

## 2. Lexical structure

The existing Odin lexer already implements much of this; it will be ported to Rust (§17)
with the rules below as its specification.

### 2.1 Source encoding

Source files are UTF-8. A UTF-8 BOM is permitted and stripped. UTF-16 files are rejected
with a diagnostic telling the user to convert. `\r\n` and lone `\r` are normalised to `\n`
before lexing, so column numbers are always measured in the normalised text. `[DECIDED]`
File extension: `.dz`.

### 2.2 Terminators

Dazzle is newline-terminated. A newline emits a `TERMINATOR` token, as does `;`.
Consecutive terminators collapse into one, and terminators are suppressed:

- immediately after an infix or prefix operator (`+`, `=`, `,`, `&&`, …),
- immediately after an opening `(`, `[`, or `{` that begins an expression or argument list,
- immediately before a closing `)` or `]`.

That is what lets a call wrap across lines without a continuation character:

```dz
m_data = m_allocator.realloc(m_data as ^mut u8,
                             m_capacity * sizeof(T),
                             new_size * sizeof(T),
                             alignof(T)) as ^mut T
```

Semicolons are legal but never required, and `dz fmt` removes them. `[PROPOSED]`

### 2.3 Comments

```dz
// line comment
/* block comment /* which nests */ still a comment */
/// doc comment, attaches to the next declaration, surfaced by the LSP on hover
```

Nested block comments are supported. `[DECIDED]`

### 2.4 Identifiers

An identifier begins with a Unicode `XID_Start` character or `_`, and continues with
`XID_Continue` or `_`. Identifiers are compared by exact code points; no normalisation is
performed, so `aá` written two different ways are two different identifiers. The formatter
warns on non-NFC identifiers. `[PROPOSED]`

### 2.5 Literals

**Integers.** `42`, `1_000_000`, `0xDEAD_BEEF`, `0o755`, `0b1010_1010`. Underscores are
separators and may not lead or trail. A malformed prefix (`1x42`, `0xx35`) is a diagnostic,
not two tokens.

**Floats.** `3.14`, `1_000.5`, `1e9`, `2.5e-3`. A second `.` in a numeric literal
(`4.535.346`) is a diagnostic that reports the full malformed span rather than silently
lexing a member access — this is already what the Odin lexer's `has_multiple_dots` flag
does, and it should be preserved.

**Suffixes.** `10i8`, `20u32`, `1.5f32`, `1.5f64`, plus the shorthand `1.5f` for `f32`.
A suffix **is** the literal's type, wherever it is written: `let n = 5u8` is a `u8`,
`300u8` does not fit, and `5u8` where a `u16` is wanted is a mismatch rather than a
conversion. An integer with a float suffix (`2f32`) is a float. `[DECIDED]`

**Default types.** An unsuffixed integer literal is `i32`; an unsuffixed float literal is
`f64`. `[DECIDED]`

This default applies **only where there is no context to type the literal**, which in
practice is rare. `let x = 1.5` is `f64`, but `pos.x = 1.5`, `lerp(a, b, 0.5)` and
`let t: f32 = 1.5` are all `f32`, because the literal is untyped until the surrounding
expression types it (below). So the `f64` default costs an `f32`-heavy codebase almost
nothing while matching what every other language does.

Literals are *untyped* until they meet a context that types them, so `let x: u64 = 5` and
`let y: f32 = 1` are both fine and neither is a cast. Overflow at the point of typing is a
compile error: `let b: u8 = 300` does not compile.

**Characters.** `'a'`, `'\n'`, `'\u{1F600}'`. A `char` is a Unicode scalar value, 32 bits.

**Strings.** `"hello"` with escapes `\n \r \t \\ \" \0 \xNN \u{...}`. A newline inside a
non-raw string literal is a diagnostic, and the lexer must recover by ending the string at
the line break rather than swallowing the rest of the file — that is the bug the current
`main.dz` sample is exercising.

Raw strings use backticks: `` `C:\path\no\escapes` ``. Multi-line raw strings are allowed.
`[PROPOSED]`

Interpolation is *not* part of the string literal. `println("Hello {}", name)` uses
compile-time format-string checking instead (§12.9). `[PROPOSED]`

### 2.6 Operators and punctuation

```
+   -   *   /   %          arithmetic
+=  -=  *=  /=  %=         compound assignment
==  !=  <   >   <=  >=     comparison
&&  ||  !                  logical
&   |   ^^  ~   <<  >>     bitwise   (^^ is xor; ^ is reserved for pointers)
&   &mut                   reference formation
^                          pointer type / dereference
=                          assignment
.                          member access (auto-derefs through ^ and &)
::                         scope access (module, enum, associated item, type query)
..  ..=                    exclusive / inclusive range
->                         return type
?                          postfix optional-type suffix (§5.6)
!->                        fallible return marker, in a function signature only (§9)
or                         failure clause (§9.4)
#[...]                     attribute
#                          compiler intrinsic / macro invocation
$                          macro variable
_                          discard binding
```

`^` is the most contested character in the grammar: it is the pointer sigil (`^T`, `p^`),
which means it cannot also be xor. Xor is therefore `^^`. Odin makes the same trade and it
causes no practical friction. `[DECIDED]`

`?` is postfix and binds to the type name, so access sigils always sit outside it: `^T?`
is a pointer to an optional `T`. This is the one place the language reads right-to-left,
and it is deliberate — access describes how you reach a value, `?` describes the value.
`!` never appears in a type, so `^T!` is not a parse problem; it is not a thing. `[DECIDED]`

### 2.7 Keywords

```
as       break     case     const    continue  defer     else     enum
extern   false     fn       for      if        ignore    impl     import
in       let       macro    module   move      mut       none     null
or       own       pub      return   self      Self      struct   switch
static   trait     true      unreachable       union     unsafe    while
```

`copy` and `drop` are not keywords: they are the methods of the `Copy` and `Drop` traits
(§6.1), and ordinary names otherwise.

Reserved for future use: `async`, `await`, `impl`, `where`, `yield`, `comptime`, `collect`.

---

## 3. Projects, modules, and imports

**A module is a project. A project is a module.** Folders are organisation only; the
compiler never infers anything from directory layout. `[DECIDED]`

This is the C#/CMake model, not the Odin/Go one. A project is declared by a `dazzle.toml`
at its root, which names the module and lists the files belonging to it.

### 3.1 The project file

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

### 3.2 Imports

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
  operators, `println` — work either way, because they reach the standard library without
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

### 3.3 `::` versus `.`

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

---

## 4. Visibility

Four levels. Default is private. `[DECIDED]`

| On | `private` (default) | `protected` | `pub(module)` | `pub` |
|---|---|---|---|---|
| Top-level declaration | visible within its module | not allowed | same as private | exported from the module |
| Struct field | visible only inside the struct's own methods | also inside the methods of the structs that inherit it | visible anywhere in the module | visible to importers |
| Method | visible only inside the struct's own methods | also inside the methods of the structs that inherit it | visible anywhere in the module | visible to importers |
| Enum / union variant | always as visible as its type | — | — | — |

**`protected` is a different axis from the other three**, which is why it is spelled on its
own rather than as a `pub(…)` form. `private`, `pub(module)` and `pub` widen by *where the
code is written* — this type, this module, anywhere. `protected` widens by *what the type
is*: a struct that inherits from this one may reach the member wherever it is written, and
a struct that does not may not, however close it sits. It exists because of inheritance
(§7.3) and means nothing without it, so it is refused on a top-level declaration, where
nothing can inherit.

Note the deliberate asymmetry: at top level, `private` and `pub(module)` mean the same
thing, because the module *is* the encapsulation boundary and there is no file-level
privacy. `pub(module)` earns its keep on **struct members**, where it expresses "the rest of
this engine may touch this field, but nobody who imports the engine may."

```dz
pub struct SpriteBatch {
    m_verts:                List<Vertex>     // private: only SpriteBatch's own methods
    pub(module) m_gl_handle: u32             // the rest of the engine may poke this
    pub         count:       u32             // anyone may read

    pub fn draw(&mut self, s: &Sprite) { ... }
    fn flush(&mut self) { ... }          // private helper
}
```

**Why no file-private.** It was considered and rejected: it adds a fourth level, forces
every helper to think about two boundaries instead of one, and its main benefit —
preventing accidental coupling inside a module — is better achieved by keeping modules
small.

---

## 5. Types

### 5.1 Primitives

```
i8 i16 i32 i64        signed integers, two's complement
u8 u16 u32 u64        unsigned integers
isize usize           pointer-sized
f32 f64               IEEE-754
bool                  1 byte, values true / false
char                  32-bit Unicode scalar value
void                  the type of a function that returns nothing
```

Integer overflow **wraps, in every build**, and never panics: `+`, `-` and `*` on an `i32`
that goes past its range come back round, as they do in C, Java and C# — for signed types
too, where C++ would leave it undefined. A program that wants something else says so with
the methods on `Integer` (§14.4): `a.checked_add(b)` gives a `T?`, `a.saturating_sub(b)`
stops at the limit. `[DECIDED — §18.63]`

There are no implicit numeric conversions, in either direction, ever — not even widening.
`let a: i64 = some_i32` is an error; write `some_i32 as i64`. Untyped *literals* are the
sole exception (§2.5). This is the single most common source of silent bugs in C++ game
code and it costs nothing to forbid. `[DECIDED]`

### 5.2 Pointers and references

Five ways to refer to something — the model your samples already use, plus `&mut` and
`^mut`.

| Syntax | Meaning | Nullable | Mutable | Owns |
|---|---|---|---|---|
| `&T` | shared reference | no | no | no |
| `&mut T` | exclusive reference | no | yes | no |
| `^T` | raw pointer, read-only | yes | no | no |
| `^mut T` | raw pointer | yes | yes | no |
| `own ^mut T` | owning pointer | yes | yes | **yes** |

- **`&T` / `&mut T`** are what you pass to functions. They are guaranteed non-null and
  **cannot be stored in a struct field**, cannot be heap-allocated, and cannot be `own`ed.
  A reference therefore only ever lives on the stack, so it cannot outlive its frame by
  being smuggled into a heap object. `[DECIDED — §18.3]`

  Non-null is enforced at the point of conversion, not by analysis — a `&T` whose target has
  since been freed is still your problem (§1.1).

  This is a cheap heuristic, not a proof — Dazzle has no lifetime analysis, so a `&T`
  passed *down* into a function that outlives its target is still your problem. What the
  rule buys is that the most common dangling case costs zero analysis to prevent, and that
  `&T` keeps a meaning stronger than "non-null pointer".

  The practical consequence: a **context struct holds `^T`, not `&T`.**

  ```dz
  struct ButtonCtx {
      btn:        ^Button      // not &Button — this struct could be stored
      input:      ^Input
      delta_time: f32
  }
  ```
- **`^T`** and **`^mut T`** are C pointers — `const T*` and `T*`. Nullable, freely
  storable, no guarantees. This is how you build a scene graph with parent back-pointers,
  and it is on you to keep it valid. A `^T` is only read through; a `^mut T` may be written
  through as well. They stand to each other as `&T` and `&mut T` do, and as `[]T` and
  `[]mut T` do: read-only unless `mut` is written. `[DECIDED — §18.37]`
- **`own ^mut T`** is a `^mut T` that this struct is responsible for releasing. It changes
  the *type's* semantics — see §6. `own ^T` exists too, an owned block that is only read,
  but a container writes into what it owns, so `own ^mut T` is the usual form.

**Reading through a reference.** A `&T` or `&mut T` is accepted where a `T` is wanted, and
copies the value it refers to — `return m` in a function returning `Movie`, `let v: Vec3 =
r`, an argument, a field. Reading cannot fail, so it is not spelled out. Only a copyable `T`
can be read this way: a move-only one would have to be moved, and nothing may be moved out
of a borrow (`DZ0402`, §6.4), and in a generic body that means `T: Copy`. Where no type is
wanted, a reference stays a reference: `let r2 = r` is another `&T`. `[DECIDED — §18.35]`

**Writing through a reference.** Assigning to a `&mut T` writes the `T` it refers to — `x = 1` and `x += 1` on an `x: &mut i32` change the caller's integer — since a reference cannot be pointed elsewhere. Through a `&T` that is an error. `[DECIDED — §18.44]`

**Converting between them.** Forming a pointer changes nothing, so a reference becomes a
pointer of the same writability for free — `&T` → `^T`, `&mut T` → `^mut T` — and anything
writable also serves where the read-only form is wanted (`&mut T` → `^T`, `^mut T` → `^T`).
Nothing converts the other way: a `&T` or a `^T` never becomes a `^mut T`, which would gain
the right to write. Behind a `^mut` the pointee must match exactly, so a `^mut ^mut T` is not
a `^mut ^T` — through it, a read-only pointer could be stored where a writable one is
expected, which is why C forbids `T**` → `const T**` too. A pointer becomes a reference
through an `or` clause like any other fallible thing (§9.4), because a pointer may be null
and a reference may not:

```dz
let btn = ctx.btn or return                     // propagate
let btn = ctx.btn or panic("button missing")    // assert, with a message
let btn = ctx.btn or unreachable                // checked in debug, free in release
draw(ctx.btn)                                   // error[DZ0522]: expected &Button, found ^Button
```

The reference it produces is as writable as the pointer: a `^mut T` gives a `&mut T`, and a
`^T` gives a `&T`. Non-null is the only guarantee a reference carries that a pointer does
not, so it is worth one line at the boundary. The check happens once, where the pointer
enters scope, rather than on every function entry downstream. `[DECIDED — §18.16, §18.37]`

**Using a pointer is `unsafe`.** Nothing can check that a `^T` still points at something, so
the operations that assume it does are allowed only inside an `unsafe` block, or in the body
of an `unsafe fn`. Dereference is postfix `^`, and `.` reaches through a pointer on its own —
both count:

```dz
let p: ^mut Player = &mut world.players[0]   // forming a pointer: safe
if p != null { ... }                         // comparing, copying, storing, passing: safe
let pl = p or return                         // the checked conversion: safe
pl.health -= 10                              // through the reference: safe

unsafe {
    p.health -= 10                           // `.` through a `^mut T`
    let h = p^                               // explicit deref, copies the Player
}
```

Unsafe, and only these: `p^`, `.` and `[i]` through a `^T`, a pointer cast (§10.6),
`construct`, `destroy`, `memcpy` and `free` (§14.3), and calling an `unsafe fn`. Everything
else stays safe — forming, copying, storing, comparing and passing pointers, `alloc`, and the
`or` conversion, whose null check is the one thing that *can* be checked. An `unsafe fn` is
one whose callers must uphold something the compiler cannot see; its body is an `unsafe`
block, and calling it needs one:

```dz
unsafe fn poke(p: ^mut i32, v: i32) { p^ = v }

unsafe { poke(slot, 3) }
```

`unsafe` marks raw memory; it is not a proof. It does not promise that safe code cannot
misbehave — a stale `&T` handed down a call is still your problem (§1.1) — and it relaxes no
other rule: moving out of `p^` is still `DZ0402` inside `unsafe`. What it buys is that every
place a dangling pointer can bite is marked, and that a container's raw-memory work sits
behind the safe interface it offers. Pointers are the escape hatch, not the everyday tool:
shared ownership is the standard library's `Rc<T>`, sole ownership of a heap value `Box<T>`, and
a reference that notices its target is gone `Weak<T>` (§14.5). `[DECIDED — §18.16]`

`null` is a value of every pointer type, `^T` and `^mut T` alike. There is no null `&T`.

**Auto-referencing.** Method receivers are auto-referenced: `buf.resize(8)` works and
passes `&mut buf` when `resize` takes `&mut self`. Free functions are **not** auto-referenced —
you write `resize(&mut buf, 8)` — so that reading a call site tells you whether the callee
can modify the argument. `[PROPOSED]`

### 5.3 Arrays, slices, and strings

```dz
[8]f32          fixed-size array; a value type, copied on assignment
[]f32           slice: { ptr: ^f32, len: usize }; a read-only view, never owns
[]mut f32       the same view, through which elements may be written
[N]T            N may be any compile-time constant expression
```

**A slice carries its own writability, exactly as a reference does.** `[]T` and `[]mut T`
stand to each other as `&T` and `&mut T` do (§5.2): the elements of a `[]T` cannot be
assigned or `&mut`-borrowed no matter how the slice itself is bound, and a `[]mut T`
satisfies a `[]T` parameter while the reverse is rejected. Both are two words wide and
identical at runtime; the distinction is entirely in the type.

The alternative — one `[]T` whose writability follows the binding that holds it — was
rejected because a slice is a *view*, so the binding says nothing about the elements: a
`fn fill(dst: []f32)` could then never be written, and there would be no spelling for
"you may read these and not write them", which is the whole reason `&`/`&mut` exists.
`[DECIDED — §18.22]`

**Indexing an array or slice is bounds-checked in every build**, release included: it is
safe code, and an out-of-range `buf[i]` panics with the file, line, index and length rather
than reading past the end. A loop over `0..xs.len` pays nothing for it, since the optimiser
proves the check redundant. Where it does cost, the unchecked read is spelled out: `xs.ptr` is
the first element as a pointer, and indexing that is pointer arithmetic, inside `unsafe`.
`.ptr` is as writable as its view: a `[]mut T` or a writable array gives a `^mut T`, and a
`[]T` or a read-only array a `^T` (§5.2). `[DECIDED — §18.37]`

**Indexing a `^T` is pointer arithmetic**, as in C: `p[i]` is the `i`th `T` after `p`, and
it is never checked, because a pointer carries no length to check against — which is why it
is `unsafe` (§5.2). This is what a container's `m_data[i]` over its `own ^mut T` is (§8.3). A
`^T` is not a sequence, so `for x in p` remains an error — make a slice when you want one.

**An array literal** is a bracketed list, and its type is an array of exactly as many
elements as it lists:

```dz
let a = [1, 2, 3]                  // [3]i32 — the length comes from the literal
let b: [4]f32 = [1, 2, 3, 4]       // the elements take the annotation's element type
let grid = [[1, 2], [3, 4]]        // [2][2]i32
let empty: [0]i32 = []
```

Three rules, each of which exists to keep the literal readable rather than clever:

- **The length is exactly what is written.** `let a: [8]i32 = [1, 2]` is an error naming
  both lengths. "The rest is zero" is already spelled `let mut a: [8]i32`, since §6.2
  initialises every value — so a short literal would be a second way to say something the
  language already says, at the price of the length living only in the annotation.
- **There is no repeat form.** A zeroed array is `let mut a: [1024]f32` and needs no syntax;
  a non-zero fill is a loop or the stdlib's `fill`. A `[0; 8]` spelling is in any case not
  available: §2.2 makes `;` a terminator and suppresses terminators after `[`, so it could
  not be told apart from a line break inside the literal.
- **A literal may not be stored as a slice.** `let s: []i32 = [1, 2, 3]` is rejected: the
  array is a temporary, so the slice would point at storage that is already gone. Passing
  one is fine — `f([1, 2, 3])` on an `fn f(xs: []i32)` — because the temporary outlives the
  call. This is not lifetime analysis; it is one rule about one certain mistake.

The element type comes from the annotation when there is one, so `[4]f32 = [1, 2, 3, 4]`
gives four `f32`s rather than four `i32`s converted (there are no implicit conversions —
§5.1). With no annotation the elements must agree, and an untyped literal defaults as §2.5
says. `[DECIDED — §18.24]`

```dz
str             a UTF-8 string slice: { ptr: ^u8, len: usize }. Never owns.
                Not null-terminated. Immutable.
String          stdlib: owning, growable, allocator-aware. Written in Dazzle, not builtin.
cstr            a null-terminated ^u8, for C interop only.
```

**A slice of part of a sequence** is written with a range: `xs[a..b]`, `xs[a..]`, `xs[..b]`.
On an array or a slice both ends are checked when it runs, as an index is; on a pointer it is
`unsafe` and unchecked, and the end must be written, since a pointer carries no length. The
slice is as writable as what it views — a `[]mut T`, a `^mut T`, or an array that may be
written gives a `[]mut T`. `[DECIDED — §18.50]`

**A `str` is `{ ptr, len }`**, and `s.len` and `s.ptr` read them. Its bytes are `s as []u8`.
The way back is `str::from_utf8(bytes)`, a `str?` that is `none` when the bytes are not
well-formed UTF-8; `bytes as str` skips the check and is `unsafe`, as is `p as cstr` for a
`^u8`. `[DECIDED — §18.50]`

String literals live in static storage, with a zero after the last byte. **A string literal
is a `cstr` wherever a `cstr` is wanted** — a parameter, an annotated `let`, a field — and
costs nothing: C++ sees the literal itself, so `rl::InitWindow(1280, 720, "Dazzle")` is the
call it looks like. `"x" as cstr` says the same explicitly. The other way, `c as str`
measures a C string and copies nothing. A runtime `str` cannot become a `cstr`: nothing
follows its last byte, so a C string needs a copy, and a copy has to be owned and freed. That
owning C string arrives with the stdlib's `String` (M12), freed by its drop. `String::from("x")`
copies; `s.as_str()` borrows. `[DECIDED — §18.39]`

Because `?` binds to the type name, `[]T?` is a slice **of optionals**. An optional slice
is `([]T)?` — parenthesised, because it is far rarer and the parentheses say which level
is optional. `[PROPOSED]`

### 5.4 Compound types

```dz
(i32, f32)              tuple
T?                      optional  — see §5.6; built in, not a library type
fn(i32, f32) -> bool    function pointer
```

Tuples exist mainly for multiple return values; they have positional access `t.0`, `t.1`
and destructuring `let (x, y) = point()`. `[PROPOSED]`

### 5.5 Compile-time type queries

Every type answers a set of queries via `::`, evaluated during checking and usable in `if`
conditions that the compiler then folds away:

```dz
T::size                    usize, like sizeof(T)
T::align                   usize
T::name                    str, the source-level name
T::is_trivially_copyable   bool — no drop, no copy, all fields trivially copyable
T::is_copyable             bool — has an implicit or explicit copy
T::has_drop                bool
T::fields                  a compile-time sequence, iterable in macros (§12) and reflection (§13)
```

`sizeof(T)` and `alignof(T)` are accepted as aliases for `T::size` / `T::align`, because
they read better in allocation code, which is where they almost always appear.

### 5.6 Modality

The type sigils divide on two axes, and keeping them apart is what stops them composing
into unreadable stacks.

| Axis | Spelled | Asks |
|---|---|---|
| **Access** | `^T` `^mut T` `&T` `&mut T` `own ^mut T` `[]T` `[]mut T` | how do I reach this value? |
| **Responsibility** | `own` | who frees it? |
| **Modality** | `T?` | is the value there at all? |

Access is prefix, modality is postfix, and they never compete for the same position:

```dz
T?              an optional T
^T              a pointer, which may be null
^T?             a pointer to an optional T
&T?             a reference to an optional T; dereferences to T?
[]T?            a slice of optionals
own ^T?         an owning pointer to an optional T
```

**`?` applies to value types only.** There is no `(&T)?` and no `(^T)?`, because a pointer
already encodes absence — `^T` *is* the optional pointer, and layering `?` on it would give
you two spellings for one type. This is why the niche optimisation that earlier drafts
promised for `?^T` no longer appears anywhere: it has nothing to optimise. `[DECIDED]`

Fallibility is not on this table because it is not a property of a value. See §9.

---

## 6. Value semantics: ownership, copy, move, drop

This is the heart of the language. Everything else is ordinary.

### 6.1 The three special members

A struct has at most one of each — a constructor, a `drop` and a `copy`:

```dz
struct Buffer<T> {
    Buffer(capacity: u32) { ... }   // constructor: name matches the struct, no return type
}

impl<T> Drop for Buffer<T> {        // destructor
    fn drop(&mut self) { ... }
}

impl<T: Copy> Copy for Buffer<T> {  // copy constructor, for the instances it covers
    fn copy(&mut self, other: &Self) { ... }
}
```

The constructor is a member, since it is how a value comes to exist. `drop` and `copy` are
implementations of the standard library's `Drop` and `Copy` traits (§7.6): they are behaviour a type
has, and a bound can then ask for it — `T: Copy`. They are stricter than other
implementations, because they become the struct's C++ destructor and copy constructor and
so are part of its definition: written in the struct's own module, for the struct itself —
every instance, `impl<T> Drop for Buffer<T>`, never one — and `impl Drop`'s parameters take
no bounds, since a destructor cannot exist for some instances and not others. Neither is
ever called by name: `drop` runs at the end of a scope, and `copy` on every copy.
`[DECIDED — §18.31]`

That is the complete list. There is no move constructor, no move assignment, no copy
assignment, no default constructor to write, no rule of five. The compiler synthesises
whatever C++ needs from these three (§6.6).

### 6.2 Default initialisation

**Every value is always initialised.** There is no uninitialised memory in Dazzle.

```dz
let f: File          // fully constructed, fields at their declared defaults
let n: i32           // 0
let p: ^Player       // null
let b: Buffer<u8>    // all fields at defaults: a valid, empty, allocator-less buffer
```

A field's default is its `= expr` initialiser if it has one, otherwise the zero value of
its type. Defaults are evaluated in declaration order.

`let f: File` is legal only if `File` can be default-initialised — that is, if it has no
constructor, or has one callable with zero arguments. Otherwise the error names the
constructor you have to call. `[PROPOSED]`

This is what makes the comment in your `Buffer::copy` true *and checkable*: `copy` always
runs on a defaults-initialised object, so an early `return` out of it leaves a valid empty
value rather than a half-built one.

### 6.3 Copyability

A type is **copyable** if:

- it has an `impl Copy` that applies to it — `impl<T: Copy> Copy for Buffer<T>` covers
  `Buffer<f32>` and not `Buffer<File>` — **or**
- it implements neither `Copy` nor `Drop`, has no `own` field, and every field is copyable.

Every copyable type satisfies a `T: Copy` bound, whether or not it has an `impl Copy`.
Otherwise it is **move-only**, and assignment or passing by value requires an explicit
`move`:

```dz
struct File {
    handle: own ^u64
}

impl Drop for File {
    fn drop(&mut self) { close(handle) }
}

let f  = File("save.dat")
let f2 = f              // error: File is move-only because it owns a resource.
                        //   help: use `move(f)` to transfer ownership, or implement
                        //         `Copy` for File to make it copyable.
let f3 = move(f)        // ok
```

`own` on a field is what flips the switch. It is not a comment — it is the declaration that
this struct is responsible for a resource, and it makes the whole struct move-only.

**`own` describes the struct, not the pointer.** An `own ^T` *value* is one pointer, and
reading one — comparing it to `null`, dereferencing it, handing it to a release function —
copies a pointer and nothing else. What is move-only is the struct that declares the field,
because that is the thing with a `drop` to run. Anything else would make `if handle != null`
inside `File::drop` a move out of `self`. `[DECIDED]`

Acquiring ownership is an ordinary assignment: a plain `^T` may be stored into an `own ^T`
field, which is what `alloc` returning `^mut u8` (§9.8) has to mean. The reverse is also allowed
— an `own ^T` satisfies a `^T` — so `own` narrows nothing at the use site. `[DECIDED]`

### 6.4 Move semantics

`move(x)` transfers ownership out of `x`. Afterwards `x` is **moved-from**, and the
compiler tracks that with flow analysis:

```dz
let a = String("hello")
let b = move(a)
println("{}", a.length())   // error: `a` was moved on line 2 and cannot be used
```

The analysis is flow-sensitive and handles branches:

```dz
let s = String("x")
if cond {
    consume(move(s))
}
use(s)                      // error: `s` is moved on some paths reaching here
```

At runtime a moved-from value is left **at its defaults** — not in some unspecified state.
This is a deliberate divergence from C++: the source of a move is reset to exactly the
state a freshly declared value would have, which means running its `drop` is always safe
and always a no-op for the resource. It costs a few stores; it removes an entire category
of bug. `[DECIDED]`

**This holds for every type.** `move` on a copyable value — an `i32`, a `str` — is still a
move: the source is moved-from afterwards, a later use of it is an error, and at runtime it
is reset to its defaults. To go on using a copyable value, copy it instead: leave out
`move`. One rule for every type is also what keeps a generic body honest: `move(v)` in a
`Buffer<T>` means the same for `Buffer<i32>` as for `Buffer<File>`. `[DECIDED — §18.33]`

**A move needs a place the function owns.** `move` resets its source, and a `&`, a `^` or an
array element is not something the function has the right to reset — so `move(p^)`,
`move(r.field)` through a reference, and `move(xs[i])` are errors. A local, a by-value
parameter, and a field of either are fine. `[DECIDED]`

The same flow analysis handles **use-before-assignment** for the cases where default
initialisation does not apply (a `let` with no type and no initialiser is rejected outright;
a partially-moved struct field is tracked per field), and **definite return**: a function
that promises a value must produce one on every path. An exhaustive `switch` counts as
returning if every arm does, since §10.3 already proved there is no other path.

**What is *not* checked:** whether a `&T` outlives what it points at, and whether a `^T`
dangles. Dazzle has no lifetime system, by design (§1.1). `[DECIDED]`

### 6.5 Drop

A type's `drop` runs deterministically:

- at the end of the enclosing scope, in **reverse declaration order**;
- before a value is overwritten by assignment;
- when the owner of the value is itself dropped (fields drop in reverse declaration order);
- on a moved-from value — but harmlessly, since the value is back at its defaults.

`defer { ... }` runs a block at scope exit, after that scope's drops, in reverse order of
registration. It is for the non-RAII cases (`defer profiler::end_zone()`).

**A type with an `own` field and no `drop` is an error.** `[DECIDED — §18.4]`

```
error[DZ0412]: `File` owns a resource but never releases it
  --> src/io/file.dz:2:5
   |
 2 |     handle: own ^u64
   |     ^^^^^^^^^^^^^^^^ this field is marked `own`
   |
   = help: add an `impl Drop for File` that releases `handle`, or remove `own` if File
           does not own this pointer
```

### 6.6 How this lowers to C++

Everything above becomes ordinary C++ special members, which is why Dazzle does not have to
implement destruction ordering, temporary lifetime, or exception safety itself — C++ already
does, correctly, and every C++ debugger and profiler already understands it.

| Dazzle | Generated C++ |
|---|---|
| `Buffer(cap)` | `Buffer(uint32_t cap)` |
| `impl Copy`'s `copy` | `Buffer(const Buffer& other)` — body runs on a defaults-initialised `*this` |
| `impl Drop`'s `drop` | `~Buffer()` |
| any other `impl`'s method | a free function taking the receiver first: `Mul_f32__Vec2__mul(const Vec2& self, float rhs)` |
| move | `Buffer(Buffer&& o)` — memberwise steal, then reset `o` to defaults |
| copy assignment | `~Buffer(); new (this) Buffer{}; copy(other);` + self-assignment guard |
| move assignment | `~Buffer(); new (this) Buffer{}; steal(o); reset(o);` |
| move-only type | copy constructor and copy assignment `= delete`d |

The copy-assignment lowering is exactly the `drop() + reset-to-defaults + copy()` sequence
your `Buffer` comment describes, so the invariant it relies on is guaranteed by codegen.

Deleting the C++ copy constructor for move-only types means that even if Dazzle's own flow
analysis had a hole, the C++ compiler catches the copy as a second line of defence.

---

## 7. Declarations

### 7.1 Bindings

```dz
let x = 5                     // immutable, inferred i32
let mut y: f32 = 1.5          // mutable
const MAX_ENTITIES = 4096     // compile-time constant, no storage
let _ = returns_something()   // explicitly discard
```

`let` without an initialiser requires a type annotation and default-initialises (§6.2).
Shadowing in an inner scope is allowed; shadowing in the *same* scope is an error — the
`main.dz` sample shows how easily a duplicate `let b` slips past you.

### 7.2 Functions

```dz
fn lerp(a: f32, b: f32, t: f32) -> f32 {
    return a + (b - a) * t
}

pub fn find(items: []Entity, id: EntityId) -> ^Entity { ... }

fn spawn(w: &mut World, pos: Vec3, hp: i32 = 100) -> EntityId { ... }
```

- The return type is omitted for `void`.
- A fallible function is marked by `!->` in place of `->`; the marker sits on the arrow
  rather than on the return type so that it lands at a fixed column near the left edge,
  which is the same argument §9.5 makes for a leading propagation keyword.
- Default arguments must be trailing, and are evaluated at the **call site** — which is what
  makes `allocator: ^Allocator = Allocators::Global` behave the way you expect.
- **There is no function overloading.** `[DECIDED — §18.5]` One name, one function.
  Imported C++ overloads are the exception (§15.3), because we do not control those headers.
- Constructors may be overloaded on arity and parameter types, since `Buffer(8)` and
  `Buffer(8, arena)` are genuinely the same operation.

**Arguments are evaluated left to right**, each completely before the next: in
`println("{} {}", touch(), COUNT.hits)`, `touch()` has run by the time `COUNT.hits` is
read. The receiver of a method call is evaluated before its arguments. `[DECIDED — §18.49]`

The same holds for **every expression with more than one operand**: both sides of a binary
operator (`read_u8(r) << 8 | read_u8(r)` reads the high byte first), the container and the
index of `xs[i]`, the bounds of a slice or range, and the elements of a tuple or array
literal. A struct or union literal evaluates its fields **in the order they are written**,
whatever order the type declares them in. `&&` and `||` still skip their right operand when
the left decides. **Assignment is the one exception: the value is evaluated before the
place it is stored in** — in `xs[next()] = compute()`, `compute()` runs first — for `=` and
for every compound assignment alike. `[DECIDED — §18.49]`

**Variadic parameters.** One parameter may take any number of arguments, written
`name: ..T`. In the body it is a read-only slice, `[]T`:

```dz
fn sum(xs: ..i32) -> i32 {
    let mut t = 0
    for x in xs { t += x }
    return t
}

sum()               // an empty slice
sum(1, 2, 3)        // the arguments, in a temporary array on the caller's stack
sum(..numbers)      // spread: an existing slice or array, passed as it is
```

- There is at most one, and it has no default; its arguments come after every fixed one.
- `..xs` spreads a slice or array, and must then be the *only* variadic argument —
  `sum(1, ..xs)` is an error; build the slice you want first. Forwarding is
  `inner(..args)`.
- Arguments are evaluated left to right like any others, moved into the array when they
  are move-only, and dropped when the call's statement ends.
- **Arguments of different types** share a slice through trait references: with
  `things: ..&Describe`, `tell(cat, 7, Cat{})` borrows each argument as a `&Describe` for
  the call. `..^Describe` works too, though a call through a `^T` is `unsafe` (§5.2).
- Methods, constructors and generic functions may be variadic; `fn biggest<T: Ord>(first:
  T, rest: ..T)` infers `T` from the arguments.
- As a value, a variadic function is a plain `fn([]T) -> R` that takes its slice.

C variadic functions (`printf`) are still not imported (§15.2). `[DECIDED — §18.56]`

**Arguments passed by name.** The parameters after a variadic one cannot be reached by
position — every positional argument there is one more variadic argument — so they are
passed **by name**, `name = value`, after all the others:

```dz
fn total(xs: ..i32, scale: i32 = 1, loc: SourceLocation = #location) -> i32 { … }

total(1, 2, 3)                        // scale = 1, loc = this line
total(1, 2, scale = 10)
total(loc = here, scale = 2)          // any order; no variadic arguments at all
String::format("{} hp", hp, allocator = Allocators::Temp)
```

- Only these parameters are passed by name, and only by name. Naming any other parameter —
  or passing a name to a function with no variadic parameter — is an error (`DZ0558`), as
  is a name the function does not have, a name given twice, or a positional argument after
  a named one.
- One with a default may be left out, whichever others are given; its default is evaluated
  at the call, as any default is. One with no default must be passed.
- A name is `name = value` only inside an argument list; assignment is a statement, never an
  expression, so the two cannot be confused. `[DECIDED — §18.72]`

**`extern fn`** declares a function whose body is not Dazzle: `extern fn monotonic_nanos() ->
u64`, with no braces. **Only the standard library may write one** (`DZ0317`), at module
level; its body is C++ the compiler emits beside the runtime shim (§16.4), and it is how
`std` reaches the clocks and the file system. It has no type parameters and is not
fallible — the Dazzle function that calls it builds any `Error`. A program reaches C and
C++ through a `cpp` dependency instead (§15.1). `[DECIDED — §18.65]`

### 7.3 Structs

```dz
pub struct Transform {
    position: Vec3
    rotation: Quat = Quat::identity()
    scale:    Vec3 = Vec3{1, 1, 1}

    pub fn matrix(&self) -> Mat4 { ... }
    pub fn translate(&mut self, d: Vec3) { position += d }
}
```

A method's receiver is its first parameter: `&self` to read the value, `&mut self` to
mutate it. The receiver is the only place a method says it mutates — there is no `mut fn`,
which would say it a second time and could disagree (§18.34). Inside a
method, fields are in scope unqualified (`position`), and `self.position` is also valid —
and required when a local shadows a field.

**A function with no receiver is an associated function**: it belongs to the type, not to a
value of it, and is called through the type — `Quat::identity()`, `Buffer<f32>::with_capacity(8)`.
It has no `self`, so the type's fields are not in scope in it, and calling one on a value
(`q.identity()`) is an error (DZ0520). Inside the type's own body a bare `Buffer` is
`Buffer<T>`, as everywhere else there.

**A `const` declared in a type is an associated constant**, named through the type:
`Allocators::Global`. It writes its type, and its value is a constant or the address of a
static — `unsafe { &mut HEAP }` — and nothing that runs code (DZ0553). It is evaluated where
it is named, so the address of a `#[thread_local]` static is the naming thread's own.

Struct literals:

```dz
let t = Transform{ position: Vec3{0, 1, 0} }  // unlisted fields take their defaults
let v = Vec3{1, 2, 3}                          // positional, allowed when all fields listed
let s: ButtonState = .Transition{}             // leading-dot inference from expected type
```

#### Inheritance

**`: Base` after a struct's name names the one struct it inherits from** — `struct Button :
UiElement`. One base, never more, and never a trait: those are implemented with `impl`
(§7.6). `[DECIDED — 18.32]`

```dz
pub struct UiElement {
    protected rect: Rect
    pub       shown: bool = true

    pub virtual fn draw(&self)               // no body: `UiElement` is abstract
    pub virtual fn hit(&self, p: Vec2) -> bool { return rect.contains(p) }

    UiElement(r: Rect) { rect = r }
}

pub struct Button : UiElement {
    pub label: str = ""

    Button(r: Rect, text: str) : UiElement(r) { label = text }

    pub override fn draw(&self) { ... }
    pub override fn hit(&self, p: Vec2) -> bool {
        return UiElement::hit(self, p) && shown
    }
}
```

**The base comes first in memory**, so `&Derived` → `&Base` and `^Derived` → `^Base` are
implicit and cost nothing. A **by-value** `Derived` where a `Base` is wanted is an error:
copying one would silently drop everything the derived type adds, which is C++'s slicing.
A base's fields and methods are the derived type's, in scope unqualified inside its methods
exactly as its own are, and a base's field or method may not be redeclared — a name means
one thing in a type, whichever declaration it came from.

**Constructors.** A base with a constructor must be constructed through it, written after
the derived constructor's parameters: `Button(r: Rect, text: str) : UiElement(r) { … }`.
Leaving it out is an error. A base without a constructor is default-initialised (§6.2) and
needs nothing written, though `: Base()` may be written to say so. A type that has a
constructor — its own, or an inherited one — is **not** built with a struct literal: the
constructor is what establishes the type's invariant, and a literal would step around it.
The base's arguments are ordinary expressions over the constructor's parameters, including
ones that need statements — `: UiElement(r or Rect{})` — but not `return` or `or return`:
the base is built before the constructor begins, so there is nothing yet to return from.

**`virtual` and `override`.** A method is dispatched statically unless it says otherwise.
`virtual fn` opts one into runtime dispatch; `override fn` replaces one, and must match the
base's signature exactly. Both are checked in both directions: an `override` with nothing
to override is an error, and so is a method that redeclares a base's without saying
`override`. A `virtual fn` **with no body** makes its struct **abstract** — there is
nothing to call, so no value of it can exist, by value, in a field or as a parameter. An
abstract type behind a `&` or a `^` is exactly what it is for. A derived type becomes
concrete by overriding every body-less `virtual` it inherits.

`Base::method(self, …)` calls the base's version *without* virtual dispatch, which is how
an override builds on what it replaces rather than discarding it. `drop` becomes virtual
automatically as soon as a type or one of its bases has a `virtual` method, so a value
destroyed through a base reference runs the right one.

**Implementations are inherited.** A `Button` is `Inspect` if `UiElement` is, and a more
specific `impl Inspect for Button` replaces the inherited one. Dispatch stays static
(§7.6): through a `&UiElement` it is `UiElement`'s implementation that runs, and reaching
the derived behaviour from there is what a `virtual` method is for.

### 7.4 Enums

```dz
pub enum Sprite : u16 {
    None = 0
    Default
    Selected
    Pressed
}

let s = Sprite::Selected
```

Enums are named integers with an optional backing type (`i32` by default). They are **not**
implicitly convertible to that backing type; use `as`. A `switch` on an enum must be
exhaustive or carry a `default` arm.

Like the integers they are, enums implement **`Eq`, `Hash` and `Ord`** without an `impl`:
`==` and `<` compare values, so an enum is a `HashMap` key and satisfies `T: Ord`.
`[DECIDED — §18.54]`

**A variant may be written `.Selected` wherever the enum is already known**, which is most
places worth writing one:

```dz
let s: Sprite = .Selected
fn draw(s: Sprite) { ... }        draw(.Pressed)
if s == .Default { ... }          if .Default == s { ... }
return .None                      Style{ sprite: .Selected }
let s = if hot { .Pressed } else { .Default }
```

`Sprite::Selected` always works and is what you write where there is no expected type to
read it from — including `let s = Sprite::Selected`, since a bare `let` has nothing to infer
from. The leading dot is the same mechanism a union variant uses (§7.5) and the same one
`.Circle{ radius: 1 }` uses; it is written `.Selected`, never `Sprite.Selected`, because `.`
is the runtime operator and its left side is always a value (§3.3). `[DECIDED — §18.24]`

### 7.5 Unions

A `union` is a **tagged** union — a sum type, not C's overlapping storage.

```dz
pub union Shape {
    Circle { radius: f32 }
    Rect   { w: f32, h: f32 }
    Point {}                     // no payload
}

let s = Shape::Circle{ radius: 2 }

switch s {
    case Circle(c) { area = PI * c.radius * c.radius }
    case Rect(r)   { area = r.w * r.h }
    case Point     { area = 0 }
}
```

A variant with no payload is written with empty braces, `Point {}`, and built as
`Shape::Point` or `Shape::Point{}`.

**A bare variant is an existing struct.** Written without braces, a variant names a struct in
scope and carries a value of it:

```dz
struct DefaultState {
    pub fn update(&self, input: &Input) -> ButtonState? { … }
}
struct TransitionState {
    pub t: f32 = 0
    pub fn update(&mut self, ctx: &ButtonCtx, btn: &mut Button) -> ButtonState? { … }
}

union ButtonState {
    DefaultState
    TransitionState
}

let s: ButtonState = .TransitionState{}            // built as a `TransitionState` is: `t` is 0
switch s {
    case DefaultState(d)         { d.update(input) }         // `d` is a `DefaultState`
    case TransitionState(&mut t) { t.update(ctx, btn) }
}
```

The payload is the struct itself — its fields, defaults, visibility and methods — so a variant
literal is checked as that struct's literal and a `case` binding has the struct's type. A bare
name with no struct of that name in scope, or one naming an enum, a union, a trait or a
generic struct (which has nowhere to write its arguments), is an error rather than an empty
variant: otherwise a misspelling, or a struct added or removed elsewhere, would change what
the union holds without a word. `[DECIDED — §18.45]`

A union's size is its largest variant plus a tag, aligned. `Shape::Kind` is an
automatically generated enum of the variant names, so `s.kind()` gives you something cheap
to store or compare. **A union owns its active payload**: a variant may hold a `String`, a
`List` or a struct with a `drop`, and the union drops whichever variant is active when it
dies, drops the old variant before a new one is assigned, and moves by moving the active
payload. A union is copyable only when every variant is. `[DECIDED — §18.52]`

Untagged/overlapping storage, should interop ever need it, is a separate `#[raw] union`.

### 7.6 Traits

```dz
pub trait Allocator {
    fn alloc(&self, size: usize, align: usize) -> ^mut u8
    fn realloc(&self, p: ^mut u8, old_size: usize, new_size: usize, align: usize) -> ^mut u8
    fn free(&self, p: ^mut u8, size: usize, align: usize)
}

pub trait Drawable {
    fn bounds(&self) -> Rect
    fn draw(&self, batch: &mut SpriteBatch)

    fn is_visible(&self, cam: &Camera) -> bool {   // default method
        return cam.frustum.contains(self.bounds())
    }
}
```

**A trait is implemented by an `impl` block**, and checked where it is written:

```dz
pub struct Arena {
    m_base: own ^mut u8
    m_used: usize
}

impl Allocator for Arena {
    fn alloc(&self, size: usize, align: usize) -> ^mut u8 { ... }
    fn realloc(...) -> ^mut u8 { ... }
    fn free(...) { }
}
```

The `impl` holds exactly the trait's methods — every one the trait gives no default for, with
the declared signature, and nothing the trait does not declare — and chooses each associated
type the trait leaves open (§8.5). A trait's supertraits need implementations of their own:
`impl Ord for S` requires an `impl Eq for S` as well. Inside the methods `self` is the type
implemented for, and its fields are in scope unqualified as in the type's own methods (§7.3)
— all of them when the `impl` is in the type's module, and only the `pub` ones otherwise (§4).

**Where an `impl` may be written: with the trait, or with the type.** In the module that
declares the trait, or the one that declares the type, and nowhere else. That is what lets a
module add behaviour to a type it did not write, without touching it:

```dz
// the editor module: `Inspect` is its own; `engine::Transform` and `f32` are not
pub trait Inspect { fn inspect(&self, ui: &mut Ui) }

impl Inspect for engine::Transform { fn inspect(&self, ui: &mut Ui) { ... } }
impl Inspect for f32               { fn inspect(&self, ui: &mut Ui) { ... } }
```

— and it is also what keeps implementations from colliding. Two libraries cannot both
implement a third's trait for a fourth's type, so a program using both is never left to
choose, and code that can see both the trait and the type can always see the implementation
that joins them. Implementing someone else's trait for someone else's type means wrapping the
type in a struct of your own. `[DECIDED — §18.31]`

**Generic implementations** have parameters of their own: `impl<T: Inspect> Inspect for
Buffer<T>` applies to every `Buffer<T>` whose `T` is `Inspect`. An implementation for every
type at once — `impl<T> Inspect for T` — is not allowed, since it would overlap every other
implementation of the trait. Two implementations of one trait that could apply to the same
type at the same arguments are an error; at different arguments they are how a type takes
several right-hand sides (§11.2).

**A type's own method wins over a trait's** of the same name. When two traits both provide a
method, the call names the one it means: `Describe::name(&v)`. `drop` and `copy` are never
called by name (§6.1). `[DECIDED — §18.31]`

**Static dispatch** happens whenever the trait is a generic bound (§8): the call is
monomorphized and direct.

**Dynamic dispatch** happens through `^Trait` or `&Trait`, which is a **fat pointer**:

```dz
struct TraitPtr { data: ^mut void, vtable: ^VTable }
```

The vtable lives in static storage; **the object carries nothing** — no vptr, no header, no
size change. A `Vec3` implementing `Drawable` is still 12 bytes. This is the same principle
as the reflection design (§13): metadata sits beside the data, never inside it.
`[DECIDED — §18.6]`

Each vtable is additionally reachable by a **stable, deterministic id** — a hash of the
implementing type's and the trait's fully qualified names — rather than only by its address.
That costs nothing now and is what makes dynamic dispatch re-resolvable after a hot-reload
swap (§17.6).

The cost is that `^Allocator` is 16 bytes while `^Arena` is 8. The compiler will not let you
silently confuse them, and the LSP shows the fat-pointer-ness on hover.

### 7.7 Statics

A `static` is a variable declared at module scope, and there is one of it for the whole
program — or one per thread, with `#[thread_local]`:

```dz
static FRAME_COUNT: u64 = 0
pub static PALETTE: [4]Color = [BLACK, WHITE, RED, BLUE]

#[thread_local]
static SCRATCH: TempArena = TempArena{}
```

- **The type is written.** A static is storage that every user of it relies on, so it is
  never inferred from its initialiser.
- **The initialiser is built from constants**: literals, `const`s, `null`, `none`, and
  struct, array, tuple and variant literals of those (DZ0553). A static exists before `main`
  runs, and C++ builds statics in different files in no promised order, so one static cannot
  be allowed to run code that reads another. Anything more is filled in from `main`.
- **Reading one is safe; changing one is `unsafe`** (DZ0528). Assigning to it or into it,
  taking `&mut` of it, calling a `&mut self` method on it, or binding `&mut` into it with
  `for` or `case` all need an `unsafe` block: a static is shared by every piece of code that
  names it, and nothing checks what else is reading it or holding a reference into it.
- **Nothing moves out of a static**, and a move-only one is not copied either (DZ0402). It
  belongs to the program rather than to any function, exactly as a borrow does.
- A static that owns resources is dropped when the program exits.

`const` has no storage and is folded into its uses; `static` is one place in memory. The
standard allocators (§14.2) are statics. `[DECIDED — §18.46]`

---

## 8. Generics

Generics are **checked at the definition site** and **monomorphized by Dazzle** into
concrete C++ types. They do not become C++ templates. `[DECIDED]`

### 8.1 Declaration and bounds

```dz
fn max<T: Ord>(a: T, b: T) -> T {
    if a > b { return move(a) }       // `T` may be move-only (§8.2)
    return move(b)
}

pub struct Buffer<T> { ... }

pub struct HashMap<K: Hash + Eq, V> { ... }
```

The body of `max` is checked once, against `T: Ord` alone. `a > b` is legal only because
`Ord` provides it. `a.frobnicate()` is an error reported at the line inside `max`, naming
the missing bound — not at the instantiation.

This is the whole reason for the choice: **the LSP can complete inside a generic body**,
because it knows exactly what `T` can do.

**A method may have type parameters of its own**, beside its type's:

```dz
pub struct List<T> {
    pub fn fold<A: Copy>(&self, start: A, step: fn(A, &T) -> A) -> A { ... }
    pub fn filled<V: Copy>(n: usize, value: V, make: fn(V) -> T) -> List<T> { ... }
}

let total = scores.fold(0, add)                    // A inferred as i32
let widest = names.fold<usize>(0, longest)         // written out
let grid = List<Cell>::filled<u8>(64, 0, to_cell)
```

They are solved at each call exactly as a generic function's are — from the arguments, or
written after the name — and their bounds are checked there. The type's own parameters
come from the receiver, or from the path for a function without one. Each combination
is its own instance (§8.4).

A `virtual` or `override` method, and a trait's method, **cannot** have type parameters of
their own (`DZ0557`): a vtable slot is one function, and a trait's method is called through
implementations, or a `^Trait`, that do not know the arguments. Make the trait generic
instead, `trait Show<T>`. `[DECIDED — §18.66]`

**A generic function may be taken as a value**, instantiated there: `xs.sort_by(greater<i32>)`
writes its arguments, and `let f: fn(&i32, &i32) -> bool = greater` reads them off the
function type wanted. Its bounds are checked as at a call.

### 8.2 Unbounded parameters

`Buffer<T>` has no bound, so `T` supports only what every type supports: being moved,
default-initialised, dropped, and queried via `T::` (§5.5). That is precisely enough for
`Buffer` — your implementation only ever branches on `T::is_trivially_copyable` and calls
`construct` / `destroy`, all of which are universal.

Being *copied* is not on that list, so inside the body an unbounded `T` is a move-only type
(§6.3) and every rule for one applies: returning or passing a `T` by value is a move, and is
written `move(x)` as it is anywhere else (§6.4). The instantiation may well be `Buffer<f32>`,
where the copy would have been free — but the body is checked once, for every `T`.

A bound says otherwise: **a `T: Copy` may be copied.** Every copyable type (§6.3) satisfies
`Copy`, primitives included, whether or not it has an `impl Copy`. `[DECIDED — §18.26]`

**A container is copyable exactly when its elements are** by bounding its `impl Copy` rather
than the container: `impl<T: Copy> Copy for Buffer<T>` gives `Buffer<f32>` a `copy` and
leaves `Buffer<File>` move-only — while `Buffer<File>` remains a perfectly good type.
Bounding the struct instead, `Buffer<T: Copy>`, would shut out every element that owns a
resource. Inside that `copy`, `T` is `Copy` because the implementation says so.
`[DECIDED — §18.26]`

### 8.3 Compile-time branches

```dz
impl<T: Copy> Copy for Buffer<T> {
    fn copy(&mut self, other: &Self) {
        unsafe {
            m_data = alloc(other.m_cap * sizeof(T), alignof(T)) as ^mut T
            if T::is_trivially_copyable {
                memcpy(m_data, other.m_data, other.m_len * sizeof(T))
            } else {
                for i in 0..other.m_len { construct(m_data[i], other.m_data[i]) }
            }
        }
        m_cap = other.m_cap
        m_len = other.m_len
    }
}
```

A condition made only of compile-time queries is folded during monomorphization: the dead
branch is not emitted and not present in the C++ output. "Only" means type queries (§5.5),
`bool` constants, and `!`, `&&` and `||` over them; a condition that also reads a runtime
value is an ordinary `if`, and both branches are emitted. `[PROPOSED]`

Folding happens *after* checking, not instead of it. Both branches are checked against the
bounds, for every `T`, exactly as the rest of the body is (§8.1) — a branch that only makes
sense for some `T` is still an error in the others. That is what keeps a generic's errors at
its definition. The `else` branch above copies an element, which it may because the
implementation is bounded `T: Copy` (§8.2).

### 8.4 Monomorphization

`Buffer<f32>` and `Buffer<Entity>` become two distinct C++ structs:

```cpp
struct dz_engine_Buffer__f32 { /* ... */ };
struct dz_engine_Buffer__engine_Entity { /* ... */ };
```

Instantiations are cached per (generic, type-arguments) pair across the whole build, so a
type used in forty places is emitted once. Because Dazzle drives this rather than C++,
recursive instantiation depth is bounded by the compiler with a clear diagnostic instead of
a template-depth error.

### 8.5 Associated types

An **associated type** is a type that a trait names but does not choose — each implementer
fills it in. It exists because some traits need to talk about a type that varies per
implementer, and a generic parameter is the wrong tool for that.

The motivating case is iteration. A trait for "something you can step through" has to say
what it yields, but that answer is different for every container:

```dz
pub trait Iterate {
    type Item                       // "each implementer names this"

    fn valid(&self) -> bool         // is there a current element?
    fn get(&self) -> Item           // ...then this is it
    fn advance(&mut self)       // step past it
}
```

Two implementers, two different `Item`s:

```dz
struct SliceIter<T> {
    m_slice: []T
    m_index: usize
}

impl<T> Iterate for SliceIter<T> {
    type Item = &T                     // stepping a []T yields references into it

    fn valid(&self) -> bool   { return m_index < m_slice.len }
    fn get(&self)   -> &T     { return &m_slice[m_index] }
    fn advance(&mut self) { m_index += 1 }
}

struct RangeIter {
    m_current: i32
    m_end:     i32
}

impl Iterate for RangeIter {
    type Item = i32                    // stepping 0..n yields plain i32 values

    fn valid(&self) -> bool   { return m_current < m_end }
    fn get(&self)   -> i32    { return m_current }
    fn advance(&mut self) { m_current += 1 }
}
```

Now `for` works on both, and the loop variable gets the right type — `&Entity` in the first
case, `i32` in the second — because `for e in xs` desugars to the three calls and the
compiler reads `Item` off whichever implementer `xs` provides (§10.4):

```dz
for &e in entities { e.draw(&mut batch) }   // e is &Entity
for i in 0..count  { grid[i] = 0 }          // i is i32
```

**Why not just `trait Iterate<Item>`?** Because a generic parameter is chosen by the
*caller*, so `SliceIter<T>` could then implement `Iterate<&T>` *and* `Iterate<T>` *and*
`Iterate<u8>` all at once, and `for e in xs` would have no way to know which one you meant —
every use would need an annotation. An associated type is chosen by the *implementer*, so
there is exactly one answer per type and inference always succeeds. That is the entire
distinction: **generic parameters are inputs, associated types are outputs.** `[PROPOSED]`

Associated types may also carry bounds (`type Item : Format`), which the implementer must
satisfy.

**Each implementation chooses its own.** A type implementing one trait at several arguments
answers for each separately: `impl Mul<Vec4> for Mat4` has `type Output = Vec4`, and `impl
Mul<f32> for Mat4` has `type Output = Mat4` (§11.2). `[DECIDED — §18.29]`

**Why three methods and not one `next() -> Item?`.** The one-method form is what every
`Option`-returning iterator does, and it does not work here. It uses absence as the
end-of-iteration signal, which means the signal lives **inside the item's own value
domain**:

- If `Item` is `^T`, `Item?` is `(^T)?` — the nesting §5.6 bans, because `^T` already spells
  absence as `null`. Collapsing it to `^T` makes "there is no element 3" and "element 3 is a
  null pointer" the same value, so iterating a `[]^Entity` would stop at the first null
  element instead of yielding it.
- If `Item` is itself `T?`, `Item?` is `T??`. Either that collapses — and a `none` *element*
  ends the loop early, the identical bug — or nested optionals have to become a real type.

Languages that use the one-method form get away with it because `Option<Option<T>>` and
`Option<&T>` are distinct types there. Dazzle deliberately gave pointers exactly one
spelling for absence (§18.15), and that choice is what rules the one-method form out.

Separating "is there one" from "what is it" removes the overlap entirely: `valid` answers
the first question, `get` the second, and `Item` is never wrapped, so it may be **any** type
— `&T`, `&mut T`, `^T`, `T?`, a tuple — and a null or `none` element is just a value. It is
also the shape a C++ `for (it = …; it.valid(); it.advance())` loop already has, so the
generated code is an ordinary loop rather than a repeated optional test (§16.2).

The cost is three methods per implementer instead of one, and an iterator has to hold its
current position somewhere `get` can read it. That is natural for slices, ranges, arrays and
maps; an iterator that must fetch a value in order to discover whether one exists has to
buffer it internally. `[DECIDED — §18.21]`

### 8.6 Why Dazzle monomorphizes instead of emitting C++ templates

The obvious alternative to §8.4 is to emit `template<typename T> struct Buffer { … }` and
let the C++ compiler do the instantiation. It would mean less codegen work and more
idiomatic output. It is still the wrong trade, for four reasons — but the reasoning is
worth stating, because "let C++ do it" is the right answer for *most* of this language and
it is important to be clear about why this is the exception.

Dazzle delegates to C++ wherever C++ does a job **once per construct**: destructors,
destruction order, copy constructors, `offsetof`, temporary lifetime, overload resolution in
imported headers. Templates are the opposite — C++ does that work **once per instantiation,
expensively, and with bad diagnostics**. That is the whole distinction.

1. **It would undo the reason you chose definition-site generics.** We check `Buffer<T>`'s
   body once, against `T`'s bounds (§8.1). If we then hand a template to C++, C++ re-checks
   it at every instantiation — and any mismatch between our type system and C++'s surfaces
   as a template error pointing into *generated* code the user never wrote. Principle 4 (§1)
   exists specifically to prevent that.
2. **Compile time.** Template instantiation is among the slowest things a C++ compiler
   does. A game codebase full of `Buffer<T>`, `HashMap<K,V>`, and `Array<T>` pays that cost
   on every build. Emitting plain structs means the C++ compiler does almost no work per
   instantiation, which is principle 6.
3. **Trait bounds have no C++17 equivalent.** `T: Ord + Hash` would have to be dropped on
   the floor (concepts are C++20, and §16.5 targets C++17 for console compatibility), so any
   bound violation that slipped past us would become a deep instantiation error instead of a
   clean one.
4. **The compile-time branch folding you just approved (§8.3) gets subtler.** With
   monomorphization we simply do not emit the dead branch. With templates we would depend on
   `if constexpr`, which requires the discarded branch to still be well-formed for every
   dependent `T` — a rule that is easy to violate and produces exactly the error messages
   we are trying to avoid.

The cost of monomorphizing is a mono pass in the compiler (a bounded, well-understood
amount of work) and more generated C++ text. Output readability is preserved by emitting
each instantiation as a plain struct with a comment naming its Dazzle origin:

```cpp
// dazzle: engine::Buffer<f32>
struct Buffer__f32 { /* ... */ };
```

`[DECIDED — §18.14]`

---

## 9. Errors and absence

Three different things can fail to give you a value, and Dazzle gives them one handling
construct:

| Source | Written | Carries on failure |
|---|---|---|
| an optional value | `T?` | nothing |
| a fallible call | `fn f(…) !-> T` | an `Error` |
| a pointer | `^T` | nothing |

`T?` is a **type**. `!` is a **signature marker** and never a type — there is no `T!` to
store in a field, annotate a `let` with, or point at. That asymmetry is deliberate: an
optional is a data shape, a failure is control flow, and the notation now says so.
`[DECIDED — 18.7]`

### 9.1 Producing them

In a function returning `T?` or marked `!->`, returning a plain `T` **is** the success case;
there is no wrapping step. The failure case has its own literal:

```dz
fn find_player(&mut self, id: EntityId) -> ^Player {
    for &mut p in m_players {
        if p.id == id { return ^p }
    }
    return null
}

fn read_header(f: &mut File) !-> Header {
    let mut h: Header
    if f.read_into(&mut h) < Header::size {
        return error("truncated header")
    }
    if h.magic != LEVEL_MAGIC {
        return error(ErrorCode::BadMagic, "bad magic 0x{:x}", h.magic)
    }
    return h
}
```

- The same conversion happens **wherever a `T?` is expected**: `let hp: i32? = 100`, an
  argument to a `T?` parameter, an assignment, and the arms of an `if` or `switch` whose
  other arms are `T?` or `none` — `if alive { 100 } else { none }` is an `i32?`. A `T?` never
  becomes a `T` without `or`. `[DECIDED — §18.53]`
- `none` is a keyword literal, valid only where a `T?` is expected.
- `error(...)` builds an `Error`, and is valid only in a `!->` function. It takes an
  optional code, a format string, and arguments (§12.9). **It never allocates.**
- A lookup that returns "the thing or nothing" returns `^T`, not `T?`. `T?` is for optional
  *values*; `^T` is for optional *access*, and it is one pointer.

### 9.2 The `Error` type

```dz
pub struct Error {
    pub code:     u32                // 0 = unspecified; use your own enum `as u32`
    pub message:  str                // static storage, or a formatted temp
    pub location: &SourceLocation    // where the error was created; static storage
}
```

32 bytes, no allocation, no heap, no vtable. The location is a reference into static storage
filled in by `#location` (§12.7) — `e.location.line` reads it directly — so an error tells you
where it came from at zero runtime cost, which is most of what a stack trace would have given
you, for free. It is the one reference the language stores in a struct: §18.3 bans that
because a stored reference could outlive what it points at, and a location lives in static
storage for the whole run of the program. User structs still cannot. `[DECIDED — §18.30]`

**`Error` and `SourceLocation` are provided by the language, not by the stdlib.** Both names
are in scope everywhere with no import, like `str` and `bool`, because `or { e -> … }` binds
one and there is nowhere for an import to come from. They are the only two structs with that
status. `[DECIDED]`

Domain-specific codes are ordinary enums:

```dz
pub enum ErrorCode : u32 { NotFound = 1, BadMagic, UnsupportedVersion, OutOfMemory }

return error(ErrorCode::NotFound, "no such asset: {}", path)
```

> **Typed errors are deliberately not supported.** A fallible signature always carries
> `Error`; there is no per-call error type. This is what buys the native representation and
> makes propagation free of conversion rules. If exhaustive matching on error kinds is ever
> genuinely needed, the reserved extension syntax is `LoadError!-> Texture`, which can be
> added without breaking anything. `[DECIDED — §18.7]`

### 9.3 Representation

| Shape | Size | Note |
|---|---|---|
| `T?` | `sizeof(T)` + 1, padded | `{ T value; bool has; }` |
| `!-> T` | `max(sizeof(T), 32)` + tag | `{ u8 tag; union { T value; Error error; } }`, returned by value |
| `^T` | one pointer | absence is the null bit pattern; nothing added |

Note what is *not* here. Earlier drafts niche-optimised `?^T` and `?&T` down to one pointer;
those types no longer exist, so a "might be there" lookup returns `^T` and the check is a
null check — the same code you would have written by hand, with the compiler enforcing that
you write it (§9.4).

### 9.4 The `or` clause

`or` is a postfix binary operator whose left side is fallible — a `!->` call, a `T?`, a
`^T`, or a handle such as an `Rc` (§14.5) — and whose right side says what to do when it
fails. A `&T?` is read as the optional it refers to, when `T` can be copied (§18.35); a
handle gives a reference to its target, as a pointer does, and keeps the target alive for
the rest of the enclosing block.

```dz
let tex  = load_texture(path) or Textures::Missing            // fall back
let f    = File::open(path)   or return                       // propagate
let hp   = find_player(id)    or continue                     // diverge
let btn  = ctx.btn            or panic("no button")           // assert
let cfg  = load_config()      or { e ->                       // handle
    log::warn("config: {} ({}:{})", e.message, e.location.file, e.location.line)
    Config{}
}
file.write(data) or ignore                                    // deliberately drop
```

**The single rule: an `or` clause must either produce a `T` or diverge.** Everything above
is an instance of it. The clause forms are:

| Form | Meaning |
|---|---|
| *expression* | the fallback value; must be a `T` |
| `{ … }` | a block whose final expression is a `T`, or which diverges |
| `{ e -> … }` | the same, with the `Error` bound to `e` |
| `return` | propagate the failure (§9.5) |
| `return expr` / `break` / `continue` | ordinary divergence |
| `panic("…")` | abort (§9.8) |
| `unreachable` | assert non-failure; see below |
| `ignore` | statement position only; discards the value |

- The error binding must be named explicitly. There is no implicit `it`. `[PROPOSED]`
- On a `T?` or a `^T` there is no payload, so `{ e -> … }` is an error; write `{ … }`.
- `or` binds looser than `||` and tighter than assignment, and is **non-associative**
  (§11.1): `a() or b() or c` is a syntax error, not a fallback chain. The first clause has
  already produced a value or diverged, so a second `or` could only apply to something that
  can no longer fail. Comparison is non-associative for the same reason (§11.1).

**A fallback that can itself fail** gets its own clause, which is what the block form is
for:

```dz
let tex = load(path) or { load(fallback_path) or Textures::Missing }
```

Read outside-in: try `load(path)`; if that fails try `load(fallback_path)`; if that fails
too, use the placeholder. Each fallible call has exactly one clause, and the nesting is
visible rather than inferred from precedence. Flat chaining cannot express this and is
rejected instead of being silently misread. `[DECIDED]`

**`or unreachable`** asserts that the failure cannot happen. In debug builds it panics with
a `#location`; in release and shipping builds it is free, and the compiler assumes the
success path (`__builtin_unreachable`). Reaching it there is undefined behaviour. It is the
escape hatch for hot paths where a null check is provably redundant, and it is spelled
loudly on purpose. `[DECIDED — §18.20]`

**`or ignore`** is statement position only, because it neither produces a `T` nor diverges
and so has nothing to bind:

```dz
file.write(data) or ignore              // ok
let x = f() or ignore                   // error[DZ0524]: `ignore` produces no value
```

`ignore` is also valid as a bare statement inside a block clause, where it marks a
deliberate drop (§9.5). `[PROPOSED — this dual role is the part of the design least
supported by real code; revisit after the M12 stdlib pass.]`

**`if let`** is for when both outcomes carry on — "do this if it is there, that if it is
not, then continue" — which an `or` clause cannot say, since it must produce a value or
diverge:

```dz
if let m = find_movie(&mut list, "Nemo") {
    println("found {}", m.id)        // m: Movie
} else {
    println("not found")
}
```

**The binding holds the value as a `for` or `case` binding holds its element** (§10.3,
§10.4), whatever the subject — a `T?`, a `&T?`, a `^T`, or a handle (§14.5):

```dz
if let v = p { … }          // v: T, a copy that cannot be changed (`mut v`: one that can)
if let &v = p { … }         // v: &T, refers to the value where it lives
if let &mut v = p { … }     // v: &mut T, and writes land there
```

- **`v`** is a copy, so `T` must be copyable where the value lives somewhere else — behind a
  pointer, a reference or a handle (`DZ0403`, with `&v` as the help). Out of an optional
  the `if let` owns, such as a call's result, it is the value itself, moved.
- **`&v`** refers to the value in place; nothing is copied.
- **`&mut v`** needs something writable: a `^mut T`, a `&mut T?`, or an optional or handle
  in a place that may be written — a `let mut`, a field through `&mut self` (`DZ0507`).

So `if let &mut p = ptr` is the checked pointer-to-reference conversion of §5.2. The binding
exists in the first block only, `else if` chains as for any `if`, and `if let` is an
expression when both blocks produce a value. It does not take a fallible call: its `else`
has no `Error` to hand over, so the failure would be dropped unseen, which §9.5 exists to
prevent — that is `or`'s job (`DZ0529`). `[DECIDED — §18.36, §18.61]`

An optional can also be tested without taking the value out: `m == none` and `m != none`.
Nothing else compares a `T?` — two optionals have no `==` (§18.36).

### 9.5 Propagation: `or return`

Bare `or return` means **propagate the failure**, and is legal only where the enclosing
signature can carry it:

| Left side | Enclosing signature | `or return` |
|---|---|---|
| `!->` call | `!->` | returns the `Error` ✅ |
| `!->` call | anything else | error[DZ0516] — the error has nowhere to go |
| `T?` | `-> U?` | returns `none` ✅ |
| `T?` | `-> void` | returns ✅ (nothing is discarded) |
| `^T` | `-> void` | returns ✅ |
| `^T` | `-> U?` | returns `none` ✅ |

```dz
pub fn load_level(path: str, alloc: ^Allocator) !-> Level {
    let file = File::open(path) or return
    let hdr  = file.read_header() or return

    if hdr.magic != LEVEL_MAGIC {
        return error(ErrorCode::BadMagic, "not a level file: {}", path)
    }

    let mut data = List<u8>::try_new(hdr.size, alloc) or return
    file.read_into(&mut data) or return

    return Level::parse(data)
}
```

Reserving bare `or return` for propagation is what keeps "I passed this up" and "I threw
this away" from sharing a spelling. A non-fallible function that wants to bail on failure
writes the discard out:

```dz
fn update_hud(&mut self) {
    let cfg = load_config() or { ignore; return }               // explicit drop
    let cfg = load_config() or { e -> log::debug("{}", e.message); return }   // better
}
```

Any clause that binds and uses the error is self-evidently deliberate, so neither form
warns. `[DECIDED — 18.8]`

### 9.6 Fallback

The most common thing game code does with a failure is substitute something and keep
rendering, so that is the cheapest clause to write:

```dz
let tex = load_texture(path) or Textures::Missing
let cfg = load_config() or Config{}
let hp  = self.find_player(id) or return
```

A missing texture becomes the pink checkerboard, a missing config becomes the defaults, and
the frame keeps going.

### 9.7 Consumption

**A fallible call must be consumed in the statement in which it appears.** There is no `T!`
type, so there is nowhere for the value to go otherwise.

```dz
file.write(data)                        // error[DZ0510]: fallible call not consumed
file.write(data) or ignore              // ok — explicitly dropped
file.write(data) or return              // ok — propagated
let n = file.write(data) or 0           // ok — handled
f(g() or return)                        // ok — `or` is an expression
```

`T?` and `^T` are ordinary values and are not subject to this rule; they may be stored,
passed, and returned freely. The rule exists because a fallible call carries an `Error` that
someone has to look at. `[DECIDED]`

This replaces the v0.4 must-use rule. It is stricter — the old rule permitted a `!T` to sit
in a binding and be examined later — and that is the intended trade: a result that outlives
its statement is a design mistake in a language with no way to store one.

### 9.8 What is *not* an error: panics

Most things that go wrong in a game are not recoverable and should not be modelled as
errors. `panic("...")` aborts with a message, a `SourceLocation`, and a stack trace in debug
builds.

The location is the `panic`'s own line, unless a `SourceLocation` is passed as the **last
argument**, after the format arguments: `panic("missing {}", what, loc)`. That is how a
helper reports its caller's line — it takes `loc: SourceLocation = #location` (§12.7) and
hands it on. A `SourceLocation` cannot be formatted, so it is never mistaken for a value.
`assert(cond, msg = "")` is the standard library's helper of this kind. `[DECIDED]`

Panics are for: out-of-bounds indexing, failed assertions,
exhausted fixed-size pools, and violated invariants. Errors are for: I/O, parsing,
user-supplied data, and anything crossing a boundary you do not control.

**Allocation failure is a panic by default.** `alloc` returns `^mut u8` and panics on failure;
`fn try_alloc(n: usize) !-> ^mut u8` covers the rare code (console memory budgets, streaming)
that genuinely handles OOM. Making every allocation fallible would put `or return` on every
line of `Buffer` for a case that, on a PC game, means the process is already dead.
`[PROPOSED]`

Panics compile to a call into the Dazzle runtime shim (§16.4), never to a C++ exception.
The generated C++ builds cleanly with `-fno-exceptions`.

### 9.9 Reserved: `collect`

`collect` is reserved as a fourth clause for accumulating failures rather than acting on
each one — batch validation, asset manifests, anything where you want every error rather
than the first:

```dz
let mut errs: List<Error>
for path in manifest { load_asset(path) collect errs }
```

Nothing in this section forecloses it, because it is another eliminator rather than a new
type. Not in v0.5. `[OPEN — 18.18]`

---

## 10. Statements and control flow

### 10.1 `if`

```dz
if x > 0 {
    ...
} else if x < 0 {
    ...
} else {
    ...
}
```

Parentheses around the condition are not used. Braces are mandatory. The condition must be
`bool` — there is no truthiness, and no pointer-to-bool conversion (`if p != null`).
`if let x = v { … }` takes the value out of a `T?`, a `^T` or a handle (`Rc`, `Weak`, `Box`) for
the first block (§9.4, §14.5).

### 10.2 `while`

```dz
while running {
    ...
}

while true {
    if done { break }
}
```

No `do`/`while`. `break` and `continue` accept an optional label:

```dz
outer: while true {
    for &e in entities {
        if e.id == target { break outer }
    }
}
```

Neither carries a value: a loop is a statement, so there is nothing for a value to become.
A result is assigned to a variable declared before the loop. `[DECIDED — §18.28]`

`[PROPOSED]`

### 10.3 `switch`

```dz
switch state {
    case Idle          { ... }
    case Walking(w)    { speed = w.speed }
    case Falling(&mut f) { f.time += dt }      // writes into the union
    default            { ... }
}
```

- Exhaustive over unions and enums; a missing variant is an error listing which. `default`
  silences it, and the LSP offers a "fill in missing cases" action.
- No fallthrough. Ever. So `break` and `continue` inside an arm act on the enclosing loop —
  there is nothing in a `switch` to break out of.
- `case A, B { ... }` matches multiple variants when neither binds a payload.
- A payload binding holds the payload exactly as a `for` binding holds an element (§10.4):
  `case Walking(w)` is a copy that cannot be changed, `case Walking(&w)` a read-only
  reference, and `case Falling(&mut f)` a writable one, which needs the value switched on to
  be writable — a `let mut`, a `&mut` or a `^mut`. A payload that cannot be copied is bound
  with `&` or `&mut`. `[DECIDED — §18.43]`
- `switch` is also an expression when every arm yields the same type.
- Naming the same variant or value in two labels is an error: the later one could never
  match.

An integer, a `char`, a `bool` or a `str` is matched against **constants** instead of
variants. A label is anything known while compiling — a literal, a `const`, or arithmetic
on those — of the subject's type, and there is no exhaustiveness requirement: with no
matching arm and no `default`, nothing runs. A float cannot be switched on, since it is never
matched exactly.

```dz
switch key {
    case 'w', 'k'          { move_up() }
    case KEY_ESCAPE        { quit() }
    case FIRST_SLOT + 1    { select(1) }
    default                { }
}

switch command {
    case "quit" { return }
    case "help" { print_help() }
}
```

A bare name in a label means a variant when the subject is an enum or union, and a
constant otherwise, so `case Idle` and `case MAX` are written the same way.

### 10.4 `for`

```dz
for i in 0..count           { ... }     // exclusive
for i in 0..=count          { ... }     // inclusive
for c in text               { ... }     // iterates chars of a str
for e in entities           { ... }     // e is a copy of each element
for &e in entities          { ... }     // e refers to each element
for &mut e in entities      { ... }     // e refers to each element, and may change it
for i, &e in entities.indexed() { ... } // a pass count, then the element
for id, &btn, &mut state in query { ... } // a tuple element, taken apart
```

**How a binding holds its element** is written on the binding, as C++ writes `auto`,
`const auto&` and `auto&` — not on the sequence. `[DECIDED — §18.42]`

- **`e`** is a copy that cannot be changed. Writing `e.hp -= 1` would change a copy and
  nothing else, so it is an error, with a help pointing at `&mut e`. An element that cannot
  be copied — a type with a `drop` — cannot be bound this way either (`DZ0403`); refer to it
  with `&e`, or consume the sequence. A copy is always a real copy — its `copy` runs, and
  its `drop` at the end of the pass — so the compiler never quietly reads through a
  reference instead: with RAII that would change what the program does.
- **`&e`** refers to the element, read-only. Nothing is copied.
- **`&mut e`** refers to the element and may change it: `e.hp -= 1`, or `e = 0` for a number,
  writes into the sequence (§5.2). The sequence must be writable — a `let mut` array, a
  `&mut` or `^mut` to one, a `[]mut T` — and an iterator must hand out `&mut` references
  (`DZ0507` otherwise).

`&e` and `&mut e` need an element that lives somewhere. An array, a slice and an iterator
whose `get` returns `&T` or `&mut T` provide one; a range's numbers, an iterator whose `get`
returns a value, and a consuming loop do not, and take only `e` (`DZ0549`). What an iterator's
`Item` is decides what its loops may ask for: a query that returns `&mut State` can be
written through, and one that returns `&State` cannot.

**A tuple element is taken apart by naming its parts**, without parentheses: `for id, &btn,
&mut state in query` over an `Item` of `(EntityId, &Button, &mut State)`. Each part takes its
own `&` or `&mut`. A part that is itself a reference refers to what it names; the other parts
live where the tuple does. The number of names must be the tuple's size (`DZ0549`), so several
names always mean the parts of one element and never anything else.

**A pass count** comes from `indexed()`: `for i, e in xs.indexed()` binds a `usize` counting
passes from zero, then the element with the same choice of `e`, `&e` or `&mut e`. It works on
anything `for` does. For now `indexed()` is known only in a loop's head.

**Consuming.** `for e in move(entities)` moves each element out into `e` and leaves
`entities` moved-from (§6.4) — the one way to take move-only elements by value.

`for e in xs` desugars to the three `Iterate` methods, with the iterator in a scope of its
own so the loop variable may shadow anything outside it:

```dz
{
    let mut it = <xs as an iterator>
    while it.valid() {
        let e = it.get()
        { ... body ... }
        it.advance()
    }
}
```

**`<xs as an iterator>`** is `xs` itself when it implements `Iterate`. Otherwise it is
`xs.iter()` from **`Iterable`**, or `xs.iter_mut()` from **`IterableMut`** when a binding is
`&mut` — the standard library traits a container implements to hand out a fresh iterator, since an
iterator keeps its position in itself and a container must not:

```dz
pub trait Iterable {
    type Iter : Iterate
    fn iter(&self) -> Iter
}
pub trait IterableMut {
    type IterMut : Iterate
    fn iter_mut(&mut self) -> IterMut
}
```

`List` and `HashMap` implement both — a `HashMap`'s `Item` is `(&K, &V)`, so `for k, &v in
map` — and a generic function can take `C: Iterable` and loop over it. `[DECIDED — §18.51]`

`e` is bound from `Item`, read off the implementer. `advance` runs after the body on every
completed pass, and `continue` jumps to it rather than past it, so a `continue` cannot spin
forever. `break` leaves the whole construct.

### 10.5 `defer`

```dz
fn upload(&mut self) {
    gl::bind_buffer(m_handle)
    defer gl::bind_buffer(0)
    // ...
}
```

Runs at scope exit, after that scope's drops, in reverse order of registration. Runs on
every exit path including `return`, `break`, and a diverging `or` clause.

### 10.6 Casts

```dz
x as i64            // numeric conversion, explicit, may truncate
p as ^u8            // pointer reinterpretation — `unsafe` (§5.2)
e as u16            // enum to its backing type
p as usize          // a pointer's address, for aligning or hashing
n as ^mut u8        // an address back to a pointer — `unsafe` (§5.2)
```

`as` never invokes user code and never fails. Fallible conversions are library functions
returning `T?`, or marked `!->` when they have something to say about the failure. `as`
never fails, so there is deliberately no `as ref` for `^T` → `&T`; that conversion is an
`or` clause (§5.2) precisely because it is partial.

---

## 11. Operators

### 11.1 Precedence

Highest to lowest:

```
1   ::  .  ()  []  ^(deref)        postfix
2   -  !  ~  &  &mut  move        prefix
3   as                             cast
4   *  /  %
5   +  -
6   <<  >>
7   &                              bitwise and
8   ^^                             bitwise xor
9   |                              bitwise or
10  ..  ..=                        range
11  <  >  <=  >=  ==  !=           comparison (non-associative)
12  &&
13  ||
14  or                            failure clause (non-associative)
15  =  +=  -=  *=  /=  %=          assignment (statement, not expression)
```

Comparison is non-associative: `a < b < c` is a syntax error, not a silent bug. Mixing `&`,
`|`, and `^^` with comparison requires parentheses. `[PROPOSED]`

`or` is non-associative too: `a() or b() or c` is a syntax error (§9.4). `[DECIDED]`

### 11.2 Operator overloading

Overloading is via traits, and only for the operators where a user type genuinely has the
same meaning:

```dz
pub trait Add<Rhs = Self> { type Output; fn add(&self, rhs: Rhs) -> Output }
pub trait Sub<Rhs = Self> { type Output; fn sub(&self, rhs: Rhs) -> Output }
pub trait Mul<Rhs = Self> { type Output; fn mul(&self, rhs: Rhs) -> Output }
pub trait Div<Rhs = Self> { type Output; fn div(&self, rhs: Rhs) -> Output }
pub trait Rem<Rhs = Self> { type Output; fn rem(&self, rhs: Rhs) -> Output }
pub trait Neg              { type Output; fn neg(&self) -> Output }
pub trait Eq               { fn eq(&self, rhs: &Self) -> bool }
pub trait Ord : Eq         { fn cmp(&self, rhs: &Self) -> Ordering }
pub trait Index<I>         { type Output; fn index(&self, i: I) -> &Output }
pub trait IndexMut<I>      { type Output; fn index_mut(&mut self, i: I) -> &mut Output }
```

`b[i]` on a user type calls `index_mut` when `b` is a writable place that implements
`IndexMut` — so `b[i] = v` and `b[i] += 1` write through it — and `index` otherwise.

A game language without `v1 + v2 * scalar` for vectors and matrices is not usable, so this
is not optional. `[DECIDED]`

**The primitives implement them**, so a bound is satisfied by a number exactly as by a user
type — `fn largest<T: Ord>` accepts an `i32`. Every integer and float is `Eq`, `Ord`, `Add`,
`Sub`, `Mul` and `Div`; integers are also `Rem`; signed integers and floats are `Neg`;
`bool` is `Eq` and `char` is `Eq` and `Ord`. Each arithmetic implementation has `Rhs =
Self` and nothing else, since there are no implicit numeric conversions (§5.1). The
operators on these types are still the machine's own — the implementations exist to satisfy
bounds, not to put a call where an `add` instruction was.

**A type may implement one operator trait at several arguments**, one `impl` each, and the
right operand's type picks one — an exact match first, then the type an untyped literal would
default to. Each implementation chooses its own associated types:

```dz
impl Mul<f32> for Vec2 {
    type Output = Vec2
    fn mul(&self, rhs: f32) -> Vec2 { return Vec2{ x: x * rhs, y: y * rhs } }
}
impl Mul<Vec2> for Vec2 {
    type Output = Vec2
    fn mul(&self, rhs: Vec2) -> Vec2 { return Vec2{ x: x * rhs.x, y: y * rhs.y } }
}
impl Mul<Vec4> for Mat4 {
    type Output = Vec4                 // Mat4 × Vec4 is a Vec4, while Mat4 × f32 is a Mat4
    fn mul(&self, rhs: Vec4) -> Vec4 { ... }
}
let a = v * 2.0      // Mul<f32>
let b = v * w        // Mul<Vec2>
```

That is the only overloading there is (§18.5), and the only place it is needed: the argument
whose type decides is right there at the call. A literal that fits several and defaults to
none — `3` against `Mul<i64>` and `Mul<u8>` — is an error asking for its type.
`[DECIDED — §18.27, §18.29]`

**The right-hand side is taken by value, and the comparison side by reference.** That is not
an inconsistency; the two cases want opposite things.

- `Rhs` may be a scalar. `Vec3 : Mul<f32>` is the whole point of the `Rhs` parameter, and a
  `&f32` cannot be read — `&T` reaches through `.` for members and is otherwise not a value
  (§5.2). By value it is also the better signature for what these traits are for: a 4-byte
  copy beats an indirection, and `Vec3`, `Mat4` and friends are copyable by construction.
- `Eq` and `Ord` take `&Self` because **comparing must not consume.** A move-only type
  (§6.3) passed by value would be moved, so `a == b` would destroy `b` — and comparison is
  exactly the operation that has no business doing that. `Self` is the receiver's own type
  there, so it is always something `&` can reach into.

`[DECIDED — §18.25]`

**Not overloadable:** `&&`, `||`, `!` (short-circuiting must stay predictable), `=`
(assignment is defined by §6), `.` and `::`, and there is no `operator()`. Compound
assignment `a += b` desugars to `a = a + b` and is not separately overloadable.
`[PROPOSED]`

---

## 12. Metaprogramming

Dazzle's metaprogramming is the **declarative macro** system you sketched: a macro declares
the syntax it accepts and the code it expands to. `[DECIDED]`

The reason for this choice over comptime code execution is tooling: because a macro
declares its grammar up front, the LSP can parse a macro invocation, complete inside it,
and report errors at the right span — none of which is possible when a macro is an opaque
function that eats tokens.

### 12.1 Anatomy

```dz
macro machine -> declaration {
    syntax Field {                                 // t: f32   or   t: f32 = 0
        $name:#name : $type:#type ( = $default:#value )?
    }

    syntax State {                                 // state Grow(t: f32 = 0) { ... }
        state $name:#name ( $fields:Field[,] )? {
            $items:#declaration[]
            $body:#code[]
        }
    }

    pattern {                                      // ButtonState(ButtonCtx) { ... }
        $name:#name ( $ctx:#type ) {
            $states:State[]
        }
    }

    expand {
        struct $name : StateMachine<$ctx> {
            union States {
                $states[,] {
                    struct $.name : State<$ctx, States> {
                        $.fields[,] {
                            $.name: $.type $.default? { = $.default }
                        }
                        $.items
                        fn update(&mut self, ctx: &mut $ctx) -> States? { $.body }
                    }
                }
            }
        }
    }
}
```

- `macro NAME -> KIND` declares where the macro may appear: `declaration`, `expression`,
  `statement`, or `type`.
- `syntax NAME { ... }` declares a reusable sub-grammar, which is what keeps the pattern
  readable and is why nested repetition works.
- `pattern { ... }` is the top-level grammar of an invocation.
- `expand { ... }` is the output, written as Dazzle source with `$` holes.

### 12.2 Fragment kinds

| Kind | Matches |
|---|---|
| `#name` | a single identifier |
| `#type` | a type expression |
| `#value` | a constant expression |
| `#expr` | any expression |
| `#code` | a sequence of statements |
| `#stmt` | one statement |
| `#declaration` | one declaration (fn, struct, field, …) |
| `#literal` | a literal token |
| `#block` | a braced block |

Each fragment kind is parsed by the *real* parser, so a `#type` hole in a macro gets the
same diagnostics, the same completion, and the same hover as a type written anywhere else.

### 12.3 Repetition and options

- `$x:Kind[]` — zero or more, no separator.
- `$x:Kind[,]` — zero or more, comma-separated (trailing comma allowed).
- `( ... )?` — optional group.
- In `expand`, `$xs[,] { ... }` iterates a captured repetition, emitting the body once per
  element with `$.field` referring to that element's captures.
- `$x? { ... }` in `expand` emits the body only if the optional capture matched.

### 12.4 Hygiene

Identifiers *introduced by* the `expand` block are hygienic: a temporary named `tmp` inside
a macro cannot collide with the caller's `tmp`. Identifiers *captured from* the invocation
carry the caller's scope, which is what makes `$.body` in `machine` able to see `ctx`.

This is the standard hygiene rule and it is the one that makes macros safe to use without
reading their bodies. `[DECIDED]`

### 12.5 Invocation

```dz
#machine ButtonState(ButtonCtx) { ... }   // declaration position
let v = vec![1, 2, 3]                     // expression position
```

Declaration macros are invoked with a leading `#`; expression and statement macros with a
trailing `!`. The two positions are lexically distinguishable, so both spellings can coexist
without ambiguity in the parser. `[DECIDED — §18.9]`

### 12.6 Errors inside macros

A macro may reject an invocation deliberately:

```dz
expand {
    #error($name, "a state machine needs at least one state")
}
```

Diagnostics produced during expansion are reported at the *invocation* span by default, and
the compiler adds an "in expansion of `#machine`" note pointing at the macro. Expanded code
is available via `dz expand file.dz`, which prints the post-expansion source — the single
most important debugging tool for a macro system, and cheap to build.

### 12.7 Compiler intrinsics

Intrinsics use `#` in expression or parameter position. They are resolved during checking
and become constants in the C++ output.

```dz
#file            str    the source file of the current call site
#line            u32
#column          u32
#function        str    the enclosing function's name
#module          str
#location        SourceLocation   { file, function, line, column }
```

Used as a **default parameter value**, an intrinsic is evaluated at the *caller's* location,
which is exactly the `#src_code` behaviour you asked for (and the same trick as Odin's
`#caller_location` and C++20's `std::source_location`). A defaulted `#location`'s
`column` is 0: the caller's line is exact, its column is not known there.
`[DECIDED — §18.48]`

```dz
pub fn assert(cond: bool, msg: str = "", loc: SourceLocation = #location) {
    if !cond {
        panic("assertion failed: {}", msg, loc)   // reports `loc`, the caller's line (§9.8)
    }
}

fn update() {
    assert(dt > 0, "bad delta time")   // loc is update()'s line, not assert()'s
}
```

The generated C++ passes a pre-built static `SourceLocation` at each call site: zero runtime
cost, no `__FILE__` macro trickery.

```dz
#expr_text(e)    str    the literal source text of expression `e`
```

is what lets an assertion macro print `assert(hp > 0)` rather than `assert(false)`.
`[PROPOSED]`

### 12.8 Attributes

```dz
#[test]         fn test_buffer_grows() { ... }
#[bench]        fn bench_push() { ... }
#[thread_local] static SCRATCH: TempArena = TempArena{}
```

Attributes deliberately share the `#` sigil with macros and intrinsics, because they are the
same category of thing: **`#` means "the compiler does something here at compile time."**
An attribute is really just a macro in attribute position — `#[derive(Serialize)]` is a
compile-time transform on the declaration that follows it, no different in kind from
`#machine`. `[DECIDED — §18.2]`

The three `#` forms are distinguished by the character that follows, so the parser never
has to guess:

| Form | Position | Meaning |
|---|---|---|
| `#[name]`, `#[name(...)]` | before a declaration | attribute |
| `#name ...` | declaration position | declaration macro (§12.5) |
| `#name`, `#name(...)` | expression or default-argument position | intrinsic (§12.7) |

Intrinsic names are reserved: a macro may not be called `file`, `line`, `column`,
`function`, `module`, `location`, or `expr_text`.

**Every attribute is one the language defines, written where it applies.** An attribute the
language does not have is an error (`DZ0314`): a misspelled `#[tset]` accepted in silence
would leave a test out of every run, and nothing would say so. So is a known attribute on a
declaration it does not apply to, with arguments it does not take, or written twice
(`DZ0315`). `[DECIDED — §18.60]`

| Attribute | On | Meaning |
|---|---|---|
| `#[test]` | a function at module level | a test `dz test` runs (§19 M14) |
| `#[bench]` | a function at module level | a benchmark `dz bench` times (§19 M15) |
| `#[thread_local]` | a `static` | one variable per thread (§7.7) |

Attributes this specification describes but the compiler does not implement yet —
`#[inline]`, `#[align(n)]`, `#[packed]`, `#[deprecated(…)]`, `#[export]` (§15.5),
`#[derive(…)]` (§12), `#[persist]` (§17.6) and `#[raw]` (§7.5) — are errors that say so
(`DZ0316`), rather than being accepted and doing nothing. Conditional compilation is `#if`
(§12.10), not an attribute.

### 12.9 Format strings

Formatting is not string interpolation and not varargs — it is a compile-time-checked
call. `print`, `println`, `eprint`, `eprintln`, `panic`, `error` and `String::format` take a
**string literal** as their format string, and the compiler parses it during checking. `print` and `println` write to standard output; `eprint` and
`eprintln` write the same way to standard error, for diagnostics that must not mix with a
program's output:

```dz
println("player {} has {} hp", name, hp)
println("{:.2}", elapsed)          // 2 decimal places
println("{:x}", flags)             // hex
println("{{literal braces}}")
```

Because the format string is parsed at compile time, an argument-count mismatch, an
unknown format spec, or an argument whose type has no `Format` impl are all ordinary
compile errors pointing at the offending `{}` — not runtime garbage, and not a `printf`
mismatch that corrupts the stack.

The spec set is deliberately small, and each one names the kind of value it suits:

| Spec | Writes | Suits |
|---|---|---|
| `{}` | the value, in its natural form | anything formattable |
| `{:x}` / `{:X}` | hex, lower / upper case | integers |
| `{:.N}` | `N` digits after the point | floats |
| `{{` / `}}` | a literal brace | — |

**Text you keep is a `String`.** There is no global `format`: `String::format` builds a new
`String`, in memory from `Allocators::Global` unless an allocator is passed by name (§7.2):

```dz
let hud = String::format("Score {}   Lives {}", g.score, g.lives)
let msg = String::format("{} took {:.1} ms", name, ms, allocator = Allocators::Temp)
```

**`Format` is how a value writes itself.** Its arguments are `..&Format`, so anything that
implements the trait can be one — the numbers, `bool`, `char`, `str`, `cstr` and `String`
implement it in `std`, and a type of your own implements it to be formatted too:

```dz
pub trait Format {
    fn format(&self, f: &mut Formatter)
}

impl Format for Vec2 {
    fn format(&self, f: &mut Formatter) {
        f.write_str("(")
        self.x.format(f)
        f.write_str(", ")
        self.y.format(f)
        f.write_str(")")
    }
}
```

A `Formatter` appends to a `String` with `write_str` and `write_char`, and says what the
placeholder asked for: `hex()` and `upper()` for `{:x}`/`{:X}`, `precision()` for `{:.N}`. On
the built-in kinds a spec must suit the value, as in the table above; on a type with its own
`Format` it is that implementation's to interpret.

**A function of one's own that takes a format string waits for macros** (§19 M10), as
`println` does: a format string is checked only where the compiler knows the call is
formatting, and `String::format` is the one library function it knows. A wrapper such as a
`log` with levels is written as a macro once there are macros; until then, it takes the
`String` its caller built.

`print`, `println`, `eprint`, `eprintln` and `panic` are still the compiler's own, and still
take only the built-in kinds: a value with its own `Format` is printed through
`String::format`. They become macros (§19 M10) — expanded at the call, which is also when
`{}` lowers to a direct sequence of `Format::format` calls with no intermediate string.
`[DECIDED — §18.73]`

Interpolation (`"hp is {hp}"`) is deliberately absent for now: it hides a name lookup
inside a literal, which complicates the lexer, the LSP's rename, and macro capture. It can
be added later as pure sugar over this. `[PROPOSED]`

### 12.10 Compile-time conditionals

`#if` keeps code in a build or leaves it out, by **build flags**:

```dz
#if TRACK_MEMORY {
    static TRACKER: Tracker = Tracker{}
}

pub fn alloc(size: usize, loc: SourceLocation = #location) -> ^mut u8 {
    let p = heap_alloc(size)
    #if TRACK_MEMORY {
        unsafe { TRACKER.record(p, size, loc) }
    }
    return p
}

#if OS_WINDOWS {
    fn open_window() { … }
} #else #if OS_LINUX || OS_MACOS {
    fn open_window() { … }
} #else {
    fn open_window() { panic("no window on this platform") }
}
```

- **Where**: around declarations, around a type's fields and methods, and around statements.
  A statement branch opens no scope — its statements belong to the enclosing block.
  `#if` is never an expression: a value that depends on a flag is an ordinary `if` on a
  `const`.
- **Conditions** are flags joined by `!`, `&&` and `||`, with parentheses. A flag is on or
  off; nothing is compared.
- **Only registered flags exist.** An unknown name is an error (DZ0309), never "off", so a
  misspelt `#if DEBUGG` cannot silently compile nothing. The compiler defines `DEBUG`,
  `RELEASE`, `SHIPPING` (from the build mode) and `OS_WINDOWS`, `OS_LINUX`, `OS_MACOS` in
  every package. A package registers its own under `[flags]` in its dazzle.toml (§3.1),
  each off unless its `default` says otherwise, and may not reuse a built-in name.
- **Whose flags**: a package reads its own flags bare, and a dependency's through the
  dependency — `#if engine::PROFILE`. A package's flags are turned on by what depends on it
  (`engine = { path = "…", flags = ["PROFILE"] }`) or from the command line for one build:
  `dz run --flag CHEATS`, `--flag engine::PROFILE`. The standard library's `TRACK_MEMORY` is
  `--track-memory` or `[build] track_memory = true`.
- **The inactive branch is parsed, not checked.** It is formatted and shown by the editor,
  but never resolved or type-checked, so it may name what exists only on another platform or
  under another flag. What that costs is that a branch nobody builds can rot unnoticed; what
  it buys is that platform code can be written at all. `[DECIDED — §18.48]`

---

## 13. Reflection

Reflection is **a compile-time-generated static table**, in the Odin spirit: your data
carries nothing, and the metadata lives in read-only memory beside it. `[DECIDED]`

### 13.1 The type table

The compiler writes one table per program. A `TypeId` is an index into it, and **nothing in
your structs ever stores one unless you put it there yourself**. The table's types are
ordinary declarations of the standard library (§14.4):

```dz
pub struct TypeId {
    pub index: u32
}

pub enum TypeKind : u8 {
    Void  Bool  Char  I8  I16  I32  I64  Isize  U8  U16  U32  U64  Usize  F32  F64
    Str  Cstr  Pointer  Slice  Array  Optional  Tuple  Function  TraitObject
    Struct  Union  Enum
}

pub struct TypeInfo {
    pub name:       str             // `Transform`, `List<i32>`, `[]u8` — as Dazzle writes it
    pub module_name: str            // the declaring module of a named type; empty otherwise
    pub size:       usize
    pub align:      usize
    pub kind:       TypeKind
    pub fields:     []FieldInfo     // what a value is made of; see below
    pub variants:   []VariantInfo   // an enum's or a union's
    pub elem:       TypeId          // what a pointer, slice, array or optional holds; an enum's backing type
    pub len:        usize           // an array's length
    pub mutable:    bool            // a `^mut`, `&mut` or `[]mut`
    pub tag_offset: usize           // where a union keeps which variant it holds (a `u32`)
}

pub struct FieldInfo {
    pub name:    str
    pub type_id: TypeId
    pub offset:  usize              // bytes from the start of the value
    pub vis:     Visibility         // Private, Protected, Module, Public
    pub owning:  bool               // an `own` field
    pub tag:     str                // the field's tag, `json:"hp"`, or empty

    pub fn tag_value(&self, key: str) -> str?   // `f.tag_value("json")` → `"hp"`
}

pub struct VariantInfo {
    pub name:   str
    pub value:  i64                 // an enum variant's value; a union variant's tag
    pub fields: []FieldInfo         // a union variant's payload, offsets from the union's start
}

pub fn typeinfo_of<T>() -> &TypeInfo    // resolved at compile time; a static reference
pub fn typeid_of<T>()   -> TypeId
pub fn typeinfo(id: TypeId) -> &TypeInfo
```

**Every kind of type is described**, so generic code can walk any value it is handed
`[DECIDED — §18.59]`:

| `kind` | What else it fills in |
|---|---|
| the primitives, `Void` … `F64` | nothing: `size` says how many bytes to read |
| `Str`, `Cstr` | `fields`: `ptr` and `len` for a `str`; a `cstr` is one pointer |
| `Pointer` | `elem`, `mutable` — `^T`, `^mut T`, `&T`, `&mut T` and `own` pointers alike |
| `Slice` | `elem`, `mutable`, and `fields`: `ptr`, `len` |
| `Array` | `elem`, `len`: the elements are contiguous, `elem`'s size apart |
| `Optional` | `elem`, and `fields`: `value` and `has`, a `bool` saying whether it is there |
| `Tuple` | `fields` named `0`, `1`, … |
| `Function`, `TraitObject` | nothing more |
| `Struct` | `fields`: its own and every base's, bases first (§7.3) |
| `Enum` | `variants` with their values, and `elem`, the backing integer type |
| `Union` | `variants`, each with its payload's fields — a variant carrying an existing struct has one field, named after that struct — and `tag_offset` |

Every field is described, private ones included, with the visibility it was declared with:
the table describes memory, and what code may *name* is still §4's business. A field whose
address C++ cannot take — an imported C bit-field (§15.2) — is left out.

**Field tags.** A field may end in a raw string, its *tag*: free-form text that means
nothing to the compiler and is carried into the table for the library that reads it.
`key:"value"` pairs separated by spaces are the convention, as in Odin and Go, and
`FieldInfo::tag_value` looks one up:

```dz
pub struct Player {
    pub name:   String              `json:"player_name"`
    pub hp:     i32 = 100           `json:"health" save:"-"`
    m_session:  u64
}
```

The tag comes after the type and the default. `[DECIDED — §18.59]`

### 13.2 What gets included

Controlled by `type_info` in `dazzle.toml`:

- `"none"` — no table at all. Smallest possible binary; a reflection call is an error
  (`DZ0555`).
- `"used"` *(default)* — a type is in the table if the program calls `typeinfo_of<T>()` or
  `typeid_of<T>()` on it, or reaches it from such a type: through its fields, variants and
  element types. This is the pay-for-what-you-use setting. There is no attribute to opt a
  type in: code that must find types it does not name — an editor listing every component —
  keeps their ids itself, `let components = [typeid_of<Transform>(), typeid_of<Health>()]`,
  which is also what puts them in the table.
- `"all"` — every non-generic type the program declares, and every instance of a generic
  one it uses. What an editor build wants.

### 13.3 Reading and writing fields

Because a `FieldInfo` carries a byte offset and a `TypeId`, generic code can walk a value it
does not know the type of:

```dz
pub fn write_json(out: &mut String, value: ^u8, id: TypeId) {
    let info = typeinfo(id)
    switch info.kind {
        case Struct {
            out.push('{')
            for &f in info.fields {
                let key = f.tag_value("json") or f.name
                out.push_str(String::format("\"{}\":", key).as_str())
                write_json(out, unsafe { ((value as usize) + f.offset) as ^u8 }, f.type_id)
            }
            out.push('}')
        }
        case I32 { out.push_str(String::format("{}", unsafe { (value as ^i32)^ }).as_str()) }
        // ...
    }
}
```

That is the whole serializer, and it is the same code that drives an editor inspector, a
network replication layer, and a save system.

### 13.4 Compile-time reflection

The same field information is available *during compilation* through `T::fields` (§5.5),
which is what a `derive`-style macro iterates to generate specialised code with no runtime
table lookup at all:

```dz
macro derive_serialize -> declaration {
    pattern { $T:#type }
    expand {
        fn serialize(self: &$T, w: &mut Writer) {
            $T::fields[] {
                w.key($.name)
                self.$.name.serialize(w)
            }
        }
    }
}
```

The two mechanisms are complementary: compile-time reflection generates fast, specialised
code for types you know at build time; the runtime table handles types you only know by
`TypeId`. `T::fields` arrives with macros (§19 M10).

### 13.5 Mapping back to the C++ output

Field offsets in the table are computed by the C++ compiler, with `offsetof`, so the table is
correct under any padding, packing, or ABI the target uses. It is filled in once, the first
time it is read, rather than by a global constructor (§16.4). Type names are the
Dazzle-level names — `Transform`, `List<i32>`, with the module in `module_name` — never the
mangled C++ ones.

---

## 14. Memory and allocators

Dazzle has no global allocator baked into the language. Allocation is a library concern
expressed through the `Allocator` trait (§7.6), and **the allocator is an explicit
parameter**. `[DECIDED — §18.10]`

```dz
pub trait Allocator {
    fn alloc(&mut self, size: usize, align: usize, loc: SourceLocation = #location) -> ^mut u8
    fn realloc(&mut self, p: ^mut u8, old_size: usize, new_size: usize, align: usize,
               loc: SourceLocation = #location) -> ^mut u8
    fn free(&mut self, p: ^mut u8, size: usize, align: usize, loc: SourceLocation = #location)
}

pub struct List<T> {
    m_allocator: ^mut Allocator = Allocators::Global

    List(capacity: u32, allocator: ^mut Allocator = Allocators::Global) { ... }
}
```

- **`&mut self`**: an arena changes on every allocation. `[DECIDED — §18.47]`
- **Sized free**: `free` is told the size and alignment the allocation was made with, so an
  allocator needs no header to remember them, and tracking can check them.
- **`loc`** is the caller's location (§12.7). A container forwards the one it was given, so a
  report names the line that built the container rather than the container's own code.
- `^mut Allocator` is a fat trait pointer (§7.6), which is what lets one `List<f32>` work
  with any allocator.

Underneath, two builtins reach the C heap and are what `Allocators::Global` is built on:
`alloc(size, align) -> ^mut u8`, which panics on exhaustion (§9.8), and `free(p)`, both
`unsafe`. User code allocates through an `Allocator`.

### 14.1 Why explicit rather than an implicit context

Odin's implicit `context.allocator` removes the parameter from every signature, and the
price is that you cannot tell from a call whether it allocates or from where. Given
principle 1 (§1), the parameter stays. The default-argument mechanism (§7.2) gives back
almost all the ergonomics: `List(8)` reads as cleanly as the implicit version, and
`List(8, &frame_arena)` is right there when you need it.

### 14.2 Standard allocators

```dz
Allocators::Global      // &mut Allocator — the C heap
Allocators::Temp        // &mut TempArena — this thread's scratch arena
```

Both are associated constants (§7.3): references to statics (§7.7), usable directly —
`Allocators::Temp.reset()` — and converting to the `^mut Allocator` a container stores.

- **`Global`** is `malloc` and `free`, with nothing of its own in front of them: the C heap is
  already a good general-purpose allocator, and the platform's tools understand it. An
  alignment stricter than the C heap guarantees is over-allocated, with the heap's own
  pointer kept just before the one handed out.
- **`Temp`** is a pointer bump inside a chunk taken from the heap, a new chunk when one
  fills, and **freeing does nothing**. `reset()` keeps the first chunk and returns the rest;
  every pointer it handed out is dangling afterwards. `realloc` of the last allocation grows
  in place. `used()` says how much of the current chunk a frame took.

```dz
fn update(&mut self, dt: f32) {
    defer Allocators::Temp.reset()

    let visible = self.cull(&camera, Allocators::Temp)   // freed at frame end, no bookkeeping
    for &e in visible { e.draw(&mut self.batch) }
}
```

**Memory tracking** is off unless a build turns it on — `dz run --track-memory`, or
`[build] track_memory = true` in the root manifest — and costs nothing when off: it is the
standard library's `TRACK_MEMORY` flag (§12.10), and the tracker does not exist without it. With it:

- every `Global` allocation is recorded with its size, alignment and `loc`;
- **a leak** — still live when the program ends — is reported with the line that allocated it;
- **a double free** panics, naming where it was allocated and where it was first freed;
- **a free the allocator never handed out**, including a `Temp` pointer freed through `Global`
  or the reverse, panics;
- **a free whose size or alignment differs** from the allocation panics;
- **`Temp.reset()` poisons** what it releases with `0xDD`, so a stale read shows.

**`List<T>`** is the growable array, in `std`. `List<i32>()` is empty and allocates
nothing; `List<T>(capacity, allocator)` takes both. It doubles when it grows; `push`,
`pop` (a `T?`), `insert`, `remove`, `swap_remove`, `clear`, `truncate`, `resize` (new
elements at their defaults), `reserve`, `len`, `capacity`, `is_empty`; `b[i]` reads and
writes, checked against the length; `as_slice()` and `as_mut_slice()` view the elements.
It is move-only unless `T` is copyable, and then a copy copies into a new allocation from the
same allocator. Every call that may allocate takes `loc = #location`, so tracking names the
line that pushed.

**`String`** is owned UTF-8 text, in `std`: `String("x")` or `String::from("x")` copy a
`str`, with an allocator as a second argument. `push_str`, `push` (a `char`, encoded),
`clear`, `reserve`, `len` (bytes), `capacity`, `is_empty`; `as_str()` borrows the text and
`as_cstr()` borrows it as a C string, since the bytes are always followed by a zero. `==`
compares text, and a copy copies it. `String::format("…", args)` builds one from a format
string, taking `allocator = …` by name (§12.9).

**`HashMap<K: Hash + Eq, V>`** maps keys to values, in `std`. `HashMap<K, V>()` is
empty and allocates nothing; `HashMap<K, V>(capacity, allocator)` takes both. `insert(k, v)`
adds or replaces, dropping a replaced value; `replace(k, v)` returns it as a `V?` instead.
A lookup takes the key by reference and follows §9.1 — `get(&k)` is a `^V` and `get_mut(&k)`
a `^mut V`, `null` when the key is absent, so `let hp = health.get(&id) or return`.
`remove(&k)` returns the value as a `V?`; `contains`, `len`, `is_empty`, `capacity`,
`clear`, `reserve`. A loop visits every entry: `for k, &v in m`, or `for k, &mut v in m`
to change the values (§10.4) — keys stay read-only. By slot works too: `for s in
0..m.slot_count()` with `m.key_at(s)`, `m.value_at(s)` or `m.value_at_mut(s)`, each `null`
for an empty slot. It is
move-only unless `K` and `V` are both copyable. Keys are found by linear probing, the table
doubles at three quarters full, and removal leaves no markers behind.

**`Hash`** is the `std` trait a key needs: `fn hash(&self) -> u64`, equal values hashing
equally. Integers, `bool`, `char` and `str` implement it, and `Eq`, without an `impl`, and
the methods can be called on them directly — `id.hash()`, `name.eq(&other)`, `a.cmp(&b)`.
Floats do not implement `Hash`, since `NaN != NaN` would break that promise. `String`
hashes as its text. A struct combines its fields' hashes with `hash_combine(seed, h)`.

A reference to a value that is not a place — `m.get(&7)`, `&make()` — refers to a
temporary holding the value until the end of the enclosing block, and the literal takes its
type from what the reference is wanted to point at.

Additional allocators — `Arena`, `Pool<T>`, `FreeList` — are ordinary Dazzle structs
implementing the trait, and come later.

### 14.3 Placement primitives

Container authors need to construct, destroy and move values in raw memory. Three
intrinsics:

```dz
construct(dst, args...)   // in-place construct dst from args (or copy-construct from a value)
destroy(dst)              // run dst's drop, leave the storage raw
move_out(dst)             // the value, moved out; the storage is left raw, as after destroy
```

They compile to C++ placement `new`, an explicit destructor call, and a move followed by
one. They are the only place in the language where a value can be observed in a
not-yet-constructed state, so all three are `unsafe` (§5.2), as are `memcpy` and `free`.
`move_out` is how `List::pop` returns an element, and how growing a list of move-only
elements carries them to the new memory. `[DECIDED — §18.16, §18.50]`

### 14.4 The standard library: `std`

The standard library is **one module, `std`**, carried by the compiler as Dazzle source:
the language's traits (`Drop`, `Copy`, `Eq`, `Ord`, the operators, `Iterate`, `Format`),
the allocators, the containers and the math below. Its names are flat — `std::String`,
`std::List`, `std::Vec2` — with one declaration to a file, as any project may organise
itself (§3.1). A module names it in its manifest like any dependency, and usually opens it
(§3.2):

```toml
[dependencies]
std = { open = true }       # String, Vec2, Allocators — and std::Vec2 where needed
```

Every declaration the language itself relies on lives here, and the compiler reaches those
without the program naming them: a `for` over a `List` or a `println` of a `Vec2` works in a
module that does not list `std`, which simply cannot write `List` or `Vec2` itself. Only what
a program uses is instantiated and emitted, so a flat, always-present library costs a
program nothing it does not call.

The name `std` is reserved: a project named `std`, or a dependency key `std` with a `path`,
is an error. `[DECIDED — §18.57, superseding §18.23 and §18.55]`

**The math** holds:

- **Constants** `PI`, `TAU`, `E` (`f32`) and `PI_F64`.
- **`Float`**, implemented by `f32` and `f64`, and **`Integer`**, implemented by every integer
  type, both without an `impl`. Their methods are called on the number: `x.sqrt()`,
  `angle.sin()`, `t.lerp(to, 0.5)`, `hp.clamp(0, max)`, `seed.wrapping_mul(31)`,
  `a.checked_add(b)` (a `T?`), `n.saturating_sub(1)`. Plain integer arithmetic makes no
  promise on overflow; these methods are how a program says what should happen. A method
  of the wrong kind of number is an error that says which kind has it.
- **`Number`**, implemented by every integer and float type, and required by both `Integer`
  and `Float`: the bound for code that works on either kind. `x.to_f64()`, and
  `lo.uniform(hi, bits)`, a value in `[lo, hi)` chosen by 64 random bits — what `Random`
  is built on, and what any other generator can use.
- **Vectors** `Vec2`, `Vec3`, `Vec4` of `f32`, and **`Vec2i`**, **`Vec3i`** of `i32`, with
  `+`, `-`, unary `-`, `==`, `* scalar` and `/ scalar` (component-wise `*` for the float
  ones), and methods: `dot`, `length`, `normalized`, `distance`, `lerp`, `min`, `max`,
  `cross` and `reflect` on `Vec3`, `perp`, `angle` and `rotated` on `Vec2`. The integer
  vectors are `Hash`, so a grid position keys a `HashMap`.
- **`Quat`**, a rotation, identity at its defaults: `Quat::from_axis_angle`,
  `Quat::from_euler`, `rotate` (also `q * v`), `q * q`, `normalized`, `conjugate`, `slerp`,
  `to_mat4`.
- **`Mat4`**, **column-major** and acting on column vectors: `m * v`, `projection * view *
  model`, `cols[3]` the translation — the OpenGL, GLSL and glm convention, right-handed.
  `Mat4::identity`, `translation`, `scaling`, `rotation`, `perspective` and `orthographic`
  (into OpenGL's clip space), `look_at`, `transform_point`, `transform_vector`, `transpose`,
  `determinant`, and `inverse` (a `Mat4?`, `none` when singular).

Methods rather than free functions throughout, since there is no overloading (§18.5) and
one name must work across `f32`, `f64` and every vector: `a.lerp(b, t)` is the same call on
each. A generic `Vec3<T>` waits on associated-type projection in generic code.
`[DECIDED — §18.55]`

**Time** holds:

- **`Duration`**, a span to the nanosecond that is never negative: `Duration::from_secs`,
  `from_millis`, `from_micros`, `from_nanos`, `from_secs_f32`/`f64`; `as_secs`, `as_millis`,
  `as_micros`, `as_nanos`, `as_secs_f32`/`f64`, `is_zero`; `+`, `-` (stopping at zero), `==`,
  the comparisons, and `Hash`.
- **`Instant`**, a point on a monotonic clock, which never goes backwards: `Instant::now()`,
  `elapsed`, `since`, `later - earlier` giving a `Duration`, `instant + duration`, `==` and
  the comparisons.
- **`Time`**: `Time::sleep(d)`, at least `d` and as much longer as the operating system
  chooses, and `Time::unix()`, the wall clock as a `Duration` since 1970-01-01 UTC.

```dz
let start = Instant::now()
update(world)
let dt = start.elapsed().as_secs_f32()
```

Like the math, these are associated functions and methods of types, not free functions.

**Reading text.** The `Text` trait, implemented for `str` and `String`, gives both the same
operations, literals included: `starts_with`, `ends_with`, `find` (a byte offset, `usize?`),
`contains`, `trim`, `trim_start`, `trim_end`, `split(separator)` and `lines()` (iterators of
`str` views), `replace(from, to)`, `to_ascii_lowercase`, `to_ascii_uppercase` (new
`String`s), and `parse_i64`, `parse_u64`, `parse_f64`, `parse_f32` — the whole text as a
number, or `none` for anything else, including surrounding spaces and a value that does not
fit. Pieces are views of the text they came from, never copies.

```dz
let text = File::read_text(&config) or return
for line in text.lines() {
    if line.trim().starts_with("#") { continue }
    for field in line.split("=") { … }
}
let volume = setting.trim().parse_f32() or 1.0
```

`[DECIDED — §18.71]`

**Sorting.** `xs.sort()` orders a `List` of anything `Ord` by `<` — through the `Sort`
trait — and `xs.sort_by(before)` by any strict order, a `fn(&T, &T) -> bool`; `xs.swap(i,
j)` exchanges two elements. The sort is a heapsort: no allocation, `n log n` at worst, and
not stable. `String` and `Path` are `Ord` by their bytes, which for UTF-8 is code-point
order, not a language's alphabetical one. `[DECIDED — §18.70]`

**`Random`** is one generator per thread, called through its type with no value to hold:
`Random::next_u32()`, `next_u64()`, `next_f32()` and `next_f64()` (the floats in `[0, 1)`),
`Random::range(lo, hi)` for any `Number` in `[lo, hi)` — `Random::range(1, 7)` is a die,
`Random::range<f32>(-1.0, 1.0)` a float — `Random::chance(p)`, and `Random::unit_vec2()` and
`unit_vec3()`, directions spread evenly. It seeds itself from the clocks on first use, so
runs differ; `Random::seed(n)` makes this thread's numbers repeat exactly. The generator is
SplitMix64: fast and well distributed, and not for cryptography. `[DECIDED — §18.67]`

**Text is UTF-8.** A `str` is UTF-8 bytes and a `String` owns them; `len` counts bytes, and
a character outside ASCII takes two to four. Characters are decoded as they are read:

- `s.chars()` — or `for c in s` on a `String`, or `Chars(text)` on any `str` — steps through
  the characters as `char`s. A byte that is not valid UTF-8, which only the unchecked
  `bytes as str` can produce, reads as U+FFFD.
- `char_count()`, `pop()` (the last character, a `char?`), `is_char_boundary(i)`,
  `slice(from, to)` (a `str?`: `none` when an end falls inside a character) and
  `truncate(len)` (a panic inside one), so text is never cut into invalid bytes.
- `String::from_utf8(bytes)` copies bytes that are valid UTF-8, `none` otherwise.
- A `char` formatted with `{}` is written as UTF-8, as a `str` is.

A character's position is in bytes throughout; there is no indexing by character, which
would take time in proportion to the text's length on every use. `[DECIDED — §18.69]`

**Files** are reached through three types, and no free functions:

- **`Path`**, a place in the file system, owning its text. `Path("assets") / "hero.png"`
  and `join` build one; a `\` becomes `/` and a trailing `/` is dropped, so the same text
  works on every platform, and an absolute part (`/…`, `C:/…`) replaces what it is joined
  to. `file_name`, `stem`, `extension`, `parent`, `with_extension`, `is_absolute`, `as_str`
  and `as_cstr` (for a C API) read it without touching the disk; `exists`, `is_file`,
  `is_dir` and `modified` (the last write, as a `Duration` since the Unix epoch) ask the
  disk. `==`, `<` and `Hash` compare the text.
- **`File`**, an open file: move-only, closed when dropped. `File::open(&path, mode)` with a
  `FileMode` of `.Read`, `.Write` (created or emptied), `.Append` or `.ReadWrite`; then
  `read(buf)` (0 at the end), `write(data)`, `seek(position)`, `position()`, `size()`,
  `flush()`. The whole-file functions are what most code wants: `File::read_bytes`,
  `File::read_text` (UTF-8, checked), `File::write_bytes`, `File::write_text`, and
  `File::remove`.
- **`Directory`**: `Directory::create` (with any missing parents; one that exists is fine),
  `Directory::remove` (empty), and `Directory::list`, the paths directly inside, sorted by
  their text so the order is the same on every platform.

```dz
let save = Path("saves") / "slot1.txt"
Directory::create(&save.parent()) or return
File::write_text(&save, state) or { e -> eprintln("could not save: {}", e.message) }
let text = File::read_text(&save) or { e ->
    if e.code != IoError::NotFound as u32 {
        eprintln("could not load: {}", e.message)
    }
    String()
}
```

Every call that touches the disk is fallible (§9), and its error's `code` is an `IoError`:
`NotFound`, `PermissionDenied`, `AlreadyExists`, `InvalidData` or `Other`. Paths are UTF-8
everywhere, including on Windows. `[DECIDED — §18.68]`

**The process**: `Process::args()`, the command line as a `List<String>` with the program
first, in UTF-8 on every platform; `Process::env(name)`, a variable as a `String?`; and
`Process::exit(code)`, which flushes output and ends the program at once, without dropping
what is still alive — returning from `main` is the normal way to end.

### 14.5 Handles: `Rc`, `Weak` and `Box`

Game code shares things — a texture drawn by many sprites, an entity several systems
track — and a raw pointer is the wrong tool for it (§5.2). The standard library has three
**handles**, each holding a value on the heap from an allocator, as `List` does:

| Handle | Owns | Copying it | The value is dropped |
|---|---|---|---|
| `Rc<T>` | a share | shares the value, counting one more owner | with the last `Rc` |
| `Weak<T>` | nothing | refers to the same value | never by a `Weak` |
| `Box<T>` | all of it | not allowed: move-only | with the `Box` |

```dz
let tex = Rc<Texture>(load("hero.png"))       // Rc(value, allocator = Allocators::Global)
let other = tex                                // tex.count() is now 2
let watch = Weak<Texture>(&tex)                // does not keep it alive
let node = Box<Node>(Node{ value: 1 })
```

**A handle may be `null`**, referring to nothing, exactly as a pointer may. `null` is every
handle's default and what a moved-from one is left as, so a field of a handle type is
written `texture: Rc<Texture> = null`. `h = null` releases this handle — for an `Rc`, one
share, and the value only if it was the last. A `Weak` is also `null` once no `Rc` owns
its value: dropping the last `Rc` makes every `Weak` to it read as `null` at once, which is
how an asset owner unloads something others still refer to.

**Its value is reached as a pointer's is**, and never with a bare `.`: `h == null` and
`h != null` test it, `h or …` gives a reference to the value (§9.4), and `if let` binds it
with `v`, `&v` or `&mut v`. `&mut` needs the handle itself in a writable place.

```dz
if let &mut t = sprite.texture { t.width = 64 } else { use_placeholder() }
let target = self.target or return             // a Weak: `return` if the entity is gone
```

**While the value is used, it cannot be freed.** `or` and `if let` on an `Rc` or a `Weak`
first take a strong `Rc` of their own, held until the `if let` block ends, or for `or` until
the enclosing block does. Whatever the code does meanwhile — sets the handle to `null`, or
removes the last other `Rc` from a map — the value outlives the reference:

```dz
if let &mut t = self.texture {
    self.texture = null        // releases this handle; the value is still held
    t.width += 1               // fine
}                              // dropped here, if that was the last owner
```

That costs one count up and one down per `or` or `if let`: plain integers, not atomics. A
`Box` has no second owner to take, so `self.node = null` inside `if let &n = self.node`
frees what `n` refers to, as it would through a `^T` (§1.1).

**`Nullable`** is the trait behind this, and any type may implement it: `is_null`,
`guard` — what `or` and `if let` hold for the duration, itself `Nullable` — and `ptr`, the
value's address or `null`. `Rc` and `Weak` guard with an `Rc`; `Box` with an `Unguarded`,
which holds nothing.

**A handle to a derived struct converts to a handle to its base**, as a reference does
(§7.3): an `Rc<Button>` is accepted where an `Rc<UiElement>` is wanted, sharing the same
count, and likewise `Weak` and `Box`. `[DECIDED — §18.64]`

Not yet: `Arc<T>`, which needs atomics (§18.11); and a handle to a trait object.
`[DECIDED — §18.62]`

---

## 15. C++ interoperability

This is a headline feature, not an afterthought. Because Dazzle *emits* C++, calling C++ is
a direct call in the generated source — no wrappers, no shims, no ABI marshalling, no
overhead. The importer's only job is to teach the type checker what the header contains.

### 15.1 Importing a header

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

### 15.2 What maps, and how

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

### 15.3 Overloads

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

### 15.4 Templates

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

### 15.5 Exporting Dazzle to C++

```dz
#[export]
pub fn dazzle_update(dt: f32) { ... }

#[export(c)]
pub fn dz_init() -> i32 { ... }     // extern "C", no mangling
```

`#[export]` emits the declaration into the module's generated header with a stable,
documented name so an existing C++ codebase can call into Dazzle — the path for adopting
Dazzle inside an engine you already have.

---

## 16. Code generation

### 16.1 Output layout

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

### 16.2 Readability of the output

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

### 16.3 Source mapping

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

### 16.4 The runtime shim

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
`print` and `format` sit in `dz_prelude.h` until they become library code (§12.9).

### 16.5 Compilation flags

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

### 16.6 Incremental builds

Dazzle writes a generated file only when its content hash changes. That way the C++
compiler's own incremental build does the right thing without Dazzle having to model object
files. A one-line change in one function rewrites one `.cpp`, and the C++ compiler rebuilds
one translation unit.

---

## 17. Compiler architecture and toolchain

### 17.1 Implementation

The compiler is written in **Rust** `[DECIDED]`, as a Cargo workspace. The existing Odin
lexer is the reference for §2 and is ported, not extended.

```
crates/
  dz_span        byte spans, source maps, file ids
  dz_diag        diagnostic model + rendering (codespan-style, with notes and fix-its)
  dz_lex         tokens (§2)
  dz_ast         untyped syntax tree, one node per source construct
  dz_parse       recursive-descent + Pratt expressions, error-recovering
  dz_macro       macro grammar, matching, hygienic expansion (§12)
  dz_resolve     modules, imports, visibility, name binding (§3, §4)
  dz_hir         resolved, desugared tree
  dz_check       types, traits, generics, definition-site checking (§5, §8)
  dz_flow        moves, drops, must-use, use-before-init (§6)
  dz_mono        monomorphization + compile-time folding (§8.4)
  dz_cpp         C++ emission, mangling, #line mapping (§16)
  dz_cppimport   libclang header importer (§15)
  dz_project     dazzle.toml parsing, dependency graph, build orchestration
  dz_driver      the pipeline as a library, shared by the CLI and the LSP (§17.2)
  dz_lsp         language server (§17.4)
  dz_fmt         formatter
  dz             the CLI
```

### 17.2 Pipeline

```
 source .dz
     │  dz_lex        tokens
     │  dz_parse      AST (recovers from errors; always produces a tree)
     │  dz_macro      AST with macros expanded
     │  dz_resolve    every name bound to a definition; visibility enforced
     │  dz_check      every expression typed; traits solved; generics checked once
     │  dz_flow       ownership, moves, drops inserted, must-use verified
     │  dz_mono       generics instantiated; comptime branches folded
     │  dz_cpp        C++ text + #line map
     ▼
 build/cpp/*.cpp ──► clang++ / MSVC ──► binary
```

Two properties are load-bearing:

- **The parser always produces a tree.** It never bails on the first error, because the LSP
  runs the same parser on half-typed code every keystroke.
- **Every phase is a pure function of its input**, so results can be cached per module and
  the LSP can re-run only what changed. Incremental caching starts as a simple per-module
  content-hash cache; if that stops being enough, `salsa` is the upgrade path.

### 17.3 CLI

```
dz new <name>          scaffold a project
dz build               generate C++ and compile
dz run                 build and execute
dz check               type-check only, no codegen (what the editor calls)
dz test                run #[test] functions
dz fmt                 format in place
dz expand <file>       print post-macro-expansion source (§12.6)
dz doc                 generate docs from /// comments
dz lsp                 language server on stdio
dz cpp                 emit C++ only, no compilation
```

### 17.4 Language server

Sharing every crate above — the server is not a reimplementation, it is the compiler with a
different front end. It is built on `lsp-server`, a synchronous loop, rather than
`tower-lsp`: the compiler's types are single-threaded (`Ty` holds `Rc`s), so an analysis
cannot live in the `Send + Sync` state an async server needs. Target feature set, in
priority order:

1. Diagnostics on save, then on keystroke.
2. Hover: type, doc comment, and for a fat trait pointer or `own` field, a note saying so.
3. Go to definition, find references, rename (rename is safe here because there is no
   overloading — one name, one symbol).
4. Completion: members after `.`, module members after `::`, struct-literal field names,
   `case` arms for a union, **and inside generic bodies**, which is the payoff for
   definition-site checking (§8.1). A bare name also offers the exports of dependencies
   that are not opened, and choosing one writes it with its key: typing `DrawLi` inserts
   `rl::DrawLine` (§3.2). In `dazzle.toml`, a header in a recipe's `import` and a
   dependency's `path` are links to what they name.
5. Completion inside macro invocations, using the macro's declared grammar (§12).
6. Code actions: fill missing `switch` arms, add a missing trait method, insert `move()` on
   a use-after-move error, add a missing `impl Drop`.
7. Inlay hints: inferred types on `let`, and — specific to this language — a marker where
   the compiler inserts an implicit `drop`.
8. Semantic tokens, document symbols, workspace symbols, formatting.

### 17.5 Testing strategy

The snapshot approach already in `tests/runner.odin` is the right one and carries over.

| Layer | Method |
|---|---|
| Lexer | `tests/lex/*.dz` + `.snap` token dumps (the existing corpus, ported) |
| Parser | `tests/parse/*.dz` + `.snap` AST dumps, including deliberately broken input |
| Diagnostics | `tests/errors/*.dz` with the expected rendered error as the snapshot — every error code in the spec gets one |
| Type checking | `tests/check/*.dz`, pass and fail cases |
| Ownership | `tests/flow/*.dz` — use-after-move, double-drop, partial move, must-use |
| Macros | `tests/expand/*.dz` + expanded-source snapshots |
| Codegen | `tests/cpp/*.dz` + generated-C++ snapshots |
| End to end | `tests/run/*.dz` compiled with a real C++ compiler, executed, stdout compared |
| Interop | a small vendored C and C++ header, imported and called |
| Fuzz | `cargo-fuzz` on lexer and parser: no panics, no hangs, ever |

`insta` provides `cargo insta review` for accepting snapshot changes, which replaces the
`.snap.new` file dance in the current runner.

Every `[DECIDED]` rule in this document should end up with at least one test named after
its section.

### 17.6 Hot reload — constraints to honour now

Hot reload is **not in v0.1**, but it is important enough that the design must not
accidentally foreclose it. Nothing here needs building yet; everything here is a rule that
later work must not break.

The target: `dz run --hot` builds each module as a shared library, watches the source, and
on change rebuilds and swaps that module without restarting the game or losing world state.
This is one command, not a mechanism you re-implement per project.

**What already points the right way.** Several decisions taken for other reasons happen to
be exactly what reload needs, and must stay that way:

| Decision | Why reload needs it |
|---|---|
| One module = one translation unit (§16.1) | gives a natural DLL boundary with no extra build config |
| No global constructors in generated code (§16.4) | a reloaded module has no hidden init order to re-run |
| Explicit allocators (§14) | memory allocated by the old DLL is freed by an allocator owned by the host, so a swap never orphans an allocation |
| The static type table (§13) | field names, types, and offsets are exactly what a state-migration pass needs |
| `#[export]` (§15.5) | already the declared, stable interface between host and module |

**What must be added later, and what it constrains now.**

1. **State migration.** On reload, live objects were laid out by the *old* build. The type
   table makes this tractable: walk old vs new `TypeInfo`, copy fields matched by name and
   type, default-initialise fields that are new, drop fields that disappeared. This requires
   the table to be complete for reloadable types, so `type_info = "all"` becomes implied by
   `--hot`.
2. **Vtable and function-pointer staleness.** Any dynamic-dispatch pointer held across a
   reload points into the unloaded DLL. **Constraint on §7.6:** a vtable must be
   addressable by a stable, deterministic id (a hash of the type and trait names) rather
   than only by its raw address, so it can be re-resolved after a swap. That is cheap to
   guarantee now and impossible to retrofit cheaply.
3. **Persistent state.** `#[persist]` on a module-level binding marks state that survives a
   reload; everything else is reconstructed. Not designed yet.
4. **Reload safety.** A swap may only happen at a point where no reloaded code is on the
   stack — in practice, the top of the frame loop.

`[PROPOSED]`

---

## 18. Decisions log

### 18.1 Resolved

| # | Question | Resolution |
|---|---|---|
| 18.1 | Default type of an unsuffixed float literal | **`f64`**, matching every other language. Context still wins, so `f32` code is unaffected (§2.5) |
| 18.2 | Attribute syntax | **`#[inline]`**. Attributes share the `#` sigil with macros and intrinsics because they are the same category — compile-time transforms. Disambiguated by the following character (§12.8) |
| 18.4 | `own` field with no `Drop` | **Error.** It is always a leak and the fix is mechanical (§6.5) |
| 18.5 | Function overloading | **Deferred, not rejected.** Default arguments (§7.2) cover most of it, and constructors already overload. Nothing in this spec precludes adding it in a later version; the resolution machinery has to exist anyway for imported C++ overload sets (§15.3). The one exception is a trait implemented at several arguments (18.27) |
| 18.7 | Keep the `!T` sigil? | **No — `!` moves to the signature.** `T?` is a postfix type suffix; fallibility is marked `!->` on the arrow and is never a type. `Maybe<T>` / `Result<T,E>` stay rejected: the value in a built-in is compiler-known layout and no instantiation in every signature, and that survives the notation change (§9) |
| 18.8 | Error propagation style | **One postfix `or` clause.** `with`/`else`, `try` and `catch` are all deleted; `or` handles `T!`, `T?` and `^T` identically, and bare `or return` is reserved for propagation (§9.4–9.6) |
| 18.9 | Macro invocation syntax | **Two spellings:** `#name …` in declaration position, `name!(…)` in expression position (§12.5) |
| 18.10 | Allocator passing | **Explicit parameter with a default argument.** It is the user's and the library's concern, not the language's (§14) |
| 18.11 | Concurrency | **Deferred to v2.** Threads, atomics, and a job system go in the stdlib once the core language is stable; nothing here forecloses them |
| 18.12 | Hot reload | **Not in v0.1, but a first-class goal.** Manual DLL plumbing per project is exactly what it should replace. Constraints that later work must not break are written down now in §17.6 |
| 18.13 | Is `machine` native syntax or a macro? | **A macro, written in user code** — it belongs in your engine, not the stdlib. It may graduate to the stdlib later if it proves general. It stays the benchmark the macro system must be good enough for (§12.1) |
| 18.14 | Generics: monomorphize or emit C++ templates? | **Monomorphize in Dazzle** (§8.6). Reopen only if compiler-side mono ever dominates build times |
| 18.3 | May a `&T` be stored in a struct field? | **No** — banned outright, with no escape hatch. References live only on the stack; context structs hold `^T` (§5.2) |
| 18.6 | Is `^Trait` one pointer or two? | **Two — a fat pointer.** `{ data, vtable }`, vtable in static storage, implementing types unchanged. `Vec3 : Drawable` is still 12 bytes and still blittable (§7.6) |
| 18.15 | May `?` wrap a pointer or reference? | **No.** `?` applies to value types only. `^T` is already the optional pointer; `(&T)?` would need lifetimes to be a library type and buys nothing as a builtin (§5.6) |
| 18.17 | `^T` → `&T` | **Checked.** Goes through an `or` clause, which yields a reference as writable as the pointer (18.37). The second half — "`&T` → `^T` stays implicit" — is **reversed by 18.16** (§5.2) |
| 18.21 | How `Iterate::next` spells one signature for both item kinds | **The question is void — there is no `next`.** `Iterate` is `valid` / `get` / `advance`, so termination is not encoded in the item and `Item` is never wrapped (§8.5). This **reverses the decided half of the old 18.21** ("a pointer iterator returns `^T`, a value iterator returns `Item?`"): that split existed only to dodge `(^T)?`, and with the item unwrapped it buys nothing. `Item` may now be `&T`, which `Item?` could never express, so §10.4's `for e in &entities` yields a `&Entity` as it always claimed |
| 18.22 | Is a slice writable through an immutable binding? | **Neither — writability moves into the type.** `[]T` is a read-only view and `[]mut T` a writable one, standing to each other as `&T` and `&mut T` do. Following the binding was rejected because a slice is a view, so the binding says nothing about the elements (§5.3) |
| 18.25 | Can a `&T` be read as a `T`? | **Not yet — and the operator traits stop needing it.** `&T` reaches through `.` for members and is otherwise not a value: `fn f(b: &f32) -> f32 { return b }` is an error, and `^` derefs a `^T` only (§5.2). That made §11.2's `fn mul(&self, rhs: &Rhs)` unimplementable for `Mul<f32>`, so `Rhs` is taken **by value** instead, which is the better signature for the copyable math types these traits exist for. `Eq`/`Ord` keep `&Self`, because comparing a move-only type must not consume it. Whether `&T` should gain a read of its own is left open — nothing needs it now. **Settled by 18.35**: it reads as a `T` where one is wanted |
| 18.24 | Array literals, and reaching an enum variant without naming its type | **`[a, b, c]`, with the length exactly as written**: no repeat form (§6.2 already zeroes an array, and `;` is a terminator so `[0; 8]` is unavailable), no short form, and a literal may not be *stored* as a slice though it may be passed as one (§5.3). **`.Selected` works wherever the enum is already known** — the same mechanism a union variant already used — while `Sprite::Selected` always works and is what an uninferrable position needs (§7.4). `Sprite.Selected` stays invalid: `.` is the runtime operator (§3.3) |
| 18.23 | Where do `Ord`, `Add`, `Iterate` and `Format` live before the stdlib? | **An implicit prelude written in Dazzle**, module `dz`, carried as source by the compiler and in scope everywhere with no import. Not compiler-invented symbols: the trait machinery is then exercised by real Dazzle code from the first day it exists, and at M12 the prelude grows into the stdlib instead of being deleted (§3.2). *Superseded by 18.57:* the prelude became the module `std`, named in the manifest like any dependency |
| 18.16 | Implicit `&T` → `^T` widening, and what a raw pointer may do | **Forming a pointer is safe; using one is `unsafe`.** `&mut T` → `^T` stays implicit; `&T` → `^T` is forbidden, since a `^T` may be written through and a `&T` promises it will not be. Reading or writing through a `^T` (`p^`, `.`, `[i]`), a pointer cast, `construct`/`destroy`/`memcpy`/`free`, and calling an `unsafe fn` are allowed only inside `unsafe { }` or an `unsafe fn`. The `or` conversion stays safe and yields a `&mut T`. `unsafe` is a marker, not a proof: it promises that raw-memory operations are findable, not that safe code is free of undefined behaviour. Array and slice indexing is therefore safe and bounds-checked in every build, with `xs.ptr` as the unchecked spelling (§5.2, §5.3). An explicit `as ^T` was rejected because it makes forming the pointer loud and leaves using it silent — the opposite of where the danger is. **Its first half is revised by 18.37**: a pointer now says whether it may be written through, so `&T` → `^T` is allowed — a `^T` is only read — and it is `&T` → `^mut T` that is refused |
| 18.26 | Copying a `T` inside a compile-time branch | **A `copy` only where `T` is `Copy`.** Written since r13 as a bounded implementation, `impl<T: Copy> Copy for Buffer<T>`: `Buffer<f32>` copies, `Buffer<File>` is move-only, and inside the `copy` `T` is `Copy` because the implementation says so (r12 had the same effect through an implicit rule). Every copyable type satisfies `Copy`, so `fn dup<T: Copy>` can say what it needs. Bounding the container itself (`Buffer<T: Copy>`) was rejected because it would shut out every element type that owns a resource (§8.2, §8.3) |
| 18.27 | One operator, two right-hand sides | **A type may implement one trait at several arguments**, one `impl` each, and the argument's type picks one — an exact match, then an untyped literal's default type, else an error asking for a type. The one overloading there is, and only for this; a repeated method name in a struct is still an error (§11.2) |
| 18.29 | An associated type per implementation | **Each implementation chooses its own.** The associated type belongs to the `impl`, not the type, so `impl Mul<Vec4> for Mat4` answers `Output = Vec4` and `impl Mul<f32> for Mat4` answers `Output = Mat4`. Falls out of 18.31; `Mul<Rhs, Output>` with a defaulted second parameter was the alternative, and would have needed a rule forbidding two outputs for one right-hand side (§8.5, §11.2) |
| 18.30 | `Error.location` is a `^T` | **It is a `&SourceLocation`**, the one reference the language stores in a struct: the location lives in static storage for the whole program, so the reason §18.3 bans stored references does not apply. `e.location.line` reads it directly, and `Error` stays 32 bytes. User structs still cannot store a reference (§9.2) |
| 18.31 | How is a trait implemented, and where? | **Only by `impl Trait for Type { … }`, written with the trait or with the type** — Rust's orphan rule. A module can then add behaviour to a type it did not write (an editor implementing `Inspect` for an engine's structs, or for `f32`), and two libraries can never both implement a third's trait for a fourth's type. A struct no longer lists traits after `:`, which is reserved for inheritance (18.32). `Drop` and `Copy` become prelude traits, stricter than others: the struct's own module, every instance, no bounds on `Drop`, never called by name — they are its C++ destructor and copy constructor. A type's own method wins over a trait's; two traits providing one name are chosen between as `Trait::method(&v)`. An implementation for every type at once (`impl<T> X for T`) is not allowed (§6.1, §7.6) |
| 18.28 | `break` with a value | **Removed.** A loop is a statement and has no result to give; the parser rejects a value after `break`, which keeps its optional label (§10.2) |
| 18.33 | `move` on a copyable value | **It moves.** The source is moved-from, a later use is an error, and it is reset to its defaults at runtime — the same as for any type. The compiler had been treating it as a copy, which §6.4 never said; one rule for every type is simpler to learn, and it keeps a generic body meaning one thing for every argument. To keep using a copyable value, leave out `move` (§6.4) |
| 18.34 | `mut fn` | **Removed — the receiver says it.** A method mutates when it takes `&mut self` and reads when it takes `&self`; `fn resize(&mut self, n: u32)` is the one spelling. `mut fn` stated the same fact twice, and the two could disagree: `fn f(&mut self)` was accepted, and the compiler went on to treat the method as mutating in one place and as read-only in another. The parser now rejects `mut fn` with a help saying what to write (§7.3) |
| 18.35 | Reading a `&T` as a `T` — the open half of 18.25 | **Implicit, for a copyable `T`.** A reference is accepted where its referent's type is wanted, and the value is copied: `for m in list.iter() { if m.name == name { return m } }` returns a `Movie`. Reading cannot fail, so by the rule that makes `^T` → `&T` loud and `&mut T` → `^T` quiet, it is not spelled. A move-only `T` is still refused, as a move out of a borrow; a generic body needs `T: Copy`. An explicit `r^` was considered and rejected — it would put a sigil on the most ordinary read in the language — and `^` stays the pointer dereference (§5.2) |
| 18.36 | Using a `T?` when both outcomes continue | **`if let name = value { … } else { … }`.** The binding holds what `or` would produce — the `T` of a `T?`, the `&mut T` of a `^T` — in the first block only. Chosen over narrowing `m` after `if m != none`, which needs flow-sensitive types and works only on locals, and over "`or` only", which leaves "both branches, then carry on" with no spelling. It does not take a fallible call, whose `else` would drop the `Error` unseen (§9.5); binding the error in `else` is left for later. `m == none` / `m != none` test presence; two optionals do not compare (§9.4). *How the binding holds the value is revised by 18.61:* `if let p = ptr` is now a copy, and the `&mut T` is `if let &mut p = ptr` |
| 18.37 | Read-only pointers, and what C's `const T*` becomes | **`^T` is read-only and `^mut T` writable**, standing to each other as `&T`/`&mut T` and `[]T`/`[]mut T` do: read-only unless `mut` is written. `const T*` maps to `^T` and `T*` to `^mut T` (§15.2). `&T` → `^T` and `&mut T` → `^mut T` are implicit, anything writable serves where the read-only form is wanted, and nothing gains writability; `or` gives a `&T` from a `^T` and a `&mut T` from a `^mut T`; `.ptr` is as writable as its view; the owning pointer is `own ^mut T`. Behind a `^mut` the pointee must match exactly, for the reason C forbids `T**` → `const T**`. A `^const T` was rejected as the Rust shape, which reads against the rest of the language. Forced by M7: with one writable pointer, every read-only C parameter needed an `&mut` at the call (§5.2, §5.3) |
| 18.38 | How C's unscoped enums and `#define`s come in | **An unscoped enumerator is also an untyped integer constant** at the header's scope, beside the enum itself: raylib's `IsKeyDown(int key)` takes `rl::KEY_A` as it is, and `.KEY_A` still names the typed variant wherever the enum is wanted. An `enum class` keeps its enumerators behind its name. **An object-like macro is a constant when its value can be typed**, emitted by its own name: at M7a a number (an integer untyped, a float `f32` with an `f` suffix and untyped without), with other expressions clang can type — `RAYWHITE`'s compound literal — to follow. Typed enumerators, which would need a cast at every raylib input call, were the alternative (§15.2) |
| 18.39 | How is a `cstr` made? | **A string literal is one wherever a `cstr` is wanted**, typed by its context as an untyped number is, and emitted as the C literal itself: it is zero-terminated static storage already, so nothing is allocated. `"x" as cstr` is the explicit spelling; `c as str` measures a C string. A runtime `str` has no terminating zero, so making a C string from one needs an owned copy, freed by its drop — which arrives with `String` (M12) and is spelled then. This replaces §5.3's `[PROPOSED]` "conversion to `cstr` is explicit and may allocate", which contradicted §15.1's own `InitWindow(…, "Dazzle")` (§5.3) |

| 18.40 | A C type whose definition its header does not show | **It imports as a handle**: a type used behind `^T` or `^mut T` and never by value — no local, field, parameter, return or `p^`, and not copyable, because nothing outside the library knows its size. That is how `RGFW_window`, `GLFWwindow`, `sqlite3` and `FILE` are handed out, and the functions taking one are then callable as they are in C. A type the header never mentions at all, declared in a system header it includes, is adopted the same way **when every use of it is behind a pointer**; where one is used by value the declaration that does so is left out instead, since its size really is unknown. Refusing such a header outright was the alternative, and would have shut out most C libraries (§15.2) |

| 18.41 | What a C `union` becomes | **A type whose members overlap**, read and written as C does it — not a Dazzle `union`, which is tagged (§7.5) and would promise a discriminant the C type does not have, with an exhaustive `switch` that means nothing. So `RGFW_event` comes in with all its members, `e.type` reads the one C would, and reading a member nobody wrote is C's own bargain rather than something Dazzle can check. A literal may set **one** member, since they are the same bytes (`DZ0705`); the type is never defined by Dazzle, so its layout stays the header's (§15.2) |

| 18.32 | Struct inheritance | **Single inheritance, written `struct Button : UiElement`.** The base comes first in memory, so `&Derived` → `&Base` and `^Derived` → `^Base` are implicit and free, while a by-value `Derived` where a `Base` is wanted is an error — C++'s slicing, which drops what the derived type adds. A base's fields and methods are the derived type's, unqualified inside its methods, and neither may be redeclared. A base with a constructor is constructed through it, `Button(…) : UiElement(r)`, and a type with a constructor is not built with a struct literal. Dispatch is static unless written: `virtual fn` opts one method in, `override fn` replaces one, both checked in both directions, and a body-less `virtual fn` makes the struct abstract — no value of it exists, only a `&` or a `^` to one. `Base::method(self)` calls the base's version without dispatch, and `drop` becomes virtual as soon as anything in the chain is. A fourth visibility, `protected`, opens a member to the types that inherit it (§4). Implementations are inherited and a more specific `impl` replaces the inherited one, statically. This reverses §7.3's old "there is no inheritance", which was never a decision of the language's author (§7.3, §4) |
| 18.42 | How a `for` binding holds its element | **On the binding, as C++ does it: `e` a copy, `&e` a reference, `&mut e` a writable reference.** `e` cannot be changed, so a write meant for the element cannot silently land on a copy. A copy is a real copy, never a reference in disguise, since a `copy` or `drop` with side effects would otherwise run or not depending on the compiler. Replaces `for e in &xs` / `for e in &mut xs`, where the sequence expression said it. Several names always take a tuple element apart, with no parentheses — `for id, &btn, &mut state in query` — and a pass count comes from `xs.indexed()`, so `for i, e in xs` no longer means an index: two readings of two names was the conflict this settles. What an iterator's `Item` is decides which bindings its loops may use (§10.4) |
| 18.43 | How a `case` binding holds the payload | **Exactly as a `for` binding (18.42).** `case Circle(c)` is a copy that cannot be changed, `&c` a read-only reference, `&mut c` a writable one that needs a `let mut`, `&mut` or `^mut` subject. One rule for both places; a binding that was always a reference would make `&mut c` the only mode worth writing. The binding had been writable always, which let a `switch` change a value declared immutable and failed in C++ behind a `&` (§10.3) |
| 18.44 | Assigning to a `&mut T` | **Writes the `T` it refers to**, as in C++. A reference cannot be pointed elsewhere, so there was no other meaning to give it, and `for &mut n in counts { n += 1 }` needs it. Through a `&T` it is an error (§5.2) |
| 18.45 | A union variant that is an existing struct | **A bare variant names a struct in scope and carries it; a variant with no payload is written `Point {}`.** `union ButtonState { DefaultState  TransitionState }` holds a `DefaultState` or a `TransitionState`, each built as that struct is built — its fields, defaults and visibility — and bound by `case DefaultState(d)` as that struct, methods included. A bare name that is no struct in scope, names something else, or names a generic struct is an error (DZ0551) rather than a variant with nothing in it: giving the bare spelling both meanings would let a misspelling, or a struct added or removed elsewhere, silently change what a union holds. `Point {}` is already how a variant with an empty field list was written, so nothing new is spelled; `Shape::Point`, with no braces, still names one (§7.5) |
| 18.46 | State that lives for the whole program | **`static`, declared at module scope, with a written type and a constant initialiser; `#[thread_local]` for one per thread.** Reading is safe, changing is `unsafe`, nothing moves out. Needed by the standard allocators — Temp's arena and the tracking tables are state no function owns — rather than by Global's speed: Global stays `malloc`. A static built by arbitrary code would depend on C++'s unordered initialisation across files, so the initialiser is constants only (§7.7, §14.2) |
| 18.50 | Slices, text and bytes, and moving out of raw memory | **`xs[a..b]`** (and `xs[a..]`, `xs[..b]`) slices an array or slice with checked ends, and a pointer inside `unsafe` with its end written — chosen over an unsafe `slice(ptr, len)` builtin, since the range form also serves checked subslicing. **`s.len` and `s.ptr`** on a `str`; `s as []u8` is safe; **`str::from_utf8(bytes) -> str?`** is the checked conversion, and `bytes as str` and `p as cstr` the unsafe, unchecked ones — the user asked for the safe way to exist, not only the fast one. **`IndexMut`** beside `Index`, chosen when the container is writable. **`move_out(dst)`** completes `construct`/`destroy`: a container cannot return an element by value without it (§5.3, §11.2, §14.3) |
| 18.49 | Evaluation order, and pointers as numbers | **Arguments left to right, receiver first**, as they are written — C++'s unspecified order is not inherited, so a program means one thing on every compiler. Since r44 the same for operands, indexing, slice and range bounds, and literal elements, with struct fields in written order; **assignment evaluates its value before its place**, as C++17 and Rust do. **`^T as usize` is allowed, `usize as ^T` only inside `unsafe`**: reading an address is harmless and allocators need it to align and to hash; making a pointer from a number is how memory gets corrupted (§7.2, §10.6) |
| 18.48 | Compile-time conditionals, and where a location comes from | **`#if FLAG { … } #else #if … { … } #else { … }` around declarations, members and statements, over registered flags only.** Built-ins `DEBUG`/`RELEASE`/`SHIPPING` and `OS_WINDOWS`/`OS_LINUX`/`OS_MACOS`; a package's own under `[flags]`; a dependency's read as `dep::FLAG` and turned on by the dependent's `flags = [...]` or `--flag`. An unknown flag is an error (DZ0309). The inactive branch is parsed but not checked. Chosen over checking both branches, which would forbid naming platform APIs absent on the host, and over free-form defines, which let a typo compile nothing. **An intrinsic default argument is the caller's**: `loc: SourceLocation = #location` records the call site, column 0 (§12.7, §12.10) |
| 18.47 | The standard allocators | **`Allocators::Global` and `Allocators::Temp`, no `Arena` for now; memory tracking is opt-in.** Global is `malloc`/`free`; Temp is a per-thread arena reset where the program chooses. The `Allocator` trait takes `&mut self` — an arena changes on every allocation — and its methods end in `loc: SourceLocation = #location`, which containers forward, so tracking reports name user code. Tracking is off in every build mode until `[build] track_memory = true` or `--track-memory` turns it on, and then reports leaks at exit, double and foreign frees, a free whose size or alignment differs from the allocation, and poisons Temp's memory on reset so a stale read shows. The allocator types are in the prelude; larger stdlib areas are imported modules (§14) |
| 18.51 | How `for` gets an iterator from a container | **Two prelude traits, `Iterable` and `IterableMut`**, each naming its iterator as an associated type: `fn iter(&self) -> Iter` and `fn iter_mut(&mut self) -> IterMut`. `for` over a value that is not itself `Iterate` calls `iter()`, or `iter_mut()` when a binding is `&mut`. Chosen over finding `iter()` by name, which states no contract and cannot be a bound: `fn total<C: Iterable>(c: &C)` must be writable (§8.5, §10.4) |
| 18.52 | A union variant that owns something | **§7.5's proposal adopted: a union owns its active payload.** It is dropped with the union, dropped before another variant is assigned, and moved by moving the active payload. A union is copyable only when every variant is. This is also what lets a variant carry a struct with a constructor, `copy`, `drop`, base or `virtual` method (§7.5) |
| 18.53 | `T` where a `T?` is wanted | **Converts, as at `return`.** A `T` becomes a `T?` wherever a `T?` is expected — a `let` with that type, an argument, an assignment — and where `if` or `switch` arms are joined and some arm is already a `T?` or `none`. Never the other way: a `T?` to a `T` still takes `or`. Chosen over `return`-only, which made state-machine `switch`es write every arm out as an optional (§9.1) |
| 18.56 | Variadic parameters | **`name: ..T`, last, seen as a read-only `[]T`; `f(..xs)` spreads a slice as the only variadic argument; C varargs stay unimported.** The caller builds a temporary array, so there is no allocation and nothing is instantiated per call. Arguments of different types go through a trait reference (`..&Format`), which is what a library `println` needs. Chosen over same-type-only, which leaves `println` a builtin forever, and over compile-time packs (C++ `Ts...`), which cannot be walked without macros (M10). `..` rather than `...`: it already means "a run of" and needs no new token. A spread is explicit, so a `[]T` argument is never silently one element or many (§7.2). **"Last" is revised by 18.72**: there is at most one, and the parameters after it are passed by name |
| 18.57 | Where a module's imports are declared | **In `dazzle.toml`, for the whole module; not in each file.** A dependency's key is its name in every file (`rl::DrawLine`); `open = true` also makes its exports bare; `std` is one flat module (`std::Vec2`, `std::String`) listed like any dependency, and not listed means not nameable; a C or C++ header is named once, in its recipe's `import`, and depended on as `rl = { cpp = "raylib" }`. A bare name is found in locals, then the module's own declarations, then a file's `{ … }` import, then `[names]`, then the opened dependencies — where **two different declarations are an error at the use** (DZ0310), with no precedence by manifest order, since reordering a manifest or upgrading a library must never silently change which declaration a name means. A file keeps `import m::{ … }` and `import m as x` as the exception. Chosen over per-file imports (Java, Rust `use`, the earlier Dazzle): in a game the same engine, math and container types are used in nearly every file, and a per-file alias let one file call a library `rl` and the next `raylib`. Precedent: C#'s project-level global usings, and Rust's extern prelude, which puts every Cargo dependency in scope under its manifest name. What made C++'s `using namespace` fail — names leaking transitively, and clashes settled silently — is ruled out by private dependencies (18.58) and the use-site error. The cost is that a bare name's origin is not written in the file; the editor shows it (§3.2, §14.4, §15.1) |
| 18.58 | Whether a dependency's names reach the modules that depend on this one | **Private by default; `pub = true` re-exports it, nested under the exporting module** (`rl::shapes::Rectangle`), and a dependent may name it itself with `via = "rl::shapes"`. A private dependency's type in a `pub` signature is an error (DZ0311). Nested rather than merged into the exporter's names, because a wrapper usually reuses the names of what it wraps. Follows Java's `requires` / `requires transitive` and Swift 6's `internal import` / `public import`, which also rejects a public API naming an internally-imported type; Rust merely warns, and leaves users holding values whose type they cannot write (§3.2) |
| 18.59 | What reflection describes, and how a field carries metadata | **Every kind of type, and an Odin-style tag per field.** `TypeInfo` gains `module_name`, `variants` (an enum's names and values; a union's, with each payload's fields), `elem` (a pointer's, slice's, array's or optional's element, an enum's backing type), `len`, `mutable` and `tag_offset`; a `str`, a slice and an optional expose their parts as `fields`. `FieldInfo` has `vis` and `owning` instead of a flags set, and `tag`: a raw string ending the field declaration, `name: String `json:"player_name"``, read with `tag_value(key)`. Chosen over struct-only tables, with which a serializer cannot write an enum by name or follow a slice, and over language-defined field attributes such as `#[skip]`, which a JSON or save library could not extend: a tag is text only the library reading it gives meaning to. `TypeId` is a struct around the `u32` rather than a bare `u32`, so an index is never mistaken for a number; the fields are `type_id`, `module_name` and `owning` because `type` begins a member (§8.5) and `module` and `own` are keywords. Names are Dazzle's — `List<i32>` — with the module apart, replacing §13.5's `engine.Transform`, which contradicted `::` (§13) |
| 18.60 | An attribute the compiler does not know | **An error, as is a known one out of place.** Unknown names are DZ0314, a known attribute on the wrong declaration, with arguments it does not take, or repeated is DZ0315, and one the specification describes but the compiler does not implement is DZ0316. Chosen over accepting and ignoring what is not understood, the C++ and Rust-lint tradition, because an attribute changes what a declaration means — a misspelled `#[test]` silently drops a test, and a `#[packed]` that is not implemented silently gives a different layout than the one written (§12.8) |
| 18.55 | The standard library's math | **A compiler-carried module `std::math`, loaded by `import std::math`; the name `std` reserved.** Chosen over the prelude, which would put `Vec3` and `PI` in every file, and over a plain `math`, which a project's own module could take. `f32` vectors, `Quat` and `Mat4`, plus `Vec2i`/`Vec3i` for grids — a generic `Vec3<T>` would wait on associated-type projection. **Methods everywhere**, scalars included through `Float` and `Integer` (`x.sqrt()`, `n.wrapping_add(m)`), since without overloading a free `lerp` could not serve `f32` and `Vec3` both. **Column-major matrices** on column vectors, as OpenGL, GLSL and glm, so a `Mat4` is `projection * view * model` and its floats are in the order a shader wants (§14.4). *Loading by `import std::math` superseded by 18.57:* the math is part of the one flat `std` module |
| 18.54 | Traits an enum has without an `impl` | **`Eq`, `Hash` and `Ord`, as the integers they are.** Equality and order compare values — declaration order when values are implicit — and a hash is the value's. So an enum is a `HashMap` key and satisfies `T: Ord` with nothing written (§7.4) |
| 18.61 | How an `if let` binding holds the value | **As a `for` or `case` binding does (18.42, 18.43): `v` a copy that cannot be changed, `&v` a read-only reference, `&mut v` a writable one** — for an optional, a pointer and a handle alike. `&mut` needs a writable pointer, reference or place (`DZ0507`); `v` needs a copyable value wherever the `if let` does not own it (`DZ0403`). This **reverses the pointer half of 18.36**, where `if let p = ptr` gave a `&mut T`: with handles added, one rule in four places was chosen over three rules and an exception, and `p` meaning a reference on a pointer but a copy on everything else would have been that exception. A `&T?` is read as its optional by `or` and `if let` both, as 18.35 reads a `&T` as a `T` (§9.4) |
| 18.20 | `or unreachable` in a release build | **The compiler assumes the success path** — `__builtin_unreachable`, not merely a skipped check. Debug still panics with the location. Chosen for speed: `or unreachable` is already the loud spelling of "this cannot fail", and §1.1 does not promise safety (§9.4) |
| 18.63 | Integer overflow | **Wraps, in every build; never panics.** Replaces §5.1's proposal of a panic in debug. What every mainstream language but Rust and Swift does, and what hashing code relies on; signed overflow included, which C++ leaves undefined and Dazzle does not. The `Integer` methods say anything else (`checked_add`, `saturating_sub`) (§5.1) |
| 18.64 | `Rc<Derived>` where an `Rc<Base>` is wanted | **Converts implicitly, as `&Derived` → `&Base` does (18.32)** and as C++'s `shared_ptr<Derived>` does: the same for `Weak` and `Box`. The count is shared, not copied. A handle to a trait object stays deferred, having no C++ counterpart to follow (§14.5) |
| 18.65 | How `std` reaches the operating system | **`extern fn name(…) -> T` with no body, allowed only in `std`**, whose body is C++ the compiler ships beside the runtime shim — not in it, so §16.4's shim still does no I/O. Not generic and not fallible: an extern function is a plain call, and `std` builds the `Error`. A program's own way to C and C++ stays a `cpp` dependency. First written as a `#[native]` attribute (r54); a keyword was chosen instead because `extern` was already reserved and is what C, C++ and Rust readers expect for "the body is elsewhere" (§7.2, §15) |
| 18.72 | Arguments passed by name | **`name = value`, for the parameters after a variadic one and for nothing else**, after the positional arguments, in any order; one with a default may be left out whatever else is given, one without must be passed. Chosen by the user for `String::format("…", x, allocator = Allocators::Temp)`: a variadic parameter takes every positional argument after the fixed ones, so an allocator or a `#location` after it was unreachable, and putting the allocator first would make every call name it. Naming is not extended to ordinary parameters, which keeps one way to pass each argument; it can be widened later without breaking a program. Revises 18.56's "last" |
| 18.73 | Library formatting | **No global `format`: `String::format(fmt, args: ..&Format, allocator = …)` builds a `String`**, chosen by the user over a global function and over a macro, which waits for M10. **`Format` is a real trait**: `std` implements it for the numbers, `bool`, `char`, `str`, `cstr` and `String`, and a type of one's own implements it to be formatted; a `Formatter` appends to a `String` and reports the placeholder's spec, which a type's own implementation interprets. **No attribute marks a format string**: an `#[format]` was built and removed by the user, who wants formatting functions of one's own to be **macros, as in Rust** — `String::format` is the one function whose format string the compiler checks, until M10. `println` and friends stay builtins, taking only the built-in kinds, until macros (M10); `error(…)` keeps its static storage (§9.2) |
| 18.71 | Text operations | **One `Text` trait, implemented for `str` and `String`**, whose operations are default methods over `text()`: both get every one, a literal included, with no `Deref` and no duplicate APIs. Pieces are `str` views; only `replace` and the case changes allocate. Parsing takes the whole text and returns an optional, refusing surrounding spaces rather than skipping them, since `trim()` says that explicitly. Case changes are ASCII only — Unicode case mapping needs tables `std` does not carry (§14.4) |
| 18.70 | Sorting | **`List::sort` (by `Ord`, through a `Sort` trait implemented for `List<T: Ord>`) and `List::sort_by(fn(&T, &T) -> bool)`; a heapsort.** A trait rather than a method, because a method cannot require more of `T` than its type does. Heapsort over quicksort or merge sort: no allocation and no quadratic input, which a frame budget cares about more than the constant factor; stability is given up, and a stable sort can come later as its own name. `String` and `Path` gain byte-order `Ord`, so `Directory::list` can return a sorted, platform-independent order (§14.4) |
| 18.69 | Characters in UTF-8 text | **Positions are bytes; characters are decoded by iteration.** `String` keeps UTF-8 as it always did, and gains `chars()` (and `for c in s`), `char_count`, `pop`, `is_char_boundary`, `slice` and `truncate`, every cut checked against a character boundary. Rejected: indexing by character, which is a linear walk disguised as `s[i]`, and UTF-32 storage, which quadruples memory for the text a game mostly holds. Invalid bytes decode as U+FFFD rather than panicking, since only unchecked code can make them (§14.4) |
| 18.68 | Files and paths | **`Path`, `File` and `Directory`, with no free functions**, chosen by the user: every operation belongs to the type it is about, and a `Path` type rather than bare `str` gives one place for joining, splitting and cross-platform separators. A `Path` always holds `/`; `\` is accepted and converted, since Windows accepts `/` and nothing else does `\`. `File` is move-only and closes on drop; whole-file `read_text`/`write_text` beside a streaming `open`. Failures are `!->` with an `IoError` code rather than typed errors (§9.2). Not yet: a `cstr` for a C API from a `Path` (`as_str` plus the owned-`cstr` row of the to-do list), file metadata such as modification times, and memory-mapped files (§14.4) |
| 18.67 | Random numbers | **One generator per thread, reached as `Random::…` with no instance**, chosen by the user over a value-type generator: game code wants a number, not a generator to pass around. SplitMix64 (one `u64` of state) rather than PCG or xoshiro, for its size and simplicity; seeded from the clocks unless `Random::seed` is called. `Random::range<T: Number>` covers every number kind through the new `Number` trait, which `Integer` and `Float` both require. Integer ranges are `[lo, hi)`, as floats are. A separate seedable stream — a replay that must not share state with gameplay — can come later as a value type (§14.4) |
| 18.66 | Generic methods | **A method may declare type parameters of its own, solved per call like a generic function's; written after `.name` as they are after a path, `xs.map<f32>(f)`, with no turbofish.** The `<…>` after a field name is read as type arguments only when `(` follows the `>`, so `n.len < max` is unaffected — the same non-associativity argument as §3.3. Not allowed on `virtual`/`override` methods or in a trait (`DZ0557`), where one function would have to serve every argument; a generic trait says the same thing and works through a `^Trait`. Each instance is emitted as a free function taking the receiver, not a C++ member template, so §8.6 still holds (§8.1) |
| 18.62 | Shared ownership, and handles that may be empty | **`Rc<T>`, `Weak<T>` and `Box<T>` in `std`, each of which may be `null`**, like C++'s `shared_ptr`/`weak_ptr`/`unique_ptr` rather than Rust's never-empty `Rc`: `null` is the default and the moved-from state, so no rule for "a type without a default" is needed, and a field of one is `= null`. **`null`, not `none`**: `null` is for handles — pointers included — and `none` for a value that is absent; the two stay apart so `x = null` reads as releasing something. A handle's value is reached only through `==`/`!=` `null`, `or` and `if let` — never a bare `.` — so an empty handle is always checked. **`or` and `if let` on an `Rc` or `Weak` hold a strong `Rc` of their own** until the `if let` block, or the block enclosing the `or`, ends, so the value cannot be freed while referred to — the cost being one plain count up and down. `if let` takes a `Weak` directly, with no `upgrade()`. Forced unloading is one owner plus `Weak`s, since a value others own cannot be deleted from under them; `Rc<T?>` is a shared slot, not a nullable `Rc`. The compiler knows the prelude trait `Nullable` (`Target`, `Guard`, `is_null`, `guard`, `ptr`) rather than the three names. A `Box` guards with nothing and can dangle as a `^T` can (§1.1). Deferred: `Arc`, `Rc<Derived>` as `Rc<Base>`, handles to trait objects (§14.5) |

### 18.2 Still open

| # | Question | State |
|---|---|---|
| 18.18 | `collect` | Reserved, unspecified. A fourth `or`-family clause that accumulates errors instead of acting on the first (§9.9) |
| 18.19 | `ignore` in two positions | It is a clause (`f() or ignore`) and a bare statement (`or { ignore; return }`). Probably fine, possibly one word doing two jobs. Decide against real code after M12 |

New questions will appear as implementation continues. They belong here, with the same
resolved/open split.

---

## 19. Roadmap

Each milestone ends with something you can actually run.

| # | Milestone | Ends when |
|---|---|---|
| M0 ✅ | Rust workspace, spans, diagnostics, lexer port | **done** — `dz_span` / `dz_diag` / `dz_lex` / `dz` build clean, 16 tests green, 10 snapshot corpora in `tests/lex/`, and `examples/example.dz` and `examples/state.dz` lex with zero errors |
| M1 ✅ | Parser + AST + `dz fmt` | **done** — full §20 grammar parses with error recovery; `dz parse` and `dz fmt` work; 40 tests green. Round-tripping is pinned as three properties over the corpus: formatting is idempotent, preserves the tree, and re-parses cleanly. Note: `examples/example.dz` and `examples/state.dz` parse only up to the point where they switch to **pre-spec sketches** (nested `struct` inside `union`, native `machine`, `def`, `@(...)`); lines 1–121 of `example.dz` parse with zero errors |
| M2 ✅ | Projects, modules, imports, visibility | **done** — `dazzle.toml` loading with span-accurate manifest diagnostics, source globs, dependency graph with cycle detection, module symbol tables, import resolution, `::` path resolution and visibility enforcement. `dz check` runs it. A three-module workspace resolves clean; violations are rejected with cross-file labels and "did you mean". Scope note: `.field` / `.method()` resolution needs the receiver's type and belongs to M3 — `::` is the compile-time operator, `.` the runtime one (§3.3), and the phase split falls exactly there |
| M3 ✅ | Type checker: primitives, structs, functions, enums | **done** — literals and their contexts, no implicit numeric conversions, casts, structs/fields/methods with `.`-side visibility, mutability, enums and tagged unions with exhaustive `switch`, `T?` and fallible returns with the `or` clause and the consumption rule, arrays, slices, ranges and tuples. Note: M3 shipped against the v0.4 sigils; the v0.5 syntax is a re-spelling of the same checks plus the `^T` → `&T` conversion rule. `dz check` runs it. Generic bodies are skipped rather than misreported, since generics are M6 |
| M4 ✅ | Codegen + runtime shim + `dz run` | **done — hello world runs.** C++17 emission with `#line` mapping back to `.dz`, the runtime shim, `dz cpp` / `dz build` / `dz run`, and end-to-end tests that compile and execute real programs. Output is warning-clean under `-Wall -Wextra -fno-exceptions -fno-rtti` on both clang++ and g++. Codegen covers functions, structs, enums, control flow, pointers, arrays and `defer`; `T?`, fallible returns, and tagged unions report `DZ0801` and are owed by M5 |
| M5 ✅ | Ownership: `own`, `copy`, `drop`, `move`, flow analysis | **done for everything that is not generic.** `copy` and `drop` become C++ special members, `move` steals and resets, move-only types have their copy operations deleted, and flow analysis reports use-after-move, partial moves, moves out of a borrow, implicit copies and missing returns. Codegen moved onto the HIR, which unblocked `T?`, fallible returns, tagged unions, tuples and block-valued expressions — the `DZ0801` rows M4 left behind. `tests/run/ownership` compiles and runs clean under ASan on clang++ and g++. `Buffer<T>` and `String` themselves are **generic**, so the literal criterion moves to M6; nothing about them is blocked on ownership any more |
| M6 ✅ | Generics + traits + monomorphization | **done.** `Buffer<T>` runs as a generic struct with an `own ^T` field, a `drop`, a folded compile-time branch on `T::is_trivially_copyable`, and `construct`/`destroy`, allocating through `alloc`/`free` — instantiated for a trivially-copyable and a resource-owning `T` in `tests/run/generics`. Generic bodies are checked once against their bounds; traits, conformance, associated types, default methods and bounds work; operators dispatch through the prelude's traits; `for` runs over a user `Iterate`; format strings are checked. `Buffer<T>` has a `copy` exactly when `T` is `Copy` (§18.26, settled in r12). **Static dispatch only:** `^Trait` fat pointers (§7.6, §18.6) and therefore a stored `^Allocator` are not in this milestone, so `Allocator` exists as a trait and a bound while `Arena` and `Allocators::Global` wait for the stdlib pass |
| M7a ✅ | C interop: a vendored C library, built from source | **done** — `tests/run/cinterop` calls a two-file C library (`mini.h` + `mini.c`) that `dz` compiles from source from a `[cpp.libs]` recipe, with no CMake and no prebuilt binary, on clang++ and g++. `import cpp` reads the header through libclang into ordinary Dazzle names; `^T`/`^mut T` (r18), recipes (r19) and the import itself (r20) got there |
| M7b ✅ | Full C and C++ interop | **done — a raylib window opens.** `tests/run/raylib` opens one with raylib 6.0 built from its own seven `.c` files. r21–r28 added what that needed and then the C++ half: handles, C unions, array members and bit-fields, callbacks, compound-literal macros, then namespaces, classes, methods, constructors, C++-owned destructors, overloaded methods, references and `[cpp.instantiate]` template instances, covered by `tests/run/cppinterop` on both compilers. What is left is narrower than the milestone and is in `PROGRESS.md`'s deferral register: overloaded *free* functions (methods do resolve), the boundary cast for an overloaded method, and inheritance between imported C++ classes — which waits on struct inheritance (18.32) |
| M8 ✅ | Unions, `switch`, `T?`, `!->`, the `or` clause | **done** — `tests/run/button` runs the `ButtonState` sample as the language specifies it: a union whose variants are the existing structs `DefaultState`, `TransitionState` and `ChangeSprite` (r38), each state's `update` returning the next as a `ButtonState?`, dispatched by a `switch` that calls the payload's own methods, around a fallible `!->` load handled by `or return`, a fallback and `{ e -> … }`. `switch` on constants, generic unions and statement-built values landed with r36, and a base initialiser whose arguments need statements with r38. A variant carrying a struct that owns a resource or has special members waits on variant drops, with the M12 stdlib |
| M9 | LSP: diagnostics, hover, go-to-definition, completion | usable in VS Code for real work |
| M10 | Macros | a user-written `#machine` expands to the `ButtonState` machine and the LSP completes inside it |
| M11 ✅ | Reflection + type table | **done** — `tests/run/reflect` is the criterion: a JSON serializer of about 50 lines, written once against `typeinfo(id)`, writes structs (by `json:"…"` tag), enums by name, a union's active variant, arrays, a slice and optionals, and skips private fields. The table describes every kind of type (18.59), is filled once by a function-local static rather than a global constructor, takes offsets from `offsetof`, and compiles warning-clean on clang++ and g++. Compile-time `T::fields` waits for macros (M10) |
| M12 | Stdlib pass: `List`, `String`, `HashMap`, allocators, math | you can write a small game without touching C++ |
| M13 | Hot reload (§17.6) | `dz run --hot` swaps a rebuilt module and migrates world state without restarting the game |
| M14 ✅ | `dz test` — tests as a language feature | **done** — `#[test] fn name() { … }` is written beside the code it tests; it passes by returning and fails by panicking. `dz test [path] [filter]` builds a separate test binary, runs **each test in its own process** — a panic aborts, so one process could report only the first failure — and prints a per-test result, the failing test's own panic message, and a count, exiting non-zero if anything failed. A `#[test]` that takes an argument, returns a value, is fallible, is generic or has no body is refused (`DZ0546`). **A normal build leaves every test out**, so a program never carries them |
| M15 ✅ | `dz bench`, and the C++ comparison | **done** — `#[bench] fn` is timed by `dz bench`, which calibrates the iteration count until a batch lasts 50ms, runs five batches and reports the **fastest** (noise only ever adds time). It builds `--release` by default, since timing `-O0` measures a build nobody ships. `tests/bench` is the comparison: each benchmark is a **pair**, one Dazzle and one calling hand-written C++ vendored beside it, compiled at the same optimisation level into the same binary and timed by the same clock. Covers bounds-checked indexing against unchecked, `drop` against a C++ destructor, and `virtual`/`override` against C++'s own. Still to add: generic instantiation, and tracking the numbers over time |
| M16 | Packages: dependencies that are fetched | `dependencies` accepts a git source pinned to an exact commit, resolved into a lockfile that is checked in, fetched into a local cache and built from source like any path dependency (§3.1) |

**Milestone order is not release order.** M14 and M16 are tooling rather than language, and
either may be pulled forward when it starts costing more to do without — as M7 and most of
M9 already were.

M4 is the one to aim at hard. Everything before it is invisible; the moment `dz run` prints
something, every later milestone becomes testable end to end.

---

## 20. Appendix: grammar sketch

Informal EBNF. `TERM` is a terminator (§2.2). This is a sketch for orientation, not the
normative grammar — the parser is the normative grammar, and this appendix is updated to
match it.

```ebnf
file          = { import | declaration } ;

import        = "import" ( module_path "as" ident
                         | module_path "::" "{" ident { "," ident } "}" ) TERM ;
module_path   = ident { "::" ident } ;

declaration   = { attribute } [ visibility ] ( fn_decl | struct_decl | union_decl
                                             | enum_decl | trait_decl | impl_decl
                                             | const_decl | static_decl | macro_decl
                                             | macro_invoke ) ;
static_decl   = "static" ident ":" type "=" expr TERM ;   (* §7.7 *)
cfg_if<T>     = "#" "if" cfg_cond "{" { T } "}"                  (* §12.10: T is a declaration, *)
                [ "#" "else" ( cfg_if<T> | "{" { T } "}" ) ] ;   (* a member or a statement    *)
cfg_cond      = cfg_and { "||" cfg_and } ;
cfg_and       = cfg_unary { "&&" cfg_unary } ;
cfg_unary     = "!" cfg_unary | "(" cfg_cond ")" | ident [ "::" ident ] ;
visibility    = "pub" [ "(" "module" ")" ] ;
attribute     = "#" "[" ident [ "(" token_tree ")" ] "]" ;

fn_decl       = [ "unsafe" ] [ "mut" ] "fn" ident [ generics ] "(" [ params ] ")"
                [ ( "->" | "!->" ) type ] block ;
params        = param { "," param } ;
param         = [ "&" | "&mut" ] "self" | ident ":" type [ "=" expr ] ;
generics      = "<" gparam { "," gparam } ">" ;
gparam        = ident [ ":" bound { "+" bound } ] ;

struct_decl   = "struct" ident [ generics ] [ ":" path ] "{" { member } "}" ;  (* one base *)
member        = field | fn_decl | ctor ;
field         = { attribute } [ visibility ] [ "own" ] ident ":" type [ "=" expr ] [ raw_string ] TERM ;  (* the tag, §13.1 *)
ctor          = ident "(" [ params ] ")" block ;
impl_decl     = "impl" [ generics ] path [ "<" type_list ">" ] "for" type "{" { impl_item } "}" ;
impl_item     = "type" ident "=" type TERM | fn_decl ;

union_decl    = "union" ident [ generics ] "{" { variant } "}" ;
variant       = ident [ "{" field_list "}" ] TERM ;   (* a bare ident carries that struct, §7.5 *)
enum_decl     = "enum" ident [ ":" type ] "{" { ident [ "=" expr ] TERM } "}" ;
trait_decl    = "trait" ident [ generics ] [ ":" trait_list ] "{" { trait_item } "}" ;
trait_list    = path [ "<" type_list ">" ] { "+" path [ "<" type_list ">" ] } ;
trait_item    = "type" ident TERM | fn_decl_sig | fn_decl ;

type          = access_type [ "?" ] ;
access_type   = "^" type | "&" [ "mut" ] type
              | "[" [ expr ] "]" type
              | "own" "^" type
              | "fn" "(" [ type_list ] ")" [ ( "->" | "!->" ) type ]
              | "(" type_list ")"
              | path [ "<" type_list ">" ] ;
path          = ident { "::" ident } ;
```

`?` binds to the innermost complete type to its left, so `^T?` parses as `^(T?)`. Inside a
function type, a trailing `?` belongs to the return type; `(fn(i32) -> bool)?` is how you
spell an optional function pointer.

```ebnf

statement     = let_stmt | assign | expr_stmt | if_stmt | while_stmt | for_stmt
              | switch_stmt | return_stmt | defer_stmt | block | "unsafe" block
              | "break" [ ident ] | "continue" [ ident ] ;   (* no value after `break` *)

let_stmt      = "let" [ "mut" ] pattern [ ":" type ] [ "=" expr ] TERM ;
assign        = place ( "=" | "+=" | "-=" | "*=" | "/=" | "%=" ) expr TERM ;
if_stmt       = "if" if_head block { "else" "if" if_head block } [ "else" block ] ;
if_head       = expr | "let" [ "mut" ] ident "=" expr ;
while_stmt    = [ ident ":" ] "while" expr block ;
for_stmt      = [ ident ":" ] "for" for_binding { "," for_binding } "in" expr block ;
for_binding   = [ "&" [ "mut" ] ] ( ident | "_" ) ;
switch_stmt   = "switch" expr "{" { case_arm } [ "default" block ] "}" ;
case_arm      = "case" case_label { "," case_label } block ;
case_label    = ident [ "(" [ "&" [ "mut" ] ] ident ")" ]  (* a variant, or a constant's name *)
              | expr ;                      (* a constant: `1`, `"idle"`, `lib::MAX` *)
or_expr       = expr "or" or_clause ;
or_clause     = "{" [ ident "->" ] { statement } "}"
              | "return" [ expr ]
              | "break" [ ident ] | "continue" [ ident ]
              | "ignore" | "unreachable"
              | expr ;
return_stmt   = "return" [ expr ] TERM ;
defer_stmt    = "defer" ( statement | block ) ;

expr          = assign_free_expr ;   (* precedence per §11.1; includes `or` *)
primary       = literal | "none" | path | struct_lit | call | index | field_access
              | "(" expr ")" | "move" "(" expr ")"
              | "error" "(" [ expr { "," expr } ] ")"
              | "&" expr | "&mut" expr | unary | intrinsic | macro_invoke
              | "." ident [ struct_lit_body ]
              | "unsafe" block
              | array_lit ;
array_lit     = "[" [ expr { "," expr } [ "," ] ] "]" ;
struct_lit    = path struct_lit_body ;
struct_lit_body = "{" [ field_init { "," field_init } [ "," ] ] "}" ;
field_init    = ident ":" expr | expr ;
intrinsic     = "#" ident [ "(" [ expr ] ")" ] ;

macro_decl    = "macro" ident "->" macro_kind "{" { syntax_rule } pattern_rule expand_rule "}" ;
macro_kind    = "declaration" | "expression" | "statement" | "type" ;
syntax_rule   = "syntax" ident "{" macro_pattern "}" ;
pattern_rule  = "pattern" "{" macro_pattern "}" ;
expand_rule   = "expand" "{" token_tree "}" ;
macro_pattern = { "$" ident ":" frag_kind [ rep ] | "(" macro_pattern ")" [ "?" ] | token } ;
frag_kind     = "#" ident | ident ;
rep           = "[" [ "," ] "]" ;
macro_invoke  = "#" ident token_tree | ident "!" "(" token_tree ")" ;
```

---

## 21. Changelog

Revisions of the unreleased 0.1. Every change to the **language** — syntax, semantics, or a
§18 decision — gets a row here, whether or not the compiler has caught up with it yet.
Compiler-only work belongs in `docs/PROGRESS.md`, not here.

| Revision | Date | Change |
|---|---|---|
| r62 | 2026-10-20 | **Arguments passed by name, and library formatting** (§7.2, §12.9, §14.4, 18.72, 18.73). The parameters after a variadic one are passed by name, `name = value`, after the other arguments and in any order; nothing else is. So "variadic last" (18.56) becomes "at most one variadic". **The global `format` is gone**: `String::format("…", args, allocator = …)` returns a `String` — no more four shared 256-byte buffers, so a long or a fifth message is no longer cut or overwritten. `Format` is implemented for the numbers, `bool`, `char`, `str`, `cstr` and `String`, and a type of your own can implement it; `Formatter` appends text and reports the placeholder's spec. A format string is checked at compile time in `String::format` as in `println`; a formatting function of one's own waits for macros (M10). `println` and friends are unchanged. |
| r61 | 2026-09-15 | **`Process`** (§14.4): `Process::args()`, `Process::env(name)`, `Process::exit(code)`. |
| r60 | 2026-09-15 | **Text operations** (§14.4, 18.71): the `Text` trait gives `str` and `String` `starts_with`, `ends_with`, `find`, `contains`, `trim`, `trim_start`, `trim_end`, `split`, `lines`, `replace`, `to_ascii_lowercase`, `to_ascii_uppercase`, `parse_i64`, `parse_u64`, `parse_f64`, `parse_f32`. |
| r59 | 2026-09-15 | **r54's decisions, implemented.** Integer overflow **wraps** in every build — `i32::MAX + 1` and `250u8 + 10` included, where the generated C++ had been undefined or had not wrapped at all (§5.1, 18.63). `or unreachable` **assumes success** in release and shipping builds (§9.4, 18.20). **`Rc<Derived>`, `Weak<Derived>` and `Box<Derived>` convert** to their base's, sharing one count and dropping the whole value (§14.5, 18.64). **A generic function may be a value**: `xs.sort_by(greater<i32>)`, or inferred from the function type wanted (§8.1). **Sorting** (§14.4, 18.70): `List::sort`, `sort_by`, `swap`; `String` and `Path` are `Ord`. `Directory::list` is sorted; `Path::modified` and `Path::as_cstr`. Output to a Windows console is shown as UTF-8. |
| r58 | 2026-09-15 | **A literal's suffix is its type** (§2.5): `let n = 5u8` is a `u8` — it had been an `i32` — and `300u8` or `5u8` where a `u16` is wanted are errors. **UTF-8 text by character** (§14.4, 18.69): `String::chars`, `for c in s`, `Chars(text)`, `char_count`, `pop`, `is_char_boundary`, `slice`, `truncate`, `String::from_utf8`; a `char` printed with `{}` is UTF-8 (it had come out as one Latin-1 byte, or nothing). |
| r57 | 2026-09-15 | **Files** (§14.4, 18.68): `Path` (`/`, `join`, `file_name`, `stem`, `extension`, `parent`, `with_extension`, `exists`, `is_file`, `is_dir`), `File` (move-only, closed on drop: `open` with a `FileMode`, `read`, `write`, `seek`, `position`, `size`, `flush`; `File::read_bytes`, `read_text`, `write_bytes`, `write_text`, `remove`) and `Directory` (`create`, `remove`, `list`), every failure an `Error` whose code is an `IoError`. **A method may be called on a number literal**: `5.hash()`, `2.25.sqrt()` — the literal takes the type `let` would give it. |
| r56 | 2026-09-15 | **`extern fn` replaces `#[native]`** (§7.2, 18.65): the standard library's body-less functions are written `extern fn name(…) -> T`; the attribute is gone, and `extern fn` outside `std` is DZ0317. **`Random`** (§14.4, 18.67): one generator per thread — `Random::seed`, `next_u32`/`next_u64`/`next_f32`/`next_f64`, `Random::range<T: Number>(lo, hi)` in `[lo, hi)`, `chance`, `unit_vec2`, `unit_vec3`. **`Number`**, a trait every integer and float implements (`to_f64`, `uniform`), now required by `Integer` and `Float`. |
| r55 | 2026-09-15 | **Generic methods** (§8.1, 18.66). A method may have type parameters of its own — `fn fold<A: Copy>(&self, start: A, …)`, and likewise a function without a receiver — inferred from the arguments or written out as `xs.fold<usize>(0, f)` and `List<Cell>::filled<u8>(…)`. A method's bounds are now checked, at the call and in its body; before, they were silently dropped. Not on a `virtual` or `override` method, nor in a trait (DZ0557). |
| r54 | 2026-09-15 | **`eprint` and `eprintln`** (§12.9): `print`/`println` to standard error, formatted the same way. **Time in `std`** (§14.4): `Duration`, `Instant` and `Time::sleep`/`Time::unix`. **`#[native]`** (§12.8, 18.65): a body-less function whose body is the compiler's C++, allowed only in `std`. A function with no body that is not a trait's or a `virtual` method is now an error (DZ0556) rather than a link failure. **Decided, not yet implemented:** integer overflow wraps in every build and never panics (§5.1, 18.63), which replaces the proposed debug panic; `or unreachable` in a release build lets the compiler assume success (§9.4, 18.20, no longer open); an `Rc<Derived>`, `Weak<Derived>` or `Box<Derived>` converts to one of its base (§14.5, 18.64). |
| r53 | 2026-09-14 | **Handles: `Rc<T>`, `Weak<T>`, `Box<T>`** (§14.5, 18.62). Shared ownership, a reference that notices its value is gone, and sole ownership of a heap value, in `std`. Each may be `null` — the default, and the moved-from state — and its value is reached only through `h == null`, `h or …` and `if let`, as a pointer's is; `Weak<T>(&rc)` makes a weak one, and `if let` takes it directly. `or` and `if let` on an `Rc` or `Weak` keep the value alive while it is used, even if the handle is set to `null` meanwhile. Any type can be a handle by implementing `Nullable`. **`if let` bindings follow `for` and `case`** (§9.4, 18.61): `if let v` copies, `if let &v` refers, `if let &mut v` refers and writes — so **`if let p = ptr` now copies the pointee**, and code that wrote through `p` must say `if let &mut p = ptr`. `or` and `if let` also read through a `&T?`. |
| r52 | 2026-09-14 | **Attributes are checked** (§12.8, 18.60). An attribute the language does not have is an error (DZ0314), and so is `#[test]`, `#[bench]` or `#[thread_local]` on a declaration it does not apply to, given arguments, or repeated (DZ0315). The attributes the specification describes but the compiler does not yet implement — `#[inline]`, `#[align]`, `#[packed]`, `#[deprecated]`, `#[export]`, `#[derive]`, `#[persist]`, `#[raw]` — are errors saying so (DZ0316) instead of being ignored. §12.8's `#[cfg(…)]` example is gone: conditional compilation is `#if` (§12.10). |
| r51 | 2026-09-14 | **Reflection** (§13, 18.59). `typeinfo_of<T>()`, `typeid_of<T>()` and `typeinfo(id)` return the standard library's `TypeInfo`, which describes every kind of type: fields with offsets for structs, tuples, `str`, slices and optionals; variants with values for enums and with payload fields for unions; the element type of pointers, slices, arrays and optionals. **Field tags**: a field may end in a raw string, `pub hp: i32 = 100 `json:"health"``, carried into `FieldInfo::tag` and read with `f.tag_value("json")`. A type is in the table when `typeinfo_of` or `typeid_of` names it, or it is reached from one (`type_info = "used"`) — no attribute opts a type in; `type_info = "none"` makes reflection an error (DZ0555). |
| r50 | 2026-09-14 | **Imports move into `dazzle.toml`** (§3.2, §14.4, §15.1, 18.57–18.58). A module's `[dependencies]` are what every one of its files can name, by their key — no `import` line per file. `open = true` makes a dependency's exports bare (`Vec2`, and `std::Vec2` still works); a bare name two opened dependencies both export is an error on the line that writes it (DZ0310), settled by qualifying it or by `[names] Vec2 = "std"`. **The prelude is now `std`**, one flat module holding the language traits, allocators, `String`, `List`, `HashMap` and the math: `std::Vec2`, not `std::math::Vec2`. It must be listed — `std = { open = true }` — or its names cannot be written. **`import cpp` is gone**: a recipe lists the headers Dazzle sees in `import = ["raylib.h"]`, and the module depends on it with `rl = { cpp = "raylib" }`; several headers are read together as one module. A dependency nothing names is a warning (DZ0313). **Dependencies are private**; `pub = true` re-exports one as `lib::dep`, a dependent can name it with `via`, and a private dependency's type in a `pub` signature is an error (DZ0311). In a file, `import m::{ A, B }` and `import m as x` remain, for one file's exceptions; `import m` alone and `import cpp` are errors. |
| r49 | 2026-09-14 | **Variadic parameters** (§7.2, 18.56): `fn sum(xs: ..i32)` takes any number of `i32`s and sees a read-only `[]i32`; `sum(..xs)` spreads a slice or array, as the only variadic argument. Must be last, with no default. `..&Trait` takes arguments of different types, each borrowed for the call. Works on methods, constructors and generic functions; as a value the function takes its slice. |
| r48 | 2026-09-13 | **`std::math`** (§14.4, 18.55): the first standard-library module beyond the prelude, carried by the compiler and loaded by `import std::math`; `std` and names under it are reserved for the standard library. `Float` and `Integer` methods on numbers (`x.sqrt()`, `n.wrapping_add(m)`, `checked_add`, `saturating_sub`), `Vec2`/`Vec3`/`Vec4`, `Vec2i`/`Vec3i`, `Quat` and a column-major `Mat4`. Also: **unary `-` on a struct is its `Neg`** (§11.2), **`==` accepts a temporary on its right** as well as a place, and **the most negative value of a signed type is a literal** — `let x: i8 = -128` was rejected because `128` was checked before the minus (§2.5). |
| r47 | 2026-09-13 | Four design decisions, 18.51–18.54. **`Iterable` and `IterableMut`** (§10.4): `for` over a container calls `iter()`, or `iter_mut()` for a `&mut` binding; `List` and `HashMap` implement both, and `C: Iterable` is a bound generic code can loop over. **A union owns its active payload** (§7.5): variants may hold resources, dropped with the union or when another variant is assigned; a union is copyable only when every variant is. **`T` converts to `T?` wherever a `T?` is expected** (§9.1) — `let`, arguments, assignment, and `if`/`switch` arms joined with a `T?` or `none` — not only at `return`. **Enums implement `Eq`, `Hash` and `Ord`** without an `impl` (§7.4). Decided now, implemented in the order: enums, `T?`, unions, iteration. |
| r46 | 2026-09-13 | **`assert` is an ordinary prelude function**, `assert(cond: bool, msg: str = "", loc: SourceLocation = #location)`, no longer a builtin; the message is optional. **`panic` takes a location as its last argument** (§9.8): `panic("missing {}", what, loc)` reports `loc` instead of its own line, which is how `assert` — and any helper taking `loc = #location` — names its caller's line. |
| r45 | 2026-09-13 | **`HashMap<K: Hash + Eq, V>` and `Hash`** in the prelude (§14.2). `insert` adds or replaces and drops the old value; `replace` returns it as a `V?`; `get`/`get_mut` return `^V`/`^mut V`, `null` when absent (§9.1); `remove` returns a `V?`; entries are visited by slot. **`Hash`** (`fn hash(&self) -> u64`) and **`Eq`** are built in for integers, `bool`, `char` and `str` — `str` gains `Eq` — and **prelude trait methods can be called on those types directly**: `n.hash()`, `s.eq(&t)`, `a.cmp(&b)`. Floats are not `Hash`. `hash_combine(seed, h)` combines hashes for a struct's `impl Hash`. **`&` of a value that is not a place** (`&7`, `&make()`) refers to a temporary living to the end of the block, and a literal there takes its type from the reference's context. |
| r44 | 2026-09-13 | **Evaluation order covers every operand, not only arguments** (§7.2, 18.49). Both sides of a binary operator, a container and its index, slice and range bounds, and tuple and array elements are evaluated left to right; a struct or union literal evaluates its fields in the order they are written, not the order declared. **Assignment evaluates the value first, then the place** — `xs[next()] = compute()` runs `compute()` before `next()` — for `=` and compound assignment alike. Why: `read_u8(r) << 8 \| read_u8(r)` decoded bytes backwards on some compilers, and a rule that stopped at arguments would have left that to C++. |
| r43 | 2026-09-13 | **The prelude's growable array is `List<T>`**, renamed from `Buffer<T>` (§14, §14.2). Same type, same methods; only the name changes. "Buffer" reads as raw bytes or a fixed block, which a growable list of elements is not, and `List<T>` is what a C# or Unity programmer already calls it. A program that declares its own `List` shadows the prelude's, as before. The examples that teach how to *write* a container (§0.1, §6, §8) keep a user-defined `Buffer<T>`. |
| r42 | 2026-09-13 | **`Buffer<T>` and `String`** in the prelude (§14.2), and what they needed. **Slicing** with a range: `xs[a..b]`, `xs[a..]`, `xs[..b]` — checked on arrays and slices, `unsafe` on a pointer (§5.3). **`str`** has `.len` and `.ptr`; `s as []u8` views its bytes; `str::from_utf8(bytes)` is a `str?` that checks them, and `bytes as str` and `p as cstr` are the unchecked, `unsafe` conversions. **`IndexMut`**, so `b[i] = v` writes through a user type (§11.2). **`move_out(dst)`**, the third placement primitive (§14.3). |
| r41 | 2026-09-13 | **Arguments are evaluated left to right** (§7.2): `f(a(), b)` runs `a()` before it reads `b`, whatever C++ does. **A pointer converts to `usize`** with `as`, and a `usize` to a pointer inside `unsafe` (§10.6) — what an allocator needs to align and to key a table by address. **Associated constants** (§7.3): `const NAME: T = value` in a type, named `Type::NAME`, a constant or a static's address, evaluated where named. **The standard allocators** (§14): the `Allocator` trait on `&mut self` with a trailing `loc: SourceLocation = #location`; `Allocators::Global` (`&mut Allocator`, the C heap) and `Allocators::Temp` (`&mut TempArena`, `reset()` and `used()`); tracking under `--track-memory`. A trait's default arguments apply to calls through a trait object and to its implementations' methods that do not repeat them. |
| r40 | 2026-09-13 | **Compile-time conditionals** (§12.10, 18.48). `#if FLAG { … } #else #if OTHER { … } #else { … }` around declarations, a type's members, or statements, over registered flags joined by `!`, `&&`, `\|\|`. Built in: `DEBUG`, `RELEASE`, `SHIPPING`, `OS_WINDOWS`, `OS_LINUX`, `OS_MACOS`. A package declares its own under `[flags]` in dazzle.toml, reads a dependency's as `engine::PROFILE`, and turns a dependency's on with `flags = [...]` on the dependency; `dz … --flag NAME` and `--track-memory` set them for one build. An unregistered flag is an error. The inactive branch is parsed but not checked. **`#location`** is a `SourceLocation`, and as a default argument it is the caller's location (§12.7). |
| r39 | 2026-09-13 | **`static`** (§7.7, 18.46), and the allocator decisions M12 builds on (§14, 18.47). `static` is a keyword: `static NAME: T = value` declares one variable for the whole program, `#[thread_local]` one per thread. Its type is written, its initialiser is made of constants (DZ0553), reading it is safe, and changing it — assigning, `&mut`, a `&mut self` method, a `&mut` binding — needs `unsafe`. Nothing is moved or copied out of one. **Associated functions** (§7.3): a `fn` in a type with no receiver is called through the type, `Quat::identity()`, and never on a value; the type's fields are not in scope in it. For allocators: the `Allocator` trait's methods take `&mut self` and end in `loc: SourceLocation = #location`; the standard ones are `Allocators::Global` and `Allocators::Temp`; memory tracking is opt-in. |
| r38 | 2026-09-13 | **A union variant can be an existing struct** (§7.5, 18.45). A bare variant name now always names a struct in scope and carries a value of it: `union ButtonState { DefaultState  TransitionState }` is built with `ButtonState::DefaultState{ … }` or `.DefaultState{}` exactly as a `DefaultState` is, and `case DefaultState(d)` binds a `DefaultState`, so `d.update()` calls its method. **A variant with no payload must now be written with braces, `Point {}`** — a bare `Point` with no struct of that name is DZ0551, as is one naming an enum, a union or a generic struct. `Shape::Point` without braces still builds it. Also: `return .Variant{}` from a `-> T?` function infers from `T`, as returning a plain `T` there already did (§9.1); and a base initialiser's arguments may not `return` (DZ0552) — the base is built before the constructor begins (§7.3). |
| r37 | 2026-09-13 | **How `for` and `case` bindings hold what they name** (§10.3, §10.4, 18.42–18.44). `for e in xs` copies each element and `e` cannot be changed; `for &e in xs` refers to it; `for &mut e in xs` refers to it and writes into `xs`, which must be writable. The forms `for e in &xs` and `for e in &mut xs` are gone. A move-only element is taken with `&e`, or consumed with `for e in move(xs)`. Several names take a tuple element apart — `for id, &btn, &mut state in query` — and the pass count moves to `for i, e in xs.indexed()`, so `for i, e in xs` no longer binds an index. `case` bindings follow the same rules: `case Circle(c)` is a copy, `&c` a read-only reference, and `&mut c` changes the payload, on a writable value only. Assigning to a `&mut T` writes what it refers to. A generic union's variant infers its type arguments from its payload: `Choice::One{ value: 4 }` is a `Choice<i32>`. |
| r36 | 2026-09-13 | **`switch` on an integer, `char`, `bool` or `str`** (§10.3). Labels are constants — literals, `const`s, and arithmetic on them — of the subject's type: `case 1, 2`, `case "quit"`, `case MAX + 1`. There is no exhaustiveness requirement, and a float cannot be a subject. A bare name in a label is a variant on an enum or union and a constant otherwise. Two related rules now stated: **the same variant or value in two labels is an error**, since the later one can never match; and **`break` and `continue` inside an arm act on the enclosing loop**, since `switch` has no fallthrough to break out of — the compiler had been leaving only the `switch`, which was a bug. |
| r35 | 2026-09-13 | **The built program is in `build/bin/`**, not `build/` (§16.1). `build/` now holds only directories `dz` names itself — `bin`, `cpp`, `obj`, and the roots `dz test` and `dz bench` create — because the two could collide: a module named `bench` produced the *file* `build/bench`, which is exactly the directory `dz bench` wants, and the build failed with "Not a directory" on every platform whose executables have no extension. `dz run` is unaffected; `dz build` prints the path it wrote. |
| r34 | 2026-09-12 | **`flags` in a library's recipe** (§15.1): raw flags passed to the compiler for *that library's* sources, after the ones `dz` chooses, so the last one wins. Needed because some libraries require something no `defines` or `include_dirs` can say — raylib's `rglfw.c` includes GLFW's Cocoa backend, so on macOS it must be compiled as Objective-C despite its `.c` name, and `flags = ["-x", "objective-c"]` in a `target.macos` section is how a recipe says that. It applies to a vendored library only, never to Dazzle's own generated code: invariant 7 fixes the flags that must compile warning-clean, and a manifest does not get to weaken them. |
| r33 | 2026-09-12 | **Which C++ compiler builds a project is configurable** (§16.5). `cxx = "g++"` in `[build]`, or in `[build.target.<os>]` for one platform, says what a project is known to build with; `--cxx <compiler>` on `run`, `build`, `cpp`, `test` and `bench` overrides it for one command, and `DZ_CXX` sits between them. With none of them `dz` still picks the first of `clang++`, `g++`, `c++` it finds. A compiler named explicitly is not probed first: if it is not there the build says so, rather than quietly using a different one. |
| r32 | 2026-09-12 | **`frameworks` in a library's recipe** (§15.1). A macOS system library is a framework, and `link_libs` becomes `-l<name>`, which does not find one — so `frameworks = ["Cocoa", "IOKit"]` in a `[cpp.libs.<name>.target.macos]` section is linked as `-framework <name>`, and ignored on every other platform. Without it no recipe for a windowing library could link on macOS, and the failure named undefined Objective-C symbols rather than anything about a framework. |
| r31 | 2026-09-12 | **Tests and benchmarks are language features (§19 M14, M15).** `#[test] fn name() { … }` is a test: it passes by returning and fails by panicking. `#[bench] fn name() { … }` is a benchmark: the runner times its body over as many iterations as it takes to measure. Both take no arguments, return nothing, are not fallible, are not generic and have a body — anything else is `DZ0546` — and both are written **beside the code they cover**. A normal build contains **neither**: `dz test` and `dz bench` each build a binary of their own, so a shipped program never carries them. **New builtin `blackbox(v)`**, which returns `v` as a value the optimiser knows nothing about. It exists because a benchmark has two ways to measure nothing — its result is never read, so the work computing it may be deleted; its input is usually a literal, so the answer may be computed at compile time — and passing the value through `blackbox` stops both. It is the only new name a program can collide with. |
| r30 | 2026-09-12 | **Struct inheritance (§7.3, §4, 18.32).** `struct Button : UiElement` inherits from one struct. The base's fields come first, so **`&Derived` → `&Base` and `^Derived` → `^Base` are implicit and free**; a by-value `Derived` where a `Base` is wanted is an error, because copying one would drop what the derived type adds. A base's fields and methods are the derived type's, in scope unqualified inside its methods, and neither may be redeclared. **A base with a constructor is constructed through it** — `Button(r: Rect, text: str) : UiElement(r) { … }` — and, as part of the same rule, **a type with a constructor is no longer built with a struct literal**: the constructor is what establishes its invariant. **Dispatch is static unless written:** `virtual fn` opts a method into runtime dispatch, `override fn` replaces one and must match its signature, both checked in both directions, and a **body-less `virtual fn` makes the struct abstract** — no value of it exists, only a `&` or a `^` to one. `Base::method(self)` calls the base's version without dispatch, so an override can build on what it replaces; `drop` becomes virtual as soon as anything in the chain has a `virtual` method. **`protected` is a fourth visibility** (§4), opening a member to the structs that inherit it and to nothing else — a different axis from the other three, which is why it is not a `pub(…)` form, and it is refused at module scope. **Implementations are inherited**: a `Button` is `Inspect` if `UiElement` is, and a more specific `impl Inspect for Button` replaces the inherited one, statically. `virtual`, `override` and `protected` are new keywords and are no longer usable as identifiers. |
| r29 | 2026-09-12 | **`header` in a library's recipe, for a single-header library** (§15.1). `header = "RGFW.h"` replaces `sources`: `dz` writes and compiles the one translation unit that includes it with the recipe's `defines` in force, so RGFW, stb and their kind need no hand-written one-line `.c` beside the header. Forgetting that file used to produce an unresolved symbol at link time, which named nothing that was actually missing. |
| r28 | 2026-09-12 | **C++ references import**: `const T&` is a `&T` and `T&` a `&mut T`, so a C++ API that takes a reference — which is most of them — is callable: `shapes::length_sq(&p)`, `shapes::translate(&mut p, 1, 1)`. An rvalue reference `T&&` is not imported: it is C++'s spelling of a move and §6.4 has its own (§15.2). |
| r27 | 2026-09-12 | **C++ imports: namespaces, classes, overloaded methods, and named template instances (§15.2, §15.3, §15.4).** A `namespace` is a scope reached through, one segment per level — `shp::shapes::geom::manhattan(a, b)` — with an `inline` namespace adding none, as in C++. A `class` is a struct: its **public** fields are fields and its public member functions are methods, `const` taking `&self` and any other `&mut self`; private and protected members stay in the type as padding and cannot be named. A constructor is a constructor call, `shapes::Counter(10)`. **A destructor stays C++'s:** Dazzle emits no `drop` for an imported type, and makes it move-only instead so it is never destroyed twice — unless the class also has a public copy constructor, which makes it an ordinary value again. Two member functions of one name are an overload set, resolved at the call site by argument type (§15.3); an overloaded *free* function still imports only its first declaration. A **template** is imported only where `[cpp.instantiate]` names an instantiation — `"shapes::Pair<int>" = "IntPair"` — which becomes an ordinary concrete type whose members have the template's parameters bound to those arguments (§15.4). A macro whose value is not a number but which clang can type now keeps that type, so raylib's `RAYWHITE` is a `Color` (§18.38). `dazzle.toml` accepts a quoted key, which `[cpp.instantiate]` needs. |
| r26 | 2026-09-12 | **C callbacks import.** A function-pointer parameter, field or return — `void (*)(const RGFW_event *)` — is a Dazzle `fn(^RGFW_event)` (§5.4), so a Dazzle function can be given to a C library as a callback: `rgfw::RGFW_setEventCallback(rgfw::RGFW_windowResized, on_event)`. A variadic one is still left out (§15.2). |
| r25 | 2026-09-12 | **A C struct's array members and bit-fields import.** `float m[16]` is a `[16]f32`: indexed, iterated with `for`, asked for `.len` and `.ptr`, and bounds-checked like any other array — but not taken as a whole value, which C cannot do either (`DZ0707`). `unsigned on : 1` is an ordinary member to read and assign, though `&` on one is refused (`DZ0706`): C++ has no reference to a few bits inside another member (§15.2). |
| r24 | 2026-09-12 | **A C `union` imports.** Its members come in as members of one piece of memory, exactly as C has them — so RGFW's `RGFW_event` can be held, filled in through `&mut`, and read with `e.type`. A literal sets one member (`DZ0705`); it is not a Dazzle `union`, which is tagged (§15.2, 18.41). |
| r23 | 2026-09-12 | **A C type the header only declares is a handle.** `import cpp` brings in `RGFW_window`, `sqlite3` or `FILE` as an opaque type, so the functions that take or return `^T` are callable exactly as in C. Holding one by value — a local, a field, a parameter, `p^` — is `DZ0704`: nothing outside the library knows its size (§15.2, 18.40). |
| r22 | 2026-09-12 | **A vendored library's `defines` are its own.** They build its sources and are no longer given to everything that includes its header, so a single-header library's `RGFW_IMPLEMENTATION` compiles the implementation once instead of in every translation unit. A module's `[cpp]` defines are the shared ones, and now reach its libraries' sources as well as its generated C++ — that is where a define both sides need belongs (§15.1). |
| r21 | 2026-09-11 | **A string literal is a `cstr` where one is wanted.** `show("aaa")` for a `cstr` parameter, `let c: cstr = "aaa"` and `"aaa" as cstr` all work, cost nothing, and reach C++ as the literal. `c as str` measures a C string. A runtime `str` still cannot become a `cstr`: that needs an owned copy, which arrives with `String` (§5.3, 18.39). |
| r20 | 2026-09-11 | **`import cpp` reads C headers.** `import cpp "mini.h" as mini` makes the header's functions, structs, enums, typedefs and numeric macros names under `mini::`, called and constructed like Dazzle's own and emitted as the C they name. A C struct's fields default to zero; an unscoped enumerator is also an untyped integer constant, and an integer macro is one too (18.38); `size_t` is `usize`; a call into C is safe code. An import without `as` is named after the header's file (§15.1, §15.2). |
| r19 | 2026-09-11 | **C and C++ libraries are vendored as source and built by `dz`.** A `[cpp.libs.<name>]` table in `dazzle.toml` is a library's recipe — `sources` (paths or globs), `include_dirs`, `defines`, `lib_dirs`, `link_libs`, and a `root` they are relative to — and `[cpp.target.<os>]` / `[cpp.libs.<name>.target.<os>]` add to it for one platform. Every manifest path is now relative to the manifest; `include_dirs` had been passed to the C++ compiler as written. A module's include dirs and defines apply to the modules that depend on it (§3.1, §15.1). |
| r18 | 2026-09-11 | **`^T` is read-only; `^mut T` is writable.** A pointer now says whether it may be written through, as a reference and a slice already do: `p^ = v` needs a `^mut T`, and an owning field that is written into is `own ^mut T`. In exchange `&T` → `^T` is allowed, a `^mut T` passes where a `^T` is wanted, `or` on a `^T` gives a `&T`, and `.ptr` works on a read-only view, giving a `^T`. `alloc` returns `^mut u8`. This is what C's `const T*` maps to (§15.2), which is why it changed. `^^T` now parses as a pointer to a pointer (§5.2, §5.3, §15.2, 18.16, 18.37). |
| r17 | 2026-09-11 | **`if let`.** `if let m = find(...) { … } else { … }` takes the value out of a `T?` (or a `^T`, giving a `&mut T`) for the first block, so "if it is there do this, otherwise that, then carry on" has a spelling — `or` must produce a value or diverge. Not for a fallible call, which still goes through `or`. Also written down: `x == none` and `x != none` test an optional, and nothing else compares one (§9.4, §10.1, 18.36). |
| r16 | 2026-09-11 | **A reference reads as a value.** Where a `T` is wanted, a `&T` or `&mut T` is accepted and the value copied — `return m` from inside `for m in list.iter()` returns the element itself. Only for a copyable `T`; a move-only one is still an error, since nothing moves out of a borrow. Where no type is wanted, a reference stays a reference (§5.2, 18.35). |
| r15 | 2026-09-11 | **`mut fn` is removed.** Whether a method mutates is said once, by its receiver: `fn push(&mut self, v: T)` mutates, `fn len(&self)` reads. `mut fn` repeated that and could contradict it; it is now a parse error (`DZ0214`) whose fix is to delete `mut`. Trait methods follow suit — `Drop` is `fn drop(&mut self)`, `Iterate::advance` is `fn advance(&mut self)` (§7.3, 18.34). |
| r14 | 2026-09-11 | **`move` always moves**, whatever the type. `move` on a copyable value — an `i32`, a `str` — leaves the source moved-from, so a later use is an error, and resets it to its defaults at runtime, exactly as for a resource. The compiler had been treating it as a copy, which §6.4 never said. To keep using a copyable value, leave out `move` (§6.4, 18.33). |
| r13 | 2026-09-11 | Implementations. **A trait is implemented only by `impl Trait for Type { … }`**, written in the trait's module or the type's (the orphan rule), so a module can add a trait to a type it did not write — an editor's `Inspect` for an engine's structs, or for `f32` — while two libraries can never both implement a third's trait for a fourth's type (§7.6, 18.31). `impl` is a keyword. An implementation holds exactly the trait's methods and chooses the trait's associated types, **each for itself**, so `Mat4 × Vec4` can be a `Vec4` while `Mat4 × f32` is a `Mat4` (§8.5, 18.29); a trait implemented at several arguments is one `impl` per argument (§11.2). Generic implementations (`impl<T: X> X for Buffer<T>`) are allowed; one for every type at once is not; two that could apply to the same type are an error; a type's own method wins over a trait's, and `Trait::method(&v)` chooses between two traits. **`Drop` and `Copy` are prelude traits** rather than special members: `impl Drop for File { mut fn drop(&mut self) … }`, `impl<T: Copy> Copy for Buffer<T> { mut fn copy(&mut self, other: &Self) … }` — only for a struct, in its own module, for every instance, never called by name; `copy` and `drop` stop being keywords (§6.1). **A struct no longer lists traits after `:`**; `struct Button : UiElement` is reserved for inheritance, whose design is recorded as 18.32 and lands with the next revision (§7.3). **`Error.location` is a `&SourceLocation`**, read as `e.location.line` — the one stored reference, since it points into static storage (§9.2, 18.30). |
| r12 | 2026-09-11 | Safety and generics pass, settling r11's open questions. **`unsafe`**: a new keyword, formerly reserved. Forming, copying, storing, comparing and passing a pointer are safe; reading or writing *through* a `^T` (`p^`, `.`, `[i]`), a pointer cast, `construct`/`destroy`/`memcpy`/`free`, and calling an `unsafe fn` are allowed only inside `unsafe { … }` or an `unsafe fn` (§5.2). It marks raw memory and is not a safety proof, and it relaxes no other rule. **`&T` → `^T` is forbidden**, reversing half of 18.17: a `^T` may be written through, so widening a read-only reference would launder it. `&mut T` → `^T` stays implicit, and **`p or …` now yields a `&mut T`**, so a context struct's pointer can be written through safely once checked (18.16). **Indexing an array or slice is bounds-checked in every build**, not just debug; `xs.ptr` is the unchecked way in, on a writable view only (§5.3). **`Copy`**: a prelude trait with no methods, satisfied by every copyable type without declaring it, so `fn dup<T: Copy>` may copy its `T`; a generic struct's own `copy` exists only for instances whose arguments are `Copy`, so `Buffer<f32>` copies and `Buffer<File>` is move-only (§8.2, 18.26). **A type may implement one trait at several arguments** — `Vec2 : Mul<f32> + Mul<Vec2>` with two `mul`s, picked by the argument's type; the only overloading there is (§11.2, 18.27). **`break` takes no value** (§10.2, 18.28). §8.1's `max` example now writes `move`, as r11's rule requires, and §11.2's example uses `+` between traits, which is the syntax. Opened 18.29 (an associated type per implementation) and 18.30 (`Error.location` is a `^T`). |
| r11 | 2026-09-11 | Generics pass, second half — what implementing M6 turned up. **The primitives implement the operator traits**: every number is `Eq`, `Ord` and the arithmetic traits with `Rhs = Self`, so `fn largest<T: Ord>` accepts an `i32` (§11.2); `Rem`, already in the prelude, joins §11.2's listing. **An unbounded `T` is move-only inside its body**, so returning or passing one by value is written `move(x)`, as for any move-only type (§8.2). **What a compile-time branch may test** is spelled out — type queries, `bool` constants, and `!`, `&&`, `||` over them — and both branches are checked for every `T` although only one is emitted (§8.3). **Format specs** are `{}`, `{:x}`/`{:X}` on integers and `{:.N}` on floats, with `{{`/`}}` for a literal brace; until the stdlib, numbers, `bool`, `char`, `str`, `cstr` and pointers are formattable and a struct is an error (§12.9). **Indexing a `^T` is unchecked pointer arithmetic**, as a container's `m_data[i]` needs (§5.3). **`alloc(size, align)` and `free(p)`** are builtins until the stdlib's allocators replace them (§14). Three questions opened: 18.26 (§8.3's own example copies a `T` that §8.2 says cannot be copied), 18.27 (two right-hand sides for one operator runs into §18.5), and 18.28 (`break` with a value is parsed but is not in the grammar). |
| r10 | 2026-09-10 | **The prelude exists.** `Ordering`, `Eq`, `Ord`, `Add`/`Sub`/`Mul`/`Div`/`Rem`, `Neg`, `Index`, `Iterate`, `Format`, `Formatter` and `Allocator` are ordinary Dazzle declarations in a module `dz`, in scope in every file with no import, and `dz::Ord` names the same trait explicitly (§3.2, §18.23). A local declaration of the same name shadows the unqualified spelling. **The arithmetic traits take their right-hand side by value** — `fn mul(&self, rhs: Rhs) -> Output` — because `Rhs` may be a scalar and a `&T` is not readable as a `T` (§5.2, §18.25); `Eq` and `Ord` keep `&Self`, since comparing a move-only type must not consume it. **A signed enum may have negative values**: `Less = -1` on an `i8`-backed enum was rejected, because unary minus is an operator rather than part of the literal (§7.4). |
| r9 | 2026-09-10 | **Array literals**: `[1, 2, 3]` is an array of exactly the length written, taking its element type from an annotation when there is one (§5.3). No repeat form — §6.2 already zeroes `let mut a: [1024]f32`, and `;` cannot separate one because §2.2 makes it a terminator — and no short form, since "the rest is zero" already has a spelling. A literal may be *passed* where a slice is wanted but not *stored* as one, because the array is a temporary and the slice would certainly dangle. **An enum variant may be written `.Selected`** wherever the enum is already known — annotation, argument, either side of a comparison, `return`, a struct-literal field, an `if` branch — with `Sprite::Selected` always available for a position with nothing to infer from (§7.4). This is the mechanism union variants already had; `Sprite.Selected` remains invalid, since `.` is the runtime operator (§3.3). |
| r8 | 2026-09-10 | Generics pass, settling what M6 needs before it can start. **`Iterate` loses `next`** and becomes `valid` / `get` / `advance` (§8.5). An `Option`-returning `next` puts the end-of-iteration signal inside the item's own value domain, so a `[]^Entity` would stop at its first null element and an `Item` of `T?` would stop at its first `none`; separating "is there one" from "what is it" removes the overlap and lets `Item` be **any** type, including `&T` and `&mut T`. This reverses the decided half of 18.21. **Slices carry their writability**: `[]T` is a read-only view and `[]mut T` a writable one, standing to each other as `&T` and `&mut T` do, so `fn fill(dst: []mut f32)` is expressible and a read-only view has a spelling (§5.3, §5.6). **A prelude exists**: `Error`, `SourceLocation`, the §11.2 operator traits, `Ordering`, `Iterate` and `Format` are ordinary Dazzle declarations in a module `dz` that is in scope everywhere with no import (§3.2). `for` gains a written-out desugaring (§10.4). §18 gains 18.21–18.23 resolved; the four open items are unchanged. |
| r7 | 2026-09-10 | Ownership pass, settling what §6 left implicit. **`own` describes the struct, not the pointer**: an `own ^T` value is one pointer and reading one copies a pointer, so `if handle != null` inside a `drop` is not a move; what is move-only is the struct that declares the field (§6.3). **Ownership is acquired by ordinary assignment** — a plain `^T` stores into an `own ^T` field, which is what `alloc` returning `^u8` has to mean (§6.3). **`move` needs a place the function owns**: `move(p^)`, `move(xs[i])` and a move through a `&` are errors, because a move resets its source and a borrow does not confer that right (§6.4). **Definite return** is now a rule: a function that promises a value must produce one on every path, and an exhaustive `switch` counts as returning if every arm does (§6.4). **`Error` and `SourceLocation` are language-provided types**, in scope everywhere with no import, since `or { e -> … }` binds one and there is nowhere for an import to come from (§9.2). |
| r6 | 2026-09-10 | `or` is **non-associative**: `a() or b() or c` is a syntax error rather than a fallback chain (§9.4, §11.1). The first clause has already produced a value or diverged, so a second `or` could only apply to something that can no longer fail. Same reasoning as non-associative comparison. A fallback that can itself fail gets a clause of its own — `a() or { b() or c }` — which is how "try a, else try b, else c" is written. |
| r5 | 2026-09-10 | Access/modality split. `?` becomes a postfix suffix on the type name (`T?`, `^T?`, `[]T?`) and applies to value types only — `?^T`, `?&T`, `!&T` and the rest are gone, along with the niche-optimisation table that depended on them. `!` leaves the type system entirely and becomes a signature marker (`!-> T`), so a fallible value cannot be stored, annotated, or pointed at. `with`/`else`, `try` and `catch` are replaced by one postfix `or` clause governed by a single rule: produce a `T` or diverge. Bare `or return` is reserved for propagation. `^T` → `&T` now requires an `or` clause, which is what makes `&T`'s non-null property mean anything. §18 reopens with four items. |
| r4 | 2026-09-10 | All open questions closed. 18.3: `&T` in a struct field is banned outright (no `ref struct`); context structs use `^T`, and §5.2 now says honestly that this is a heuristic rather than a proof. 18.6: `^Trait` is a fat pointer, with a stable deterministic vtable id added for hot-reload re-resolution. §18 has no open items. |
| r3 | 2026-09-10 | Decisions pass. Resolved 18.1 (`f64` default), 18.2 (attributes are `#[...]`, unified with macros under the `#` sigil), 18.4, 18.5 (overloading deferred, not rejected), 18.9, 18.10, 18.11, 18.12, 18.13 (`machine` is a user macro, not stdlib), 18.14. Added §17.6 hot-reload constraints and roadmap M13. §18 restructured into a decisions log; 18.3 and 18.6 remain open with concrete proposals (`ref struct`, and the fat-vs-thin trait pointer trade). |
| r2 | 2026-09-10 | Review pass. `::` is now the only scope separator (§3.2/3.3); turbofish removed, justified by non-associative comparison; `?T`/`!T` are **built-in type constructors** — `Maybe<T>`/`Result<T,E>` deleted, one `Error` type, `none`/`error()` literals, niche-optimised `?^T` (§9); error style resolved to **Option C** (`with` + `try` + `catch`); associated types explained properly with worked examples (§8.5); new §8.6 arguing monomorphization over C++ templates; added §0.1 one-page summary. |
| r1 | 2026-09-10 | First draft. Decisions taken: Rust compiler, project-as-module, three visibility levels, `&`/`&mut`/`^`/`own ^`, definition-site generics, declarative macros, static type-table reflection, C++17 output via libclang interop. §18 lists what is still open, with §18.8 (error propagation) as the one that most needs an answer. |
