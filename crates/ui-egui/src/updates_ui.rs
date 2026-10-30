//! One background update check at startup, the "Update and Restart" prompt, and Help ▸ Check for
//! Updates… (desktop only; the web build is always the latest). The flow itself is
//! `craft_update::updater::Updater`.

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
}

impl Default for Runtime {
    fn default() -> Self {
        Self { updater: Updater::new(&crate::UPDATE_APP, VERSION), enabled: false, check_requested: false }
    }
}

impl CadApp {
    /// Turn on the startup update check (the desktop host does; tests and the web build do not).
    pub fn enable_update_check(&mut self) {
        self.updates_rt.enabled = true;
    }
}

/// Help ▸ Check for Updates…: check now, and say so when there is nothing newer.
pub fn check_now(app: &mut CadApp) {
    app.updates_rt.check_requested = true;
}

/// Start the checks and collect the answers.
pub fn tick(app: &mut CadApp, ctx: &egui::Context) {
    let rt = &mut app.updates_rt;
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
    if let Some(message) = outcome {
        rt.updater.dismiss_status();
        app.session.echo(message);
    }
}

/// Draw the prompt while there is one and act on its answer.
pub fn show(app: &mut CadApp, ctx: &egui::Context) {
    // The restart helper is scheduled before the app closes, so no update starts over unsaved work.
    let clean = !app.session.docs.iter().any(|d| d.is_dirty());
    match app.updates_rt.updater.show_if(ctx, clean) {
        Action::OpenUrl(url) => ctx.open_url(egui::OpenUrl::new_tab(url)),
        // The guarded close path asks about unsaved changes; the restart helper relaunches once the
        // process has exited.
        Action::Close => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
        Action::None => {}
    }
}
