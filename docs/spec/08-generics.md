# 8. Generics

Generics are **checked at the definition site** and **monomorphized by Dazzle** into
concrete C++ types. They do not become C++ templates. `[DECIDED]`

## 8.1 Declaration and bounds

```dz
fn max<T: Ord>(a: T, b: T) -> T {
    if a > b { return move(a) }       // `T` may be move-only (§8.2)
    return move(b)
}

pub struct Buffer<T> { ... }

pub struct HashMap<K: Hash + Eq, V> { ... }
```

The body of `max` is checked once, against `T: Ord` alone. `a > b` is legal only because
`Ord` provides it. `a.frobnicate()` is an error reported at the line inside `max`, naming
the missing bound — not at the instantiation.

This is the whole reason for the choice: **the LSP can complete inside a generic body**,
because it knows exactly what `T` can do.

**A method may have type parameters of its own**, beside its type's:

```dz
pub struct List<T> {
    pub fn fold<A: Copy>(&self, start: A, step: fn(A, &T) -> A) -> A { ... }
    pub fn filled<V: Copy>(n: usize, value: V, make: fn(V) -> T) -> List<T> { ... }
}

let total = scores.fold(0, add)                    // A inferred as i32
let widest = names.fold<usize>(0, longest)         // written out
let grid = List<Cell>::filled<u8>(64, 0, to_cell)
```

They are solved at each call exactly as a generic function's are — from the arguments, or
written after the name — and their bounds are checked there. The type's own parameters
come from the receiver, or from the path for a function without one. Each combination
is its own instance (§8.4).

A `virtual` or `override` method, and a trait's method, **cannot** have type parameters of
their own (`DZ0557`): a vtable slot is one function, and a trait's method is called through
implementations, or a `^Trait`, that do not know the arguments. Make the trait generic
instead, `trait Show<T>`. `[DECIDED — §18.66]`

**A generic function may be taken as a value**, instantiated there: `xs.sort_by(greater<i32>)`
writes its arguments, and `let f: fn(&i32, &i32) -> bool = greater` reads them off the
function type wanted. Its bounds are checked as at a call.

## 8.2 Unbounded parameters

`Buffer<T>` has no bound, so `T` supports only what every type supports: being moved,
default-initialised, dropped, and queried via `T::` (§5.5). That is precisely enough for
`Buffer` — your implementation only ever branches on `T::is_trivially_copyable` and calls
`construct` / `destroy`, all of which are universal.

Being *copied* is not on that list, so inside the body an unbounded `T` is a move-only type
(§6.3) and every rule for one applies: returning or passing a `T` by value is a move, and is
written `move(x)` as it is anywhere else (§6.4). The instantiation may well be `Buffer<f32>`,
where the copy would have been free — but the body is checked once, for every `T`.

A bound says otherwise: **a `T: Copy` may be copied.** Every copyable type (§6.3) satisfies
`Copy`, primitives included, whether or not it has an `impl Copy`. `[DECIDED — §18.26]`

**A container is copyable exactly when its elements are** by bounding its `impl Copy` rather
than the container: `impl<T: Copy> Copy for Buffer<T>` gives `Buffer<f32>` a `copy` and
leaves `Buffer<File>` move-only — while `Buffer<File>` remains a perfectly good type.
Bounding the struct instead, `Buffer<T: Copy>`, would shut out every element that owns a
resource. Inside that `copy`, `T` is `Copy` because the implementation says so.
`[DECIDED — §18.26]`

## 8.3 Compile-time branches

```dz
impl<T: Copy> Copy for Buffer<T> {
    fn copy(&mut self, other: &Self) {
        m_allocator = other.m_allocator
        unsafe {
            m_data = m_allocator.alloc(other.m_cap * sizeof(T), alignof(T)) as ^mut T
            if T::is_trivially_copyable {
                memcpy(m_data, other.m_data, other.m_len * sizeof(T))
            } else {
                for i in 0..other.m_len { construct(m_data[i], other.m_data[i]) }
            }
        }
        m_cap = other.m_cap
        m_len = other.m_len
    }
}
```

A condition made only of compile-time queries is folded during monomorphization: the dead
branch is not emitted and not present in the C++ output. "Only" means type queries (§5.5),
`bool` constants, and `!`, `&&` and `||` over them; a condition that also reads a runtime
value is an ordinary `if`, and both branches are emitted. `[PROPOSED]`

Folding happens *after* checking, not instead of it. Both branches are checked against the
bounds, for every `T`, exactly as the rest of the body is (§8.1) — a branch that only makes
sense for some `T` is still an error in the others. That is what keeps a generic's errors at
its definition. The `else` branch above copies an element, which it may because the
implementation is bounded `T: Copy` (§8.2).

## 8.4 Monomorphization

`Buffer<f32>` and `Buffer<Entity>` become two distinct C++ structs:

```cpp
struct dz_engine_Buffer__f32 { /* ... */ };
struct dz_engine_Buffer__engine_Entity { /* ... */ };
```

Instantiations are cached per (generic, type-arguments) pair across the whole build, so a
type used in forty places is emitted once. Because Dazzle drives this rather than C++,
recursive instantiation depth is bounded by the compiler with a clear diagnostic instead of
a template-depth error.

## 8.5 Associated types

An **associated type** is a type that a trait names but does not choose — each implementer
fills it in. It exists because some traits need to talk about a type that varies per
implementer, and a generic parameter is the wrong tool for that.

The motivating case is iteration. A trait for "something you can step through" has to say
what it yields, but that answer is different for every container:

```dz
pub trait Iterate {
    type Item                       // "each implementer names this"

    fn valid(&self) -> bool         // is there a current element?
    fn get(&self) -> Item           // ...then this is it
    fn advance(&mut self)       // step past it
}
```

Two implementers, two different `Item`s:

```dz
struct SliceIter<T> {
    m_slice: []T
    m_index: usize
}

impl<T> Iterate for SliceIter<T> {
    type Item = &T                     // stepping a []T yields references into it

    fn valid(&self) -> bool   { return m_index < m_slice.len }
    fn get(&self)   -> &T     { return &m_slice[m_index] }
    fn advance(&mut self) { m_index += 1 }
}

struct RangeIter {
    m_current: i32
    m_end:     i32
}

impl Iterate for RangeIter {
    type Item = i32                    // stepping 0..n yields plain i32 values

    fn valid(&self) -> bool   { return m_current < m_end }
    fn get(&self)   -> i32    { return m_current }
    fn advance(&mut self) { m_current += 1 }
}
```

Now `for` works on both, and the loop variable gets the right type — `&Entity` in the first
case, `i32` in the second — because `for e in xs` desugars to the three calls and the
compiler reads `Item` off whichever implementer `xs` provides (§10.4):

```dz
for &e in entities { e.draw(&mut batch) }   // e is &Entity
for i in 0..count  { grid[i] = 0 }          // i is i32
```

**Why not just `trait Iterate<Item>`?** Because a generic parameter is chosen by the
*caller*, so `SliceIter<T>` could then implement `Iterate<&T>` *and* `Iterate<T>` *and*
`Iterate<u8>` all at once, and `for e in xs` would have no way to know which one you meant —
every use would need an annotation. An associated type is chosen by the *implementer*, so
there is exactly one answer per type and inference always succeeds. That is the entire
distinction: **generic parameters are inputs, associated types are outputs.** `[PROPOSED]`

Associated types may also carry bounds (`type Item : Format`), which the implementer must
satisfy.

**Each implementation chooses its own.** A type implementing one trait at several arguments
answers for each separately: `impl Mul<Vec4> for Mat4` has `type Output = Vec4`, and `impl
Mul<f32> for Mat4` has `type Output = Mat4` (§11.2). `[DECIDED — §18.29]`

**A generic body names an associated type through a bound**, as `T::Item`. It is the type
`T`'s implementation chooses, whatever that turns out to be:

```dz
fn first<T: Iterate>(it: &T) -> T::Item {
    return it.get()
}

fn each<C: Iterable>(xs: &C, f: fn(C::Iter::Item)) {
    for x in xs { f(move(x)) }                // x is a C::Iter::Item
}

impl<I: Iterate> Iterate for Numbered<I> {
    type Item = (usize, I::Item)              // built from the inner iterator's
    …
}
```

- **It comes from a bound.** `T::Item` is valid because `T: Iterate` and `Iterate` declares
  `type Item`; supertraits count. A name no bound declares is an error (`DZ0560`), and so is
  a name two bounds declare, since each answers it separately — `T: Mul<f32> + Mul<i32>`
  has two `Output`s (`DZ0561`). A projection's own bounds reach further: `Iterable` declares
  `type Iter : Iterate`, so `C::Iter::Item` is a type too.
- **Inside the body, it is checked once, like `T` itself** (§8.1). `T::Item` is one type
  wherever it is reached from `T`, and a different type from `U::Item`. It can do what the
  trait's bounds on `type Item` say, and nothing more: `Iterate` puts none on `Item`, so a
  `T::Item` is moved, never copied (§8.2), and has no methods.
- **At an instance, it is the implementation's choice.** `first(&list.iter())` returns a
  `&i32`, because `SliceIter<i32>` chose `type Item = &i32`; a caller sees that type, never
  the projection. `T::Output` for a number is the number: `i32 + i32` is an `i32` (§11.2).

`[DECIDED]`

**Why three methods and not one `next() -> Item?`.** The one-method form is what every
`Option`-returning iterator does, and it does not work here. It uses absence as the
end-of-iteration signal, which means the signal lives **inside the item's own value
domain**:

- If `Item` is `^T`, `Item?` is `(^T)?` — the nesting §5.6 bans, because `^T` already spells
  absence as `null`. Collapsing it to `^T` makes "there is no element 3" and "element 3 is a
  null pointer" the same value, so iterating a `[]^Entity` would stop at the first null
  element instead of yielding it.
- If `Item` is itself `T?`, `Item?` is `T??`. Either that collapses — and a `none` *element*
  ends the loop early, the identical bug — or nested optionals have to become a real type.

Languages that use the one-method form get away with it because `Option<Option<T>>` and
`Option<&T>` are distinct types there. Dazzle deliberately gave pointers exactly one
spelling for absence (§18.15), and that choice is what rules the one-method form out.

Separating "is there one" from "what is it" removes the overlap entirely: `valid` answers
the first question, `get` the second, and `Item` is never wrapped, so it may be **any** type
— `&T`, `&mut T`, `^T`, `T?`, a tuple — and a null or `none` element is just a value. It is
also the shape a C++ `for (it = …; it.valid(); it.advance())` loop already has, so the
generated code is an ordinary loop rather than a repeated optional test (§16.2).

The cost is three methods per implementer instead of one, and an iterator has to hold its
current position somewhere `get` can read it. That is natural for slices, ranges, arrays and
maps; an iterator that must fetch a value in order to discover whether one exists has to
buffer it internally. `[DECIDED — §18.21]`

## 8.6 Why Dazzle monomorphizes instead of emitting C++ templates

The obvious alternative to §8.4 is to emit `template<typename T> struct Buffer { … }` and
let the C++ compiler do the instantiation. It would mean less codegen work and more
idiomatic output. It is still the wrong trade, for four reasons — but the reasoning is
worth stating, because "let C++ do it" is the right answer for *most* of this language and
it is important to be clear about why this is the exception.

Dazzle delegates to C++ wherever C++ does a job **once per construct**: destructors,
destruction order, copy constructors, `offsetof`, temporary lifetime, overload resolution in
imported headers. Templates are the opposite — C++ does that work **once per instantiation,
expensively, and with bad diagnostics**. That is the whole distinction.

1. **It would undo the reason you chose definition-site generics.** We check `Buffer<T>`'s
   body once, against `T`'s bounds (§8.1). If we then hand a template to C++, C++ re-checks
   it at every instantiation — and any mismatch between our type system and C++'s surfaces
   as a template error pointing into *generated* code the user never wrote. Principle 4 (§1)
   exists specifically to prevent that.
2. **Compile time.** Template instantiation is among the slowest things a C++ compiler
   does. A game codebase full of `Buffer<T>`, `HashMap<K,V>`, and `Array<T>` pays that cost
   on every build. Emitting plain structs means the C++ compiler does almost no work per
   instantiation, which is principle 6.
3. **Trait bounds have no C++17 equivalent.** `T: Ord + Hash` would have to be dropped on
   the floor (concepts are C++20, and §16.5 targets C++17 for console compatibility), so any
   bound violation that slipped past us would become a deep instantiation error instead of a
   clean one.
4. **The compile-time branch folding you just approved (§8.3) gets subtler.** With
   monomorphization we simply do not emit the dead branch. With templates we would depend on
   `if constexpr`, which requires the discarded branch to still be well-formed for every
   dependent `T` — a rule that is easy to violate and produces exactly the error messages
   we are trying to avoid.

The cost of monomorphizing is a mono pass in the compiler (a bounded, well-understood
amount of work) and more generated C++ text. Output readability is preserved by emitting
each instantiation as a plain struct with a comment naming its Dazzle origin:

```cpp
// dazzle: engine::Buffer<f32>
struct Buffer__f32 { /* ... */ };
```

`[DECIDED — §18.14]`
