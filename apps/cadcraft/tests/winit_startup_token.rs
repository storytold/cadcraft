//! vendor/winit patch (vendor/README.md).

#[test]
fn winit_hands_the_launchers_activation_token_back() {
    // A launcher with `StartupNotify=true` (the packaged desktop entry) passes an activation token
    // in `XDG_ACTIVATION_TOKEN` (Wayland) or `DESKTOP_STARTUP_ID` (X11) and shows a busy cursor
    // until a window hands it back. eframe never gives winit a token, so GNOME kept the busy
    // cursor for 15 seconds and the window opened without focus. vendor/winit gives the first
    // window the environment's token. The policy is tested beside it (`startup_token::tests`, run
    // them in the vendored crate); this guards the copy, which workspace tests don't test. A
    // re-vendored copy without the patch would silently bring the bug back: re-apply the patch, or
    // drop the copy once winit or eframe hands the token back.
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let linux = root.join("vendor/winit/src/platform_impl/linux");
    let module = std::fs::read_to_string(linux.join("mod.rs")).unwrap();
    assert!(module.contains("mod startup_token;"));
    let (_, new) = module.split_once("    pub(crate) fn new(\n        window_target: &ActiveEventLoop,").expect("Window::new moved");
    let new = new.split_once("\n    }\n").map_or(new, |(body, _)| body);
    assert!(
        new.contains("startup_token::for_new_window(given, startup_notify::WAYLAND_VAR)"),
        "a Wayland window no longer takes the launcher's token"
    );
    assert!(new.contains("startup_token::for_new_window(given, startup_notify::X11_VAR)"), "an X11 window no longer takes the launcher's token");
    let policy = std::fs::read_to_string(linux.join("startup_token.rs")).unwrap();
    assert!(policy.contains("fn the_first_window_takes_the_launchers_token"));
    assert!(policy.contains("fn a_later_window_does_not_reuse_the_spent_token"));
    // The token reaches the app only when the launcher asks for startup notification.
    let desktop = std::fs::read_to_string(root.join("packaging/linux/ai.storyteller.cadcraft.desktop")).unwrap();
    assert!(desktop.contains("StartupNotify=true"));
}
