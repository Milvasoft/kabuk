//! Embeds Kabuk's logo as the icon and the application manifest (per-monitor DPI awareness, as invoker) and puts
//! WebView2's loader (`WebView2Loader.dll`, shipped with the `webview2-com-sys` crate)
//! next to the program, where Windows looks for it.

use embed_manifest::manifest::{DpiAwareness, ExecutionLevel};
use embed_manifest::{embed_manifest, new_manifest};
use std::path::{Path, PathBuf};

/// The `webview2-com-sys` version the workspace pins (`webview2-com = "=0.39.1"`).
const SYS_CRATE: &str = "webview2-com-sys-0.39.1";
const LOADER: &str = "WebView2Loader.dll";

fn arch_dir() -> &'static str {
    match std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() {
        Ok("aarch64") => "arm64",
        Ok("x86") => "x86",
        _ => "x64",
    }
}

/// The loader in the cargo registry's unpacked sources.
fn find_loader() -> Option<PathBuf> {
    let home = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(|h| Path::new(&h).join(".cargo")))?;
    let src = home.join("registry").join("src");
    std::fs::read_dir(src)
        .ok()?
        .flatten()
        .map(|index| index.path().join(SYS_CRATE).join(arch_dir()).join(LOADER))
        .find(|p| p.is_file())
}

fn main() {
    if std::env::var_os("CARGO_CFG_WINDOWS").is_some() {
        let manifest = new_manifest("Kabuk.Settings")
            .dpi_awareness(DpiAwareness::PerMonitorV2)
            .requested_execution_level(ExecutionLevel::AsInvoker);
        embed_manifest(manifest).expect("unable to embed manifest");
        embed_icon();
    }
    // OUT_DIR is target/<profile>/build/kabuk-settings-<hash>/out; the program is in target/<profile>.
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    let profile_dir = out.ancestors().nth(3).expect("target profile directory").to_path_buf();
    match find_loader() {
        Some(loader) => {
            for dir in [profile_dir.clone(), profile_dir.join("deps")] {
                let _ = std::fs::create_dir_all(&dir);
                std::fs::copy(&loader, dir.join(LOADER)).expect("copy WebView2Loader.dll");
            }
        }
        None => println!("cargo:warning={LOADER} not found in the cargo registry; the settings app will not start"),
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
