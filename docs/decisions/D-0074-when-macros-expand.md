---
id: D-0074
title: When macros expand
status: decided
---

**On syntax alone: after parsing and `#if`, before name resolution.** A macro sees tokens, never types, so each module expands on its own, the editor parses an invocation as it is typed, and nothing depends on checking order. Chosen over expanding after checking, which would let a macro iterate `T::fields` but would make expansion depend on types that depend on expansion. A macro that needs a type's fields takes its declaration (§12.11, §13.4)
