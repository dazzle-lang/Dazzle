---
id: D-0072
title: Arguments passed by name
status: decided
---

**`name = value`, for the parameters after a variadic one and for nothing else**, after the positional arguments, in any order; one with a default may be left out whatever else is given, one without must be passed. Chosen by the user for `String::format("…", x, allocator = Allocators::Temp)`: a variadic parameter takes every positional argument after the fixed ones, so an allocator or a `#location` after it was unreachable, and putting the allocator first would make every call name it. Naming is not extended to ordinary parameters, which keeps one way to pass each argument; it can be widened later without breaking a program. Revises 18.56's "last"
