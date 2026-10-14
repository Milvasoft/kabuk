//! PLAN M6: a keystroke searches 5,000 items in under 5 ms. Run with a release build:
//!   cargo test --release -p kabuk-search --features perf --test perf -- --nocapture

#![cfg(feature = "perf")]

use std::time::{Duration, Instant};

use kabuk_search::{Casing, Index};

const ITEMS: usize = 5_000;
const LIMIT: Duration = Duration::from_millis(5);
/// Results the launcher shows.
const SHOWN: usize = 7;

/// Titles like a real machine's: apps, files and folders, bookmarks, commands.
fn titles() -> Vec<String> {
    let words = [
        "Ekranı",
        "kilitle",
        "Milvasoft",
        "Helpers",
        "Windows",
        "Terminal",
        "Rider",
        "Opsiyon",
        "ERP",
        "yönetim",
        "paneli",
        "kabuk",
        "toml",
        "Görev",
        "Yöneticisi",
        "İstanbul",
        "rapor",
        "Şubat",
        "fatura",
        "çizelge",
    ];
    (0..ITEMS)
        .map(|i| {
            let a = words[i % words.len()];
            let b = words[(i / words.len()) % words.len()];
            let c = words[(i / 7) % words.len()];
            format!("{a} {b} {c} {i}.docx")
        })
        .collect()
}

#[test]
fn a_keystroke_searches_five_thousand_items_in_time() {
    let titles = titles();
    let index = Index::new(&titles, Casing::Turkish);
    let mut worst = Duration::ZERO;
    // Typing "EKRANI kil" one letter at a time, and a query that finds nothing.
    let typed = "EKRANI kil";
    let queries: Vec<&str> = (1..=typed.len())
        .filter(|n| typed.is_char_boundary(*n))
        .map(|n| &typed[..n])
        .chain(["zzzz"])
        .collect();
    for q in &queries {
        let started = Instant::now();
        let hits = index.search(q, SHOWN);
        let took = started.elapsed();
        worst = worst.max(took);
        assert!(hits.len() <= SHOWN);
    }
    println!(
        "worst keystroke over {} items: {:.2} ms",
        titles.len(),
        worst.as_secs_f64() * 1000.0
    );
    assert!(worst < LIMIT, "{worst:?}");
}
