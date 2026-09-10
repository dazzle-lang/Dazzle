# The Dazzle Language Specification

**Version:** 0.1 — **unreleased**, revision 6
**Status:** M0–M4 implemented — Dazzle programs compile and run, but the language is not
yet usable for real work. See `docs/PROGRESS.md`.

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
import engine::render as gfx          // :: is the only scope separator, everywhere

pub struct Buffer<T> {                // private by default; pub / pub(module) / (nothing)
    m_allocator: ^Allocator = null    // ^T  nullable pointer
    m_data:      own ^T               // own ^T  this struct frees it -> Buffer is move-only
    m_len:       u32                  // fields default-initialise; no uninitialised memory

    Buffer(cap: u32, a: ^Allocator = Allocators::Global) { ... }   // constructor
    copy(other: &Self) { ... }        // copy constructor   -> makes the type copyable
    drop()             { ... }        // destructor         -> deterministic, C++ handles it

    pub fn     length(&self)      -> u32 { return m_len }   // fn      = &self
    pub mut fn resize(&mut self, n: u32) { ... }            // mut fn  = &mut self
}
```

| Concept | Syntax | One-line rule |
|---|---|---|
| Refer to a value | `&T` `&mut T` `^T` `own ^T` | `&`/`&mut` are non-null and **stack-only**; `^` is a C pointer; `own ^` means you free it; `^T` → `&T` needs an `or` clause, `&T` → `^T` is free |
| Copy vs move | `let b = a` / `let b = move(a)` | if the type owns anything, copying needs an explicit `copy()`; otherwise `move()` |
| Absence | `T?` | postfix, binds to the type name. `^T?` is a pointer to an optional; an optional pointer is just `^T` |
| Failure | `fn f(…) !-> T` | `!` marks the *signature*, never a type. There is no `T!` to store, annotate, or point at |
| Handle anything fallible | `let v = f() or { e -> … }` | one clause for `T!`, `T?`, and `^T` alike |
| Propagate | `f() or return` | only where the enclosing signature can carry the failure |
| Fall back | `f() or Textures::Missing` | "on failure use this instead" — the common case in game code |
| Sum type | `union S { A{…} B }` + `switch` | tagged, exhaustive, no fallthrough |
| Interface | `trait X` + `struct Y : X` | bound `<T: X>` = static dispatch; `^X` = fat pointer, object stays the same size |
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
  double-free; it does not prevent dangling references. `[DECIDED]`
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
m_data = m_allocator.realloc(m_data as ^u8,
                             m_capacity * sizeof(T),
                             new_size * sizeof(T),
                             alignof(T)) as ^T
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
as       break     case     const    continue  copy      defer    drop
else     enum      extern   false    fn        for       if       ignore
import   in        let      macro    module    move      mut      none
null     or        own      pub      return    self      Self     struct
switch   trait     true     unreachable        union     while
```

Reserved for future use: `async`, `await`, `impl`, `where`, `yield`, `unsafe`, `comptime`,
`collect`.

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

[dependencies]
math   = { path = "../libs/math" }
raylib = { path = "../libs/raylib" }

[build]
cpp_standard = "c++17"
output       = "static-lib"   # "executable" | "static-lib" | "shared-lib"
optimize     = "debug"        # "debug" | "release" | "shipping"
type_info    = "used"         # "none" | "used" | "all"   (§13)

[cpp]
include_dirs = ["vendor/raylib/include"]
link_libs    = ["raylib", "opengl32", "gdi32", "winmm"]
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

```dz
import math                    // math::pow(x, 2)
import engine::render as gfx  // gfx::SpriteBatch
import math::{ pow, sqrt, PI } // pow(x, 2) — unqualified
```

Imports are file-scoped: an `import` at the top of `player.dz` is visible only in
`player.dz`. This keeps a file's dependencies readable at its head without making the file
an encapsulation boundary. `[DECIDED]`

**`::` is the only scope separator, everywhere.** A module path, an enum member, and an
associated function all use it, so `engine::render::SpriteBatch` reads uniformly from the
outside in and there is never a question of which separator applies at which level.

A module *name* may itself contain `::`, which is how a nested-looking hierarchy is built
out of flat projects:

```toml
# libs/engine_render/dazzle.toml
[module]
name = "engine::render"
```

`engine` and `engine::render` are two separate projects with two separate `dazzle.toml`
files and no implied dependency between them — the compiler treats the name as an opaque
path for scoping only. This is the C# assembly/namespace split: `engine::render` is a
namespace you can reach, not a module physically nested inside `engine`. `[DECIDED]`

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
exist. `[DECIDED]`

The one residual case is an argument list: `f(a<b, c>(d))` could be two comparisons or one
generic call. The parser prefers the **generic reading**, and the comparison meaning is
written `f((a < b), (c > (d)))`. `[PROPOSED]`

---

## 4. Visibility

Three levels. Default is private. `[DECIDED]`

| On | `private` (default) | `pub(module)` | `pub` |
|---|---|---|---|
| Top-level declaration | visible within its module | same as private | exported from the module |
| Struct field | visible only inside the struct's own methods | visible anywhere in the module | visible to importers |
| Method | visible only inside the struct's own methods | visible anywhere in the module | visible to importers |
| Enum / union variant | always as visible as its type | — | — |

Note the deliberate asymmetry: at top level, `private` and `pub(module)` mean the same
thing, because the module *is* the encapsulation boundary and there is no file-level
privacy. `pub(module)` earns its keep on **struct members**, where it expresses "the rest of
this engine may touch this field, but nobody who imports the engine may."

```dz
pub struct SpriteBatch {
    m_verts:                Buffer<Vertex>   // private: only SpriteBatch's own methods
    pub(module) m_gl_handle: u32             // the rest of the engine may poke this
    pub         count:       u32             // anyone may read

    pub mut fn draw(&mut self, s: &Sprite) { ... }
    mut fn flush(&mut self) { ... }          // private helper
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

Signed integer overflow **panics in debug builds and wraps in release**. Explicit
wrapping/saturating/checked operations live in the stdlib (`math::wrapping_add`,
`math::checked_add` returning `T?`). `[PROPOSED]`

There are no implicit numeric conversions, in either direction, ever — not even widening.
`let a: i64 = some_i32` is an error; write `some_i32 as i64`. Untyped *literals* are the
sole exception (§2.5). This is the single most common source of silent bugs in C++ game
code and it costs nothing to forbid. `[DECIDED]`

### 5.2 Pointers and references

Four ways to refer to something — the model your samples already use, plus `&mut`.

| Syntax | Meaning | Nullable | Mutable | Owns |
|---|---|---|---|---|
| `&T` | shared reference | no | no | no |
| `&mut T` | exclusive reference | no | yes | no |
| `^T` | raw pointer | yes | yes | no |
| `own ^T` | owning pointer | yes | yes | **yes** |

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
- **`^T`** is a C pointer. Nullable, freely storable, no guarantees. This is how you build
  a scene graph with parent back-pointers, and it is on you to keep it valid.
- **`own ^T`** is a `^T` that this struct is responsible for releasing. It changes the
  *type's* semantics — see §6.

**Converting between them.** `&T` → `^T` is free and implicit. `^T` → `&T` is not: a
pointer may be null and a reference may not, so the conversion goes through an `or` clause
like any other fallible thing (§9.4):

```dz
let btn = ctx.btn or return                     // propagate
let btn = ctx.btn or panic("button missing")    // assert, with a message
let btn = ctx.btn or unreachable                // checked in debug, free in release
draw(ctx.btn)                                   // error[DZ0522]: expected &Button, found ^Button
```

This is the only guarantee `&T` carries that `^T` does not, so it is worth one line at the
boundary. The check happens once, where the pointer enters scope, rather than on every
function entry downstream. `[DECIDED]`

The reverse direction stays implicit for now, which means the §18.3 field ban is
bypassable by widening a `&T` to a `^T` and storing that. The ban is a heuristic, not a
proof, and this is one of its holes. `[OPEN — 18.16]`

Dereference is postfix `^`, but you rarely write it because `.` auto-dereferences:

```dz
let p: ^Player = &mut world.players[0]
p.health -= 10        // auto-deref
let h = p^            // explicit deref, copies the Player
```

`null` is a value of type `^T` for any `T`. There is no null `&T`.

**Auto-referencing.** Method receivers are auto-referenced: `buf.resize(8)` works and
passes `&mut buf` when `resize` is a `mut fn`. Free functions are **not** auto-referenced —
you write `resize(&mut buf, 8)` — so that reading a call site tells you whether the callee
can modify the argument. `[PROPOSED]`

### 5.3 Arrays, slices, and strings

```dz
[8]f32          fixed-size array; a value type, copied on assignment
[]f32           slice: { ptr: ^f32, len: usize }; a view, never owns
[N]T            N may be any compile-time constant expression
```

Indexing is bounds-checked in debug and unchecked in release. An out-of-range `buf[i]`
panics with the file, line, index, and length. `[PROPOSED]`

```dz
str             a UTF-8 string slice: { ptr: ^u8, len: usize }. Never owns.
                Not null-terminated. Immutable.
String          stdlib: owning, growable, allocator-aware. Written in Dazzle, not builtin.
cstr            a null-terminated ^u8, for C interop only.
```

String literals live in static storage. `String::from("x")` copies; `s.as_str()` borrows.
Conversion to `cstr` is explicit and may allocate. `[PROPOSED]`

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
| **Access** | `^T` `&T` `&mut T` `own ^T` `[]T` | how do I reach this value? |
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

A struct may declare at most one of each:

```dz
struct Buffer<T> {
    Buffer(capacity: u32) { ... }   // constructor: name matches the struct, no return type
    copy(other: &Self)    { ... }   // copy constructor
    drop()                { ... }   // destructor
}
```

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

- it declares a `copy`, **or**
- it declares neither `copy` nor `drop`, has no `own` field, and every field is copyable.

Otherwise it is **move-only**, and assignment or passing by value requires an explicit
`move`:

```dz
struct File {
    handle: own ^u64
    drop() { close(handle) }
}

let f  = File("save.dat")
let f2 = f              // error: File is move-only because it owns a resource.
                        //   help: use `move(f)` to transfer ownership, or define
                        //         `copy(other: &Self)` to make File copyable.
let f3 = move(f)        // ok
```

`own` on a field is what flips the switch. It is not a comment — it is the declaration that
this struct is responsible for a resource, and it makes the whole struct move-only.

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

The same flow analysis handles **use-before-assignment** for the cases where default
initialisation does not apply (a `let` with no type and no initialiser is rejected outright;
a partially-moved struct field is tracked per field).

**What is *not* checked:** whether a `&T` outlives what it points at, and whether a `^T`
dangles. Dazzle has no lifetime system, by design (§1.1). `[DECIDED]`

### 6.5 Drop

`drop()` runs deterministically:

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
   = help: add a `drop()` that releases `handle`, or remove `own` if File does not
           own this pointer
```

### 6.6 How this lowers to C++

Everything above becomes ordinary C++ special members, which is why Dazzle does not have to
implement destruction ordering, temporary lifetime, or exception safety itself — C++ already
does, correctly, and every C++ debugger and profiler already understands it.

| Dazzle | Generated C++ |
|---|---|
| `Buffer(cap)` | `Buffer(uint32_t cap)` |
| `copy(other: &Self)` | `Buffer(const Buffer& other)` — body runs on a defaults-initialised `*this` |
| `drop()` | `~Buffer()` |
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

### 7.3 Structs

```dz
pub struct Transform {
    position: Vec3
    rotation: Quat = Quat::identity()
    scale:    Vec3 = Vec3{1, 1, 1}

    pub fn matrix(&self) -> Mat4 { ... }
    pub mut fn translate(&mut self, d: Vec3) { position += d }
}
```

Methods declared `fn` take `&self`; methods declared `mut fn` take `&mut self`. Inside a
method, fields are in scope unqualified (`position`), and `self.position` is also valid —
and required when a local shadows a field.

Struct literals:

```dz
let t = Transform{ position: Vec3{0, 1, 0} }  // unlisted fields take their defaults
let v = Vec3{1, 2, 3}                          // positional, allowed when all fields listed
let s: ButtonState = .Transition{}             // leading-dot inference from expected type
```

There is no inheritance. Composition and traits cover the ground. `[DECIDED]`

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

### 7.5 Unions

A `union` is a **tagged** union — a sum type, not C's overlapping storage.

```dz
pub union Shape {
    Circle { radius: f32 }
    Rect   { w: f32, h: f32 }
    Point                        // no payload
}

let s = Shape::Circle{ radius: 2 }

switch s {
    case Circle(c) { area = PI * c.radius * c.radius }
    case Rect(r)   { area = r.w * r.h }
    case Point     { area = 0 }
}
```

Variants may also name existing structs, which is what your `state.dz` sketch does:

```dz
union ButtonState {
    DefaultState
    TransitionState
    ChangeSprite
}
```

A union's size is its largest variant plus a tag, aligned. `Shape::Kind` is an
automatically generated enum of the variant names, so `s.kind()` gives you something cheap
to store or compare. Unions drop their active variant correctly, and assigning a new
variant drops the old one first. `[PROPOSED]`

Untagged/overlapping storage, should interop ever need it, is a separate `#[raw] union`.

### 7.6 Traits

```dz
pub trait Allocator {
    fn alloc(&self, size: usize, align: usize) -> ^u8
    fn realloc(&self, p: ^u8, old_size: usize, new_size: usize, align: usize) -> ^u8
    fn free(&self, p: ^u8, size: usize, align: usize)
}

pub trait Drawable {
    fn bounds(&self) -> Rect
    fn draw(&self, batch: &mut SpriteBatch)

    fn is_visible(&self, cam: &Camera) -> bool {   // default method
        return cam.frustum.contains(self.bounds())
    }
}
```

Implementation is declared on the struct, and checked at the point of declaration:

```dz
pub struct Arena : Allocator {
    m_base: own ^u8
    m_used: usize

    fn alloc(&self, size: usize, align: usize) -> ^u8 { ... }
    fn realloc(...) -> ^u8 { ... }
    fn free(...) { }
    drop() { ... }
}
```

**Static dispatch** happens whenever the trait is a generic bound (§8): the call is
monomorphized and direct.

**Dynamic dispatch** happens through `^Trait` or `&Trait`, which is a **fat pointer**:

```dz
struct TraitPtr { data: ^void, vtable: ^const VTable }
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

---

## 8. Generics

Generics are **checked at the definition site** and **monomorphized by Dazzle** into
concrete C++ types. They do not become C++ templates. `[DECIDED]`

### 8.1 Declaration and bounds

```dz
fn max<T: Ord>(a: T, b: T) -> T {
    if a > b { return a }
    return b
}

pub struct Buffer<T> { ... }

pub struct HashMap<K: Hash + Eq, V> { ... }
```

The body of `max` is checked once, against `T: Ord` alone. `a > b` is legal only because
`Ord` provides it. `a.frobnicate()` is an error reported at the line inside `max`, naming
the missing bound — not at the instantiation.

This is the whole reason for the choice: **the LSP can complete inside a generic body**,
because it knows exactly what `T` can do.

### 8.2 Unbounded parameters

`Buffer<T>` has no bound, so `T` supports only what every type supports: being moved,
default-initialised, dropped, and queried via `T::` (§5.5). That is precisely enough for
`Buffer` — your implementation only ever branches on `T::is_trivially_copyable` and calls
`construct` / `destroy`, all of which are universal.

### 8.3 Compile-time branches

```dz
if T::is_trivially_copyable {
    memcpy(m_data, other.m_data, other.m_len * sizeof(T))
} else {
    for i in 0..other.m_len { construct(m_data[i], other.m_data[i]) }
}
```

A condition made only of compile-time queries is folded during monomorphization: the dead
branch is not emitted and not present in the C++ output. `[PROPOSED]`

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
what `next` returns, but that answer is different for every container:

```dz
pub trait Iterate {
    type Item                          // "each implementer names this"
    mut fn next(&mut self) -> Item?    // ...and next() returns it
}
```

Two implementers, two different `Item`s:

```dz
struct SliceIter<T> : Iterate {
    type Item = ^T                     // stepping a []T yields pointers into it

    m_slice: []T
    m_index: usize

    mut fn next(&mut self) -> ^T {
        if m_index >= m_slice.len { return null }
        let e = &m_slice[m_index]
        m_index += 1
        return e
    }
}

struct RangeIter : Iterate {
    type Item = i32                    // stepping 0..n yields plain i32 values

    m_current: i32
    m_end:     i32

    mut fn next(&mut self) -> i32? {
        if m_current >= m_end { return none }
        let v = m_current
        m_current += 1
        return v
    }
}
```

Now `for` works on both, and the loop variable gets the right type — `&Entity` in the first
case, `i32` in the second — because `for e in xs` desugars to repeated `next()` calls and
the compiler reads `Item` off whichever implementer `xs` provides (§10.4):

```dz
for e in &entities { e.draw(&mut batch) }   // e is &Entity
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

> **A pointer `Item` is already optional.** `Item?` is the right spelling when `Item` is a
> value type, as it is for `RangeIter`. It cannot be written when `Item` is `^T`, because
> `(^T)?` does not exist (§5.6) — so a container that yields pointers into itself declares
> `Item = ^T` and `next` returns it bare. **A pointer iterator returns `^T`; a value iterator
> returns `Item?`.** `[DECIDED]`
>
> What is still open is only how the *trait* spells one signature covering both. The likely
> answer is that `Item?` collapses to `Item` when `Item` is a pointer type — `?` on something
> that already encodes absence is the identity rather than an error — which keeps one
> protocol and one `for` desugaring. That rule only has to exist once generics do, so it is
> settled with `Iterate` in M6. `[OPEN — 18.21]`

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
    for p in &mut m_players {
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
    pub location: ^SourceLocation    // where the error was created; static, never null
}
```

32 bytes, no allocation, no heap, no vtable. The location is a pointer into static storage
filled in by `#location` (§12.7), so an error tells you where it came from at zero runtime
cost — which is most of what a stack trace would have given you, for free.

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

`or` is a postfix binary operator whose left side is fallible — a `!->` call, a `T?`, or a
`^T` — and whose right side says what to do when it fails.

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
a `#location`; in release it is free, and the compiler may assume the success path.
Reaching it in release is undefined behaviour. It is the escape hatch for hot paths where a
null check is provably redundant, and it is spelled loudly on purpose. `[PROPOSED]`

**`or ignore`** is statement position only, because it neither produces a `T` nor diverges
and so has nothing to bind:

```dz
file.write(data) or ignore              // ok
let x = f() or ignore                   // error[DZ0524]: `ignore` produces no value
```

`ignore` is also valid as a bare statement inside a block clause, where it marks a
deliberate drop (§9.5). `[PROPOSED — this dual role is the part of the design least
supported by real code; revisit after the M12 stdlib pass.]`

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

    let mut data = Buffer<u8>::try_new(hdr.size, alloc) or return
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

Panics are for: out-of-bounds indexing, integer overflow in debug, failed assertions,
exhausted fixed-size pools, and violated invariants. Errors are for: I/O, parsing,
user-supplied data, and anything crossing a boundary you do not control.

**Allocation failure is a panic by default.** `alloc` returns `^u8` and panics on failure;
`fn try_alloc(n: usize) !-> ^u8` covers the rare code (console memory budgets, streaming)
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
let mut errs: Buffer<Error>
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
    for e in entities {
        if e.id == target { break outer }
    }
}
```

`[PROPOSED]`

### 10.3 `switch`

```dz
switch state {
    case Idle          { ... }
    case Walking(w)    { speed = w.speed }
    case Falling(f)    { f.time += dt }        // f is &mut into the union
    default            { ... }
}
```

- Exhaustive over unions and enums; a missing variant is an error listing which. `default`
  silences it, and the LSP offers a "fill in missing cases" action.
- No fallthrough. Ever.
- `case A, B { ... }` matches multiple variants when neither binds a payload.
- Payload bindings are references into the union, so matching does not copy.
- `switch` is also an expression when every arm yields the same type.

### 10.4 `for`

```dz
for i in 0..count      { ... }     // exclusive
for i in 0..=count     { ... }     // inclusive
for c in text          { ... }     // iterates chars of a str
for e in &entities     { ... }     // shared refs
for e in &mut entities { ... }     // mutable refs
for i, e in &entities  { ... }     // index + element
```

Iterating a container **by value** consumes it and is written `for e in move(entities)`.
Anything implementing `Iterate` (§8.5) works, so `for btn, state in w.query<Button, ButtonState>()`
from your `state.dz` sketch works by giving the query result an `Iterate` impl whose `Item`
is a tuple. `[PROPOSED]`

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
p as ^u8            // pointer reinterpretation
e as u16            // enum to its backing type
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
pub trait Add<Rhs = Self> { type Output; fn add(&self, rhs: &Rhs) -> Output }
pub trait Sub<Rhs = Self> { type Output; fn sub(&self, rhs: &Rhs) -> Output }
pub trait Mul<Rhs = Self> { type Output; fn mul(&self, rhs: &Rhs) -> Output }
pub trait Div<Rhs = Self> { type Output; fn div(&self, rhs: &Rhs) -> Output }
pub trait Neg              { type Output; fn neg(&self) -> Output }
pub trait Eq               { fn eq(&self, rhs: &Self) -> bool }
pub trait Ord : Eq         { fn cmp(&self, rhs: &Self) -> Ordering }
pub trait Index<I>         { type Output; fn index(&self, i: I) -> &Output }
```

A game language without `v1 + v2 * scalar` for vectors and matrices is not usable, so this
is not optional. `[DECIDED]`

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
                        mut fn update(&mut self, ctx: &mut $ctx) -> States? { $.body }
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
#location        SourceLocation   { file, line, column, function }
```

Used as a **default parameter value**, an intrinsic is evaluated at the *caller's* location,
which is exactly the `#src_code` behaviour you asked for (and the same trick as Odin's
`#caller_location` and C++20's `std::source_location`):

```dz
pub fn assert(cond: bool, msg: str, loc: SourceLocation = #location) {
    if !cond {
        panic("assertion failed at {}:{}: {}", loc.file, loc.line, msg)
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
#[inline]              fn lerp(...) { ... }
#[align(64)]           struct CacheLine { ... }
#[packed]              struct Header { ... }
#[reflect]             struct Transform { ... }
#[deprecated("use X")] fn old(...) { ... }
#[test]                fn test_buffer_grows() { ... }
#[cfg(os = "windows")] fn platform_init() { ... }
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

### 12.9 Format strings

Formatting is not string interpolation and not varargs — it is a compile-time-checked
call. `println`, `format`, `panic`, and friends take a **string literal** as their first
argument, and the compiler parses it during checking:

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

```dz
pub trait Format {
    fn format(&self, f: &mut Formatter)
}
```

The call lowers to a direct sequence of `Format::format` calls with no allocation and no
intermediate string, so `println` in a hot loop costs what hand-written output costs.
`[PROPOSED]`

Interpolation (`"hp is {hp}"`) is deliberately absent for now: it hides a name lookup
inside a literal, which complicates the lexer, the LSP's rename, and macro capture. It can
be added later as pure sugar over this. `[PROPOSED]`

---

## 13. Reflection

Reflection is **a compile-time-generated static table**, in the Odin spirit: your data
carries nothing, and the metadata lives in read-only memory beside it. `[DECIDED]`

### 13.1 The type table

Codegen emits one static array per program:

```cpp
static const dz_TypeInfo dz_type_table[] = { /* ... */ };
```

Each entry holds the type's name, size, alignment, kind (struct / union / enum / pointer /
slice / primitive), and for structs a slice of field records (name, type id, byte offset,
attributes). A `TypeId` is an index into that table — a `u32`, and **nothing in your structs
ever stores one unless you put it there yourself**.

```dz
pub struct TypeInfo {
    pub name:   str
    pub size:   usize
    pub align:  usize
    pub kind:   TypeKind
    pub fields: []FieldInfo    // structs only
}

pub struct FieldInfo {
    pub name:   str
    pub type:   TypeId
    pub offset: usize
    pub flags:  FieldFlags     // pub / pub(module) / private / own / attributes
}

pub fn typeinfo_of<T>() -> &TypeInfo    // compile-time resolved, returns a static ref
pub fn typeid_of<T>()   -> TypeId
pub fn typeinfo(id: TypeId) -> &TypeInfo
```

### 13.2 What gets included

Controlled by `type_info` in `dazzle.toml`:

- `"none"` — no table at all. Smallest possible binary; reflection APIs fail to compile.
- `"used"` *(default)* — a type is in the table if the program calls `typeinfo_of<T>()` on
  it, tags it `#[reflect]`, or reaches it transitively from such a type's fields. This is
  the pay-for-what-you-use setting.
- `"all"` — every type in the program. What an editor build wants.

### 13.3 Reading and writing fields

Because a `FieldInfo` carries a byte offset and a `TypeId`, generic code can walk a value it
does not know the type of:

```dz
pub fn write_json(w: &mut Writer, value: ^void, id: TypeId) {
    let info = typeinfo(id)

    switch info.kind {
        case Struct {
            w.begin_object()
            for f in info.fields {
                if f.flags.has(FieldFlags::Skip) { continue }
                w.key(f.name)
                write_json(w, (value as ^u8 + f.offset) as ^void, f.type)
            }
            w.end_object()
        }
        case I32 { w.number((value as ^i32)^ as f64) }
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
`TypeId`.

### 13.5 Mapping back to the C++ output

Field offsets in the table are emitted as `offsetof(dz_engine_Transform, position)` rather
than as literal numbers, so the C++ compiler computes them and the table is correct under
any padding, packing, or ABI the target uses. Type names are the Dazzle-level names, not the
mangled C++ ones, so an inspector shows `engine.Transform`, not
`dz_engine_Transform__f32`.

---

## 14. Memory and allocators

Dazzle has no global allocator baked into the language. Allocation is a library concern
expressed through the `Allocator` trait (§7.6), and **the allocator is an explicit
parameter** — as it already is in your `Buffer`. `[DECIDED — §18.10]`

```dz
pub struct Buffer<T> {
    m_allocator: ^Allocator = null

    Buffer(capacity: u32, allocator: ^Allocator = Allocators::Global) { ... }
}
```

### 14.1 Why explicit rather than an implicit context

Odin's implicit `context.allocator` removes the parameter from every signature, and the
price is that you cannot tell from a call whether it allocates or from where. Given
principle 1 (§1), the parameter stays. The default-argument mechanism (§7.2) gives back
almost all the ergonomics: `Buffer(8)` reads as cleanly as the implicit version, and
`Buffer(8, &frame_arena)` is right there when you need it.

### 14.2 Standard allocators

```dz
Allocators::Global      // malloc/free, or the platform heap
Allocators::Temp        // per-thread scratch arena, reset at a point you choose
Allocators::Null        // fails every allocation; for testing OOM paths
```

`Allocators::Temp` is the one that matters day to day:

```dz
fn update(&mut self, dt: f32) {
    defer Allocators::Temp.reset()

    let visible = self.cull(&camera, Allocators::Temp)   // freed at frame end, no bookkeeping
    for e in visible { e.draw(&mut self.batch) }
}
```

Additional allocators live in the stdlib and are ordinary Dazzle structs implementing the
trait: `Arena`, `Pool<T>`, `FreeList`, `Tracking` (wraps another and records call sites
using `#location`, §12.7, so leak reports point at real source lines).

### 14.3 Placement primitives

Container authors need to construct and destroy values in raw memory. Two intrinsics,
both of which your `Buffer` already uses:

```dz
construct(dst, args...)   // in-place construct dst from args (or copy-construct from a value)
destroy(dst)              // run dst's drop, leave the storage raw
```

They compile to C++ placement `new` and an explicit destructor call. They are the only
place in the language where a value can be observed in a not-yet-constructed state, and
using them outside a container implementation is a lint warning. `[PROPOSED]`

---

## 15. C++ interoperability

This is a headline feature, not an afterthought. Because Dazzle *emits* C++, calling C++ is
a direct call in the generated source — no wrappers, no shims, no ABI marshalling, no
overhead. The importer's only job is to teach the type checker what the header contains.

### 15.1 Importing a header

```toml
[cpp]
include_dirs = ["vendor/raylib/include", "vendor/jolt"]
link_libs    = ["raylib", "Jolt"]
defines      = ["JPH_PROFILE_ENABLED=0"]
```

```dz
import cpp "raylib.h" as rl
import cpp "Jolt/Physics/Body/Body.h" as jolt

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

Import results are cached, keyed on the header's content hash plus the flags, so the
libclang pass runs once and not on every build.

### 15.2 What maps, and how

| C / C++ | Dazzle |
|---|---|
| `int`, `unsigned`, `float`, … | `i32`, `u32`, `f32`, … (exact width resolved by libclang) |
| `T*` | `^T` |
| `const T*` | `^T` (immutability is not enforced across the boundary) |
| `T&` | `&mut T` |
| `const T&` | `&T` |
| `const char*` | `cstr` |
| `struct` / `class` | `struct`, fields mapped, private C++ members become opaque padding |
| member function | method |
| constructor / destructor | Dazzle constructor / `drop` |
| C++ copy constructor | `copy` |
| `namespace A::B` | nested scope: `ns::A::B::thing` |
| `enum` / `enum class` | `enum` |
| overload set | see §15.3 |
| `virtual` method | callable normally; Dazzle types cannot inherit from it |
| template | not imported unless explicitly instantiated (§15.4) |
| macro `#define X 1` | constant, when it is a simple literal |
| function-like macro | not imported |
| variadic `...` | not imported; use a fixed-arity overload |

An imported type with a non-trivial C++ destructor is **move-only in Dazzle** unless it also
has a copy constructor, which makes C++ RAII types obey Dazzle's rules automatically.

Imported types can be marked owning at the use site:

```dz
struct Texture {
    handle: own rl::Texture2D
    drop() { rl::UnloadTexture(handle) }
}
```

### 15.3 Overloads

Dazzle has no function overloading (§7.2), but C++ headers are full of it. Imported
overload sets are resolved by argument types at the call site, using exact matches only —
no implicit conversions, no promotion, no SFINAE. When two overloads both match exactly, or
none does, the error lists the candidates with their parameter types and asks you to
disambiguate:

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

```dz
import cpp "vector" as std
let v: std::StdVecF32
```

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
  obj/
  game.exe
```

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

### 16.5 Compilation flags

The generated code is required to build cleanly with:

```
-std=c++17 -fno-exceptions -fno-rtti -Wall -Wextra
```

Nothing Dazzle emits may need exceptions or RTTI, because console and mobile targets
routinely disable both.

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
  dz_lsp         tower-lsp server (§17.4)
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

Built on `tower-lsp`, sharing every crate above — the server is not a reimplementation, it
is the compiler with a different front end. Target feature set, in priority order:

1. Diagnostics on save, then on keystroke.
2. Hover: type, doc comment, and for a fat trait pointer or `own` field, a note saying so.
3. Go to definition, find references, rename (rename is safe here because there is no
   overloading — one name, one symbol).
4. Completion: members after `.`, module members after `::`, struct-literal field names,
   `case` arms for a union, **and inside generic bodies**, which is the payoff for
   definition-site checking (§8.1).
5. Completion inside macro invocations, using the macro's declared grammar (§12).
6. Code actions: fill missing `switch` arms, add a missing trait method, insert `move()` on
   a use-after-move error, add a missing `drop()`.
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
| 18.4 | `own` field with no `drop()` | **Error.** It is always a leak and the fix is mechanical (§6.5) |
| 18.5 | Function overloading | **Deferred, not rejected.** Default arguments (§7.2) cover most of it, and constructors already overload. Nothing in this spec precludes adding it in a later version; the resolution machinery has to exist anyway for imported C++ overload sets (§15.3) |
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
| 18.17 | `^T` → `&T` | **Checked.** Goes through an `or` clause. `&T` → `^T` stays implicit (§5.2) |

### 18.2 Still open

| # | Question | State |
|---|---|---|
| 18.16 | Implicit `&T` → `^T` widening | It launders a reference into a storable pointer and so punches a hole in the 18.3 field ban. Options: leave it (the ban is admittedly a heuristic), require an explicit `as ^T`, or add a `&raw` form. Costs nothing to defer; costs a migration to fix late (§5.2) |
| 18.18 | `collect` | Reserved, unspecified. A fourth `or`-family clause that accumulates errors instead of acting on the first (§9.9) |
| 18.19 | `ignore` in two positions | It is a clause (`f() or ignore`) and a bare statement (`or { ignore; return }`). Probably fine, possibly one word doing two jobs. Decide against real code after M12 |
| 18.21 | How `Iterate::next` spells one signature for both item kinds | **Decided:** a pointer iterator returns `^T`, a value iterator returns `Item?` (§8.5). Open only in the trait declaration: probably `Item?` collapses to `Item` for pointer types, since `?` on something that already encodes absence is the identity. Settle with `Iterate` in M6 |
| 18.20 | `or unreachable` lowering | Debug: panic with `#location`. Release: does it emit `__builtin_unreachable`, or merely skip the check? The first is faster and the second is kinder. §1.1 says Dazzle does not promise safety, which argues for the first (§9.4) |

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
| M5 | Ownership: `own`, `copy`, `drop`, `move`, flow analysis | your `Buffer<T>` and `String` samples compile and run leak-free under ASan |
| M6 | Generics + traits + monomorphization | `Buffer<T>`, `Allocator`, and `Arena` work as written in §6 |
| M7 | C++ interop via libclang | a raylib window opens from Dazzle |
| M8 | Unions, `switch`, `T?`, `!->`, the `or` clause | the `ButtonState` union sample from `example.dz` works |
| M9 | LSP: diagnostics, hover, go-to-definition, completion | usable in VS Code for real work |
| M10 | Macros | a user-written `#machine` expands to the `ButtonState` machine and the LSP completes inside it |
| M11 | Reflection + type table | a 50-line generic JSON serializer works on any `#[reflect]` type |
| M12 | Stdlib pass: `Buffer`, `String`, `HashMap`, allocators, math | you can write a small game without touching C++ |
| M13 | Hot reload (§17.6) | `dz run --hot` swaps a rebuilt module and migrates world state without restarting the game |

M4 is the one to aim at hard. Everything before it is invisible; the moment `dz run` prints
something, every later milestone becomes testable end to end.

---

## 20. Appendix: grammar sketch

Informal EBNF. `TERM` is a terminator (§2.2). This is a sketch for orientation, not the
normative grammar — the parser is the normative grammar, and this appendix is updated to
match it.

```ebnf
file          = { import | declaration } ;

import        = "import" ( module_path [ "as" ident ]
                         | module_path "::" "{" ident { "," ident } "}"
                         | "cpp" string [ "as" ident ] ) TERM ;
module_path   = ident { "::" ident } ;

declaration   = { attribute } [ visibility ] ( fn_decl | struct_decl | union_decl
                                             | enum_decl | trait_decl | const_decl
                                             | macro_decl | macro_invoke ) ;
visibility    = "pub" [ "(" "module" ")" ] ;
attribute     = "#" "[" ident [ "(" token_tree ")" ] "]" ;

fn_decl       = [ "mut" ] "fn" ident [ generics ] "(" [ params ] ")"
                [ ( "->" | "!->" ) type ] block ;
params        = param { "," param } ;
param         = [ "&" | "&mut" ] "self" | ident ":" type [ "=" expr ] ;
generics      = "<" gparam { "," gparam } ">" ;
gparam        = ident [ ":" bound { "+" bound } ] ;

struct_decl   = "struct" ident [ generics ] [ ":" trait_list ] "{" { member } "}" ;
member        = field | fn_decl | ctor | copy_fn | drop_fn ;
field         = { attribute } [ visibility ] [ "own" ] ident ":" type [ "=" expr ] TERM ;
ctor          = ident "(" [ params ] ")" block ;
copy_fn       = "copy" "(" ident ":" "&" "Self" ")" block ;
drop_fn       = "drop" "(" ")" block ;

union_decl    = "union" ident [ generics ] "{" { variant } "}" ;
variant       = ident [ "{" field_list "}" ] TERM ;
enum_decl     = "enum" ident [ ":" type ] "{" { ident [ "=" expr ] TERM } "}" ;
trait_decl    = "trait" ident [ generics ] "{" { trait_item } "}" ;
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
              | switch_stmt | return_stmt | defer_stmt | block
              | "break" [ ident ] | "continue" [ ident ] ;

let_stmt      = "let" [ "mut" ] pattern [ ":" type ] [ "=" expr ] TERM ;
assign        = place ( "=" | "+=" | "-=" | "*=" | "/=" | "%=" ) expr TERM ;
if_stmt       = "if" expr block { "else" "if" expr block } [ "else" block ] ;
while_stmt    = [ ident ":" ] "while" expr block ;
for_stmt      = [ ident ":" ] "for" pattern { "," pattern } "in" expr block ;
switch_stmt   = "switch" expr "{" { case_arm } [ "default" block ] "}" ;
case_arm      = "case" variant_pat { "," variant_pat } block ;
variant_pat   = ident [ "(" ident ")" ] ;
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
              | "." ident [ struct_lit_body ] ;
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
| r6 | 2026-09-10 | `or` is **non-associative**: `a() or b() or c` is a syntax error rather than a fallback chain (§9.4, §11.1). The first clause has already produced a value or diverged, so a second `or` could only apply to something that can no longer fail. Same reasoning as non-associative comparison. A fallback that can itself fail gets a clause of its own — `a() or { b() or c }` — which is how "try a, else try b, else c" is written. |
| r5 | 2026-09-10 | Access/modality split. `?` becomes a postfix suffix on the type name (`T?`, `^T?`, `[]T?`) and applies to value types only — `?^T`, `?&T`, `!&T` and the rest are gone, along with the niche-optimisation table that depended on them. `!` leaves the type system entirely and becomes a signature marker (`!-> T`), so a fallible value cannot be stored, annotated, or pointed at. `with`/`else`, `try` and `catch` are replaced by one postfix `or` clause governed by a single rule: produce a `T` or diverge. Bare `or return` is reserved for propagation. `^T` → `&T` now requires an `or` clause, which is what makes `&T`'s non-null property mean anything. §18 reopens with four items. |
| r4 | 2026-09-10 | All open questions closed. 18.3: `&T` in a struct field is banned outright (no `ref struct`); context structs use `^T`, and §5.2 now says honestly that this is a heuristic rather than a proof. 18.6: `^Trait` is a fat pointer, with a stable deterministic vtable id added for hot-reload re-resolution. §18 has no open items. |
| r3 | 2026-09-10 | Decisions pass. Resolved 18.1 (`f64` default), 18.2 (attributes are `#[...]`, unified with macros under the `#` sigil), 18.4, 18.5 (overloading deferred, not rejected), 18.9, 18.10, 18.11, 18.12, 18.13 (`machine` is a user macro, not stdlib), 18.14. Added §17.6 hot-reload constraints and roadmap M13. §18 restructured into a decisions log; 18.3 and 18.6 remain open with concrete proposals (`ref struct`, and the fat-vs-thin trait pointer trade). |
| r2 | 2026-09-10 | Review pass. `::` is now the only scope separator (§3.2/3.3); turbofish removed, justified by non-associative comparison; `?T`/`!T` are **built-in type constructors** — `Maybe<T>`/`Result<T,E>` deleted, one `Error` type, `none`/`error()` literals, niche-optimised `?^T` (§9); error style resolved to **Option C** (`with` + `try` + `catch`); associated types explained properly with worked examples (§8.5); new §8.6 arguing monomorphization over C++ templates; added §0.1 one-page summary. |
| r1 | 2026-09-10 | First draft. Decisions taken: Rust compiler, project-as-module, three visibility levels, `&`/`&mut`/`^`/`own ^`, definition-site generics, declarative macros, static type-table reflection, C++17 output via libclang interop. §18 lists what is still open, with §18.8 (error propagation) as the one that most needs an answer. |
