//! Private memory after each renderer setup step (manual profiling).

fn mb() -> f64 {
    let me = kabuk_win_free_private();
    me as f64 / 1048576.0
}

fn kabuk_win_free_private() -> usize {
    use windows::Win32::System::ProcessStatus::{
        GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX,
    };
    use windows::Win32::System::Threading::GetCurrentProcess;
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
    m.PrivateUsage
}

fn main() {
    println!("start            {:6.1} MB", mb());
    let gpu = kabuk_ui::render::Gpu::new().unwrap();
    println!("gpu + text       {:6.1} MB", mb());
    let l = kabuk_ui::node::layout(
        &kabuk_ui::node::Node::icon("cpu", 16.0, kabuk_ui::node::Role::Fg),
        kabuk_ui::geom::Point::default(),
        &gpu.text,
    );
    let _ = gpu.render_pixels(
        &l,
        &Default::default(),
        kabuk_ui::geom::Scale(1.0),
        2560,
        65,
        kabuk_theme::Color::BLACK,
    );
    println!("after one draw   {:6.1} MB", mb());
}
