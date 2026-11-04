//! A window that can't start, or stops, says why instead of the app quitting silently.
//!
//! Without a usable GPU, graphics driver or desktop (a broken or very old driver, a remote or
//! virtual machine, a program started by a service or a test harness), the window's surface
//! can't be configured and eframe returns an error. CADCraft used to log it and exit with no
//! window, which looks like "the app doesn't open".
//! winget-pkgs validation starts installed programs without a desktop, so it rejected every
//! CADCraft version since 0.8 with `Validation-Executable-Error` and WinGet stayed on 0.7.0.
//!
//! Now the reason also goes to an error dialog, as FilmCraft does since 0.14.0. The dialog keeps
//! the process alive until it is dismissed; the exit code afterwards is still the error's.

/// What the dialog and the log say. Any error `run_native` returns lands here. One from the app's
/// own start-up (`AppCreation`, e.g. a `--control` file it can't write) says only that it couldn't
/// start. Any other comes from the window or graphics, possibly after the window was up, so
/// graphics is named as the likely cause, not the certain one.
pub(crate) fn message(error: &eframe::Error) -> String {
    match error {
        eframe::Error::AppCreation(_) => format!("CADCraft could not start: {error}"),
        _ => format!(
            "CADCraft's window could not start or stopped with an error: {error}\n\nIf this keeps happening, updating the graphics driver often fixes it."
        ),
    }
}

/// Logs the failure and shows it in a dialog, which stays until the user closes it.
pub(crate) fn report(error: &eframe::Error) {
    let message = message(error);
    log::error!("{message}");
    let _ = rfd::MessageDialog::new().set_title("CADCraft").set_description(&message).set_level(rfd::MessageLevel::Error).show();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_window_or_graphics_error_names_the_driver_as_the_likely_fix() {
        let text = message(&eframe::Error::Wgpu(eframe::egui_wgpu::WgpuError::NoSurfaceFormatsAvailable));
        assert!(text.starts_with("CADCraft's window could not start or stopped"), "{text}");
        assert!(text.contains("no valid format for the surface"), "{text}");
        assert!(text.contains("graphics driver"), "{text}");
    }

    #[test]
    fn an_app_start_up_error_does_not_blame_the_graphics() {
        let text = message(&eframe::Error::AppCreation("can't write the control file".into()));
        assert!(text.starts_with("CADCraft could not start"), "{text}");
        assert!(text.contains("can't write the control file"), "{text}");
        assert!(!text.contains("graphics"), "{text}");
    }
}
