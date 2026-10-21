---
id: D-0040
title: A C type whose definition its header does not show
status: decided
---

**It imports as a handle**: a type used behind `^T` or `^mut T` and never by value — no local, field, parameter, return or `p^`, and not copyable, because nothing outside the library knows its size. That is how `RGFW_window`, `GLFWwindow`, `sqlite3` and `FILE` are handed out, and the functions taking one are then callable as they are in C. A type the header never mentions at all, declared in a system header it includes, is adopted the same way **when every use of it is behind a pointer**; where one is used by value the declaration that does so is left out instead, since its size really is unknown. Refusing such a header outright was the alternative, and would have shut out most C libraries (§15.2)
