//! vendor/winit patch (9); provenance: `vendor/patches/winit/09-macos-korean-space.md`.

#[test]
fn winit_sends_a_key_committed_through_macos_ime_only_once() {
    // Apple Korean answers Space with no syllable in progress with `setMarkedText(" ")`,
    // `insertText(" ")` and `setMarkedText("")`. The last call resets winit 0.30.13's IME state, so
    // the key went to the app both as `Ime::Commit(" ")` and as `KeyboardInput` with text " ", and
    // CADCraft's text fields got two spaces. vendor/winit doesn't send `KeyboardInput` for a key
    // whose text was committed, unless AppKit forwards it as a command (Enter) (the goal of
    // rust-windowing/winit#4478). A re-vendored copy without the patch would silently bring the bug
    // back: re-apply the patch, or drop the copy once a winit 0.30 release has the fix.
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let view = std::fs::read_to_string(root.join("vendor/winit/src/platform_impl/macos/view.rs")).unwrap();
    assert!(view.contains("self.ivars().committed_in_key_down.set(true);"), "a commit in keyDown isn't recorded");
    let consumed = "let had_ime_input = self.ivars().committed_in_key_down.replace(false) || had_ime_input;";
    assert!(view.contains(consumed), "a key committed through IME is sent again as KeyboardInput");
}
