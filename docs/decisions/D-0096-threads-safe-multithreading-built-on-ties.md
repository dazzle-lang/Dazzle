---
id: D-0096
title: Threads: safe multithreading built on ties
status: open
---

**Where it stands.** A design, worked out with the user on 2026-11-10 and agreed in
direction, to be built after 0.1 ([D-0011](D-0011-concurrency.md) places concurrency in
v2; the work is [T-0011](../tasks/T-0011-threads-mutex-atomics-job-system.md)). Nothing
here is in the spec, and the syntax is a sketch. It reaches the spec, with a §21 row, when
the user decides it. **Agreed so far** and **still open** are listed at the end.

**The question.** How can Dazzle have threads with the same promise it makes on one thread:
a program with no `unsafe` has no undefined behaviour (§1.1, D-0094)? And how can it do
that without taking from single-threaded code the aliasing it allows today?

## Why ties alone are not enough

On one thread, `&mut xs[i]` beside `&xs[j]` is legal, because the only danger there is
memory being **freed or moved** while something still sees it, a kill (§6.7). Across
threads there is a second danger: memory **written while another thread reads it**. That
is a data race, undefined behaviour even when nothing is freed. Ties answer "is this memory
still alive?", not "is someone else writing it right now?".

Three things go wrong across threads:

- **Data races**, as above.
- **A thread outliving** the stack frame or region its references point into.
- **Shared bookkeeping without atomics:** an `Rc`'s count (an `if let` on an `Rc` bumps
  it, even through `&`), a region's bump pointer, the memory tracker
  ([T-0038](../tasks/T-0038-memory-tracking-single-threaded.md)).

## The rule

**Memory another thread can see is, for as long as it can see it, one of three things:**

1. **owned by one thread**, having arrived there by `move`;
2. **frozen**: nothing writes or kills it until the threads that share it are done;
3. **behind a type that synchronises**: `Mutex<T>`, an atomic, `Arc<T>`, `Once<T>`, a
   channel.

Ties already name memory precisely enough to check this. The one new notion is **frozen**:
a tie nothing writes or kills for a span. Single-threaded code pays nothing. Aliasing,
kills and placement are unchanged outside the parallel calls; the stricter discipline
applies only to memory that crosses threads.

## The pieces

**1. Two properties of types**, worked out from a type's fields as copyability is (names
open):

- **sendable**: may move to another thread. It holds no view, no `Rc`/`Weak` and no raw
  pointer.
- **shareable**: may be read through `&` by several threads at once. It holds no
  `Rc`/`Weak`, since reading one through `&` writes its count. `Mutex<T>`, atomics, `Arc<T>`
  and `Once<T>` are shareable by declaration, an `unsafe` promise in `std`, trusted as
  `std`'s `kills` clauses are.

Generic code needs them as bounds, since generics are checked at the definition (§8).

**2. Parallelism is library, not syntax.** `std` provides:

- **`join`**: runs several calls at once and returns when all are done. The building block
  for systems and job graphs.
- **`for_each`** over a container or a query: a parallel loop, each pass given one element.
- **`spawn`**: a long-lived thread, given only sendable values, moved in. It shares state
  through `Arc`, `Mutex` and channels. Its handle can never hold a reference, since
  references never live in fields (D-0003, §6.7).
- `Mutex<T>`, atomics, `Arc<T>` ([T-0001](../tasks/T-0001-arc-t.md)), channels, `Once<T>`.

Dazzle has no closures, so `join` and `for_each` are macros (`join!`, `for_each!`) that
turn their calls into function values plus arguments. A loop body that needs locals becomes
a function and a context struct. If that is too clunky in practice, closures become a
question of their own.

**3. Scoped borrows are what make references safe.** `join` and `for_each` return only when
every call has finished, so the parent cannot touch what it lent until then. That is the
only way a thread can hold a reference into another's memory, and the reason it is sound.
`spawn` takes values only.

**4. `join`'s arguments are checked as if they run at once.** D-0094 point 1 allows
`f(&mut a, &a)` when `f` kills nothing. A parallel call needs the stricter rule: a `&mut`
argument of one call must not overlap a tie that any other call receives. A `&` beside a
`&` is fine. One marker on `std`'s parallel functions, written once, switches the checker
to this rule; it is the one language hook that keeps parallelism a library.

```dz
join!(
    physics(&mut w.positions, &w.velocities),
    animate(&mut w.skeletons, &w.time),       // fine: different ties
)
join!(
    physics(&mut w.positions, &w.velocities),
    render(&w.positions),                     // error: physics writes `w.positions` while render reads it
)
```

`&T` in Dazzle is truly read-only, so a function's parameters say everything it can write,
except statics and handles. That is what makes the check work from signatures alone.

**5. No marker on functions.** There is no `thread fn`, by the user's choice of 2026-11-10.
A plain function type `fn(…)` means "touches no plain static". A function that touches a
plain static, directly or through its calls, says so in a clause, as `kills SCORES.list@items`
already names a static (§7.7). The clause's spelling is open; `uses FRAME_COUNT` is the
placeholder. Such a function cannot become a plain `fn(…)` value, so it can never reach
`spawn` or `join`. Most functions touch only their parameters and write nothing. Writing
the clause, rather than inferring it, keeps a change to a body in one module from breaking
a `join` in another, the same reason `kills` clauses are explicit.

**6. Statics.**

| Static | On a thread other than the main one |
|---|---|
| `const` | ✅ |
| `#[thread_local] static` | ✅ each thread has its own (T-0163: dropped on every thread, not only the main one) |
| `static X: Mutex<T>`, an atomic, `Once<T>` | ✅ the type synchronises |
| a plain `static` | ❌ main thread only (piece 5) |

`Once<T>` is built once and then read-only: a lookup table built at startup and read
everywhere.

**7. Placement and regions (§14.6).** Each worker has its own region pool. A value moved
to another thread already fails the region check and goes to the heap, so placement needs
no new rule for it. Two new ones:

- a worker's result goes to the heap, since writing into the parent's region would race on
  its bump pointer;
- a container lent `&mut` into a parallel call allocates on the heap or from the worker's
  own pool, never from the parent's region, which another worker may be bumping at the same
  time. Placement treats "lent to a parallel call" as it treats "lent to code it cannot
  see".

`Rc` stays on one thread; `Arc` crosses. The memory tracker becomes atomic (T-0038).

**8. C and C++ are main-thread by default.** The compiler cannot see inside a C function,
and raylib and SDL must be called from the main thread. An imported function is treated as
touching a plain static (piece 5) unless its recipe marks it thread-safe, as
`keeps = […]` marks what a header keeps (§15, D-0094 point 6).

## Worked example: an ECS whose world is keyed by type

Checked against the user's Odin ECS: a registry of type-erased sparse sets keyed by
`typeid`, components registered at run time, queries over one to five component types, and
a `CommandBuffer` for spawning entities and adding or removing components. Single-threaded,
it ports almost directly. Two parts get simpler:

- the hand-built vtable (`ComponentInfo` and a trampoline per component) becomes
  `trait AnySet` with `impl<T> AnySet for SparseSet<T>`;
- the comment "pointer is invalidated by any `sparse_add` / `sparse_remove` call" becomes a
  `kills` clause, so keeping a component reference across an add is a compile-time error.

The problem with threads is that every set lives in one map, so to the ties check they are
all one tie: `w.query<Position>()` and `w.query<Sprite>()` look like the same memory. The
user chose **option A**: a separate tie for each component type, checked at compile time.
The alternative was a run-time reader/writer flag per set, as Bevy does.

```dz
pub struct World {
    m_sets: TypeMap                       // TypeId -> that type's SparseSet, type-erased
    m_generations: List<u32>
    tie @[T] = m_sets[T]                  // NEW: one tie per component type

    pub fn register<T>(&mut self) kills self@sets { … }

    pub fn query<ref A, ref B>(&mut self) -> Query<A, B>
        in self@[A::Target], self@[B::Target]          // NEW: tied per component
    {
        return Query<A, B>{
            m_a: m_sets.get<A::Target>(),
            m_b: m_sets.get<B::Target>(),
            m_generations: &m_generations,
        }
    }
}

pub view struct Query<ref A, ref B> {                  // NEW: `ref` parameters
    m_a: ^mut SparseSet<A::Target>
    m_b: ^mut SparseSet<B::Target>
    m_generations: &List<u32>
    m_index: usize = 0
}

impl<ref A, ref B> Iterate for Query<A, B> {
    type Item = (Entity, A, B)
    fn valid(&self) -> bool { … }
    fn get(&self) -> (Entity, A, B) {
        let e = …
        let a = unsafe { m_a.at_dense(m_index) }       // a &mut to the component
        let b = unsafe { m_b.at(e) }
        return (e, a, b)                               // &mut X becomes A, whether A is &X or &mut X
    }
    fn advance(&mut self) { … skip entities that have no B … }
}
```

Game code is all safe:

```dz
fn physics(q: Query<&mut Position, &Velocity>, cmds: &mut CommandBuffer) {
    for (e, pos, vel) in q {
        pos.x += vel.dx
    }
}

fn frame(w: &mut World, pool: &Pool) {
    let mut phys_cmds = CommandBuffer()
    let mut anim_cmds = CommandBuffer()
    pool.join!(
        physics(w.query(), &mut phys_cmds),
        sprite_animation(w.query(), &mut anim_cmds, dt),
    )
    w.apply(move(phys_cmds))      // kills the sets' ties: fine, nothing is running now
    w.apply(move(anim_cmds))
}
```

**The new language pieces it needs:**

- **`ref` type parameters.** `ref A` takes a `&T` or a `&mut T`, and implies `view`
  (D-0093: a view may be a type argument only to a `view` parameter). The parameter must
  say it takes a reference so the body can be checked once, at the definition, and never
  fail in an instance the user did not write (principle 4). The user chose that no trait
  and no impl are written: the compiler checks `ref` at each use.
- **The type a reference points at**, given by the compiler: `A::Target` or
  `target_of<A>`, spelling open. The body never asks whether `A` is `&mut`: it fetches a
  `&mut`, which converts to either.
- **A tie per type**, `@[T]`, over a map `std` promises gives each type its own storage.
  In generic code `A::Target` and `B::Target` may be the same type, so they count as
  overlapping unless known apart.
- **`join` reads access off the types.** A view whose type arguments include `&mut X`
  writes `@[X]`, and one with `&X` reads it. No `Write<>`/`Read<>` wrappers: the user's
  call, since `&mut Position` is already the type each item has.

**What the compiler enforces from rules Dazzle already has:**

- A system never receives the world, only its queries, so it cannot spawn or add a
  component directly. Doing so kills a set's tie while another system may hold it. The
  `CommandBuffer` pattern is not a convention; it is the only thing that compiles.
- Each thread gets its own `CommandBuffer`, so recording needs no lock. Every buffer in a
  `join` is applied after the group finishes, where Odin flushes after each system.
- An entity's id can be reserved at once (`cmds.spawn(&w.entities)`) through an atomic
  counter, shareable, so two systems may both reserve.
- Splitting one system's loop across workers (`for_each!` over a query) needs a fact the
  checker cannot prove: a sparse set yields each entity once, so the `&mut`s are distinct.
  `std`'s `Query` promises it, under `unsafe`, checked at run time in debug builds.

**A physics thread on its own clock** (a fixed 60 Hz, whatever the frame rate) cannot
borrow the world, since no scope is waited on by both sides. That is `spawn`. The physics
thread owns its rigid bodies outright. It receives inputs and new bodies through a channel
and sends back snapshots of transforms, which the main thread copies into the world.
Nothing is shared, so nothing needs a lock.

**Later: registered systems, scheduled automatically.** Because each system's access is in
its signature, systems can be registered and grouped into parallel runs by `std` or a
library, instead of arranged in `join`s by hand. Agreed as the direction; not designed.

## Known problems

- **`Rc` in components.** A `Sprite` holding `Rc<Texture>` cannot be read on two threads.
  The answer, agreed: `Arc`, or asset handles as plain ids, as Rust does. This changes how
  game code holds shared assets.
- **Prerequisites that do not exist yet:** owned trait objects (`Box<AnySet>`, "a handle to
  a trait object", deferred in §14.5), a safe `TypeMap` with a checked downcast, atomics,
  `Arc` (T-0001), an atomic memory tracker (T-0038), and `thread_local` drops on every thread
  (T-0163). Without the first two, the world is `unsafe` inside, as in Odin.
- **No variadic type parameters.** `Query<A>` to `Query<A, B, C, D, E>` are written five
  times, or generated by a macro with `#for`, as Odin writes them five times.
- **Not designed:** what a panic on a worker does; how `for` destructures a tuple of views,
  `for (e, pos, vel) in q` (§10.4); implementations on reference types, which this design
  avoids needing.

## Before deciding: try to break it

D-0094's holes were found by writing small programs the checker should refuse and finding
that it accepted them. Do the same here before building anything:

1. **Port four or five real systems of the Odin ECS on paper**: sprite animation, the
   player, bullets, something with random access to another entity.
2. **Write the programs that must be refused**, and find for each the rule that refuses it:
   - `Query<&mut Position, &Position>` joined with a system that reads `Position`;
   - a component holding an `Rc`, read by two systems in one `join`;
   - a raylib call from a function given to `join`;
   - a generic system `fn s<ref A>(q: Query<A>)` joined with a concrete one;
   - a function that touches a plain static through a call two modules away, passed to
     `spawn`;
   - a container in the parent's region, lent `&mut` to two calls of one `join`;
   - an `Arc<T>` whose `T` is not shareable.

The design is ready to decide when every one of them has a rule that refuses it.

## Agreed and open

| | |
|---|---|
| **Agreed, 2026-11-10** | the three-way rule (owned, frozen, synchronised); parallelism in `std` (`join`, `for_each`, `spawn`), not syntax; no `thread fn` marker; `Arc` for sharing, as Rust does; C and C++ main-thread unless the recipe says otherwise; a tie per component type (option A); `&`/`&mut` as query arguments, no `Write<>`/`Read<>`; the pointed-at type given by the compiler, no user-written trait; registered systems scheduled automatically, later; owned trait objects and `TypeMap` as prerequisites |
| **Open** | the names of sendable and shareable; the clause naming plain statics (`uses`?); `A::Target` or `target_of<A>`; `ref` as the keyword; the marker on `join`; a panic on a worker; closures |

**What it would change, once decided:** §1.1 (the promise loses "on one thread"), §6.7
(frozen ties, `join`'s argument rule, ties per type), §7.7 (plain statics main-thread
only), §8 (`ref` parameters and the pointed-at type), §14.5 (`Arc`, handles to trait
objects), §14.6 (worker pools, lent containers), §15 (thread-safe imports).
