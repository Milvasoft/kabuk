//! Created wallpapers on the GPU (SPEC §9, PLAN M13): a wallpaper's pixel shader
//! compiled with `D3DCompile` and refused unless every pixel's work is bounded (no
//! dynamic flow control, at most [`MAX_INSTRUCTIONS`]), so no frame can hang the GPU;
//! frames drawn with a full-screen triangle, timed with timestamps at 4K and read back
//! for the OLED checks.

use std::ffi::c_void;

use kabuk_wallgen::format::{constants, shader_source, Manifest, TokenColor};
use kabuk_wallgen::prompt::MAX_INSTRUCTIONS;
use windows::core::{Interface, PCSTR};
use windows::Win32::Graphics::Direct3D::Fxc::{
    D3DCompile, D3DReflect, D3DCOMPILE_AVOID_FLOW_CONTROL, D3DCOMPILE_OPTIMIZATION_LEVEL3,
};
use windows::Win32::Graphics::Direct3D::{ID3DBlob, D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST};
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};

use super::{Context, Gpu, Pixels, Result};

/// The full-screen triangle: three vertices from their index, no buffers.
const VERTEX_SHADER: &str = "float4 main(uint id : SV_VertexID) : SV_Position\n{\n    float2 p = float2((id << 1) & 2, id & 2);\n    return float4(p * float2(2, -2) + float2(-1, 1), 0, 1);\n}\n";
/// The size frames are timed at (SPEC §9: 4K) and how many.
pub const TIMED_SIZE: (u32, u32) = (3840, 2160);
pub const TIMED_FRAMES: usize = 120;
/// A frame this many times over the limit ends the timing at once.
const HOPELESS: f32 = 10.0;
/// Seconds between timed frames.
const TIMED_STEP: f32 = 1.0 / 30.0;
/// The OLED checks' frames are drawn at this size and averaged down to their grid, so
/// thin lines count with their real share of light.
pub const OLED_SIZE: (u32, u32) = (960, 540);

/// What a frame is drawn with besides the time.
#[derive(Clone, Copy)]
pub struct WallInputs<'a> {
    pub tokens: TokenColor<'a>,
    pub values: &'a [f32],
    /// The user's peak brightness, 0-1.
    pub brightness: f32,
}

/// A compiled wallpaper.
pub struct WallShader {
    manifest: Manifest,
    vs: ID3D11VertexShader,
    ps: ID3D11PixelShader,
    constants: ID3D11Buffer,
    pub instructions: u32,
}

fn blob_text(b: &ID3DBlob) -> String {
    // SAFETY: the blob's pointer and size describe its own bytes.
    let bytes = unsafe { std::slice::from_raw_parts(b.GetBufferPointer() as *const u8, b.GetBufferSize()) };
    String::from_utf8_lossy(bytes).trim_end_matches('\0').trim().to_string()
}

fn blob_bytes(b: &ID3DBlob) -> &[u8] {
    // SAFETY: as above; the slice lives as long as the blob borrowed.
    unsafe { std::slice::from_raw_parts(b.GetBufferPointer() as *const u8, b.GetBufferSize()) }
}

/// Compiles `source`; the compiler's messages on failure.
fn compile(source: &str, name: &str, target: &str, flags: u32) -> std::result::Result<ID3DBlob, String> {
    let name_z = format!("{name}\0");
    let target_z = format!("{target}\0");
    let mut code = None;
    let mut errors = None;
    // SAFETY: the source, names and target are valid for the call; the blobs it returns
    // are owned by us.
    let r = unsafe {
        D3DCompile(
            source.as_ptr().cast(),
            source.len(),
            PCSTR(name_z.as_ptr()),
            None,
            None,
            PCSTR(c"main".as_ptr().cast()),
            PCSTR(target_z.as_ptr()),
            flags,
            0,
            &mut code,
            Some(&mut errors),
        )
    };
    match (r, code) {
        (Ok(()), Some(c)) => Ok(c),
        (r, _) => Err(errors
            .as_ref()
            .map(|b| without_folders(&blob_text(b), name))
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| r.err().map_or_else(|| "no code".to_string(), |e| e.to_string()))),
    }
}

/// The compiler names the source with the working folder before it; the messages go to
/// the model, so the folder (the user's name in it) is cut off.
fn without_folders(messages: &str, name: &str) -> String {
    messages
        .lines()
        .map(|l| l.find(name).map_or(l, |i| &l[i..]))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Why compiled code is refused: the work per pixel must be bounded (at most `limit`
/// instructions, no dynamic flow control).
fn bounded(code: &ID3DBlob, limit: u32) -> std::result::Result<u32, String> {
    let bytes = blob_bytes(code);
    let mut out: *mut c_void = std::ptr::null_mut();
    // SAFETY: the bytecode is valid for the call; the reflector returned is wrapped and
    // released by its COM wrapper.
    let reflector = unsafe {
        D3DReflect(
            bytes.as_ptr().cast(),
            bytes.len(),
            &ID3D11ShaderReflection::IID,
            &mut out,
        )
        .map_err(|e| e.to_string())?;
        ID3D11ShaderReflection::from_raw(out)
    };
    let mut d = D3D11_SHADER_DESC::default();
    // SAFETY: plain query into a struct of the right type.
    unsafe { reflector.GetDesc(&mut d) }.map_err(|e| e.to_string())?;
    if d.DynamicFlowControlCount > 0 {
        return Err(format!(
            "it has {} dynamic flow control instructions (a loop that cannot be unrolled, or [loop]/[branch]); every loop needs constant bounds",
            d.DynamicFlowControlCount
        ));
    }
    if d.InstructionCount > limit {
        return Err(format!(
            "it has {} instructions once compiled; at most {limit} are allowed",
            d.InstructionCount
        ));
    }
    Ok(d.InstructionCount)
}

/// A render target of `size` on the device, with its view.
struct Target {
    texture: ID3D11Texture2D,
    view: ID3D11RenderTargetView,
    size: (u32, u32),
}

impl Gpu {
    /// Compiles a wallpaper; the reason it is refused otherwise (said to the model).
    pub fn compile_wall(&self, manifest: &Manifest, hlsl: &str) -> std::result::Result<WallShader, String> {
        let flags = D3DCOMPILE_OPTIMIZATION_LEVEL3 | D3DCOMPILE_AVOID_FLOW_CONTROL;
        let ps_code = compile(&shader_source(manifest, hlsl), "wall.hlsl", "ps_5_0", flags)?;
        let instructions = bounded(&ps_code, MAX_INSTRUCTIONS)?;
        let vs_code = compile(VERTEX_SHADER, "kabuk-vs", "vs_5_0", D3DCOMPILE_OPTIMIZATION_LEVEL3)?;
        let d3d = self.d3d();
        let mut vs = None;
        let mut ps = None;
        let mut cb = None;
        let size = constants(manifest, 0.0, 1.0, (1.0, 1.0), &|_| None, &[]).len();
        let desc = D3D11_BUFFER_DESC {
            ByteWidth: u32::try_from(size).map_err(|e| e.to_string())?,
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
            ..Default::default()
        };
        // SAFETY: creation from bytecode the compiler gave us and a valid description.
        unsafe {
            d3d.CreateVertexShader(blob_bytes(&vs_code), None, Some(&mut vs))
                .map_err(|e| e.to_string())?;
            d3d.CreatePixelShader(blob_bytes(&ps_code), None, Some(&mut ps))
                .map_err(|e| e.to_string())?;
            d3d.CreateBuffer(&desc, None, Some(&mut cb))
                .map_err(|e| e.to_string())?;
        }
        Ok(WallShader {
            manifest: manifest.clone(),
            vs: vs.ok_or("no vertex shader")?,
            ps: ps.ok_or("no pixel shader")?,
            constants: cb.ok_or("no constant buffer")?,
            instructions,
        })
    }

    fn wall_target(&self, size: (u32, u32), shared: bool) -> Result<Target> {
        let desc = D3D11_TEXTURE2D_DESC {
            Width: size.0.max(1),
            Height: size.1.max(1),
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: (D3D11_BIND_RENDER_TARGET.0 | if shared { D3D11_BIND_SHADER_RESOURCE.0 } else { 0 }) as u32,
            ..Default::default()
        };
        let mut texture = None;
        let mut view = None;
        // SAFETY: creation with a valid description; the view is of the texture made here.
        unsafe {
            self.d3d()
                .CreateTexture2D(&desc, None, Some(&mut texture))
                .ctx("wall target")?;
            let t = texture.clone().ok_or(super::RenderError::DeviceLost)?;
            self.d3d()
                .CreateRenderTargetView(&t, None, Some(&mut view))
                .ctx("wall target view")?;
        }
        Ok(Target {
            texture: texture.ok_or(super::RenderError::DeviceLost)?,
            view: view.ok_or(super::RenderError::DeviceLost)?,
            size,
        })
    }

    /// Draws one frame of `shader` at `time` into `target` (submitted, not waited for).
    fn wall_draw(&self, shader: &WallShader, inputs: WallInputs, time: f32, target: &Target) -> Result<()> {
        let c = self.immediate()?;
        let bytes = constants(
            &shader.manifest,
            time,
            inputs.brightness,
            (target.size.0 as f32, target.size.1 as f32),
            inputs.tokens,
            inputs.values,
        );
        let viewport = D3D11_VIEWPORT {
            TopLeftX: 0.0,
            TopLeftY: 0.0,
            Width: target.size.0 as f32,
            Height: target.size.1 as f32,
            MinDepth: 0.0,
            MaxDepth: 1.0,
        };
        // SAFETY: state set on the immediate context with live objects of this device; the
        // constant bytes are the buffer's size.
        unsafe {
            c.UpdateSubresource(&shader.constants, 0, None, bytes.as_ptr().cast(), 0, 0);
            c.OMSetRenderTargets(Some(&[Some(target.view.clone())]), None);
            c.RSSetViewports(Some(&[viewport]));
            c.IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            c.IASetInputLayout(None);
            c.VSSetShader(&shader.vs, None);
            c.PSSetShader(&shader.ps, None);
            c.PSSetConstantBuffers(0, Some(&[Some(shader.constants.clone())]));
            c.Draw(3, 0);
            c.OMSetRenderTargets(None, None);
        }
        Ok(())
    }

    /// The slowest of [`TIMED_FRAMES`] frames at 4K in milliseconds (stopping early once
    /// a frame is hopelessly over `limit_ms`); `None` when the GPU cannot time (WARP).
    pub fn wall_frame_ms(&self, shader: &WallShader, inputs: WallInputs, limit_ms: f32) -> Result<Option<f32>> {
        let target = self.wall_target(TIMED_SIZE, false)?;
        let mut slowest = 0.0f32;
        for i in 0..TIMED_FRAMES {
            let (drawn, t) = self.gpu_time(|mark| {
                mark();
                self.wall_draw(shader, inputs, i as f32 * TIMED_STEP, &target)
            });
            drawn?;
            let Some(t) = t else { return Ok(None) };
            slowest = slowest.max(t.as_secs_f32() * 1000.0);
            if slowest > limit_ms * HOPELESS {
                break;
            }
        }
        Ok(Some(slowest))
    }

    /// Frames of `shader` at `size` at each of `times`, read back.
    pub fn wall_frames(
        &self,
        shader: &WallShader,
        inputs: WallInputs,
        size: (u32, u32),
        times: &[f32],
    ) -> Result<Vec<Pixels>> {
        let target = self.wall_target(size, false)?;
        let staging_desc = D3D11_TEXTURE2D_DESC {
            Width: size.0.max(1),
            Height: size.1.max(1),
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Usage: D3D11_USAGE_STAGING,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            ..Default::default()
        };
        let mut staging = None;
        // SAFETY: creation with a valid description.
        unsafe { self.d3d().CreateTexture2D(&staging_desc, None, Some(&mut staging)) }.ctx("wall staging")?;
        let staging = staging.ok_or(super::RenderError::DeviceLost)?;
        let c = self.immediate()?;
        let mut out = Vec::with_capacity(times.len());
        for t in times {
            self.wall_draw(shader, inputs, *t, &target)?;
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            // SAFETY: a copy between two textures of the same size and format, then the
            // staging texture mapped for reading and unmapped after its rows are copied.
            unsafe {
                c.CopyResource(&staging, &target.texture);
                c.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
                    .ctx("map wall frame")?;
                let mut rgba = Vec::with_capacity((size.0 * size.1 * 4) as usize);
                for y in 0..size.1 as usize {
                    let row = std::slice::from_raw_parts(
                        (mapped.pData as *const u8).add(y * mapped.RowPitch as usize),
                        size.0 as usize * 4,
                    );
                    for px in row.chunks_exact(4) {
                        rgba.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
                    }
                }
                c.Unmap(&staging, 0);
                out.push(Pixels {
                    width: size.0,
                    height: size.1,
                    rgba,
                });
            }
        }
        Ok(out)
    }
}

/// A wallpaper's frames for a window: a texture the shader draws into and the Direct2D
/// bitmap over it that the window's surface copies ([`super::WindowSurface::draw_wall`]).
pub struct WallCanvas {
    target: Target,
    pub(crate) bitmap: windows::Win32::Graphics::Direct2D::ID2D1Bitmap1,
}

impl WallCanvas {
    pub fn size(&self) -> (u32, u32) {
        self.target.size
    }
}

impl Gpu {
    /// A canvas of `size` for drawing a wallpaper into a window.
    pub fn wall_canvas(&self, size: (u32, u32)) -> Result<WallCanvas> {
        use windows::Win32::Graphics::Direct2D::Common::{D2D1_ALPHA_MODE_IGNORE, D2D1_PIXEL_FORMAT};
        use windows::Win32::Graphics::Direct2D::{D2D1_BITMAP_OPTIONS_NONE, D2D1_BITMAP_PROPERTIES1};
        let target = self.wall_target(size, true)?;
        let surface: windows::Win32::Graphics::Dxgi::IDXGISurface = target.texture.cast().ctx("wall surface")?;
        let props = D2D1_BITMAP_PROPERTIES1 {
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_IGNORE,
            },
            dpiX: crate::geom::Scale::BASE_DPI,
            dpiY: crate::geom::Scale::BASE_DPI,
            bitmapOptions: D2D1_BITMAP_OPTIONS_NONE,
            colorContext: std::mem::ManuallyDrop::new(None),
        };
        // SAFETY: a bitmap over our own texture, on the same device as the context.
        let bitmap =
            unsafe { self.context()?.CreateBitmapFromDxgiSurface(&surface, Some(&props)) }.ctx("wall bitmap")?;
        Ok(WallCanvas { target, bitmap })
    }

    /// Draws `shader` at `time` into `canvas`.
    pub fn draw_wall_frame(
        &self,
        shader: &WallShader,
        inputs: WallInputs,
        time: f32,
        canvas: &WallCanvas,
    ) -> Result<()> {
        self.wall_draw(shader, inputs, time, &canvas.target)
    }
}

/// The Create pipeline's checks on this GPU ([`kabuk_wallgen::pipeline::Judge`]): the
/// frames are drawn in the look's colours at full brightness (the strictest case).
pub struct GpuJudge<'a> {
    pub gpu: &'a Gpu,
    pub tokens: TokenColor<'a>,
}

impl GpuJudge<'_> {
    fn inputs(&self) -> WallInputs<'_> {
        WallInputs {
            tokens: self.tokens,
            values: &[],
            brightness: 1.0,
        }
    }
}

/// `p` averaged down to `grid` cells of brightness ([`kabuk_wallgen::check`]).
pub fn brightness_grid(p: &Pixels, grid: (u32, u32)) -> Vec<f32> {
    let (gw, gh) = (grid.0.max(1), grid.1.max(1));
    let mut sum = vec![0.0f32; (gw * gh) as usize];
    let mut count = vec![0u32; (gw * gh) as usize];
    for y in 0..p.height {
        for x in 0..p.width {
            let [r, g, b, _] = p.at(x, y);
            let cell = ((y * gh / p.height.max(1)) * gw + x * gw / p.width.max(1)) as usize;
            sum[cell] += kabuk_wallgen::check::brightness(r, g, b);
            count[cell] += 1;
        }
    }
    sum.iter().zip(&count).map(|(s, c)| s / (*c).max(1) as f32).collect()
}

impl kabuk_wallgen::pipeline::Judge for GpuJudge<'_> {
    type Compiled = WallShader;

    fn compile(&mut self, manifest: &Manifest, hlsl: &str) -> std::result::Result<WallShader, String> {
        self.gpu.compile_wall(manifest, hlsl)
    }

    fn frame_ms(&mut self, compiled: &WallShader) -> std::result::Result<f32, String> {
        match self
            .gpu
            .wall_frame_ms(compiled, self.inputs(), kabuk_wallgen::prompt::MAX_FRAME_MS)
        {
            Ok(Some(ms)) => Ok(ms),
            Ok(None) => Err("this computer's GPU cannot time frames".into()),
            Err(e) => Err(e.to_string()),
        }
    }

    fn oled_frames(&mut self, compiled: &WallShader) -> std::result::Result<Vec<Vec<f32>>, String> {
        let times = kabuk_wallgen::check::sample_times();
        let frames = self
            .gpu
            .wall_frames(compiled, self.inputs(), OLED_SIZE, &times)
            .map_err(|e| e.to_string())?;
        Ok(frames
            .iter()
            .map(|f| brightness_grid(f, kabuk_wallgen::check::GRID))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    //! Edge cases are in `tests/render.rs` (they need the GPU); here only the vertex
    //! shader and the bounded-work rule on code compiled without a device.
    use super::*;

    #[test]
    fn the_vertex_shader_compiles() {
        assert!(compile(VERTEX_SHADER, "vs", "vs_5_0", 0).is_ok());
    }

    #[test]
    fn unbounded_loops_and_huge_shaders_are_refused() {
        let m = Manifest::parse("name = \"x\"").unwrap();
        let flags = D3DCOMPILE_OPTIMIZATION_LEVEL3 | D3DCOMPILE_AVOID_FLOW_CONTROL;
        // A loop without constant bounds compiles as a loop and is refused by the
        // bounded-work rule; so is one kept as a loop on purpose.
        let looped = "float3 wall(float2 px)\n{\n    float3 c = 0;\n    for (int i = 0; i < (int)(time * 10); i++) { c += 0.001; }\n    return c;\n}\n";
        let code = compile(&shader_source(&m, looped), "wall.hlsl", "ps_5_0", flags).unwrap();
        assert!(bounded(&code, MAX_INSTRUCTIONS)
            .unwrap_err()
            .contains("dynamic flow control"));
        let kept = "float3 wall(float2 px)\n{\n    float3 c = 0;\n    [loop] for (int i = 0; i < 8; i++) { c += sin(px.x * i + time) * 0.01; }\n    return c;\n}\n";
        let code = compile(&shader_source(&m, kept), "wall.hlsl", "ps_5_0", flags).unwrap();
        assert!(bounded(&code, MAX_INSTRUCTIONS)
            .unwrap_err()
            .contains("dynamic flow control"));
        let unrolled = "float3 wall(float2 px)\n{\n    float3 c = 0;\n    [unroll] for (int i = 0; i < 8; i++) { c += sin(px.x * i + time) * 0.01; }\n    return c;\n}\n";
        let code = compile(&shader_source(&m, unrolled), "wall.hlsl", "ps_5_0", flags).unwrap();
        assert!(bounded(&code, MAX_INSTRUCTIONS).is_ok());
        // Unrolled but longer than allowed (a small limit here; compiling a shader of
        // MAX_INSTRUCTIONS takes the compiler most of a minute).
        let code = compile(&shader_source(&m, unrolled), "wall.hlsl", "ps_5_0", flags).unwrap();
        let n = bounded(&code, MAX_INSTRUCTIONS).unwrap();
        assert!(bounded(&code, n - 1)
            .unwrap_err()
            .contains(&format!("at most {} are allowed", n - 1)));
        // Past the compiler's own unrolling limit: its message names no folder of the
        // user's.
        let endless = "float3 wall(float2 px)\n{\n    float3 c = 0;\n    [unroll] for (int i = 0; i < 3000; i++) { c += 0.0001; }\n    return c;\n}\n";
        let msg = compile(&shader_source(&m, endless), "wall.hlsl", "ps_5_0", flags).unwrap_err();
        assert!(msg.starts_with("wall.hlsl(4,") && msg.contains("unroll"), "{msg}");
        assert!(!msg.contains(":\\"), "{msg}");
        let broken = compile(
            &shader_source(&m, "float3 wall(float2 px) { return nope; }"),
            "wall.hlsl",
            "ps_5_0",
            flags,
        );
        let msg = broken.unwrap_err();
        assert!(msg.starts_with("wall.hlsl(1,") && msg.contains("nope"), "{msg}");
    }
}
