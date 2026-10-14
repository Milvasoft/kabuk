//! Prints the bar's system figures twice, one second apart (manual check).

fn main() {
    let mut s = kabuk_win::stats::Sampler::default();
    let _ = s.sample(kabuk_win::stats::Wants::ALL);
    std::thread::sleep(std::time::Duration::from_secs(1));
    println!("{:?}", s.sample(kabuk_win::stats::Wants::ALL));
}
