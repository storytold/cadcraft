//! vendor/winit patch (3); provenance: `vendor/patches/winit/03-windows-ime-caret.md`.

#[test]
fn winit_reads_the_windows_ime_cursor_only_when_it_is_reported() {
    // winit 0.30.13 reads `GCS_CURSORPOS` for every composition without a target clause, whether
    // or not `WM_IME_COMPOSITION` says the IME set it. The Microsoft Korean IME never sets it, so
    // the composition's cursor read as 0, before the syllable being composed. vendor/winit puts it
    // at the end of the composition instead (rust-windowing/winit#4746). A re-vendored copy without
    // the patch would silently bring the bug back: re-apply the patch, or drop the copy once a winit
    // 0.30 release has the fix.
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let windows = root.join("vendor/winit/src/platform_impl/windows");
    let ime = std::fs::read_to_string(windows.join("ime.rs")).unwrap();
    assert!(ime.contains("if composition_flags & GCS_CURSORPOS != 0 {"), "vendor/winit reads GCS_CURSORPOS unconditionally again");
    let event_loop = std::fs::read_to_string(windows.join("event_loop.rs")).unwrap();
    assert!(event_loop.contains("get_composing_text_and_cursor(lparam as u32)"), "WM_IME_COMPOSITION no longer passes its flags on");
}
