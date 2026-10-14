//! Windows' accent colour as the system theme's colour layer writes it (SPEC §2.10,
//! DECISIONS D42): the seven shades Windows keeps of an accent (`AccentPalette`: three
//! lighter, the accent, three darker), and the colour words its registry values hold.
//! Pure.

use crate::Color;

/// How far each lighter shade goes toward white, and each darker one toward black, as
/// Windows' own palettes do (read from the user's machine: a grey accent's shades).
const LIGHTER: [f64; 3] = [0.82, 0.5, 0.16];
const DARKER: [f64; 3] = [0.15, 0.49, 0.81];

fn mix(c: Color, toward: u8, t: f64) -> Color {
    let ch = |v: u8| {
        (f64::from(v) + (f64::from(toward) - f64::from(v)) * t)
            .round()
            .clamp(0.0, 255.0) as u8
    };
    Color {
        r: ch(c.r),
        g: ch(c.g),
        b: ch(c.b),
        a: 1.0,
    }
}

/// The seven shades of `accent`, lightest first; the accent itself is the fourth.
pub fn shades(accent: Color) -> [Color; 7] {
    let base = Color { a: 1.0, ..accent };
    [
        mix(base, 0xff, LIGHTER[0]),
        mix(base, 0xff, LIGHTER[1]),
        mix(base, 0xff, LIGHTER[2]),
        base,
        mix(base, 0x00, DARKER[0]),
        mix(base, 0x00, DARKER[1]),
        mix(base, 0x00, DARKER[2]),
    ]
}

/// `AccentPalette`: the seven shades as RGBA bytes (alpha 0), then `last`, Windows' own
/// eighth colour, kept as it was.
pub fn palette_bytes(accent: Color, last: [u8; 4]) -> Vec<u8> {
    let mut out = shade_bytes(accent);
    out.extend_from_slice(&last);
    out
}

/// Which shade Start takes (a darker one, as Windows' own settings write it).
pub const START_SHADE: usize = 4;

/// The seven shades as `AccentPalette`'s first 28 bytes (Windows' eighth colour follows).
pub fn shade_bytes(accent: Color) -> Vec<u8> {
    shades(accent).iter().flat_map(|c| [c.r, c.g, c.b, 0]).collect()
}

/// A colour as Windows' `…ColorMenu` and `AccentColor` words hold it: 0xAABBGGRR, opaque.
pub fn abgr(c: Color) -> u32 {
    0xff00_0000 | (u32::from(c.b) << 16) | (u32::from(c.g) << 8) | u32::from(c.r)
}

#[cfg(test)]
mod tests {
    //! Edge cases: Windows' own palette of a grey accent is matched within a step; the
    //! shades go lighter then darker; black and white stay in range; the words' byte
    //! orders, against the values read from the user's machine.
    use super::*;

    fn rgb(r: u8, g: u8, b: u8) -> Color {
        Color { r, g, b, a: 1.0 }
    }

    #[test]
    fn the_shades_are_windows_own_within_a_step() {
        // Read from the user's machine: `AccentPalette` of the accent #4c4a48.
        let windows = [
            rgb(0xdf, 0xde, 0xdc),
            rgb(0xa6, 0xa5, 0xa1),
            rgb(0x68, 0x65, 0x62),
            rgb(0x4c, 0x4a, 0x48),
            rgb(0x41, 0x3f, 0x3d),
            rgb(0x27, 0x25, 0x24),
            rgb(0x10, 0x0d, 0x0d),
        ];
        for (ours, theirs) in shades(rgb(0x4c, 0x4a, 0x48)).iter().zip(windows) {
            for (a, b) in [(ours.r, theirs.r), (ours.g, theirs.g), (ours.b, theirs.b)] {
                assert!(a.abs_diff(b) <= 4, "{ours:?} vs {theirs:?}");
            }
        }
    }

    #[test]
    fn shades_go_from_light_to_dark_and_stay_in_range() {
        for accent in [
            rgb(0x8f, 0xd3, 0xff),
            rgb(0, 0, 0),
            rgb(0xff, 0xff, 0xff),
            rgb(0xfb, 0xc4, 0x19),
        ] {
            let s = shades(accent);
            let light = |c: &Color| u32::from(c.r) + u32::from(c.g) + u32::from(c.b);
            assert!(s.windows(2).all(|p| light(&p[0]) >= light(&p[1])), "{accent:?}");
            assert_eq!(s[3], accent);
        }
        let bytes = palette_bytes(rgb(1, 2, 3), [0x10, 0x7c, 0x10, 0]);
        assert_eq!(bytes.len(), 32);
        assert_eq!(&bytes[12..16], &[1, 2, 3, 0]);
        assert_eq!(&bytes[28..], &[0x10, 0x7c, 0x10, 0]);
    }

    #[test]
    fn the_words_are_in_windows_byte_order() {
        // As read from the user's machine: AccentColorMenu 0xff484a4c for #4c4a48, and the
        // DWM's AccentColor 0xff0c00d0 for #d0000c.
        assert_eq!(abgr(rgb(0x4c, 0x4a, 0x48)), 0xff48_4a4c);
        assert_eq!(abgr(rgb(0xd0, 0x00, 0x0c)), 0xff0c_00d0);
    }
}
