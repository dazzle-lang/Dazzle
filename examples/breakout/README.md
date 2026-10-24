# Breakout

![Breakout, mid-game](screenshot.png)

A complete game, written with no C++ of its own. The rules are plain Dazzle over the standard
library, in their own module with no window and no renderer; [raylib](https://www.raylib.com)
draws them and reads the keyboard, called directly from Dazzle.

```bash
dz run examples/breakout
```

Left and right move the paddle, Space launches the ball, and R starts again. The best score
is kept in `saves/best.txt`.

## What it shows

- **A game split into modules.** [`game/`](game) is the rules: `Game`, `Brick`, `Phase` and
  collisions, with only `std` as a dependency. The same module is played headless by
  [`tests/run/breakout_sim`](../../tests/run/breakout_sim), so the rules are tested without
  a window. [`src/main.dz`](src/main.dz) only draws and reads input.
- **raylib used as it is.** `rl::DrawRectangleRec`, `rl::IsKeyDown(rl::KEY_LEFT)` and
  `rl::RAYWHITE` come straight from `raylib.h`. raylib's seven `.c` files are compiled by
  `dz` from the recipe in [`dazzle.toml`](dazzle.toml), with no CMake.
- **Errors as values.** Saving the best score can fail, and the failure is handled where it
  happens, with one `or` clause:

  ```dz
  let saved = save_best(&best_path, g.score) or { e ->
      eprintln!("could not save the best score: {}", e.message)
      false
  }
  ```

- **Tagged unions and exhaustive `switch`**: the game's `Phase` drives what is drawn, and
  forgetting a case is a compile error.
- **A fixed physics step.** A long frame is taken in small steps, so the ball cannot pass
  through a brick when the window is dragged.

## Files

| | |
|---|---|
| [`dazzle.toml`](dazzle.toml) | the module, its dependencies, and raylib's recipe for each platform |
| [`src/main.dz`](src/main.dz) | the window, input and drawing |
| [`game/src/game.dz`](game/src/game.dz) | the rules |
| [`game/src/best.dz`](game/src/best.dz) | loading and saving the best score with `Path` and `File` |
