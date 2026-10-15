//! Applies an accent policy to every bar window of the running Kabuk, to see which one
//! Windows draws behind a window with no redirection bitmap.
//!
//!   cargo run -p kabuk-win --example blur_probe -- <state> <flags> <0xAABBGGRR>

use windows::core::{s, BOOL};
use windows::Win32::Foundation::HWND;
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};

#[repr(C)]
struct AccentPolicy {
    state: u32,
    flags: u32,
    gradient: u32,
    animation: u32,
}

#[repr(C)]
struct CompositionData {
    attribute: u32,
    data: *mut core::ffi::c_void,
    size: usize,
}

type SetAttribute = unsafe extern "system" fn(HWND, *mut CompositionData) -> BOOL;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let state: u32 = args[1].parse().unwrap();
    let flags: u32 = args[2].parse().unwrap();
    let gradient = u32::from_str_radix(args[3].trim_start_matches("0x"), 16).unwrap();
    // SAFETY: user32's export, with the signature above.
    let set: SetAttribute = unsafe {
        let user32 = GetModuleHandleW(windows::core::w!("user32.dll")).unwrap();
        std::mem::transmute::<unsafe extern "system" fn() -> isize, SetAttribute>(
            GetProcAddress(user32, s!("SetWindowCompositionAttribute")).unwrap(),
        )
    };
    for w in kabuk_win::window::windows_of_class("Kabuk.Bar") {
        let mut policy = AccentPolicy {
            state,
            flags,
            gradient,
            animation: 0,
        };
        let mut data = CompositionData {
            attribute: 19,
            data: (&mut policy as *mut AccentPolicy).cast(),
            size: std::mem::size_of::<AccentPolicy>(),
        };
        // SAFETY: a valid window handle and data.
        let ok = unsafe { set(HWND(w.0 as *mut _), &mut data) };
        println!("{:?}: {}", w, ok.as_bool());
    }
}
