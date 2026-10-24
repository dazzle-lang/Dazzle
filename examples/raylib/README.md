# raylib

![A window saying Hello from Dazzle](screenshot.png)

The smallest windowed program: a [raylib](https://www.raylib.com) window, text, and a dot
that follows the mouse, in sixteen lines of Dazzle.

```bash
dz run examples/raylib
```

```dz
fn main() {
    rl::InitWindow(800, 450, "Dazzle + raylib")
    defer rl::CloseWindow()
    rl::SetTargetFPS(60)

    while !rl::WindowShouldClose() {
        rl::BeginDrawing()
        rl::ClearBackground(rl::RAYWHITE)
        rl::DrawText("Hello from Dazzle", 190, 200, 40, rl::DARKGRAY)
        rl::DrawCircleV(rl::GetMousePosition(), 8, rl::MAROON)
        rl::EndDrawing()
    }
}
```

## What it shows

- **raylib compiled from its own source.** The recipe in [`dazzle.toml`](dazzle.toml) names
  raylib's `.c` files and the system libraries each platform needs, and `dz` builds them: no
  CMake, no Make, no prebuilt binaries.
- **The header, read as it is.** `rl::InitWindow`, `rl::Vector2` and `rl::RAYWHITE` come from
  `raylib.h`, read by libclang. `RAYWHITE` is a macro in C, and Dazzle still knows it is a
  `Color`.
- **`defer`** closes the window however `main` ends.

For a whole game on top of this, see [`examples/breakout`](../breakout).
