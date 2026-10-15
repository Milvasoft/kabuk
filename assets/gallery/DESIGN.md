# Kabuk gallery: how a design is made

Every wallpaper and screen saver in Kabuk's gallery (DECISIONS D58) is one folder:

    assets/gallery/<category>/<id>/wall.toml
    assets/gallery/<category>/<id>/wall.hlsl

`<id>` is lowercase ASCII with hyphens (`ink-tide`). Categories:

| Category      | What it is                                   | Drawn                                   |
| ------------- | -------------------------------------------- | --------------------------------------- |
| `wall-live`   | an animated desktop wallpaper                | every frame, behind the icons, all day  |
| `wall-still`  | a still desktop wallpaper                    | once, at `time = 0`, saved as a picture |
| `saver-live`  | an animated screen saver                     | every frame, full screen, OLED-safe     |
| `saver-still` | a still screen saver (a picture that drifts) | once, at `time = 0`; Kabuk drifts it    |

## The format

`wall.toml`:

```toml
name = "Ink tide"          # English, at most 40 characters
author = "Kabuk"
version = 1
tokens = ["accent"]        # look colours used: accent, accent2, desk, fg, dim, surf, trace, trace_strong

[[values]]                 # at most 8 sliders, each an HLSL identifier
name = "speed"
min = 0.1
max = 2.0
default = 0.6
```

`wall.hlsl` (shader model 5.0) defines `float3 wall(float2 px)` and any helpers:

- `px`: the pixel, in pixels from the top-left corner
- `float time`: seconds since the start
- `float2 resolution`: the screen in pixels; any aspect ratio (16:9, 16:10, 21:9, 32:9), up to 3840×2160 and more
- `float3 tok_<name>`: the user's look colours (sRGB 0-1) for each token listed in `wall.toml`
- each value of `wall.toml`, by its name, as a `float`

It returns an sRGB colour (0-1); Kabuk clamps it and scales it by the user's brightness.

Not allowed (the source is refused if it contains these, even inside a longer word): `#include`, `#pragma`, `Texture`, `SamplerState`, `sampler`, `RWTexture`, `Buffer`, `cbuffer`, `tbuffer`, `register`, `SV_`, `groupshared`, `discard`, `clip(`. Every loop has constant bounds and is written `[unroll]`; no `[loop]`, `[branch]`, `while` or recursion. At most 4096 instructions once compiled.

## Rules per category

- `wall-live`: it runs all day behind the user's windows, so it is calm (slow motion, no flashing), never busy where desktop icons sit (the left edge), and cheap: aim for **≤ 1 ms** per 4K frame (4 ms is the hard limit). Colour and brightness are free; it should look good at the user's default brightness of 60 %.
- `wall-still`: one picture, so it may cost more (keep it under 40 ms at 4K) and must look finished at `time = 0`. Desktop icons sit on the left; windows cover most of it; it should please for months.
- `saver-live`: it runs on OLED screens when nobody looks: pass `--oled` (most of the screen pure black, average brightness ≤ 8 % over a minute, nothing bright stays in place). ≤ 2 ms per 4K frame (4 ms hard).
- `saver-still`: one picture shown while nobody looks, which Kabuk slowly drifts and breathes; it must be OLED-friendly as a picture: mostly pure black (≥ 70 % of the pixels), average brightness ≤ 8 %. Pass `--oled` except for the "still and bright" fault, which the drift takes care of.

## Checking and looking

    E:\kabuk-target\release\examples\gallery_check.exe --dir assets\gallery\<category>\<id> [--oled] [--out <png>]

prints the instruction count, the slowest 4K frame, the brightness at 0/15/30/45 s and, with `--oled`, the OLED measures; it writes a 2×2 sheet of those moments (1920×1080) to `<dir>\sheet.png` (or `--out`). Look at it. A full-size frame:

    E:\kabuk-target\release\examples\wall_shot.exe <out.png> --dir <folder> --seconds 0 --size 3840x2160

`--look` picks the look whose colours the tokens take, `a` to `g` (seven looks); try a few when a design uses tokens.

Delete `sheet.png` and any other picture from the design folders when done; only `wall.toml` and `wall.hlsl` stay.
