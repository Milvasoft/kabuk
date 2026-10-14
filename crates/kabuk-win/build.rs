//! Gives the crate's examples (spikes and profiling tools) the same application manifest
//! as Kabuk's programs: Windows 10/11 compatibility (needed for layered child windows, as
//! the M12 wallpaper spike uses) and per-monitor (V2) DPI awareness. The library itself
//! is not linked here, so nothing changes for the programs that use it.

use std::io::Write;

const MANIFEST: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
    <application>
      <supportedOS Id="{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}"/>
    </application>
  </compatibility>
  <application xmlns="urn:schemas-microsoft-com:asm.v3">
    <windowsSettings>
      <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2</dpiAwareness>
    </windowsSettings>
  </application>
</assembly>
"#;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() != Ok("msvc") {
        return;
    }
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    let path = out.join("examples.manifest");
    let mut f = std::fs::File::create(&path).expect("manifest file");
    f.write_all(MANIFEST.as_bytes()).expect("manifest written");
    println!("cargo:rustc-link-arg-examples=/MANIFEST:EMBED");
    println!("cargo:rustc-link-arg-examples=/MANIFESTINPUT:{}", path.display());
    println!("cargo:rustc-link-arg-examples=/MANIFESTUAC:NO");
}
