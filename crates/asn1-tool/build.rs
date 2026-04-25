//! Build script: embed a Windows icon, application manifest, and version
//! resource into the binary. No-op on other platforms.
//!
//! We route through `winresource` rather than hand-writing a `.rc` file so
//! `cargo build` works on non-Windows cross targets without needing `rc.exe`
//! — `winresource` bundles `windres` via a fallback when MSVC tooling is
//! absent.

fn main() {
    // Rebuild if any of the resource inputs change.
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/icon.ico");
    println!("cargo:rerun-if-changed=assets/app.manifest");

    #[cfg(windows)]
    embed_windows_resources();
}

#[cfg(windows)]
fn embed_windows_resources() {
    use std::path::Path;

    let manifest = Path::new("assets/app.manifest");
    let icon = Path::new("assets/icon.ico");

    let mut res = winresource::WindowsResource::new();
    res.set("ProductName", "asn1-tool")
        .set("FileDescription", "Interactive ASN.1 tree visualizer")
        .set("CompanyName", env!("CARGO_PKG_AUTHORS"))
        .set("LegalCopyright", &format!("Copyright (c) {}", env!("CARGO_PKG_AUTHORS")))
        .set("OriginalFilename", "asn1-tool.exe")
        .set("InternalName", "asn1-tool");

    if icon.exists() {
        res.set_icon(icon.to_str().expect("icon path is UTF-8"));
    } else {
        println!(
            "cargo:warning=assets/icon.ico missing — binary will use the default Windows icon"
        );
    }

    if manifest.exists() {
        let manifest_text = std::fs::read_to_string(manifest).expect("reading app.manifest");
        res.set_manifest(&manifest_text);
    } else {
        println!(
            "cargo:warning=assets/app.manifest missing — binary will use the default manifest"
        );
    }

    if let Err(e) = res.compile() {
        println!("cargo:warning=failed to embed Windows resources: {e}");
    }
}
