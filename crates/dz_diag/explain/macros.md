# Macros, DZ06xx

A macro is declared with a pattern its invocation must match and an `expand` block that
writes code; an `expr` or `stmt` macro is invoked `name!(…)`, a `decl` macro `#name …`
(spec §12).

## DZ0601

No macro of that name is in scope: not declared in this module, not exported by a
dependency (`dep::name!(…)`), and not one of the compiler's own. The help gives the closest
name.

```dz
macro twice -> expr {
    pattern { ( $e: expr ) }
    expand { $e * 2 }
}

fn main() {
    println!("{}", twise!(4))
}
```

```dz
macro twice -> expr {
    pattern { ( $e: expr ) }
    expand { $e * 2 }
}

fn main() {
    println!("{}", twice!(4))
}
```

## DZ0602

The invocation must fit the macro's pattern token for token — the same separators, the
captures in their places (spec §12.3). Matching never backtracks, so the label is where the
first token stopped fitting.

```dz
macro pair -> expr {
    pattern { ( $a: expr , $b: expr ) }
    expand { $a + $b }
}

fn main() {
    println!("{}", pair!(1; 2))
}
```

```dz
macro pair -> expr {
    pattern { ( $a: expr , $b: expr ) }
    expand { $a + $b }
}

fn main() {
    println!("{}", pair!(1, 2))
}
```

## DZ0603

A macro's kind says where it may stand: a `decl` macro writes declarations, so it is invoked
where a declaration goes, never inside an expression; an `expr` macro produces a value
(spec §12.5).

```dz
macro constant -> decl {
    pattern { $name: ident = $value: expr }
    expand { const $name: i32 = $value }
}

#constant ANSWER = 42

fn main() {
    let x = #constant(ANSWER = 1)
    println!("{} {}", ANSWER, x)
}
```

```dz
macro constant -> decl {
    pattern { $name: ident = $value: expr }
    expand { const $name: i32 = $value }
}

#constant ANSWER = 42

fn main() {
    println!("{}", ANSWER)
}
```

## DZ0604

`#error(capture, "message")` in a macro's `expand` reports a mistake in the invocation, at
the capture it names — which is how a macro checks what its pattern alone cannot (spec
§12.6). The message is the macro author's.

```dz
macro machine -> decl {
    syntax State { state $name: ident { $body: code } }
    pattern { $name: ident { $states: []State } }
    expand {
        #if !$states {
            #error($name, "a state machine needs at least one state")
        }
        union $name {
            #for s in $states {
                $s.name {}
            }
        }
    }
}

#machine Empty {}

fn main() {}
```

```dz
macro machine -> decl {
    syntax State { state $name: ident { $body: code } }
    pattern { $name: ident { $states: []State } }
    expand {
        #if !$states {
            #error($name, "a state machine needs at least one state")
        }
        union $name {
            #for s in $states {
                $s.name {}
            }
        }
    }
}

#machine Light {
    state On {}
    state Off {}
}

fn main() {}
```

## DZ0605

An expansion may invoke macros, which may invoke more — up to 64 deep (spec §12.5). Deeper
than that is taken to be a macro that invokes itself without end.

```dz
macro forever -> expr {
    pattern { ( $e: expr ) }
    expand { forever!($e + 1) }
}

fn main() {
    println!("{}", forever!(0))
}
```

```dz
macro once -> expr {
    pattern { ( $e: expr ) }
    expand { $e + 1 }
}

fn main() {
    println!("{}", once!(0))
}
```

## DZ0606

The declaration itself is malformed: an unknown kind, an unknown `syntax` rule, a `$x` in
`expand` that the pattern never captured, or a `#for` over a capture that is not a
repetition (spec §12.1–§12.3).

```dz
macro sum -> expr {
    pattern { ( $a: expr ) }
    expand { #for x in $a { $x } }
}

fn main() {}
```

```dz
macro sum -> expr {
    pattern { ( $xs: [,]expr ) }
    expand { 0 #for x in $xs { + $x } }
}

fn main() {
    println!("{}", sum!(1, 2, 3))
}
```

## DZ0607

The compiler's own macros and intrinsics — `println`, `format`, `line`, `location`, `error`
and the rest — keep their names (spec §12.8), so a macro cannot take one.

```dz
macro line -> expr {
    pattern { }
    expand { 0 }
}

fn main() {}
```

```dz
macro zero -> expr {
    pattern { ( ) }
    expand { 0 }
}

fn main() {
    println!("{}", zero!())
}
```

## DZ0608

An expansion may invoke macros but not declare one (spec §12.5): every macro has to be
visible in the source before any expansion runs.

```dz
macro maker -> decl {
    pattern { $name: ident }
    expand {
        macro $name -> expr {
            pattern { }
            expand { 1 }
        }
    }
}

#maker one

fn main() {}
```

```dz
macro maker -> decl {
    pattern { $name: ident }
    expand {
        fn $name() -> i32 {
            return 1
        }
    }
}

#maker one

fn main() {
    println!("{}", one())
}
```

## DZ0609

A module has one macro of a name.

```dz
macro twice -> expr {
    pattern { ( $e: expr ) }
    expand { $e * 2 }
}

macro twice -> expr {
    pattern { ( $e: expr ) }
    expand { $e + $e }
}

fn main() {}
```

```dz
macro twice -> expr {
    pattern { ( $e: expr ) }
    expand { $e * 2 }
}

macro doubled -> expr {
    pattern { ( $e: expr ) }
    expand { $e + $e }
}

fn main() {}
```

## DZ0610

A macro's kind decides how it is invoked (spec §12.5, §18.9): an `expr` or `stmt` macro reads
like a call, `twice!(4)`, and a `decl` macro like what it declares, `#constant ANSWER = 42`.

```dz
macro twice -> expr {
    pattern { ( $e: expr ) }
    expand { $e * 2 }
}

fn main() {
    println!("{}", #twice(4))
}
```

```dz
macro twice -> expr {
    pattern { ( $e: expr ) }
    expand { $e * 2 }
}

fn main() {
    println!("{}", twice!(4))
}
```
