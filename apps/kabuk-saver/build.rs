//! Embeds Kabuk's logo as the icon and the application manifest: per-monitor (V2) DPI awareness, Windows 10/11
//! compatibility, UTF-8 code page, as-invoker execution level.

use embed_manifest::manifest::{DpiAwareness, ExecutionLevel};
use embed_manifest::{embed_manifest, new_manifest};

fn main() {
    if std::env::var_os("CARGO_CFG_WINDOWS").is_some() {
        let manifest = new_manifest("Kabuk.Saver")
            .dpi_awareness(DpiAwareness::PerMonitorV2)
            .requested_execution_level(ExecutionLevel::AsInvoker);
        embed_manifest(manifest).expect("unable to embed manifest");
        embed_icon();
    }
    println!("cargo:rerun-if-changed=build.rs");
}

/// Kabuk's logo (`assets/logo/kabuk.ico`) as the program's icon, resource 1.
fn embed_icon() {
    let dir = std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let icon = dir.join("..").join("..").join("assets").join("logo").join("kabuk.ico");
    let rc = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR")).join("kabuk-icon.rc");
    let quoted = icon.display().to_string().replace('\\', "\\\\");
    std::fs::write(&rc, format!("1 ICON \"{quoted}\"\n")).expect("icon resource written");
    embed_resource::compile(&rc, embed_resource::NONE)
        .manifest_optional()
        .expect("icon resource compiled");
    println!("cargo:rerun-if-changed={}", icon.display());
}
