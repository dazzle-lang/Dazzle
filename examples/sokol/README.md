# sokol particle fountain

![A fountain of glowing particles](screenshot.gif)

Four thousand particles with additive blending, drawn with
[sokol](https://github.com/floooh/sokol): `sokol_app` for the window and the frame loop,
`sokol_gfx` for the GPU, and `sokol_gl` for immediate-mode drawing. All of it is single-header
C, compiled from source by `dz` and called directly from Dazzle.

```bash
dz run examples/sokol
```

The fountain follows the mouse, a click throws a burst, and Escape quits.

## What it shows

- **Several single-header libraries as one module.** The recipe in
  [`dazzle.toml`](dazzle.toml) compiles every sokol implementation in one translation unit
  and imports all five headers as `sk`, so `sk::sapp_run`, `sk::sg_setup` and
  `sk::sgl_begin_quads` sit side by side.
- **C callbacks with user data.** sokol owns the frame loop and calls back for init, each
  frame, each event and cleanup. The callbacks are plain Dazzle functions, and sokol passes
  each one a pointer to the `Fountain` that `main` owns:

  ```dz
  fn frame(data: ^mut void) {
      let app = app(data) or return          // a checked pointer becomes a reference
      app.frame()
  }
  ```

- **C structs with nested arrays**, such as a pipeline's blend state, set field by field:
  `desc.colors[0].blend.enabled = true`. Array indexing on them is bounds-checked like any
  Dazzle array.
- **C enums, typed.** `sk::sg_blend_factor::SG_BLENDFACTOR_ONE` is a value of the header's
  own enum type, so passing the wrong enum's value is a compile error.

## Use it as a template

Copy the folder, point `root` in `dazzle.toml` at your copy of sokol, and replace the
`Fountain`. The backend is chosen by `SOKOL_GLCORE` in `[cpp]`; any of sokol's other backends
is a define away.
