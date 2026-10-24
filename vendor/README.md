# Vendored crates

Third-party crates patched for CADCraft, used through `[patch.crates-io]` in the workspace
`Cargo.toml` and excluded from the workspace. Each is the published crate with only the patches
listed here; each patch is meant to go upstream.

| Crate | Version | Licence | Patches | Tests |
|---|---|---|---|---|
| winit | 0.30.13 | Apache-2.0 | (1) on Linux, the first window created without an activation token takes the one the launcher put in the environment (`XDG_ACTIVATION_TOKEN` on Wayland, `DESKTOP_STARTUP_ID` on X11) and hands it back as a token given through `with_activation_token` would be (`startup_token.rs`). eframe never gives winit a token, so with `StartupNotify=true` GNOME kept its busy cursor for 15 seconds after the window appeared, and a Wayland window opened without focus. Later windows don't reuse the spent token, and an empty or oversized (over 1,024 bytes) token is ignored. The variable is left in the environment, unlike `reset_activation_token_env`: changing the environment while other threads may read it is unsound. Port of PdfCraft's vendored winit patch (14) | (1) `cadcraft` `tests/winit_startup_token.rs` keeps the patch in the build, and `startup_token.rs`'s own tests test the policy (run them in the vendored crate); the busy cursor itself needs a desktop session and a launcher with `StartupNotify=true` |
