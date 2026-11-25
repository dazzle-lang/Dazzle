# 22. Threads

`[DECIDED — D-0096, D-0112, D-0118, D-0119; partly built: §22.2, §22.5, §22.6, §22.9 to §22.11 and §22.12 are, and `parallel { … }` is, but for `Rc`, results and queries keyed by type; the parallel loop is not (T-0011)]` Threads keep the promise Dazzle makes on
one thread (§1.1): **a program with no `unsafe` has no undefined behaviour**, data races
included. They do it without changing single-threaded code: everything §5 to §14 allow on one
thread stays allowed, a `&mut` beside a `&` included (D-0094). The stricter rule of this
section applies only where code runs at the same time as other code, and is checked there.

The model is that of a game's job system, as Unity's ECS and EnTT have it: **a loop whose
passes run on every core**, and **systems that do not interfere run side by side**.
Converting a single-threaded program means writing those two things where it wants them,
never changing its types.

## 22.1 What another thread may see

Memory another thread can see is, for as long as it can see it, one of three things:

1. **owned by one thread**, having arrived there by a move;
2. **frozen**: nothing writes or kills it until the threads that share it are done;
3. **behind a type that synchronises**: a `Mutex<T>`, an atomic, a `Once<T>`, a channel.

Ties (§6.7) name memory precisely enough to check this; frozen is a tie nothing writes or
kills for a span.

Three things are refused because they break it: a **data race**, memory written while
another thread reads or writes it; a **thread outliving** the frame or region its references
point into; and **bookkeeping shared without atomics**, such as a count.

## 22.2 `Send` and `Share`

Two traits say what may cross, and the compiler works both out from a type's fields, as it
works out whether a type is plain (§6.3). Both are `std`'s, and nothing implements them
outside it (DZ0596):

- **`Send`**: a value may move to another thread, or be lent to it as `&mut`.
- **`Share`**: a value may be read through `&` by several threads at once.

A type is `Send` and `Share` when every field is. Numbers, `bool`, plain structs and arrays of
them are both; an `own ^mut T` counts as `T`, so a `Box<T>` is what `T` is. A raw `^T` or
`^mut T` field that is not `own` makes a type neither, since what it points at may be anyone's.
**Only `std` implements either by hand**, under `unsafe`, as it writes its containers'
`kills` clauses by hand (§6.7): `Mutex<T>`, the atomics, `Once<T>` and channels are `Share`;
`List`, `String`, `HashMap` and the other containers are what their elements are, though
they hold a `^mut Allocator`, because what a parallel call grows is placed for it (§22.6);
`Rc<T>` and `Weak<T>` are §22.4's. A type whose `drop` is `#[main_thread]` (§22.5) is
neither, and neither is anything holding one, so it only ever dies on the main thread. A
`#[main_thread] fn` is neither; a plain `fn` is both.

Where a value is not `Send` or `Share`, the error names the field that stops it: "its field
`m_header` holds a raw pointer".

Generic code names them as bounds, `fn sum<T: Share + Add>(…)`, since a generic body is
checked where it is written (§8).

**`&T` is read-only all the way down** (§5.2): nothing reachable through a `&` is written
outside `unsafe`. That is what makes `Share` mean what it says.

## 22.3 Running at the same time

Code runs at the same time in three ways: a parallel loop, a `parallel { … }` block, and a
thread started on its own. The loop and the thread are `std`'s, each marked `#[parallel]`
there (§12.8), which is what tells the checker to hold it to this section's rule; user code
never writes that. The block is the language's own, a keyword (D-0118).

**A parallel loop** is a `for` over a parallel iterator: its passes run on every core, each
pass given an element no other pass is given.

```dz
let dt = clock.delta()
for _, &mut p, &v in w.query_parallel<&mut Position, &Velocity>() {
    p.x += v.x * dt
}
for &mut e in enemies.parallel() {
    e.think()
}
```

A container's `parallel()` and a query's `query_parallel` hand one out; a type joins them by
implementing `std`'s `ParallelIterate`. The head is a `for`'s (§10.4), bindings and all; the
loop ends when every pass has, and what follows sees every pass's writes. Since the passes run
at once:

- each pass's element is its own, so `&mut e` writes it freely;
- whatever else the body uses from outside the loop is shared by every pass: it may read
  what is `Share`, and may write nothing (a `&mut` reached from outside, or an assignment to
  an outer local, is an error naming the place);
- `break`, `return` and a jump out by `or return` are errors: no pass can stop the others.
  `continue` ends its own pass.

A loop that must collect, such as two units striking one target, writes into memory of its
own per pass and is merged after; how `std` helps with that is not designed yet.

**`parallel { … }`** runs a few calls at the same time, and ends when every one has
`[DECIDED — D-0118]`:

```dz
parallel {
    physics(&mut w.positions, &w.velocities)
    animate(&mut w.skeletons, &w.time)
}
```

- **Each line is one call**, to a function or a method, that returns nothing (DZ0597). A
  `let`, an assignment or a loop is worked out before the block; a result is written through a
  `&mut` argument. Handing results back is not designed yet.
- **Every argument is evaluated first**, in order, on the thread that runs the block, the
  receiver and default arguments included; then the calls start. So what the arguments make
  and what they move is done before any call runs.
- **The first call runs on that thread, and each other on a thread of its own**; the block
  ends when every call has, and what follows sees every call's writes. A thread that cannot
  start runs its call on the block's thread, after the first.
- **No call may need the main thread** (DZ0598, §22.5), inside `unsafe` too, which vouches for
  a static, not for where a function runs.
- **What crosses is held to §22.2** (DZ0599), and two calls to the rule below (DZ0424). An
  `Rc` or a `Weak` reaches none of its calls yet (§22.4).

A block may be in any function, `main` and a call of another block included; it opens no
scope of its own, since it declares nothing.

**`spawn!`** starts a thread that lives on its own, `spawn!(physics_thread(inbox, outbox))`,
and gives back a handle that joins it.

**The rule between calls that run at once.** A place one call writes is touched by no other
call; a place several calls only read is frozen while they run. A `&` beside a `&` is fine, a
`&mut` beside anything that reaches the same place is an error naming both calls (DZ0424). The
check reads each call's arguments, as §6.7 reads them for a call's kills:

- an argument crosses as `&T` only if `T` is `Share`, and as `&mut T` or by value only if `T`
  is `Send`; a method's receiver is an argument, `&mut self` a `&mut` and `&self` a `&`;
- a call writes what it is lent as `&mut`, and what is moved into it, which is gone before
  any call starts; a value copied into it is read;
- two fields of one value are apart, and so are two elements whose indices are proven to
  differ, `cells[0]` and `cells[2]`; `cells[i]` and `cells[j]` may be one;
- a view (§6.7), a query included, may cross into a parallel loop or a `parallel` block,
  which wait for every call; never into `spawn!`, which does not, so its arguments are values.
  A view taken by value writes what it views unless it is `Share`;
- the access a query gives is read off its type arguments: `&mut Position` writes every
  `Position`, `&Velocity` reads every `Velocity` (§22.8, not built yet).

```dz
parallel {
    physics(&mut w.positions, &w.velocities)
    render(&w.positions)               // error: `physics` writes `w.positions`, which `render` reads
}
```

Two systems that need the same data run one after the other, or read last frame's copy.

**A panic on any thread ends the program**, as it does on the main thread (§9.8), with the
worker's message and location.

## 22.4 `Rc` across threads

`[Not built yet: a `parallel` block refuses an `Rc` or a `Weak` in any argument, DZ0599, until
the counts below are atomic (T-0266).]`

There is no `Arc`. **`Rc<T>` and `Weak<T>` cross into a parallel loop and a `parallel` block**, and
are checked by type, as one thread already checks them (§14.5): every `Rc<T>` of one `T` is one
place. Calls that only read through `Rc<Texture>` run side by side; a call that writes through
any `Rc<Texture>` conflicts with every other call that reaches one.

Their counts are **atomic in a program that runs anything at the same time**, one where a
parallel loop, a `parallel` block or `spawn!` appears, and plain integers in any other, so a
single-threaded program pays nothing; the compiler sees the whole program (§8.4). Where nothing
in an `if let` or `or` body can release the handle, no count is taken at all (T-0002).

**`spawn!` takes no `Rc` or `Weak`**, in any value it is given: nothing waits for its thread,
so no scope lets the compiler see who writes when. A spawned thread shares through a channel,
a `Mutex<T>` or an atomic.

## 22.5 Statics and C: `#[main_thread]`

A static is reached by name, not through an argument, so §22.3's check cannot see it from a
call. **A function that touches a plain static, or calls C its recipe does not mark
thread-safe (§15.1), is written `#[main_thread]`**, and so is every function that calls one:

```dz
static FRAME_COUNT: u64 = 0

#[main_thread]
fn tick() { FRAME_COUNT += 1 }

#[main_thread]
fn draw_hud(hud: &Hud) { rl::DrawText(hud.title, 10, 10, 20, rl::WHITE) }
```

It is held to the body as a `kills` clause is (§6.7): a body that needs it and lacks it is an
error, and one that has it and does not need it is too; `dz fix` writes it. An entry point
(`main`, a `#[test]`, a `#[bench]`) is on the main thread already. A `#[main_thread]` function
is never run by a parallel loop, a `parallel` block or `spawn!`, and becomes a value only of a function
type written with it, `#[main_thread] fn(w: &World)`.

Needing nothing: a `const`; a `#[thread_local]` static, of which each thread has its own
(§7.7), dropped when its thread ends; a static whose type synchronises, a `Mutex` or an
atomic; and a static touched only inside `unsafe`, which vouches for it there, as for a `kills`
clause. `Allocators::Global` is such a one, so allocating runs anywhere. A plain `u64` is
`Share`, but a static of one is written by assignment, never through `&`, so it is the main
thread's; which types synchronise is `std`'s to say, with a trait of its own no program names.

**Where it is held** (DZ0592, DZ0593):

- A call through a value of type `#[main_thread] fn(…)` is the main thread's, as a call by
  name is; any function fits that type, and only one that runs anywhere fits a plain `fn(…)`
  (DZ0594). A C function pointer is a `#[main_thread] fn`.
- A method that implements a trait's, or overrides a `virtual` one, may need the main thread
  only where the method it implements says `#[main_thread]`, since a call through the trait or
  the base may run anywhere (DZ0595). A `virtual` method and a trait's default body may say it
  without needing it, for what replaces them.
- **A `drop` may be `#[main_thread]`**, as one that gives back what a GPU or a window made
  must be. A value of such a type, or of anything holding one, must die on the main thread: it
  is neither `Send` nor `Share`, and a function that makes one, by a literal or a call, or
  takes one by value, is `#[main_thread]` itself. Lent by `&`, it is no one's to drop.
- A field's default runs wherever its struct is built, so it may need nothing of the main
  thread; a constructor that does is called from a function that is `#[main_thread]`.
- A constructor may be `#[main_thread]`, as a function may.

## 22.6 Placement

Anything a parallel call receives by value or as `&mut`, and what it builds there, is placed
as for code the compiler cannot see (§14.6): on the heap, or in the worker's own pool, and
never in the region of the code that started it, whose arena another worker may be growing at
the same moment. Each worker has its own region pool. A value `spawn!` takes is already on
the heap, since it outlives every region.

A container that reaches a `parallel` call from further out, through a parameter, is placed
by a caller that cannot see the block: `lends` in the signature tells it (§22.12), and it puts
that container on the heap. So a region is only ever grown by the thread that opened it, and
nothing is locked or checked at run time: a program pays for threads only in what it gives
them.

## 22.7 Closures in parallel code

A view closure (§7.8) captures by reference, so passed to code that runs at once its captures
are checked as that code's arguments are (§22.3): a captured `&mut` crosses only into one
call, a captured `&` only if it is `Share`.

## 22.8 Queries keyed by type

An ECS keeps one storage per component type in one map, and a query holds two of them; to
the ties check (§6.7) the map would be one place, and two systems on different components
would always conflict. Three pieces let a query say which component it writes:

- **A tie per type**, `tie @[T] = m_sets[T]`, over a map `std` promises keeps each type's
  storage apart, its `TypeMap`. `self@[Position]` and `self@[Sprite]` are apart; in a
  generic body `self@[A::Target]` and `self@[B::Target]` may be one type and count as
  overlapping unless known apart.
- **`ref` type parameters.** `ref A` takes a `&T` or a `&mut T`, and only those; it is a
  `view` parameter (§6.7). The body is checked once, for both.
- **`A::Target`**, the type a `ref` parameter points at, given by the compiler.

```dz
pub fn query<ref A, ref B>(&mut self) -> Query<A, B>
    in self@[A::Target], self@[B::Target]
{ … }

fn physics(q: Query<&mut Position, &Velocity>) {
    for _, &mut p, &v in q { p.x += v.x }
}
```

A query's type arguments are its access: a view whose type arguments include `&mut X` writes
`@[X]`, and one with `&X` reads it, which is how §22.3 reads it.

## 22.9 Atomics

`[DECIDED — D-0113]` **`Atomic<T>`** is a value several threads may read and write at once.
`T` is `bool` or an integer type, and nothing else can be made one.

```dz
static NEXT_ID: Atomic<u32> = Atomic<u32>(0)

fn new_id() -> u32 {
    return NEXT_ID.add(1)
}
```

Every operation takes `&self`, since writing here is what the type is for:

| Operation | What it does | Returns |
|---|---|---|
| `load()` | reads the value | the value |
| `store(v)` | replaces the value | nothing |
| `swap(v)` | replaces the value | the value it had |
| `compare_exchange(expected, v)` | stores `v` if the value is `expected` | the value it found, so it stored exactly when that equals `expected` |
| `add(n)`, `sub(n)` | integers only; wrap on overflow | the value it had |

Every operation is **sequentially consistent**: all threads see all atomic operations in one
order. Weaker orderings are not in the language yet.

An `Atomic` is not plain data (§6.3), so using one by value moves it; two copies would be two
separate counters. `clone` makes a new one from a single read. A `static` of one may start as
`Atomic<T>(c)` for a constant `c` (§7.7).

## 22.10 `Mutex<T>`

`[PROPOSED — D-0114]` **`Mutex<T>`** owns a value that one thread at a time may reach.
`lock()` waits for its turn and returns a **`MutexGuard<T>`**; dropping the guard releases the
lock.

```dz
static SCORE: Mutex<i32> = Mutex<i32>(0)

fn add(points: i32) {
    let mut held = SCORE.lock()
    let score = held.get_mut()
    score += points
}                                   // unlocked here
```

`get()` gives the value as `&T` and `get_mut()` as `&mut T`. Each is **tied to the guard**, so
it cannot be used once the guard is dropped, moved or assigned over, and a guard that was not
kept gives out nothing that outlives its statement (`m.lock().get()` bound to a name is
`DZ0407`).

A guard is a view (§6.7): it lives in a local, a parameter or a result, never in a field or a
container, and its mutex cannot be moved or dropped while it exists. A `Mutex` is not plain
data, so it is never copied. A `static` of one may start as `Mutex<T>(v)`, for a `v` a static
may start as (§7.7).

There is **no poisoned state**, since a panic on any thread ends the program (§22.3). A mutex
is **not reentrant**: locking one the thread already holds waits forever.

## 22.11 Starting a thread

`[PROPOSED — D-0117]` **`spawn(entry, value)`** runs `entry(value)` on a new thread and returns a
**`Thread`**, the handle that joins it. It is the function `spawn!` (§22.3) is to write; the macro
is not built.

```dz
struct Job { pub first: u32, pub last: u32 }

fn sum_range(job: Job) { … }

let mut t = spawn(sum_range, Job{ first: 0, last: 100 })
t.join()
```

`value` moves to the thread, into an allocation on the heap that the thread frees as it starts
(§22.6). A thread takes one value, so a struct or a tuple gives it more. `join()` waits for the
thread to finish, and everything it wrote is then visible; joining again does nothing. A
`Thread` dropped without being joined **detaches**: the thread runs on, and ends with the
program. A thread returns nothing; it shares through an atomic (§22.9) or a `Mutex<T>` (§22.10).

What crosses is checked, so a program with no `unsafe` that starts threads has no data
race: `spawn<A: Send>` takes a value that holds no `Rc`, view or raw pointer (§22.2), and its
`entry` is a plain `fn(A)`, which a `#[main_thread]` function is not (§22.5). What the value
holds is placed on the heap, never in a region of the code that started the thread (§22.6).

## 22.12 Lending to a block: `lends`

`[DECIDED — D-0119]` Where memory lives is decided by the function that makes it, which puts
it in its region, used by one thread at a time, unless it knows two threads may grow it at once
(§14.6). A container made in a function with a `parallel` block, and given to its calls, is
placed seeing the block. One made further up and handed down as a parameter is not, so the
function that gives it to a block says so in its signature:

```dz
fn frame(w: &mut World) kills w.positions@items, w.velocities@items lends w {
    parallel {
        fill(&mut w.positions, 3000)
        fill(&mut w.velocities, 2000)
    }
}

fn game_loop(w: &mut World) kills w.positions@items, w.velocities@items lends w {
    frame(w)                        // passes `w` on to a function that lends it
}

fn main() {
    let mut w = World{ … }          // on the heap: `game_loop` lends what it is given
    game_loop(&mut w)
}
```

- **`lends p`** says that some call in a `parallel` block may change, and grow, what `p`
  reaches: written through `&mut`, a `&mut self` receiver or a view, directly or through a
  function that `lends` it in turn. A caller puts what it gives there on the heap.
- **It names whole parameters** the function can change its caller's memory through: a
  `&mut`, `[]mut` or `^mut` one, `self` for `&mut self`, or a view taken by value
  (DZ0427). What is lent as `&` is only read, and what is given by value is the function's
  own.
- **It is held to the body**, as `kills` is: a body that lends a parameter its clause leaves
  out is an error (DZ0425), and so is a clause naming one the body never lends (DZ0426); `dz
  fix` writes it. Only what may own memory counts: an `i32` lent to a block needs no `lends`.
- **A function that lends is called by name.** A function type has no `lends`, so such a
  function is never a function value (DZ0428). An implementation of a trait method, or an
  override, lends only what the method it implements says (DZ0429).

The clause is about placement alone: it changes where memory lives, never what a program does
(§14.6). It is what lets a region be grown by one thread only, so nothing is locked or checked
at run time (§22.6).
