# SDL3 lit scene

![A gold trefoil knot over a dais in a field of grass, ringed by pillars and three coloured lamps, under a blue sky with clouds](screenshot.avif)

A real-time 3D scene on [SDL3](https://libsdl.org) and OpenGL 3.3. A gold knot turns over a
dais in a field of grass, three lamps circle it, and the sun goes slowly round under drifting
clouds. All the lighting is computed per pixel on the GPU: soft shadows from the sun,
highlights and sky reflections, fog that fades into the sky, and the glow of the air round
each lamp. Four hundred thousand blades of stylized grass stir in the wind, drawn in one
call. SDL3 is built from its own source by its own CMake, which `dz` runs, and called
directly from Dazzle.

```bash
dz run examples/sdl3
```

Drag with the mouse to turn the camera, scroll to move it nearer, Space pauses, and Escape
quits.

## What it shows

- **A large C library in three lines.** SDL3's recipe in [`dazzle.toml`](dazzle.toml) names
  its CMake project and nothing else that says how to build it:

  ```toml
  [cpp.libs.sdl3]
  cmake   = "../vendor/SDL"
  options = { SDL_SHARED = false, SDL_STATIC = true, SDL_TEST_LIBRARY = false, SDL_EXAMPLES = false }
  import  = ["SDL3/SDL.h"]
  ```

  `dz` configures, builds and installs it into `build/cmake/` with the compilers it uses
  itself. Dazzle code sees the headers the build installed, and the link line comes from the
  `sdl3.pc` it installed. The first build takes under half a minute; after that SDL3 is built
  again only when a file in it changes.
- **C enums, unions and macros.** `sdl::SDL_Event` is a C union, read by its fields:
  `ev.key.key == sdl::SDLK_ESCAPE`, `ev.motion.xrel`. A typed enum such as
  `sdl::SDL_GLAttr::SDL_GL_DEPTH_SIZE` cannot be passed where another is wanted. A macro like
  `SDL_WINDOW_OPENGL` is an untyped integer, so the flags are combined into a variable
  annotated with SDL3's own type:

  ```dz
  let flags: u64 = sdl::SDL_WINDOW_OPENGL | sdl::SDL_WINDOW_RESIZABLE | sdl::SDL_WINDOW_HIGH_PIXEL_DENSITY
  ```

- **OpenGL through glad, called directly.** Past OpenGL 1.1 every function is asked of the
  driver at run time. [glad](https://github.com/Dav1dde/glad), the usual loader, declares
  each one as a macro over a function pointer, `#define glClear glad_glClear`, and Dazzle
  imports those like any other name: `gl::glClear(gl::GL_COLOR_BUFFER_BIT)`. `main` hands
  glad SDL3's lookup function, a C function passed as a value:

  ```dz
  if gl::gladLoadGL(sdl::SDL_GL_GetProcAddress) == 0 {
      eprintln!("no OpenGL 3.3 driver")
      return
  }
  ```

- **Instancing with no vertex data.** The grass is one `glDrawArraysInstanced` call of
  four hundred thousand instances and no vertex buffer: the vertex shader builds each blade
  from `gl_InstanceID` and `gl_VertexID`, places it on a jittered grid, and stirs its tip
  in the wind.
- **Shaders as raw strings.** The GLSL in [`shaders.dz`](src/shaders.dz) is written between
  backticks, with no escapes. `glShaderSource` takes several strings, so the shared code is
  written once and passed as a slice in front of each shader:
  `link(&gl, &[VERSION, LIT_VS], &[VERSION, COMMON, LIT_FS])`.
- **Cleanup with `Drop` and `defer`.** The `Renderer` deletes its GL objects in `drop`, and
  `main` closes the context, the window and SDL3 with `defer`. Drops run before defers when a
  scope ends, so the GL objects are deleted while their context still exists.
- **The standard library's math.** `Mat4::perspective`, `look_at`, `orthographic` for the
  sun's shadow map, and `inverse`, which returns an optional because not every matrix has
  one.

| File | What it holds |
|---|---|
| [`main.dz`](src/main.dz) | The window, the context, and the event loop |
| [`scene.dz`](src/scene.dz) | What is in the scene at a time `t`, and the orbiting camera |
| [`mesh.dz`](src/mesh.dz) | The geometry: the ground, a cube, a sphere and a trefoil knot |
| [`renderer.dz`](src/renderer.dz) | The GL objects, the shadow pass, the lit pass and the grass |
| [`shaders.dz`](src/shaders.dz) | The GLSL: sky, shadows, lighting, grass, fog and tone mapping |

## Platforms

Linux, Windows and macOS, wherever CMake is installed; on Windows, Ninja too. On Linux SDL3
loads the window system when it starts, so building needs only its headers (`libx11-dev`,
`libxcursor-dev`, `libxi-dev` and `libxrandr-dev` on Debian and Ubuntu, and Wayland's if
you have them).

## Use it as a template

Copy the folder, point `cmake` in `dazzle.toml` at your copy of SDL3, and replace the scene.
SDL3's own build options, such as `SDL_AUDIO = false`, go in `options`.
