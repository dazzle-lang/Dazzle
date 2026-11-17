---
id: T-0008
title: Variadic C functions
milestone: M7b
status: done
kind: feature
area: interop
where: `function` and `printf_format` in `crates/dz_cppimport/src/clang.rs` (the attribute, read from the pretty-printed declaration); `DeclKind::Fn::printf`; `TypeTable::printf_fns` in `crates/dz_check/src/lib.rs`; the `"%s"` in the call, `crates/dz_cpp/src/lib.rs` before `let call = format!("{target}(…)")`; tested by `tests/run/cinterop` (`mini_log`) and `examples/sdl3/src/ui.dz`
---

**Done 2026-11-17, the user's choice of the first option, made safe (D-0111, r143).** A
variadic free function declared `format(printf, N, N+1)`, its format the last fixed
parameter, imports as taking one text there, and the call passes it after `"%s"`:
`ImGui::Text(format!("{} fps", fps))` is `ImGui::Text("%s", text)`, so the "catch" below, a
`%` in the user's text, is gone. libclang does not expose the `format` attribute, but prints
it with the declaration (`clang_getCursorPrettyPrinted`), which is read for the kind
(`printf`, `gnu_printf`, `__printf__`) and the two indices. Every other variadic stays out:
one without the attribute (`TraceLog`), one whose format is not last, a printf-style method,
and the `va_list` forms. The SDL3 panel writes `ImGui::Text` and `SetItemTooltip`.

What follows is the task as it was written.

**What is missing.** A C function declared with a trailing `...` — `printf`,
`ImGui::Text(const char* fmt, ...)`, `TraceLog(int level, const char* text, ...)` — is
**skipped by the importer** and recorded in `Header::skipped` with that as the reason
([T-0015](T-0015-declarations-left-out-import.md)). It is not reachable by any spelling, so
a header's variadic functions simply are not part of the imported module.

**Why it matters.** Two kinds of C API are built on it. The `printf`-family logging that
nearly every C library exposes as its diagnostic channel (raylib's `TraceLog`, SDL's
`SDL_Log`), and — the one that actually hurts — **Dear ImGui's text API**: `ImGui::Text`,
`TextColored`, `LabelText` and `BulletText` are all variadic, and `ImGui::Text` is the single
most-called function in any debug UI. Without it ImGui is reachable only through the parts
of its surface that are not variadic.

**Why it is hard, and the honest first cut.** Dazzle has no varargs of its own and should not
get them: §12.9 exists precisely so that a format string is checked at compile time rather
than trusted at run time, and a `printf`-style call is the mismatch-corrupts-the-stack
failure the design rules out. So this is not "add varargs to Dazzle" — it is "call a C
variadic from Dazzle", and the useful question is what the call site looks like.

1. **The narrow fix, and the right one for ImGui**: import a variadic function as if it took
   only its named parameters, and call it with a **single already-formatted argument**. Every
   variadic C function in practice has a non-variadic use — `ImGui::Text("%s", s)`, or better
   `ImGui::Text(text)` where `text` is a `cstr` with no specifiers in it. So
   `ImGui::Text(hud.as_cstr())` covers the real use, needs no varargs, and composes with
   `format!` on the Dazzle side where the checking happens:

   ```dz
   let hud = format!("{} hp", player.hp, allocator = Allocators::Temp)
   ImGui::Text(hud.as_cstr())
   ```

   This needs [T-0006](T-0006-cstr-from-runtime-str.md) and nothing else, and it is worth
   checking whether "import the named parameters only, and say so" is all that is actually
   owed here. The catch is that a `%` in the user's own text would then be read as a
   specifier by the C side, so the imported signature should be the one that takes a format
   string (`"%s"`) and the wrapper should pass it.
2. **The general fix**: pass a spread to a C variadic, which means Dazzle emitting a C
   variadic call with the default promotions (`f32` → `double`, small ints → `int`) applied
   — mechanical in codegen, but it is an unchecked call, and deciding whether this language
   wants to offer one at all is the user's call.

The decision to make before writing code is which of those two this row means. If it is the
first, it is small; if it is the second, it wants a decision file for the `unsafe` story
around it.
