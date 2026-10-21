---
id: D-0045
title: A union variant that is an existing struct
status: decided
---

**A bare variant names a struct in scope and carries it; a variant with no payload is written `Point {}`.** `union ButtonState { DefaultState  TransitionState }` holds a `DefaultState` or a `TransitionState`, each built as that struct is built — its fields, defaults and visibility — and bound by `case DefaultState(d)` as that struct, methods included. A bare name that is no struct in scope, names something else, or names a generic struct is an error (DZ0551) rather than a variant with nothing in it: giving the bare spelling both meanings would let a misspelling, or a struct added or removed elsewhere, silently change what a union holds. `Point {}` is already how a variant with an empty field list was written, so nothing new is spelled; `Shape::Point`, with no braces, still names one (§7.5)
