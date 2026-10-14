//! Prints how many recent files Windows keeps for an app id (the jump list's "Recent"),
//! to try which id a program goes by. Only the count is printed, never the files.
//!
//!   cargo run -p kabuk-win --example recent_count -- <app id> [<app id> ...]

fn main() {
    kabuk_win::system::com_init();
    for id in std::env::args().skip(1) {
        let n = kabuk_win::jumplist::documents(&id, kabuk_core::jumplist::Known::Recent, 50).len();
        println!("{n:>3}  {id}");
    }
}
