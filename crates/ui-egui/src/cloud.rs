//! CADCraft's half of Open, Save and Share against cloud storage.
//!
//! The pages themselves, the account badge, the share dialog and the transport contract are
//! `craft_ui::cloud`, `craft_ui::share` and `craft_ui::presence` (`storytold/craft-libs#160`,
//! `#665`). Every Craft app wires the same contract; what is here is what only CADCraft can
//! answer: a drawing is a DXF, DWG or SVG file, bytes become a drawing through the `open`
//! command, and an open drawing becomes bytes through `cadcraft_io::write`.
//!
//! CADCraft holds no provider credential and speaks no provider protocol. Nothing in this crate,
//! in `apps/cadcraft` or in `apps/cadcraft-web` connects a [`craft_ui::cloud::CloudBackend`] or a
//! [`craft_ui::share::ShareBackend`]: a build of this repository today has no backend wired, so
//! [`available`] is always false and nothing cloud-shaped is drawn or offered anywhere. A host
//! application connects one of its own, through [`craft_ui::cloud::CloudFiles::connect`] and
//! [`craft_ui::share::ShareDialog::connect`], the same way `crates/cloud-local` (this workspace's
//! tests) does.
//!
//! Nothing in this file opens a socket. Every call a provider needs is blocking, so the pages
//! state a [`craft_ui::cloud::Job`], a host's worker runs it, and the answer arrives on a channel
//! that [`craft_ui::cloud::pump`] drains on a later frame ([`CadApp::draw`]).

use std::collections::HashMap;
use std::sync::Arc;

use cadcraft_doc::Drawing;
pub use craft_ui::cloud::{CloudApp, CloudFiles, Destination, DocumentKind, UNAVAILABLE, available, cancel_save_as, opens as cloud_opens};
use egui::{Color32, Painter, Rect, Ui};
use serde_json::json;

use crate::CadApp;
use crate::theme::Tokens;

/// Extensions CADCraft opens from the cloud. The same set the desktop file picker offers.
const OPENS: [&str; 4] = ["dwg", "dxf", "dwt", "svg"];

/// Content types CADCraft opens, matched against the start of what the provider reported. Read
/// beside the extension because neither is reliable alone: a WebDAV server often answers
/// `application/octet-stream` for a perfectly good drawing, and a file with no extension may
/// still carry a correct content type.
const MIMES: [&str; 3] = ["application/dxf", "image/vnd.dxf", "image/svg+xml"];

/// What a CADCraft drawing is, as a cloud file list needs to know.
pub const DRAWING: DocumentKind = DocumentKind { extensions: &OPENS, mimes: &MIMES, noun: "drawing", default_extension: "dxf" };

/// The drawing snapshot each cloud save in flight encoded, by drawing uid.
///
/// Only CADCraft knows what one version of a drawing is; the pages hold none of this. A save
/// that lands clears the dirty flag for exactly the version that reached the service
/// ([`CloudApp::mark_saved`]). An edit made while the bytes were on the wire stays unsaved.
pub type Sent = HashMap<u64, Arc<Drawing>>;

/// Which cloud page is open as a modal over the drawing canvas.
///
/// Used when a drawing is already open, so the Start tab (and its own [`open_section`]) is not
/// on screen. The File menu's `cloud.open` and `cloud.saveAs` commands set this.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    /// The account's own files and what other people shared.
    Open,
    /// A folder and a name to save under.
    Save,
}

/// Whether CADCraft can open this cloud entry.
pub fn opens(entry: &craft_cloud::Entry) -> bool {
    cloud_opens(entry, DRAWING)
}

/// Begin an ordinary Save As to the cloud: a fresh destination, not the conflict modal's "Save
/// as a copy" (which sets its own target through [`CloudApp::go_to_save_as`] and must keep it).
/// Clears whatever an abandoned earlier Save As left behind first, so it cannot carry into this
/// one ([`cancel_save_as`]).
fn begin_save_as(app: &mut CadApp) {
    cancel_save_as(app);
    app.cloud_page = Some(Page::Save);
}

/// Routes the cloud UI commands. `None` when `id` isn't one.
pub fn route(app: &mut CadApp, id: &str, _params: &serde_json::Value) -> Option<Result<serde_json::Value, String>> {
    match id {
        "cloud.open" if !available(app) => Some(Err(UNAVAILABLE.to_owned())),
        "cloud.open" => {
            // Leaving the Save As page, if it was open, for one that is not saving: an abandoned
            // conflict-copy target or typed name must not carry into whatever opens next.
            if app.cloud_page == Some(Page::Save) {
                cancel_save_as(app);
            }
            app.cloud_page = Some(Page::Open);
            Some(Ok(serde_json::Value::Null))
        }
        "cloud.saveAs" if !available(app) => Some(Err(UNAVAILABLE.to_owned())),
        "cloud.saveAs" => {
            begin_save_as(app);
            Some(Ok(serde_json::Value::Null))
        }
        _ => None,
    }
}

/// Output file name for a cloud save: the destination's own name when it has one, or `name` with
/// the user's preferred format appended.
fn output_name(name: &str, ext: &str, save_format: &str) -> String {
    if ext.is_empty() { format!("{}.{}", name.trim_end_matches('.'), save_format) } else { name.to_owned() }
}

impl CloudApp for CadApp {
    fn cloud(&self) -> &CloudFiles {
        &self.cloud
    }

    fn cloud_mut(&mut self) -> &mut CloudFiles {
        &mut self.cloud
    }

    fn kind(&self) -> DocumentKind {
        DRAWING
    }

    fn active(&self) -> Option<u64> {
        // The Start tab shows no drawing, even with one open behind it.
        if self.ui.start_tab || self.session.docs.is_empty() {
            return None;
        }
        self.session.docs.get(self.session.active).map(|d| d.uid)
    }

    fn title(&self, uid: u64) -> String {
        self.session.docs.iter().find(|d| d.uid == uid).map(|d| d.title.clone()).unwrap_or_default()
    }

    fn open_bytes(&mut self, name: &str, bytes: Vec<u8>) -> Result<Option<u64>, String> {
        if let Some(e) = CadApp::open_size_error(name, bytes.len() as u64) {
            return Err(e);
        }
        let data = cadcraft_engine::cmd::file::base64_encode(&bytes);
        self.run("open", json!({ "data": data, "name": name }))?;
        self.cloud_page = None;
        self.ui.start_tab = false;
        self.canvas.zoom_pending = true;
        Ok(self.session.docs.get(self.session.active).map(|d| d.uid))
    }

    fn encode(&mut self, uid: u64, into: Destination<'_>) -> Result<Vec<u8>, String> {
        let doc = self.session.docs.iter().find(|d| d.uid == uid).ok_or("no drawing open")?;
        let snapshot = Arc::clone(&doc.doc);
        let ext = into.extension();
        // The destination's own format wins; a name with no extension goes out as the user's
        // preferred one (DXF or DWG), the same default Save As applies.
        let name = output_name(into.name, &ext, self.ui.save_format.as_str());
        let bytes = cadcraft_io::write(&snapshot, &name).map_err(|e| e.to_string())?;
        // Recorded before the bytes leave, so a conflicting or failed answer still knows which
        // version to drop ([`CloudApp::drop_pending`]) and a landed one knows which to mark saved
        // ([`CloudApp::mark_saved`]), rather than whatever the drawing has become by then.
        self.cloud_sent.insert(uid, snapshot);
        Ok(bytes)
    }

    fn close(&mut self, uid: u64) {
        if let Some(index) = self.session.docs.iter().position(|d| d.uid == uid) {
            // CLOSE cancels the running command and keeps the active index valid; safer than a
            // bare `Vec::remove`.
            let _ = self.run("close", json!({ "index": index }));
        }
    }

    fn mark_saved(&mut self, uid: u64) {
        // The exact version that reached the service clears the dirty flag. An edit made while
        // the bytes were on the wire left `doc.doc` pointing elsewhere by now, and stays unsaved.
        let Some(snapshot) = self.cloud_sent.remove(&uid) else { return };
        if let Some(doc) = self.session.docs.iter_mut().find(|d| d.uid == uid) {
            doc.saved = snapshot;
        }
    }

    fn drop_pending(&mut self, uid: u64) {
        self.cloud_sent.remove(&uid);
    }

    fn sharing(&mut self) -> &mut craft_ui::share::ShareDialog {
        &mut self.share
    }

    fn presence(&mut self) -> &mut craft_ui::presence::PresenceBar {
        &mut self.presence
    }

    fn paint_icon(&self, painter: &Painter, rect: Rect, _kind: craft_cloud::EntryKind, _colour: Color32) {
        // CADCraft's icon set draws from a typed enum, not a per-call colour, and has no
        // dedicated document icon; Open (a folder with an arrow) stands for both files and
        // folders in the cloud file list.
        crate::icons::paint(painter, rect, crate::icons::Icon::Open, false);
    }

    fn open_url(&self, url: &str) {
        if let Some(ctx) = &self.egui_ctx {
            ctx.open_url(egui::OpenUrl::new_tab(url));
        }
    }

    fn say(&mut self, text: String) {
        self.set_status(text.clone());
        self.session.echo(text);
    }

    fn toast(&mut self, text: String) {
        self.set_status(text);
    }

    fn go_to_save_as(&mut self) {
        // The conflict modal already set its own `save_as_target`; only the page changes here.
        self.cloud_page = Some(Page::Save);
    }

    fn app_name(&self) -> &str {
        "CADCraft"
    }

    fn tr(&self, text: &str) -> String {
        crate::i18n::t(text).to_owned()
    }

    fn save_note(&self) -> Option<String> {
        Some(self.tr("The drawing is saved in the format its name or the service already gave it, or your preferred format otherwise."))
    }

    fn style(&self, _ui: &Ui) -> craft_ui::cloud::CloudStyle {
        let t = Tokens::get();
        craft_ui::cloud::CloudStyle {
            surface: t.panel,
            border: t.border,
            text: t.text,
            dim: t.text_dim,
            hover: t.hover,
            soft: t.select_bg,
            family: egui::FontFamily::Proportional,
            bold: egui::FontFamily::Proportional,
        }
    }
}

// ---------------------------------------------------------------- where the pages are drawn

/// The Cloud section of the Start tab (the Open page), beside the local picker and never in
/// place of it. Draws nothing without a backend connected: an account badge over an empty list
/// reads to a user as a broken window.
pub fn open_section(app: &mut CadApp, ui: &mut Ui) {
    if !available(app) {
        return;
    }
    let t = Tokens::get();
    ui.add_space(22.0);
    ui.label(egui::RichText::new(crate::i18n::t("Cloud")).font(egui::FontId::proportional(15.0)).color(t.text));
    ui.add_space(8.0);
    craft_ui::cloud::open_page(app, ui);
}

/// The Open and Save As pages as a modal over the drawing canvas, for `cloud.open` and
/// `cloud.saveAs` while a drawing is open (the Start tab, and [`open_section`] with it, is not
/// visible then).
pub fn modal(app: &mut CadApp, ctx: &egui::Context) {
    let Some(page) = app.cloud_page else { return };
    let title = match page {
        Page::Open => crate::i18n::t("Open from the Cloud"),
        Page::Save => crate::i18n::t("Save to the Cloud"),
    };
    // Kept apart: a save that just started closes the modal too, but must not cancel the Save
    // As interaction it just submitted (`cancel_save_as` is for leaving *without* saving).
    let mut saved = false;
    let mut dismissed = false;
    egui::Modal::new(egui::Id::unique("cloud_page")).show(ctx, |ui| {
        let margin = egui::Frame::popup(ui.style()).total_margin().sum();
        let room = (ctx.content_rect().size() - margin - egui::vec2(16.0, 16.0)).max(egui::Vec2::splat(1.0));
        let width = 680.0_f32.min(room.x);
        ui.set_min_width(380.0_f32.min(width));
        ui.set_max_width(width);
        ui.label(egui::RichText::new(title).font(egui::FontId::proportional(16.0)).strong());
        ui.add_space(10.0);
        let body = (room.y - ui.min_size().y - 48.0).max(1.0);
        egui::ScrollArea::vertical().id_salt(("cloud_page", page == Page::Save)).max_height(body).show(ui, |ui| match page {
            Page::Open => craft_ui::cloud::open_page(app, ui),
            Page::Save => saved = craft_ui::cloud::save_page(app, ui),
        });
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(crate::i18n::t("Close")).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    dismissed = true;
                }
            });
        });
    });
    if saved || dismissed {
        if dismissed && page == Page::Save {
            // Leaving the Save As page without having saved: an abandoned conflict-copy target
            // or typed name must not carry into whatever opens the page next.
            cancel_save_as(app);
        }
        app.cloud_page = None;
    }
}
