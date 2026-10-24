//! vendor/winit patch (9); provenance: `vendor/patches/winit/09-macos-korean-space.md`.

#[test]
fn winit_sends_a_key_committed_through_macos_ime_only_once() {
    // Apple Korean answers Space with no syllable in progress with `setMarkedText(" ")`,
    // `insertText(" ")` and `setMarkedText("")`. The last call resets winit 0.30.13's IME state,
    // so the key went to the app both as `Ime::Commit(" ")` and as `KeyboardInput` with text " ",
    // and CADCraft's text fields got two spaces. vendor/winit records an IME commit during
    // `keyDown:` and consumes that marker once after the event (rust-windowing/winit#4478). A
    // re-vendored copy without the patch would silently bring the bug back: re-apply the patch, or
    // drop the copy once a winit 0.30 release has the fix.
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let view = std::fs::read_to_string(root.join("vendor/winit/src/platform_impl/macos/view.rs")).expect("vendored winit macOS view source");
    assert!(view.contains("self.ivars().committed_in_key_down.set(true);"), "a commit in keyDown isn't recorded");
    assert!(
        view.contains("let had_ime_input = self.ivars().committed_in_key_down.replace(false) || had_ime_input;"),
        "the per-key commit marker isn't consumed"
    );
    assert!(
        view.contains("if !had_ime_input || self.ivars().forward_key_to_app.get() {"),
        "command forwarding can no longer override duplicate-key suppression"
    );
}
