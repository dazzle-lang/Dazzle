# Synthesizer

A tiny synthesizer: an arpeggio over four chords and a bass line, generated sample by sample
and played through [miniaudio](https://miniaud.io), a C audio library compiled from source by
`dz` and called directly from Dazzle. It plays for about seven seconds.

```bash
dz run examples/miniaudio
```

```text
Am F C G   (6.85714 s at 48000 Hz)
```

## What it shows

- **A real-time audio callback written in Dazzle.** miniaudio asks for sound from its own
  audio thread, and the function it calls is a plain Dazzle function:

  ```dz
  fn fill(device: ^mut ma::ma_device, output: ^mut void, _input: ^void, frames: u32) {
      let synth = synth_of(device) or return     // the user data, checked once
      let out = unsafe {
          output as ^mut f32
      }
      for i in 0..frames as usize {
          let s = synth.next()
          unsafe {
              out[2 * i] = s
              out[2 * i + 1] = s
          }
      }
  }
  ```

  Every access through a raw pointer is inside `unsafe`, so the few lines that trust C are
  easy to find. The rest, the oscillators, envelopes and the tune, is ordinary safe code.
- **C's nested structs, as they are.** miniaudio's device configuration keeps its playback
  settings in a struct with no name of its own, and they are set exactly as in C:
  `config.playback.format = ma::ma_format::ma_format_f32`.
- **No garbage collector and no hidden allocation**, which is what an audio callback needs:
  nothing in `fill` can pause for memory.

## Files

| | |
|---|---|
| [`dazzle.toml`](dazzle.toml) | the module, and miniaudio's recipe: one `.c` file and the libraries each platform links |
| [`src/main.dz`](src/main.dz) | the synthesizer, the callback, and `main` |
