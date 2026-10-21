# 12. Metaprogramming

Dazzle's metaprogramming is the **declarative macro**: a macro declares the syntax it
accepts and the code it expands to. `[DECIDED]`

The reason for this choice over comptime code execution is tooling: because a macro
declares its grammar up front, the LSP can parse a macro invocation, complete inside it,
and report errors at the right span — none of which is possible when a macro is an opaque
function that eats tokens.

The notation reuses what the rest of the language already means. A capture is written
like a parameter, `$name: kind`; repetition and absence are written like the types that
mean them, `[]K` and `K?`; and what runs while expanding is spelled with `#`, which always
means "the compiler does something here" (§12.8) — `#for`, `#if`, `#error`.
`[DECIDED — §18.75]`

## 12.1 Anatomy

```dz
macro machine -> decl {
    syntax Param {                                    // input: &Input
        $name: ident : $type: type
    }

    syntax Field {                                    // t: f32 = 0
        $name: ident : $type: type $( = $default: expr )?
    }

    syntax State {                                    // state Grow(t: f32 = 0) { … }
        state $name: ident $( ( $fields: [,]Field ) )? {
            $body: code
        }
    }

    pattern {                                         // ButtonState(ctx: &Ctx) { … }
        $name: ident ( $params: [,]Param ) {
            $states: []State
        }
    }

    expand {
        #for s in $states {
            struct $s.name {
                #for f in $s.fields {
                    $f.name: $f.type #if $f.default { = $f.default }
                }

                pub(module) fn update(&mut self, $params) -> $name? {
                    $s.body
                    return none
                }
            }
        }

        union $name {
            #for s in $states {
                $s.name
            }

            // One frame: the active state decides whether there is a next one.
            pub fn step(&mut self, $params) {
                let next: $name? = switch self {
                    #for s in $states {
                        case $s.name(&mut st) { st.update(#for p in $params[,] { $p.name }) }
                    }
                }
                let n = next or return
                self = n
            }
        }
    }
}

#machine ButtonState(ctx: &ButtonCtx, input: &Input, btn: &mut Button) {
    state DefaultState {
        if input.mouse_down && input.over {
            return .TransitionState{}
        }
    }

    state TransitionState(t: f32 = 0) {
        t += ctx.delta_time
        btn.scale = lerp(1, 2, t)
        if t >= 1 {
            return .ChangeSprite{}
        }
    }

    state ChangeSprite {
        btn.sprite = Sprite::Selected
        return .DefaultState{}
    }
}
```

- `macro NAME -> KIND` declares where the macro may be invoked: `decl` (where a
  declaration may be), `stmt` (where a statement may be) or `expr` (where an expression may
  be, which includes a statement). A `type` macro is reserved and not yet available.
- `syntax Name { … }` declares a reusable sub-grammar, written in `PascalCase` like a type.
  It is what keeps a pattern readable, and a capture of one has a field per capture inside
  it, which is how nested repetition is reached (`$s.fields`).
- `pattern { … }` is the grammar of everything after the macro's name in an invocation.
- `expand { … }` is the output: Dazzle source with `$` holes, and `#for`, `#if` and
  `#error` to shape it.
- A macro is a declaration like any other: private to its module by default, `pub macro`
  to export it (§12.5).

## 12.2 Fragment kinds

A capture is `$name: kind`. The kind is one of these, or the name of a `syntax` rule:

| Kind | Matches |
|---|---|
| `ident` | one identifier |
| `literal` | one literal: a number, string, character, `true` or `false` |
| `type` | a type |
| `expr` | an expression |
| `stmt` | one statement |
| `block` | a braced block, `{ … }` |
| `code` | statements, up to the bracket that closes the group they are in — a block's inside |
| `decl` | one declaration |
| `Name` | the `syntax Name` rule |

Each built-in kind is parsed by the *real* parser, so a `type` hole in a macro gets the same
diagnostics, the same completion, and the same hover as a type written anywhere else.

## 12.3 Repetition and options

In a pattern, the shape of a capture is written the way a type is:

- `$x: K?` — optional: matched if the invocation has one here.
- `$xs: []K` — zero or more, one after another.
- `$xs: [,]K` — zero or more separated by `,`, with a trailing `,` allowed. Any one
  punctuation token may stand for the comma: `[;]K`, `[|]K`.
- `$( … )?` — an optional group. Every capture inside it is absent when the group is.

Every other token in a pattern matches itself, so `(`, `=`, `state` and `{` above are
literal. Line breaks in a pattern are layout; between the parts of an invocation, line
breaks are skipped, while inside a captured fragment they are that fragment's own.

**Matching never backtracks.** An optional or repeated element is taken exactly when the
next token can begin it, and once taken it must complete. That is what lets a mismatch be
reported at the token that does not fit, and lets the editor know at every point of an
invocation which part of the grammar it is in.

In `expand`:

- `$x` emits a capture exactly as it was written; a repeated capture emits all of its
  elements with the separators between them, and an absent one emits nothing.
- `$x.field` is a capture inside a `syntax` capture, and `$v` / `$v.field` inside `#for`
  is the current element.
- `#for v in $xs { … }` emits its body once per element of `$xs`. `#for v in $xs[,] { … }`
  puts a `,` between the copies, which an argument list needs; a list of declarations or
  variants needs nothing, since each copy ends its own line.
- `#if $x { … } #else { … }` emits the first body when `$x` is present — an optional that
  matched, or a repetition with at least one element — and `#if !$x` the opposite. An `#if`
  whose condition does not begin with `$` or `!$` is a build-flag `#if` (§12.10), which the
  macro emits as it is rather than deciding.
- A line break in `expand` ends a statement, as it does anywhere else in Dazzle source.

`[DECIDED — §18.75]`

## 12.4 Hygiene

**Locals the expansion introduces are its own.** A `let`, a parameter, a `for` or `case`
binding, or an `or { e -> … }` name written in `expand` cannot be seen by captured code and
cannot capture a name of the caller's: `next`, `n` and `st` in `machine` would not collide
with a parameter of the machine named `next`.

**Declarations the expansion introduces are ordinary declarations.** Types, functions,
methods, fields, variants and constants written in `expand` are named by the caller like
any other — `ButtonState`, `.step(…)` — and inside the expansion a type's members are
reachable bare from its methods, as in any method (§7.1). That is how a state body's `t`
reaches the field the machine declared for it.

**Captured code keeps the caller's scope**, which is what makes `$s.body` able to see
`input` and `btn`.

**Other names in `expand` resolve where the macro is declared**, with that module's view of
what is visible, so an engine's `pub macro` may use the engine's types without its users
naming them. There is no way to build an identifier out of pieces: every name an expansion
introduces is written in it, or captured.

This is the standard hygiene rule, refined for declarations, and it is the one that makes
macros safe to use without reading their bodies. `[DECIDED — §18.76]`

## 12.5 Invocation

```dz
#machine ButtonState(ctx: &ButtonCtx) { … }   // declaration position
trace!("entering the loop")                   // statement position
let v = max!(a, b)                            // expression position
```

**A macro's kind decides how it is invoked**, so each macro has one spelling:

- an **`expr` or `stmt` macro** reads like the call it stands for: `name!`, then one
  bracketed group, `( … )`, `[ … ]` or `{ … }` — `max!(a, b)`, `count![xs]`,
  `log!("warn", …)`. An `expr` macro goes wherever an expression may be, a statement
  included, and a `stmt` macro where a statement may be, as the whole statement;
- a **`decl` macro** reads like the declaration it writes: `#name`, then everything up to
  the end of the line, outside brackets — so `#machine`'s braces run over as many lines as
  they need. It is invoked where a declaration may be, at module level or in a body.

A dependency's `pub macro` is reached through the dependency (§3.2): `dep::max!(a, b)`,
`#dep::machine …`. The pattern is matched against everything after the name, the brackets
of a `name!` invocation included. A macro invoked in another kind's spelling is an error
that says which one to write (DZ0610).

**The intrinsics are the compiler's own macros**: `#file`, `#line`, `#location` and the
rest (§12.7) are written with `#`, since they stand for a value rather than a call; the
formatting macros, `println!` and its family (§12.9), read like a call and take `!`. Their
names are reserved, and the other spelling of any of them is an error (DZ0217).

The invocation is replaced by its expansion: declarations where the invocation was,
statements into the enclosing block (opening no scope, as a statement `#if` does), and an
expression in place. An expansion may itself invoke macros, which expand in turn up to a
depth of 64; it may not declare one. `[DECIDED — §18.9, §18.77]`

## 12.6 Errors inside macros

An invocation that does not fit the pattern is an error at the first token that does not
fit, saying what the pattern expected there.

A macro may reject an invocation deliberately:

```dz
expand {
    #if !$states {
        #error($name, "a state machine needs at least one state")
    }
    …
}
```

`#error(message)` reports at the invocation's name, `#error($x, message)` at the tokens
`$x` captured.

**A diagnostic points where its code was written.** A mistake inside captured code —
a state's body — is reported there, in the user's own text, as if no macro were involved.
A mistake in code the macro wrote is reported at the invocation's name, with a note naming
the macro: "in expansion of `#machine`". An expansion that copies a capture to several
places reports a mistake in it once, and a parameter it copies is unused only if every copy
leaves it unused (DZ0406). A warning about a local the macro introduced is the macro
author's to fix and is not shown at the invocation. `[DECIDED — §18.79]`

`dz expand file.dz` prints the file with every invocation replaced by its expansion — the
single most important debugging tool for a macro system, and cheap to build.

## 12.7 Compiler intrinsics

Intrinsics are the macros the compiler provides, written with `#` (§12.5) in expression or
parameter position. Unlike a user macro they are resolved during checking,
since what they produce depends on where they end up, and become constants in the C++
output.

```dz
#file            str    the source file of the current call site
#line            u32
#column          u32
#function        str    the enclosing function's name
#module          str
#location        SourceLocation   { file, function, line, column }
```

Used as a **default parameter value**, an intrinsic is evaluated at the *caller's* location,
which is exactly the `#src_code` behaviour you asked for (and the same trick as Odin's
`#caller_location` and C++20's `std::source_location`). A defaulted `#location`'s
`column` is 0: the caller's line is exact, its column is not known there.
`[DECIDED — §18.48]`

```dz
pub fn assert(cond: bool, msg: str = "", loc: SourceLocation = #location) {
    if !cond {
        panic!("assertion failed: {}", msg, loc)   // reports `loc`, the caller's line (§9.8)
    }
}

fn update() {
    assert(dt > 0, "bad delta time")   // loc is update()'s line, not assert()'s
}
```

The generated C++ passes a pre-built static `SourceLocation` at each call site: zero runtime
cost, no `__FILE__` macro trickery.

```dz
#expr_text(e)    str    the literal source text of expression `e`
```

is what lets an assertion macro print `assert(hp > 0)` rather than `assert(false)`.
`[PROPOSED]`

## 12.8 Attributes

```dz
#[test]         fn test_buffer_grows() { ... }
#[bench]        fn bench_push() { ... }
#[thread_local] static SCRATCH: TempArena = TempArena{}
```

Attributes deliberately share the `#` sigil with `decl` macros and intrinsics, because they
are the same category of thing: **`#` means "the compiler does something here at compile
time."** An attribute is really just a macro in attribute position — `#[derive(Serialize)]`
is a compile-time transform on the declaration that follows it, no different in kind from
`#machine`. What reads like a call — an `expr` or `stmt` macro, a formatting macro — is
`name!` instead, so that it looks like the call it stands for. `[DECIDED — §18.2]`

The forms are distinguished by their first tokens, so the parser never has to guess:

| Form | Position | Meaning |
|---|---|---|
| `#[name]`, `#[name(...)]` | before a declaration | attribute |
| `#if`, `#else` | around declarations, members or statements | build-flag conditional (§12.10) |
| `#name …`, `#dep::name …` | declaration position, at module level or in a body, to the end of the line | `decl` macro invocation (§12.5) |
| `name!(…)`, `name![…]`, `name!{…}`, `dep::name!(…)` | expression or statement position; one bracketed group | `expr` or `stmt` macro invocation (§12.5) |
| `#file`, `#location`, `#expr_text(…)`, … | expression or default-argument position | intrinsic — a built-in macro (§12.7) |
| `println!(…)`, `format!(…)`, … | expression position, which includes a statement | formatting — a built-in macro (§12.9) |

`!` after a name is an invocation and nowhere else: as a prefix it is "not", and in `!->`
"fallible" (§7.1, §9), neither of which can follow a name.

Intrinsic names are reserved, and so is `error`, which `#error` (§12.6) uses: a macro may
not be called `file`, `line`, `column`, `function`, `module`, `location`, `expr_text`,
`print`, `println`, `eprint`, `eprintln`, `panic`, `format` or `error`.

**Every attribute is one the language defines, written where it applies.** An attribute the
language does not have is an error (`DZ0314`): a misspelled `#[tset]` accepted in silence
would leave a test out of every run, and nothing would say so. So is a known attribute on a
declaration it does not apply to, with arguments it does not take, or written twice
(`DZ0315`). `[DECIDED — §18.60]`

| Attribute | On | Meaning |
|---|---|---|
| `#[test]` | a function at module level | a test `dz test` runs (§19 M14) |
| `#[bench]` | a function at module level | a benchmark `dz bench` times (§19 M15) |
| `#[thread_local]` | a `static` | one variable per thread (§7.7) |

Attributes this specification describes but the compiler does not implement yet —
`#[inline]`, `#[align(n)]`, `#[packed]`, `#[deprecated(…)]`, `#[export]` (§15.5),
`#[derive(…)]` (§12), `#[persist]` (§17.6) and `#[raw]` (§7.5) — are errors that say so
(`DZ0316`), rather than being accepted and doing nothing. Conditional compilation is `#if`
(§12.10), not an attribute.

## 12.9 Format strings

Formatting is not string interpolation and not varargs — it is a compile-time-checked
macro. **`print!`, `println!`, `eprint!`, `eprintln!`, `format!` and `panic!` are the
compiler's own macros** (§12.5, §12.7): each takes a **string literal** as its format string
and the values it names, and the compiler parses the literal where it is written. `print!`
and `println!` write to standard output; `eprint!` and `eprintln!` write the same way to
standard error, for diagnostics that must not mix with a program's output; `format!`
builds a `String`; `panic!` aborts with the text (§9.8):

```dz
println!("player {} has {} hp", name, hp)
println!("{:.2}", elapsed)          // 2 decimal places
println!("{:x}", flags)             // hex
println!("{{literal braces}}")
```

Because the format string is parsed at compile time, an argument-count mismatch, an
unknown format spec, or an argument whose type has no `Format` impl are all ordinary
compile errors pointing at the offending `{}` — not runtime garbage, and not a `printf`
mismatch that corrupts the stack. They are spelled with `!` because they are macros that
read like a call: `println("…")` without it is not a name, and the error says to write
`println!`.

The spec set is deliberately small, and each one names the kind of value it suits:

| Spec | Writes | Suits |
|---|---|---|
| `{}` | the value, in its natural form | anything formattable |
| `{:x}` / `{:X}` | hex, lower / upper case | integers |
| `{:.N}` | `N` digits after the point | floats |
| `{{` / `}}` | a literal brace | — |

**The arguments are evaluated as a call's are** (§7.2): left to right, all of them before
anything is written, and a temporary one of them makes lives until the text is written. An
argument is read, never moved: `println!("{}", name)` leaves `name` where it was.

**Text you keep is a `String`.** `format!` builds a new `String`, in memory from
`Allocators::Global` unless an allocator is passed by name (§7.2), the one argument any of
these macros takes by name:

```dz
let hud = format!("Score {}   Lives {}", g.score, g.lives)
let msg = format!("{} took {:.1} ms", name, ms, allocator = Allocators::Temp)
```

**`Format` is how a value writes itself.** Anything that implements the trait can be an
argument — the numbers, `bool`, `char`, `str`, `cstr` and `String` implement it in `std`,
a reference is written as what it refers to, and a type of your own implements it to be
formatted too:

```dz
pub trait Format {
    fn format(&self, f: &mut Formatter)
}

impl Format for Vec2 {
    fn format(&self, f: &mut Formatter) {
        f.write_str("(")
        self.x.format(f)
        f.write_str(", ")
        self.y.format(f)
        f.write_str(")")
    }
}

println!("at {}", player.position)
```

A `Formatter` takes text with `write_str` and `write_char`, and says what the placeholder
asked for: `hex()` and `upper()` for `{:x}`/`{:X}`, `precision()` for `{:.N}`. On the
built-in kinds a spec must suit the value, as in the table above; on a type with its own
`Format` it is that implementation's to interpret. `Formatter::string(&mut text)` appends
to a `String`, and `Formatter::stdout()` and `Formatter::stderr()` write to a stream, for
code that formats a value without a format string.

**A function of one's own that takes a format string is a macro** (§12): a format string is
checked only where it is written in one of these, so a wrapper such as a `log` with levels
hands its literal on to one of them, where it is checked against the arguments exactly as
the caller wrote them:

```dz
macro log -> stmt {
    syntax Arg { $value: expr }
    pattern { ( $level: literal , $fmt: literal $( , $args: [,]Arg )? ) }
    expand {
        print!("[{}] ", $level)
        println!($fmt #for a in $args { , $a.value })
    }
}

log!("warn", "hp is {} of {}", hp, max_hp)   // a missing argument is reported at this string
```

Nothing of the format string is left to parse when the program runs: an invocation lowers
to one call that hands the text between the placeholders, each placeholder's spec and the
arguments to the standard library, which writes them in turn — straight to the stream for
`print!`, with no intermediate `String`. `error(…)` (§9.1) checks its format string the
same way, but keeps its message in static storage (§9.2), so it writes only the built-in
kinds. `[DECIDED — §18.73, §18.80]`

Interpolation (`"hp is {hp}"`) is deliberately absent for now: it hides a name lookup
inside a literal, which complicates the lexer, the LSP's rename, and macro capture. It can
be added later as pure sugar over this. `[PROPOSED]`

## 12.10 Compile-time conditionals

`#if` keeps code in a build or leaves it out, by **build flags**:

```dz
#if TRACK_MEMORY {
    static TRACKER: Tracker = Tracker{}
}

pub fn alloc(size: usize, loc: SourceLocation = #location) -> ^mut u8 {
    let p = heap_alloc(size)
    #if TRACK_MEMORY {
        unsafe { TRACKER.record(p, size, loc) }
    }
    return p
}

#if OS_WINDOWS {
    fn open_window() { … }
} #else #if OS_LINUX || OS_MACOS {
    fn open_window() { … }
} #else {
    fn open_window() { panic!("no window on this platform") }
}
```

- **Where**: around declarations, around a type's fields and methods, and around statements.
  A statement branch opens no scope — its statements belong to the enclosing block.
  `#if` is never an expression: a value that depends on a flag is an ordinary `if` on a
  `const`.
- **Conditions** are flags joined by `!`, `&&` and `||`, with parentheses. A flag is on or
  off; nothing is compared.
- **Only registered flags exist.** An unknown name is an error (DZ0309), never "off", so a
  misspelt `#if DEBUGG` cannot silently compile nothing. The compiler defines `DEBUG`,
  `RELEASE`, `SHIPPING` (from the build mode) and `OS_WINDOWS`, `OS_LINUX`, `OS_MACOS` in
  every package. A package registers its own under `[flags]` in its dazzle.toml (§3.1),
  each off unless its `default` says otherwise, and may not reuse a built-in name.
- **Whose flags**: a package reads its own flags bare, and a dependency's through the
  dependency — `#if engine::PROFILE`. A package's flags are turned on by what depends on it
  (`engine = { path = "…", flags = ["PROFILE"] }`) or from the command line for one build:
  `dz run --flag CHEATS`, `--flag engine::PROFILE`. The standard library's `TRACK_MEMORY` is
  `--track-memory` or `[build] track_memory = true`.
- **The inactive branch is parsed, not checked.** It is formatted and shown by the editor,
  but never resolved or type-checked, so it may name what exists only on another platform or
  under another flag. What that costs is that a branch nobody builds can rot unnoticed; what
  it buys is that platform code can be written at all. `[DECIDED — §18.48]`


## 12.11 When macros expand

**Macros expand on syntax alone**: after a module is parsed and its `#if`s are decided
(§12.10), and before any name in it is resolved. A macro sees tokens, never types, so an
expansion depends only on the invocation and the macro — each module is expanded on its
own, the editor can parse an invocation as it is typed, and a macro can never observe the
order in which the compiler happens to check things.

What that gives up is a macro asking questions of a type: a macro cannot be handed `T` and
iterate `T::fields`. A macro that generates code per field — serialization, an editor
inspector — takes the declaration itself and reads its fields from the tokens (§13.4).
Iterating the fields of a type parameter is a compile-time loop over an instantiation, not
a macro, and is deferred. `[DECIDED — §18.74]`
