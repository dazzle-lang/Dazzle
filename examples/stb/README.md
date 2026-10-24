# Ray tracer

![Spheres on a checkered floor, with reflections and shadows](screenshot.png)

A small ray tracer: spheres, a checkered floor, soft shadows, highlights and mirror
reflections, rendered on the CPU and saved with
[stb_image_write](https://github.com/nothings/stb), a single-header C library called directly
from Dazzle. This image is its output.

```bash
dz run examples/stb --release
```

```text
rendered 960x540 in 1669 ms: render.png
```

Sixteen rays a pixel on a jittered 4×4 grid, a light with an area for soft shadows, and up to
five bounces between mirrors: more than eight million rays from the camera alone, in under two
seconds on one core of a desktop machine.

## What it shows

- **Tagged unions and exhaustive `switch`.** A `Shape` is a sphere or a floor, a `Material`
  is matte, mirror or checkered, and each is handled by a `switch` the compiler checks for
  missing cases:

  ```dz
  union Material {
      Matte { color: Vec3 }
      Mirror { tint: Vec3 }
      Checker { a: Vec3, b: Vec3 }
  }
  ```

- **Optionals.** `intersect` returns a `Hit?`, and the closest hit is taken with
  `if let`. A ray that hits nothing returns the sky in one line:
  `let hit = self.closest(ray, 1000) or return self.sky(ray.dir)`.
- **Errors as values.** Writing the file can fail; `save_png` says so with `!->`, and `main`
  handles it with `or`.
- **The standard library's math.** `Vec3` with `+`, `-`, `*`, `dot`, `reflect` and
  `normalized`, `Random` for the jitter, `Instant` for the timing.
- **A single-header library with no C file.** `header = "stb_image_write.h"` in
  [`dazzle.toml`](dazzle.toml) has `dz` write the one translation unit that compiles it.
  `stb_image.h` (loading images) and `stb_truetype.h` (fonts) are used the same way.
