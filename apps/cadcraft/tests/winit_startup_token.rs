//! The winit patch W30 (the pinned storytold/winit fork's `PATCHES.md`).

use std::path::PathBuf;

/// The source directory of the winit the workspace builds, from `cargo metadata`.
fn winit_source() -> PathBuf {
    let out = std::process::Command::new(env!("CARGO"))
        .args(["metadata", "--format-version", "1", "--offline", "--manifest-path"])
        .arg(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml"))
        .output()
        .unwrap();
    assert!(out.status.success(), "cargo metadata failed: {}", String::from_utf8_lossy(&out.stderr));
    let meta: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let winit = meta["packages"].as_array().unwrap().iter().find(|p| p["name"] == "winit").expect("winit is in the build");
    PathBuf::from(winit["manifest_path"].as_str().unwrap()).parent().unwrap().to_path_buf()
}

#[test]
fn winit_hands_the_launchers_activation_token_back() {
    // A launcher with `StartupNotify=true` (the packaged desktop entry) passes an activation token
    // in `XDG_ACTIVATION_TOKEN` (Wayland) or `DESKTOP_STARTUP_ID` (X11) and shows a busy cursor
    // until a window hands it back. eframe never gives winit a token, so GNOME kept the busy
    // cursor for 15 seconds and the window opened without focus. W30 gives the first window the
    // environment's token. The policy is tested beside it (`startup_token::tests`, run them in the
    // winit fork); this guards the pin: moving winit to a revision without W30 would silently
    // bring the bug back. Drop it once winit or eframe hands the token back.
    let linux = winit_source().join("src/platform_impl/linux");
    let module = std::fs::read_to_string(linux.join("mod.rs")).unwrap();
    assert!(module.contains("mod startup_token;"), "the pinned winit lacks W30");
    let (_, new) = module.split_once("    pub(crate) fn new(\n        window_target: &ActiveEventLoop,").expect("Window::new moved");
    let new = new.split_once("\n    }\n").map_or(new, |(body, _)| body);
    assert!(new.contains("startup_token::with_token(given, startup_notify::WAYLAND_VAR,"), "a Wayland window no longer takes the launcher's token");
    assert!(new.contains("startup_token::with_token(given, startup_notify::X11_VAR,"), "an X11 window no longer takes the launcher's token");
    let policy = std::fs::read_to_string(linux.join("startup_token.rs")).unwrap();
    assert!(policy.contains("fn the_first_window_takes_the_launchers_token"));
    assert!(policy.contains("fn a_later_window_does_not_reuse_the_spent_token"));
    assert!(policy.contains("fn a_failed_window_leaves_the_token_for_a_retry"));
    // The token reaches the app only when the launcher asks for startup notification.
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let desktop = std::fs::read_to_string(root.join("packaging/linux/ai.storyteller.cadcraft.desktop")).unwrap();
    assert!(desktop.contains("StartupNotify=true"));
}
