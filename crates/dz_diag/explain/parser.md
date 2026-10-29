# The parser, DZ02xx

## DZ0201

The grammar wanted one particular token here — a `)` to close a call, a `{` to open a body,
a `:` before a type — and found something else. The label names what was expected.

```dz
fn main() {
    let total = add(1, 2
    println!("{}", total)
}

fn add(a: i32, b: i32) -> i32 {
    return a + b
}
```

```dz
fn main() {
    let total = add(1, 2)
    println!("{}", total)
}

fn add(a: i32, b: i32) -> i32 {
    return a + b
}
```

## DZ0202

At the top of a file, and in a type's body, only declarations may appear: `fn`, `struct`,
`enum`, `union`, `trait`, `impl`, `const`, `static`, `import`, a macro. A statement belongs
inside a function.

```dz
let limit = 10

fn main() {
    println!("{}", limit)
}
```

```dz
const LIMIT: i32 = 10

fn main() {
    println!("{}", LIMIT)
}
```

## DZ0203

An operand was wanted — a literal, a name, a call, a bracketed expression — and the line
ended or an operator came instead.

```dz
fn main() {
    let x = (4 * )
    println!("{}", x)
}
```

```dz
fn main() {
    let x = 4 * 2
    println!("{}", x)
}
```

## DZ0204

After `:` in a binding or parameter, and after `->`, a type is wanted: a name such as `i32`
or `Player`, or a form built from one — `&T`, `^T`, `[]T`, `[4]T`, `T?`.

```dz
fn half(x: ) -> i32 {
    return x / 2
}

fn main() {
    println!("{}", half(4))
}
```

```dz
fn half(x: i32) -> i32 {
    return x / 2
}

fn main() {
    println!("{}", half(4))
}
```

## DZ0205

`a < b < c` does not mean "b is between a and c" in Dazzle, and in C++ it means something
nobody wants — `(a < b) < c`, comparing a `bool` with a number. So comparisons do not chain
(spec §11.1): write both comparisons and join them.

```dz
fn main() {
    let x = 5
    if 0 < x < 10 {
        println!("in range")
    }
}
```

```dz
fn main() {
    let x = 5
    if 0 < x && x < 10 {
        println!("in range")
    }
}
```

## DZ0206

A few words are kept back for features that may come later — `async`, `await`, `where`,
`yield`, `comptime`, `collect` — so they cannot be names today (spec §2.7).

```dz
fn main() {
    let yield = 3
}
```

```dz
fn main() {
    let produced = 3
    println!("{}", produced)
}
```

## DZ0207

A `let` binds a name, or takes a tuple apart into names: `let x = …`, `let (a, b) = …`. A
literal or an expression cannot be bound to.

```dz
fn main() {
    let 5 = 5
}
```

```dz
fn main() {
    let five = 5
    println!("{}", five)
}
```

## DZ0208

A macro invocation's brackets are collected before the macro sees them, so one left open
runs to the end of the file. The label is on the bracket left open, which is more use than
where the parser noticed. (An open bracket elsewhere is reported where its partner was
expected, as DZ0201.)

```dz
macro twice -> expr {
    pattern { ( $e: expr ) }
    expand { $e * 2 }
}

fn main() {
    let n = twice!(4
```

```dz
macro twice -> expr {
    pattern { ( $e: expr ) }
    expand { $e * 2 }
}

fn main() {
    let n = twice!(4)
    println!("{}", n)
}
```

## DZ0209

Assignment is a statement and produces no value (spec §11.1), so `a = b = 0` does not
assign twice, and `if x = 3` is never a typo that compiles. Assign each on a line of its own.

```dz
fn main() {
    let mut a = 1
    let mut b = 2
    a = b = 0
    println!("{} {}", a, b)
}
```

```dz
fn main() {
    let mut a = 1
    let mut b = 2
    b = 0
    a = b
    println!("{} {}", a, b)
}
```

## DZ0210

In the head of an `if`, `while`, `for` or `switch`, a `{` starts the body, so a struct
literal there would be read as one: `if p == Point{ x: 1 } {` opens the body at the
literal's brace. Put the literal in brackets, `(Point{ x: 1 })`, or bind it first.

No example: the parser reports this case as DZ0201 at present, and this code is reserved for
it.

## DZ0211

`or` handles one failure or absence (spec §9.4): `a or b or c` would leave it unclear which
`or` applies to which. Name the intermediate value, or bracket the inner `or`.

```dz
fn positive(n: i32) -> i32? {
    if n > 0 {
        return n
    }
    return none
}

fn main() {
    let n = positive(-1) or positive(2) or 0
    println!("{}", n)
}
```

```dz
fn positive(n: i32) -> i32? {
    if n > 0 {
        return n
    }
    return none
}

fn main() {
    let n = positive(-1) or (positive(2) or 0)
    println!("{}", n)
}
```

## DZ0212

A loop is a statement and produces no value (spec §10.2), so `break` takes none. Assign
the result to a variable declared before the loop, then `break`.

```dz
fn main() {
    let mut i = 0
    while true {
        i += 1
        if i == 3 {
            break 3
        }
    }
}
```

```dz
fn main() {
    let mut i = 0
    let mut found = 0
    while true {
        i += 1
        if i == 3 {
            found = i
            break
        }
    }
    println!("{}", found)
}
```

## DZ0213

A destructor and a copy are the `Drop` and `Copy` traits, implemented with `impl` (spec
§6.1) — not special members written into the struct, as they once were and as C++ writes
them.

```dz
struct File {
    pub fd: i32

    drop() {
        println!("closing {}", fd)
    }
}

fn main() {
    let f = File{ fd: 3 }
    println!("{}", f.fd)
}
```

```dz
struct File {
    pub fd: i32
}

impl Drop for File {
    fn drop(&mut self) {
        println!("closing {}", fd)
    }
}

fn main() {
    let f = File{ fd: 3 }
    println!("{}", f.fd)
}
```

## DZ0214

The receiver alone says whether a method changes its value: `fn bump(&mut self)`. A `mut`
in front of `fn` said it twice and could disagree with itself, so it was removed (r15).
`dz fix` removes it.

```dz
struct Counter {
    pub n: i32

    pub mut fn bump(&mut self) {
        n += 1
    }
}

fn main() {
    let mut c = Counter{ n: 0 }
    c.bump()
    println!("{}", c.n)
}
```

```dz
struct Counter {
    pub n: i32

    pub fn bump(&mut self) {
        n += 1
    }
}

fn main() {
    let mut c = Counter{ n: 0 }
    c.bump()
    println!("{}", c.n)
}
```

## DZ0216

A union's body lists its variants and its methods (spec §7.5). Data belongs on a variant —
each variant is a struct — or in a struct that holds the union.

```dz
union Shape {
    Circle { r: f32 }
    pub scale: f32
}

fn main() {
    let s = Shape::Circle{ r: 1.0 }
}
```

```dz
union Shape {
    Circle { r: f32 }
}

struct Scaled {
    pub shape: Shape
    pub scale: f32
}

fn main() {
    let s = Scaled{ shape: Shape::Circle{ r: 1.0 }, scale: 2.0 }
    println!("{}", s.scale)
}
```

## DZ0217

A formatting macro reads like the call it stands for, `println!(…)`; the other intrinsics
are written with `#`, `#location`, `#line` (spec §12.7, §12.9). `dz fix` respells
`#println(…)` and its siblings.

```dz
fn main() {
    #println("{}", 1)
}
```

```dz
fn main() {
    println!("{}", 1)
}
```

## DZ0218

A `kills` clause names memory by a tie path: a parameter or `self`, then fields, `[*]` for
any element and `@name` for a tie (spec §6.7). A clause with nothing after `kills`, or a
path that breaks off at `.`, `@` or `[`, names nothing.

```dz
fn fill(xs: &mut List<i32>) kills {
    xs.push(1)
}

fn main() {
    let mut xs = List<i32>()
    fill(&mut xs)
}
```

```dz
fn fill(xs: &mut List<i32>) kills xs {
    xs.push(1)
}

fn main() {
    let mut xs = List<i32>()
    fill(&mut xs)
    println!("{}", xs.len())
}
```

## DZ0219

A struct declares its ties itself, among its members, and says what memory of its own each
stands for: `tie @items = m_data` (spec §6.7). Until r84 a field declared one, `m_data:
own ^mut T in @items`, which could only name the whole field; the struct's own declaration
can also name a field's tie, `tie @items = m_list@items`, or several places at once.

```dz
pub struct Party {
    m_members: List<String> in @members = List<String>()

    pub fn recruit(&mut self, name: str) kills self@members {
        m_members.push(String::from(name))
    }
}

fn main() {
    let mut p = Party{}
    p.recruit("Ann")
}
```

```dz
pub struct Party {
    m_members: List<String> = List<String>()
    tie @members = m_members@items

    pub fn recruit(&mut self, name: str) kills self@members {
        m_members.push(String::from(name))
    }
}

fn main() {
    let mut p = Party{}
    p.recruit("Ann")
}
```
