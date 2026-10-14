//! Pictures through Windows Imaging (WIC): a picture file read and scaled to cover a
//! size (its middle kept, the rest cut), and pixels written as a JPEG file; for the still
//! wallpaper's drift (DECISIONS D47). COM must be initialised.

use std::path::Path;

use windows::core::HSTRING;
use windows::Win32::Graphics::Imaging::*;
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};

use crate::sync::io_error;

const BGR: usize = 3;

fn factory() -> std::io::Result<IWICImagingFactory> {
    // SAFETY: creates WIC's factory, an in-process COM object.
    unsafe { CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER) }.map_err(io_error)
}

/// The picture in `file` scaled to cover `w` × `h` with its shape kept and its middle
/// cut out: top-down rows of blue, green, red.
pub fn cover(file: &Path, w: u32, h: u32) -> std::io::Result<Vec<u8>> {
    if w == 0 || h == 0 {
        return Err(std::io::Error::other("no size"));
    }
    let wic = factory()?;
    // SAFETY: (whole block) WIC objects created and used here; the buffer holds exactly
    // the rows copied into it.
    unsafe {
        let decoder = wic
            .CreateDecoderFromFilename(
                &HSTRING::from(file.as_os_str()),
                None,
                windows::Win32::Foundation::GENERIC_READ,
                WICDecodeMetadataCacheOnDemand,
            )
            .map_err(io_error)?;
        let frame = decoder.GetFrame(0).map_err(io_error)?;
        let (mut iw, mut ih) = (0u32, 0u32);
        frame.GetSize(&mut iw, &mut ih).map_err(io_error)?;
        if iw == 0 || ih == 0 {
            return Err(std::io::Error::other("an empty picture"));
        }
        // Covering: the larger of the two scales, rounded up so nothing is missing.
        let k = (f64::from(w) / f64::from(iw)).max(f64::from(h) / f64::from(ih));
        let sw = ((f64::from(iw) * k).ceil() as u32).max(w);
        let sh = ((f64::from(ih) * k).ceil() as u32).max(h);
        let scaler = wic.CreateBitmapScaler().map_err(io_error)?;
        scaler
            .Initialize(&frame, sw, sh, WICBitmapInterpolationModeHighQualityCubic)
            .map_err(io_error)?;
        let converter = wic.CreateFormatConverter().map_err(io_error)?;
        converter
            .Initialize(
                &scaler,
                &GUID_WICPixelFormat24bppBGR,
                WICBitmapDitherTypeNone,
                None,
                0.0,
                WICBitmapPaletteTypeCustom,
            )
            .map_err(io_error)?;
        let rect = WICRect {
            X: i32::try_from((sw - w) / 2).unwrap_or(0),
            Y: i32::try_from((sh - h) / 2).unwrap_or(0),
            Width: i32::try_from(w).unwrap_or(0),
            Height: i32::try_from(h).unwrap_or(0),
        };
        let stride = w as usize * BGR;
        let mut out = vec![0u8; stride * h as usize];
        converter
            .CopyPixels(&rect, u32::try_from(stride).unwrap_or(0), &mut out)
            .map_err(io_error)?;
        Ok(out)
    }
}

/// `w` × `h` pixels (top-down rows of blue, green, red) written as a JPEG file.
pub fn save_jpeg(file: &Path, w: u32, h: u32, bgr: &[u8]) -> std::io::Result<()> {
    if bgr.len() != w as usize * h as usize * BGR {
        return Err(std::io::Error::other("pixels of another size"));
    }
    let wic = factory()?;
    // SAFETY: (whole block) WIC objects created and used here; the pixels outlive the
    // calls that read them.
    unsafe {
        let stream = wic.CreateStream().map_err(io_error)?;
        stream
            .InitializeFromFilename(
                &HSTRING::from(file.as_os_str()),
                windows::Win32::Foundation::GENERIC_WRITE.0,
            )
            .map_err(io_error)?;
        let encoder = wic
            .CreateEncoder(&GUID_ContainerFormatJpeg, std::ptr::null())
            .map_err(io_error)?;
        encoder.Initialize(&stream, WICBitmapEncoderNoCache).map_err(io_error)?;
        let mut frame = None;
        encoder
            .CreateNewFrame(&mut frame, std::ptr::null_mut())
            .map_err(io_error)?;
        let frame = frame.ok_or_else(|| std::io::Error::other("no frame"))?;
        frame.Initialize(None).map_err(io_error)?;
        frame.SetSize(w, h).map_err(io_error)?;
        let mut format = GUID_WICPixelFormat24bppBGR;
        frame.SetPixelFormat(&mut format).map_err(io_error)?;
        frame
            .WritePixels(h, u32::try_from(w as usize * BGR).unwrap_or(0), bgr)
            .map_err(io_error)?;
        frame.Commit().map_err(io_error)?;
        encoder.Commit().map_err(io_error)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    //! Edge cases: a wide picture covering a tall size keeps its middle; a written JPEG
    //! reads back at its size; no size and wrong pixels are refused.
    use super::*;

    #[test]
    fn a_picture_covers_a_size_and_round_trips_as_jpeg() {
        crate::system::com_init();
        let dir = tempfile::tempdir().unwrap();
        // 40×10: left half red, right half blue (BGR rows).
        let (w, h) = (40u32, 10u32);
        let px: Vec<u8> = (0..h)
            .flat_map(|_| (0..w).flat_map(|x| if x < w / 2 { [0, 0, 255] } else { [255, 0, 0] }))
            .collect();
        let file = dir.path().join("p.jpg");
        save_jpeg(&file, w, h, &px).unwrap();
        // Covering 10×10: the middle, half red and half blue.
        let c = cover(&file, 10, 10).unwrap();
        assert_eq!(c.len(), 10 * 10 * 3);
        let left = &c[..3];
        let right = &c[(10 - 1) * 3..10 * 3];
        assert!(left[2] > 150 && left[0] < 100, "red at the left: {left:?}");
        assert!(right[0] > 150 && right[2] < 100, "blue at the right: {right:?}");
        assert!(cover(&file, 0, 5).is_err());
        assert!(save_jpeg(&file, 2, 2, &[0; 5]).is_err());
    }
}
