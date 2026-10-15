//! Puts Kabuk's gallery (DECISIONS D58: `assets/gallery/<category>/<id>/wall.toml` and
//! `wall.hlsl`) into the crate: a table of every design with its files' text, so the
//! programs carry the designs and the setup ships nothing more.

use std::fmt::Write;
use std::path::Path;

/// The gallery's categories, in the order the table lists them.
const CATEGORIES: [&str; 4] = ["wall-live", "wall-still", "saver-live", "saver-still"];

fn main() {
    let root = Path::new(&std::env::var("CARGO_MANIFEST_DIR").expect("manifest dir")).join("../../assets/gallery");
    println!("cargo:rerun-if-changed={}", root.display());
    let mut table = String::from("pub const DESIGNS: &[Design] = &[\n");
    for category in CATEGORIES {
        let dir = root.join(category);
        println!("cargo:rerun-if-changed={}", dir.display());
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        let mut ids: Vec<_> = entries
            .flatten()
            .filter(|e| e.path().join("wall.hlsl").exists() && e.path().join("wall.toml").exists())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        ids.sort();
        for id in ids {
            let folder = dir.join(&id);
            let (toml, hlsl) = (folder.join("wall.toml"), folder.join("wall.hlsl"));
            println!("cargo:rerun-if-changed={}", folder.display());
            println!("cargo:rerun-if-changed={}", toml.display());
            println!("cargo:rerun-if-changed={}", hlsl.display());
            let _ = writeln!(
                table,
                "    Design {{ category: {category:?}, id: {id:?}, toml: include_str!({:?}), hlsl: include_str!({:?}) }},",
                toml.display().to_string(),
                hlsl.display().to_string(),
            );
        }
    }
    table.push_str("];\n");
    let out = Path::new(&std::env::var("OUT_DIR").expect("out dir")).join("gallery_table.rs");
    std::fs::write(out, table).expect("gallery table written");
}
