//! Windows only: embed the app icon and version info into `cadcraft.exe`. Elsewhere a no-op.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../../assets/app-icon/cadcraft.ico");
    println!("cargo:rerun-if-env-changed=CADCRAFT_REQUIRE_WINRES");
    println!("cargo:rerun-if-env-changed=CADCRAFT_BUILD_SHA");
    println!("cargo:rerun-if-env-changed=CADCRAFT_VERSION_LABEL");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    if std::path::Path::new("../../assets/app-icon/cadcraft.ico").exists() {
        res.set_icon("../../assets/app-icon/cadcraft.ico");
    }
    res.set("ProductName", "CADCraft")
        .set("FileDescription", "CADCraft computer-aided design")
        .set("CompanyName", "Learning Machines LLC")
        .set("LegalCopyright", "Copyright (c) 2026 ArtCraft Team and the CADCraft contributors. MIT OR Apache-2.0.")
        .set("OriginalFilename", "cadcraft.exe")
        .set("InternalName", "cadcraft");
    // A build's own label (`0.9.0-b3`) in the file's Properties ▸ Details.
    if let Ok(label) = std::env::var("CADCRAFT_VERSION_LABEL") {
        let label: String = label.chars().filter(|c| c.is_ascii_graphic()).take(40).collect();
        if !label.is_empty() {
            res.set("ProductVersion", &label).set("FileVersion", &label);
        }
    }
    if let Err(e) = res.compile() {
        if std::env::var_os("CADCRAFT_REQUIRE_WINRES").is_some() {
            eprintln!("embedding Windows resources failed: {e}");
            std::process::exit(1);
        }
        println!("cargo:warning=cadcraft.exe built without icon/version resources: {e}");
    }
}
