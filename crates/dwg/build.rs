//! Sets `no_dwg_bridge` on the targets built without acadrust (see Cargo.toml): the DWG functions
//! there return an error that says to use DXF.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo::rustc-check-cfg=cfg(no_dwg_bridge)");
    let arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if arch == "wasm32" || (os == "freebsd" && arch == "aarch64") {
        println!("cargo::rustc-cfg=no_dwg_bridge");
    }
}
