---
id: T-0237
title: "A union payload field's default is accepted, never checked, and never applied"
milestone: M12
status: done
kind: bug
area: checker
---

Found 2026-11-16 writing `tests/run/value_shapes` for T-0231. A field of a variant's own
payload may be written with a default, as a struct field is:

```dz
union U {
    A { x: i32 }
    B { y: i32, w: i32 = "text" }
}

fn main() {
    let u = U::B{ y: 2 }       // accepted: `w` "has a default"
    switch u {
        case A(a) { println!("{}", a.x) }
        case B(b) { println!("{} {}", b.y, b.w) }    // prints `2 0`
    }
}
```

- **Never checked.** The checker marks the field `has_default`, so a literal may leave it
  out (`DZ0512` is not raised), but builds no body for the expression: a default of the
  wrong type, `"text"` for an `i32`, is accepted. A struct's field defaults are checked as
  `BodyOwner::FieldDefault` (`check.rs`, the `MemberKind::Field` arm); a variant's fields
  have no such owner.
- **Never applied.** The generated payload struct declares every field `T name{}`, so a
  field left out is zero, not its default: `w: i32 = 1` left out reads `0`.
- (The formatter also dropped the default until T-0236.)

**Spec §7.5 does not say** whether an inline payload's fields take defaults: it gives
defaults only to a variant that carries a struct, through the struct's own fields. So this
is the user's call: either refuse a default on a payload field, with a diagnostic, or define
it as a struct field's default (checked against the field's type, evaluated where a literal
leaves the field out) and add it to §7.5 with a revision row.

| Piece | Where |
|---|---|
| A variant's fields, `has_default` | the union arm of the table's signature building, `crates/dz_check/src/lib.rs` |
| Where struct field defaults get a body | `MemberKind::Field` in `crates/dz_check/src/check.rs` |
| The payload struct's fields, emitted | `union_decl` in `crates/dz_cpp/src/lib.rs` |
| A literal that leaves a payload field out | `union_literal` in `crates/dz_cpp/src/lib.rs` (the general path) |

**Done 2026-11-16 (r136), the user's choice of (b): a payload field's default is a struct
field's.**

- **Checked.** Each default is a body of its own, `BodyOwner::PayloadDefault { owner,
  variant, index }`, checked against the field's type: `w: i32 = "text"` is `DZ0501` at the
  default. `a_payload_default_is_checked_against_its_field` in `dz_check/tests/check.rs`,
  and the matched pair: `Pane` in `typed/` and `Margin` in `typeerrors/`.
- **Applied.** The generated payload struct gives the field its default as a member
  initialiser, so a literal that leaves it out, either path (`union_literal` builds the
  payload in place or at its defaults), and `let u: U` hold it. A plain-data union's literal
  now builds the payload before it sets the fields written: it set only the tag and those
  fields, so a field left out was whatever the storage held, which only a default could
  reach. A default calling a function of its module is declared first, as T-0167 did for a
  struct field's (`field_default_prototypes`).
- `tests/run/value_shapes` holds the values: trailing and middle fields left out, a
  `String` default, one calling a function declared later, a generic union's, and the
  union's own default, the same on clang++ and g++ and warning-clean on both.
