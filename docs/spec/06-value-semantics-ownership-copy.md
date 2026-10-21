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
field, which is what `alloc` returning `^mut u8` (§9.8) has to mean. The reverse is also allowed
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

**What is *not* checked:** whether a `&T` outlives what it points at, and whether a `^T`
dangles. Dazzle has no lifetime system, by design (§1.1). `[DECIDED]`

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
