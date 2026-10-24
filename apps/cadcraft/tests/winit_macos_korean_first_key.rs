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
    let policy = std::fs::read_to_string(macos.join("ime_startup.rs")).expect("vendored winit first-key policy");
    assert!(
        policy.contains("source.starts_with(\"com.apple.inputmethod.Korean.\")") && policy.contains("'\\u{3131}'..='\\u{318E}'"),
        "retry is not restricted to an Apple Korean compatibility jamo"
    );
    assert!(policy.contains("if chars.next().is_none()"), "retry accepts more than one raw character");
    assert!(
        policy.contains("if serial != self.serial") && policy.contains("self.attempted = true;"),
        "nested key handling can replay the outer key or retry recursively"
    );
    for test in [
        "cold_single_jamo_gets_one_retry_only",
        "language_change_and_ime_reset_start_new_attempts",
        "preserves_ascii_paste_multi_char_other_layouts_and_shortcuts",
        "no_callback_is_not_a_fallback",
        "multiple_inserts_or_any_other_native_callback_prevent_replay",
        "nested_key_cannot_replay_outer_key",
        "focus_source_or_ime_change_during_callback_prevents_replay",
    ] {
        assert!(policy.contains(test), "missing first-key policy test: {test}");
    }

    let view = std::fs::read_to_string(macos.join("view.rs")).expect("vendored winit macOS view source");
    let (_, key_down) = view.split_once("fn key_down(").expect("WinitView::key_down");
    let (key_down, _) = key_down.split_once("fn key_up(").expect("end of keyDown method");
    for guard in [
        "self.ivars().ime_allowed.get()",
        "old_ime_state == ImeState::Disabled",
        "!self.ivars().pending_commit.get()",
        "!unsafe { self.hasMarkedText() }",
        "!unsafe { event.isARepeat() }",
        "ModifiersState::SUPER | ModifiersState::CONTROL | ModifiersState::ALT",
        "&& self.ime_retry_has_focus()",
        "self.current_input_source() == source",
    ] {
        assert!(view.contains(guard), "missing first-key guard: {guard}");
    }
    assert!(
        view.contains("self.ivars().ime_startup.borrow_mut().insert(&string);")
            && view.contains("self.ivars().ime_startup.borrow_mut().finish(serial, same_context)")
            && view.contains("if retry {\n                    unsafe { self.interpretKeyEvents(&events_for_nsview) };\n                }"),
        "keyDown no longer retries the single verified raw-jamo callback"
    );
    assert!(
        key_down.matches("self.current_input_source()").count() == 2
            && key_down.contains("let same_context = eligible\n                    && self.ivars().ime_allowed.get()"),
        "keyDown repeats input-source queries or checks retry context for ineligible events"
    );
    let (_, focus_guard) = view.split_once("fn ime_retry_has_focus(&self) -> bool {").expect("WinitView::ime_retry_has_focus");
    let (focus_guard, _) = focus_guard.split_once("fn current_input_source(&self) -> String {").expect("end of focus guard");
    assert!(
        focus_guard.contains("window.isKeyWindow()")
            && focus_guard.contains("window.firstResponder().is_some_and(|responder| *responder == ***self)"),
        "retry is not restricted to the key window with this view as first responder"
    );
    for callback in ["setMarkedText:selectedRange:replacementRange:", "unmarkText", "doCommandBySelector:"] {
        let (_, body) = view.split_once(callback).expect("native callback selector");
        assert!(body.contains("self.ivars().ime_startup.borrow_mut().other_callback();"), "{callback} no longer cancels a pending retry");
    }
}
