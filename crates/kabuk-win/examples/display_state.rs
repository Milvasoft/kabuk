//! Prints whether Windows has the displays on, as the animated wallpaper sees it
//! (`kabuk_win::power::DisplayWatch`), then each change for `--seconds` (default 1).
//!
//!   cargo run -p kabuk-win --example display_state -- [--seconds 60]

fn main() {
    let seconds: u64 = std::env::args()
        .position(|a| a == "--seconds")
        .and_then(|i| std::env::args().nth(i + 1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);
    let watch = kabuk_win::power::DisplayWatch::start().expect("display state watched");
    let mut last = None;
    let end = std::time::Instant::now() + std::time::Duration::from_secs(seconds);
    while std::time::Instant::now() < end {
        let on = watch.displays_on();
        if last != Some(on) {
            println!("displays on: {on}");
            last = Some(on);
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}
