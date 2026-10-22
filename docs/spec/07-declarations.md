# 7. Declarations

## 7.1 Bindings

```dz
let x = 5                     // immutable, inferred i32
let mut y: f32 = 1.5          // mutable
const MAX_ENTITIES = 4096     // compile-time constant, no storage
let _ = returns_something()   // explicitly discard
```

`let` without an initialiser requires a type annotation and default-initialises (§6.2).
Shadowing in an inner scope is allowed; shadowing in the *same* scope is an error — the
`main.dz` sample shows how easily a duplicate `let b` slips past you.

## 7.2 Functions

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
  `Buffer(8, arena)` are genuinely the same operation. A call — and a base initialiser,
  `: Base(…)` (§7.3) — reaches the constructor that takes that many arguments, **counting
  those its defaults supply**, and whose parameters the arguments fit. Where several fit,
  the one every argument's type matches exactly is chosen, an untyped literal matching the
  type it would default to; if that still leaves more than one, the call is ambiguous and an
  error. So `Color(r: u8, g: u8, b: u8, a: u8 = 255)` and `Color(gray: f32)` are told apart
  by `Color(1, 2, 3)` and `Color(0.5)`, while `Span(from: i32, to: i32 = 0)` beside
  `Span(len: i32)` makes `Span(4)` an error.

**Arguments are evaluated left to right**, each completely before the next: in
`println!("{} {}", touch(), COUNT.hits)`, `touch()` has run by the time `COUNT.hits` is
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
format!("{} hp", hp, allocator = Allocators::Temp)
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

## 7.3 Structs

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

### Inheritance

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

**A base may be an instance of a generic struct**: `struct Button : Panel<i32>` inherits
`Panel<i32>`, its fields and methods with `T` read as `i32`, and is built through
`Panel<i32>`'s constructor, `: Panel(clicks)`. A generic derived type may pass its own
parameter on — `struct Slider<T> : Panel<T>` — and a non-generic base is as before. A
`&Button` converts to a `&Panel<i32>` and to no other instance of `Panel`.

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

## 7.4 Enums

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

## 7.5 Unions

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

**A union may declare methods**, after or among its variants, written as a struct's are:

```dz
union ButtonState {
    DefaultState
    TransitionState
    ChangeSprite

    pub fn name(&self) -> str {
        return switch self {
            case DefaultState { "default" }
            case TransitionState { "transition" }
            case ChangeSprite { "change" }
        }
    }

    pub fn step(&mut self, ctx: &ButtonCtx) {
        let next: ButtonState? = switch self {
            case TransitionState(&mut t) { t.update(ctx) }
            default { none }
        }
        self = next or return                      // replaces the whole union
    }
}
```

`self` is the union, and its payload is reached with `switch self`. A method has the same
visibility rules as a struct's (§4), and a method without `self` is called through the
union's name, `ButtonState::initial()`. A union declares methods only: its fields are its
variants, it is built from a variant, and copying and dropping it are already defined by
its variants (§18.52), so a union has no constructors, `copy` or `drop` of its own. `kind`
is taken by the generated `s.kind()`. `[DECIDED — §18.78]`

Untagged/overlapping storage, should interop ever need it, is a separate `#[raw] union`.

## 7.6 Traits

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
implementation of the trait. Nor is one for a pointer or reference type — `impl Hash for
^Node` — since a method call reaches through a pointer to what it points at (§5.2) and so
could never find it; to give a pointer behaviour of its own, wrap it in a struct and
implement the trait for that. Two implementations of one trait that could apply to the same
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

## 7.7 Statics

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
