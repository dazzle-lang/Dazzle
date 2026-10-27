# 2. Lexical structure

The existing Odin lexer already implements much of this; it will be ported to Rust (§17)
with the rules below as its specification.

## 2.1 Source encoding

Source files are UTF-8. A UTF-8 BOM is permitted and stripped. UTF-16 files are rejected
with a diagnostic telling the user to convert. `\r\n` and lone `\r` are normalised to `\n`
before lexing, so column numbers are always measured in the normalised text. `[DECIDED]`
File extension: `.dz`.

## 2.2 Terminators

Dazzle is newline-terminated. A newline emits a `TERMINATOR` token, as does `;`.
Consecutive terminators collapse into one, and terminators are suppressed:

- immediately after an infix or prefix operator (`+`, `=`, `,`, `&&`, …),
- immediately after an opening `(`, `[`, or `{` that begins an expression or argument list,
- immediately before a closing `)` or `]`.

That is what lets a call wrap across lines without a continuation character:

```dz
m_data = m_allocator.realloc(m_data as ^mut u8,
                             m_capacity * sizeof(T),
                             new_size * sizeof(T),
                             alignof(T)) as ^mut T
```

Semicolons are legal but never required, and `dz fmt` removes them. `[PROPOSED]`

## 2.3 Comments

```dz
// line comment
/* block comment /* which nests */ still a comment */
/// doc comment, attaches to the next declaration, surfaced by the LSP on hover
```

Nested block comments are supported. `[DECIDED]`

## 2.4 Identifiers

An identifier begins with a Unicode `XID_Start` character or `_`, and continues with
`XID_Continue` or `_`. Identifiers are compared by exact code points; no normalisation is
performed, so `aá` written two different ways are two different identifiers. The formatter
warns on non-NFC identifiers. `[PROPOSED]`

## 2.5 Literals

**Integers.** `42`, `1_000_000`, `0xDEAD_BEEF`, `0o755`, `0b1010_1010`. Underscores are
separators and may not lead or trail. A malformed prefix (`1x42`, `0xx35`) is a diagnostic,
not two tokens.

**Floats.** `3.14`, `1_000.5`, `1e9`, `2.5e-3`. A second `.` in a numeric literal
(`4.535.346`) is a diagnostic that reports the full malformed span rather than silently
lexing a member access — this is already what the Odin lexer's `has_multiple_dots` flag
does, and it should be preserved.

**Suffixes.** `10i8`, `20u32`, `1.5f32`, `1.5f64`, plus the shorthand `1.5f` for `f32`.
A suffix **is** the literal's type, wherever it is written: `let n = 5u8` is a `u8`,
`300u8` does not fit, and `5u8` where a `u16` is wanted is a mismatch rather than a
conversion. An integer with a float suffix (`2f32`) is a float. `[DECIDED]`

**Default types.** An unsuffixed integer literal is `i32`; an unsuffixed float literal is
`f64`. `[DECIDED]`

This default applies **only where there is no context to type the literal**, which in
practice is rare. `let x = 1.5` is `f64`, but `pos.x = 1.5`, `lerp(a, b, 0.5)` and
`let t: f32 = 1.5` are all `f32`, because the literal is untyped until the surrounding
expression types it (below). So the `f64` default costs an `f32`-heavy codebase almost
nothing while matching what every other language does.

Literals are *untyped* until they meet a context that types them, so `let x: u64 = 5` and
`let y: f32 = 1` are both fine and neither is a cast. Overflow at the point of typing is a
compile error: `let b: u8 = 300` does not compile.

**Characters.** `'a'`, `'\n'`, `'\u{1F600}'`. A `char` is a Unicode scalar value, 32 bits.

**Strings.** `"hello"` with escapes `\n \r \t \\ \" \0 \xNN \u{...}`. A newline inside a
non-raw string literal is a diagnostic, and the lexer must recover by ending the string at
the line break rather than swallowing the rest of the file — that is the bug the current
`main.dz` sample is exercising.

Raw strings use backticks: `` `C:\path\no\escapes` ``. Multi-line raw strings are allowed.
`[PROPOSED]`

Interpolation is *not* part of the string literal. `println!("Hello {}", name)` uses
compile-time format-string checking instead (§12.9). `[PROPOSED]`

## 2.6 Operators and punctuation

```
+   -   *   /   %          arithmetic
+=  -=  *=  /=  %=         compound assignment
==  !=  <   >   <=  >=     comparison
&&  ||  !                  logical
&   |   ^^  ~   <<  >>     bitwise   (^^ is xor; ^ is reserved for pointers)
&   &mut                   reference formation
^                          pointer type / dereference
=                          assignment
.                          member access (auto-derefs through ^ and &)
::                         scope access (module, enum, associated item, type query)
..  ..=                    exclusive / inclusive range
->                         return type
?                          postfix optional-type suffix (§5.6)
!->                        fallible return marker, in a function signature only (§9)
or                         failure clause (§9.4)
#[...]                     attribute
#                          compiler intrinsic / macro invocation
@                          a tie, in a tie path (§6.7)
$                          macro variable
_                          discard binding
```

`^` is the most contested character in the grammar: it is the pointer sigil (`^T`, `p^`),
which means it cannot also be xor. Xor is therefore `^^`. Odin makes the same trade and it
causes no practical friction. `[DECIDED]`

`?` is postfix and binds to the type name, so access sigils always sit outside it: `^T?`
is a pointer to an optional `T`. This is the one place the language reads right-to-left,
and it is deliberate — access describes how you reach a value, `?` describes the value.
`!` never appears in a type, so `^T!` is not a parse problem; it is not a thing. `[DECIDED]`

## 2.7 Keywords

```
as       break     case     const    continue  defer     else     enum
extern   false     fn       for      if        ignore    impl     import
in       kills     let      macro    module    move      mut      none     null
or       own       pub      return   self      Self      struct   switch
static   trait     true      unreachable       union     unsafe    while
```

`copy` and `drop` are not keywords: they are the methods of the `Copy` and `Drop` traits
(§6.1), and ordinary names otherwise.

Reserved for future use: `async`, `await`, `impl`, `where`, `yield`, `comptime`, `collect`.
