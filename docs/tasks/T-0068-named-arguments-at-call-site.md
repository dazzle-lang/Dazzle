---
id: T-0068
title: Named arguments at the call site
milestone: none
status: open
kind: design
area: language
---

**design: named arguments at the call site**, e.g. `DrawRectangle(x: 10, y: 20,
w: 50, h: 50)`. Argument order is a classic model mistake, worst against C APIs taking
six floats in a row. Dazzle has default arguments (§7.2) but no names; names would also
strengthen 18.5's case for leaving overloading out.
