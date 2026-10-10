//! vendor/winit patch (6); provenance: `vendor/patches/winit/06-macos-ime-commit.md`.

#[test]
fn winit_commits_macos_compositions_that_end_without_marked_text() {
    // winit 0.30.13 treats `insertText` as an IME commit only while marked text is left. Input
    // methods that end a composition with `unmarkText` first lost the committed text, and Apple
    // Korean, which clears its marked text before inserting the key typed after a syllable (`한`
    // then `5`), lost that key. vendor/winit commits when a composition was started
    // (`pending_commit`, rust-windowing/winit#4650, the 0.30 backport of #4651 on winit master), and
    // a commit outside a key press no longer swallows the next key (#4748). A re-vendored copy
    // without the patch would silently bring the bug back: re-apply the patch, or drop the copy once
    // a winit 0.30 release has the fix.
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let view = std::fs::read_to_string(root.join("vendor/winit/src/platform_impl/macos/view.rs")).unwrap();
    assert!(view.contains("if pending_commit && ime_enabled && !is_control {"), "insertText commits only while marked text is left");
    assert!(view.contains("self.ivars().pending_commit.set(true);"), "setMarkedText no longer starts a pending commit");
    assert!(view.contains("self.ivars().in_key_down.set(true);"), "a commit outside a key press swallows the next key again");
    assert!(view.contains("NSString::from_str(\"NSUnderline\")"), "validAttributesForMarkedText lost the underline attribute");
}
