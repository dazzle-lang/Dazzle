# 14. Memory and allocators

Dazzle has no global allocator baked into the language. Allocation is a library concern
expressed through the `Allocator` trait (§7.6), and **the allocator is an explicit
parameter**. `[DECIDED — §18.10]`

```dz
pub trait Allocator {
    fn alloc(&mut self, size: usize, align: usize, loc: SourceLocation = #location) -> ^mut u8
    fn realloc(&mut self, p: ^mut u8, old_size: usize, new_size: usize, align: usize,
               loc: SourceLocation = #location) -> ^mut u8
    fn free(&mut self, p: ^mut u8, size: usize, align: usize, loc: SourceLocation = #location)
}

pub struct List<T> {
    m_allocator: ^mut Allocator = Allocators::Global

    List(capacity: u32, allocator: ^mut Allocator = Allocators::Global) { ... }
}
```

- **`&mut self`**: an arena changes on every allocation. `[DECIDED — §18.47]`
- **Sized free**: `free` is told the size and alignment the allocation was made with, so an
  allocator needs no header to remember them, and tracking can check them.
- **`loc`** is the caller's location (§12.7). A container forwards the one it was given, so a
  report names the line that built the container rather than the container's own code.
- `^mut Allocator` is a fat trait pointer (§7.6), which is what lets one `List<f32>` work
  with any allocator.

Underneath, two builtins reach the C heap and are what `Allocators::Global` is built on:
`alloc(size, align) -> ^mut u8`, which panics on exhaustion (§9.8), and `free(p)`, both
`unsafe`. User code allocates through an `Allocator`.

## 14.1 Why explicit rather than an implicit context

Odin's implicit `context.allocator` removes the parameter from every signature, and the
price is that you cannot tell from a call whether it allocates or from where. Given
principle 1 (§1), the parameter stays. The default-argument mechanism (§7.2) gives back
almost all the ergonomics: `List(8)` reads as cleanly as the implicit version, and
`List(8, &frame_arena)` is right there when you need it.

## 14.2 Standard allocators

```dz
Allocators::Global      // &mut Allocator — the C heap
Allocators::Temp        // &mut TempArena — this thread's scratch arena
```

Both are associated constants (§7.3): references to statics (§7.7), usable directly —
`Allocators::Temp.reset()` — and converting to the `^mut Allocator` a container stores.

- **`Global`** is `malloc` and `free`, with nothing of its own in front of them: the C heap is
  already a good general-purpose allocator, and the platform's tools understand it. An
  alignment stricter than the C heap guarantees is over-allocated, with the heap's own
  pointer kept just before the one handed out.
- **`Temp`** is a pointer bump inside a chunk taken from the heap, a new chunk when one
  fills, and **freeing does nothing**. `reset()` keeps the first chunk and returns the rest;
  every pointer it handed out is dangling afterwards. `realloc` of the last allocation grows
  in place. `used()` says how much of the current chunk a frame took.

```dz
fn update(&mut self, dt: f32) {
    defer Allocators::Temp.reset()

    let visible = self.cull(&camera, Allocators::Temp)   // freed at frame end, no bookkeeping
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

**`List<T>`** is the growable array, in `std`. `List<i32>()` is empty and allocates
nothing; `List<T>(capacity, allocator)` takes both. It doubles when it grows; `push`,
`pop` (a `T?`), `insert`, `remove`, `swap_remove`, `clear`, `truncate`, `resize` (new
elements at their defaults), `reserve`, `len`, `capacity`, `is_empty`; `b[i]` reads and
writes, checked against the length; `as_slice()` and `as_mut_slice()` view the elements.
It is move-only unless `T` is copyable, and then a copy copies into a new allocation from the
same allocator. Every call that may allocate takes `loc = #location`, so tracking names the
line that pushed.

**`String`** is owned UTF-8 text, in `std`: `String("x")` or `String::from("x")` copy a
`str`, with an allocator as a second argument. `push_str`, `push` (a `char`, encoded),
`clear`, `reserve`, `len` (bytes), `capacity`, `is_empty`; `as_str()` borrows the text and
`as_cstr()` borrows it as a C string, since the bytes are always followed by a zero. `==`
compares text, and a copy copies it. `#format("…", args)` builds one from a format string,
taking `allocator = …` by name (§12.9).

**`HashMap<K: Hash + Eq, V>`** maps keys to values, in `std`. `HashMap<K, V>()` is
empty and allocates nothing; `HashMap<K, V>(capacity, allocator)` takes both. `insert(k, v)`
adds or replaces, dropping a replaced value; `replace(k, v)` returns it as a `V?` instead.
A lookup takes the key by reference and follows §9.1 — `get(&k)` is a `^V` and `get_mut(&k)`
a `^mut V`, `null` when the key is absent, so `let hp = health.get(&id) or return`.
`remove(&k)` returns the value as a `V?`; `contains`, `len`, `is_empty`, `capacity`,
`clear`, `reserve`. A loop visits every entry: `for k, &v in m`, or `for k, &mut v in m`
to change the values (§10.4) — keys stay read-only. By slot works too: `for s in
0..m.slot_count()` with `m.key_at(s)`, `m.value_at(s)` or `m.value_at_mut(s)`, each `null`
for an empty slot. It is
move-only unless `K` and `V` are both copyable. Keys are found by linear probing, the table
doubles at three quarters full, and removal leaves no markers behind.

**`Hash`** is the `std` trait a key needs: `fn hash(&self) -> u64`, equal values hashing
equally. Integers, `bool`, `char` and `str` implement it, and `Eq`, without an `impl`, and
the methods can be called on them directly — `id.hash()`, `name.eq(&other)`, `a.cmp(&b)`.
Floats do not implement `Hash`, since `NaN != NaN` would break that promise. `String`
hashes as its text. A struct combines its fields' hashes with `hash_combine(seed, h)`.

A reference to a value that is not a place — `m.get(&7)`, `&make()` — refers to a
temporary holding the value until the end of the enclosing block, and the literal takes its
type from what the reference is wanted to point at.

Additional allocators — `Arena`, `Pool<T>`, `FreeList` — are ordinary Dazzle structs
implementing the trait, and come later.

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
not-yet-constructed state, so all three are `unsafe` (§5.2), as are `memcpy` and `free`.
`move_out` is how `List::pop` returns an element, and how growing a list of move-only
elements carries them to the new memory. `[DECIDED — §18.16, §18.50]`

## 14.4 The standard library: `std`

The standard library is **one module, `std`**, carried by the compiler as Dazzle source:
the language's traits (`Drop`, `Copy`, `Eq`, `Ord`, the operators, `Iterate`, `Format`),
the allocators, the containers and the math below. Its names are flat — `std::String`,
`std::List`, `std::Vec2` — with one declaration to a file, as any project may organise
itself (§3.1). A module names it in its manifest like any dependency, and usually opens it
(§3.2):

```toml
[dependencies]
std = { open = true }       # String, Vec2, Allocators — and std::Vec2 where needed
```

Every declaration the language itself relies on lives here, and the compiler reaches those
without the program naming them: a `for` over a `List` or a `#println` of a `Vec2` works in a
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
  `a.checked_add(b)` (a `T?`), `n.saturating_sub(1)`. Plain integer arithmetic makes no
  promise on overflow; these methods are how a program says what should happen. A method
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
  `Mat4::identity`, `translation`, `scaling`, `rotation`, `perspective` and `orthographic`
  (into OpenGL's clip space), `look_at`, `transform_point`, `transform_vector`, `transpose`,
  `determinant`, and `inverse` (a `Mat4?`, `none` when singular).

Methods rather than free functions throughout, since there is no overloading (§18.5) and
one name must work across `f32`, `f64` and every vector: `a.lerp(b, t)` is the same call on
each. A generic `Vec3<T>` waits on associated-type projection in generic code.
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

**Reading text.** The `Text` trait, implemented for `str` and `String`, gives both the same
operations, literals included: `starts_with`, `ends_with`, `find` (a byte offset, `usize?`),
`contains`, `trim`, `trim_start`, `trim_end`, `split(separator)` and `lines()` (iterators of
`str` views), `replace(from, to)`, `to_ascii_lowercase`, `to_ascii_uppercase` (new
`String`s), and `parse_i64`, `parse_u64`, `parse_f64`, `parse_f32` — the whole text as a
number, or `none` for anything else, including surrounding spaces and a value that does not
fit. Pieces are views of the text they came from, never copies.

```dz
let text = File::read_text(&config) or return
for line in text.lines() {
    if line.trim().starts_with("#") { continue }
    for field in line.split("=") { … }
}
let volume = setting.trim().parse_f32() or 1.0
```

`[DECIDED — §18.71]`

**Sorting.** `xs.sort()` orders a `List` of anything `Ord` by `<` — through the `Sort`
trait — and `xs.sort_by(before)` by any strict order, a `fn(&T, &T) -> bool`; `xs.swap(i,
j)` exchanges two elements. The sort is a heapsort: no allocation, `n log n` at worst, and
not stable. `String` and `Path` are `Ord` by their bytes, which for UTF-8 is code-point
order, not a language's alphabetical one. `[DECIDED — §18.70]`

**`Random`** is one generator per thread, called through its type with no value to hold:
`Random::next_u32()`, `next_u64()`, `next_f32()` and `next_f64()` (the floats in `[0, 1)`),
`Random::range(lo, hi)` for any `Number` in `[lo, hi)` — `Random::range(1, 7)` is a die,
`Random::range<f32>(-1.0, 1.0)` a float — `Random::chance(p)`, and `Random::unit_vec2()` and
`unit_vec3()`, directions spread evenly. It seeds itself from the clocks on first use, so
runs differ; `Random::seed(n)` makes this thread's numbers repeat exactly. The generator is
SplitMix64: fast and well distributed, and not for cryptography. `[DECIDED — §18.67]`

**Text is UTF-8.** A `str` is UTF-8 bytes and a `String` owns them; `len` counts bytes, and
a character outside ASCII takes two to four. Characters are decoded as they are read:

- `s.chars()` — or `for c in s` on a `String`, or `Chars(text)` on any `str` — steps through
  the characters as `char`s. A byte that is not valid UTF-8, which only the unchecked
  `bytes as str` can produce, reads as U+FFFD.
- `char_count()`, `pop()` (the last character, a `char?`), `is_char_boundary(i)`,
  `slice(from, to)` (a `str?`: `none` when an end falls inside a character) and
  `truncate(len)` (a panic inside one), so text is never cut into invalid bytes.
- `String::from_utf8(bytes)` copies bytes that are valid UTF-8, `none` otherwise.
- A `char` formatted with `{}` is written as UTF-8, as a `str` is.

A character's position is in bytes throughout; there is no indexing by character, which
would take time in proportion to the text's length on every use. `[DECIDED — §18.69]`

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
File::write_text(&save, state) or { e -> #eprintln("could not save: {}", e.message) }
let text = File::read_text(&save) or { e ->
    if e.code != IoError::NotFound as u32 {
        #eprintln("could not load: {}", e.message)
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
**handles**, each holding a value on the heap from an allocator, as `List` does:

| Handle | Owns | Copying it | The value is dropped |
|---|---|---|---|
| `Rc<T>` | a share | shares the value, counting one more owner | with the last `Rc` |
| `Weak<T>` | nothing | refers to the same value | never by a `Weak` |
| `Box<T>` | all of it | not allowed: move-only | with the `Box` |

```dz
let tex = Rc<Texture>(load("hero.png"))       // Rc(value, allocator = Allocators::Global)
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

That costs one count up and one down per `or` or `if let`: plain integers, not atomics. A
`Box` has no second owner to take, so `self.node = null` inside `if let &n = self.node`
frees what `n` refers to, as it would through a `^T` (§1.1).

**`Nullable`** is the trait behind this, and any type may implement it: `is_null`,
`guard` — what `or` and `if let` hold for the duration, itself `Nullable` — and `ptr`, the
value's address or `null`. `Rc` and `Weak` guard with an `Rc`; `Box` with an `Unguarded`,
which holds nothing.

**A handle to a derived struct converts to a handle to its base**, as a reference does
(§7.3): an `Rc<Button>` is accepted where an `Rc<UiElement>` is wanted, sharing the same
count, and likewise `Weak` and `Box`. `[DECIDED — §18.64]`

Not yet: `Arc<T>`, which needs atomics (§18.11); and a handle to a trait object.
`[DECIDED — §18.62]`
