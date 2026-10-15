//! Pictures of `HICON`s (tray icons of other programs), and PNG files (Kabuk's logo), as
//! premultiplied BGRA pixels, for Kabuk's renderer.

use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
    DIB_RGB_COLORS, HDC,
};
use windows::Win32::UI::WindowsAndMessaging::{DrawIconEx, DI_FLAGS, DI_MASK, DI_NORMAL, HICON};

const BYTES_PER_PIXEL: usize = 4;

/// Draws `hicon` at `size`×`size` with `flags` into a fresh 32-bit DIB and returns its
/// pixels, top row first.
fn draw(hicon: HICON, size: u32, flags: DI_FLAGS) -> Option<Vec<u8>> {
    let side = i32::try_from(size).ok()?;
    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: u32::try_from(std::mem::size_of::<BITMAPINFOHEADER>()).unwrap_or_default(),
            biWidth: side,
            // Negative: top-down rows.
            biHeight: -side,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let len = usize::try_from(size).ok()?.pow(2) * BYTES_PER_PIXEL;
    // SAFETY: (whole block) a memory DC and a DIB section owned here and released before
    // returning; `bits` points to `len` bytes while the section exists.
    unsafe {
        let dc: HDC = CreateCompatibleDC(None);
        if dc.is_invalid() {
            return None;
        }
        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        let Ok(bitmap) = CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0) else {
            let _ = DeleteDC(dc);
            return None;
        };
        let old = SelectObject(dc, bitmap.into());
        let drawn = DrawIconEx(dc, 0, 0, hicon, side, side, 0, None, flags).is_ok();
        let pixels = (drawn && !bits.is_null()).then(|| std::slice::from_raw_parts(bits.cast::<u8>(), len).to_vec());
        SelectObject(dc, old);
        let _ = DeleteObject(bitmap.into());
        let _ = DeleteDC(dc);
        pixels
    }
}

/// The icon the shell draws for `name` (a file path, a shortcut, or a
/// `shell:AppsFolder\<app id>` of a packaged app) at `size`×`size`, as premultiplied
/// BGRA, top row first. COM must be initialised on this thread.
pub fn shell_item_icon_pixels(name: &str, size: u32) -> Option<Vec<u8>> {
    use windows::Win32::Foundation::SIZE;
    use windows::Win32::Graphics::Gdi::GetDIBits;
    use windows::Win32::UI::Shell::{IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_ICONONLY};
    let side = i32::try_from(size).ok()?;
    let wide = windows::core::HSTRING::from(name);
    // SAFETY: a valid name; the item is released when dropped.
    let factory: IShellItemImageFactory = unsafe { SHCreateItemFromParsingName(&wide, None) }.ok()?;
    // SAFETY: the item's own image at the size asked; we own the bitmap and delete it below.
    let bitmap = unsafe { factory.GetImage(SIZE { cx: side, cy: side }, SIIGBF_ICONONLY) }.ok()?;
    let mut info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: u32::try_from(std::mem::size_of::<BITMAPINFOHEADER>()).unwrap_or_default(),
            biWidth: side,
            // Negative: top-down rows.
            biHeight: -side,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut px = vec![0u8; usize::try_from(size).ok()?.pow(2) * BYTES_PER_PIXEL];
    // SAFETY: (whole block) a memory DC released here; `px` holds the `size` rows of
    // 32-bit pixels `info` describes; the bitmap is ours to delete.
    let rows = unsafe {
        let dc = CreateCompatibleDC(None);
        let rows = GetDIBits(
            dc,
            bitmap,
            0,
            size,
            Some(px.as_mut_ptr().cast()),
            &mut info,
            DIB_RGB_COLORS,
        );
        let _ = DeleteDC(dc);
        let _ = DeleteObject(bitmap.into());
        rows
    };
    if rows != side {
        return None;
    }
    // A picture without any alpha is opaque.
    if px.chunks_exact(BYTES_PER_PIXEL).all(|p| p[3] == 0) {
        px.chunks_exact_mut(BYTES_PER_PIXEL).for_each(|p| p[3] = u8::MAX);
    }
    // The shell gives straight alpha; the renderer takes premultiplied (found by the
    // user: drawn as it came, the soft edges of the dock's and the app grid's icons
    // showed as white specks).
    premultiply(&mut px);
    Some(px)
}

/// Straight-alpha BGRA pixels made premultiplied: each colour times its alpha.
fn premultiply(px: &mut [u8]) {
    for p in px.chunks_exact_mut(BYTES_PER_PIXEL) {
        let a = u16::from(p[3]);
        for c in &mut p[..3] {
            *c = u8::try_from((u16::from(*c) * a + 127) / 255).unwrap_or(u8::MAX);
        }
    }
}

/// Icon `index` of a file (an `.ico`, a program's resources) at `size`×`size`, as
/// [`icon_pixels`] gives it (a jump list entry's own icon).
pub fn icon_in_file_pixels(path: &std::path::Path, index: i32, size: u32) -> Option<Vec<u8>> {
    use windows::Win32::UI::Shell::SHDefExtractIconW;
    use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, HICON};
    let mut large = HICON::default();
    let wide = windows::core::HSTRING::from(path.as_os_str());
    // SAFETY: a valid path and an out handle; the icon is destroyed below. The size goes
    // in the low word, as the call asks.
    let r = unsafe { SHDefExtractIconW(&wide, index, 0, Some(&mut large), None, size & 0xFFFF) };
    if r.is_err() || large.is_invalid() {
        return None;
    }
    let px = icon_pixels(large.0 as usize as u64, size);
    // SAFETY: the icon SHDefExtractIconW made for us.
    let _ = unsafe { DestroyIcon(large) };
    px
}

/// The icon the shell shows for a file (a program, a shortcut) at `size`×`size`, as
/// [`icon_pixels`] gives it. COM must be initialised on this thread.
pub fn file_icon_pixels(path: &std::path::Path, size: u32) -> Option<Vec<u8>> {
    use windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES;
    use windows::Win32::UI::Shell::{SHGetFileInfoW, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON};
    use windows::Win32::UI::WindowsAndMessaging::DestroyIcon;
    let mut info = SHFILEINFOW::default();
    let wide = windows::core::HSTRING::from(path.as_os_str());
    // SAFETY: a valid path and an out struct of the size passed; the icon is destroyed below.
    let found = unsafe {
        SHGetFileInfoW(
            &wide,
            FILE_FLAGS_AND_ATTRIBUTES(0),
            Some(&mut info),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_LARGEICON,
        )
    };
    if found == 0 || info.hIcon.is_invalid() {
        return None;
    }
    let px = icon_pixels(info.hIcon.0 as usize as u64, size);
    // SAFETY: the icon SHGetFileInfoW made for us.
    let _ = unsafe { DestroyIcon(info.hIcon) };
    px
}

/// The icon at `size`×`size` as premultiplied BGRA, or `None` if it cannot be drawn
/// (the program destroyed it). Icons without an alpha channel get it from their mask.
pub fn icon_pixels(hicon: u64, size: u32) -> Option<Vec<u8>> {
    let h = HICON(hicon as usize as *mut core::ffi::c_void);
    let mut px = draw(h, size, DI_NORMAL)?;
    let has_alpha = px.chunks_exact(BYTES_PER_PIXEL).any(|p| p[3] != 0);
    if !has_alpha {
        // Old-style icon: opaque where the AND mask is black.
        let mask = draw(h, size, DI_MASK)?;
        for (p, m) in px
            .chunks_exact_mut(BYTES_PER_PIXEL)
            .zip(mask.chunks_exact(BYTES_PER_PIXEL))
        {
            if m[0] == 0 {
                p[3] = u8::MAX;
            } else {
                p.copy_from_slice(&[0, 0, 0, 0]);
            }
        }
    }
    Some(px)
}

/// A PNG picture as `side`×`side` premultiplied BGRA pixels: scaled to fit and centred,
/// the rest transparent (the renderer's pictures are square). COM must be initialised.
pub fn png_pixels(png: &[u8], side: u32) -> Option<Vec<u8>> {
    use windows::Win32::Graphics::Imaging::*;
    use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
    if side == 0 {
        return None;
    }
    // SAFETY: (whole block) WIC objects created and used here; the stream reads `png`,
    // which outlives it; the buffer holds the copied rows exactly.
    unsafe {
        let wic: IWICImagingFactory = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok()?;
        let stream = wic.CreateStream().ok()?;
        stream.InitializeFromMemory(png).ok()?;
        let decoder = wic
            .CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnDemand)
            .ok()?;
        let frame = decoder.GetFrame(0).ok()?;
        let (mut w, mut h) = (0u32, 0u32);
        frame.GetSize(&mut w, &mut h).ok()?;
        if w == 0 || h == 0 {
            return None;
        }
        // Fitted inside the square, keeping its shape.
        let (fw, fh) = if w >= h {
            (side, (u64::from(side) * u64::from(h) / u64::from(w)).max(1) as u32)
        } else {
            ((u64::from(side) * u64::from(w) / u64::from(h)).max(1) as u32, side)
        };
        let scaler = wic.CreateBitmapScaler().ok()?;
        scaler
            .Initialize(&frame, fw, fh, WICBitmapInterpolationModeHighQualityCubic)
            .ok()?;
        let converter = wic.CreateFormatConverter().ok()?;
        converter
            .Initialize(
                &scaler,
                &GUID_WICPixelFormat32bppPBGRA,
                WICBitmapDitherTypeNone,
                None,
                0.0,
                WICBitmapPaletteTypeCustom,
            )
            .ok()?;
        let stride = fw as usize * BYTES_PER_PIXEL;
        let mut fitted = vec![0u8; stride * fh as usize];
        converter
            .CopyPixels(std::ptr::null(), u32::try_from(stride).ok()?, &mut fitted)
            .ok()?;
        let n = side as usize;
        let mut out = vec![0u8; n * n * BYTES_PER_PIXEL];
        let (ox, oy) = ((n - fw as usize) / 2, (n - fh as usize) / 2);
        for row in 0..fh as usize {
            let to = ((oy + row) * n + ox) * BYTES_PER_PIXEL;
            out[to..to + stride].copy_from_slice(&fitted[row * stride..(row + 1) * stride]);
        }
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    //! Edge cases: a system icon (has alpha) at two sizes, a bogus handle.
    use super::*;
    use windows::Win32::UI::WindowsAndMessaging::{LoadIconW, IDI_APPLICATION};

    const NEARLY_OPAQUE: u8 = 250;

    #[test]
    fn draws_a_system_icon_and_rejects_a_bogus_one() {
        // SAFETY: loads a shared system icon; it is never destroyed.
        let icon = unsafe { LoadIconW(None, IDI_APPLICATION) }.unwrap();
        for size in [16u32, 32] {
            let px = icon_pixels(icon.0 as usize as u64, size).unwrap();
            assert_eq!(px.len(), (size * size * 4) as usize);
            // GDI's alpha blend rounds full opacity to 254.
            let opaque = px.chunks_exact(4).filter(|p| p[3] >= NEARLY_OPAQUE).count();
            assert!(opaque > (size * size / 4) as usize, "{size}: {opaque} opaque pixels");
            // Premultiplied: no channel above its alpha.
            assert!(px.chunks_exact(4).all(|p| p[0] <= p[3] && p[1] <= p[3] && p[2] <= p[3]));
        }
        assert_eq!(icon_pixels(0x1234_5678, 16), None);
    }

    #[test]
    fn an_icon_in_a_file_is_drawn_by_its_index() {
        let shell32 = std::path::PathBuf::from(std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into()))
            .join("System32")
            .join("shell32.dll");
        let side = 20;
        let px = icon_in_file_pixels(&shell32, 3, side).expect("shell32's folder icon");
        assert_eq!(px.len(), (side * side * 4) as usize);
        assert!(px.chunks_exact(4).any(|p| p[3] > 0), "something drawn");
        assert!(icon_in_file_pixels(std::path::Path::new(r"C:\no\such.ico"), 0, side).is_none());
    }

    #[test]
    fn a_programs_file_gives_its_icon() {
        crate::system::com_init();
        let exe = std::env::var("SystemRoot")
            .map(|r| std::path::Path::new(&r).join("explorer.exe"))
            .unwrap();
        let px = file_icon_pixels(&exe, 20).expect("explorer's icon");
        assert_eq!(px.len(), 20 * 20 * 4);
        assert!(px.chunks_exact(4).any(|p| p[3] > 0), "not empty");
        assert!(file_icon_pixels(std::path::Path::new("C:\\nope\\missing.exe"), 20).is_none());
    }

    /// Edge cases: a program, a packaged app by its app id (Windows Settings, always
    /// installed), a name the shell does not know.
    #[test]
    fn shell_items_give_their_icons() {
        const SETTINGS: &str =
            r"shell:AppsFolder\windows.immersivecontrolpanel_cw5n1h2txyewy!microsoft.windows.immersivecontrolpanel";
        crate::system::com_init();
        let exe = std::env::var("SystemRoot")
            .map(|r| std::path::Path::new(&r).join("explorer.exe"))
            .unwrap();
        for name in [exe.to_str().unwrap(), SETTINGS] {
            let px = shell_item_icon_pixels(name, 24).unwrap_or_else(|| panic!("{name}"));
            assert_eq!(px.len(), 24 * 24 * 4);
            assert!(px.chunks_exact(4).any(|p| p[3] > 0), "{name}: empty");
            assert!(px.chunks_exact(4).any(|p| p[3] == 0), "{name}: no transparent corner");
            // Premultiplied, as the renderer draws them: no channel above its alpha (found
            // by the user: white specks around the dock's and the app grid's icons).
            let bright = px
                .chunks_exact(4)
                .filter(|p| p[0] > p[3] || p[1] > p[3] || p[2] > p[3])
                .count();
            assert_eq!(bright, 0, "{name}: {bright} pixels brighter than their alpha");
        }
        assert!(shell_item_icon_pixels(r"C:\nope\missing.exe", 24).is_none());
    }

    /// Edge cases: transparent, opaque and half-transparent pixels, the rounding.
    #[test]
    fn straight_alpha_becomes_premultiplied() {
        let mut px = vec![255, 255, 255, 0, 10, 20, 30, 255, 255, 128, 0, 128, 200, 100, 50, 1];
        premultiply(&mut px);
        assert_eq!(px, [0, 0, 0, 0, 10, 20, 30, 255, 128, 64, 0, 128, 1, 0, 0, 1]);
    }

    /// Edge cases: a wide PNG (Kabuk's mark) fitted into a square and centred, its
    /// corners transparent and its middle drawn; bytes that are no PNG.
    #[test]
    fn a_png_is_fitted_into_a_square() {
        crate::system::com_init();
        let png = include_bytes!("../../../assets/logo/kabuk-mark-small-on-dark-64.png");
        let px = png_pixels(png, 48).expect("decoded");
        assert_eq!(px.len(), 48 * 48 * 4);
        assert_eq!(px[3], 0, "the top-left corner is transparent");
        assert!(px.chunks_exact(4).filter(|p| p[3] > 200).count() > 100, "nothing drawn");
        assert!(png_pixels(b"not a png", 16).is_none());
        assert!(png_pixels(png, 0).is_none());
    }
}
