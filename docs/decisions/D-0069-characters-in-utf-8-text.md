---
id: D-0069
title: Characters in UTF-8 text
status: decided
---

**Positions are bytes; characters are decoded by iteration.** `String` keeps UTF-8 as it always did, and gains `chars()` (and `for c in s`), `char_count`, `pop`, `is_char_boundary`, `slice` and `truncate`, every cut checked against a character boundary. Rejected: indexing by character, which is a linear walk disguised as `s[i]`, and UTF-32 storage, which quadruples memory for the text a game mostly holds. Invalid bytes decode as U+FFFD rather than panicking, since only unchecked code can make them (§14.4)
