# 11. Operators

## 11.1 Precedence

Highest to lowest:

```
1   ::  .  ()  []  ^(deref)        postfix
2   -  !  ~  &  &mut  move        prefix
3   as                             cast
4   *  /  %
5   +  -
6   <<  >>
7   &                              bitwise and
8   ^^                             bitwise xor
9   |                              bitwise or
10  ..  ..=                        range
11  <  >  <=  >=  ==  !=           comparison (non-associative)
12  &&
13  ||
14  or                            failure clause (non-associative)
15  =  +=  -=  *=  /=  %=          assignment (statement, not expression)
```

Comparison is non-associative: `a < b < c` is a syntax error, not a silent bug. Mixing `&`,
`|`, and `^^` with comparison requires parentheses. `[PROPOSED]`

`or` is non-associative too: `a() or b() or c` is a syntax error (§9.4). `[DECIDED]`

## 11.2 Operator overloading

Overloading is via traits, and only for the operators where a user type genuinely has the
same meaning:

```dz
pub trait Add<Rhs = Self> { type Output; fn add(&self, rhs: Rhs) -> Output }
pub trait Sub<Rhs = Self> { type Output; fn sub(&self, rhs: Rhs) -> Output }
pub trait Mul<Rhs = Self> { type Output; fn mul(&self, rhs: Rhs) -> Output }
pub trait Div<Rhs = Self> { type Output; fn div(&self, rhs: Rhs) -> Output }
pub trait Rem<Rhs = Self> { type Output; fn rem(&self, rhs: Rhs) -> Output }
pub trait Neg              { type Output; fn neg(&self) -> Output }
pub trait Eq               { fn eq(&self, rhs: &Self) -> bool }
pub trait Ord : Eq         { fn cmp(&self, rhs: &Self) -> Ordering }
pub trait Index<I>         { type Output; fn index(&self, i: I) -> &Output }
pub trait IndexMut<I>      { type Output; fn index_mut(&mut self, i: I) -> &mut Output }
```

`b[i]` on a user type calls `index_mut` when `b` is a writable place that implements
`IndexMut` — so `b[i] = v` and `b[i] += 1` write through it — and `index` otherwise.

A game language without `v1 + v2 * scalar` for vectors and matrices is not usable, so this
is not optional. `[DECIDED]`

**The primitives implement them**, so a bound is satisfied by a number exactly as by a user
type — `fn largest<T: Ord>` accepts an `i32`. Every integer and float is `Eq`, `Ord`, `Add`,
`Sub`, `Mul` and `Div`; integers are also `Rem`; signed integers and floats are `Neg`;
`bool` is `Eq` and `char` is `Eq` and `Ord`. Each arithmetic implementation has `Rhs =
Self` and nothing else, since there are no implicit numeric conversions (§5.1). The
operators on these types are still the machine's own — the implementations exist to satisfy
bounds, not to put a call where an `add` instruction was.

**A type may implement one operator trait at several arguments**, one `impl` each, and the
right operand's type picks one — an exact match first, then the type an untyped literal would
default to. Each implementation chooses its own associated types:

```dz
impl Mul<f32> for Vec2 {
    type Output = Vec2
    fn mul(&self, rhs: f32) -> Vec2 { return Vec2{ x: x * rhs, y: y * rhs } }
}
impl Mul<Vec2> for Vec2 {
    type Output = Vec2
    fn mul(&self, rhs: Vec2) -> Vec2 { return Vec2{ x: x * rhs.x, y: y * rhs.y } }
}
impl Mul<Vec4> for Mat4 {
    type Output = Vec4                 // Mat4 × Vec4 is a Vec4, while Mat4 × f32 is a Mat4
    fn mul(&self, rhs: Vec4) -> Vec4 { ... }
}
let a = v * 2.0      // Mul<f32>
let b = v * w        // Mul<Vec2>
```

That is the only overloading there is (§18.5), and the only place it is needed: the argument
whose type decides is right there at the call. A literal that fits several and defaults to
none — `3` against `Mul<i64>` and `Mul<u8>` — is an error asking for its type.
`[DECIDED — §18.27, §18.29]`

**The right-hand side is taken by value, and the comparison side by reference.** That is not
an inconsistency; the two cases want opposite things.

- `Rhs` may be a scalar. `Vec3 : Mul<f32>` is the whole point of the `Rhs` parameter, and a
  `&f32` cannot be read — `&T` reaches through `.` for members and is otherwise not a value
  (§5.2). By value it is also the better signature for what these traits are for: a 4-byte
  copy beats an indirection, and `Vec3`, `Mat4` and friends are copyable by construction.
- `Eq` and `Ord` take `&Self` because **comparing must not consume.** A move-only type
  (§6.3) passed by value would be moved, so `a == b` would destroy `b` — and comparison is
  exactly the operation that has no business doing that. `Self` is the receiver's own type
  there, so it is always something `&` can reach into.

`[DECIDED — §18.25]`

**Not overloadable:** `&&`, `||`, `!` (short-circuiting must stay predictable), `=`
(assignment is defined by §6), `.` and `::`, and there is no `operator()`. Compound
assignment `a += b` desugars to `a = a + b` and is not separately overloadable.
`[PROPOSED]`
