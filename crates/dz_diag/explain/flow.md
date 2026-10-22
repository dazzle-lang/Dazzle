# Ownership and flow, DZ04xx

A value that owns something — a `String`, a `List`, a struct with a `drop` — has one owner.
`move(x)` hands it on and leaves `x` at its defaults; nothing is copied unless the type is
`Copy` (spec §6).

## DZ0401

After `move(x)`, `x` holds nothing — it is back at its defaults — so reading it is almost
certainly a mistake. The analysis follows every path: a value moved on one branch is moved
where the branches meet. The second label points at the move. Use the value before moving
it, or move a copy of what you need.

```dz
fn main() {
    let name = String::from("hero")
    let kept = move(name)
    println!("{} {}", kept, name)
}
```

```dz
fn main() {
    let name = String::from("hero")
    println!("{}", name)
    let kept = move(name)
    println!("{}", kept)
}
```

## DZ0402

`move` resets its source to its defaults, so it needs a place the function owns. Behind a
`&` or a `^` the value belongs to someone else, who would find it reset. Copy a copyable
value instead, or take ownership by value.

```dz
struct Point {
    pub x: i32
}

fn steal(p: &Point) -> i32 {
    return move(p.x)
}

fn main() {
    let p = Point{ x: 3 }
    println!("{}", steal(&p))
}
```

```dz
struct Point {
    pub x: i32
}

fn read(p: &Point) -> i32 {
    return p.x
}

fn main() {
    let p = Point{ x: 3 }
    println!("{}", read(&p))
}
```

## DZ0403

A move-only value — one that owns something — is never copied by accident (spec §6.3): to
hand it to a binding, a parameter or a field, write `move(…)`, which says the old place
gives it up. A type that should be copyable implements `Copy`.

```dz
struct Ticket {
    pub id: i32
}

impl Drop for Ticket {
    fn drop(&mut self) {
        println!("ticket {} released", id)
    }
}

fn redeem(t: Ticket) {
    println!("{}", t.id)
}

fn main() {
    let t = Ticket{ id: 7 }
    redeem(t)
}
```

```dz
struct Ticket {
    pub id: i32
}

impl Drop for Ticket {
    fn drop(&mut self) {
        println!("ticket {} released", id)
    }
}

fn redeem(t: Ticket) {
    println!("{}", t.id)
}

fn main() {
    let t = Ticket{ id: 7 }
    redeem(move(t))
}
```

## DZ0404

Moves are tracked per field, so `move(h.a)` leaves `h.b` usable — but `h` as a whole is
then missing a part, and cannot be read, passed or moved as one value.

```dz
struct Pair {
    pub a: String
    pub b: String
}

fn keep(p: Pair) {
    println!("{}", p.b)
}

fn main() {
    let p = Pair{ a: String::from("a"), b: String::from("b") }
    let first = move(p.a)
    keep(move(p))
    println!("{}", first)
}
```

```dz
struct Pair {
    pub a: String
    pub b: String
}

fn keep(p: Pair) {
    println!("{}", p.b)
}

fn main() {
    let p = Pair{ a: String::from("a"), b: String::from("b") }
    let second = move(p.b)
    let first = move(p.a)
    println!("{} {}", first, second)
}
```

## DZ0405

A function that returns a value must return one on every path. The label is on the end of
the function a path can reach without a `return`.

```dz
fn sign(n: i32) -> i32 {
    if n > 0 {
        return 1
    }
    if n < 0 {
        return -1
    }
}

fn main() {
    println!("{}", sign(3))
}
```

```dz
fn sign(n: i32) -> i32 {
    if n > 0 {
        return 1
    }
    if n < 0 {
        return -1
    }
    return 0
}

fn main() {
    println!("{}", sign(3))
}
```

## DZ0406

A warning for a local or parameter that nothing reads, or one assigned and never read
after. A leading `_` says it is unused on purpose; `dz fix` adds one.

```dz
fn main() {
    let spare = 3
    println!("{}", 1)
}
```

```dz
fn main() {
    let _spare = 3
    println!("{}", 1)
}
```

## DZ0407

A temporary — a value built in the middle of a statement and not bound to a name — is
dropped when the statement ends (spec §6.5). A view taken from one — a `str`, `cstr`, slice,
reference or pointer — may be passed to a call in that same statement, but keeping it in a
`let` would leave it pointing at freed memory. Bind the owner first.

```dz
fn text_len(c: cstr) -> usize {
    return (c as str).len
}

fn main() {
    let name: str = "hero"
    let c = String::from(name).as_cstr()
    println!("{}", text_len(c))
}
```

```dz
fn text_len(c: cstr) -> usize {
    return (c as str).len
}

fn main() {
    let name: str = "hero"
    let owned = String::from(name)
    let c = owned.as_cstr()
    println!("{}", text_len(c))
}
```

## DZ0411

A struct has one destructor and one copy: two `impl Drop` or two `impl Copy` for it would
leave the compiler to choose.

```dz
struct Handle {
    pub id: i32
}

impl Drop for Handle {
    fn drop(&mut self) {}
}

impl Drop for Handle {
    fn drop(&mut self) {}
}

fn main() {
    let h = Handle{ id: 1 }
    println!("{}", h.id)
}
```

```dz
struct Handle {
    pub id: i32
}

impl Drop for Handle {
    fn drop(&mut self) {}
}

fn main() {
    let h = Handle{ id: 1 }
    println!("{}", h.id)
}
```

## DZ0412

An `own` field declares that the struct is responsible for releasing what it points at
(spec §6.5, §18.4), so the struct needs an `impl Drop` that does.

```dz
struct Buffer {
    m_data: own ^mut u8 = null
    pub len: usize

    Buffer(len: usize) {
        self.len = len
    }
}

fn main() {
    let b = Buffer(0)
    println!("{}", b.len)
}
```

```dz
struct Buffer {
    m_data: own ^mut u8 = null
    pub len: usize

    Buffer(len: usize) {
        self.len = len
    }
}

impl Drop for Buffer {
    fn drop(&mut self) {
        Allocators::Global.free(m_data, len, 1)
    }
}

fn main() {
    let b = Buffer(0)
    println!("{}", b.len)
}
```
