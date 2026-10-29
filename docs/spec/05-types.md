# 5. Types

## 5.1 Primitives

```
i8 i16 i32 i64        signed integers, two's complement
u8 u16 u32 u64        unsigned integers
isize usize           pointer-sized
f32 f64               IEEE-754
bool                  1 byte, values true / false
char                  32-bit Unicode scalar value
void                  the type of a function that returns nothing
```

Integer overflow **wraps, in every build**, and never panics: `+`, `-` and `*` on an `i32`
that goes past its range come back round, as they do in C, Java and C# — for signed types
too, where C++ would leave it undefined. A program that wants something else says so with
the methods on `Integer` (§14.4): `a.checked_add(b)` gives a `T?`, `a.saturating_sub(b)`
stops at the limit. `[DECIDED — §18.63]`

There are no implicit numeric conversions, in either direction, ever — not even widening.
`let a: i64 = some_i32` is an error; write `some_i32 as i64`. Untyped *literals* are the
sole exception (§2.5). This is the single most common source of silent bugs in C++ game
code and it costs nothing to forbid. `[DECIDED]`

## 5.2 Pointers and references

Five ways to refer to something — the model your samples already use, plus `&mut` and
`^mut`.

| Syntax | Meaning | Nullable | Mutable | Owns |
|---|---|---|---|---|
| `&T` | shared reference | no | no | no |
| `&mut T` | exclusive reference | no | yes | no |
| `^T` | raw pointer, read-only | yes | no | no |
| `^mut T` | raw pointer | yes | yes | no |
| `own ^mut T` | owning pointer | yes | yes | **yes** |

- **`&T` / `&mut T`** are what you pass to functions. They are guaranteed non-null and
  **cannot be stored in a struct field**, cannot be heap-allocated, and cannot be `own`ed.
  A reference therefore only ever lives on the stack, so it cannot outlive its frame by
  being smuggled into a heap object. `[DECIDED — §18.3]`

  Non-null is enforced at the point of conversion, not by analysis — a `&T` whose target has
  since been freed is still your problem (§1.1).

  This is a cheap heuristic, not a proof. What the rule buys is that the most common
  dangling case costs zero analysis to prevent, that `&T` keeps a meaning stronger than
  "non-null pointer", and that ties (§6.7) — which check the rest, a reference into storage a
  call has moved — never have to appear on a struct.

  The practical consequence: a **context struct holds `^T`, not `&T`.**

  ```dz
  struct ButtonCtx {
      btn:        ^Button      // not &Button — this struct could be stored
      input:      ^Input
      delta_time: f32
  }
  ```
- **`^T`** and **`^mut T`** are C pointers — `const T*` and `T*`. Nullable, freely
  storable, no guarantees. This is how you build a scene graph with parent back-pointers,
  and it is on you to keep it valid. A `^T` is only read through; a `^mut T` may be written
  through as well. They stand to each other as `&T` and `&mut T` do, and as `[]T` and
  `[]mut T` do: read-only unless `mut` is written. `[DECIDED — §18.37]`
- **`own ^mut T`** is a `^mut T` that this struct is responsible for releasing. It changes
  the *type's* semantics — see §6. `own ^T` exists too, an owned block that is only read,
  but a container writes into what it owns, so `own ^mut T` is the usual form.

**Reading through a reference.** A `&T` or `&mut T` is accepted where a `T` is wanted, and
copies the value it refers to — `return m` in a function returning `Movie`, `let v: Vec3 =
r`, an argument, a field. Reading cannot fail, so it is not spelled out. Only a copyable `T`
can be read this way: a move-only one would have to be moved, and nothing may be moved out
of a borrow (`DZ0402`, §6.4), and in a generic body that means `T: Copy`. Where no type is
wanted, a reference stays a reference: `let r2 = r` is another `&T`. `[DECIDED — §18.35]`

An operand of a comparison, an arithmetic or a bitwise operator is read the same way when
it refers to a number, `bool`, `char`, `str` or an enum: `x > 2`, `x * 2` and `x & 1` on a
`&i32` work on the `i32`, whether or not a type is wanted of the result, so a predicate
taking `&T` reads as one taking `T`. A reference to a struct is compared through `Eq` and
`Ord` as before (§11.2).

**There is no reference to a reference.** `&r` or `&mut r` on an `r: &mut T` refers to the
`T` that `r` refers to, and is a `&T` or a `&mut T`; so is `&mut list[i]`, whose `IndexMut`
already hands back a reference (§11.2). `&mut` of a `&T` is an error, since nothing writable
is behind it.

**Writing through a reference.** Assigning to a `&mut T` writes the `T` it refers to — `x = 1` and `x += 1` on an `x: &mut i32` change the caller's integer — since a reference cannot be pointed elsewhere. Through a `&T` that is an error. `[DECIDED — §18.44]`

**Converting between them.** Forming a pointer changes nothing, so a reference becomes a
pointer of the same writability for free — `&T` → `^T`, `&mut T` → `^mut T` — and anything
writable also serves where the read-only form is wanted (`&mut T` → `^T`, `^mut T` → `^T`).
Nothing converts the other way: a `&T` or a `^T` never becomes a `^mut T`, which would gain
the right to write. Behind a `^mut` the pointee must match exactly, so a `^mut ^mut T` is not
a `^mut ^T` — through it, a read-only pointer could be stored where a writable one is
expected, which is why C forbids `T**` → `const T**` too. A pointer becomes a reference
through an `or` clause like any other fallible thing (§9.4), because a pointer may be null
and a reference may not:

```dz
let btn = ctx.btn or return                     // propagate
let btn = ctx.btn or panic!("button missing")    // assert, with a message
let btn = ctx.btn or unreachable                // checked in debug, free in release
draw(ctx.btn)                                   // error[DZ0522]: expected &Button, found ^Button
```

The reference it produces is as writable as the pointer: a `^mut T` gives a `&mut T`, and a
`^T` gives a `&T`. Non-null is the only guarantee a reference carries that a pointer does
not, so it is worth one line at the boundary. The check happens once, where the pointer
enters scope, rather than on every function entry downstream. `[DECIDED — §18.16, §18.37]`

**Using a pointer is `unsafe`.** Nothing can check that a `^T` still points at something, so
the operations that assume it does are allowed only inside an `unsafe` block, or in the body
of an `unsafe fn`. Dereference is postfix `^`, and `.` reaches through a pointer on its own —
both count:

```dz
let p: ^mut Player = &mut world.players[0]   // forming a pointer: safe
if p != null { ... }                         // comparing, copying, storing, passing: safe
let pl = p or return                         // the checked conversion: safe
pl.health -= 10                              // through the reference: safe

unsafe {
    p.health -= 10                           // `.` through a `^mut T`
    let h = p^                               // explicit deref, copies the Player
}
```

Unsafe, and only these: `p^`, `.` and `[i]` through a `^T`, a pointer cast (§10.6),
`construct`, `destroy`, `move_out`, `memcpy` and `memmove` (§14.3), and calling an `unsafe
fn`. Everything else stays safe — forming, copying, storing, comparing and passing pointers,
and the `or` conversion, whose null check is the one thing that *can* be checked. An `unsafe fn` is
one whose callers must uphold something the compiler cannot see; its body is an `unsafe`
block, and calling it needs one:

```dz
unsafe fn poke(p: ^mut i32, v: i32) { p^ = v }

unsafe { poke(slot, 3) }
```

`unsafe` marks raw memory; it is not a proof. It does not promise that safe code cannot
misbehave — a stale `&T` handed down a call is still your problem (§1.1) — and it relaxes no
other rule: moving out of `p^` is still `DZ0402` inside `unsafe`. What it buys is that every
place a dangling pointer can bite is marked, and that a container's raw-memory work sits
behind the safe interface it offers. Pointers are the escape hatch, not the everyday tool:
shared ownership is the standard library's `Rc<T>`, sole ownership of a heap value `Box<T>`, and
a reference that notices its target is gone `Weak<T>` (§14.5). `[DECIDED — §18.16]`

`null` is a value of every pointer type, `^T` and `^mut T` alike. There is no null `&T`.

**Auto-referencing.** Method receivers are auto-referenced: `buf.resize(8)` works and
passes `&mut buf` when `resize` takes `&mut self`. Free functions are **not** auto-referenced —
you write `resize(&mut buf, 8)` — so that reading a call site tells you whether the callee
can modify the argument. `[PROPOSED]`

## 5.3 Arrays, slices, and strings

```dz
[8]f32          fixed-size array; a value type, copied on assignment
[]f32           slice: { ptr: ^f32, len: usize }; a read-only view, never owns
[]mut f32       the same view, through which elements may be written
[N]T            N may be any compile-time constant expression
```

**A slice carries its own writability, exactly as a reference does.** `[]T` and `[]mut T`
stand to each other as `&T` and `&mut T` do (§5.2): the elements of a `[]T` cannot be
assigned or `&mut`-borrowed no matter how the slice itself is bound, and a `[]mut T`
satisfies a `[]T` parameter while the reverse is rejected. Both are two words wide and
identical at runtime; the distinction is entirely in the type.

The alternative — one `[]T` whose writability follows the binding that holds it — was
rejected because a slice is a *view*, so the binding says nothing about the elements: a
`fn fill(dst: []f32)` could then never be written, and there would be no spelling for
"you may read these and not write them", which is the whole reason `&`/`&mut` exists.
`[DECIDED — §18.22]`

**Indexing an array or slice is bounds-checked in every build**, release included: it is
safe code, and an out-of-range `buf[i]` panics with the file, line, index and length rather
than reading past the end. A loop over `0..xs.len` pays nothing for it, since the optimiser
proves the check redundant. Where it does cost, the unchecked read is spelled out: `xs.ptr` is
the first element as a pointer, and indexing that is pointer arithmetic, inside `unsafe`.
`.ptr` is as writable as its view: a `[]mut T` or a writable array gives a `^mut T`, and a
`[]T` or a read-only array a `^T` (§5.2). `[DECIDED — §18.37]`

**Indexing a `^T` is pointer arithmetic**, as in C: `p[i]` is the `i`th `T` after `p`, and
it is never checked, because a pointer carries no length to check against — which is why it
is `unsafe` (§5.2). This is what a container's `m_data[i]` over its `own ^mut T` is (§8.3). A
`^T` is not a sequence, so `for x in p` remains an error — make a slice when you want one.

**An array literal** is a bracketed list, and its type is an array of exactly as many
elements as it lists:

```dz
let a = [1, 2, 3]                  // [3]i32 — the length comes from the literal
let b: [4]f32 = [1, 2, 3, 4]       // the elements take the annotation's element type
let grid = [[1, 2], [3, 4]]        // [2][2]i32
let empty: [0]i32 = []
```

Three rules, each of which exists to keep the literal readable rather than clever:

- **The length is exactly what is written.** `let a: [8]i32 = [1, 2]` is an error naming
  both lengths. "The rest is zero" is already spelled `let mut a: [8]i32`, since §6.2
  initialises every value — so a short literal would be a second way to say something the
  language already says, at the price of the length living only in the annotation.
- **There is no repeat form.** A zeroed array is `let mut a: [1024]f32` and needs no syntax;
  a non-zero fill is a loop or the stdlib's `fill`. A `[0; 8]` spelling is in any case not
  available: §2.2 makes `;` a terminator and suppresses terminators after `[`, so it could
  not be told apart from a line break inside the literal.
- **A literal may not be stored as a slice.** `let s: []i32 = [1, 2, 3]` is rejected: the
  array is a temporary, so the slice would point at storage that is already gone. Passing
  one is fine — `f([1, 2, 3])` on an `fn f(xs: []i32)` — because the temporary outlives the
  call. This is not lifetime analysis; it is one rule about one certain mistake.

The element type comes from the annotation when there is one, so `[4]f32 = [1, 2, 3, 4]`
gives four `f32`s rather than four `i32`s converted (there are no implicit conversions —
§5.1). With no annotation the elements must agree, and an untyped literal defaults as §2.5
says. `[DECIDED — §18.24]`

```dz
str             a UTF-8 string slice: { ptr: ^u8, len: usize }. Never owns.
                Not null-terminated. Immutable.
const str       a str whose bytes are in static storage: a literal's, a constant's.
String          stdlib: owning, growable, allocator-aware. Written in Dazzle, not builtin.
cstr            a null-terminated ^u8, for C interop only.
```

**A slice of part of a sequence** is written with a range: `xs[a..b]`, `xs[a..]`, `xs[..b]`.
On an array or a slice both ends are checked when it runs, as an index is; on a pointer it is
`unsafe` and unchecked, and the end must be written, since a pointer carries no length. The
slice is as writable as what it views — a `[]mut T`, a `^mut T`, or an array that may be
written gives a `[]mut T`. `[DECIDED — §18.50]`

**A `str` is `{ ptr, len }`**, and `s.len` and `s.ptr` read them. Its bytes are `s as []u8`.
The way back is `str::from_utf8(bytes)`, a `str?` that is `none` when the bytes are not
well-formed UTF-8; `bytes as str` skips the check and is `unsafe`, as is `p as cstr` for a
`^u8`. `[DECIDED — §18.50]`

String literals live in static storage, with a zero after the last byte. **A string literal
is a `cstr` wherever a `cstr` is wanted** — a parameter, an annotated `let`, a field — and
costs nothing: C++ sees the literal itself, so `rl::InitWindow(1280, 720, "Dazzle")` is the
call it looks like. `"x" as cstr` says the same explicitly. The other way, `c as str`
measures a C string and copies nothing. A runtime `str` cannot become a `cstr`: nothing
follows its last byte, so a C string needs a copy, and a copy has to be owned and freed. That
owning C string is a `String`, which keeps a zero after its bytes and frees them when dropped:
`String::from(name)` copies, and its `as_cstr()` borrows the copy as a `cstr` (§14.4); a
`String` passed as an argument where a `cstr` is wanted is borrowed that way on its own
(§7.2). `s.as_str()` borrows the text. `[DECIDED — §18.39, D-0081, D-0089]`

**A `const str` is text that never dies**: its bytes are in static storage, a literal's or
a constant's. It is the same `{ ptr, len }` as a `str` and the same C++ type, and reads as a
`str` wherever a `const str` is not wanted — a call, a comparison, `{}`, `.len` — so every
implementation for `str` is one for `const str`. A `str` never becomes a `const str`, since
it may view a `String`. **A string literal is a `const str` wherever one is wanted**, as it
is a `cstr`; elsewhere it is a `str`, so `let mut label = "none"` can later take any text,
while a `let` with no type keeps a `const str` value's type. Because nothing kills it, a
`const str` carries no tie (§6.7): it can be returned from anything, held in a field and
put in a container, where a `str` there would be unchecked.

```dz
pub fn name(&self) -> const str {
    switch self {
        case Idle { return "idle" }
        case Walking { return "walking" }
    }
}
pub struct Enemy { pub kind: const str }
let was = enemy.state.name()
enemy.state = State::Idle{}         // `was` is still valid: it points into nothing that dies
```

Only `str` may follow `const`: `const i32` is `DZ0569`. `[DECIDED — D-0092]`

Because `?` binds to the type name, `[]T?` is a slice **of optionals**. An optional slice
is `([]T)?` — parenthesised, because it is far rarer and the parentheses say which level
is optional. `[PROPOSED]`

## 5.4 Compound types

```dz
(i32, f32)              tuple
T?                      optional  — see §5.6; built in, not a library type
fn(i32, f32) -> bool    function pointer
```

Tuples exist mainly for multiple return values; they have positional access `t.0`, `t.1`
and destructuring `let (x, y) = point()`. `[PROPOSED]`

## 5.5 Compile-time type queries

Every type answers a set of queries via `::`, evaluated during checking and usable in `if`
conditions that the compiler then folds away:

```dz
T::size                    usize, like sizeof(T)
T::align                   usize
T::name                    str, the source-level name
T::is_trivially_copyable   bool — no drop, no copy, all fields trivially copyable
T::is_copyable             bool — has an implicit or explicit copy
T::has_drop                bool
T::fields                  a compile-time sequence of the fields — deferred (§12.11, §13.4)
```

`sizeof(T)` and `alignof(T)` are accepted as aliases for `T::size` / `T::align`, because
they read better in allocation code, which is where they almost always appear.

Dazzle has no layout model of its own: `T::size` and `T::align` are what the target's C++
compiler says, for a struct laid out as §7.3 describes.

## 5.6 Modality

The type sigils divide on two axes, and keeping them apart is what stops them composing
into unreadable stacks.

| Axis | Spelled | Asks |
|---|---|---|
| **Access** | `^T` `^mut T` `&T` `&mut T` `own ^mut T` `[]T` `[]mut T` | how do I reach this value? |
| **Responsibility** | `own` | who frees it? |
| **Modality** | `T?` | is the value there at all? |

Access is prefix, modality is postfix, and they never compete for the same position:

```dz
T?              an optional T
^T              a pointer, which may be null
^T?             a pointer to an optional T
&T?             a reference to an optional T; dereferences to T?
[]T?            a slice of optionals
own ^T?         an owning pointer to an optional T
```

**`?` applies to value types only.** There is no `(&T)?` and no `(^T)?`, because a pointer
already encodes absence — `^T` *is* the optional pointer, and layering `?` on it would give
you two spellings for one type. This is why the niche optimisation that earlier drafts
promised for `?^T` no longer appears anywhere: it has nothing to optimise. `[DECIDED]`

Fallibility is not on this table because it is not a property of a value. See §9.
