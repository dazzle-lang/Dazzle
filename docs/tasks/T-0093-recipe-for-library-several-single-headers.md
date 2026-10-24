---
id: T-0093
title: A recipe for a library of several single headers is not documented
milestone: none
status: open
kind: feature
area: llm-tooling
---

**What it is.** Writing `examples/sokol` on 2026-10-24, an LLM had to read the compiler
(`crates/dz/src/native.rs`, `implementation_unit`) to learn how to build sokol. That is the
failure this area exists to prevent: everything needed to use a library should be in the spec,
the diagnostics or `dz explain`.

sokol is several single-header libraries that depend on each other. `sokol_gl.h` refuses to
compile unless `sokol_gfx.h` was included before it, and every header's implementation is
switched on by one `SOKOL_IMPL`. Spec §15.1 says `header` names "a single-header library" and
that `dz` writes the one translation unit including it. It does not say:

- that this unit contains `#include "<header>"` and nothing else, so nothing can come before
  it;
- whether `header` takes a list (it does not);
- how to build several headers in one unit. What works is the library's `flags` with
  `-include` (GCC's and Clang's "include this file first"):

  ```toml
  [cpp.libs.sokol]
  root         = "../vendor/sokol"
  header       = "util/sokol_gl.h"
  include_dirs = [".", "util"]
  defines      = ["SOKOL_IMPL"]
  flags        = ["-include", "sokol_log.h", "-include", "sokol_gfx.h",
                  "-include", "sokol_app.h", "-include", "sokol_glue.h"]
  import       = ["sokol_log.h", "sokol_gfx.h", "sokol_app.h", "sokol_glue.h", "sokol_gl.h"]
  ```

  It works, but nobody would find it without reading the compiler, and it is a trick rather
  than something the recipe says.

**Why it matters.** sokol is not unusual: the stb family and many other header-only libraries
are often used several at a time, and C++ libraries built this way are common. Each is a place
where an LLM, or a person, gets stuck or guesses.

**Other gaps from the same session.** These cost a compile-and-retry round each rather than a
read of the compiler, and belong with the same documentation:

- A field of an imported C enum type cannot be compared with that enum's enumerator constant:
  `e.type == sk::SAPP_EVENTTYPE_MOUSE_MOVE` is `DZ0514`, while
  `event.type == rgfw::RGFW_keyPressed` works, because RGFW's field is a plain integer typedef.
  What works is `switch e.type { case SAPP_EVENTTYPE_MOUSE_MOVE { … } }` or
  `sk::sapp_event_type::SAPP_EVENTTYPE_MOUSE_MOVE`. §15.2 says each enumerator is also an
  untyped constant, but not that the constant does not compare with the enum's own type. The
  diagnostic's help could name the typed spelling.
- Handing a Dazzle value to C as `void*` user data takes two steps,
  `let p: ^mut T = &mut x` and then `unsafe { p as ^mut void }`. `&mut x as ^mut void` parses
  as `&mut (x as ^mut void)`. A callback-with-user-data pattern in §15.2 would cover it.
- `Random::range(0.6, 1.0)` infers `f64`, which then does not mix with an `f32`. The
  diagnostic says so clearly, so this one only needs an example.

**What it would take.** Documentation, and possibly a small recipe change, which would be the
user's call:

1. Spec §15.1: say what the generated implementation unit contains, and give the sokol recipe
   above as the way to build interdependent single headers. Or let `header` take a list,
   included in order in one unit, so the `-include` trick is not needed. That is a manifest
   change, so a §21 row.
2. A "using a C library" section in §15 with the patterns every example needed: callbacks with
   user data, C enums, `void*`, a pointer checked once into a reference, and freeing with
   `defer`. `examples/` already has working code for each.
3. Point `dz explain DZ0514` at the typed-enum spelling when one side is an imported enum.

Related: [T-0061](T-0061-api-dump-dz-api-module.md) (`dz api`), which would show what an
import produced, enum types included, without reading anything else.
