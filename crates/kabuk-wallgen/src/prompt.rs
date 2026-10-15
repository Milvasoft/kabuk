//! What the model is told (ARCHITECTURE §11b): the format, Kabuk's inputs, the rules a
//! wallpaper is checked against and a sample; then the user's request; then, after a
//! refusal, the reason and a request to fix it.

use crate::format::{MAX_NAME, MAX_VALUES, TOKENS};

/// The longest frame allowed at 4K, in milliseconds (SPEC §9).
pub const MAX_FRAME_MS: f32 = 4.0;
/// The most instructions a compiled wallpaper may have, so that no frame can run for
/// long whatever the model wrote (with loops unrolled, every pixel's work is bounded).
pub const MAX_INSTRUCTIONS: u32 = 4096;

/// A sample wallpaper that passes every check (tested in Kabuk's GPU tests).
pub const SAMPLE_TOML: &str = r#"name = "Slow ribbons"
author = "Kabuk"
version = 1
tokens = ["accent"]

[[values]]
name = "speed"
min = 0.1
max = 2.0
default = 0.6
"#;

pub const SAMPLE_HLSL: &str = r#"float3 wall(float2 px)
{
    float2 uv = px / resolution.y;
    float t = time * speed;
    float3 c = 0;
    [unroll] for (int i = 0; i < 3; i++)
    {
        float y = 0.5 + 0.18 * sin(uv.x * (1.3 + i * 0.4) + t * (0.2 + i * 0.07) + i * 2.1);
        float d = abs(uv.y - y);
        c += tok_accent * (0.002 / (d + 0.002)) * 0.3;
    }
    return c;
}
"#;

/// The system prompt.
pub fn system() -> String {
    let tokens = TOKENS
        .iter()
        .map(|(n, _)| format!("tok_{n}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        r#"You write animated desktop wallpapers for Kabuk, a Windows desktop shell made for OLED screens. A wallpaper is two files.

wall.toml:
- name: a short name in the language of the request, at most {MAX_NAME} characters
- author: who made it (you may write your model's name)
- version: 1
- tokens: the look colours the shader uses, from: {token_names}
- [[values]]: at most {MAX_VALUES} adjustable values, each with name (an HLSL identifier that is not one of HLSL's own words or functions, such as dot, line or sin), min, max and default

wall.hlsl: HLSL (shader model 5.0) defining exactly this function and any helpers it needs:
    float3 wall(float2 px)
px is the pixel's position in pixels from the top-left corner. Kabuk writes the rest of the pixel shader. The inputs are:
- float time: seconds since the wallpaper started
- float2 resolution: the screen's size in pixels (any aspect ratio, up to 3840x2160 and beyond)
- float3 {tokens}: the user's look colours (sRGB, 0-1); use them so the wallpaper follows the user's theme
- each value from wall.toml, by its name, as a float
Return an sRGB colour (0-1). Kabuk clamps it and scales it by the user's brightness setting. There is nothing else: no textures, samplers, buffers, registers, #include, #pragma, discard or clip.

The wallpaper is checked and refused unless:
- the background is pure black: most of the screen must be exactly black (0, 0, 0)
- the average brightness over a minute is at most 8 %
- nothing bright stays in place: every bright part moves or fades away within a minute (OLED burn-in)
- a frame takes at most {MAX_FRAME_MS} ms at 3840x2160 on the GPU
- every loop has constant bounds and can be unrolled (write [unroll]); no [loop], no [branch], no while loops, no recursion; at most {MAX_INSTRUCTIONS} instructions once compiled
Keep motion slow and calm; it sits behind the user's windows all day.

A sample that passes:
```toml
{SAMPLE_TOML}```
```hlsl
{SAMPLE_HLSL}```

Reply with exactly two fenced code blocks, ```toml for wall.toml first and then ```hlsl for wall.hlsl, and no other code blocks."#,
        token_names = TOKENS.iter().map(|(n, _)| *n).collect::<Vec<_>>().join(", "),
    )
}

/// The user's request, as the first message.
pub fn request(prompt: &str) -> String {
    format!("Make a wallpaper for this request: {}", prompt.trim())
}

/// After a refusal: the reason and what to send back.
pub fn fix(reason: &str) -> String {
    format!("Kabuk refused the wallpaper: {reason}\nFix it and reply again with both files, in the same form.")
}

#[cfg(test)]
mod tests {
    //! Edge cases: the system prompt names every input, rule and the reply's form; the
    //! sample in it is a valid wallpaper by the format's rules.
    use super::*;
    use crate::format::{check_source, Manifest};

    #[test]
    fn the_system_prompt_states_the_format_and_the_rules() {
        let s = system();
        for needed in [
            "float3 wall(float2 px)",
            "float time",
            "float2 resolution",
            "tok_accent",
            "tok_trace_strong",
            "8 %",
            "4 ms",
            "[unroll]",
            "4096",
            "```toml",
            "```hlsl",
        ] {
            assert!(s.contains(needed), "missing {needed}");
        }
    }

    #[test]
    fn the_sample_follows_the_format() {
        assert!(Manifest::parse(SAMPLE_TOML).is_ok());
        assert_eq!(check_source(SAMPLE_HLSL), Ok(()));
        assert!(request("  mor dalgalar ").ends_with(": mor dalgalar"));
        assert!(fix("too bright").contains("too bright"));
    }
}
