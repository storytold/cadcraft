//! One background update check at startup, the "Update and Restart" prompt, and Help ▸ Check for
//! Updates… (desktop only; the web build is always the latest). The flow itself is
//! `craft_update::updater::Updater`; its dialogs run on [`CadApp::framework_dialogs`].

use craft_ui::dialog::DialogResult;
use craft_update::stack_dialogs::{CheckChoice, UpdateCheckDialog};
use craft_update::updater::{Action, Status, Updater};

use crate::CadApp;

/// The version this build reports to the update check.
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// What the update check and prompt are doing between frames.
pub struct Runtime {
    updater: Updater,
    /// Whether the host asked for a startup check (see [`CadApp::enable_update_check`]).
    enabled: bool,
    /// Help ▸ Check for Updates… was chosen; the next frame's `tick` starts it with the context.
    check_requested: bool,
    /// The current result was already written to the command history.
    result_echoed: bool,
}

impl Default for Runtime {
    fn default() -> Self {
        Self { updater: Updater::new(&crate::UPDATE_APP, VERSION), enabled: false, check_requested: false, result_echoed: false }
    }
}

impl CadApp {
    /// Turn on the startup update check (the desktop host does; tests and the web build do not).
    pub fn enable_update_check(&mut self) {
        self.updates_rt.enabled = true;
    }
}

/// Help ▸ Check for Updates…: check now, and show the result when there is nothing newer.
pub fn check_now(app: &mut CadApp) {
    app.updates_rt.check_requested = true;
    app.updates_rt.result_echoed = false;
}

/// Whether the startup check runs: the host asked for it and the user did not turn it off
/// (Options ▸ User Preferences, `checkUpdatesAtStart`).
fn checks_at_start(app: &CadApp) -> bool {
    app.updates_rt.enabled && app.options.check_updates_at_start
}

/// Start the checks and collect the answers.
pub fn tick(app: &mut CadApp, ctx: &egui::Context) {
    let at_start = checks_at_start(app);
    let rt = &mut app.updates_rt;
    if std::mem::take(&mut rt.check_requested) {
        rt.updater.check_now();
    } else if at_start {
        rt.updater.check_at_start();
    }
    rt.updater.tick(ctx);
    let outcome = match rt.updater.status() {
        Status::UpToDate => Some(crate::tl!("CADCraft is up to date.")),
        Status::Failed(_) => Some(crate::tl!("Could not check for updates.")),
        _ => None,
    };
    if let Some(message) = outcome
        && !std::mem::replace(&mut rt.result_echoed, true)
    {
        app.session.echo(message);
    }
}

/// Whether the check result window shows: for every status but idle (a requested check, and the
/// first start after an update, which reports that it installed or why it failed), and never
/// over the startup prompt.
fn result_window_shows(status: &Status, prompt_open: bool) -> bool {
    !prompt_open && !matches!(status, Status::Idle)
}

/// One pass of the update flow, before the dialog host runs ([`CadApp::run_framework_dialogs`]): answers the
/// check result dialog closed in the last pass, drives the startup prompt, and keeps the result dialog open and
/// current while the updater has something to report.
pub fn show(app: &mut CadApp, ctx: &egui::Context) {
    // The close for a ready update waits until no drawing holds unsaved changes.
    let clean = !app.session.docs.iter().any(|d| d.is_dirty());
    // Answered before the dialog is kept open below, so a dismissed result does not open again at once.
    match app.framework_dialogs.take::<UpdateCheckDialog>() {
        Some(DialogResult::Ok(CheckChoice::CheckNow | CheckChoice::CheckAgain)) => {
            app.updates_rt.result_echoed = false;
            app.updates_rt.check_requested = true;
        }
        // The next `run` asks the window to close once nothing is unsaved.
        Some(DialogResult::Ok(CheckChoice::RestartToUpdate)) => app.updates_rt.updater.restart_to_update(),
        Some(DialogResult::Ok(CheckChoice::Update)) => {
            app.updates_rt.updater.request_install(clean);
        }
        Some(DialogResult::Ok(CheckChoice::Download)) => ctx.open_url(egui::OpenUrl::new_tab(crate::UPDATE_APP.releases_url())),
        // Close, Esc and the close box dismiss the outcome it showed.
        Some(_) => app.updates_rt.updater.dismiss_status(),
        None => {}
    }
    match app.updates_rt.updater.run(&mut app.framework_dialogs, clean) {
        Action::OpenUrl(url) => ctx.open_url(egui::OpenUrl::new_tab(url)),
        // The guarded close path asks about unsaved changes. The install helper starts when the
        // app exits and drops the updater.
        Action::Close => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
        _ => {}
    }
    let rt = &app.updates_rt;
    let shows = result_window_shows(rt.updater.status(), rt.updater.prompt_open());
    if shows && !app.framework_dialogs.is_open::<UpdateCheckDialog>() {
        app.framework_dialogs.open(UpdateCheckDialog::new(VERSION));
    }
    let updater = &app.updates_rt.updater;
    let (status, installing, message) = (updater.status().clone(), updater.is_installing(), updater.status_message());
    if let Some(dialog) = app.framework_dialogs.get_mut::<UpdateCheckDialog>() {
        dialog.set_status(status);
        dialog.set_installing(installing);
        dialog.set_message(message);
    }
    // The prompt taking over dismisses the outcome the dialog showed.
    if !shows && app.framework_dialogs.is_open::<UpdateCheckDialog>() {
        app.framework_dialogs.cancel::<UpdateCheckDialog>(ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn result_window_shows_every_status_but_idle() {
        let shown = [
            Status::Checking,
            Status::UpToDate,
            Status::Failed("no network".into()),
            Status::Installing,
            Status::Ready,
            Status::Managed,
            Status::Updated("0.2.0".into()),
            Status::InstallFailed("disk full".into()),
        ];
        for status in &shown {
            assert!(result_window_shows(status, false), "{status:?}");
            assert!(!result_window_shows(status, true), "{status:?} over the prompt");
        }
        assert!(!result_window_shows(&Status::Idle, false));
    }

    /// The startup check runs only where the host enabled it and the user did not turn it off;
    /// the choice is kept in the profile and set through the OPTIONS JSON form.
    #[test]
    fn the_startup_check_can_be_turned_off() {
        let mut app = CadApp::new(cadcraft_engine::Session::new(), crate::Services::default());
        assert!(!checks_at_start(&app), "tests and the web never check");
        app.enable_update_check();
        assert!(checks_at_start(&app), "on by default");
        let r = app.run("options", serde_json::json!({ "checkUpdatesAtStart": false })).unwrap();
        assert_eq!(r["checkUpdatesAtStart"], false);
        assert!(!checks_at_start(&app));
        assert!(app.run("options", serde_json::json!({ "checkUpdatesAtStart": "no" })).is_err());
        // Kept across restarts.
        let profile = app.profile_json();
        let mut next = CadApp::new(cadcraft_engine::Session::new(), crate::Services::default());
        next.enable_update_check();
        next.load_profile(&profile);
        assert!(!checks_at_start(&next));
        // Reset to Defaults turns it back on.
        app.run("options", serde_json::json!({ "reset": true })).unwrap();
        assert!(checks_at_start(&app));
    }
}
