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
/// `titlebar`: `CADCRAFT_TITLEBAR`.
pub fn app_drawn(x11: bool, titlebar: Option<&str>) -> Option<Layout> {
    if cfg!(any(target_os = "macos", target_os = "windows")) || x11 {
        return None;
    }
    let drawn = match titlebar.map(str::trim) {
        Some("app") => true,
        Some("system") => false,
        _ => compositor_leaves_frame_to_app(),
    };
    drawn.then(|| Layout::or_default(read_button_layout().as_deref()))
}

/// Whether `CADCRAFT_TITLEBAR=app` asks for the app's frame, over Use system title bar too.
pub fn forces_app(titlebar: Option<&str>) -> bool {
    titlebar.map(str::trim) == Some("app")
}

/// Whether this is a Wayland session whose compositor draws no frames.
#[cfg(all(unix, not(target_os = "macos")))]
fn compositor_leaves_frame_to_app() -> bool {
    use wayland_client::Connection;
    use wayland_client::globals::registry_queue_init;
    // Not `Connection::connect_to_env`: given `WAYLAND_SOCKET`, it takes that descriptor (the
    // launcher's one connection, meant for winit), removes the variable and closes it when the
    // probe's connection drops, so winit could open no Wayland window.
    let display = std::env::var_os("WAYLAND_DISPLAY");
    let Some(path) = display_socket(display.as_deref(), std::env::var_os("XDG_RUNTIME_DIR").as_deref()) else {
        // No Wayland compositor: X11, whose window manager draws the frame (winit falls back to
        // it). Or only `WAYLAND_SOCKET`, which isn't probed; winit then draws its own frame.
        return false;
    };
    let Ok(stream) = std::os::unix::net::UnixStream::connect(path) else { return false };
    let Ok(conn) = Connection::from_socket(stream) else { return false };
    let Ok((globals, _queue)) = registry_queue_init::<Registry>(&conn) else { return false };
    let offered = globals.contents().with_list(|list| list.iter().any(|g| g.interface == "zxdg_decoration_manager_v1"));
    if !offered {
        log::info!("the Wayland compositor draws no window frames; CADCraft draws its own window buttons");
    }
    !offered
}

/// The compositor's socket named by `WAYLAND_DISPLAY`, as `wayland-client` finds it: an absolute
/// path, or a name in `XDG_RUNTIME_DIR` (which must be absolute). `None` when either is missing.
#[cfg(all(unix, not(target_os = "macos")))]
fn display_socket(display: Option<&std::ffi::OsStr>, runtime_dir: Option<&std::ffi::OsStr>) -> Option<std::path::PathBuf> {
    let name = std::path::Path::new(display.filter(|d| !d.is_empty())?);
    if name.is_absolute() {
        return Some(name.to_path_buf());
    }
    let dir = std::path::Path::new(runtime_dir?);
    dir.is_absolute().then(|| dir.join(name))
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

#[cfg(all(test, unix, not(target_os = "macos")))]
mod tests {
    use std::ffi::OsStr;
    use std::path::PathBuf;

    use super::{display_socket, forces_app};

    #[test]
    fn the_probe_finds_the_socket_through_wayland_display_only() {
        let some = |s: &'static str| Some(OsStr::new(s));
        assert_eq!(display_socket(some("wayland-0"), some("/run/user/1000")), Some(PathBuf::from("/run/user/1000/wayland-0")));
        assert_eq!(display_socket(some("/tmp/wl"), None), Some(PathBuf::from("/tmp/wl")), "an absolute WAYLAND_DISPLAY");
        // Only WAYLAND_SOCKET (or nothing): no socket to probe, so the launcher's stays for winit.
        assert_eq!(display_socket(None, some("/run/user/1000")), None);
        assert_eq!(display_socket(some(""), some("/run/user/1000")), None, "empty counts as unset");
        assert_eq!(display_socket(some("wayland-0"), None), None);
        assert_eq!(display_socket(some("wayland-0"), some("relative")), None);
    }

    #[test]
    fn only_titlebar_app_forces_the_apps_frame() {
        assert!(forces_app(Some("app")));
        assert!(forces_app(Some(" app ")));
        assert!(!forces_app(Some("system")));
        assert!(!forces_app(None));
    }
}
