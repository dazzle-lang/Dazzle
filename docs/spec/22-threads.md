# 22. Threads

`[DECIDED — D-0096, D-0112; not built yet: T-0011]` Threads keep the promise Dazzle makes on
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
works out whether a type is plain (§6.3):

- **`Send`**: a value may move to another thread, or be lent to it as `&mut`.
- **`Share`**: a value may be read through `&` by several threads at once.

A type is `Send` and `Share` when every field is. Numbers, `bool`, plain structs and arrays of
them are both; an `own ^mut T` counts as `T`, so a `Box<T>` is what `T` is. A raw `^T` or
`^mut T` field that is not `own` makes a type neither, since what it points at may be anyone's.
**Only `std` implements either by hand**, under `unsafe`, as it writes its containers'
`kills` clauses by hand (§6.7): `Mutex<T>`, the atomics, `Once<T>` and channels are `Share`;
`List`, `String`, `HashMap` and the other containers are what their elements are, though
they hold a `^mut Allocator`, because what a parallel call grows is placed for it (§22.6);
`Rc<T>` and `Weak<T>` are §22.4's.

Generic code names them as bounds, `fn sum<T: Share + Add>(…)`, since a generic body is
checked where it is written (§8).

**`&T` is read-only all the way down** (§5.2): nothing reachable through a `&` is written
outside `unsafe`. That is what makes `Share` mean what it says.

## 22.3 Running at the same time

`std` runs code at the same time in three ways. Each is marked `#[parallel]` there (§12.8),
which is what tells the checker to hold it to this section's rule; user code never writes it.

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

**`join!`** runs a few calls at the same time and returns when every one has:

```dz
join!(
    physics(w.query<&mut Position, &Velocity>()),
    animate(w.query<&mut Sprite, &Clock>()),
)
```

**`spawn!`** starts a thread that lives on its own, `spawn!(physics_thread(inbox, outbox))`,
and gives back a handle that joins it.

**The rule between calls that run at once.** A place one call writes is touched by no other
call; a place several calls only read is frozen while they run. A `&` beside a `&` is fine, a
`&mut` beside anything that reaches the same place is an error naming both calls. The check
reads each call's arguments, as §6.7 reads them for a call's kills:

- an argument crosses as `&T` only if `T` is `Share`, and as `&mut T` or by value only if `T`
  is `Send`;
- a view (§6.7), a query included, may cross into a parallel loop or a `join!`, which wait
  for every call; never into `spawn!`, which does not, so its arguments are values;
- the access a query gives is read off its type arguments: `&mut Position` writes every
  `Position`, `&Velocity` reads every `Velocity` (§22.8).

```dz
join!(
    physics(w.query<&mut Position, &Velocity>()),
    render(w.query<&Position, &Sprite>()),       // error: `physics` writes `Position`, which `render` reads
)
```

Two systems that need the same data run one after the other, or read last frame's copy.

**A panic on any thread ends the program**, as it does on the main thread (§9.8), with the
worker's message and location.

## 22.4 `Rc` across threads

There is no `Arc`. **`Rc<T>` and `Weak<T>` cross into a parallel loop and a `join!`**, and
are checked by type, as one thread already checks them (§14.5): every `Rc<T>` of one `T` is one
place. Calls that only read through `Rc<Texture>` run side by side; a call that writes through
any `Rc<Texture>` conflicts with every other call that reaches one.

Their counts are **atomic in a program that runs anything at the same time**, one where a
parallel loop, `join!` or `spawn!` appears, and plain integers in any other, so a
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
is never run by a parallel loop, `join!` or `spawn!`, and becomes a value only of a function
type written with it, `#[main_thread] fn(w: &World)`.

Needing nothing: a `const`; a `#[thread_local]` static, of which each thread has its own
(§7.7), dropped when its thread ends; a static whose type is `Share`, a `Mutex` or an atomic;
and a static touched only inside `unsafe`, which vouches for it there, as for a `kills` clause.
`Allocators::Global` is such a one, so allocating runs anywhere.

## 22.6 Placement

Anything a parallel call receives by value or as `&mut`, and what it builds there, is placed
as for code the compiler cannot see (§14.6): on the heap, or in the worker's own pool, and
never in the region of the code that started it, whose arena another worker may be growing at
the same moment. Each worker has its own region pool. A value `spawn!` takes is already on
the heap, since it outlives every region.

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
