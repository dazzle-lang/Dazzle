# Name resolution and visibility, DZ03xx

## DZ0301

Nothing in scope has this name: not a local, a parameter, a declaration of this module, nor
an export of a dependency the module lists. The help says where one of that name is, or
which declared name is closest; an editor offers the closest as a fix.

```dz
fn main() {
    let count = 3
    println!("{}", cuont)
}
```

```dz
fn main() {
    let count = 3
    println!("{}", count)
}
```

## DZ0302

A module's declarations share one namespace, whichever files they are in, so two of one
name would make every use of it ambiguous. There is no overloading by parameters (spec
§7.2): give the second its own name, or one function default arguments.

```dz
fn area(w: f32, h: f32) -> f32 {
    return w * h
}

fn area(side: f32) -> f32 {
    return side * side
}

fn main() {
    println!("{}", area(2.0))
}
```

```dz
fn area(w: f32, h: f32) -> f32 {
    return w * h
}

fn square_area(side: f32) -> f32 {
    return side * side
}

fn main() {
    println!("{}", square_area(2.0))
}
```

## DZ0303

A module can name only the modules its `dazzle.toml` lists under `[dependencies]` (spec
§3.2). The standard library is one module, `std`; it has no submodules to import.

```dz
import std::physics as physics

fn main() {}
```

```dz
fn main() {
    let v = std::Vec2{ x: 1.0, y: 2.0 }
    println!("{}", v.x)
}
```

## DZ0304

A declaration without `pub` belongs to its module (spec §4). From outside, only what the
module exports can be named — here, a helper the standard library keeps to itself.

```dz
fn main() {
    let h = std::key_hash(&3)
}
```

```dz
fn main() {
    let h = 3.hash()
    println!("{}", h)
}
```

## DZ0305

The path names a type or module that exists, but the last segment is not one of its
members: no such variant, associated function or constant.

```dz
enum Dir {
    Up
    Down
}

fn main() {
    let d = Dir::Left
}
```

```dz
enum Dir {
    Up
    Down
}

fn main() {
    let d = Dir::Up
    println!("{}", d as i32)
}
```

## DZ0306

A name means one thing in a scope (spec §7.1): a second `let` of the same name in the same
block would make the code above and below it read the name differently. Assign to a `let
mut` instead, or give the new value its own name.

```dz
fn main() {
    let hp = 10
    let hp = hp - 3
    println!("{}", hp)
}
```

```dz
fn main() {
    let hp = 10
    let damaged = hp - 3
    println!("{}", damaged)
}
```

## DZ0307

An `import` that nothing uses is a warning: it says the file depends on something it does
not.

```dz
import std::{ Vec3 }

fn main() {
    println!("{}", 1)
}
```

```dz
import std::{ Vec3 }

fn main() {
    let up = Vec3{ x: 0.0, y: 1.0, z: 0.0 }
    println!("{}", up.y)
}
```

## DZ0308

Two imports in one file bring in the same name, so it would mean two things. Import it once,
or rename one with `as`.

```dz
import std::{ Vec2 }
import std::{ Vec2 }

fn main() {
    let v = Vec2{ x: 1.0, y: 2.0 }
    println!("{}", v.x)
}
```

```dz
import std::{ Vec2 }

fn main() {
    let v = Vec2{ x: 1.0, y: 2.0 }
    println!("{}", v.x)
}
```

## DZ0309

`#if` reads build flags, and a flag must be registered: one of the built-in ones — `DEBUG`,
`RELEASE`, `SHIPPING`, `OS_WINDOWS`, `OS_LINUX`, `OS_MACOS` — or declared under `[flags]` in a
`dazzle.toml` (spec §12.10). An unregistered flag is refused rather than read as off, which
is what turns a misspelling into a silently dead branch.

```dz
fn main() {
    #if DEBUGG {
        println!("checks on")
    }
}
```

```dz
fn main() {
    #if DEBUG {
        println!("checks on")
    }
}
```

## DZ0310

Two dependencies opened with `open = true` both export this name, so the bare name could
mean either. Qualify it — `engine::Vec2` — or choose one for the whole module in
`dazzle.toml`: `[names] Vec2 = "std"` (spec §3.2).

No example: it takes two dependencies, each a module with its own `dazzle.toml`.

## DZ0311

A `pub` declaration is part of the module's interface, so every type in its signature must
be nameable by the module's users — and a private dependency's types are not (spec §3.2).
Make the dependency `pub = true`, or keep the declaration private.

No example: it takes a dependency, a module with its own `dazzle.toml`.

## DZ0312

Since r50 a module's dependencies are listed in `dazzle.toml`, and every file can name them
by their key; `import m` alone and `import cpp` are gone. A file still writes `import m::{
A, B }` or `import m as x` for an exception of its own.

```dz
import std

fn main() {}
```

```dz
fn main() {
    let v = std::Vec2{ x: 1.0, y: 2.0 }
    println!("{}", v.x)
}
```

## DZ0313

A dependency in `[dependencies]` that nothing in the module names is a warning: the
manifest claims a dependency the code does not have.

No example: the mistake is in `dazzle.toml`, which every example here shares.

## DZ0314

Dazzle's attributes are the ones spec §12.8 lists — `#[test]`, `#[bench]`,
`#[thread_local]` among them. An unknown one is an error rather than ignored, so a
misspelt `#[test]` is not a test that silently never runs.

```dz
#[tset]
fn adds() {
    assert(1 + 1 == 2)
}

fn main() {}
```

```dz
#[test]
fn adds() {
    assert(1 + 1 == 2)
}

fn main() {}
```

## DZ0315

The attribute exists but does not apply here: `#[test]` and `#[bench]` go on functions,
`#[thread_local]` on a `static`. Each takes no arguments and is written once.

```dz
#[thread_local]
fn count() -> i32 {
    return 1
}

fn main() {}
```

```dz
#[thread_local]
static COUNT: i32 = 1

fn main() {}
```

## DZ0316

The specification describes the attribute, but the compiler does not implement it yet, so
it is refused rather than quietly doing nothing.

```dz
#[packed]
struct Header {
    pub tag: u8 = 0
    pub size: u32 = 0
}

fn main() {}
```

```dz
struct Header {
    pub tag: u8 = 0
    pub size: u32 = 0
}

fn main() {}
```

## DZ0317

An `extern fn`'s body is C++ that the compiler supplies, which is how the standard library
reaches the operating system (spec §7.2). A program reaches C and C++ through a `cpp`
dependency instead (spec §15.1).

```dz
extern fn monotonic_nanos() -> u64

fn main() {
    println!("{}", monotonic_nanos())
}
```

```dz
fn main() {
    let t = Instant::now()
    println!("{}", t.elapsed().as_secs_f64() >= 0.0)
}
```
