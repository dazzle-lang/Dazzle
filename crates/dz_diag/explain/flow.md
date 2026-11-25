# Ownership and flow, DZ04xx

A value that owns something — a `String`, a `List`, a struct with a `drop` — has one owner.
Using it by value — `let b = a`, passing it, returning it, storing it — hands it on and
leaves `a` reset; nothing is copied unless the copy is written, `a.clone()`. Plain
data, such as an `i32` or a `Vec3`, copies by itself (spec §6, D-0106).

## DZ0401

After a value moves, its old place holds nothing — it has been reset — so reading
it is almost certainly a mistake. The analysis follows every path: a value moved on one
branch is moved where the branches meet. The second label points at the move. Use the value
before it moves, or move a copy of it: where nothing wrote the move, the fix writes
`.clone()` there.

```dz
fn main() {
    let name = String("hero")
    let kept = name
    println!("{} {}", kept, name)
}
```

```dz
fn main() {
    let name = String("hero")
    let kept = name.clone()
    println!("{} {}", kept, name)
}
```

## DZ0402

A move resets its source to its defaults, so it needs a place the function owns. Behind a
`&` or a `^`, in a `static` or in an element of a container, the value belongs to someone
else, who would find it reset. Reading a value that is not plain data there by value is
such a move; write `.clone()` for a copy, which the fix does, or take ownership by value.

```dz
struct Named {
    pub name: String
}

fn steal(n: &Named) -> String {
    return n.name
}

fn main() {
    let n = Named{ name: String("hero") }
    println!("{}", steal(&n))
}
```

```dz
struct Named {
    pub name: String
}

fn copied(n: &Named) -> String {
    return n.name.clone()
}

fn main() {
    let n = Named{ name: String("hero") }
    println!("{}", copied(&n))
}
```

## DZ0403

A value that owns something is copied only by `a.clone()`, and only when its type is
`Clone` (spec §6.3, D-0106): plain data is `Clone` by its shape, and a type that owns
something by an `impl Clone`. One that is neither — a type with a `drop` and no `clone`, or a
`T` with no `Clone` bound — cannot be copied at all, so asking for a copy of it is this
error. Hand the value on instead, which moves it, or give the type an `impl Clone`. A `for`,
`if let` or `case` binding that would copy such a value is the same mistake: refer to it
with `&e`.

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
    redeem(t.clone())
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
    redeem(t)
}
```

## DZ0404

Moves are tracked per field, so moving `h.a` out leaves `h.b` usable — but `h` as a whole is
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
    let p = Pair{ a: String("a"), b: String("b") }
    let first = p.a
    keep(p)
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
    let p = Pair{ a: String("a"), b: String("b") }
    let second = p.b
    let first = p.a
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
fn text_len(c: &cstr) -> usize {
    return (c as &str).byte_len
}

fn main() {
    let name: &str = "hero"
    let c = String(name).as_cstr()
    println!("{}", text_len(c))
}
```

```dz
fn text_len(c: &cstr) -> usize {
    return (c as &str).byte_len
}

fn main() {
    let name: &str = "hero"
    let owned = String(name)
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
struct Window {
    m_handle: own ^mut u8 = null    // what a C library's create_window returned
    pub width: i32

    pub Window(width: i32) {
        self.width = width
    }
}

fn main() {
    let w = Window(640)
    println!("{}", w.width)
}
```

```dz
struct Window {
    m_handle: own ^mut u8 = null    // what a C library's create_window returned
    pub width: i32

    pub Window(width: i32) {
        self.width = width
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        destroy_window(m_handle)
    }
}

// Stands in for the C library's own, which a real program imports from its header.
fn destroy_window(handle: ^mut u8) {
    let _ = handle
}

fn main() {
    let w = Window(640)
    println!("{}", w.width)
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

fn first_name(t: &Team) -> &str in t.names[*] {
    return t.coach.as_str()
}

fn main() {
    let t = Team{ names: List<String>(), coach: String("Kim") }
    println!("{}", first_name(&t))
}
```

```dz
pub struct Team {
    pub names: List<String>
    pub coach: String
}

fn coach_name(t: &Team) -> &str in t.coach {
    return t.coach.as_str()
}

fn main() {
    let t = Team{ names: List<String>(), coach: String("Kim") }
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
    xs.push(String("x"))
}

fn main() {
    let mut xs = List<String>()
    add(&mut xs)
    println!("{}", xs.len())
}
```

```dz
fn add(xs: &mut List<String>) kills xs@items {
    xs.push(String("x"))
}

fn main() {
    let mut xs = List<String>()
    add(&mut xs)
    println!("{}", xs.len())
}
```

## DZ0418

`or` turns a pointer into a reference safely only when the pointer has a tie: it was made
from a place (`&mut world.players[0]`), or a safe call returned it with `in`, or elided to its
one view input (spec 5.2, 6.7). A pointer parameter, a pointer field with no tie, a cast or
C's pointer has none: it may point at memory already gone, aliasing a reference the checker
sees, and nothing here can tell. The conversion is then the programmer's word, written in
`unsafe { … }`, which an editor offers to do.

```dz
pub struct World {
    pub hp: i32 = 0
}

fn hurt(p: ^mut World) {
    let w = p or return
    w.hp -= 1
}

fn main() {
    let mut world = World{}
    hurt(&mut world)
    println!("{}", world.hp)
}
```

```dz
pub struct World {
    pub hp: i32 = 0
}

fn hurt(p: ^mut World) {
    let w = unsafe { p or return }
    w.hp -= 1
}

fn main() {
    let mut world = World{}
    hurt(&mut world)
    println!("{}", world.hp)
}
```

## DZ0419

A function's result tie is a promise its callers rely on: they turn a pointer it returns
into a reference safely (`DZ0418` does not ask them for `unsafe`), because the body is held
to the tie. Returning a pointer whose tie is unknown — read from a field with no tie, or
cast — under that promise would launder an untied pointer into a tied one (spec 6.7). The
body vouches for it in `unsafe { … }` instead, or returns what its `in` names.

```dz
pub struct World {
    pub hp: i32 = 0
}

pub struct Ctx {
    pub world: ^mut World = null
}

fn world_of(ctx: &Ctx) -> ^mut World in ctx {
    return ctx.world
}

fn main() {
    let mut world = World{}
    let ctx = Ctx{ world: &mut world }
    let w = world_of(&ctx) or return
    println!("{}", w.hp)
}
```

```dz
pub struct World {
    pub hp: i32 = 0
}

pub struct Ctx {
    pub world: ^mut World = null
}

fn world_of(ctx: &Ctx) -> ^mut World in ctx {
    return unsafe { ctx.world }
}

fn main() {
    let mut world = World{}
    let ctx = Ctx{ world: &mut world }
    let w = world_of(&ctx) or return
    println!("{}", w.hp)
}
```

## DZ0420

A function is checked once, for every caller, with its parameters taken as separate places.
Inside `append_copy` below, `dst` and `src` are two lists, so growing `dst` cannot touch what
`src` walks. A caller passing one list as both breaks that: the first `push` moves the array
`src` is walking, and the rest of the copies are read from freed memory (spec 6.7). So at a
call, memory the clause kills may be reached through the argument the clause names it by,
and no other: not by one pointing at what owns it, as `&ys` does, nor by a second argument
naming the same kill. Two arguments may still share memory the call does not kill, or
memory that cannot die, such as an `i32` field.

```dz
fn append_copy(dst: &mut List<String>, src: &List<String>) kills dst@items {
    for &s in src {
        dst.push(String(s.as_str()))
    }
}

fn main() {
    let mut ys = List<String>()
    ys.push(String("a"))
    append_copy(&mut ys, &ys)
    println!("{}", ys.len())
}
```

```dz
fn append_copy(dst: &mut List<String>, src: &List<String>) kills dst@items {
    for &s in src {
        dst.push(String(s.as_str()))
    }
}

fn main() {
    let mut ys = List<String>()
    ys.push(String("a"))
    let copy = ys.clone()
    append_copy(&mut ys, &copy)
    println!("{}", ys.len())
}
```

## DZ0421

The compiler places each value's memory, in a region or on the heap, and a caller places
what it lends by the signatures of the functions it lends it to (D-0097). So a function
that hands what one input owns to another says so, `sends from[*] to store`, and one that
returns what an input owns says the result lives in that input's home, `-> T in xs@home`.
A body that does either without saying so could leave the memory where the caller put it,
while the value now lives somewhere that outlasts it. `dz fix` writes the clause.

```dz
struct Store {
    pub names: List<String>
}

fn give(from: &mut List<String>, store: &mut Store) kills from@items, from[*], store.names@items {
    let s = from.pop() or return
    store.names.push(s)
}

fn main() {
    let mut store = Store{ names: List<String>() }
    let mut names = List<String>()
    names.push(String::from("one"))
    give(&mut names, &mut store)
}
```

```dz
struct Store {
    pub names: List<String>
}

fn give(from: &mut List<String>, store: &mut Store) kills from@items, from[*], store.names@items sends from[*] to store {
    let s = from.pop() or return
    store.names.push(s)
}

fn main() {
    let mut store = Store{ names: List<String>() }
    let mut names = List<String>()
    names.push(String::from("one"))
    give(&mut names, &mut store)
}
```

## DZ0422

A `sends` clause, and a result's `in x@home`, say exactly where memory goes, as a `kills`
clause says exactly what dies (D-0097). One the body never does would make every caller
place memory further out than it goes: a list it lends kept off its region for nothing, a
result kept where an input lives though it is built fresh. Remove what the body does not
do; `dz fix` writes the clause it needs.

```dz
fn label(xs: &mut List<String>) -> String in xs@home {
    return format!("{} names", xs.len())
}

fn main() {
    let mut names = List<String>()
    println!("{}", label(&mut names))
}
```

```dz
fn label(xs: &mut List<String>) -> String {
    return format!("{} names", xs.len())
}

fn main() {
    let mut names = List<String>()
    println!("{}", label(&mut names))
}
```

## DZ0423

`move(x)` was how a value was moved; it is not written any more (D-0106). A value that is
not plain data moves wherever it is used by value — bound, passed, returned, stored — and
plain data copies, so `move(x)` is `x`, and a consuming loop is `for r in rs`. A copy of a
value that owns something is written `x.clone()`. `dz fix` removes every `move(…)`.

```dz
fn main() {
    let a = String("text")
    let b = move(a)
    println!("{}", b)
}
```

```dz
fn main() {
    let a = String("text")
    let b = a
    println!("{}", b)
}
```

## DZ0424

The calls of `parallel { … }` run at the same time, so a place one of them writes is touched
by no other (spec §22.3). A call writes what it is lent as `&mut`, what its `&mut self`
receiver is, and what is moved into it, which is gone before any call starts, since every
argument is evaluated first. Two reads beside each other are fine, and so are two writes to
separate places: two fields, or two elements whose indices are known to differ. Give each
call memory of its own, or call the two one after the other.

```dz
fn physics(positions: &mut List<i32>) kills positions@items {
    positions.clear()
}

fn render(positions: &List<i32>) {
    println!("{}", positions.len())
}

fn main() {
    let mut positions = List<i32>()
    parallel {
        physics(&mut positions)
        render(&positions)
    }
}
```

```dz
fn physics(positions: &mut List<i32>) kills positions@items {
    positions.clear()
}

fn render(positions: &List<i32>) {
    println!("{}", positions.len())
}

fn main() {
    let mut positions = List<i32>()
    let last_frame = positions.clone()
    parallel {
        physics(&mut positions)
        render(&last_frame)
    }
}
```

## DZ0425

Where memory lives is decided by the function that makes it, which puts it in its region,
used by one thread at a time, unless it knows two threads may grow it at once (spec §14.6,
§22.12). A function that gives what a parameter reaches to a call in a `parallel` block, or
to a function that `lends` it, says so with `lends`, so its callers put that memory on the
heap. `dz fix` writes it, and a caller that passes the parameter on says it too.

```dz
struct World {
    pub a: List<i32>
    pub b: List<i32>
}

fn fill(xs: &mut List<i32>) kills xs@items {
    xs.push(1)
}

fn frame(w: &mut World) kills w.a@items, w.b@items {
    parallel {
        fill(&mut w.a)
        fill(&mut w.b)
    }
}

fn main() {
    let mut w = World{ a: List<i32>(), b: List<i32>() }
    frame(&mut w)
}
```

```dz
struct World {
    pub a: List<i32>
    pub b: List<i32>
}

fn fill(xs: &mut List<i32>) kills xs@items {
    xs.push(1)
}

fn frame(w: &mut World) kills w.a@items, w.b@items lends w {
    parallel {
        fill(&mut w.a)
        fill(&mut w.b)
    }
}

fn main() {
    let mut w = World{ a: List<i32>(), b: List<i32>() }
    frame(&mut w)
}
```

## DZ0426

`lends` makes every caller put what it gives on the heap, so it names exactly what a
`parallel` block may grow (spec §22.12). A parameter the body never lends to one, or one that
reaches nothing that owns memory, such as an `i32`, needs no `lends`. `dz fix` narrows the
clause.

```dz
fn bump(n: &mut i32) {
    n += 1
}

fn twice(a: &mut i32, b: &mut i32) lends a {
    parallel {
        bump(a)
        bump(b)
    }
}

fn main() {
    let mut a = 0
    let mut b = 0
    twice(&mut a, &mut b)
}
```

```dz
fn bump(n: &mut i32) {
    n += 1
}

fn twice(a: &mut i32, b: &mut i32) {
    parallel {
        bump(a)
        bump(b)
    }
}

fn main() {
    let mut a = 0
    let mut b = 0
    twice(&mut a, &mut b)
}
```

## DZ0427

`lends` names a whole parameter through which the function can change its caller's memory:
a `&mut` one, `&mut self`, or a view taken by value (spec §22.12). Its callers place that
parameter, never a part of it, and what is lent as `&` is only read, so another thread cannot
grow it. A static is never lent: a call in a `parallel` block cannot touch a plain one.

```dz
fn count(xs: &List<i32>) lends xs {
    println!("{}", xs.len())
}

fn main() {
    let xs = List<i32>()
    count(&xs)
}
```

```dz
fn count(xs: &List<i32>) {
    println!("{}", xs.len())
}

fn main() {
    let xs = List<i32>()
    count(&xs)
}
```

## DZ0428

A function type has no `lends`, so a caller through a function value could not know to put
what it gives on the heap (spec §22.12). A function that lends a parameter is called by its
name.

```dz
fn fill(xs: &mut List<i32>) kills xs@items {
    xs.push(1)
}

fn both(a: &mut List<i32>, b: &mut List<i32>) kills a@items, b@items lends a, b {
    parallel {
        fill(a)
        fill(b)
    }
}

fn main() {
    let f: fn(a: &mut List<i32>, b: &mut List<i32>) kills (a@items, b@items) = both
    let mut a = List<i32>()
    let mut b = List<i32>()
    f(&mut a, &mut b)
}
```

```dz
fn fill(xs: &mut List<i32>) kills xs@items {
    xs.push(1)
}

fn both(a: &mut List<i32>, b: &mut List<i32>) kills a@items, b@items lends a, b {
    parallel {
        fill(a)
        fill(b)
    }
}

fn main() {
    let mut a = List<i32>()
    let mut b = List<i32>()
    both(&mut a, &mut b)
}
```

## DZ0429

A call through a trait, or through a base's `virtual` method, places what it gives by that
method's clause, so an implementation or an override lends only what the method it
implements says it may (spec §22.12). Write the same `lends` on the trait's method.

```dz
fn fill(xs: &mut List<i32>) kills xs@items {
    xs.push(1)
}

trait Grows {
    fn grow(&mut self, a: &mut List<i32>, b: &mut List<i32>) kills a@items, b@items
}

impl Grows for u8 {
    fn grow(&mut self, a: &mut List<i32>, b: &mut List<i32>) kills a@items, b@items lends a, b {
        parallel {
            fill(a)
            fill(b)
        }
    }
}

fn main() {
    let mut s: u8 = 0
    let mut a = List<i32>()
    let mut b = List<i32>()
    s.grow(&mut a, &mut b)
}
```

```dz
fn fill(xs: &mut List<i32>) kills xs@items {
    xs.push(1)
}

trait Grows {
    fn grow(&mut self, a: &mut List<i32>, b: &mut List<i32>) kills a@items, b@items lends a, b
}

impl Grows for u8 {
    fn grow(&mut self, a: &mut List<i32>, b: &mut List<i32>) kills a@items, b@items lends a, b {
        parallel {
            fill(a)
            fill(b)
        }
    }
}

fn main() {
    let mut s: u8 = 0
    let mut a = List<i32>()
    let mut b = List<i32>()
    s.grow(&mut a, &mut b)
}
```

## DZ0430

The passes of a `parallel for` run at the same time on several threads, and whatever the body
uses from outside the loop is reached by every one of them. Each pass may write the element it
is given; anything else it may only read. A total is kept in the elements and summed after the
loop, or kept in an atomic.

```dz
fn main() {
    let mut xs = [1, 2, 3, 4]
    let mut total = 0
    parallel for &mut x in xs {
        x *= 2
        total += x
    }
    println!("{}", total)
}
```

```dz
fn main() {
    let mut xs = [1, 2, 3, 4]
    parallel for &mut x in xs {
        x *= 2
    }
    let mut total = 0
    for &x in xs {
        total += x
    }
    println!("{}", total)
}
```

## DZ0431

Every pass of a `parallel for` is already running when one of them would `break` or `return`,
and nothing can stop the others part-way, so a pass ends only at the end of the body or by
`continue`, which ends that pass alone. A loop inside a pass may still `break` out of itself.
A search that stops at the first match is a plain `for`.

```dz
fn main() {
    let mut xs = [1, 2, 3, 4]
    parallel for &mut x in xs {
        if x > 2 {
            break
        }
        x += 1
    }
}
```

```dz
fn main() {
    let mut xs = [1, 2, 3, 4]
    parallel for &mut x in xs {
        if x > 2 {
            continue
        }
        x += 1
    }
}
```

## DZ0432

A `parallel for` cuts its passes into one range per thread, so it needs to know how many there
are and to start each range without walking the ones before it: a range, an array, a slice, or
an iterator that implements `ParallelIterate`, as a `List`'s does. A `HashMap` is walked one
bucket after another, so it is looped over with a plain `for`.

```dz
fn main() {
    let mut ages = HashMap<i32, i32>()
    ages.insert(1, 30)
    parallel for _, &mut age in ages {
        age += 1
    }
}
```

```dz
fn main() {
    let mut ages = List<i32>()
    ages.push(30)
    parallel for &mut age in ages {
        age += 1
    }
}
```

## DZ0433

When the passes of a `parallel for` write their elements, the container those elements are
in is being written by every thread at once. A pass reaches it only through the element it is
given: reading another element, or the container as a whole, may read what another pass is
writing at that moment. What the loop needs from the container is read before the loop.

```dz
fn main() {
    let mut xs = List<i32>()
    xs.push(1)
    xs.push(2)
    parallel for &mut x in xs {
        x += xs[0]
    }
}
```

```dz
fn main() {
    let mut xs = List<i32>()
    xs.push(1)
    xs.push(2)
    let first = xs[0]
    parallel for &mut x in xs {
        x += first
    }
}
```
