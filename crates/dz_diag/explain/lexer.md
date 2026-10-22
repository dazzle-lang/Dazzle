# The lexer, DZ01xx

`dz explain` reads this file: each section is one code, its prose and then a program that
reports the code beside one that does not. The rule itself is the code's doc comment in
`codes.rs`. See `src/explain.rs`.

## DZ0101

Dazzle source is UTF-8, so a file saved as UTF-16 — which some Windows editors still do by
default — is refused before anything else is read. Save it as UTF-8; a UTF-8 byte-order mark
is accepted and skipped.

No example: the mistake is in the file's bytes, not in any text that can be shown here.

## DZ0102

The character is not part of any token: not an operator, not a letter that could begin a
name, not a digit. It is often a typographic character pasted from a document — a curly
quote, a long dash, a non-ASCII symbol.

```dz
fn main() {
    let price = 3 € 4
}
```

```dz
fn main() {
    let price = 3 * 4
    println!("{}", price)
}
```

## DZ0103

A zero-width space, a joiner or a byte-order mark in the middle of a file looks like
nothing and would make two names that read alike differ. It is refused wherever it is,
outside a string.

```dz
fn main() {
    let count​ = 1
    println!("{}", count)
}
```

```dz
fn main() {
    let count = 1
    println!("{}", count)
}
```

## DZ0104

A string literal ends at its closing quote, on the same line. A string that runs over
lines is a raw string, written between backticks, or several literals joined.

```dz
fn main() {
    let s = "never closed
    println!("{}", s)
}
```

```dz
fn main() {
    let s = "closed"
    println!("{}", s)
}
```

## DZ0105

A raw string runs from one backtick to the next, across lines; without the second one it
would swallow the rest of the file.

```dz
fn main() {
    println!("{}", 1)
}

const TEXT: str = `begins and never ends
```

```dz
fn main() {
    println!("{}", TEXT)
}

const TEXT: str = `begins
and ends`
```

## DZ0106

A `/*` comment runs to its matching `*/` — block comments nest, so every `/*` inside needs
its own `*/` too.

```dz
fn main() {
    println!("{}", 1)
}

/* a comment that is never closed
```

```dz
fn main() {
    println!("{}", 1)
}

/* a comment that is closed */
```

## DZ0107

Only the escapes spec §2.5 lists are recognised: `\n`, `\r`, `\t`, `\0`, `\\`, `\'`, `\"`,
`\x` with two hex digits and `\u{…}`. A backslash before anything else is a mistake rather
than a literal backslash; write `\\` for one, or use a raw string.

```dz
fn main() {
    println!("C:\Users")
}
```

```dz
fn main() {
    println!("C:\\Users")
}
```

## DZ0108

A number is digits with at most one `.`, an optional exponent and an optional suffix. Two
points, a prefix with no digits after it or letters run into the digits are not a number.

```dz
fn main() {
    let version = 1.2.3
}
```

```dz
fn main() {
    let version = 1.2
    println!("{}", version)
}
```

## DZ0109

The prefix sets the radix — `0b` binary, `0o` octal, `0x` hex — and every digit must belong
to it.

```dz
fn main() {
    let mask = 0b102
}
```

```dz
fn main() {
    let mask = 0b101
    println!("{}", mask)
}
```

## DZ0110

A suffix names the literal's type: `i8` to `i64`, `u8` to `u64`, `isize`, `usize`, `f32`
or `f64`. Anything else after the digits is refused.

```dz
fn main() {
    let small = 5u7
}
```

```dz
fn main() {
    let small = 5u8
    println!("{}", small)
}
```

## DZ0111

A `char` literal holds exactly one character between single quotes. Text is a `str`, in
double quotes.

```dz
fn main() {
    let c = 'ab'
}
```

```dz
fn main() {
    let c = 'a'
    let s = "ab"
    println!("{} {}", c, s)
}
```

## DZ0112

`_` separates digit groups, so it goes between digits — never last, where it separates
nothing.

```dz
fn main() {
    let big = 1_000_
}
```

```dz
fn main() {
    let big = 1_000
    println!("{}", big)
}
```
