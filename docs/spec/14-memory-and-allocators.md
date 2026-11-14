# 14. Memory and allocators

> **Ordinary code names no allocator (D-0091, §14.6).** The compiler places memory, and no
> constructor in `std` takes an allocator (r104). What follows is the `Allocator` trait and
> the standard allocators beneath that: what a container, `std`'s or a program's own, takes
> its memory from, in `unsafe` code.

Dazzle has no global allocator baked into the language. Allocation is a library concern
expressed through the `Allocator` trait (§7.6), and **which allocator a value's memory
comes from is the compiler's choice** (§14.6). A container stores the one it was placed
in, which it reads as `#placement` (§12.7). `[DECIDED — D-0091, superseding §18.10]`

```dz
pub trait Allocator {
    fn alloc(&mut self, size: usize, align: usize, loc: SourceLocation = #location) -> ^mut u8
    fn realloc(&mut self, p: ^mut u8, old_size: usize, new_size: usize, align: usize,
               loc: SourceLocation = #location) -> ^mut u8
    fn free(&mut self, p: ^mut u8, size: usize, align: usize, loc: SourceLocation = #location)
}

pub struct List<T> {
    m_allocator: ^mut Allocator = Allocators::Global

    List(capacity: usize = 0, loc: SourceLocation = #location) {
        m_allocator = unsafe { #placement }   // where the compiler put this list
        ...
    }
}
```

- **`&mut self`**: an arena changes on every allocation. `[DECIDED — §18.47]`
- **Sized free**: `free` is told the size and alignment the allocation was made with, so an
  allocator needs no header to remember them, and tracking can check them.
- **`loc`** is the caller's location (§12.7). A container forwards the one it was given, so a
  report names the line that built the container rather than the container's own code.
- `^mut Allocator` is a fat trait pointer (§7.6), which is what lets one `List<f32>` work
  with any allocator.

**Memory comes only from an allocator.** The language has no `alloc` or `free` of its own.
Underneath `Allocators::Global` is the C heap: the standard library imports `stdlib.h` as a
C library of its own (§15.1) and calls its `malloc` and `free`, panicking when `malloc`
returns null (§9.8). Nothing outside `std` can name that import, so a program allocates
through an `Allocator`. A program that wants C's `malloc` itself imports `stdlib.h` with a
recipe of its own. `[DECIDED — D-0084]`

## 14.1 Why the compiler chooses, rather than a parameter

Odin passes the allocator as a parameter: a procedure that allocates takes `allocator :=
context.allocator`, defaulting to the allocator in the implicit `context` every procedure
receives, which a scope can change for everything it calls. Dazzle first took the same
shape without the context: an explicit parameter defaulting to the heap, so `List(8)` read
cleanly and `List(8, &frame_arena)` was there when needed (§18.10). Either way the choice is
the caller's, and nothing checks it: a value built in a frame arena and kept past the
arena's `reset()` dangles.

D-0091 replaced the parameter in r104. The compiler chooses where memory comes from, ties
make every choice safe, and principle 1 (§1) is kept by showing the choice instead of
writing it: the editor marks each allocation with where it went, and `dz placements` lists
them.

## 14.2 Standard allocators

```dz
Allocators::Global      // &mut Allocator — the C heap
Allocators::Temp        // &mut TempArena — this thread's scratch arena
```

Both are associated constants (§7.3): references to statics (§7.7), usable directly —
`Allocators::Temp.reset()` — and converting to a `^mut Allocator`. No container takes
either: the compiler places a container (§14.6), and `Global` is the heap it falls back to.
They are what raw memory is taken from in `unsafe` code.

- **`Global`** is `malloc` and `free`, with nothing of its own in front of them: the C heap is
  already a good general-purpose allocator, and the platform's tools understand it. An
  alignment stricter than the C heap guarantees is over-allocated, with the heap's own
  pointer kept just before the one handed out.
- **`Temp`** is a pointer bump inside a chunk taken from the heap, a new chunk when one
  fills, and **freeing does nothing**. `reset()` keeps the first chunk and returns the rest;
  every pointer it handed out is dangling afterwards. `realloc` of the last allocation grows
  in place. `used()` says how much of the current chunk a frame took.

What a frame builds and drops needs no `Temp` of the program's own: the compiler puts it in
the region of the function that builds it, released when that function returns (§14.6).

```dz
fn update(&mut self, dt: f32) {
    let visible = self.cull(&camera)    // in `update`'s region: freed at its end, no bookkeeping
    for &e in visible { e.draw(&mut self.batch) }
}
```

**Memory tracking** is off unless a build turns it on — `dz run --track-memory`, or
`[build] track_memory = true` in the root manifest — and costs nothing when off: it is the
standard library's `TRACK_MEMORY` flag (§12.10), and the tracker does not exist without it. With it:

- every `Global` allocation is recorded with its size, alignment and `loc`;
- **a leak** — still live when the program ends — is reported with the line that allocated it;
- **a double free** panics, naming where it was allocated and where it was first freed;
- **a free the allocator never handed out**, including a `Temp` pointer freed through `Global`
  or the reverse, panics;
- **a free whose size or alignment differs** from the allocation panics;
- **`Temp.reset()` poisons** what it releases with `0xDD`, so a stale read shows.

**Checking std's trusted clauses.** `std`'s containers write their `kills` clauses by hand
over `unsafe` code, and §6.7's checker trusts them. A debug build tests the half that costs
nothing: a function whose clause leaves a `&mut` input's pointer tie out (`@items`) traps if
the pointer changed by the time it returns — `` `List::push` killed `self@items`, which its
clause does not name ``. `[build] check_trusted = true` adds the other half, for test runs:
on entry it finds every tracked block the input's elements own, by following the words that
point at a live block's start, and on return it traps if one of them was freed or handed out
again, unless the clause kills `[*]`. Each allocation is numbered, so a block freed and
reused at the same address is not taken for the same one. It walks every element on every
call and turns tracking on, so it is for tests, not for builds a person plays.

**`List<T>`** is the growable array, in `std`. `List<i32>()` is empty and allocates
nothing; `List<T>(capacity)` has room for `capacity` elements. It doubles when it grows;
`push`, `pop` (a `T?`), `insert`, `remove`, `swap_remove`, `remove_range(from, to)`, `clear`,
`truncate`, `resize` (new elements at their defaults), `reserve`, `shrink_to_fit`, `len`,
`capacity`, `is_empty`; `retain(keep)` keeps the elements a `fn(&T) -> bool` says yes to,
in order, and drops the rest; `append(&mut other)` moves `other`'s elements onto the end;
`reverse`; `b[i]` reads and writes, checked against the length; `as_slice()` and
`as_mut_slice()` view the elements; `first()`, `last()`, `first_mut()` and `last_mut()`
are a `^T`, `null` when the list is empty, as a map's lookup is (§9.1). It is move-only
unless `T` is copyable, and then a copy copies into a new allocation, wherever the copy is
placed (§14.6).
Every call that may allocate takes `loc = #location`, so tracking names the line that
pushed.

What needs more of the element than the list does is a trait implemented for `List<T>`
with that bound, since a method cannot require more of `T` than its type does — as `sort`
is (§14.4). **`Search<T>`**, for an `Eq` element: `contains(&v)`, and `index_of(&v)`, a
`usize?`. **`Extend<T>`**, for a `Copy` one: `extend(xs)` and `insert_slice(i, xs)` copy a
slice in — one that must not be a view of the list itself, which growing would free — and
`fill(v)` overwrites every element. A list of `Eq` elements is `Eq`, and of `Hash`
elements `Hash`, so a list can key a map. `[DECIDED — D-0081]`

**`String`** is owned UTF-8 text, in `std`: `String("x")` or `String::from("x")` copy a
`&str`. `push_str`, `push` (a `char`, encoded),
`insert(at, c)`, `insert_str(at, s)`, `remove(at)` (the character there), `clear`,
`reserve`, `len` (bytes), `capacity`, `is_empty`; `as_str()` borrows the text and
`as_cstr()` borrows it as a C string, since the bytes are always followed by a zero. `==`
compares text, and a copy copies it. `format!("…", args)` builds one from a format string
(§12.9). **Text of up to 15 bytes is kept inside the `String`**, allocating nothing, as
`std::string` keeps it; longer text is where the compiler places the string (§14.6). Short
text moves with its `String`, so a view of it dies when the string moves, and when a list
holding it grows, sorts or swaps (§6.7). `[DECIDED — D-0095]`

**`HashMap<K: Hash + Eq, V>`** maps keys to values, in `std`. `HashMap<K, V>()` is
empty and allocates nothing; `HashMap<K, V>(capacity)` has room for `capacity` entries.
`insert(k, v)` adds or replaces, dropping a replaced value; `replace(k, v)` returns it as a
`V?` instead. A lookup takes the key by reference and follows §9.1 — `get(&k)` is a `^V`
and `get_mut(&k)` a `^mut V`, `null` when the key is absent, so
`let hp = health.get(&id) or return`.
`remove(&k)` returns the value as a `V?`; `contains`, `len`, `is_empty`, `capacity`,
`clear`, `reserve`. `get_or_insert(k, default)` is the value for `k` as a `&mut V`, after
adding `default` when `k` was not there — counting in one lookup. `retain(keep)` drops the
entries a `fn(&K, &mut V) -> bool` says no to, asking each once. A loop visits every entry:
`for k, &v in m`, or `for k, &mut v in m` to change the values (§10.4) — keys stay
read-only — and `keys()`, `values()` and `values_mut()` visit one side. By slot works too:
`for s in 0..m.slot_count()` with `m.key_at(s)`, `m.value_at(s)` or `m.value_at_mut(s)`,
each `null` for an empty slot. The order is the table's, which is no particular one. It is
move-only unless `K` and `V` are both copyable. It is a SwissTable: a control byte per slot
holds seven bits of the hash of the entry there, a lookup compares eight of them at once
and a key only where they match, and the table doubles at seven eighths full. Removal may
leave a marker that later probes pass, cleared when the table is next rebuilt.

A map with `String` keys is also looked up by `&str`, with no `String` built to do it:
**`StrKeys<V>`**, implemented for `HashMap<String, V>` alone, gives `get_str`,
`get_mut_str`, `contains_str` and `remove_str`. A `String` hashes as its text, so the
`&str` finds the entry its `String` would.

```dz
let n = counts.get_or_insert(word, 0)
n += 1
let tex = textures.get_str(name) or return
```

**`HashSet<T: Hash + Eq>`** holds each value once, in `std`, on the same table as a
`HashMap`: `HashSet<T>(capacity)`, `insert(v)` (whether `v` was new — when it
was not, the set is unchanged and `v` dropped), `contains(&v)`, `remove(&v)` (whether it was
there), `retain`, `clear`, `reserve`, `len`, `is_empty`, `capacity`, and `for &v in set`. A
`HashSet<String>` also answers `contains_str` and `remove_str` (**`StrSet`**). It is
move-only unless `T` is copyable. `[DECIDED — D-0081]`

**`BinaryHeap<T: Ord>`** is a priority queue, in `std`: `pop()` takes out the **largest**
element as a `T?`, as C++'s `priority_queue` and Rust's `BinaryHeap` do, and `peek()` is a
`^T` to it, `null` when the heap is empty. `BinaryHeap<T>(capacity)`, or
`BinaryHeap<T>(items)` to build one from a `List<T>` in place; `push(v)`, `len`,
`is_empty`, `capacity`, `clear`, `reserve`, and `as_slice()`, the elements in the heap's
own order. Equal elements come out in no particular order. It is move-only unless `T` is
copyable. For the smallest first, each element goes in a **`Reverse<T: Ord>`**, whose one
field, `value`, it orders the other way round: `BinaryHeap<Reverse<Node>>`, with
`Reverse<Node>{ value: n }` pushed and `.value` read from what `pop` gives. A `Reverse` is
copyable when `T` is. `[DECIDED — D-0098]`

```dz
let mut open = BinaryHeap<Reverse<Node>>()
open.push(Reverse<Node>{ value: start })
let node = (open.pop() or return).value      // the smallest
```

**`Deque<T>`** is a double-ended queue, in `std`: a growable ring buffer, so taking from
the front costs what taking from the back does, where a `List` moves every element.
`Deque<T>(capacity)`; `push_back(v)` and `push_front(v)`; `pop_back()` and `pop_front()`,
each a `T?`; `front()` and `back()`, each a `^T`, and `front_mut()` and `back_mut()`, a `^mut
T`, `null` when the queue is empty (§9.1); `d[i]`, the `i`th element from the front,
checked against the length; `len`, `is_empty`, `capacity`, `clear`, `reserve`, and `for &v
in d`, front to back. Its elements wrap round its buffer, which `tie [*] = m_data[*]`
already covers, since `[*]` on a pointer field names the whole array (§6.7). It is
move-only unless `T` is copyable; a copy's elements start at the front of its own buffer.

```dz
let mut frontier = Deque<Cell>()
frontier.push_back(start)
while true {                                       // a breadth-first search
    let c = frontier.pop_front() or break
    ...
}
```

**`SlotMap<T>`** holds values that are added and removed in any order and named by a
**`Key<T>`**, in `std`. It is what to use where a struct would keep a reference to
something it does not own — an entity, a node, a connection — which a field cannot be
(D-0003). `insert(v)` returns the value's key. `get(k)` is a `^T` and `get_mut(k)` a
`^mut T`, `null` once the value is removed (§9.1), so `let e = enemies.get(target) or
return`; `remove(k)` returns the value as a `T?`; `contains`, `len`, `is_empty`,
`capacity`, `clear`, `reserve`. `retain(keep)` drops the values a `fn(Key<T>, &mut T) ->
bool` says no to, asking each once. `for k, &v in m` and `for k, &mut v in m` visit every
value with its key; `values()` and `values_mut()` are the values alone as a slice, and
`key_at(i)` is the key of `values()[i]`. The values sit together in one array in no
particular order — removing one moves the last into its place, which no key notices. It is
move-only unless `T` is copyable; a copy's keys are the original's.

A `Key<T>` is a slot index and that slot's version: eight bytes, copyable, `Eq`, `Hash` and
`Format` (`3v1`), so it can be kept in a field, key a `HashMap` and be written to a file.
It owns nothing. Removing a value moves its slot's version on, so every key to it reads as
absent from then on — also after the slot holds a new value, until the version comes round
again, after 2³¹ reuses of that one slot. `T` is checked: a `Key<Enemy>` is not a
`Key<Bullet>`. `Key<T>()` is the null key, which no map holds, and `is_null()` asks for it;
`Key<T>(index, version)` rebuilds a key from what `index()` and `version()` read.
`[DECIDED — D-0083]`

```dz
struct Turret {
    pub target: Key<Enemy> = Key<Enemy>()
}

fn fire(t: &Turret, enemies: &mut SlotMap<Enemy>) {
    let e = enemies.get_mut(t.target) or return     // gone: dead, or never set
    e.hp -= 10
}
```

**`Hash`** is the `std` trait a key needs: `fn hash(&self) -> u64`, equal values hashing
equally. Integers, `bool`, `char` and `&str` implement it, and `Eq`, without an `impl`, and
the methods can be called on them directly — `id.hash()`, `name.eq(&other)`, `a.cmp(&b)`.
Floats do not implement `Hash`, since `NaN != NaN` would break that promise. `String`
hashes as its text. A struct combines its fields' hashes with `hash_combine(seed, h)`.

A reference to a value that is not a place — `m.get(&7)`, `&make()` — refers to a
temporary holding the value until the end of the enclosing block, and the literal takes its
type from what the reference is wanted to point at.

**A container's elements live in its home** (§6.7): what `push` or `insert` is given is
built where the container is, and what `pop` or `remove` hands back is still there, which
their `sends` and `in self@home` say. A container keeps the allocator it was placed in, which
is how a function lent one builds into it (§14.6). `[DECIDED — D-0097]`

Other allocators — an arena, a pool, a free list — are ordinary Dazzle structs implementing
the trait, which a program's own container can take memory from in `unsafe` code. Asking the
compiler to place values in one, under a memory budget or with no heap, is open (T-0143).
An allocator that can grow or shrink a block without moving it says so with
`resize_in_place(p, old_size, new_size, align) -> bool`, `false` by default: an arena can
for its last allocation, by moving its bump. It is how a `List` of values that cannot move as
bytes grows in a region without copying them.

## 14.3 Placement primitives

Container authors need to construct, destroy and move values in raw memory. Three
intrinsics:

```dz
construct(dst, args...)   // in-place construct dst from args (or copy-construct from a value)
destroy(dst)              // run dst's drop, leave the storage raw
move_out(dst)             // the value, moved out; the storage is left raw, as after destroy
```

They compile to C++ placement `new`, an explicit destructor call, and a move followed by
one. They are the only place in the language where a value can be observed in a
not-yet-constructed state, so all three are `unsafe` (§5.2), as are `memcpy` and `memmove`.
`move_out` is how `List::pop` returns an element, and how growing a list of move-only
elements carries them to the new memory. `[DECIDED — §18.16, §18.50]`

Beside them, two that copy bytes: `memcpy(dst, src, n)` copies `n` bytes between places
that do not overlap, and `memmove(dst, src, n)` between places that may — what shifts a
list's tail along by a slot. Both are `unsafe`, and `dst` must be a pointer that may be
written through. Copying bytes is moving only for a type whose bytes may be relocated,
which `T::is_trivially_copyable` answers; `List` shifts its elements with one `memmove`
when it is, and with `construct` and `move_out` one at a time when it is not.
`[DECIDED — D-0081]`

## 14.4 The standard library: `std`

The standard library is **one module, `std`**, carried by the compiler as Dazzle source:
the language's traits (`Drop`, `Copy`, `Eq`, `Ord`, the operators, `Iterate`, `Format`,
`ImplicitAs`), the allocators, the containers and the math below. Its names are flat — `std::String`,
`std::List`, `std::Vec2` — with one declaration to a file, as any project may organise
itself (§3.1). A module names it in its manifest like any dependency, and usually opens it
(§3.2):

```toml
[dependencies]
std = { open = true }       # String, Vec2, Allocators — and std::Vec2 where needed
```

Every declaration the language itself relies on lives here, and the compiler reaches those
without the program naming them: a `for` over a `List` or a `println!` of a `Vec2` works in a
module that does not list `std`, which simply cannot write `List` or `Vec2` itself. Only what
a program uses is instantiated and emitted, so a flat, always-present library costs a
program nothing it does not call.

The name `std` is reserved: a project named `std`, or a dependency key `std` with a `path`,
is an error. `[DECIDED — §18.57, superseding §18.23 and §18.55]`

**The math** holds:

- **Constants** `PI`, `TAU`, `E` (`f32`) and `PI_F64`.
- **`Float`**, implemented by `f32` and `f64`, and **`Integer`**, implemented by every integer
  type, both without an `impl`. Their methods are called on the number: `x.sqrt()`,
  `angle.sin()`, `t.lerp(to, 0.5)`, `hp.clamp(0, max)`, `seed.wrapping_mul(31)`,
  `a.checked_add(b)` (a `T?`), `n.saturating_sub(1)`. Plain integer arithmetic wraps on
  overflow (§5.1) — `hash_combine` relies on it; these methods are how a program asks for
  anything else. A method
  of the wrong kind of number is an error that says which kind has it.
- **`Number`**, implemented by every integer and float type, and required by both `Integer`
  and `Float`: the bound for code that works on either kind. `x.to_f64()`, and
  `lo.uniform(hi, bits)`, a value in `[lo, hi)` chosen by 64 random bits — what `Random`
  is built on, and what any other generator can use.
- **Vectors** `Vec2`, `Vec3`, `Vec4` of `f32`, and **`Vec2i`**, **`Vec3i`** of `i32`, with
  `+`, `-`, unary `-`, `==`, `* scalar` and `/ scalar` (component-wise `*` for the float
  ones), and methods: `dot`, `length`, `normalized`, `distance`, `lerp`, `min`, `max`,
  `cross` and `reflect` on `Vec3`, `perp`, `angle` and `rotated` on `Vec2`. The integer
  vectors are `Hash`, so a grid position keys a `HashMap`.
- **`Quat`**, a rotation, identity at its defaults: `Quat::from_axis_angle`,
  `Quat::from_euler`, `rotate` (also `q * v`), `q * q`, `normalized`, `conjugate`, `slerp`,
  `to_mat4`.
- **`Mat4`**, **column-major** and acting on column vectors: `m * v`, `projection * view *
  model`, `cols[3]` the translation — the OpenGL, GLSL and glm convention, right-handed.
  `Mat4::identity`, `translation`, `scaling`, `rotation`, `perspective` and `orthographic`,
  `look_at`, `transform_point`, `transform_vector`, `transpose`, `determinant`, and
  `inverse` (a `Mat4?`, `none` when singular). Its layout is what OpenGL, Vulkan, Direct3D,
  Metal and SDL3's GPU API read as it is. raylib's `Matrix` is the same matrix stored in
  the transposed order, so converting to it is a copy field by field, never a cast.
- **`DepthRange`**, the last argument of `perspective` and `orthographic`, with no default:
  `.MinusOneToOne` for OpenGL and raylib, `.ZeroToOne` for Vulkan, Direct3D, Metal and
  SDL3's GPU API. The wrong one still draws, with geometry near the camera clipped away, so
  it is written at every call; a program fixed to one API names it once in a `const` or
  picks it with `#if` (§12.10). `[DECIDED — D-0086]`

Methods rather than free functions throughout, since there is no overloading (§18.5) and
one name must work across `f32`, `f64` and every vector: `a.lerp(b, t)` is the same call on
each. There is no generic `Vec3<T>`; the associated-type projection one would need now
exists (§8.5).
`[DECIDED — §18.55]`

**Time** holds:

- **`Duration`**, a span to the nanosecond that is never negative: `Duration::from_secs`,
  `from_millis`, `from_micros`, `from_nanos`, `from_secs_f32`/`f64`; `as_secs`, `as_millis`,
  `as_micros`, `as_nanos`, `as_secs_f32`/`f64`, `is_zero`; `+`, `-` (stopping at zero), `==`,
  the comparisons, and `Hash`.
- **`Instant`**, a point on a monotonic clock, which never goes backwards: `Instant::now()`,
  `elapsed`, `since`, `later - earlier` giving a `Duration`, `instant + duration`, `==` and
  the comparisons.
- **`Time`**: `Time::sleep(d)`, at least `d` and as much longer as the operating system
  chooses, and `Time::unix()`, the wall clock as a `Duration` since 1970-01-01 UTC.

```dz
let start = Instant::now()
update(world)
let dt = start.elapsed().as_secs_f32()
```

Like the math, these are associated functions and methods of types, not free functions.

**Reading text.** The `Text` trait, implemented for `&str` and `String`, gives both the same
operations, literals included: `starts_with`, `ends_with`, `find` and `rfind` (a byte
offset, `usize?`), `contains`, `trim`, `trim_start`, `trim_end`, `strip_prefix` and
`strip_suffix` (a `^str`, `null` when the text does not start or end so), `split_once(sep)`
(the text before and after the first `sep`, a `(&str, &str)?`), `split(separator)`,
`split_whitespace()` and `lines()` (iterators of `&str` views), `eq_ignore_ascii_case`,
`replace(from, to)`, `to_ascii_lowercase`, `to_ascii_uppercase`, `repeat(n)` (new
`String`s), and `parse_i64`, `parse_u64`, `parse_f64`, `parse_f32` — the whole text as a
number, or `none` for anything else, including surrounding spaces and a value that does not
fit. Pieces are views of the text they came from, never copies. **`Join`** writes pieces
with a separator between them into a new `String`: `names.join(", ")` on a `&[]&str`, a
`List<const &str>` or a `List<String>`.

```dz
let text = File::read_text(&config) or return
for line in text.lines() {
    if line.trim().starts_with("#") { continue }
    for field in line.split("=") { … }
}
let volume = setting.trim().parse_f32() or 1.0
```

`[DECIDED — §18.71, D-0081]`

**Sorting.** `xs.sort()` orders a `List` of anything `Ord` by `<` — through the `Sort`
trait — and `xs.sort_by(before)` by any strict order, a `fn(&T, &T) -> bool`; `xs.swap(i,
j)` exchanges two elements. The sort is an introsort, as `std::sort` is: quicksort, with
insertion sort for short runs and a heapsort once quicksort splits badly. It allocates
nothing, is `n log n` at worst, and is not stable. `String` and `Path` are `Ord` by their
bytes, which for UTF-8 is code-point order, not a language's alphabetical one. `[DECIDED —
§18.70]`

**`Random`** is one generator per thread, called through its type with no value to hold:
`Random::next_u32()`, `next_u64()`, `next_f32()` and `next_f64()` (the floats in `[0, 1)`),
`Random::range(lo, hi)` for any `Number` in `[lo, hi)` — `Random::range(1, 7)` is a die,
`Random::range<f32>(-1.0, 1.0)` a float — `Random::chance(p)`, and `Random::unit_vec2()` and
`unit_vec3()`, directions spread evenly. It seeds itself from the clocks on first use, so
runs differ; `Random::seed(n)` makes this thread's numbers repeat exactly. The generator is
SplitMix64: fast and well distributed, and not for cryptography. `[DECIDED — §18.67]`

**Text is UTF-8.** A `&str` views UTF-8 bytes and a `String` owns them; `len` counts bytes,
and a character outside ASCII takes two to four. Characters are decoded as they are read:

- `s.chars()` — or `for c in s` on a `String`, or `Chars(text)` on any `&str` — steps
  through the characters as `char`s. A byte that is not valid UTF-8, which only the
  unchecked `bytes as &str` can produce, reads as U+FFFD.
- `char_count()`, `pop()` (the last character, a `char?`), `is_char_boundary(i)`,
  `slice(from, to)` (a `^str`: `null` when an end falls inside a character) and
  `truncate(len)` (a panic inside one), so text is never cut into invalid bytes.
- `insert(at, c)`, `insert_str(at, s)` and `remove(at)` (the character starting there)
  change a `String` in place, and panic when `at` is not where a character starts.
- `String::from_utf8(bytes)` copies bytes that are valid UTF-8, `none` otherwise.
- A `char` formatted with `{}` is written as UTF-8, as a `&str` is.

A character's position is in bytes throughout; there is no indexing by character, which
would take time in proportion to the text's length on every use. `[DECIDED — §18.69]`

**A C string from text.** A `&str` has no zero after it, so it is not a `&cstr`; a literal
is, as a `const &cstr` (§5.3). A `String` keeps a zero after its bytes, and `as_cstr()`
borrows the text as a `&cstr` tied to the `String`, with no copy — so text from anywhere
reaches a C function through one. Passed as an argument, a `String` or a `Path` *is* one,
through `std`'s `ImplicitAs<&cstr>` (§7.2):

```dz
let owned = String(name)
rl::LoadFileText(owned)                        // the same as owned.as_cstr()
rl::DrawText(format!("hp {}", hp), 8, 8, 20, rl::RED)
```

`rl::LoadFileText(String(name).as_cstr())` is the same, since a temporary lives to the
end of its statement; binding that `&cstr` with `let` keeps a view of a temporary, which is an
error (`DZ0407`, §6.5). `[DECIDED — D-0039, D-0081, D-0089]`

**Files** are reached through three types, and no free functions:

- **`Path`**, a place in the file system, owning its text. `Path("assets") / "hero.png"`
  and `join` build one; a `\` becomes `/` and a trailing `/` is dropped, so the same text
  works on every platform, and an absolute part (`/…`, `C:/…`) replaces what it is joined
  to. `file_name`, `stem`, `extension`, `parent`, `with_extension`, `is_absolute`, `as_str`
  and `as_cstr` (for a C API) read it without touching the disk; `exists`, `is_file`,
  `is_dir` and `modified` (the last write, as a `Duration` since the Unix epoch) ask the
  disk. `==`, `<` and `Hash` compare the text.
- **`File`**, an open file: move-only, closed when dropped. `File::open(&path, mode)` with a
  `FileMode` of `.Read`, `.Write` (created or emptied), `.Append` or `.ReadWrite`; then
  `read(buf)` (0 at the end), `write(data)`, `seek(position)`, `position()`, `size()`,
  `flush()`. The whole-file functions are what most code wants: `File::read_bytes`,
  `File::read_text` (UTF-8, checked), `File::write_bytes`, `File::write_text`, and
  `File::remove`.
- **`Directory`**: `Directory::create` (with any missing parents; one that exists is fine),
  `Directory::remove` (empty), and `Directory::list`, the paths directly inside, sorted by
  their text so the order is the same on every platform.

```dz
let save = Path("saves") / "slot1.txt"
Directory::create(&save.parent()) or return
File::write_text(&save, state) or { e -> eprintln!("could not save: {}", e.message) }
let text = File::read_text(&save) or { e ->
    if e.code != IoError::NotFound as u32 {
        eprintln!("could not load: {}", e.message)
    }
    String()
}
```

Every call that touches the disk is fallible (§9), and its error's `code` is an `IoError`:
`NotFound`, `PermissionDenied`, `AlreadyExists`, `InvalidData` or `Other`. Paths are UTF-8
everywhere, including on Windows. `[DECIDED — §18.68]`

**The process**: `Process::args()`, the command line as a `List<String>` with the program
first, in UTF-8 on every platform; `Process::env(name)`, a variable as a `String?`; and
`Process::exit(code)`, which flushes output and ends the program at once, without dropping
what is still alive — returning from `main` is the normal way to end.

## 14.5 Handles: `Rc`, `Weak` and `Box`

Game code shares things — a texture drawn by many sprites, an entity several systems
track — and a raw pointer is the wrong tool for it (§5.2). The standard library has three
**handles**, each holding a value in an allocation of its own, as `List` holds its elements:

| Handle | Owns | Copying it | The value is dropped |
|---|---|---|---|
| `Rc<T>` | a share | shares the value, counting one more owner | with the last `Rc` |
| `Weak<T>` | nothing | refers to the same value | never by a `Weak` |
| `Box<T>` | all of it | not allowed: move-only | with the `Box` |

Many values of one kind, made and removed by one owner and referred to from everywhere —
the entities of a level, the nodes of a graph — are better kept in a `SlotMap` (§14.4).
Its `Key` is a plain eight-byte value with no count to keep, the values sit together, and
one dies when its owner removes it rather than when the last `Rc` to it goes.

```dz
let tex = Rc<Texture>(load("hero.png"))       // on the heap: a Weak can outlive any scope
let other = tex                                // tex.count() is now 2
let watch = Weak<Texture>(&tex)                // does not keep it alive
let node = Box<Node>(Node{ value: 1 })
```

**A handle may be `null`**, referring to nothing, exactly as a pointer may. `null` is every
handle's default and what a moved-from one is left as, so a field of a handle type is
written `texture: Rc<Texture> = null`. `h = null` releases this handle — for an `Rc`, one
share, and the value only if it was the last. A `Weak` is also `null` once no `Rc` owns
its value: dropping the last `Rc` makes every `Weak` to it read as `null` at once, which is
how an asset owner unloads something others still refer to.

**Its value is reached as a pointer's is**, and never with a bare `.`: `h == null` and
`h != null` test it, `h or …` gives a reference to the value (§9.4), and `if let` binds it
with `v`, `&v` or `&mut v`. `&mut` needs the handle itself in a writable place.

```dz
if let &mut t = sprite.texture { t.width = 64 } else { use_placeholder() }
let target = self.target or return             // a Weak: `return` if the entity is gone
```

**While the value is used, it cannot be freed.** `or` and `if let` on an `Rc` or a `Weak`
first take a strong `Rc` of their own, held until the `if let` block ends, or for `or` until
the enclosing block does. Whatever the code does meanwhile — sets the handle to `null`, or
removes the last other `Rc` from a map — the value outlives the reference:

```dz
if let &mut t = self.texture {
    self.texture = null        // releases this handle; the value is still held
    t.width += 1               // fine
}                              // dropped here, if that was the last owner
```

That costs one count up and one down per `or` or `if let`: plain integers, not atomics.

**A `Box`'s value is in the box**, its `@value` (`tie @value = m_ptr[*]`). A `Box` has no
second owner to take, so a reference `or` or `if let` takes to its value is a view in
`self.node@value`, and emptying, replacing or moving the box ends it, as assigning a
`String` ends a view of its text (§6.7). The mistake is a compile-time error, not freed
memory:

```dz
if let &n = self.node {
    self.node = null           // error DZ0413 below: this write kills `self.node`
    println!("{}", n.name)     // `n` points into `self.node@value`
}
```

A clause names what is inside the value through the tie: a function changing the boxed
node's name declares `kills self.node@value.name@text`. `[DECIDED — D-0094, revising
§18.62]`

**Any two `Rc<T>`s may hold one value**, and nothing at compile time says which do, so the
ties check (§6.7) takes the value every `Rc<T>` and `Weak<T>` holds as one place. A view
through one is ended by a kill through another, and a call given two of them, one to
change, is refused as a call given one list twice is:

```dz
if let &mut dst = a {
    if let &src = b {                  // `a` and `b` are both `Rc<List<String>>`
        append_copy(&mut dst, &src)    // error: `src` may be the list `dst` grows
    }
}
```

It costs nothing at run time. The price is a rare refusal: a view into one `Rc<Texture>`'s
value kept across a kill through a different `Rc<Texture>`, fixed by copying out what is
kept. Handles of different types, an `Rc<List<String>>` and an `Rc<List<i32>>`, are apart.
`[DECIDED — D-0094]`

**`Nullable`** is the trait behind this, and any type may implement it: `is_null`,
`guard` — what `or` and `if let` hold for the duration, itself `Nullable` — and `ptr`, the
value's address or `null`. `Rc` and `Weak` guard with an `Rc`; `Box` with an `Unguarded`,
which holds nothing.

**A handle to a derived struct converts to a handle to its base**, as a reference does
(§7.3): an `Rc<Button>` is accepted where an `Rc<UiElement>` is wanted, sharing the same
count, and likewise `Weak` and `Box`. `[DECIDED — §18.64]`

Not yet: `Arc<T>`, which needs atomics (§18.11); and a handle to a trait object.
`[DECIDED — §18.62]`

## 14.6 Placement by the compiler

**Ordinary code never names an allocator.** No constructor takes one, no function passes
one on, and `Allocators::Temp` is not for user code. The compiler puts each value in the
most efficient place it can prove safe, the heap last. `[DECIDED — D-0091]`, superseding the
explicit allocator parameter (D-0010, §14.1) and `Allocators::Temp` in user code (D-0047).

| The value | Its memory |
|---|---|
| never leaves its scope | that scope's **region**, released in one step when the scope ends |
| is returned | **wherever the caller wants the result**, passed to the function out of sight |
| is the contents of a container emptied only all at once | an **arena that container owns**, reset when it is cleared (not yet: T-0120) |
| anything else | **the heap** |

- **A placement is checked like a reference** (§6.7): a region is a tie that dies when its
  scope ends, so a placement that would leave a value in memory that dies too soon fails the
  check, and the value goes to the heap. No placement changes what a program does.
- **A value's owned parts live where the value lives.** A constructor and `copy` build into
  wherever `self` is placed.
- **A copy is a value of its own**, placed like any other, never where the value it copies
  lives: `let names = other.names` or `counts.insert(word, 1)` builds its copy in a region
  when it outlives none. A copy made by assignment, or by C++ code the compiler does not see,
  is on the heap. `[DECIDED — D-0091, T-0140]`
- **A value stored into a container lives where the container lives**: what
  `names.push(…)` or `map.insert(…)` is given, when the container is a local of the
  function. Taken back out (`pop`, `remove`) and kept somewhere the region cannot follow, or
  lent with the container to a call whose clauses hand it on, it goes to the heap instead,
  or with the container to a home that outlives where it goes (D-0097).
- **A function lent a container builds into its home**: what it hands a container it was
  lent, by a call whose `sends` puts it there (`names.push(String::from("x"))` in a function
  given `names: &mut List<String>`), is built with the allocator the container keeps, read
  from the container at run time, so it lives wherever the caller put the container. A
  caller that lends a container to a function of the program's own then takes it to hold
  values in its own region, and places it so that what leaves it, by a `sends` or an
  `in x@home` result, outlives nothing: in the region of the outermost block it goes into,
  or the heap. `[DECIDED — D-0097, T-0150]` Built on every pass of a
  loop, it goes there only when the container keeps everything it is given, so the region
  grows no faster than the container: a map does when each insert follows a lookup of the
  same key that found nothing, which replaces nothing. `[DECIDED — D-0091, T-0145, T-0149]`
- **A value assigned to a local lives where the local lives**, as a `let`'s does, and so
  does each part of a tuple `let`. Built on every pass of a loop the local outlives, it goes
  there only when the local grows from itself: every assignment holds the value it
  replaces, as `lhs = Expr::Binary{ lhs: Box(move(lhs)), rhs: Box(move(rhs)) }` does, and
  nothing else drops or gives away what the local holds. Nothing a pass makes then dies
  before the region does, so a parser's nodes go into the region of the statement they
  belong to. `[DECIDED — D-0091, T-0146]`
- **Every function that returns something owning memory** takes where its result goes,
  out of sight: free functions, methods, trait methods and function values alike. A function
  called from C or C++ returns on the heap.
- **Anything passing through code the compiler cannot see** — a C or C++ call, a pointer
  stored in a struct — goes to the heap, as do `Rc`, `Weak` and statics. A `Box`'s
  contents go where the `Box` goes.
- **A drop that only frees is not run** when the region frees everything the value owns:
  the type's drop, proven for each instance, does nothing but give memory back (`String`,
  `List<i32>`, `List<String>`; not `File`, which closes a handle, nor `Rc`), the value was
  built whole in the region, everything put into it since was too, and nothing has replaced
  a part of it. No program can tell. `[DECIDED — D-0091, T-0118]`
- **The choice is shown, not written**: an editor hint on each allocation names where it
  went and why ("heap: popped elements leave `queue`"), and says when its drop is skipped.
- **A memory budget or a program with no heap** is not expressible yet.

**A container reads where it was placed as `#placement`** (§12.7), in `unsafe`: in a
constructor, where the value being made goes; in a `copy`, where the copy goes; in a
function returning something that owns memory, where its result goes; anywhere else, the
heap. `std`'s containers store it and
allocate from it, and a program's own container can do the same, and is then placed as
they are:

```dz
struct Pool {
    m_allocator: ^mut Allocator = Allocators::Global
    m_data: ^mut u8 = null

    Pool() {
        m_allocator = unsafe { #placement }     // this pool's region, its caller's, or the heap
        let a = unsafe { m_allocator or return }
        m_data = a.alloc(4096, 16)
    }
}

impl Drop for Pool {
    fn drop(&mut self) {
        let a = unsafe { m_allocator or return }
        a.free(m_data, 4096, 16)                 // into the allocator it came from
    }
}
```

The compiler checks where the value goes; what is done with the allocator is the
container's to get right, as for anything `unsafe`: memory taken from it is freed into it,
and a copy is built from the copy's own `#placement`, never the original's allocator, since
it may outlive the region its original is in. A parameter's default cannot be
`#placement`, which would make it each caller's (`DZ0579`). `[DECIDED — D-0091, r104,
r105]`

`Allocator`, `Allocators`, `TempArena` and the region arena stay in `std`, for containers,
in `unsafe` code. A container may also take raw memory from C, importing `stdlib.h` itself
(§14, D-0084), as `examples/ownership` does; that memory is C's heap, wherever the
container is placed.

Regions and result placement are on in every build; `DZ_REGIONS=0` builds the same
program with every value on the heap, which prints the same and is there to compare
against. Not yet implemented: owner arenas (T-0120) and a `virtual` method's result
(T-0141) are still to come, as are skipped drops for a value lent as `&mut` or built in a
generic body (T-0147).
