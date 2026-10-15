//! The wallpaper format (SPEC §9): `wall.toml` (name, author, version, adjustable values
//! with their ranges, the tokens used) and `wall.hlsl`, a single function
//! `float3 wall(float2 px)` giving a pixel's colour. Kabuk wraps it in its own pixel
//! shader ([`shader_source`]): the inputs are a constant buffer (time, resolution, the
//! look's colours, the values) and nothing else; the output is clamped, made opaque and
//! scaled by the user's peak brightness.

use serde::{Deserialize, Serialize};

/// The look's colours a wallpaper may use, as HLSL names (`tok_<name>`, `float3`) and the
/// theme token each stands for. Their order is the constant buffer's.
pub const TOKENS: [(&str, &str); 8] = [
    ("accent", "accent-s"),
    ("accent2", "accent2"),
    ("desk", "desk"),
    ("fg", "fg-s"),
    ("dim", "dim"),
    ("surf", "surf"),
    ("trace", "trace"),
    ("trace_strong", "trace-strong"),
];

/// At most this many adjustable values (two `float4`s in the constant buffer).
pub const MAX_VALUES: usize = 8;
/// The longest name shown in the wallpaper list.
pub const MAX_NAME: usize = 40;
/// The longest `wall.hlsl` taken, in bytes.
pub const MAX_SOURCE: usize = 32 * 1024;

/// What a source may not contain: Kabuk gives the inputs, so no resources, no includes,
/// no entry points or registers of its own.
const FORBIDDEN: [&str; 14] = [
    "#include",
    "#pragma",
    "Texture",
    "SamplerState",
    "sampler",
    "RWTexture",
    "Buffer",
    "cbuffer",
    "tbuffer",
    "register",
    "SV_",
    "groupshared",
    "discard",
    "clip(",
];

/// An adjustable value (`[[values]]`): a slider in the wallpaper's settings, read in the
/// shader by its name.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Value {
    pub name: String,
    pub min: f32,
    pub max: f32,
    pub default: f32,
}

/// `wall.toml`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub name: String,
    #[serde(default)]
    pub author: String,
    #[serde(default = "first_version")]
    pub version: u32,
    #[serde(default)]
    pub values: Vec<Value>,
    #[serde(default)]
    pub tokens: Vec<String>,
}

fn first_version() -> u32 {
    1
}

/// Why a wallpaper's files were refused (said to the model so it can fix them).
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum FormatError {
    #[error("wall.toml is not valid TOML for the format: {0}")]
    Toml(String),
    #[error("the name must be 1-{MAX_NAME} characters")]
    Name,
    #[error("at most {MAX_VALUES} values are allowed")]
    TooManyValues,
    #[error("value `{0}` must be an HLSL identifier (letters, digits, _; not starting with a digit), used once, and not a token's or Kabuk's own name")]
    ValueName(String),
    #[error("value `{0}` needs min < max and min <= default <= max")]
    ValueRange(String),
    #[error("token `{0}` is not one of: {list}", list = token_list())]
    Token(String),
    #[error("wall.hlsl is empty or longer than {MAX_SOURCE} bytes")]
    SourceSize,
    #[error("wall.hlsl may not use `{0}`: Kabuk gives the inputs and writes the entry point")]
    Forbidden(String),
    #[error("wall.hlsl must define `float3 wall(float2 px)`")]
    NoEntry,
}

fn token_list() -> String {
    TOKENS.iter().map(|(n, _)| *n).collect::<Vec<_>>().join(", ")
}

/// Names a value may not take: Kabuk's inputs and the tokens.
/// HLSL's keywords, types and intrinsic functions: a value is a `#define`, so one named
/// `dot` broke every `dot()` (found making the gallery).
const HLSL_NAMES: [&str; 92] = [
    "abs",
    "acos",
    "all",
    "any",
    "asin",
    "atan",
    "atan2",
    "bool",
    "break",
    "ceil",
    "clamp",
    "const",
    "continue",
    "cos",
    "cosh",
    "cross",
    "ddx",
    "ddy",
    "degrees",
    "determinant",
    "distance",
    "do",
    "dot",
    "double",
    "else",
    "exp",
    "exp2",
    "false",
    "float",
    "float2",
    "float3",
    "float4",
    "float2x2",
    "float3x3",
    "float4x4",
    "floor",
    "fmod",
    "for",
    "frac",
    "frexp",
    "fwidth",
    "half",
    "if",
    "in",
    "inout",
    "int",
    "int2",
    "int3",
    "int4",
    "isinf",
    "isnan",
    "ldexp",
    "length",
    "lerp",
    "line",
    "lit",
    "log",
    "log10",
    "log2",
    "mad",
    "matrix",
    "max",
    "min",
    "modf",
    "mul",
    "noise",
    "normalize",
    "out",
    "point",
    "pow",
    "radians",
    "rcp",
    "reflect",
    "refract",
    "return",
    "round",
    "rsqrt",
    "saturate",
    "sign",
    "sin",
    "sincos",
    "sinh",
    "smoothstep",
    "sqrt",
    "static",
    "step",
    "struct",
    "tan",
    "tanh",
    "true",
    "uint",
    "while",
];

fn reserved(name: &str) -> bool {
    matches!(name, "time" | "resolution" | "brightness" | "values" | "wall" | "main")
        || name.starts_with("tok_")
        || TOKENS.iter().any(|(n, _)| *n == name)
        || HLSL_NAMES.contains(&name)
}

fn identifier(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

impl Manifest {
    /// Reads and checks `wall.toml`.
    pub fn parse(text: &str) -> Result<Manifest, FormatError> {
        let m: Manifest = toml::from_str(text).map_err(|e| FormatError::Toml(e.message().to_string()))?;
        m.check()?;
        Ok(m)
    }

    pub fn check(&self) -> Result<(), FormatError> {
        let name = self.name.trim();
        if name.is_empty() || name.chars().count() > MAX_NAME {
            return Err(FormatError::Name);
        }
        if self.values.len() > MAX_VALUES {
            return Err(FormatError::TooManyValues);
        }
        for (i, v) in self.values.iter().enumerate() {
            if !identifier(&v.name) || reserved(&v.name) || self.values[..i].iter().any(|o| o.name == v.name) {
                return Err(FormatError::ValueName(v.name.clone()));
            }
            let finite = v.min.is_finite() && v.max.is_finite() && v.default.is_finite();
            if !finite || v.min >= v.max || v.default < v.min || v.default > v.max {
                return Err(FormatError::ValueRange(v.name.clone()));
            }
        }
        if let Some(t) = self.tokens.iter().find(|t| !TOKENS.iter().any(|(n, _)| n == t)) {
            return Err(FormatError::Token(t.clone()));
        }
        Ok(())
    }

    pub fn to_toml(&self) -> String {
        toml::to_string(self).unwrap_or_default()
    }

    /// The values at their defaults, in order.
    pub fn defaults(&self) -> Vec<f32> {
        self.values.iter().map(|v| v.default).collect()
    }
}

/// Checks `wall.hlsl` before it is compiled.
pub fn check_source(source: &str) -> Result<(), FormatError> {
    if source.trim().is_empty() || source.len() > MAX_SOURCE {
        return Err(FormatError::SourceSize);
    }
    if let Some(f) = FORBIDDEN.iter().find(|f| source.contains(**f)) {
        return Err(FormatError::Forbidden((*f).to_string()));
    }
    let squeezed: String = source.split_whitespace().collect::<Vec<_>>().join(" ");
    if !squeezed.contains("float3 wall(float2 px)") && !squeezed.contains("float3 wall( float2 px )") {
        return Err(FormatError::NoEntry);
    }
    Ok(())
}

/// The constant buffer's HLSL, and the names of the values as macros over it.
fn header(manifest: &Manifest) -> String {
    let mut h = String::from(
        "cbuffer Kabuk : register(b0)\n{\n    float time;\n    float brightness;\n    float2 resolution;\n",
    );
    for (name, _) in TOKENS {
        h.push_str(&format!("    float4 tok_{name}_4;\n"));
    }
    h.push_str(&format!("    float4 values[{}];\n}};\n", MAX_VALUES / 4));
    for (name, _) in TOKENS {
        h.push_str(&format!("#define tok_{name} (tok_{name}_4.rgb)\n"));
    }
    const LANES: [&str; 4] = ["x", "y", "z", "w"];
    for (i, v) in manifest.values.iter().enumerate() {
        h.push_str(&format!("#define {} (values[{}].{})\n", v.name, i / 4, LANES[i % 4]));
    }
    h.push_str("#line 1\n");
    h
}

/// The whole pixel shader Kabuk compiles: its constant buffer, the wallpaper's function,
/// and an entry point (`main`) that keeps the colour in range, opaque and under the
/// peak brightness.
pub fn shader_source(manifest: &Manifest, wall: &str) -> String {
    format!(
        "{}{wall}\n\nfloat4 main(float4 kabuk_pos : SV_Position) : SV_Target\n{{\n    return float4(saturate(wall(kabuk_pos.xy)) * brightness, 1.0);\n}}\n",
        header(manifest)
    )
}

/// A look colour by its theme token name, as sRGB `(r, g, b)` in 0-1.
pub type TokenColor<'a> = &'a dyn Fn(&str) -> Option<(f32, f32, f32)>;

/// The constant buffer's bytes: time, brightness (0-1), resolution, the tokens' colours
/// (sRGB 0-1, as `(r, g, b)`; missing ones black) and the values (missing ones at their
/// defaults). 16-byte aligned, as Direct3D wants.
pub fn constants(
    manifest: &Manifest,
    time: f32,
    brightness: f32,
    resolution: (f32, f32),
    tokens: TokenColor<'_>,
    values: &[f32],
) -> Vec<u8> {
    let mut f: Vec<f32> = vec![time, brightness, resolution.0, resolution.1];
    for (_, token) in TOKENS {
        let (r, g, b) = tokens(token).unwrap_or((0.0, 0.0, 0.0));
        f.extend_from_slice(&[r, g, b, 1.0]);
    }
    let mut v = [0.0f32; MAX_VALUES];
    for (i, slot) in v.iter_mut().enumerate().take(manifest.values.len()) {
        *slot = values.get(i).copied().unwrap_or(manifest.values[i].default);
    }
    f.extend_from_slice(&v);
    f.iter().flat_map(|x| x.to_le_bytes()).collect()
}

#[cfg(test)]
mod tests {
    //! Edge cases: a full manifest and a minimal one; each refusal of the manifest (name
    //! length, too many values, a bad or reserved or repeated name, a bad range, an
    //! unknown token, an unknown key); the source's size, forbidden words and entry; the
    //! shader wraps the function; the constant buffer's layout.
    use super::*;

    const FULL: &str = r#"
name = "Mor dalgalar"
author = "Claude"
version = 2
tokens = ["accent", "desk"]

[[values]]
name = "speed"
min = 0.1
max = 2.0
default = 0.5
"#;

    #[test]
    fn a_manifest_reads_and_writes_back() {
        let m = Manifest::parse(FULL).unwrap();
        assert_eq!((m.name.as_str(), m.version, m.values.len()), ("Mor dalgalar", 2, 1));
        assert_eq!(Manifest::parse(&m.to_toml()).unwrap(), m);
        let small = Manifest::parse("name = \"x\"").unwrap();
        assert_eq!((small.version, small.values.len(), small.author.as_str()), (1, 0, ""));
    }

    #[test]
    fn a_bad_manifest_is_refused_for_its_reason() {
        let with = |extra: &str| Manifest::parse(&format!("name = \"x\"\n{extra}"));
        assert_eq!(Manifest::parse("name = \"  \""), Err(FormatError::Name));
        assert_eq!(
            Manifest::parse(&format!("name = \"{}\"", "a".repeat(41))),
            Err(FormatError::Name)
        );
        let many: String = (0..9)
            .map(|i| format!("[[values]]\nname = \"v{i}\"\nmin = 0\nmax = 1\ndefault = 0\n"))
            .collect();
        assert_eq!(with(&many), Err(FormatError::TooManyValues));
        // HLSL's own names too: a value is a `#define`, so `dot` broke `dot()` (found making
        // the gallery).
        for bad in [
            "9x", "time", "tok_desk", "accent", "a-b", "dot", "line", "sin", "float3", "return", "lerp",
        ] {
            let v = format!("[[values]]\nname = \"{bad}\"\nmin = 0\nmax = 1\ndefault = 0\n");
            assert_eq!(with(&v), Err(FormatError::ValueName(bad.into())), "{bad}");
        }
        let twice = "[[values]]\nname = \"a\"\nmin = 0\nmax = 1\ndefault = 0\n".repeat(2);
        assert_eq!(with(&twice), Err(FormatError::ValueName("a".into())));
        let range = "[[values]]\nname = \"a\"\nmin = 1\nmax = 1\ndefault = 1\n";
        assert_eq!(with(range), Err(FormatError::ValueRange("a".into())));
        let outside = "[[values]]\nname = \"a\"\nmin = 0\nmax = 1\ndefault = 2\n";
        assert_eq!(with(outside), Err(FormatError::ValueRange("a".into())));
        assert_eq!(with("tokens = [\"neon\"]"), Err(FormatError::Token("neon".into())));
        assert!(matches!(with("colour = 1"), Err(FormatError::Toml(_))));
    }

    #[test]
    fn a_source_is_checked_before_compiling() {
        let ok = "float3 wall(float2 px)\n{\n    return tok_desk;\n}\n";
        assert_eq!(check_source(ok), Ok(()));
        assert_eq!(check_source("  "), Err(FormatError::SourceSize));
        assert_eq!(check_source(&"/".repeat(MAX_SOURCE + 1)), Err(FormatError::SourceSize));
        let tex = format!("Texture2D t;\n{ok}");
        assert_eq!(check_source(&tex), Err(FormatError::Forbidden("Texture".into())));
        let inc = format!("#include \"x\"\n{ok}");
        assert_eq!(check_source(&inc), Err(FormatError::Forbidden("#include".into())));
        assert_eq!(
            check_source("float4 main(float2 px) { return 0; }"),
            Err(FormatError::NoEntry)
        );
        assert_eq!(check_source("float3  wall(\n float2  px )\n{ return 0; }"), Ok(()));
    }

    #[test]
    fn the_shader_wraps_the_function_with_kabuks_inputs() {
        let m = Manifest::parse(FULL).unwrap();
        let s = shader_source(&m, "float3 wall(float2 px) { return tok_accent * speed; }");
        assert!(s.contains("cbuffer Kabuk : register(b0)"));
        assert!(s.contains("#define speed (values[0].x)"));
        assert!(s.contains("#define tok_accent (tok_accent_4.rgb)"));
        assert!(s.contains("saturate(wall(kabuk_pos.xy)) * brightness, 1.0)"));
        // The wallpaper's lines are numbered from 1 in the compiler's messages.
        let wall_at = s.find("float3 wall").unwrap();
        assert!(s[..wall_at].ends_with("#line 1\n"));
    }

    #[test]
    fn the_constants_follow_the_buffers_layout() {
        let m = Manifest::parse(FULL).unwrap();
        let tokens = |t: &str| (t == "desk").then_some((0.1, 0.2, 0.3));
        let b = constants(&m, 2.5, 0.6, (3840.0, 2160.0), &tokens, &[]);
        let f: Vec<f32> = b.chunks(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect();
        assert_eq!(b.len() % 16, 0);
        assert_eq!(f.len(), 4 + TOKENS.len() * 4 + MAX_VALUES);
        assert_eq!(&f[..4], &[2.5, 0.6, 3840.0, 2160.0]);
        // desk is the third token.
        assert_eq!(&f[4 + 2 * 4..4 + 3 * 4], &[0.1, 0.2, 0.3, 1.0]);
        assert_eq!(&f[4..8], &[0.0, 0.0, 0.0, 1.0], "a missing token is black");
        // speed at its default, the rest zero; a given value wins.
        let values = &f[4 + TOKENS.len() * 4..];
        assert_eq!(values[0], 0.5);
        assert!(values[1..].iter().all(|v| *v == 0.0));
        let given = constants(&m, 0.0, 1.0, (1.0, 1.0), &tokens, &[1.5]);
        let g: Vec<f32> = given
            .chunks(4)
            .map(|c| f32::from_le_bytes(c.try_into().unwrap()))
            .collect();
        assert_eq!(g[4 + TOKENS.len() * 4], 1.5);
    }
}
