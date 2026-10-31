# Generics, traits and the rest of type checking, DZ053x-DZ056x

## DZ0530

A bound, `T: Ord`, names a trait — what `T` must be able to do. A struct or enum is a type,
not a capability, so it cannot bound a parameter.

```dz
struct Point {
    pub x: i32
}

fn first<T: Point>(xs: []T) -> T {
    return xs[0]
}

fn main() {}
```

```dz
fn first<T: Copy>(xs: []T) -> T {
    return xs[0]
}

fn main() {
    let xs = [3, 4]
    println!("{}", first(xs[..]))
}
```

## DZ0531

A generic body may do with `T` only what its bounds allow (spec §8.1). This is checked in
the generic function itself, at the line that does it, rather than at some later
instantiation — which is the point of definition-site generics (§8.6). Add the bound.

```dz
fn larger<T: Copy>(a: T, b: T) -> T {
    if a > b {
        return a
    }
    return b
}

fn main() {
    println!("{}", larger(3, 4))
}
```

```dz
fn larger<T: Copy + Ord>(a: T, b: T) -> T {
    if a > b {
        return a
    }
    return b
}

fn main() {
    println!("{}", larger(3, 4))
}
```

## DZ0532

An `impl` provides every method the trait requires — every one without a default body
(spec §7.6). An editor offers the missing signatures as a fix.

```dz
trait Shape {
    fn area(&self) -> f32
    fn sides(&self) -> i32
}

struct Square {
    pub side: f32
}

impl Shape for Square {
    fn area(&self) -> f32 {
        return side * side
    }
}

fn main() {}
```

```dz
trait Shape {
    fn area(&self) -> f32
    fn sides(&self) -> i32
}

struct Square {
    pub side: f32
}

impl Shape for Square {
    fn area(&self) -> f32 {
        return side * side
    }

    fn sides(&self) -> i32 {
        return 4
    }
}

fn main() {}
```

## DZ0533

An implementation's method has exactly the trait method's signature — the same receiver,
parameters and return type — since callers were checked against the trait's (spec §7.6).

```dz
trait Shape {
    fn area(&self) -> f32
}

struct Square {
    pub side: f32
}

impl Shape for Square {
    fn area(&self) -> f64 {
        return (side * side) as f64
    }
}

fn main() {}
```

```dz
trait Shape {
    fn area(&self) -> f32
}

struct Square {
    pub side: f32
}

impl Shape for Square {
    fn area(&self) -> f32 {
        return side * side
    }
}

fn main() {}
```

## DZ0534

A generic type or function takes as many type arguments as it has parameters, or none when
every one can be inferred from the arguments (spec §8.1).

```dz
fn total(xs: List<i32, i32>) -> usize {
    return xs.len()
}

fn main() {}
```

```dz
fn total(xs: &List<i32>) -> usize {
    return xs.len()
}

fn main() {}
```

## DZ0536

Every instantiation of a generic is a new piece of code (spec §8.4). One that instantiates
itself at a new type each time — `T`, then `[1]T`, then `[1][1]T` — never ends, and the
compiler stops it with this error rather than leaving C++ to run out of template depth.

```dz
fn nest<T: Copy>(x: T, n: i32) -> i32 {
    if n == 0 {
        return 0
    }
    return nest([x], n - 1)
}

fn main() {
    println!("{}", nest(1, 3))
}
```

```dz
fn nest<T: Copy>(x: T, n: i32) -> i32 {
    if n == 0 {
        return 0
    }
    return 1 + nest(x, n - 1)
}

fn main() {
    println!("{}", nest(1, 3))
}
```

## DZ0537

A type has one method of a name; there is no overloading (spec §7.2, §18.5). The one
exception is a trait implemented at several arguments — `Mul<f32>` beside `Mul<Vec3>` —
whose methods share the trait's name (§18.27).

```dz
struct Sprite {
    pub w: f32

    pub fn scale(&mut self, by: f32) {
        w *= by
    }

    pub fn scale(&mut self, by: i32) {
        w *= by as f32
    }
}

fn main() {}
```

```dz
struct Sprite {
    pub w: f32

    pub fn scale(&mut self, by: f32) {
        w *= by
    }

    pub fn scale_by_int(&mut self, by: i32) {
        w *= by as f32
    }
}

fn main() {}
```

## DZ0538

More than one candidate accepts the call: two implementations of one trait at different
arguments, both of which an untyped literal fits, or two constructors that both take these
arguments — counting the ones their defaults supply (spec §7.2, §18.27). Give an argument a
type, or pass the argument a default would have supplied.

```dz
struct Span {
    pub from: i32
    pub to: i32

    Span(from: i32, to: i32 = 0) {
        self.from = from
        self.to = to
    }

    Span(len: i32) {
        self.from = 0
        self.to = len
    }
}

fn main() {
    let s = Span(4)
    println!("{}", s.to)
}
```

```dz
struct Span {
    pub from: i32
    pub to: i32

    Span(from: i32, to: i32 = 0) {
        self.from = from
        self.to = to
    }

    Span(len: i32) {
        self.from = 0
        self.to = len
    }
}

fn main() {
    let s = Span(4, 0)
    println!("{}", s.to)
}
```

## DZ0539

An `impl` is written with the trait or with the type — the orphan rule — so a program using
two libraries can never find both implementing a third's trait for a fourth's type (spec
§7.6). Nor is one written for every type at once, `impl<T> Show for T`, for a pointer or
reference type, which a method call reaches through, or a `Drop` or `Copy` for anything but
a struct of this module. To implement someone else's trait for someone else's type, wrap
the type in a struct of your own.

```dz
impl Add for i32 {
    type Output = i32

    fn add(&self, rhs: i32) -> i32 {
        return 0
    }
}

fn main() {}
```

```dz
struct Meters {
    pub v: i32
}

impl Add for Meters {
    type Output = Meters

    fn add(&self, rhs: Meters) -> Meters {
        return Meters{ v: v + rhs.v }
    }
}

fn main() {
    let d = Meters{ v: 1 } + Meters{ v: 2 }
    println!("{}", d.v)
}
```

## DZ0540

Two implementations of one trait that could apply to the same type would leave every call
to choose between them (spec §7.6). Keep one.

```dz
trait Named {
    fn name(&self) -> str
}

struct Unit {
}

impl Named for Unit {
    fn name(&self) -> str {
        return "unit"
    }
}

impl Named for Unit {
    fn name(&self) -> str {
        return "also unit"
    }
}

fn main() {}
```

```dz
trait Named {
    fn name(&self) -> str
}

struct Unit {
}

impl Named for Unit {
    fn name(&self) -> str {
        return "unit"
    }
}

fn main() {}
```

## DZ0541

A base is one struct of Dazzle's own (spec §7.3, §18.32): not a trait — implement that with
`impl` — not an enum or union, not an imported C++ class yet, and not the type itself or a
chain that comes back round to it, which would contain itself.

```dz
struct Node : Node {
    pub value: i32
}

fn main() {}
```

```dz
struct Node {
    pub value: i32
}

struct Leaf : Node {
    pub weight: f32 = 1.0
}

fn main() {}
```

## DZ0542

A derived type has its base's fields and methods as its own (spec §7.3). Declaring one again
would make a name mean two things in one type — there is no hiding — so pick another name,
or use the base's. A method replaces a base's `virtual` one with `override`.

```dz
struct Shape {
    pub color: i32
}

struct Circle : Shape {
    pub color: i32
}

fn main() {}
```

```dz
struct Shape {
    pub color: i32
}

struct Circle : Shape {
    pub radius: f32 = 1.0
}

fn main() {}
```

## DZ0543

A base with a constructor establishes its own invariant, so a derived constructor must call
it, after its parameters: `Button(r: Rect) : UiElement(r) { … }` (spec §7.3). For the same
reason a type that has a constructor is not built with a struct literal.

```dz
struct Shape {
    pub sides: i32

    Shape(sides: i32) {
        self.sides = sides
    }
}

struct Square : Shape {
    pub side: f32 = 1.0

    Square(side: f32) {
        self.side = side
    }
}

fn main() {}
```

```dz
struct Shape {
    pub sides: i32

    Shape(sides: i32) {
        self.sides = sides
    }
}

struct Square : Shape {
    pub side: f32 = 1.0

    Square(side: f32) : Shape(4) {
        self.side = side
    }
}

fn main() {}
```

## DZ0544

`override` replaces a base's `virtual` method, with exactly its signature (spec §7.3). One
with nothing to override, one aimed at a method that is not `virtual`, or one whose
signature has drifted from the base's is refused, so a rename in the base is noticed.

```dz
struct Widget {
    pub virtual fn draw(&self) {}
}

struct Button : Widget {
    pub override fn render(&self) {}
}

fn main() {}
```

```dz
struct Widget {
    pub virtual fn draw(&self) {}
}

struct Button : Widget {
    pub override fn draw(&self) {}
}

fn main() {}
```

## DZ0545

A `virtual fn` without a body makes its type abstract: there is nothing to call, so no value
of it can exist (spec §7.3). Make a derived type that overrides it, and refer to that through
a `&Base` or `^Base`.

```dz
struct Widget {
    pub virtual fn draw(&self)
}

fn main() {
    let w = Widget{}
}
```

```dz
struct Widget {
    pub virtual fn draw(&self)
}

struct Label : Widget {
    pub override fn draw(&self) {
        println!("label")
    }
}

fn show(w: &Widget) {
    w.draw()
}

fn main() {
    let l = Label{}
    show(&l)
}
```

## DZ0546

`dz test` calls each `#[test]` function with nothing and expects nothing back: a test passes
by returning and fails by panicking (spec §19 M14). So it takes no arguments, returns no
value, cannot fail, is not generic, and has a body.

```dz
#[test]
fn adds(n: i32) {
    assert(n + n == 2 * n)
}

fn main() {}
```

```dz
#[test]
fn adds() {
    let n = 3
    assert(n + n == 2 * n)
}

fn main() {}
```

## DZ0547

A `case` label is a constant the subject is compared with — a value on an integer or `str`,
a variant on an enum or union — and only a union's variant binds a payload, `case Circle(c)`
(spec §10.3). A float is not switched on, since equal floats are rarely what was meant.

```dz
fn main() {
    let n = 3
    switch n {
        case X(x) {}
        default {}
    }
}
```

```dz
fn main() {
    let n = 3
    switch n {
        case 3 {
            println!("three")
        }
        default {}
    }
}
```

## DZ0548

Each variant or value is handled by one `case`: a second would never run. Labels are folded
first, so `2 + 2` and `4` are the same label.

```dz
fn main() {
    let n = 4
    switch n {
        case 4 {}
        case 2 + 2 {}
        default {}
    }
}
```

```dz
fn main() {
    let n = 4
    switch n {
        case 4 {}
        case 2 + 3 {}
        default {}
    }
}
```

## DZ0549

A `for` or `case` binding fits what it binds (spec §10.4): a tuple element is taken apart
into as many names as it has parts, without brackets — `for a, b in pairs` — and `&e` refers
only to elements that live somewhere.

```dz
fn main() {
    let pairs = [(1, 2.5)]
    for (a, b) in pairs {
        println!("{} {}", a, b)
    }
}
```

```dz
fn main() {
    let pairs = [(1, 2.5)]
    for a, b in pairs {
        println!("{} {}", a, b)
    }
}
```

## DZ0550

`break` and `continue` act on a loop: the innermost one, or the one whose label they name.
Outside every loop there is nothing to leave; a function is left with `return`.

```dz
fn main() {
    let n = 3
    if n > 2 {
        break
    }
}
```

```dz
fn main() {
    let n = 3
    if n > 2 {
        return
    }
}
```

## DZ0551

A union variant written as a bare name carries a value of the struct of that name (spec
§7.5, §18.45). With no such struct in scope it is an error — a misspelling, often — and a
variant without a payload is written with braces, `Idle {}`.

```dz
union State {
    Idle
    Running { speed: f32 }
}

fn main() {
    let s = State::Running{ speed: 1.0 }
}
```

```dz
union State {
    Idle {}
    Running { speed: f32 }
}

fn main() {
    let s = State::Running{ speed: 1.0 }
}
```

## DZ0552

The base is built before the constructor's body begins, so in `: Base(…)` there is no
constructor yet to return from (spec §7.3). Supply a fallback instead of `or return`.

```dz
struct Sized {
    pub w: i32 = 0

    Sized(width: i32) {
        w = width
    }
}

struct Boxed : Sized {
    Boxed(width: i32?) : Sized(width or return) {}
}

fn main() {
    let b = Boxed(none)
}
```

```dz
struct Sized {
    pub w: i32 = 0

    Sized(width: i32) {
        w = width
    }
}

struct Boxed : Sized {
    Boxed(width: i32?) : Sized(width or 0) {}
}

fn main() {
    let b = Boxed(none)
    println!("{}", b.w)
}
```

## DZ0553

A `static` is built before `main` runs, in an order C++ does not promise across files, so
its initialiser is made of constants only — no calls, no reads of other statics (spec §7.7).
Set it in `main` if it must be computed.

```dz
fn seven() -> i32 {
    return 7
}

static COMPUTED: i32 = seven()

fn main() {
    println!("{}", COMPUTED)
}
```

```dz
const SEVEN: i32 = 7

static COMPUTED: i32 = SEVEN

fn main() {
    println!("{}", COMPUTED)
}
```

## DZ0554

A function takes at most one variadic parameter, `xs: ..T`, with no default (spec §7.2); the
parameters after it are passed by name. A spread, `..xs`, passes a slice as the whole of the
variadic part.

```dz
fn sum(xs: ..i32, ys: ..i32) -> i32 {
    return 0
}

fn main() {}
```

```dz
fn sum(xs: ..i32) -> i32 {
    let mut total = 0
    for x in xs {
        total += x
    }
    return total
}

fn main() {
    println!("{}", sum(1, 2, 3))
}
```

## DZ0555

A project can turn reflection off with `type_info = "none"` under `[build]`, which leaves no
type table in the program; a call to `typeinfo_of` is then an error (spec §13.2).

No example: it takes a `dazzle.toml` with reflection turned off.

## DZ0556

A function needs a body, except a trait's required method, a `virtual fn` that makes its type
abstract, and the standard library's `extern fn`s, whose bodies the compiler supplies
(spec §7.2).

```dz
fn ticks() -> u64

fn main() {
    println!("{}", ticks())
}
```

```dz
fn ticks() -> u64 {
    return 0
}

fn main() {
    println!("{}", ticks())
}
```

## DZ0557

A method with type parameters of its own is one function per set of arguments. A `virtual`
method has one slot in a vtable, and a trait's method is called through `^Trait` with no
types known, so neither can have them (spec §8.1, §7.6). Make the type or trait generic
instead.

```dz
struct Widget {
    pub virtual fn notify<T>(&self, event: T) {}
}

fn main() {}
```

```dz
struct Widget {
    pub virtual fn notify(&self, event: i32) {}
}

fn main() {}
```

## DZ0558

The parameters after a variadic one are passed by name, `scale = 3`, since a positional
argument there would be taken as one more variadic one (spec §7.2). Only they are passed by
name, each once, after the other arguments.

```dz
fn sum(xs: ..i32, scale: i32 = 1) -> i32 {
    return xs.len as i32 * scale
}

fn main() {
    println!("{}", sum(1, 2, scael = 3))
}
```

```dz
fn sum(xs: ..i32, scale: i32 = 1) -> i32 {
    return xs.len as i32 * scale
}

fn main() {
    println!("{}", sum(1, 2, scale = 3))
}
```

## DZ0559

Every union has a `kind()` method, which says which variant it holds (spec §7.5), so a
method of its own cannot take that name.

```dz
union Shape {
    Circle { r: f32 }
    Point {}

    pub fn kind(&self) -> i32 {
        return 0
    }
}

fn main() {}
```

```dz
union Shape {
    Circle { r: f32 }
    Point {}

    pub fn sides(&self) -> i32 {
        return 0
    }
}

fn main() {}
```

## DZ0560

An associated type is named off a generic parameter through one of its bounds: `T::Item`
exists because `T: Iterate` and `Iterate` declares `type Item` (spec §8.5). A name no bound
declares is not a type, and an unbounded parameter has no associated types at all.

```dz
fn first<view T: Iterate>(it: &T) -> T::Element {
    return it.get()
}

fn main() {}
```

```dz
fn first<view T: Iterate>(it: &T) -> T::Item {
    return it.get()
}

fn main() {
    let mut xs = List<i32>()
    xs.push(7)
    println!("{}", first(&xs.iter()))
}
```

## DZ0561

Each implementation of a trait chooses its own associated types (spec §8.5, §18.29), so a
parameter bounded by one trait at two arguments has one `Output` for each. `T::Output` does
not say which it means. Bound the parameter by the one trait whose associated type you use.

```dz
fn scaled<T: Mul<f32> + Mul<i32> + Copy>(v: T) -> T::Output {
    return v * 2.0
}

fn main() {}
```

```dz
fn scaled<T: Mul<f32> + Copy>(v: T) -> T::Output {
    return v * 2.0
}

fn main() {
    let x: f32 = 1.5
    println!("{}", scaled(x))
}
```
