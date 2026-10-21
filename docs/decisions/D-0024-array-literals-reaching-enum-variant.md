---
id: D-0024
title: Array literals, and reaching an enum variant without naming its type
status: decided
---

**`[a, b, c]`, with the length exactly as written**: no repeat form (§6.2 already zeroes an array, and `;` is a terminator so `[0; 8]` is unavailable), no short form, and a literal may not be *stored* as a slice though it may be passed as one (§5.3). **`.Selected` works wherever the enum is already known** — the same mechanism a union variant already used — while `Sprite::Selected` always works and is what an uninferrable position needs (§7.4). `Sprite.Selected` stays invalid: `.` is the runtime operator (§3.3)
