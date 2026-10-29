# Type checking, DZ05xx

## DZ0501

A value of one type where another is wanted. Dazzle converts nothing implicitly — not
`i32` to `i64`, not `f32` to `f64`, not a number to `bool` (spec §5.1) — so a conversion is
written with `as`.

```dz
fn total(a: i32) -> i64 {
    return a
}

fn main() {
    println!("{}", total(3))
}
```

```dz
fn total(a: i32) -> i64 {
    return a as i64
}

fn main() {
    println!("{}", total(3))
}
```

## DZ0502

A literal takes its type from where it is used, and must fit that type (spec §2.5): `300`
cannot be a `u8`, nor `-1` a `u32`.

```dz
fn main() {
    let level: u8 = 300
    println!("{}", level)
}
```

```dz
fn main() {
    let level: u16 = 300
    println!("{}", level)
}
```

## DZ0503

Only a function — a declared one, or a value of function type — can be called with `(…)`.

```dz
fn main() {
    let speed = 4
    let x = speed(2)
}
```

```dz
fn main() {
    let speed = 4
    let x = speed * 2
    println!("{}", x)
}
```

## DZ0504

Every parameter without a default needs an argument, and no more arguments than parameters
may be given (spec §7.2).

```dz
fn area(w: f32, h: f32) -> f32 {
    return w * h
}

fn main() {
    println!("{}", area(2.0))
}
```

```dz
fn area(w: f32, h: f32 = 1.0) -> f32 {
    return w * h
}

fn main() {
    println!("{}", area(2.0))
}
```

## DZ0505

The type has no field, variant or member of that name. The help gives the closest one, and
an editor offers it as a fix.

```dz
struct Player {
    pub health: i32
}

fn main() {
    let p = Player{ health: 10 }
    println!("{}", p.helth)
}
```

```dz
struct Player {
    pub health: i32
}

fn main() {
    let p = Player{ health: 10 }
    println!("{}", p.health)
}
```

## DZ0506

A field or method without `pub` belongs to its type: only the type's own methods reach it
(spec §4). From outside, use a `pub` method — which is what keeps a type's invariant its
own.

```dz
struct Counter {
    count: u32 = 0

    pub fn get(&self) -> u32 {
        return count
    }
}

fn main() {
    let c = Counter{}
    println!("{}", c.count)
}
```

```dz
struct Counter {
    count: u32 = 0

    pub fn get(&self) -> u32 {
        return count
    }
}

fn main() {
    let c = Counter{}
    println!("{}", c.get())
}
```

## DZ0507

A `let` binding, a parameter and a `&T` are read-only. Write `let mut` for a binding that
changes, `&mut T` for a reference written through, and call a `&mut self` method on
something mutable.

```dz
fn main() {
    let total = 1
    total = 2
    println!("{}", total)
}
```

```dz
fn main() {
    let mut total = 1
    total = 2
    println!("{}", total)
}
```

## DZ0508

A condition is a `bool`; nothing else is true or false (spec §10.1). Compare explicitly.

```dz
fn main() {
    let lives = 3
    if lives {
        println!("alive")
    }
}
```

```dz
fn main() {
    let lives = 3
    if lives > 0 {
        println!("alive")
    }
}
```

## DZ0510

A fallible call — one whose function returns `!->` — can fail, and the failure must go
somewhere in the statement that makes the call (spec §9.7): handled with `or`, propagated
with `or return`, or deliberately dropped with `or ignore`.

```dz
fn save() !-> i32 {
    return 1
}

fn main() {
    save()
}
```

```dz
fn save() !-> i32 {
    return 1
}

fn main() {
    save() or ignore
}
```

## DZ0511

`as` converts between numbers, between enums and their integers, and among pointers inside
`unsafe` — not between unrelated types, such as a struct and a number.

```dz
struct Vec2 {
    pub x: f32
    pub y: f32
}

fn main() {
    let v = Vec2{ x: 1.0, y: 2.0 }
    let n = v as i32
}
```

```dz
struct Vec2 {
    pub x: f32
    pub y: f32
}

fn main() {
    let v = Vec2{ x: 1.0, y: 2.0 }
    let n = v.x as i32
    println!("{}", n)
}
```

## DZ0512

A struct literal gives every field that has no default (spec §6.2). Give the missing ones,
or give the field a default in the declaration.

```dz
struct Vec3 {
    pub x: f32
    pub y: f32
    pub z: f32
}

fn main() {
    let v = Vec3{ x: 1.0, y: 2.0 }
    println!("{}", v.x)
}
```

```dz
struct Vec3 {
    pub x: f32
    pub y: f32
    pub z: f32 = 0.0
}

fn main() {
    let v = Vec3{ x: 1.0, y: 2.0 }
    println!("{}", v.x)
}
```

## DZ0513

A `switch` on an enum or union covers every variant, or has a `default` (spec §10.3), so
adding a variant later finds every place that has to handle it. An editor and `dz check
--format=json` carry the missing arms as a fix.

```dz
enum Dir {
    Up
    Down
    Left
}

fn main() {
    let d = Dir::Up
    switch d {
        case Up {
            println!("up")
        }
        case Down {
            println!("down")
        }
    }
}
```

```dz
enum Dir {
    Up
    Down
    Left
}

fn main() {
    let d = Dir::Up
    switch d {
        case Up {
            println!("up")
        }
        case Down {
            println!("down")
        }
        case Left {
            println!("left")
        }
    }
}
```

## DZ0514

The operator does not apply to these operands. Arithmetic needs two numbers of one type —
`i32 + i64` is refused, as there is no implicit widening — and on a struct an operator is the
trait that implements it, `Add` for `+` (spec §11.2).

```dz
fn main() {
    let a: i32 = 1
    let b: i64 = 2
    println!("{}", a + b)
}
```

```dz
fn main() {
    let a: i32 = 1
    let b: i64 = 2
    println!("{}", a as i64 + b)
}
```

## DZ0515

`[i]` indexes an array, a slice, a `List`, or a type that implements `Index`.

```dz
fn main() {
    let n = 5
    println!("{}", n[0])
}
```

```dz
fn main() {
    let ns = [5, 6]
    println!("{}", ns[0])
}
```

## DZ0516

`or return` hands a failure to the caller, so the function must be able to give one back:
its signature needs `!->` (spec §9.5).

```dz
fn load() !-> i32 {
    return 1
}

fn start() -> i32 {
    let n = load() or return
    return n
}

fn main() {
    println!("{}", start())
}
```

```dz
fn load() !-> i32 {
    return 1
}

fn start() !-> i32 {
    let n = load() or return
    return n
}

fn main() {
    println!("{}", start() or 0)
}
```

## DZ0517

A struct cannot hold a `&T` (spec §5.2, §18.3): a reference is a borrow for the length of a
call, and a field would outlive it. Hold the value, a handle such as `Rc<T>`, or — where the
lifetime is managed by hand — a `^T`.

```dz
struct Camera {
    target: &i32
}

fn main() {}
```

```dz
struct Camera {
    target: ^i32
}

fn main() {}
```

## DZ0518

`p^` reads what a pointer points at; it applies to a `^T` only. A value is already itself,
and a `&T` is read through without `^`.

```dz
fn main() {
    let n = 5
    println!("{}", n^)
}
```

```dz
fn main() {
    let n = 5
    let p: ^i32 = &n
    unsafe {
        println!("{}", p^)
    }
}
```

## DZ0519

A struct declares each field once, and a literal sets each field once.

```dz
struct Size {
    pub w: i32
    pub w: i32
}

fn main() {}
```

```dz
struct Size {
    pub w: i32
    pub h: i32
}

fn main() {}
```

## DZ0520

Neither the type nor any trait it implements has a method of that name. The help names the
closest, or the trait that has it if that trait is implemented for another type.

```dz
struct Counter {
    pub n: i32

    pub fn bump(&mut self) {
        n += 1
    }
}

fn main() {
    let mut c = Counter{ n: 0 }
    c.increment()
}
```

```dz
struct Counter {
    pub n: i32

    pub fn bump(&mut self) {
        n += 1
    }
}

fn main() {
    let mut c = Counter{ n: 0 }
    c.bump()
    println!("{}", c.n)
}
```

## DZ0521

`none` is the absent value of some `T?`, and `error(…)` the failure of some `!->`
function; on its own neither says which type it is (spec §9.1). Give the binding a type.

```dz
fn main() {
    let target = none
}
```

```dz
fn main() {
    let target: i32? = none
    println!("{}", target or 0)
}
```

## DZ0522

A `^T` may be null and a `&T` may not, so a pointer becomes a reference only through a check
(spec §5.2): `p or return`, `p or { … }`, or `if let &v = p`.

```dz
fn show(n: &i32) {
    println!("{}", n)
}

fn main() {
    let n = 3
    let p: ^i32 = &n
    show(p)
}
```

```dz
fn show(n: &i32) {
    println!("{}", n)
}

fn main() {
    let n = 3
    let p: ^i32 = &n
    if let &v = p {
        show(&v)
    }
}
```

## DZ0523

An `or` clause stands in for the value when it is missing, so it must produce a value of the
same type — or leave, with `return`, `break`, `continue` or a panic (spec §9.4).

```dz
fn find() -> i32? {
    return none
}

fn main() {
    let n = find() or "nothing"
    println!("{}", n)
}
```

```dz
fn find() -> i32? {
    return none
}

fn main() {
    let n = find() or 0
    println!("{}", n)
}
```

## DZ0524

`or ignore` drops a failure and produces nothing, so there is no value to bind. Give a
fallback instead, `or 0`, or call it as a statement of its own.

```dz
fn load() !-> i32 {
    return 1
}

fn main() {
    let n = load() or ignore
}
```

```dz
fn load() !-> i32 {
    return 1
}

fn main() {
    let n = load() or 0
    println!("{}", n)
}
```

## DZ0525

`or` handles a value that may be missing — a `T?`, a `^T`, a handle — or a call that may
fail. A plain value is always there.

```dz
fn main() {
    let n = 4
    println!("{}", n or 0)
}
```

```dz
fn main() {
    let n: i32? = 4
    println!("{}", n or 0)
}
```

## DZ0526

A format string is checked where it is written (spec §12.9): it must be a literal, with one
`{}` per argument, specs that suit their arguments, and balanced braces — write `{{` and
`}}` for a literal brace.

```dz
fn main() {
    println!("{} and {}", 1)
}
```

```dz
fn main() {
    println!("{} and {}", 1, 2)
}
```

## DZ0527

A formatting macro writes any value whose type implements `Format` — numbers, `bool`,
`char`, `str`, `String`, and types of your own that implement it (spec §12.9).

```dz
struct Point {
    pub x: i32
}

fn main() {
    let p = Point{ x: 1 }
    println!("{}", p)
}
```

```dz
struct Point {
    pub x: i32
}

impl Format for Point {
    fn format(&self, f: &mut Formatter) {
        f.write_str("Point(")
        x.format(f)
        f.write_str(")")
    }
}

fn main() {
    let p = Point{ x: 1 }
    println!("{}", p)
}
```

## DZ0528

Reading or writing through a `^T`, indexing one, casting between pointers, the placement
primitives and calling an `unsafe fn` are raw-memory operations: nothing checks that the
memory is still there, so they go inside `unsafe { … }` (spec §5.2). Forming, copying,
storing and comparing a pointer are all safe.

```dz
fn main() {
    let n = 3
    let p: ^i32 = &n
    println!("{}", p^)
}
```

```dz
fn main() {
    let n = 3
    let p: ^i32 = &n
    unsafe {
        println!("{}", p^)
    }
}
```

## DZ0529

`if let` takes a `T?`, a `^T` or a handle. A fallible call's failure carries an `Error`,
which an `if let`'s `else` would silently drop; handle it with `or` (spec §9.4, §9.5).

```dz
fn load() !-> i32 {
    return 1
}

fn main() {
    if let n = load() {
        println!("{}", n)
    }
}
```

```dz
fn load() !-> i32 {
    return 1
}

fn main() {
    let n = load() or {
        println!("failed")
        return
    }
    println!("{}", n)
}
```

## DZ0562

A tie path names memory by starting at one of the function's parameters, or `self` in a
method, and going through fields, `[*]` for any element and `@name` for a tie the type
declares (spec §6.7). Each step has to exist: a misspelt parameter or field names nothing.

```dz
pub struct Party {
    pub members: List<String>
}

fn recruit(party: &mut Party) kills party.member {
    party.members.push(String::from("Ann"))
}

fn main() {
    let mut p = Party{ members: List<String>() }
    recruit(&mut p)
}
```

```dz
pub struct Party {
    pub members: List<String>
}

fn recruit(party: &mut Party) kills party.members@items {
    party.members.push(String::from("Ann"))
}

fn main() {
    let mut p = Party{ members: List<String>() }
    recruit(&mut p)
    println!("{}", p.members.len())
}
```

## DZ0563

A `kills` clause is part of the signature, so everyone who may call the function reads it,
and it can only name what they can see (spec §6.7). A `pub` method that names a private
field would tell its callers about something they cannot name themselves. Have the struct
declare a tie for the field instead: the tie is a public name for that private memory.

```dz
pub struct Party {
    m_members: List<String> = List<String>()

    pub fn recruit(&mut self, name: str) kills self.m_members {
        m_members.push(String::from(name))
    }
}

fn main() {
    let mut p = Party{}
    p.recruit("Ann")
}
```

```dz
pub struct Party {
    m_members: List<String> = List<String>()
    tie @members = m_members@items

    pub fn recruit(&mut self, name: str) kills self@members {
        m_members.push(String::from(name))
    }
}

fn main() {
    let mut p = Party{}
    p.recruit("Ann")
}
```

## DZ0564

A function can only kill memory its caller lent it to change: through a `&mut` or `^mut`
parameter, or `&mut self` (spec §6.7). Through a `&` it cannot change anything, and what it
takes by value is its own, so nothing the caller holds can point into it.

```dz
pub struct Party {
    pub members: List<String>
}

fn count(party: &Party) -> usize kills party.members {
    return party.members.len()
}

fn main() {
    let p = Party{ members: List<String>() }
    println!("{}", count(&p))
}
```

```dz
pub struct Party {
    pub members: List<String>
}

fn count(party: &Party) -> usize {
    return party.members.len()
}

fn main() {
    let p = Party{ members: List<String>() }
    println!("{}", count(&p))
}
```

## DZ0565

`in` says where a reference points: into a parameter, or into `self` (spec §6.7). A
number, a `bool` or a value returned whole points into nothing, so there is nothing to say,
and a parameter cannot point into itself.

```dz
pub struct Party {
    pub members: List<String>
}

fn count(party: &Party) -> usize in party {
    return party.members.len()
}

fn main() {
    let p = Party{ members: List<String>() }
    println!("{}", count(&p))
}
```

```dz
pub struct Party {
    pub members: List<String>
}

fn leader(party: &Party) -> ^String in party.members {
    return party.members.first()
}

fn main() {
    let p = Party{ members: List<String>() }
    println!("{}", leader(&p) == null)
}
```

## DZ0567

A struct declares each of its ties once, listing everything the tie stands for (spec
§6.7). A second `tie @members` would leave it unclear whether the tie means the first
list, the second, or both; one declaration with both says they live and die together.

```dz
pub struct Party {
    m_members: List<String> = List<String>()
    m_guests: List<String> = List<String>()
    tie @members = m_members@items
    tie @members = m_guests

    pub fn recruit(&mut self, name: str) kills self@members {
        m_members.push(String::from(name))
    }
}

fn main() {
    let mut p = Party{}
    p.recruit("Ann")
}
```

```dz
pub struct Party {
    m_members: List<String> = List<String>()
    m_guests: List<String> = List<String>()
    tie @members = m_members@items, m_guests@items

    pub fn recruit(&mut self, name: str) kills self@members {
        m_members.push(String::from(name))
    }
}

fn main() {
    let mut p = Party{}
    p.recruit("Ann")
}
```

## DZ0568

A function type's `kills` clause is the most a function stored in it may kill, and a call
through a value of the type kills only that (spec §6.7). A function that kills more does not
fit: a call through the value would end references the caller was told survive. A function
type with no clause promises to kill nothing.

```dz
pub struct Party {
    pub members: List<String>
}

fn recruit(p: &mut Party) kills p.members@items {
    p.members.push(String::from("Ann"))
}

fn twice(f: fn(p: &mut Party), p: &mut Party) {
    f(p)
    f(p)
}

fn main() {
    let mut p = Party{ members: List<String>() }
    twice(recruit, &mut p)
}
```

```dz
pub struct Party {
    pub members: List<String>
}

fn recruit(p: &mut Party) kills p.members@items {
    p.members.push(String::from("Ann"))
}

fn twice(f: fn(p: &mut Party) kills p.members@items, p: &mut Party) kills p.members@items {
    f(p)
    f(p)
}

fn main() {
    let mut p = Party{ members: List<String>() }
    twice(recruit, &mut p)
    println!("{}", p.members.len())
}
```
