//! One background update check at startup, the "Update and Restart" prompt, and Help ▸ Check for
//! Updates… (desktop only; the web build is always the latest). The flow itself is
//! `craft_update::updater::Updater`.

use craft_update::updater::{Action, Status, Updater};
use craft_update::{CheckResultDialog, CheckResultResponse};

use crate::CadApp;

/// The version this build reports to the update check.
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// What the update check and prompt are doing between frames.
pub struct Runtime {
    updater: Updater,
    /// Whether the host asked for a startup check (see [`CadApp::enable_update_check`]).
    enabled: bool,
    /// Custom development packages can opt out of stock release replacement.
    disabled: bool,
    /// Help ▸ Check for Updates… was chosen; the next frame's `tick` starts it with the context.
    check_requested: bool,
    /// The check-result dialog is open (from a requested check until it is closed).
    result_open: bool,
    /// The current result was already written to the command history.
    result_echoed: bool,
    /// Help ▸ Check for Updates… was chosen in a build with updates turned off: say so in a
    /// window, which shows even when the Start page is open or the command line is hidden.
    disabled_notice_open: bool,
}

impl Default for Runtime {
    fn default() -> Self {
        Self {
            updater: Updater::new(&crate::UPDATE_APP, VERSION),
            enabled: false,
            disabled: updates_disabled(std::env::var("CADCRAFT_DISABLE_UPDATES").ok().as_deref()),
            check_requested: false,
            result_open: false,
            result_echoed: false,
            disabled_notice_open: false,
        }
    }
}

impl CadApp {
    /// Turn on the startup update check (the desktop host does; tests and the web build do not).
    pub fn enable_update_check(&mut self) {
        self.updates_rt.enabled = !self.updates_rt.disabled;
    }
}

/// Help ▸ Check for Updates…: check now, and show the result when there is nothing newer.
pub fn check_now(app: &mut CadApp) {
    if app.updates_rt.disabled {
        app.session.echo(disabled_notice());
        app.updates_rt.disabled_notice_open = true;
        return;
    }
    app.updates_rt.check_requested = true;
    app.updates_rt.result_open = true;
    app.updates_rt.result_echoed = false;
}

/// Start the checks and collect the answers.
pub fn tick(app: &mut CadApp, ctx: &egui::Context) {
    let rt = &mut app.updates_rt;
    if rt.disabled {
        return;
    }
    if std::mem::take(&mut rt.check_requested) {
        rt.updater.check_now(ctx);
    } else if rt.enabled {
        rt.updater.check_at_start(ctx);
    }
    rt.updater.tick(ctx);
    let outcome = match rt.updater.status() {
        Status::UpToDate => Some(crate::tl!("CADCraft is up to date.")),
        Status::Failed(_) => Some(crate::tl!("Could not check for updates.")),
        Status::Idle | Status::Checking => None,
    };
    let Some(message) = outcome else {
        // A check that found an update leaves `Idle`; the prompt takes over from the dialog.
        if rt.result_open && matches!(rt.updater.status(), Status::Idle) {
            rt.result_open = false;
        }
        return;
    };
    if !rt.result_open {
        rt.updater.dismiss_status();
    } else if !std::mem::replace(&mut rt.result_echoed, true) {
        app.session.echo(message);
    }
}

/// Draw the prompt while there is one and act on its answer.
pub fn show(app: &mut CadApp, ctx: &egui::Context) {
    if app.updates_rt.disabled {
        if app.updates_rt.disabled_notice_open {
            let mut open = true;
            let mut close = false;
            egui::Window::new(crate::tl!("Check for Updates"))
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .show(ctx, |ui| {
                    ui.label(disabled_notice());
                    let ok = ui.button(crate::tl!("OK"));
                    crate::control::record_widget(ui, &ok, "OK", "button", None);
                    close = ok.clicked();
                });
            app.updates_rt.disabled_notice_open = open && !close;
        }
        return;
    }
    // The restart helper is scheduled before the app closes, so no update starts over unsaved work.
    let clean = !app.session.docs.iter().any(|d| d.is_dirty());
    let rt = &mut app.updates_rt;
    if rt.result_open {
        match CheckResultDialog::new(&crate::UPDATE_APP, VERSION).show(ctx, rt.updater.status()) {
            CheckResultResponse::Close => {
                rt.result_open = false;
                rt.updater.dismiss_status();
            }
            CheckResultResponse::CheckAgain => {
                rt.result_echoed = false;
                rt.check_requested = true;
            }
            CheckResultResponse::None => {}
        }
    }
    match app.updates_rt.updater.show_if(ctx, clean) {
        Action::OpenUrl(url) => ctx.open_url(egui::OpenUrl::new_tab(url)),
        // The guarded close path asks about unsaved changes; the restart helper relaunches once the
        // process has exited.
        Action::Close => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
        Action::None => {}
    }
}

/// What Help ▸ Check for Updates… says in a build with updates turned off, in the UI language.
fn disabled_notice() -> &'static str {
    crate::tl!("Release updates are disabled for this development build.")
}

/// Opt-in only: normal release packages retain their existing update behavior.
fn updates_disabled(value: Option<&str>) -> bool {
    matches!(value, Some("1" | "true"))
}

#[cfg(test)]
mod tests {
    #[test]
    fn development_opt_out_blocks_startup_and_manual_release_checks() {
        let mut app = crate::CadApp::new(cadcraft_engine::Session::new(), crate::Services::default());
        app.updates_rt.disabled = true;
        app.enable_update_check();
        assert!(!app.updates_rt.enabled);
        super::check_now(&mut app);
        assert!(!app.updates_rt.check_requested);
        assert!(!app.updates_rt.result_open);
        // The answer is a window, not only a command-history line (#2544 review).
        assert!(app.updates_rt.disabled_notice_open);
        let ctx = egui::Context::default();
        super::tick(&mut app, &ctx);
        assert!(matches!(app.updates_rt.updater.status(), craft_update::updater::Status::Idle));
    }

    #[test]
    fn development_update_opt_out_preserves_default_release_policy() {
        assert!(super::updates_disabled(Some("1")));
        assert!(super::updates_disabled(Some("true")));
        assert!(!super::updates_disabled(None));
        assert!(!super::updates_disabled(Some("0")));
    }
}
