---
id: T-0053
title: `error(…)` shares a static ring of four buffers
milestone: M12
status: open
kind: deferral
area: codegen
where: `dz_cpp/src/runtime.rs`, `message_buffer`
---

`error(…)` writes into a static ring of four 256-byte buffers, so the fifth live message overwrites the first. Kept by the user's decision (2026-09-13, `docs/log/m12.md`): §9.2 requires no allocation *and* a message that outlives the call. `format` no longer shares it — it is `String::format` since r62, `#format` since r64
