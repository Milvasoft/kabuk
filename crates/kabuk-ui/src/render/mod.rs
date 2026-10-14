//! The Windows renderer: Direct3D 11 → DXGI → Direct2D 1.1 device context, text with
//! DirectWrite and the embedded fonts, and DirectComposition for window surfaces
//! (ARCHITECTURE §6). Drawing happens only when asked; the only motion is a
//! surface's pose, played by the compositor ([`crate::motion`]).

mod device;
mod draw;
mod fonts;
mod offscreen;
mod scene;
mod surface;
mod text;
mod wall;

pub use device::{Gpu, Images, Raster};
pub use draw::Palette;
pub use fonts::FONT_FILES;
pub use offscreen::Pixels;
pub use scene::{SceneCache, SceneCanvas};
pub use surface::{PixelRect, WindowSurface};
pub use text::{Families, TextEngine, CSS_LINE_HEIGHT};
pub use wall::{brightness_grid, GpuJudge, WallCanvas, WallInputs, WallShader, OLED_SIZE, TIMED_FRAMES, TIMED_SIZE};

/// Renderer failures. `DeviceLost` means every GPU object must be recreated.
#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error("the GPU device was lost")]
    DeviceLost,
    #[error("{context}: {source}")]
    Win32 {
        context: &'static str,
        #[source]
        source: windows::core::Error,
    },
}

pub type Result<T> = std::result::Result<T, RenderError>;

pub(crate) trait Context<T> {
    fn ctx(self, context: &'static str) -> Result<T>;
}

impl<T> Context<T> for windows::core::Result<T> {
    fn ctx(self, context: &'static str) -> Result<T> {
        self.map_err(|source| {
            use windows::Win32::Foundation::D2DERR_RECREATE_TARGET;
            use windows::Win32::Graphics::Dxgi::{DXGI_ERROR_DEVICE_REMOVED, DXGI_ERROR_DEVICE_RESET};
            let code = source.code();
            if code == DXGI_ERROR_DEVICE_REMOVED || code == DXGI_ERROR_DEVICE_RESET || code == D2DERR_RECREATE_TARGET {
                RenderError::DeviceLost
            } else {
                RenderError::Win32 { context, source }
            }
        })
    }
}
