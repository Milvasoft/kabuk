//! Sets a picture on Monitor 1 (main) with Kabuk's wrapper and prints the result, to tell
//! a picture Windows cannot read from one it was given too early.
//!
//!   cargo run -p kabuk-win --example wallpaper_set_probe -- <picture>

fn main() {
    kabuk_win::system::com_init();
    let picture = std::env::args().nth(1).expect("a picture");
    let main = kabuk_win::shell::monitors()
        .into_iter()
        .find(|m| m.primary)
        .expect("a main monitor");
    let path = std::path::PathBuf::from(picture);
    println!("exists: {}", path.exists());
    println!("{:?}", kabuk_win::wallpaper::set(main.rect, &path));
}
