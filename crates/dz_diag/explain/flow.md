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

## DZ0413

A reference, pointer or view points into some memory, its tie, and stays valid only while
that memory does (spec §6.7). A call that can grow, shrink or free it — `push` on the list a
reference points into, whose `kills` clause says so — ends every reference into it, and so
does writing, moving or dropping what owns it. Using one afterwards would read memory that
has moved. Use it before, or get it again after. Returning a view of a local, or of a
parameter taken by value, is the same mistake: it dies as the function returns. So is a
loop that grows the container it walks: each element, and the loop's own next pass, points
into memory the call moved; `break` right after the call, or make the change once the loop
is over.

```dz
fn main() {
    let mut scores = List<i32>()
    scores.push(10)
    let first = scores.first() or return
    scores.push(20)
    println!("{}", first)
}
```

```dz
fn main() {
    let mut scores = List<i32>()
    scores.push(10)
    scores.push(20)
    let first = scores.first() or return
    println!("{}", first)
}
```

## DZ0414

A function returning a reference says where it points with `in`, and a caller keeps the
result only as long as that memory lives (spec §6.7). Returning a view of something else
breaks that promise: the caller would keep it past a kill it was never told about.

```dz
pub struct Team {
    pub names: List<String>
    pub coach: String
}

fn first_name(t: &Team) -> str in t.names[*] {
    return t.coach.as_str()
}

fn main() {
    let t = Team{ names: List<String>(), coach: String::from("Kim") }
    println!("{}", first_name(&t))
}
```

```dz
pub struct Team {
    pub names: List<String>
    pub coach: String
}

fn coach_name(t: &Team) -> str in t.coach {
    return t.coach.as_str()
}

fn main() {
    let t = Team{ names: List<String>(), coach: String::from("Kim") }
    println!("{}", coach_name(&t))
}
```

## DZ0415

Every function says in its signature what it may kill, so a caller knows which of its
references survive the call without reading the body (spec §6.7). A body that kills what
its `kills` clause leaves out is an error where it kills; `dz fix` writes the clause the
body needs.

```dz
fn fill(xs: &mut List<i32>) {
    xs.push(1)
}

fn main() {
    let mut xs = List<i32>()
    fill(&mut xs)
    println!("{}", xs.len())
}
```

```dz
fn fill(xs: &mut List<i32>) kills xs@items {
    xs.push(1)
}

fn main() {
    let mut xs = List<i32>()
    fill(&mut xs)
    println!("{}", xs.len())
}
```

## DZ0416

A `kills` clause says exactly what the body kills (spec §6.7). A path the body never kills
would end a caller's references for nothing, so it is an error; `dz fix` removes it.

```dz
fn count(xs: &mut List<i32>) -> usize kills xs@items {
    return xs.len()
}

fn main() {
    let mut xs = List<i32>()
    println!("{}", count(&mut xs))
}
```

```dz
fn count(xs: &mut List<i32>) -> usize {
    return xs.len()
}

fn main() {
    let mut xs = List<i32>()
    println!("{}", count(&mut xs))
}
```

## DZ0417

A `kills` clause names exactly what dies, no wider (spec §6.7). `kills xs` says everything
the list owns may move, its array and each element's own memory; `push` only moves the
array, so a caller's view into an element's text survives it, and the clause has to say
`kills xs@items` for the caller to keep it. `dz fix` narrows the clause.

```dz
fn add(xs: &mut List<String>) kills xs {
    xs.push(String::from("x"))
}

fn main() {
    let mut xs = List<String>()
    add(&mut xs)
    println!("{}", xs.len())
}
```

```dz
fn add(xs: &mut List<String>) kills xs@items {
    xs.push(String::from("x"))
}

fn main() {
    let mut xs = List<String>()
    add(&mut xs)
    println!("{}", xs.len())
}
```
