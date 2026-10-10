//! Structural guard for the local winit 0.30.13 Windows 11 DPI backport.

#[test]
fn winit_uses_the_windows_11_suggested_dpi_rectangle() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let lock = std::fs::read_to_string(root.join("Cargo.lock")).unwrap();
    let winit = lock.split("[[package]]").find(|package| package.contains("\nname = \"winit\"\n")).expect("winit is in Cargo.lock");
    assert!(!winit.contains("\nsource = "), "winit must resolve to vendor/winit");

    let event_loop = std::fs::read_to_string(root.join("vendor/winit/src/platform_impl/windows/event_loop.rs")).unwrap();
    assert!(
        event_loop.contains("if !WIN10_BUILD_VERSION.is_some_and(|build| build < 22000) {")
            && event_loop.contains("new_outer_rect = suggested_rect;"),
        "vendor/winit lost the Windows 11 WM_DPICHANGED patch"
    );
}
