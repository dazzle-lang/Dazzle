---
id: D-0111
title: "C variadics of printf's kind take one text; imported enums stay closed"
status: decided
---

**A C or C++ variadic function of `printf`'s kind, declared `format(printf, N, N+1)` with its
format the last fixed parameter, is imported as taking one text in place of `fmt, ...`, and
the call passes it after a `"%s"` format: `ImGui::Text(format!("{} fps", fps))` is
`ImGui::Text("%s", text)`. Every other variadic function stays out of the import. An
imported enum's variants stay a closed set to `switch`, as today, and reflection now lists
them with the header's values. Chosen by the user on 2026-11-17, in T-0008 ("printf-style
ones, safely") and T-0048 ("keep closed, as today"); spec §15.2, r143.**

**Variadics: the options.**

1. **printf's kind, safely (taken).** The format attribute, which ImGui writes `IM_FMTARGS`
   and SDL `SDL_PRINTF_VARARG_FUNC`, tells the importer which parameter is the format; the
   text Dazzle passes goes after `"%s"`, so a `%` in it is printed rather than read as a
   specifier, and no argument crosses that C reads with a type it guessed. Formatting stays
   on the Dazzle side, where `format!` checks it at compile time (§12.9). It covers what
   ImGui's text API, tooltips and tree nodes need, and SDL's logging.
2. **Any variadic, as `unsafe`** (not taken). Extra arguments passed with C's promotions,
   `f32` to `double` and small integers to `int`, in an `unsafe` block, since a wrong format
   corrupts memory. It would reach `TraceLog`, which declares no format attribute, at the
   price of the one failure §12.9 exists to rule out.
3. **Both** (not taken).

A function whose format is not its last fixed parameter, a printf-style *method*, and the
`va_list` forms (`vprintf`) are left out too.

**Enums.** A C enum is often a flag set and may hold a value outside its list, so requiring
a `default` arm on a `switch` over one was offered, for plain enums or for every imported
one. The user kept them closed: a value outside the list reaches the end of the `switch`,
which panics where it must produce a value, and runs no arm where it is a statement. No
undefined behaviour either way.
