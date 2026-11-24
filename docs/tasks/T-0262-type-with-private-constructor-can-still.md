---
id: T-0262
title: A type with a private constructor can still be default-built
milestone: M19
status: done
kind: bug
area: checker
where: `crates/dz_check/src/check.rs` (the `(Some(d), None)` arm of a `let`, beside `holds_reference`); field declarations in `crates/dz_check/src/lib.rs`; `StructSig::no_default`
---

**What happens.** D-0115 made a constructor private unless it says otherwise, and an explicit
call now respects that. Default initialisation is not a call, so it still builds the value
from any module:

```dz
// module `a`
pub struct Token {
    pub n: i32 = 7
    Token(a: i32) { n = a }
    pub fn make() -> Token { return Token(1) }
}

// another module
let t: a::Token             // accepted, `t.n` is 7: no constructor ran
```

With a private **zero-argument** constructor, the same line runs that constructor from outside
the type. Each of these reaches it too, all checked on 2026-11-23: an array or tuple of the
type, a field of the type with no default, `let x: T` in a generic function instantiated
with it, and a generic struct's `T` field.

**Why it matters.** A private constructor is how a type keeps an invariant, and today the
invariant holds only for values that came through a call.

**What to decide; it is the user's.** §6.2 already says, as `[PROPOSED]`, that `let f: File`
is legal only if `File` has no constructor or has one callable with zero arguments, and the
compiler does not enforce it. Against that, "every Dazzle type has a default" carries weight
elsewhere: a moved-from value goes back to its defaults (§6.4), `clone` runs on a value at its
defaults (§6.6), `List::resize` fills with them, and a generic body may write `let x: T`.

1. **Enforce §6.2 at the places that are written**: `let`, arrays, tuples and field
   declarations, reading "callable" as "callable from here". Generic code stays a gap.
2. **Close generics too**: a type whose default is not public does not satisfy a plain `T`
   where the body default-builds one, which needs a bound and changes §8.
3. **Keep defaults as they are**, and say in §4 that a private constructor restricts calls
   only.

**Done when** the choice is in `docs/decisions/`, and each program above is either refused or
documented as allowed, with a `tests/errors` case for what is refused.

**Done** (2026-11-23, r148): the user chose to do as Rust and C++ do, so every program above is
refused with `DZ0590`.

- **Written bindings.** `missing_default` in `dz_check` answers whether a type can be built at
  its default from a given place. A `let` with no value asks it.
- **Constructors.** `require_field_values` needs each field with no usable default assigned at
  the body's top level, before any `return`.
- **Generics.** Monomorphization asks it of each instance, for a `let x: T` and for
  `List::resize`, extending T-0230's refusal.

This found that the C++ also ran a field's zero-argument constructor before assigning it, and
on every move. [T-0263](T-0263-field-zero-argument-constructor-runs-again.md) fixed that (r149).
