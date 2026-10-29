# 6. Value semantics: ownership, copy, move, drop

This is the heart of the language. Everything else is ordinary.

## 6.1 The three special members

A struct has at most one of each — a constructor, a `drop` and a `copy`:

```dz
struct Buffer<T> {
    Buffer(capacity: u32) { ... }   // constructor: name matches the struct, no return type
}

impl<T> Drop for Buffer<T> {        // destructor
    fn drop(&mut self) { ... }
}

impl<T: Copy> Copy for Buffer<T> {  // copy constructor, for the instances it covers
    fn copy(&mut self, other: &Self) { ... }
}
```

The constructor is a member, since it is how a value comes to exist. `drop` and `copy` are
implementations of the standard library's `Drop` and `Copy` traits (§7.6): they are behaviour a type
has, and a bound can then ask for it — `T: Copy`. They are stricter than other
implementations, because they become the struct's C++ destructor and copy constructor and
so are part of its definition: written in the struct's own module, for the struct itself —
every instance, `impl<T> Drop for Buffer<T>`, never one — and `impl Drop`'s parameters take
no bounds, since a destructor cannot exist for some instances and not others. Neither is
ever called by name: `drop` runs at the end of a scope, and `copy` on every copy.
`[DECIDED — §18.31]`

That is the complete list. There is no move constructor, no move assignment, no copy
assignment, no default constructor to write, no rule of five. The compiler synthesises
whatever C++ needs from these three (§6.6).

## 6.2 Default initialisation

**Every value is always initialised.** There is no uninitialised memory in Dazzle.

```dz
let f: File          // fully constructed, fields at their declared defaults
let n: i32           // 0
let p: ^Player       // null
let b: Buffer<u8>    // all fields at defaults: a valid, empty, allocator-less buffer
```

A field's default is its `= expr` initialiser if it has one, otherwise the zero value of
its type. Defaults are evaluated in declaration order.

`let f: File` is legal only if `File` can be default-initialised — that is, if it has no
constructor, or has one callable with zero arguments. Otherwise the error names the
constructor you have to call. `[PROPOSED]`

This is what makes the comment in your `Buffer::copy` true *and checkable*: `copy` always
runs on a defaults-initialised object, so an early `return` out of it leaves a valid empty
value rather than a half-built one.

## 6.3 Copyability

A type is **copyable** if:

- it has an `impl Copy` that applies to it — `impl<T: Copy> Copy for Buffer<T>` covers
  `Buffer<f32>` and not `Buffer<File>` — **or**
- it implements neither `Copy` nor `Drop`, has no `own` field, and every field is copyable.

Every copyable type satisfies a `T: Copy` bound, whether or not it has an `impl Copy`.
Otherwise it is **move-only**, and assignment or passing by value requires an explicit
`move`:

```dz
struct File {
    handle: own ^u64
}

impl Drop for File {
    fn drop(&mut self) { close(handle) }
}

let f  = File("save.dat")
let f2 = f              // error: File is move-only because it owns a resource.
                        //   help: use `move(f)` to transfer ownership, or implement
                        //         `Copy` for File to make it copyable.
let f3 = move(f)        // ok
```

`own` on a field is what flips the switch. It is not a comment — it is the declaration that
this struct is responsible for a resource, and it makes the whole struct move-only.

**`own` describes the struct, not the pointer.** An `own ^T` *value* is one pointer, and
reading one — comparing it to `null`, dereferencing it, handing it to a release function —
copies a pointer and nothing else. What is move-only is the struct that declares the field,
because that is the thing with a `drop` to run. Anything else would make `if handle != null`
inside `File::drop` a move out of `self`. `[DECIDED]`

Acquiring ownership is an ordinary assignment: a plain `^T` may be stored into an `own ^T`
field, which is what an allocator's `alloc` returning `^mut u8` (§14) has to mean. The reverse is also allowed
— an `own ^T` satisfies a `^T` — so `own` narrows nothing at the use site. `[DECIDED]`

## 6.4 Move semantics

`move(x)` transfers ownership out of `x`. Afterwards `x` is **moved-from**, and the
compiler tracks that with flow analysis:

```dz
let a = String("hello")
let b = move(a)
println!("{}", a.length())   // error: `a` was moved on line 2 and cannot be used
```

The analysis is flow-sensitive and handles branches:

```dz
let s = String("x")
if cond {
    consume(move(s))
}
use(s)                      // error: `s` is moved on some paths reaching here
```

At runtime a moved-from value is left **at its defaults** — not in some unspecified state.
This is a deliberate divergence from C++: the source of a move is reset to exactly the
state a freshly declared value would have, which means running its `drop` is always safe
and always a no-op for the resource. It costs a few stores; it removes an entire category
of bug. `[DECIDED]`

**This holds for every type.** `move` on a copyable value — an `i32`, a `str` — is still a
move: the source is moved-from afterwards, a later use of it is an error, and at runtime it
is reset to its defaults. To go on using a copyable value, copy it instead: leave out
`move`. One rule for every type is also what keeps a generic body honest: `move(v)` in a
`Buffer<T>` means the same for `Buffer<i32>` as for `Buffer<File>`. `[DECIDED — §18.33]`

**A move needs a place the function owns.** `move` resets its source, and a `&`, a `^` or an
array element is not something the function has the right to reset — so `move(p^)`,
`move(r.field)` through a reference, and `move(xs[i])` are errors. A local, a by-value
parameter, and a field of either are fine. `[DECIDED]`

The same flow analysis handles **use-before-assignment** for the cases where default
initialisation does not apply (a `let` with no type and no initialiser is rejected outright;
a partially-moved struct field is tracked per field), and **definite return**: a function
that promises a value must produce one on every path. An exhaustive `switch` counts as
returning if every arm does, since §10.3 already proved there is no other path.

**What is *not* checked:** whether a `^T` made in `unsafe` dangles. Whether a `&T` is
used after a call moved or freed what it points at *is* checked, by ties (§6.7); Dazzle had
no lifetime system until D-0091, and ties are a light one: since a reference is never stored
in a field (D-0003), they never appear on a struct that holds references. `[DECIDED; revised
by D-0091]`

## 6.5 Drop

A type's `drop` runs deterministically:

- at the end of the enclosing scope, in **reverse declaration order**;
- before a value is overwritten by assignment;
- when the owner of the value is itself dropped (fields drop in reverse declaration order);
- on a moved-from value — but harmlessly, since the value is back at its defaults.

`defer { ... }` runs a block at scope exit, after that scope's drops, in reverse order of
registration. It is for the non-RAII cases (`defer profiler::end_zone()`).

**A temporary is dropped at the end of its statement** — a value a call or a literal makes
that nothing binds, such as the `String` in `String::from(name).as_cstr()`. (`&make()`
written out is the exception: it lives to the end of the block, §14.2.) So a view into a
temporary — a `str`, `cstr`, slice, reference or pointer taken from it through a method,
a field, an index or a slice — may be handed to a call in the same statement, which
finishes first, but **keeping one is an error** (`DZ0407`): by `let`, by assignment, or by
`return`. Bind the owner first, `let owned = String::from(name)`, and take the view from
it. A function's result is not followed into its arguments, since what it returns need
not point into them. `[DECIDED — D-0081]`

**A type with an `own` field and no `drop` is an error.** `[DECIDED — §18.4]`

```
error[DZ0412]: `File` owns a resource but never releases it
  --> src/io/file.dz:2:5
   |
 2 |     handle: own ^u64
   |     ^^^^^^^^^^^^^^^^ this field is marked `own`
   |
   = help: add an `impl Drop for File` that releases `handle`, or remove `own` if File
           does not own this pointer
```

## 6.6 How this lowers to C++

Everything above becomes ordinary C++ special members, which is why Dazzle does not have to
implement destruction ordering, temporary lifetime, or exception safety itself — C++ already
does, correctly, and every C++ debugger and profiler already understands it.

| Dazzle | Generated C++ |
|---|---|
| `Buffer(cap)` | `Buffer(uint32_t cap)` |
| `impl Copy`'s `copy` | `Buffer(const Buffer& other)` — body runs on a defaults-initialised `*this` |
| `impl Drop`'s `drop` | `~Buffer()` |
| any other `impl`'s method | a free function taking the receiver first: `Mul_f32__Vec2__mul(const Vec2& self, float rhs)` |
| move | `Buffer(Buffer&& o)` — memberwise steal, then reset `o` to defaults |
| copy assignment | `~Buffer(); new (this) Buffer{}; copy(other);` + self-assignment guard |
| move assignment | `~Buffer(); new (this) Buffer{}; steal(o); reset(o);` |
| move-only type | copy constructor and copy assignment `= delete`d |

The copy-assignment lowering is exactly the `drop() + reset-to-defaults + copy()` sequence
your `Buffer` comment describes, so the invariant it relies on is guaranteed by codegen.

Deleting the C++ copy constructor for move-only types means that even if Dazzle's own flow
analysis had a hole, the C++ compiler catches the copy as a second line of defence.

## 6.7 Ties and `kills` clauses

A reference must never be used after the memory it points into has moved or been freed.
Ownership (§6.4) already rules out a use after a move; **ties** rule out the other half: a
reference into storage that a call has since grown, shrunk or replaced. `[DECIDED — D-0091]`

```dz
let n = counts.get_or_insert(word, 0)
counts.insert(other, 1)       // may grow the table
n += 1                        // error: `insert` kills `counts@items`, and `n` points into it
```

**A tie** is a span during which some memory is valid, written `@name`. A type declares one by
writing `in @name` after a field's type. On a pointer field it names the memory the pointer
owns; on any other field it is a public name for that private field, so a signature can
mention it without exposing the field (§4):

```dz
pub struct List<T> {
    m_data: own ^mut T in @items = null     // the array of elements
    ...
}
pub struct String {
    m_bytes: List<u8> in @text
}
```

A tie belongs to the type, not to whoever uses it: it is not a generic parameter, and every
`List` has its own.

**A tie path** names memory: a parameter or `self`, then fields visible wherever the
function is, declared ties, and `[*]` for any element. A path that ends without a tie means
every tie under it.

| Path | What it names |
|---|---|
| `list@items` | the array: what `&list[i]` points into |
| `list[*]@text` | the text of any element of a `List<String>`: what `list[i].as_str()` points into |
| `list[*]` | everything every element owns |
| `list` | everything the list owns |

`list@items` and `list[*]@text` are separate: moving the array of strings leaves each
string's bytes where they are, so a view into one element survives pushing another.

`[*]` names the elements of what a `for` can walk: an array, a slice, or a type with an
`Index<usize>` or an `Iterable` implementation. Through `Index` the elements have a type, so a
path can go on into them (`list[*]@text`); a type that can only be iterated — a `HashMap`,
whose elements are its entries — is named whole at `[*]`, and the path stops there. So does
a path at a tie on a pointer field, which names that memory as a whole.

**Every reference carries a tie**, known where it is made: `&mut world.enemies[i]` is in
`world.enemies@items`. A function returning a reference or a pointer says where it points
with `in`: `pub fn first(&self) -> ^T in self@items`. With exactly one reference input and no
`in`, the result is in that input as a whole. A reference parameter may say where it points
too, which is then checked inside the function: `e: &mut Enemy in world`.

**A `kills` clause** says which ties a function may end, after the return type:

```dz
pub fn push(&mut self, v: T) kills self@items
pub fn clear(&mut self) kills self@items, self[*]
pub fn len(&self) -> usize                              // no clause: kills nothing
pub fn spawn(world: &mut World, e: Enemy) kills world.spawned@items
fn rebuild(world: &mut World) kills world               // anything reachable from `world`
```

- **No clause means the function kills nothing, and a clause says exactly what the body
  kills.** The compiler works out what each body kills — through what it assigns, moves and
  drops, and the clauses of what it calls — and holds the clause to it, both ways:

  ```dz
  fn fill(xs: &mut List<i32>) {
      xs.push(1)             // error: `push` kills `xs@items`, and `fill` has no `kills xs@items`
  }
  fn look(xs: &mut List<i32>) kills xs@items {
      println!("{}", xs.len())   // error: `look` kills nothing, but declares `kills xs@items`
  }
  fn reset(w: &mut World) kills w {
      w.enemies.clear()      // error: `w` is wider than what dies, `w.enemies@items, w.enemies[*]`
  }
  ```

  A kill the clause leaves out is an error where it happens; a path the body never kills, or
  one wider than what it kills, is an error on the clause. Every one comes with the clause the
  body needs, which `dz fix` writes and the editor offers as a fix, and the editor shows the
  clause a body needs while it is being written. Every function says it, private ones
  included, so a signature always tells what a call can invalidate, and a change to a body
  never breaks a caller somewhere else without the signature changing too.
- **Where there is no body to hold it to**, the clause is a promise instead: a trait method
  without a default, an `extern fn`, a function type. An implementation's own clause is
  exact for its body, as anywhere, and may name no more than the trait's.
- **A clause names only what is visible wherever the function is.** A private method may
  name a private field (`kills self.m_things@items`); a `pub` one names a tie the field
  declares (`m_things: List<Thing> in @things`, then `kills self@things`).
- **A trait method's clause is the most an implementation may kill**, and a function type's
  is the most a function stored in it may; a function type names its parameters when its
  clause does:
  `fn(world: &mut World) kills world`. There, more than one path is parenthesised,
  `kills (w.a@items, w.b@items)`, since a comma after a bare path would begin the next
  parameter of whatever the type is written in.
- **A call with no clause to read** — a C or C++ function, a pointer from C — kills
  everything reachable through its `&mut` and `^mut` arguments.

**What kills what.**

- **Moving or dropping a place kills every tie under it**, inline storage included.
- **Writing a place kills what the old value owned, and its payload.** Assigning over a value
  ends every view into memory it owned — a `String`'s text, a `List`'s array — and, for a
  tagged union or an optional, every reference into its payload, since the variant may change.
  A plain value is overwritten where it is, so a reference into it stays valid, as it does
  after `p.x += 1`:

  ```dz
  let x = &body.at.x
  body.at = Vec2{ x: 1, y: 2 }        // fine: `x` still points at `body.at.x`; kills nothing
  let s = body.name.as_str()
  body.name = String::from("new")     // kills `body.name`: the old text is freed
  ```

  A clause reaches as far as a write does: a function given `&mut w` can free what `w` owns
  or change its payloads, but cannot move `w`, so `kills w` leaves a reference to a plain
  field of `w` alone.
- **Elements share their container's ties.** A kill through `xs[i]` is a kill through `xs[j]`,
  since `i` may equal `j`.
- **A call kills what its clause names, at the call's arguments.** At a call, a reference
  in a tie that the callee's clause kills is dead afterwards, and using it is an error:

  ```dz
  fn think(e: &mut Enemy, world: &mut World) kills world.enemies@items {
      world.enemies.push(Enemy::spawn(e.x))
      e.hp -= 1
  }
  think(&mut world.enemies[i], &mut world)   // error: `e` points into `world.enemies@items`
  think(&mut boss, &mut world)               // fine: `boss` is not in `world`
  ```

**What stays allowed** is aliasing. `&mut xs[i]` beside `&xs[j]`, a write through a `&mut`
that moves nothing (`p.x += 1`), a borrow of one field while another changes: none of these
kills anything. A use is an error only when a call in between can move the memory the
reference points into. Dazzle is single-threaded until §18.11 is decided, which is what lets
it keep aliasing that Rust refuses.

**What stays unchecked**, as §1.1 says: a pointer made inside `unsafe` by a cast or
arithmetic, and a pointer stored in a struct (a context struct's `^mut World`, §5.2), since
the checker cannot know that it and a separate `&mut world` are one `World`. A pointer a safe
call returns is tied like a reference. `std`'s own clauses are written by hand over `unsafe`
code, and are trusted.

The ties of `std`'s containers:

| | `self@items` | `self[*]` |
|---|---|---|
| `push`, `insert`, `reserve` | kills | — |
| `pop`, `remove`, `swap_remove` | kills | — |
| `clear`, `truncate`; assigning or dropping the container | kills | kills |
| `sort_by`, `swap`, `reverse` | kills | — |
| `len`, `get`, `first`, indexing | — | — |

`String`'s `push_str` and `push` kill `self@text`. `HashMap` and `HashSet` follow `List`.

Not yet implemented: the checker, `std`'s clauses and the diagnostics are T-0107's steps 2
and 3.
