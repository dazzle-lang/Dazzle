---
id: D-0050
title: Slices, text and bytes, and moving out of raw memory
status: decided
---

**`xs[a..b]`** (and `xs[a..]`, `xs[..b]`) slices an array or slice with checked ends, and a pointer inside `unsafe` with its end written — chosen over an unsafe `slice(ptr, len)` builtin, since the range form also serves checked subslicing. **`s.len` and `s.ptr`** on a `str`; `s as []u8` is safe; **`str::from_utf8(bytes) -> str?`** is the checked conversion, and `bytes as str` and `p as cstr` the unsafe, unchecked ones — the user asked for the safe way to exist, not only the fast one. **`IndexMut`** beside `Index`, chosen when the container is writable. **`move_out(dst)`** completes `construct`/`destroy`: a container cannot return an element by value without it (§5.3, §11.2, §14.3)
