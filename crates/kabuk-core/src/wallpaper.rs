//! Wallpapers, pure: the pictures of Kabuk's own still wallpapers (SPEC §2.8) as BMP
//! files.

/// The design's night gradient (`.wall.gradient`): `radial-gradient(120% 90% at 18% 0%,
/// color-mix(in srgb, accent 18%, desk), desk 62%)`, as `w` × `h` pixels in top-down rows
/// of blue, green, red. Colours are `(r, g, b)`.
pub fn night_gradient(w: u32, h: u32, accent: (u8, u8, u8), desk: (u8, u8, u8)) -> Vec<u8> {
    const AT: (f64, f64) = (0.18, 0.0);
    const RADII: (f64, f64) = (1.2, 0.9);
    const MIX: f64 = 0.18;
    const STOP: f64 = 0.62;
    let mix = |a: u8, d: u8| f64::from(a) * MIX + f64::from(d) * (1.0 - MIX);
    let start = (mix(accent.0, desk.0), mix(accent.1, desk.1), mix(accent.2, desk.2));
    let end = (f64::from(desk.0), f64::from(desk.1), f64::from(desk.2));
    let (fw, fh) = (f64::from(w.max(1)), f64::from(h.max(1)));
    let mut out = Vec::with_capacity((w * h * 3) as usize);
    for y in 0..h {
        for x in 0..w {
            let dx = (f64::from(x) + 0.5 - AT.0 * fw) / (RADII.0 * fw);
            let dy = (f64::from(y) + 0.5 - AT.1 * fh) / (RADII.1 * fh);
            let t = ((dx * dx + dy * dy).sqrt() / STOP).min(1.0);
            let c = |s: f64, e: f64| (s + (e - s) * t).round().clamp(0.0, 255.0) as u8;
            out.extend_from_slice(&[c(start.2, end.2), c(start.1, end.1), c(start.0, end.0)]);
        }
    }
    out
}

/// How far a still picture drifts (DECISIONS D47): it is made this many pixels larger on
/// each side and shown from these places in turn, a square of two pixels, too small and
/// too slow to be seen, so no pixel stays lit the same way.
pub const DRIFT_MARGIN: u32 = 2;
pub const DRIFT_PLACES: [(u32, u32); 4] = [(0, 0), (2, 0), (2, 2), (0, 2)];

/// The place a still picture is shown from `minutes` after 1970 with a step every
/// `interval` minutes.
pub fn drift_place(minutes: u64, interval: u32) -> usize {
    ((minutes / u64::from(interval.max(1))) % DRIFT_PLACES.len() as u64) as usize
}

/// The `w` × `h` part of a picture `(w + 2·DRIFT_MARGIN)` pixels wide (rows of blue,
/// green, red), from place `at`.
pub fn drift_frame(big: &[u8], w: u32, h: u32, at: usize) -> Vec<u8> {
    let big_w = (w + 2 * DRIFT_MARGIN) as usize;
    let (dx, dy) = DRIFT_PLACES[at % DRIFT_PLACES.len()];
    let mut out = Vec::with_capacity((w * h * 3) as usize);
    for y in 0..h as usize {
        let start = ((y + dy as usize) * big_w + dx as usize) * 3;
        out.extend_from_slice(&big[start..start + w as usize * 3]);
    }
    out
}

/// `w` × `h` pixels of blue, green, red (top-down rows) as a 24-bit BMP file, which
/// Windows takes as a wallpaper.
pub fn bmp(w: u32, h: u32, bgr: &[u8]) -> Vec<u8> {
    let row = (w * 3).div_ceil(4) * 4;
    let size = 54 + row * h;
    let mut b = Vec::with_capacity(size as usize);
    b.extend_from_slice(b"BM");
    b.extend_from_slice(&size.to_le_bytes());
    b.extend_from_slice(&[0; 4]);
    b.extend_from_slice(&54u32.to_le_bytes());
    b.extend_from_slice(&40u32.to_le_bytes());
    b.extend_from_slice(&w.to_le_bytes());
    // Negative height: top-down rows.
    b.extend_from_slice(&(-(h as i32)).to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&24u16.to_le_bytes());
    b.extend_from_slice(&[0; 24]);
    let pad = (row - w * 3) as usize;
    for y in 0..h as usize {
        let start = y * w as usize * 3;
        b.extend_from_slice(&bgr[start..start + w as usize * 3]);
        b.extend(std::iter::repeat_n(0, pad));
    }
    b
}

/// `bytes` in Base64 (RFC 4648, padded).
pub fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// A picture for a web page's `<img>`: `w` × `h` pixels of blue, green, red as a BMP
/// data URL (the settings page's preview of a created wallpaper).
pub fn bmp_data_url(w: u32, h: u32, bgr: &[u8]) -> String {
    format!("data:image/bmp;base64,{}", base64(&bmp(w, h, bgr)))
}

/// The folder Kabuk's wallpaper pictures are written to: a folder of its own in `temp`,
/// unless the state folder is in `temp` already (the integration tests' own folders).
/// Not the state folder: Windows refused every picture newly given as its wallpaper from
/// under `AppData` but its `Temp` with "file not found" (0x80070002), whatever its kind
/// (found on the user's Windows 11 build 26300: no still design of the gallery showed).
pub fn pictures_folder(state_folder: &std::path::Path, temp: &std::path::Path) -> std::path::PathBuf {
    if state_folder.starts_with(temp) {
        state_folder.to_path_buf()
    } else {
        temp.join(PICTURES_FOLDER)
    }
}

/// [`pictures_folder`]'s name in the temporary folder.
pub const PICTURES_FOLDER: &str = "kabuk-wallpapers";

/// The file names of Kabuk's still wallpapers in [`pictures_folder`]: every one starts
/// with [`FILE_PREFIX`] and is a BMP or a JPEG.
pub const FILE_PREFIX: &str = "wallpaper-";
pub const BLACK_FILE: &str = "wallpaper-black.bmp";
pub const GRADIENT_PREFIX: &str = "wallpaper-gradient-";
pub const DRIFT_PREFIX: &str = "wallpaper-drift-";
/// A still design of the gallery drawn at a monitor's size (DECISIONS D58).
pub const GALLERY_PREFIX: &str = "wallpaper-gallery-";
const FILE_EXTENSIONS: [&str; 2] = [".bmp", ".jpg"];

/// Whether the picture at `path` is one Kabuk made (by its file name, in any folder:
/// integration tests make theirs in temporary state folders). The drift's and the
/// gallery's are JPEGs (a drifting picture was once taken for the user's own).
pub fn is_kabuks(path: &str) -> bool {
    let name = path.rsplit(['\\', '/']).next().unwrap_or("").to_lowercase();
    name.starts_with(FILE_PREFIX) && FILE_EXTENSIONS.iter().any(|e| name.ends_with(e))
}

/// The file a still gallery design `key` (`wall-still/agate-slice`) is drawn into at
/// `w` × `h`; `version` names its source, so an updated design is drawn again.
pub fn gallery_file(key: &str, w: u32, h: u32, version: u64) -> String {
    let safe: String = key
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '-' })
        .collect();
    format!("{GALLERY_PREFIX}{safe}-{w}x{h}-{version:016x}.jpg")
}

/// The user's own wallpaper, to record before Kabuk sets its own: `current` unless it is
/// a picture of Kabuk's (left showing when Kabuk ended without giving the user's back),
/// then the newest of Windows' wallpaper `history` that is not Kabuk's. `None` is a plain
/// colour, which is what is left when the history holds nothing else.
pub fn users_own(current: Option<&str>, history: &[String]) -> Option<String> {
    let current = current.filter(|p| !p.is_empty())?;
    if !is_kabuks(current) {
        return Some(current.to_string());
    }
    history.iter().find(|p| !p.is_empty() && !is_kabuks(p)).cloned()
}

#[cfg(test)]
mod tests {
    //! Edge cases: the gradient's tinted start and plain end; a BMP's padded rows; Base64
    //! of every padding length; a data URL; Kabuk's own picture left showing.
    use super::*;

    #[test]
    fn pictures_go_where_windows_takes_them() {
        use std::path::Path;
        let temp = Path::new(r"C:\Users\u\AppData\Local\Temp");
        assert_eq!(
            pictures_folder(Path::new(r"C:\Users\u\AppData\Local\kabuk"), temp),
            temp.join(PICTURES_FOLDER)
        );
        let sandbox = temp.join(".tmpA1b2");
        assert_eq!(pictures_folder(&sandbox, temp), sandbox, "a test's own folder");
    }

    #[test]
    fn the_night_gradient_starts_tinted_at_the_top_left_and_ends_in_the_desk() {
        let accent = (0xfb, 0xc4, 0x19);
        let desk = (0, 0, 0);
        let (w, h) = (200, 100);
        let px = night_gradient(w, h, accent, desk);
        assert_eq!(px.len(), (w * h * 3) as usize);
        let at = |x: u32, y: u32| {
            let i = ((y * w + x) * 3) as usize;
            (px[i + 2], px[i + 1], px[i])
        };
        // Near the centre (18 %, top): 18 % of the accent over the desk.
        let c = at(36, 0);
        assert!(
            (i32::from(c.0) - 45).abs() <= 2 && (i32::from(c.1) - 35).abs() <= 2,
            "{c:?}"
        );
        // Far away (bottom right): the desk colour.
        assert_eq!(at(199, 99), desk);
        // Brightness only falls with the distance.
        assert!(at(36, 50).0 < c.0 && at(150, 0).0 < c.0);
    }

    #[test]
    fn a_bmp_holds_its_size_and_padded_rows() {
        let px = vec![1, 2, 3, 4, 5, 6, 7, 8, 9];
        let b = bmp(3, 1, &px);
        assert_eq!(&b[..2], b"BM");
        assert_eq!(b.len(), 54 + 12, "a 9-byte row padded to 12");
        assert_eq!(u32::from_le_bytes(b[18..22].try_into().unwrap()), 3);
        assert_eq!(i32::from_le_bytes(b[22..26].try_into().unwrap()), -1);
        assert_eq!(&b[54..63], &px[..]);
    }

    #[test]
    fn base64_pads_and_data_urls_carry_a_bmp() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64(&[0xff, 0xfe]), "//4=");
        let url = bmp_data_url(1, 1, &[0, 0, 0]);
        assert!(url.starts_with("data:image/bmp;base64,Qk0"), "{url}");
    }

    /// D47: the places come in turn, one each interval, round and round; a frame is the
    /// picture's part from its place.
    #[test]
    fn a_still_picture_drifts_round_a_small_square() {
        assert_eq!(drift_place(0, 10), 0);
        assert_eq!(drift_place(9, 10), 0);
        assert_eq!(drift_place(10, 10), 1);
        assert_eq!(drift_place(35, 10), 3);
        assert_eq!(drift_place(40, 10), 0);
        assert_eq!(drift_place(7, 0), 3, "no interval counts as one minute");
        // A 2×1 picture made 6×5: pixel values are their own x + 10·y.
        let (w, h) = (2u32, 1u32);
        let (bw, bh) = (w + 2 * DRIFT_MARGIN, h + 2 * DRIFT_MARGIN);
        let big: Vec<u8> = (0..bh)
            .flat_map(|y| (0..bw).flat_map(move |x| [(x + 10 * y) as u8; 3]))
            .collect();
        assert_eq!(drift_frame(&big, w, h, 0), vec![0, 0, 0, 1, 1, 1]);
        assert_eq!(drift_frame(&big, w, h, 1), vec![2, 2, 2, 3, 3, 3]);
        assert_eq!(drift_frame(&big, w, h, 2), vec![22, 22, 22, 23, 23, 23]);
        assert_eq!(drift_frame(&big, w, h, 3), vec![20, 20, 20, 21, 21, 21]);
    }

    #[test]
    fn kabuks_own_pictures_are_known_by_their_file_names() {
        assert!(is_kabuks(r"C:\Users\u\AppData\Local\kabuk\wallpaper-black.bmp"));
        assert!(is_kabuks(
            r"C:\Users\u\AppData\Local\Temp\.tmpgnfzQh\WALLPAPER-BLACK.BMP"
        ));
        assert!(is_kabuks(r"C:\x\wallpaper-gradient-2560x1440-ff7eb6-000000.bmp"));
        assert!(is_kabuks(r"C:\x\wallpaper-drift-1a2b-2560x1440-3.bmp"));
        // The drift's and the gallery's JPEGs too.
        assert!(is_kabuks(r"C:\x\wallpaper-drift-1a2b-2560x1440-3.jpg"));
        let g = gallery_file("wall-still/agate-slice", 2560, 1440, 0xabc);
        assert_eq!(
            g,
            "wallpaper-gallery-wall-still-agate-slice-2560x1440-0000000000000abc.jpg"
        );
        assert!(is_kabuks(&format!(r"C:\x\{g}")));
        assert!(!is_kabuks(r"C:\Users\u\Pictures\wallpaper-beach.png"));
        assert!(!is_kabuks(r"C:\Users\u\Pictures\Frame 79.png"));
        assert!(!is_kabuks(r"C:\Users\u\Pictures\wallpaper-black.png"));
        assert!(!is_kabuks(r"C:\Users\u\Pictures\my-wallpaper-black.bmp"));
        assert!(!is_kabuks(""));
    }

    #[test]
    fn a_picture_kabuk_left_behind_is_not_taken_for_the_users() {
        let history = [
            r"C:\Users\u\AppData\Local\kabuk\wallpaper-black.bmp".to_string(),
            r"C:\Users\u\AppData\Local\Temp\.tmpgnfzQh\wallpaper-black.bmp".to_string(),
            r"C:\Users\u\Pictures\Frame 79.png".to_string(),
            r"C:\Users\u\Pictures\older.jpg".to_string(),
        ];
        let own = Some(r"C:\Users\u\Pictures\Frame 79.png".to_string());
        // The user's own picture is kept as it is.
        assert_eq!(
            users_own(Some(r"C:\Users\u\Pictures\mine.jpg"), &history),
            Some(r"C:\Users\u\Pictures\mine.jpg".into())
        );
        // Kabuk's black, left showing after a crash: the newest of the user's own instead.
        assert_eq!(users_own(Some(&history[0]), &history), own);
        // No picture (a plain colour) stays none.
        assert_eq!(users_own(None, &history), None);
        // Only Kabuk's in the history: a plain colour rather than Kabuk's picture.
        assert_eq!(users_own(Some(&history[0]), &history[..2]), None);
        // Empty history entries are skipped.
        assert_eq!(
            users_own(Some(&history[0]), &[String::new(), history[3].clone()]),
            Some(history[3].clone())
        );
    }
}
