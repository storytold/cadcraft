//! Who draws the window frame on Linux and the BSDs.
//!
//! Wayland compositors that draw frames for their clients offer `zxdg_decoration_manager_v1`
//! (KDE, Sway, Hyprland, COSMIC). GNOME doesn't: it leaves the frame to the app, and winit then
//! drew an imitation of GNOME's above CADCraft's tool bar. Without that protocol CADCraft turns
//! winit's frame off and draws a title row as the header bar, as in GNOME's own apps
//! (`cadcraft_ui_egui::window_controls`), with the window buttons GNOME's `button-layout` names.
//! X11 window managers always draw the frame.
//!
//! `CADCRAFT_TITLEBAR=app` or `=system` overrides the choice.
//!
//! Ported from WordCraft (storytold/wordcraft#3361).

use cadcraft_ui_egui::window_controls::Layout;

/// The window buttons to draw in the app, or `None` when the desktop draws the frame. `x11`: the
/// app runs through X11 (XWayland included, `CADCRAFT_X11`), whose window manager draws it.
pub fn app_drawn(x11: bool) -> Option<Layout> {
    if cfg!(any(target_os = "macos", target_os = "windows")) || x11 {
        return None;
    }
    let drawn = match std::env::var("CADCRAFT_TITLEBAR").ok().as_deref().map(str::trim) {
        Some("app") => true,
        Some("system") => false,
        _ => compositor_leaves_frame_to_app(),
    };
    drawn.then(|| Layout::or_default(read_button_layout().as_deref()))
}

/// Whether this is a Wayland session whose compositor draws no frames.
#[cfg(all(unix, not(target_os = "macos")))]
fn compositor_leaves_frame_to_app() -> bool {
    use wayland_client::Connection;
    use wayland_client::globals::registry_queue_init;
    // No Wayland compositor: X11, whose window manager draws the frame (winit falls back to it).
    let Ok(conn) = Connection::connect_to_env() else { return false };
    let Ok((globals, _queue)) = registry_queue_init::<Registry>(&conn) else { return false };
    let offered = globals.contents().with_list(|list| list.iter().any(|g| g.interface == "zxdg_decoration_manager_v1"));
    if !offered {
        log::info!("the Wayland compositor draws no window frames; CADCraft draws its own window buttons");
    }
    !offered
}

#[cfg(not(all(unix, not(target_os = "macos"))))]
fn compositor_leaves_frame_to_app() -> bool {
    false
}

/// GNOME's `org.gnome.desktop.wm.preferences button-layout`, from the XDG Desktop Portal as
/// `desktop_theme` reads the colour scheme. Start-up waits on this reply, so a portal that never
/// answers can't hold the window back: it is read only once, and the portal may be starting on
/// demand at login, so it gets 2 s.
#[cfg(all(unix, not(target_os = "macos")))]
fn read_button_layout() -> Option<String> {
    let connection = zbus::blocking::connection::Builder::session().ok()?.method_timeout(std::time::Duration::from_secs(2)).build().ok()?;
    let reply = connection
        .call_method(
            Some("org.freedesktop.portal.Desktop"),
            "/org/freedesktop/portal/desktop",
            Some("org.freedesktop.portal.Settings"),
            "ReadOne",
            &("org.gnome.desktop.wm.preferences", "button-layout"),
        )
        .ok()?;
    let (answer,) = reply.body().deserialize::<(zbus::zvariant::OwnedValue,)>().ok()?;
    String::try_from(&*answer).ok()
}

#[cfg(not(all(unix, not(target_os = "macos"))))]
fn read_button_layout() -> Option<String> {
    None
}

#[cfg(all(unix, not(target_os = "macos")))]
struct Registry;

#[cfg(all(unix, not(target_os = "macos")))]
impl wayland_client::Dispatch<wayland_client::protocol::wl_registry::WlRegistry, wayland_client::globals::GlobalListContents> for Registry {
    fn event(
        _: &mut Self,
        _: &wayland_client::protocol::wl_registry::WlRegistry,
        _: wayland_client::protocol::wl_registry::Event,
        _: &wayland_client::globals::GlobalListContents,
        _: &wayland_client::Connection,
        _: &wayland_client::QueueHandle<Self>,
    ) {
    }
}
