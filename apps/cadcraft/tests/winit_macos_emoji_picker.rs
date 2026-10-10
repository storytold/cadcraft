//! vendor/winit patch (8); provenance: `vendor/patches/winit/08-macos-emoji-picker.md`.

#[test]
fn winit_commits_macos_text_inserted_outside_a_key_press() {
    // The macOS emoji picker inserts its text with `insertText` outside any key press and without
    // a composition. winit 0.30.13 reports `insertText` only as part of a key press or a
    // composition, so an emoji picked into a CADCraft text field was dropped. vendor/winit commits
    // such text (rust-windowing/winit#4749, the 0.30 backport of #4748). A re-vendored copy without
    // the patch would silently bring the bug back: re-apply the patch, or drop the copy once a winit
    // 0.30 release has the fix.
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let view = std::fs::read_to_string(root.join("vendor/winit/src/platform_impl/macos/view.rs")).unwrap();
    let outside_key_press = "} else if !self.ivars().in_key_down.get()\n                && self.ivars().ime_allowed.get()";
    assert!(view.contains(outside_key_press), "text inserted outside a key press is dropped again");
    // #4748's `selectedRange` change (for dictation) is deliberately left out: a valid selection
    // turns on press-and-hold for every held key in CADCraft's text fields (vendor/README.md).
    let (_, selected_range) = view.split_once("fn selected_range(&self) -> NSRange {").expect("selectedRange");
    let selected_range = selected_range.split_once("\n        }\n").map_or(selected_range, |(body, _)| body);
    assert!(!selected_range.contains("marked_text"), "selectedRange reports a selection");
}
