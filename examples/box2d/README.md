# Box2D physics

![A pyramid of boxes knocked down by thrown balls](screenshot.gif)

Rigid-body physics with [Box2D 3](https://box2d.org), drawn with
[sokol](https://github.com/floooh/sokol): two C libraries in one project, each compiled from
its own source by `dz` and called directly from Dazzle.

```bash
dz run examples/box2d --release
```

A click throws a ball at the pyramid, Space drops a rain of boxes, R builds the pyramid again,
and Escape quits.

## What it shows

- **A C library with 43 source files and no build system.** Box2D's recipe in
  [`dazzle.toml`](dazzle.toml) is five lines: every `.c` under `src/`, compiled as C17, with
  its headers in `include/`. `dz` compiles it once and then only what changed.
- **C structs and handles as ordinary values.** `b2::b2WorldId` and `b2::b2BodyId` are
  Box2D's own handle structs; `b2::b2DefaultBodyDef()` returns a struct whose fields are set
  with `.`, as in C:

  ```dz
  let mut def = b2::b2DefaultBodyDef()
  def.type = b2::b2BodyType::b2_dynamicBody
  def.position = b2::b2Vec2{ x: at.x, y: at.y }
  let body = b2::b2CreateBody(m_world, &def)
  ```

- **Callbacks into Dazzle.** sokol runs the frame loop and calls Dazzle functions for each
  frame and event, passing back a pointer to the `Demo` that `main` owns. Each callback turns
  that pointer into a reference once, and everything after it is safe code.
- **Two libraries side by side.** `b2::` and `sk::` are the names the manifest gives them,
  and nothing else in the program needs to know where they came from.

## Use it as a template

Copy the folder, point both recipes' `root` at your copies of Box2D and sokol, and replace
`src/main.dz`. The sokol half is explained line by line in
[`examples/sokol`](../sokol/dazzle.toml).
