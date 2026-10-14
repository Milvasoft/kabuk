//! Embeds the application manifest: per-monitor (V2) DPI awareness, Windows 10/11
//! compatibility, UTF-8 code page, as-invoker execution level.

use embed_manifest::manifest::{DpiAwareness, ExecutionLevel};
use embed_manifest::{embed_manifest, new_manifest};

fn main() {
    if std::env::var_os("CARGO_CFG_WINDOWS").is_some() {
        let manifest = new_manifest("Kabuk.TestApp")
            .dpi_awareness(DpiAwareness::PerMonitorV2)
            .requested_execution_level(ExecutionLevel::AsInvoker);
        embed_manifest(manifest).expect("unable to embed manifest");
    }
    println!("cargo:rerun-if-changed=build.rs");
}
