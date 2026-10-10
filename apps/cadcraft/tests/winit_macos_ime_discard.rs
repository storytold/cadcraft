//! vendor/winit patch (7); provenance: `vendor/patches/winit/07-macos-ime-discard.md`.

#[test]
fn winit_discards_the_macos_composition_when_ime_is_disallowed() {
    // egui disallows IME when a text field loses focus. winit 0.30.13 then forgets the marked text
    // but doesn't tell the input method, which keeps the composition and continues it once IME is
    // allowed again: the abandoned syllable comes back in the next field. vendor/winit calls
    // `discardMarkedText` first (rust-windowing/winit#4745). A re-vendored copy without the patch
    // would silently bring the bug back: re-apply the patch, or drop the copy once a winit 0.30
    // release has the fix.
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let view = std::fs::read_to_string(root.join("vendor/winit/src/platform_impl/macos/view.rs")).unwrap();
    let (_, set_ime_allowed) = view.split_once("pub(super) fn set_ime_allowed(").expect("WinitView::set_ime_allowed");
    let set_ime_allowed = set_ime_allowed.split_once("\n    }\n").map_or(set_ime_allowed, |(body, _)| body);
    assert!(set_ime_allowed.contains("input_context.discardMarkedText();"), "disallowing IME leaves the input method's composition");
}
