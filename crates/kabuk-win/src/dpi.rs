//! Coordinates are always physical pixels. Windows scales coordinates for callers that
//! are not DPI aware, so every wrapper that reads or writes screen coordinates runs
//! inside [`physical`], whatever the awareness of the calling process or thread.

use windows::Win32::UI::HiDpi::{SetThreadDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};

/// Runs `f` with this thread in per-monitor (V2) awareness, then restores the previous
/// awareness.
pub fn physical<T>(f: impl FnOnce() -> T) -> T {
    // SAFETY: changes only this thread's awareness; the previous context is restored below.
    let previous = unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
    let result = f();
    if !previous.is_invalid() {
        // SAFETY: restores the context returned by the call above on the same thread.
        unsafe { SetThreadDpiAwarenessContext(previous) };
    }
    result
}
