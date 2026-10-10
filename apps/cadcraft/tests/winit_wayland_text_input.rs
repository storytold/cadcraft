//! vendor/winit patch (2); provenance: `vendor/patches/winit/02-wayland-text-input.md`.

#[test]
fn winit_keeps_the_wayland_text_input_until_the_seat_goes() {
    // winit 0.30.13 destroys a Wayland seat's text input whenever the seat loses any capability (a
    // touch screen or tablet unplugged, a remote-desktop session ending) and recreates it only when
    // a capability is added, so IME stops working in a running CADCraft until it is restarted.
    // vendor/winit destroys it with the seat instead (rust-windowing/winit#4747). A re-vendored copy
    // without the patch would silently bring the bug back: re-apply the patch, or drop the copy once
    // a winit 0.30 release has the fix.
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let seat = std::fs::read_to_string(root.join("vendor/winit/src/platform_impl/linux/wayland/seat/mod.rs")).unwrap();
    let (before_remove_seat, remove_seat) = seat.split_once("fn remove_seat(").expect("SeatHandler::remove_seat");
    let (_, remove_capability) = before_remove_seat.split_once("fn remove_capability(").expect("SeatHandler::remove_capability");
    let remove_seat = remove_seat.split_once("\n    }\n").map_or(remove_seat, |(body, _)| body);
    assert!(!remove_capability.contains("text_input.destroy()"), "losing a capability destroys the seat's text input again");
    assert!(remove_seat.contains("text_input.destroy()"), "removing the seat no longer destroys its text input");
}
