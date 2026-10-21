# 10. Statements and control flow

## 10.1 `if`

```dz
if x > 0 {
    ...
} else if x < 0 {
    ...
} else {
    ...
}
```

Parentheses around the condition are not used. Braces are mandatory. The condition must be
`bool` — there is no truthiness, and no pointer-to-bool conversion (`if p != null`).
`if let x = v { … }` takes the value out of a `T?`, a `^T` or a handle (`Rc`, `Weak`, `Box`) for
the first block (§9.4, §14.5).

## 10.2 `while`

```dz
while running {
    ...
}

while true {
    if done { break }
}
```

No `do`/`while`. `break` and `continue` accept an optional label:

```dz
outer: while true {
    for &e in entities {
        if e.id == target { break outer }
    }
}
```

Neither carries a value: a loop is a statement, so there is nothing for a value to become.
A result is assigned to a variable declared before the loop. `[DECIDED — §18.28]`

`[PROPOSED]`

## 10.3 `switch`

```dz
switch state {
    case Idle          { ... }
    case Walking(w)    { speed = w.speed }
    case Falling(&mut f) { f.time += dt }      // writes into the union
    default            { ... }
}
```

- Exhaustive over unions and enums; a missing variant is an error listing which. `default`
  silences it, and the LSP offers a "fill in missing cases" action.
- No fallthrough. Ever. So `break` and `continue` inside an arm act on the enclosing loop —
  there is nothing in a `switch` to break out of.
- `case A, B { ... }` matches multiple variants when neither binds a payload.
- A payload binding holds the payload exactly as a `for` binding holds an element (§10.4):
  `case Walking(w)` is a copy that cannot be changed, `case Walking(&w)` a read-only
  reference, and `case Falling(&mut f)` a writable one, which needs the value switched on to
  be writable — a `let mut`, a `&mut` or a `^mut`. A payload that cannot be copied is bound
  with `&` or `&mut`. `[DECIDED — §18.43]`
- `switch` is also an expression when every arm yields the same type.
- Naming the same variant or value in two labels is an error: the later one could never
  match.

An integer, a `char`, a `bool` or a `str` is matched against **constants** instead of
variants. A label is anything known while compiling — a literal, a `const`, or arithmetic
on those — of the subject's type, and there is no exhaustiveness requirement: with no
matching arm and no `default`, nothing runs. A float cannot be switched on, since it is never
matched exactly.

```dz
switch key {
    case 'w', 'k'          { move_up() }
    case KEY_ESCAPE        { quit() }
    case FIRST_SLOT + 1    { select(1) }
    default                { }
}

switch command {
    case "quit" { return }
    case "help" { print_help() }
}
```

A bare name in a label means a variant when the subject is an enum or union, and a
constant otherwise, so `case Idle` and `case MAX` are written the same way.

## 10.4 `for`

```dz
for i in 0..count           { ... }     // exclusive
for i in 0..=count          { ... }     // inclusive
for c in text               { ... }     // iterates chars of a str
for e in entities           { ... }     // e is a copy of each element
for &e in entities          { ... }     // e refers to each element
for &mut e in entities      { ... }     // e refers to each element, and may change it
for i, &e in entities.indexed() { ... } // a pass count, then the element
for id, &btn, &mut state in query { ... } // a tuple element, taken apart
```

**How a binding holds its element** is written on the binding, as C++ writes `auto`,
`const auto&` and `auto&` — not on the sequence. `[DECIDED — §18.42]`

- **`e`** is a copy that cannot be changed. Writing `e.hp -= 1` would change a copy and
  nothing else, so it is an error, with a help pointing at `&mut e`. An element that cannot
  be copied — a type with a `drop` — cannot be bound this way either (`DZ0403`); refer to it
  with `&e`, or consume the sequence. A copy is always a real copy — its `copy` runs, and
  its `drop` at the end of the pass — so the compiler never quietly reads through a
  reference instead: with RAII that would change what the program does.
- **`&e`** refers to the element, read-only. Nothing is copied.
- **`&mut e`** refers to the element and may change it: `e.hp -= 1`, or `e = 0` for a number,
  writes into the sequence (§5.2). The sequence must be writable — a `let mut` array, a
  `&mut` or `^mut` to one, a `[]mut T` — and an iterator must hand out `&mut` references
  (`DZ0507` otherwise).

`&e` and `&mut e` need an element that lives somewhere. An array, a slice and an iterator
whose `get` returns `&T` or `&mut T` provide one; a range's numbers, an iterator whose `get`
returns a value, and a consuming loop do not, and take only `e` (`DZ0549`). What an iterator's
`Item` is decides what its loops may ask for: a query that returns `&mut State` can be
written through, and one that returns `&State` cannot.

**A tuple element is taken apart by naming its parts**, without parentheses: `for id, &btn,
&mut state in query` over an `Item` of `(EntityId, &Button, &mut State)`. Each part takes its
own `&` or `&mut`. A part that is itself a reference refers to what it names; the other parts
live where the tuple does. The number of names must be the tuple's size (`DZ0549`), so several
names always mean the parts of one element and never anything else.

**A pass count** comes from `indexed()`: `for i, e in xs.indexed()` binds a `usize` counting
passes from zero, then the element with the same choice of `e`, `&e` or `&mut e`. It works on
anything `for` does. For now `indexed()` is known only in a loop's head.

**Consuming.** `for e in move(entities)` moves each element out into `e` and leaves
`entities` moved-from (§6.4) — the one way to take move-only elements by value.

`for e in xs` desugars to the three `Iterate` methods, with the iterator in a scope of its
own so the loop variable may shadow anything outside it:

```dz
{
    let mut it = <xs as an iterator>
    while it.valid() {
        let e = it.get()
        { ... body ... }
        it.advance()
    }
}
```

**`<xs as an iterator>`** is `xs` itself when it implements `Iterate`. Otherwise it is
`xs.iter()` from **`Iterable`**, or `xs.iter_mut()` from **`IterableMut`** when a binding is
`&mut` — the standard library traits a container implements to hand out a fresh iterator, since an
iterator keeps its position in itself and a container must not:

```dz
pub trait Iterable {
    type Iter : Iterate
    fn iter(&self) -> Iter
}
pub trait IterableMut {
    type IterMut : Iterate
    fn iter_mut(&mut self) -> IterMut
}
```

`List` and `HashMap` implement both — a `HashMap`'s `Item` is `(&K, &V)`, so `for k, &v in
map` — and a generic function can take `C: Iterable` and loop over it. `[DECIDED — §18.51]`

`e` is bound from `Item`, read off the implementer. `advance` runs after the body on every
completed pass, and `continue` jumps to it rather than past it, so a `continue` cannot spin
forever. `break` leaves the whole construct.

## 10.5 `defer`

```dz
fn upload(&mut self) {
    gl::bind_buffer(m_handle)
    defer gl::bind_buffer(0)
    // ...
}
```

Runs at scope exit, after that scope's drops, in reverse order of registration. Runs on
every exit path including `return`, `break`, and a diverging `or` clause.

## 10.6 Casts

```dz
x as i64            // numeric conversion, explicit, may truncate
p as ^u8            // pointer reinterpretation — `unsafe` (§5.2)
e as u16            // enum to its backing type
p as usize          // a pointer's address, for aligning or hashing
n as ^mut u8        // an address back to a pointer — `unsafe` (§5.2)
```

`as` never invokes user code and never fails. Fallible conversions are library functions
returning `T?`, or marked `!->` when they have something to say about the failure. `as`
never fails, so there is deliberately no `as ref` for `^T` → `&T`; that conversion is an
`or` clause (§5.2) precisely because it is partial.
