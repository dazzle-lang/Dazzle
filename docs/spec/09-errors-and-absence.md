# 9. Errors and absence

Three different things can fail to give you a value, and Dazzle gives them one handling
construct:

| Source | Written | Carries on failure |
|---|---|---|
| an optional value | `T?` | nothing |
| a fallible call | `fn f(…) !-> T` | an `Error` |
| a pointer | `^T` | nothing |

`T?` is a **type**. `!` is a **signature marker** and never a type — there is no `T!` to
store in a field, annotate a `let` with, or point at. That asymmetry is deliberate: an
optional is a data shape, a failure is control flow, and the notation now says so.
`[DECIDED — 18.7]`

## 9.1 Producing them

In a function returning `T?` or marked `!->`, returning a plain `T` **is** the success case;
there is no wrapping step. The failure case has its own literal:

```dz
fn find_player(&mut self, id: EntityId) -> ^Player {
    for &mut p in m_players {
        if p.id == id { return ^p }
    }
    return null
}

fn read_header(f: &mut File) !-> Header {
    let mut h: Header
    if f.read_into(&mut h) < Header::size {
        return error("truncated header")
    }
    if h.magic != LEVEL_MAGIC {
        return error(ErrorCode::BadMagic, "bad magic 0x{:x}", h.magic)
    }
    return h
}
```

- The same conversion happens **wherever a `T?` is expected**: `let hp: i32? = 100`, an
  argument to a `T?` parameter, an assignment, and the arms of an `if` or `switch` whose
  other arms are `T?` or `none` — `if alive { 100 } else { none }` is an `i32?`. A `T?` never
  becomes a `T` without `or`. `[DECIDED — §18.53]`
- `none` is a keyword literal, valid only where a `T?` is expected.
- `error(...)` builds an `Error`, and is valid only in a `!->` function. It takes an
  optional code, a format string, and arguments (§12.9). **It never allocates.**
- A lookup that returns "the thing or nothing" returns `^T`, not `T?`. `T?` is for optional
  *values*; `^T` is for optional *access*, and it is one pointer.

## 9.2 The `Error` type

```dz
pub struct Error {
    pub code:     u32                // 0 = unspecified; use your own enum `as u32`
    pub message:  str                // static storage, or a formatted temp
    pub location: &SourceLocation    // where the error was created; static storage
}
```

32 bytes, no allocation, no heap, no vtable. The location is a reference into static storage
filled in by `#location` (§12.7) — `e.location.line` reads it directly — so an error tells you
where it came from at zero runtime cost, which is most of what a stack trace would have given
you, for free. It is the one reference the language stores in a struct: §18.3 bans that
because a stored reference could outlive what it points at, and a location lives in static
storage for the whole run of the program. User structs still cannot. `[DECIDED — §18.30]`

**`Error` and `SourceLocation` are provided by the language, not by the stdlib.** Both names
are in scope everywhere with no import, like `str` and `bool`, because `or { e -> … }` binds
one and there is nowhere for an import to come from. They are the only two structs with that
status. `[DECIDED]`

Domain-specific codes are ordinary enums:

```dz
pub enum ErrorCode : u32 { NotFound = 1, BadMagic, UnsupportedVersion, OutOfMemory }

return error(ErrorCode::NotFound, "no such asset: {}", path)
```

> **Typed errors are deliberately not supported.** A fallible signature always carries
> `Error`; there is no per-call error type. This is what buys the native representation and
> makes propagation free of conversion rules. If exhaustive matching on error kinds is ever
> genuinely needed, the reserved extension syntax is `LoadError!-> Texture`, which can be
> added without breaking anything. `[DECIDED — §18.7]`

## 9.3 Representation

| Shape | Size | Note |
|---|---|---|
| `T?` | `sizeof(T)` + 1, padded | `{ T value; bool has; }` |
| `!-> T` | `max(sizeof(T), 32)` + tag | `{ u8 tag; union { T value; Error error; } }`, returned by value |
| `^T` | one pointer | absence is the null bit pattern; nothing added |

Note what is *not* here. Earlier drafts niche-optimised `?^T` and `?&T` down to one pointer;
those types no longer exist, so a "might be there" lookup returns `^T` and the check is a
null check — the same code you would have written by hand, with the compiler enforcing that
you write it (§9.4).

## 9.4 The `or` clause

`or` is a postfix binary operator whose left side is fallible — a `!->` call, a `T?`, a
`^T`, or a handle such as an `Rc` (§14.5) — and whose right side says what to do when it
fails. A `&T?` is read as the optional it refers to, when `T` can be copied (§18.35); a
handle gives a reference to its target, as a pointer does, and keeps the target alive for
the rest of the enclosing block.

```dz
let tex  = load_texture(path) or Textures::Missing            // fall back
let f    = File::open(path)   or return                       // propagate
let hp   = find_player(id)    or continue                     // diverge
let btn  = ctx.btn            or panic!("no button")           // assert
let cfg  = load_config()      or { e ->                       // handle
    log::warn("config: {} ({}:{})", e.message, e.location.file, e.location.line)
    Config{}
}
file.write(data) or ignore                                    // deliberately drop
```

**The single rule: an `or` clause must either produce a `T` or diverge.** Everything above
is an instance of it. The clause forms are:

| Form | Meaning |
|---|---|
| *expression* | the fallback value; must be a `T` |
| `{ … }` | a block whose final expression is a `T`, or which diverges |
| `{ e -> … }` | the same, with the `Error` bound to `e` |
| `return` | propagate the failure (§9.5) |
| `return expr` / `break` / `continue` | ordinary divergence |
| `panic!("…")` | abort (§9.8) |
| `unreachable` | assert non-failure; see below |
| `ignore` | statement position only; discards the value |

- The error binding must be named explicitly. There is no implicit `it`. `[PROPOSED]`
- On a `T?` or a `^T` there is no payload, so `{ e -> … }` is an error; write `{ … }`.
- `or` binds looser than `||` and tighter than assignment, and is **non-associative**
  (§11.1): `a() or b() or c` is a syntax error, not a fallback chain. The first clause has
  already produced a value or diverged, so a second `or` could only apply to something that
  can no longer fail. Comparison is non-associative for the same reason (§11.1).

**A fallback that can itself fail** gets its own clause, which is what the block form is
for:

```dz
let tex = load(path) or { load(fallback_path) or Textures::Missing }
```

Read outside-in: try `load(path)`; if that fails try `load(fallback_path)`; if that fails
too, use the placeholder. Each fallible call has exactly one clause, and the nesting is
visible rather than inferred from precedence. Flat chaining cannot express this and is
rejected instead of being silently misread. `[DECIDED]`

**`or unreachable`** asserts that the failure cannot happen. In debug builds it panics with
a `#location`; in release and shipping builds it is free, and the compiler assumes the
success path (`__builtin_unreachable`). Reaching it there is undefined behaviour. It is the
escape hatch for hot paths where a null check is provably redundant, and it is spelled
loudly on purpose. `[DECIDED — §18.20]`

**`or ignore`** is statement position only, because it neither produces a `T` nor diverges
and so has nothing to bind:

```dz
file.write(data) or ignore              // ok
let x = f() or ignore                   // error[DZ0524]: `ignore` produces no value
```

`ignore` is also valid as a bare statement inside a block clause, where it marks a
deliberate drop (§9.5). `[PROPOSED — this dual role is the part of the design least
supported by real code; revisit after the M12 stdlib pass.]`

**`if let`** is for when both outcomes carry on — "do this if it is there, that if it is
not, then continue" — which an `or` clause cannot say, since it must produce a value or
diverge:

```dz
if let m = find_movie(&mut list, "Nemo") {
    println!("found {}", m.id)        // m: Movie
} else {
    println!("not found")
}
```

**The binding holds the value as a `for` or `case` binding holds its element** (§10.3,
§10.4), whatever the subject — a `T?`, a `&T?`, a `^T`, or a handle (§14.5):

```dz
if let v = p { … }          // v: T, a copy that cannot be changed (`mut v`: one that can)
if let &v = p { … }         // v: &T, refers to the value where it lives
if let &mut v = p { … }     // v: &mut T, and writes land there
```

- **`v`** is a copy, so `T` must be copyable where the value lives somewhere else — behind a
  pointer, a reference or a handle (`DZ0403`, with `&v` as the help). Out of an optional
  the `if let` owns, such as a call's result, it is the value itself, moved.
- **`&v`** refers to the value in place; nothing is copied.
- **`&mut v`** needs something writable: a `^mut T`, a `&mut T?`, or an optional or handle
  in a place that may be written — a `let mut`, a field through `&mut self` (`DZ0507`).

So `if let &mut p = ptr` is the checked pointer-to-reference conversion of §5.2. The binding
exists in the first block only, `else if` chains as for any `if`, and `if let` is an
expression when both blocks produce a value. It does not take a fallible call: its `else`
has no `Error` to hand over, so the failure would be dropped unseen, which §9.5 exists to
prevent — that is `or`'s job (`DZ0529`). `[DECIDED — §18.36, §18.61]`

An optional can also be tested without taking the value out: `m == none` and `m != none`.
Nothing else compares a `T?` — two optionals have no `==` (§18.36).

## 9.5 Propagation: `or return`

Bare `or return` means **propagate the failure**, and is legal only where the enclosing
signature can carry it:

| Left side | Enclosing signature | `or return` |
|---|---|---|
| `!->` call | `!->` | returns the `Error` ✅ |
| `!->` call | anything else | error[DZ0516] — the error has nowhere to go |
| `T?` | `-> U?` | returns `none` ✅ |
| `T?` | `-> void` | returns ✅ (nothing is discarded) |
| `^T` | `-> void` | returns ✅ |
| `^T` | `-> U?` | returns `none` ✅ |

```dz
pub fn load_level(path: str, alloc: ^Allocator) !-> Level {
    let file = File::open(path) or return
    let hdr  = file.read_header() or return

    if hdr.magic != LEVEL_MAGIC {
        return error(ErrorCode::BadMagic, "not a level file: {}", path)
    }

    let mut data = List<u8>::try_new(hdr.size, alloc) or return
    file.read_into(&mut data) or return

    return Level::parse(data)
}
```

Reserving bare `or return` for propagation is what keeps "I passed this up" and "I threw
this away" from sharing a spelling. A non-fallible function that wants to bail on failure
writes the discard out:

```dz
fn update_hud(&mut self) {
    let cfg = load_config() or { ignore; return }               // explicit drop
    let cfg = load_config() or { e -> log::debug("{}", e.message); return }   // better
}
```

Any clause that binds and uses the error is self-evidently deliberate, so neither form
warns. `[DECIDED — 18.8]`

## 9.6 Fallback

The most common thing game code does with a failure is substitute something and keep
rendering, so that is the cheapest clause to write:

```dz
let tex = load_texture(path) or Textures::Missing
let cfg = load_config() or Config{}
let hp  = self.find_player(id) or return
```

A missing texture becomes the pink checkerboard, a missing config becomes the defaults, and
the frame keeps going.

## 9.7 Consumption

**A fallible call must be consumed in the statement in which it appears.** There is no `T!`
type, so there is nowhere for the value to go otherwise.

```dz
file.write(data)                        // error[DZ0510]: fallible call not consumed
file.write(data) or ignore              // ok — explicitly dropped
file.write(data) or return              // ok — propagated
let n = file.write(data) or 0           // ok — handled
f(g() or return)                        // ok — `or` is an expression
```

`T?` and `^T` are ordinary values and are not subject to this rule; they may be stored,
passed, and returned freely. The rule exists because a fallible call carries an `Error` that
someone has to look at. `[DECIDED]`

This replaces the v0.4 must-use rule. It is stricter — the old rule permitted a `!T` to sit
in a binding and be examined later — and that is the intended trade: a result that outlives
its statement is a design mistake in a language with no way to store one.

## 9.8 What is *not* an error: panics

Most things that go wrong in a game are not recoverable and should not be modelled as
errors. `panic!("...")` aborts with a message, a `SourceLocation`, and a stack trace in debug
builds. It formats as `println!` does (§12.9), but without allocating — the allocators
panic too — so a message longer than a few hundred bytes is cut.

The location is the `panic!`'s own line, unless a `SourceLocation` is passed as the **last
argument**, after the format arguments: `panic!("missing {}", what, loc)`. That is how a
helper reports its caller's line — it takes `loc: SourceLocation = #location` (§12.7) and
hands it on. A `SourceLocation` cannot be formatted, so it is never mistaken for a value.
`assert(cond, msg = "")` is the standard library's helper of this kind. `[DECIDED]`

Panics are for: out-of-bounds indexing, failed assertions,
exhausted fixed-size pools, and violated invariants. Errors are for: I/O, parsing,
user-supplied data, and anything crossing a boundary you do not control.

**Allocation failure is a panic by default.** An allocator's `alloc` returns `^mut u8` and
panics on failure (§14);
`fn try_alloc(n: usize) !-> ^mut u8` covers the rare code (console memory budgets, streaming)
that genuinely handles OOM. Making every allocation fallible would put `or return` on every
line of `Buffer` for a case that, on a PC game, means the process is already dead.
`[PROPOSED]`

Panics compile to a call into the Dazzle runtime shim (§16.4), never to a C++ exception.
The generated C++ builds cleanly with `-fno-exceptions`.

## 9.9 Reserved: `collect`

`collect` is reserved as a fourth clause for accumulating failures rather than acting on
each one — batch validation, asset manifests, anything where you want every error rather
than the first:

```dz
let mut errs: List<Error>
for path in manifest { load_asset(path) collect errs }
```

Nothing in this section forecloses it, because it is another eliminator rather than a new
type. Not in v0.5. `[OPEN — 18.18]`
