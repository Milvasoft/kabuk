//! Embeds Kabuk's logo as the icon, the application manifest (per-monitor V2 DPI
//! awareness, as-invoker: no administrator rights) and the four programs it installs.
//!
//! The programs come from the folder `KABUK_SETUP_PAYLOAD` names (`tools/package.ps1`
//! sets it to the release build); without it the setup holds nothing and says so, so a
//! plain workspace build still works.

use embed_manifest::manifest::{DpiAwareness, ExecutionLevel};
use embed_manifest::{embed_manifest, new_manifest};

/// The programs, as `kabuk_core::install::PROGRAMS` names them (a test checks they match).
const PROGRAMS: [&str; 4] = [
    "kabuk.exe",
    "kabuk-watchdog.exe",
    "kabuk-settings.exe",
    "kabuk-saver.exe",
];

fn main() {
    if std::env::var_os("CARGO_CFG_WINDOWS").is_some() {
        let manifest = new_manifest("Kabuk.Setup")
            .dpi_awareness(DpiAwareness::PerMonitorV2)
            .requested_execution_level(ExecutionLevel::AsInvoker);
        embed_manifest(manifest).expect("unable to embed manifest");
        embed_icon();
    }
    payload();
    println!("cargo:rerun-if-changed=build.rs");
}

/// `payload.rs` in `OUT_DIR`: the programs' names and bytes, or none.
fn payload() {
    println!("cargo:rerun-if-env-changed=KABUK_SETUP_PAYLOAD");
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR")).join("payload.rs");
    let mut code = String::from("pub const FILES: &[(&str, &[u8])] = &[\n");
    if let Some(dir) = std::env::var_os("KABUK_SETUP_PAYLOAD") {
        let dir = std::path::PathBuf::from(dir);
        for p in PROGRAMS {
            let file = dir.join(p);
            assert!(file.is_file(), "{} missing for the setup", file.display());
            println!("cargo:rerun-if-changed={}", file.display());
            let quoted = file.display().to_string().replace('\\', "\\\\");
            code.push_str(&format!("    (\"{p}\", include_bytes!(\"{quoted}\")),\n"));
        }
    }
    code.push_str("];\n");
    std::fs::write(&out, code).expect("payload written");
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
