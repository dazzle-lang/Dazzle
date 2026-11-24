---
id: D-0096
title: Threads: safe multithreading built on ties
status: decided
---

**Decided, 2026-11-18, r144: spec §22**, with §1.1, §5.2, §7.7, §10.4, §12.8, §14.5, §14.6
and §15.1. Where this file and the spec differ, the spec holds: it is this design as
amended by "Tried, 2026-11-18" and the table at the end. **The parallel loop is a `for` over
a parallel iterator**, the user's choice of the same day ("this doesn't read like parallel
at all"), so `for_each!` below is gone: `for _, &mut p, &v in
w.query_parallel<&mut Position, &Velocity>() { … }`, and `xs.parallel()` for a container.
Built as [T-0011](../tasks/T-0011-threads-mutex-atomics-job-system.md) and the M19 tasks.

**Where it stood before.** A design, worked out with the user on 2026-11-10 and agreed in
direction, to be built after 0.1 ([D-0011](D-0011-concurrency.md) places concurrency in
v2). The syntax below is the sketch it was decided from.

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

## Tried, 2026-11-18 (T-0245)

Done against the language as it is at r143, with `tests/bench_programs/ecs` as the ECS
(T-0172 built it after this design was written) and the compiler checking each
single-thread fact the reasoning rests on.

### The seven programs

| Program | Refused? | By |
|---|---|---|
| `Query<&mut Position, &Position>` joined with a reader of `Position` | ✅ | piece 4: the query writes `@[Position]`, read by the other call |
| a component holding an `Rc`, read by two systems in one `join` | ✅ as written, ❌ one step on | piece 1 refuses it through `&`; two systems given `&mut` to **different** components that hold `Rc`s to one texture pass piece 4 and race on its count (hole 2) |
| a raylib call from a function given to `join` | ✅ | pieces 5 and 8, once a C call's `uses` has something to name (question 5 below) |
| a generic `fn s<ref A>(q: Query<A>)` joined with a concrete system | ✅ | written with a concrete argument, `A::Target` is known and checked; inside a generic body, `A::Target` overlaps everything, so it is refused even where it would be fine. A cost, not a hole |
| a static touched two modules away, the function passed to `spawn` | ✅, with a gap | the written `uses` clause carries it up as `kills` does; but `uses` must exempt a static touched inside `unsafe`, as `kills` does, or `Allocators::Global`, a reference to the plain static `HEAP`, bars every function that allocates (hole 6) |
| a container in the parent's region lent `&mut` to two calls | ✅ | piece 4: the two `&mut`s overlap. Lent to **one** call, or **moved** into one, it is piece 7's, and piece 7 covers only the first (hole 5) |
| an `Arc<T>` whose `T` is not shareable | ❌ as written | piece 2 makes `Arc` shareable by declaration, unconditionally (hole 3) |

### Holes the list did not have

1. **`&T` is not read-only through a view's `&mut` field.** A view struct holding `&mut
   Counter` gives `&self` methods write access to it, and `std` relies on that: `Iterate`'s
   `get(&self)` hands out `&mut` items, as `Query2::get` does. Checked: `fn reader(h:
   &Holder) { h.bump() }` writes through `h.c`, and two calls print 2. Piece 4 lets two
   parallel calls share a `&`, and piece 1 calls such a type shareable (no `Rc`), so
   `join!(reader(&h), reader(&h))` is a data race in safe code. Piece 4's premise, "`&T` in
   Dazzle is truly read-only", is false.

   **Proposed:** shareable and sendable are **structural**, worked out from the fields as
   copyability is, never named type by type. A type is not shareable if it holds a `&mut`
   or a `^mut` that is not `own`; so `Rc` and `Weak` (whose `m_header` is a plain `^mut
   RcHeader`) fall out without a rule of their own, and so does every view holding a `&mut`.
   An `own ^mut T` counts as `T`, so `Box`, `List` and `String` are what their elements are.
   `std` declares the exceptions under `unsafe`, as it vouches for `kills`: `Mutex`,
   atomics, `Arc`, `Once` and channels as shareable; `List` and `String` as sendable despite
   their `^mut Allocator` (hole 5 says what that rests on). The alternative, Rust's rule that
   a `&mut` reached through `&` reads as `&`, would change single-threaded code and break
   `Iterate`.

2. **An `Rc` races through `&mut` too, not only through `&`.** Every `Rc<T>` of one value
   shares one count, which `or` and `if let` write. Two parallel calls each given `&mut` to
   their own component, each holding an `Rc<Texture>` to one texture, touch no common tie,
   and race on the count.

   **Proposed**, Rust's mapping, which with hole 1's structural rule covers both: an argument
   crosses into a parallel call as `&T` only if `T` is shareable, and as `&mut T` or by value
   only if `T` is sendable. A view by value is allowed in `join` and `for_each` (the scope
   waits) and never in `spawn`.

3. **`Arc` cannot be `Rc` with an atomic count.** An `Rc` gives `&mut` to its value from any
   handle in a writable place (§14.5), so two threads with a clone each could both write.

   **Proposed:** an `Arc<T>` gives only `&T`; writing goes through a `Mutex` or an atomic
   inside it, `Arc<Mutex<T>>`. `Arc<T>` is shareable and sendable only when `T` is both,
   since the last drop may run on any thread.

4. **`for_each!`'s other arguments reach every pass at once.** D-0096 says each pass is given
   one element, and says nothing of what else the body is given. Those arguments are given to
   N calls at once, so they are checked as a `join` of N calls of one function: a `&mut` is
   always refused, since it overlaps itself.

   That refuses two of the battle's four systems (below) as they are written, since each
   writes **another** entity's component. A parallel loop needs **per-worker output**: each
   worker gets a value of its own to write into (a `List` of hits, a `CommandBuffer`), and
   the group's results are merged when it ends, as Rayon's `fold` and `map_init` do. Not
   designed.

5. **What a parallel call allocates into.** Piece 7 sends `&mut`-lent containers to the heap
   or the worker's pool. A container **moved** into a `join` call passes the region check,
   since the scope outlives the call, so it stays in the parent's region, and its
   `m_allocator`, a raw `^mut Allocator`, is the parent's region arena; two workers growing
   such lists bump one pointer at once. **Proposed:** anything a parallel call receives,
   by value or `&mut`, is placed as for code the compiler cannot see (heap or the worker's
   pool), so `List` and `String` may be declared sendable (hole 1) on that promise.

6. **`uses` needs `kills`' exemption for `unsafe`.** A static touched only inside `unsafe` is
   vouched for there, so `Allocators::Global` (`unsafe { &mut HEAP }`, a plain static) leaves
   allocation thread-safe, as `malloc` is. Every such voucher in `std` is then a promise to
   audit: `HEAP` holds nothing; `TRACKER` is not safe until T-0038.

7. **The battle has nothing to `join`.** Ported:

   | System | Reads | Writes | Parallel how |
   |---|---|---|---|
   | `movement` | | `Position`, `Velocity` (its own entity's) | `for_each!` as it is |
   | `separate` | `Position` | `Velocity` (both units') | `for_each!` only with per-worker output (hole 4) |
   | `fight` | `Position`, `Team` | `Health` (the target's), `Velocity` (its own) | the same: two units may strike one target |
   | `reinforce` | `Health`, `Team`, the `Rng` resource | spawns and despawns | not at all: a `CommandBuffer`, applied after |

   Every pair overlaps on `Velocity` or `Health`, so `join!` gives this program nothing; its
   parallelism is all inside `for_each!`, and half of that needs hole 4. D-0096's own
   example (physics beside sprite animation) has systems apart, which is where `join!` pays.
   M19's criterion should say `for_each!` with per-worker output, not only `join!`.

### What is left to decide

The seven programs and the seven holes each have a rule above, proposed. Deciding D-0096
means deciding those, holes 1 to 5 above all, with the open questions below and these:

- **What a function that calls C names in its `uses`.** Piece 8 makes a C call touch
  something static unless its recipe says otherwise, and the clause needs a name for it:
  the dependency (`uses raylib`), or one name for "the main thread" (`uses main`).
- **Per-worker output in `for_each!`** (hole 4): its spelling, and how results merge.

## Agreed and open

| | |
|---|---|
| **Agreed, 2026-11-10** | the three-way rule (owned, frozen, synchronised); parallelism in `std` (`join`, `for_each`, `spawn`), not syntax; no `thread fn` marker; `Arc` for sharing, as Rust does; C and C++ main-thread unless the recipe says otherwise; a tie per component type (option A); `&`/`&mut` as query arguments, no `Write<>`/`Read<>`; the pointed-at type given by the compiler, no user-written trait; registered systems scheduled automatically, later; owned trait objects and `TypeMap` as prerequisites |
| **Agreed, 2026-11-18** | a panic on a worker aborts the program, as a panic anywhere does, printing the worker's message and location; the model is Unity's ECS and EnTT's, a parallel `for_each!` and systems that do not interfere run side by side, and per-worker output (hole 4) is set aside for now. **Safe as Rust, simpler**, the user's aim for all of it, and: |
| | **`&` is deep** (hole 1): nothing reachable through a `&` is written, so a `&mut` field reached through `&` reads as `&`, and `Iterate::get` takes `&mut self`, as Rust's `next` does. The one change to single-threaded code, and only to code that writes through `&` to a view holding a `&mut` (`std`'s four mutable iterators, `ecs`'s queries); game code using them is unchanged. A language change of its own, built as T-0258 with a §21 row |
| | **One `Rc`, no `Arc`** (holes 2 and 3, replacing their proposals): converting a single-threaded program means adding `join!` and `for_each!`, never changing its types. A parallel group checks `Rc<T>` as one place per type, as one thread already does (§14.5): calls that only read through `Rc<Texture>` run side by side, one that writes through any conflicts with every other call reaching one. The count is atomic only in a program that uses threads, so a single-threaded one pays nothing; T-0002 (no count at all where the handle cannot be released) moves into M19, since a contended atomic count is the cost left. `Mutex` and channels are for `spawn` alone, where no scope lets the compiler see who writes when |
| | **Single-threaded code keeps how it works today**: D-0094's aliasing, a `&mut` beside a `&`, stays legal everywhere. The stricter rule, a place one call writes is touched by no other call, applies only between the calls of one parallel group (piece 4) |
| | **`Send` and `Share`** are the names of sendable and shareable, worked out from a type's fields (hole 1's proposal). **Holes 5 and 6's rules are approved**: anything a parallel call receives, by value or `&mut`, allocates from the heap or the worker's own pool, never the parent's region; and a static touched only inside `unsafe` is vouched for there, as for `kills`, so allocating through `Allocators::Global` stays allowed. **Closures are needed** ("we need closures"), so a `for_each!` body is written inline: a design of its own, [D-0112](D-0112-closures.md) |
| | **Statics and C: written, like `kills`** (piece 5, confirmed). A `Mutex`, atomic or `#[thread_local]` static needs nothing written. Asked the same day whether `Send`/`Share` alone covers it: they judge the static, but a static is not an argument, so a `join!` cannot see it without something in the signature |
| | **What is written: that the function runs on the main thread only**, the user's revision of the same day, in place of naming what it touches (`uses FRAME_COUNT`, `uses raylib`). A function that touches a plain static, or calls C its recipe does not mark thread-safe, is main-thread only; so is any function that calls one, carried up as `kills` is and written by `dz fix`; an entry point is on the main thread already. It is part of a function type, as a `kills` clause is, so such a function becomes a value only of a type that says it too, and never reaches `join!`, `for_each!` or `spawn`. Simpler than a list, and exactly right for raylib and SDL, which need the main thread, not merely one thread at a time; the price is that a function touching one plain static cannot run in a `join!` even when nothing else there touches it. Spelled **`#[main_thread]`**, the user's choice: `#[main_thread] fn draw_hud()`, and on a function type `#[main_thread] fn(w: &World)` |
| | **`ref A`**, a type parameter that takes `&T` or `&mut T`; **`A::Target`**, the type it points at; **`#[parallel]`**, `std`'s marker on `join!` and `for_each!` that switches the checker to the stricter rule |
| **Agreed, 2026-11-24** | **A `drop` may be `#[main_thread]`**, the user's answer when `examples/sdl3`'s `Renderer`, which frees GPU resources with OpenGL, would otherwise need `unsafe`: "drop may need to call those types", and such a type is neither `Send` nor `Share`. Built with it, as a proposal of the same day: a function that makes such a value or takes one by value is `#[main_thread]` too, or a worker could build one from its defaults and drop it there. **A static needs no mark when its type synchronises** (`Mutex`, an atomic), read so because a plain `u64` is `Share` and §22.5's own `FRAME_COUNT += 1` must need the mark; which types synchronise is `std`'s private trait, `Synchronises`. Both are in §22.5 (r150) |
| **Open** | closures (D-0112), the last question before D-0096 is decided |

**What it would change, once decided:** §1.1 (the promise loses "on one thread"), §6.7
(frozen ties, `join`'s argument rule, ties per type), §7.7 (plain statics main-thread
only), §8 (`ref` parameters and the pointed-at type), §14.5 (`Arc`, handles to trait
objects), §14.6 (worker pools, lent containers), §15 (thread-safe imports).
