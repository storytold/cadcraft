//! vendor/winit patch (5); provenance: `vendor/patches/winit/05-x11-ime-focus.md`.

#[test]
fn winit_focuses_the_x11_input_context_it_replaces() {
    // egui allows IME when a text field gains focus and disallows it when the field loses focus.
    // On X11, winit 0.30.13 replaces the window's input context each time and leaves focusing the
    // new one to the next focus event, which never comes while CADCraft's window keeps the focus:
    // the input method doesn't follow the field that has it. vendor/winit focuses the replacement
    // once the queued requests are handled (rust-windowing/winit#4727). A re-vendored copy without
    // the patch would silently bring the bug back: re-apply the patch, or drop the copy once a winit
    // 0.30 release has the fix.
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let x11 = root.join("vendor/winit/src/platform_impl/linux/x11");
    let ime = std::fs::read_to_string(x11.join("ime/mod.rs")).unwrap();
    let set_ime_allowed = "allowed: bool,\n    ) -> Result<bool, ImeContextCreationError> {";
    assert!(ime.contains(set_ime_allowed), "set_ime_allowed no longer says whether it replaced the context");
    let events = std::fs::read_to_string(x11.join("event_processor.rs")).unwrap();
    assert!(events.contains("Ok(true) => ime_focus = Some(window_id),"), "a replaced input context is not focused");
    assert!(events.contains("let _ = ime.get_mut().focus(window_id);"), "a replaced input context is not focused");
}
