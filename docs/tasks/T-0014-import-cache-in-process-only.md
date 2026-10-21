---
id: T-0014
title: The import cache is in-process only
milestone: M7a
status: open
kind: deferral
area: interop
where: `dz_cppimport`, the header cache
---

**What exists.** Reading a header with libclang is the slowest thing `dz` does — parsing
`raylib.h` and its includes is on the order of a few hundred milliseconds — so the result is
cached. But the cache lives **in the process**: it makes the second import of a header
within one run free, and it makes the language server fast, since the LSP is a long-lived
process and so pays the cost once per session.

**What is missing.** §15.1 wants it **on disk** as well, so that a `dz check` or `dz build`
on an unchanged header does not re-parse it. Every command-line invocation currently pays the
full parse.

**Why it matters, and why it is low priority.** The LSP — where latency is felt keystroke by
keystroke — is already covered, which is why this is a deferral rather than a bug. What is
left is the build: a `dz check` of a project importing raylib pays a few hundred milliseconds
it need not, which is noticeable against principle 6 ("compile fast") but is a constant, not a
per-file cost, and is dwarfed by the C++ compile that follows a `dz build`. It would matter
more in CI, where every job is a cold process, and more again once there are several imported
libraries ([T-0010](T-0010-proof-on-real-libraries-dear-imgui.md) adds two).

**What it would take.** The parsed result has to be serialised, and that is the whole
question — what gets written is not the libclang AST but the **plain declarations** the
importer produces from it (§15), which is a much smaller and more stable thing. So:

1. Serialise the importer's output (the module's declarations, and `Header::skipped`) rather
   than anything of libclang's.
2. Key it on what would invalidate it: the header's path and content hash, **every
   transitively included header's** hash, the include paths, the target triple
   ([T-0020](T-0020-libclang-parses-with-its-own-default.md) — which has to be known for this
   to be correct at all), and the importer's own version. Getting the key wrong is worse than
   having no cache: a stale import is a miscompile.
3. Put it under the build directory, so it is cleaned with everything else and never checked
   in.
4. libclang's own `clang_saveTranslationUnit` is the alternative and should be measured first
   — it may be faster to reload a PCH than to deserialise, but it ties the cache to the
   libclang version, which the shipped-beside-`dz` plan
   ([T-0076](T-0076-release-workflow-installer-with-libclang-bundled.md)) makes a moving part.
