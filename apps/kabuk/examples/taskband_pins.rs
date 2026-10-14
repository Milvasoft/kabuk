//! Prints the pins Windows' taskbar lists in its own record (`Taskband\Favorites`), in
//! its order, as the apps module reads them (`kabuk::bar::apps::taskband_pins`).
//!
//!   cargo run -p kabuk --example taskband_pins -- [<shortcut file name> ...]

fn main() {
    let key = r"Software\Microsoft\Windows\CurrentVersion\Explorer\Taskband";
    let Some(blob) = kabuk_win::registry::read_binary(key, "Favorites") else {
        println!("no Favorites value");
        return;
    };
    println!("{} bytes", blob.len());
    let names: Vec<String> = std::env::args().skip(1).collect();
    for pin in kabuk::bar::apps::taskband_pins(&blob, &names) {
        println!("{pin:?}");
    }
}
