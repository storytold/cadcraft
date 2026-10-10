//! vendor/winit patch (10); provenance: `vendor/patches/winit/10-macos-korean-first-key.md`.

#[test]
fn winit_retries_the_first_apple_korean_key_once() {
    // On the first key in a freshly started app, Apple Korean can answer with `insertText` and the
    // raw compatibility jamo instead of starting a composition, so typing g k s in CADCraft gave
    // ㅎㅏㄴ instead of 한. vendor/winit interprets that one key again before sending it to the
    // app, under the conditions `ImeStartup` checks (rust-windowing/winit#4744). A re-vendored copy
    // without the patch would silently bring the bug back: re-apply the patch, or drop the copy once
    // a winit 0.30 release has the fix.
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let macos = root.join("vendor/winit/src/platform_impl/macos");
    let policy = std::fs::read_to_string(macos.join("ime_startup.rs")).expect("vendor/winit lost ime_startup.rs");
    assert!(policy.contains("source.starts_with(\"com.apple.inputmethod.Korean.\")"), "the retry is no longer limited to Apple Korean");
    let view = std::fs::read_to_string(macos.join("view.rs")).unwrap();
    assert!(
        view.contains("let retry = self.ivars().ime_startup.borrow_mut().finish(serial, same_context);"),
        "keyDown no longer retries the first key"
    );
    assert!(view.contains("self.ivars().ime_startup.borrow_mut().insert(&string);"), "insertText no longer reports to the retry policy");
}
