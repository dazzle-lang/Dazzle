# Dazzle

**A systems language for game development that compiles to readable C++17.**

Dazzle has the control of C++ without forty years of accumulated syntax: RAII reduced to three
special members, encapsulation the compiler enforces, errors as values instead of
exceptions, generics checked where they are written, and C and C++ libraries used directly
from their source, with no wrapper or build system. The compiler is written in Rust.

```dz
fn main() {
    rl::InitWindow(800, 450, "Dazzle")
    defer rl::CloseWindow()

    while !rl::WindowShouldClose() {
        rl::BeginDrawing()
        rl::ClearBackground(rl::RAYWHITE)
        rl::DrawCircleV(rl::GetMousePosition(), 8, rl::MAROON)
        rl::EndDrawing()
    }
}
```

That is raylib, called with no bindings. `dz` compiled it from raylib's own seven `.c`
files, which is all a `dazzle.toml` recipe names. No CMake and no prebuilt library were
involved.

> **Status: pre-release.** The language is **0.1, unreleased** (spec revision 64). It compiles, runs,
> checks ownership, calls C and C++, and has a language server; it already carries a small
> raylib game. It is not finished: 0.1 is declared once the editor support, the standard
> library and C++ interop are all complete (see [Status](#status-and-roadmap)). Expect the
> language to change.

---

## Why Dazzle

C++ gives you control and abstraction, but buries them under undefined behaviour and syntax
nobody can hold in their head. Odin gives you clarity, but takes away encapsulation, RAII
and layered abstractions that hold up in a large codebase. Dazzle aims at the space between
the two. Its principles, in priority order:

1. **You can always see the machine.** No hidden allocations, no hidden control flow, no
   garbage collector, no runtime you did not ask for.
2. **Destruction is deterministic and automatic.** RAII with three special members: a
   constructor, `copy` and `drop`.
3. **Encapsulation is enforced, not conventional.** Private is the default.
4. **Errors point at your code**, never at generated code or a 400-line template trace.
5. **The escape hatch is a door, not a hole.** C++ interop is a first-class feature:
   raylib, Jolt and Dear ImGui should work on day one.
6. **Compile fast.**

Dazzle is **not** trying to be memory-safe in Rust's sense. It prevents use-after-move and
double-free, and it marks every raw-pointer access with `unsafe` so you can find it, but it
has no lifetimes and no borrow checker.

## The language at a glance

### Errors are values, not exceptions

There are no exceptions, no `try`/`catch` and no `Result<T, E>`. A function that can fail
says so on its signature with `!->`, and the caller handles the failure with **one postfix
`or` clause**. The same clause handles an optional value (`T?`) and a nullable pointer
(`^T`):

```dz
enum ErrorCode : u32 {
    Empty = 1
    TooLong
}

fn parse_name(text: str) !-> str {
    if text.len == 0 {
        return error(ErrorCode::Empty as u32, "a name cannot be empty")
    }
    if text.len > 16 {
        return error(ErrorCode::TooLong as u32, "name too long: {} bytes", text.len)
    }
    return text
}

fn greet(text: str) !-> i32 {
    let name = parse_name(text) or return          // propagate
    #println("hello, {}", name)
    return 0
}

fn main() {
    let name = parse_name("") or "stranger"        // fall back
    #println("hello, {}", name)
    greet("Ada") or ignore                         // deliberately drop the failure
    greet("a name that is far too long") or { e -> // handle it
        #println("failed at {}:{}: {}", e.location.file, e.location.line, e.message)
        0
    }
}
```

```text
hello, stranger
hello, Ada
failed at src/main.dz:11: name too long: 27 bytes
```

An `Error` is 32 bytes and **never allocates**. It carries a code, a message and the source
location where it was created, which is most of what a stack trace gives you, at no runtime
cost. A failure cannot be stored in a variable, only handled, so forgetting to handle one
is a compile error:

```text
error[DZ0510]: fallible call not consumed
  ┌─ src/main.dz:9:17
  │
9 │     let level = load("level1.dat")
  │                 ^^^^^^^^^^^^^^^^^^ a failure cannot be stored, only handled
  │
  = help: add an `or` clause: `let v = f() or return`, or `let v = f() or <fallback>` (spec §9.7)
```

### Ownership without a borrow checker

A type that owns something gets a `drop`, which runs deterministically when the value goes
away. `copy` is opt-in, and `move()` transfers ownership and resets the source to its
defaults. Copying a type that owns something by accident is an error, and so is using a
value after moving it:

```dz
struct Texture {
    pub name: str
    pub id: u32
}

impl Drop for Texture {
    fn drop(&mut self) { /* unload it */ }
}

struct Sprite {
    pub texture: Texture
    pub x: f32 = 0                 // every field is initialised; there is no garbage memory
}

fn main() {
    let a = Sprite{ texture: Texture{ name: "player.png", id: 1 } }
    let b = move(a)                // `let b = a` would not compile: Sprite owns a Texture
    #println("{}", b.texture.name)
}                                  // the texture is dropped here, once
```

Read `a` after the move, and:

```text
error[DZ0401]: `a` was moved and cannot be used again
   ┌─ src/main.dz:18:20
   │
17 │     let b = move(a)                // `let b = a` would not compile: Sprite owns a Texture
   │             ------- moved here
18 │     #println("{}", a.texture.id)
   │                    ^^^^^^^^^^^^ used here after the move
   │
   = help: a moved-from value is left at its defaults, and reading it back would almost never be what was meant (spec §6.4)
```

`&T` and `&mut T` are references that can never be stored in a field, so no lifetimes are
needed. `^T` is a C pointer, and reading or writing through one happens inside `unsafe`. The
standard library has `Rc`, `Weak` and `Box` for everything in between.

### Use C and C++ libraries as they are

A library vendored as **source** is built by `dz` from a few lines of manifest, and its
header is read by libclang. Its functions, structs, enums, macros and constants then become
an ordinary Dazzle module:

```toml
[dependencies]
rl = { cpp = "raylib" }

[cpp.libs.raylib]
root         = "vendor/raylib/src"
sources      = ["*.c"]
include_dirs = [".", "external/glfw/include"]
defines      = ["PLATFORM_DESKTOP", "GRAPHICS_API_OPENGL_33"]
import       = ["raylib.h"]

[cpp.libs.raylib.target.windows]
link_libs = ["opengl32", "gdi32", "winmm", "user32", "shell32"]
```

That covers C++ too: namespaces, classes, constructors, `const` and mutating methods,
overloaded methods chosen by argument type, references, classes that C++ destroys itself,
and named template instances. From [`tests/run/cppinterop`](tests/run/cppinterop), which
builds a small C++ library from source:

```dz
let mut counter = shp::shapes::Counter(10)    // a C++ class and its constructor
counter.add(5)                                // an overload set, resolved by argument type
counter.add(a)                                //   …the other `add`
shp::shapes::translate(&mut moving, 1, 1)     // `Point&` is a `&mut Point`
let pair = shp::IntPair(3, 9)                 // `[cpp.instantiate] "shapes::Pair<int>"`
```

The generated code is plain C++17 with `#line` directives pointing back at your `.dz`
files, so gdb, the Visual Studio debugger and profilers all show your source.

### Macros you can still get completion inside

Macros are declarative: a pattern that reads like a function's parameters, and an
expansion written in Dazzle. They expand on syntax before name resolution, are hygienic for
locals, and the editor keeps completing and hovering *inside* an invocation. A state
machine, for example, is a user macro rather than a language feature (trimmed from
[`tests/run/machine`](tests/run/machine)):

```dz
#machine ButtonState(ctx: &ButtonCtx, input: &Input, btn: &mut Button) {
    state DefaultState {
        if input.mouse_down && input.over {
            return .TransitionState{}
        }
    }

    state TransitionState(t: f32 = 0) {
        t += ctx.delta_time
        btn.scale = lerp(1, 2, t)
        if t >= 1 {
            return .DefaultState{}
        }
    }
}
```

`#machine` is about sixty lines of Dazzle in the game's own code. It writes a struct per
state, a tagged union over them and a `step` method. `dz expand` prints exactly what any
invocation turns into. Formatting is built from the same machinery: `#println`, `#format`
and `#panic` check their format string against their arguments at compile time.

### And the rest

- **Generics checked at the definition**, then monomorphized by Dazzle rather than by C++
  templates. A mistake in a generic function is reported in that function, once.
- **Tagged unions and exhaustive `switch`**, with no fallthrough. A missing case is a
  compile error, and the editor can write the missing arms for you.
- **Traits** with static dispatch through bounds (`<T: Ord>`) or dynamic dispatch through a
  fat pointer (`^Trait`); single **struct inheritance** with `virtual`/`override` where you
  want a class hierarchy.
- **Reflection** through one static type table: `typeinfo_of<T>()`, with field tags, at zero
  bytes per struct.
- **Tests and benchmarks are part of the language**: `#[test]` and `#[bench]` sit beside the
  code they cover, and each test runs in its own process, so one crash does not hide the
  rest.

  ```dz
  #[test]
  fn clamp_keeps_values_in_range() {
      assert(clamp01(2) == 1)
      assert(clamp01(-3) == 0, "negative values clamp to zero")
  }
  ```
- **Modules and visibility** come from `dazzle.toml`, not from folders or per-file imports;
  private is the default.

## Error messages, tooling, and working with an LLM

Dazzle is designed to be learned from the compiler. No model has Dazzle in its training
data, and neither does a new programmer, so the compiler has to teach. **Available today:**

- **Every diagnostic has a stable code** (`DZ0510`), points at your code and never at the
  generated C++, and nearly always ends in a `help:` that says what to write instead and
  which spec section explains why.
- **One mistake, one error.** The parser suppresses cascades, so a typo does not produce
  four restatements of itself.
- **Errors you can apply.** A diagnostic can carry the exact edit that fixes it. In the
  editor that is a quick fix (fill a `switch`'s missing arms, implement a trait method an
  `impl` left out), tested by applying the edit and re-checking the result.
- **A language server** (`dz lsp`) with diagnostics as you type, hover, go-to-definition,
  references, rename, completion (including inside macro invocations), signature help,
  inlay hints and the outline. It keeps answering around syntax errors. There is also a
  **VS Code extension** with highlighting, run and debug buttons, and debugger
  visualisers.
- **`dz expand`** shows what a macro wrote, and `dz fmt` is the single canonical format.
- **One way to do things:** one `or` clause instead of `?`/`try`/`catch`, no function
  overloading (one name is one symbol), and no implicit numeric conversions.

**Planned**, tracked in the backlog ([`docs/tasks.md`](docs/tasks.md)):

- Diagnostics that recognise **habits from Rust and C++** (`Result<T, E>`, `x?`, `nullptr`,
  `p->x`, `Foo::new()`) and give the Dazzle spelling.
- `dz check --format=json`, and `dz fix` to apply every mechanical fix.
- `dz explain DZ0510` (the rule, its spec section, a wrong and a corrected program) and
  `dz api <module>`, which prints a module's public surface, an imported C++ library's
  included, as Dazzle signatures.
- Editor queries from the command line, a primer with `dz new`, machine-readable test
  output, and `dz run --frames N` so a game can be checked without anyone watching its
  window.

## Installation

There is no release yet; Dazzle is built from source. An installer that bundles everything is
planned.

**You need:**

| | Why |
|---|---|
| A current stable **Rust** toolchain | to build `dz` |
| **clang++** or **g++** with C++17 | `dz` compiles the C++ it generates with it |
| **libclang** (any recent LLVM) | only for `import cpp`; a program that calls no C or C++ never loads it |

```bash
git clone https://github.com/Marco012/Dazzle.git
```

```bash
cd Dazzle && cargo build --release
```

The compiler is then `target/release/dz` (`dz.exe` on Windows). Put it on your `PATH`.

`dz` finds libclang beside its own executable, in a `lib/` folder next to it, through
`LIBCLANG_PATH`, or in an installed LLVM. On Windows that means LLVM's installer; on Ubuntu,
the `libclang-dev` package; on macOS, Xcode's command-line tools or Homebrew's `llvm`.

**The VS Code extension** (needs Node.js):

```bash
cd editors/vscode && npm install && npm run deploy
```

## Usage

A project is a folder with a `dazzle.toml`:

```toml
[module]
name    = "hello"
sources = ["src/**/*.dz"]

[dependencies]
std = { open = true }

[build]
output = "executable"
```

and some code, here `src/main.dz`:

```dz
fn main() {
    #println("Hello, world!")
}
```

```bash
dz run .
```

| Command | Does |
|---|---|
| `dz run [dir]` | build and run |
| `dz build [dir]` | build an executable into `build/bin/` |
| `dz check [dir]` | analyse only: every diagnostic, no C++ compiler needed |
| `dz test [dir] [filter]` | run the `#[test]` functions, each in its own process |
| `dz bench [dir]` | time the `#[bench]` functions (release mode by default) |
| `dz fmt <file>` | format in place (`--stdout` to print instead) |
| `dz expand <file>` | the file with every macro invocation expanded |
| `dz cpp [dir]` | write the generated C++ to `build/cpp/` |
| `dz lsp` | the language server, on stdio |

`dir` defaults to the current directory. `run`, `build`, `cpp`, `test` and `bench` take
`--debug` (the default), `--release` or `--shipping`, and `--track-memory` records every
allocation and reports leaks and bad frees when the program exits. `examples/` has real
projects to start from: `raylib` and `rgfw` open a window, `breakout` is a small game, and
`books` is a plain program.

## Compatibility

- **Platforms:** Windows, Linux and macOS. CI builds and tests on all three for every change.
- **C++ compilers:** clang++ and g++. The generated code is C++17, compiled with
  `-fno-exceptions -fno-rtti` and expected to be warning-clean under `-Wall -Wextra` on both;
  clang++ 21 and g++ 13 are verified. On Windows that means clang++ or MinGW g++. MSVC's
  `cl.exe` is not supported, since `dz` passes GCC-style flags. `dz` uses the first of
  `clang++`, `g++` and `c++` it finds; choose another with `--cxx`, the `DZ_CXX` environment
  variable, or `[build] cxx` in the manifest.
- **C and C++ libraries:** C, and C++ through namespaces, classes, methods, overloaded
  methods, references and named template instances. Overloaded *free* functions, variadic C
  functions and inheritance between imported C++ classes are not imported yet; they are open
  tasks.
- **The output is plain C++**, so anything that links with C++ links with Dazzle.

## Status and roadmap

| | |
|---|---|
| **Done** | lexer, parser, formatter · modules and visibility · type checker · C++ code generation · ownership · generics and traits · C and C++ interop from source · tagged unions and `switch` · macros · reflection · `dz test` · `dz bench` and the C++ comparison suite · CI on three platforms |
| **Mostly done** | the language server and VS Code extension: still to come are incremental analysis and the remaining quick fixes |
| **Now** | the standard library. Its criterion, a raylib game written without any C++, is met: [`examples/breakout`](examples/breakout) |
| **Later** | fetched packages with a lockfile, hot reload, and an installer that bundles libclang |

The open work, milestone by milestone, is the backlog in [`docs/tasks.md`](docs/tasks.md).

## Documentation

| | |
|---|---|
| [`docs/spec/`](docs/spec/README.md) | the language specification, the contract. **§0.1 is the whole language on one page.** |
| [`docs/decisions/`](docs/decisions/README.md) | every design decision, and the questions still open |
| [`docs/tasks.md`](docs/tasks.md) | the backlog: open work by milestone |
| [`docs/log/`](docs/log/README.md) | what each milestone built and why |
| [`docs/testing.md`](docs/testing.md) | what each test corpus covers |
| [`CLAUDE.md`](CLAUDE.md) | how to work in this repository: the invariants, conventions and commands |

## License

MIT, as declared in `Cargo.toml`.
