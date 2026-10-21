---
id: T-0057
title: A spread cannot be formatted
milestone: M12
status: open
kind: deferral
area: checker
where: `dz_check/src/check.rs`, `format_macro`
---

**A spread cannot be formatted**: `#format("{} {}", ..xs)` is not an invocation the macro accepts, since how many values a spread holds is not known where the string is checked. Pass the values one by one
