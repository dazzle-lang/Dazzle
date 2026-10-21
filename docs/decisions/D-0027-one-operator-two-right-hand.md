---
id: D-0027
title: One operator, two right-hand sides
status: decided
---

**A type may implement one trait at several arguments**, one `impl` each, and the argument's type picks one — an exact match, then an untyped literal's default type, else an error asking for a type. The one overloading there is, and only for this; a repeated method name in a struct is still an error (§11.2)
