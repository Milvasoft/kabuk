//! Private memory of a bare D3D11 device: hardware vs WARP, and of DirectWrite (manual profiling).

use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE, D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP};
use windows::Win32::Graphics::Direct3D11::{D3D11CreateDevice, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION};
use windows::Win32::System::ProcessStatus::{
    GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX,
};
use windows::Win32::System::Threading::GetCurrentProcess;

fn mb() -> f64 {
    let mut m = PROCESS_MEMORY_COUNTERS_EX {
        cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
        ..Default::default()
    };
    // SAFETY: valid out structure for the current process.
    unsafe {
        let _ = GetProcessMemoryInfo(
            GetCurrentProcess(),
            (&mut m as *mut PROCESS_MEMORY_COUNTERS_EX).cast::<PROCESS_MEMORY_COUNTERS>(),
            m.cb,
        );
    }
    m.PrivateUsage as f64 / 1048576.0
}

fn device(t: D3D_DRIVER_TYPE) {
    let mut d = None;
    // SAFETY: plain device creation for measurement.
    unsafe {
        D3D11CreateDevice(
            None,
            t,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            None,
            D3D11_SDK_VERSION,
            Some(&mut d),
            None,
            None,
        )
        .unwrap();
    }
    std::mem::forget(d);
}

fn main() {
    let which = std::env::args().nth(1).unwrap_or_default();
    println!("start {:6.1} MB", mb());
    match which.as_str() {
        "warp" => device(D3D_DRIVER_TYPE_WARP),
        "dwrite" => {
            let _ = kabuk_ui::render::TextEngine::new().unwrap();
        }
        _ => device(D3D_DRIVER_TYPE_HARDWARE),
    }
    println!("{which:>8} {:6.1} MB", mb());
}
