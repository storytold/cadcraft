//! Asking before a save throws content away. DWG can't yet hold everything a drawing read from
//! another program keeps (`cadcraft_engine::cmd::file::not_saved`), and QSAVE replaces the file
//! that still has it, so File ▸ Save, its shortcut or typing QSAVE says what would go first:
//! Save Anyway / Save As... / Cancel. Agreeing lasts for that same loss on that same file, so
//! later saves in the session don't ask again; the file no longer holds what was agreed to.
//!
//! Programmatic saves (`engine.execute`, the control channel, MCP, scripts) never ask: they keep
//! reporting the loss in the result's `notSaved` and on the command line.

use std::collections::BTreeMap;

use egui::{RichText, vec2};
use serde_json::{Value, json};

use cadcraft_engine::autosave::isavebak;
use cadcraft_engine::cmd::file::{backup_path, loss_on_save, save_keeps_current};

use crate::CadApp;
use crate::theme::Tokens;

/// How many object types the warning lists; a file can name as many as it likes.
const MAX_LISTED: usize = 8;
/// How many characters of a type name the warning shows; a file chooses these too.
const MAX_NAME_CHARS: usize = 40;

/// The user's answer to the warning.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    SaveAnyway,
    SaveAs,
    Cancel,
}

/// A loss the user agreed to: this drawing, this file, these objects.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Accepted {
    uid: u64,
    path: String,
    lost: BTreeMap<String, usize>,
}

/// A save held back until the user answers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pending {
    loss: Accepted,
}

impl Pending {
    /// The file the save would replace.
    pub fn path(&self) -> &str {
        &self.loss.path
    }

    /// The objects the file wouldn't keep, as "2 MLINE, 1 3DSOLID".
    pub fn summary(&self) -> String {
        let lost = &self.loss.lost;
        let name = |t: &str| t.chars().take(MAX_NAME_CHARS).collect::<String>();
        let mut parts: Vec<String> = lost.iter().take(MAX_LISTED).map(|(t, n)| format!("{n} {}", name(t))).collect();
        if lost.len() > MAX_LISTED {
            parts.push(crate::tf!("{count} more types", count = lost.len() - MAX_LISTED));
        }
        parts.join(", ")
    }
}

impl CadApp {
    /// Hold a user's QSAVE of the active drawing if it would lose content the user hasn't agreed
    /// to lose, and ask. Returns whether it was held.
    pub fn hold_lossy_save(&mut self) -> bool {
        let Ok(st) = self.session.state() else { return false };
        let (uid, Some(path)) = (st.uid, st.path.clone()) else { return false };
        let lost = loss_on_save(&self.session, &path);
        if lost.is_empty() {
            return false;
        }
        let loss = Accepted { uid, path, lost };
        if self.save_loss_accepted.as_ref() == Some(&loss) {
            return false;
        }
        self.save_loss = Some(Pending { loss });
        true
    }

    /// Answer the open warning (no-op without one).
    pub fn answer_save_loss(&mut self, choice: Choice) {
        let Some(Pending { loss }) = self.save_loss.take() else { return };
        // The drawing can go away, or stop being the active one, while the warning is up.
        let i = self.session.docs.iter().position(|d| d.uid == loss.uid);
        match (choice, i) {
            (Choice::Cancel, _) | (_, None) => self.session.echo(crate::tf!("Save cancelled: {path} was not changed.", path = loss.path)),
            (Choice::SaveAs, Some(i)) => {
                self.activate_doc(i);
                let _ = self.run("ui.saveas", Value::Null);
            }
            (Choice::SaveAnyway, Some(i)) => {
                self.activate_doc(i);
                self.save_loss_accepted = Some(loss);
                let _ = self.run("qsave", json!({}));
            }
        }
    }

    fn activate_doc(&mut self, i: usize) {
        if self.session.active != i {
            let _ = self.run("document.switch", json!({ "index": i }));
        }
    }
}

/// The warning, while a save waits for an answer. Cancel is what Escape does.
pub fn prompt(app: &mut CadApp, ctx: &egui::Context) {
    let Some(pending) = app.save_loss.clone() else { return };
    let t = Tokens::get();
    let path = std::path::Path::new(pending.path());
    let file = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| pending.path().to_string());
    let backup = backup_path(path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let mut choice = None;
    crate::modal::instant(ctx, egui::Id::unique("cc_save_loss_prompt")).show(ctx, |ui| {
        ui.set_width(420.0);
        ui.label(RichText::new(crate::tf!("Saving {name} will remove content", name = file)).strong());
        ui.add_space(4.0);
        ui.label(crate::tf!("This format can't keep these objects yet: {objects}.", objects = pending.summary()));
        ui.label(RichText::new(crate::tl!("DXF keeps them. Use Save As to keep them in the drawing.")).color(t.text_dim));
        let on_disk = if save_keeps_current(path) {
            crate::tf!("The current file is kept as {name} until the next save.", name = backup)
        } else if isavebak() {
            crate::tf!("The current file doesn't look whole, so {name} stays as it is and the current file will be replaced.", name = backup)
        } else {
            crate::tl!("Backup copies are off, so the current file will be replaced.").to_string()
        };
        ui.label(RichText::new(on_disk).color(t.text_dim));
        ui.add_space(10.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let size = vec2(96.0, 22.0);
            let save_as = ui.add(egui::Button::new(RichText::new(crate::tl!("Save As...")).color(t.text)).fill(t.accent).min_size(size));
            if save_as.clicked() {
                choice = Some(Choice::SaveAs);
            }
            if ui.add(egui::Button::new(crate::tl!("Cancel")).min_size(size)).clicked() {
                choice = Some(Choice::Cancel);
            }
            if ui.add(egui::Button::new(crate::tl!("Save Anyway")).min_size(size)).clicked() {
                choice = Some(Choice::SaveAnyway);
            }
        });
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            choice = Some(Choice::Cancel);
        }
    });
    if let Some(c) = choice {
        app.answer_save_loss(c);
        ctx.request_repaint();
    }
}
