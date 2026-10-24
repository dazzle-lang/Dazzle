# RGFW plasma

![A blue and gold plasma, moving](screenshot.gif)

A window from [RGFW](https://github.com/ColleagueRiley/RGFW), a single-header C windowing
library, showing a plasma computed pixel by pixel in Dazzle. There is no GPU API at all: the
program fills a buffer of pixels, and RGFW copies it to the window each frame.

```bash
dz run examples/rgfw --release
```

Escape or the window's close button ends it.

## What it shows

- **A single-header library from a five-line recipe.** `header = "RGFW.h"` in
  [`dazzle.toml`](dazzle.toml) has `dz` compile RGFW's implementation in a translation unit
  of its own, with `RGFW_IMPLEMENTATION` defined there and nowhere else.
- **Memory shared with C, safely owned.** The pixels are a Dazzle `List<u8>`, and RGFW's
  surface points into it. The list is sized once and never grows while the surface exists,
  and both are released by `defer`, in the right order.
- **`defer` for C resources.** The window, the surface and RGFW itself are each closed by a
  `defer` written next to the call that opened it, and they run in reverse order.
- **Plain loops over a quarter of a million pixels a frame**, with the standard library's
  `sin`, `cos` and `sqrt` and bounds-checked indexing.
