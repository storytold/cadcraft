//! Closing drawings from the UI: File ▸ Close (or typing CLOSE), a file tab's ×, and
//! File ▸ Close All.
//!
//! A drawing with unsaved changes is not closed until the user answers "Save changes to …?"
//! (Save / Don't Save / Cancel) in an in-app modal — the same on desktop and on the web, where no
//! native message box exists. Close All works through the drawings in tab order and asks about
//! each dirty one; Cancel stops it there (drawings already closed stay closed). Save closes the
//! drawing only if it really was saved.
//!
//! Programmatic `close` / `closeall` (`engine.execute`, the control channel, MCP, scripts) never
//! ask: they keep their discard semantics.

use std::collections::VecDeque;

use egui::{RichText, vec2};
use serde_json::{Value, json};

use crate::CadApp;
use crate::theme::Tokens;

/// The user's answer to "Save changes to …?".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    Save,
    Discard,
    Cancel,
}

/// A close request in progress.
#[derive(Clone, Debug, Default)]
pub struct Closing {
    /// Drawings (by [`cadcraft_engine::DocState::uid`]) still to close, front first.
    queue: VecDeque<u64>,
    /// The drawing the prompt is asking about (always the front of `queue`).
    asking: Option<u64>,
}

impl CadApp {
    /// Close one drawing (`None`: the active one) as the user asked for it: a drawing with unsaved
    /// changes asks Save / Don't Save / Cancel first.
    pub fn request_close(&mut self, index: Option<usize>) {
        let i = index.unwrap_or(self.session.active);
        match self.session.docs.get(i) {
            Some(d) => {
                let uid = d.uid;
                self.begin_close(vec![uid]);
            }
            None => self.session.echo("Close: no drawing to close"),
        }
    }

    /// Close every drawing as the user asked for it, asking about each one with unsaved changes.
    pub fn request_close_all(&mut self) {
        let uids = self.session.docs.iter().map(|d| d.uid).collect();
        self.begin_close(uids);
    }

    /// Title of the drawing the close prompt is asking about, if a prompt is open.
    pub fn close_prompt(&self) -> Option<&str> {
        let uid = self.closing.as_ref()?.asking?;
        self.session.docs.iter().find(|d| d.uid == uid).map(|d| d.title.as_str())
    }

    /// Answer the open close prompt (no-op without one).
    pub fn answer_close(&mut self, choice: Choice) {
        let Some(uid) = self.closing.as_ref().and_then(|c| c.asking) else { return };
        let i = match (choice, self.doc_index(uid)) {
            (Choice::Cancel, _) => {
                self.closing = None;
                return;
            }
            (_, None) => {
                // The drawing went away meanwhile (e.g. closed through the control channel).
                self.advance_close();
                return;
            }
            (_, Some(i)) => i,
        };
        match choice {
            Choice::Cancel => {} // handled above
            Choice::Discard => {
                self.close_front(i);
                self.advance_close();
            }
            Choice::Save => {
                // QSAVE acts on the active drawing.
                if self.session.active != i {
                    let _ = self.run("document.switch", json!({ "index": i }));
                }
                let r = self.run("qsave", Value::Null);
                let saved = r.is_ok() && self.doc_index(uid).and_then(|i| self.session.docs.get(i)).is_some_and(|d| !d.is_dirty());
                match (saved, self.doc_index(uid)) {
                    (true, Some(i)) => {
                        self.close_front(i);
                        self.advance_close();
                    }
                    _ => {
                        // Not saved (picker cancelled, save failed or unavailable): keep the
                        // drawing and its edits, and stop closing.
                        self.closing = None;
                        let title = self.doc_index(uid).and_then(|i| self.session.docs.get(i)).map(|d| d.title.clone()).unwrap_or_default();
                        let msg = match r {
                            Err(e) => format!("{title} was not saved ({e}); it stays open."),
                            Ok(_) => format!("{title} was not saved; it stays open."),
                        };
                        self.session.echo(msg.clone());
                        self.set_status(msg);
                    }
                }
            }
        }
    }

    fn begin_close(&mut self, uids: Vec<u64>) {
        if self.closing.is_some() {
            // A prompt is already open; it has to be answered first.
            return;
        }
        self.closing = Some(Closing { queue: uids.into(), asking: None });
        self.advance_close();
    }

    fn doc_index(&self, uid: u64) -> Option<usize> {
        self.session.docs.iter().position(|d| d.uid == uid)
    }

    /// Close the drawing at the front of the queue (at index `i`) without asking.
    fn close_front(&mut self, i: usize) {
        if let Some(c) = &mut self.closing {
            c.queue.pop_front();
            c.asking = None;
        }
        let _ = self.run("close", json!({ "index": i }));
    }

    /// Close queued drawings until one has unsaved changes (then ask) or the queue is empty.
    fn advance_close(&mut self) {
        loop {
            let Some(uid) = self.closing.as_ref().and_then(|c| c.queue.front().copied()) else {
                self.closing = None;
                return;
            };
            let Some(i) = self.doc_index(uid) else {
                if let Some(c) = &mut self.closing {
                    c.queue.pop_front();
                }
                continue;
            };
            if self.session.docs.get(i).is_some_and(|d| d.is_dirty()) {
                // Show the drawing being asked about.
                if self.session.active != i {
                    let _ = self.run("document.switch", json!({ "index": i }));
                }
                self.ui.start_tab = false;
                if let Some(c) = &mut self.closing {
                    c.asking = Some(uid);
                }
                return;
            }
            self.close_front(i);
        }
    }
}

/// The "Save changes to …?" modal while a close request waits for an answer.
pub fn prompt(app: &mut CadApp, ctx: &egui::Context) {
    if app.closing.is_none() {
        return;
    }
    let Some(title) = app.close_prompt().map(str::to_string) else {
        app.advance_close();
        return;
    };
    let t = Tokens::get();
    let mut choice = None;
    egui::Modal::new(egui::Id::new("cc_close_prompt")).show(ctx, |ui| {
        ui.set_width(360.0);
        ui.label(RichText::new("CADCraft").strong());
        ui.add_space(4.0);
        ui.label(format!("Save changes to {title}?"));
        ui.label(RichText::new("Your changes will be lost if you don't save them.").color(t.text_dim));
        ui.add_space(10.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let size = vec2(84.0, 22.0);
            let save = ui.add(egui::Button::new(RichText::new("Save").color(t.text)).fill(t.accent).min_size(size));
            if save.clicked() {
                choice = Some(Choice::Save);
            }
            if ui.add(egui::Button::new("Cancel").min_size(size)).clicked() {
                choice = Some(Choice::Cancel);
            }
            if ui.add(egui::Button::new("Don't Save").min_size(size)).clicked() {
                choice = Some(Choice::Discard);
            }
        });
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            choice = Some(Choice::Cancel);
        }
    });
    if let Some(c) = choice {
        app.answer_close(c);
        ctx.request_repaint();
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use cadcraft_engine::Session;
    use serde_json::json;

    use super::Choice;
    use crate::{CadApp, Services};

    fn app_with(titles: &[&str]) -> CadApp {
        let mut app = CadApp::new(Session::empty(), Services::default());
        for _ in titles {
            app.session.new_drawing(false);
        }
        for (d, t) in app.session.docs.iter_mut().zip(titles) {
            d.title = (*t).to_string();
        }
        app
    }

    fn dirty(app: &mut CadApp, i: usize) {
        app.session.execute("document.switch", &json!({ "index": i })).unwrap();
        app.session.execute("circle", &json!({ "center": [0, 0], "radius": 1 })).unwrap();
        assert!(app.session.docs[i].is_dirty());
    }

    fn titles(app: &CadApp) -> Vec<String> {
        app.session.docs.iter().map(|d| d.title.clone()).collect()
    }

    #[test]
    fn clean_drawing_closes_without_prompt() {
        let mut app = app_with(&["A", "B"]);
        app.request_close(Some(0));
        assert!(app.closing.is_none());
        assert_eq!(titles(&app), ["B"]);
    }

    #[test]
    fn dirty_drawing_prompts_and_cancel_keeps_it() {
        let mut app = app_with(&["A", "B"]);
        dirty(&mut app, 1);
        crate::menus::activate(&mut app, "close"); // File > Close
        assert_eq!(app.close_prompt(), Some("B"));
        assert_eq!(titles(&app), ["A", "B"]);
        app.answer_close(Choice::Cancel);
        assert!(app.closing.is_none());
        assert_eq!(titles(&app), ["A", "B"]);
        assert!(app.session.docs[1].is_dirty(), "edits survive Cancel");
    }

    #[test]
    fn discard_closes_the_dirty_drawing() {
        let mut app = app_with(&["A", "B"]);
        dirty(&mut app, 0);
        app.request_close(Some(0)); // tab ×
        assert_eq!(app.close_prompt(), Some("A"));
        app.answer_close(Choice::Discard);
        assert!(app.closing.is_none());
        assert_eq!(titles(&app), ["B"]);
    }

    #[test]
    fn unsuccessful_save_keeps_the_drawing() {
        // Untitled drawing and no save picker (the web today): nothing gets saved.
        let mut app = app_with(&["A"]);
        dirty(&mut app, 0);
        app.request_close(None);
        app.answer_close(Choice::Save);
        assert!(app.closing.is_none());
        assert_eq!(titles(&app), ["A"]);
        assert!(app.session.docs[0].is_dirty());
    }

    #[test]
    fn close_all_asks_per_dirty_drawing_and_cancel_stops() {
        let mut app = app_with(&["A", "B", "C", "D"]);
        dirty(&mut app, 1);
        dirty(&mut app, 3);
        app.cmdline("CLOSEALL");
        // A is clean and closes; B asks.
        assert_eq!(app.close_prompt(), Some("B"));
        assert_eq!(titles(&app), ["B", "C", "D"]);
        app.answer_close(Choice::Discard);
        // C is clean and closes; D asks.
        assert_eq!(app.close_prompt(), Some("D"));
        app.answer_close(Choice::Cancel);
        assert!(app.closing.is_none());
        assert_eq!(titles(&app), ["D"]);
        assert!(app.session.docs[0].is_dirty());
    }

    #[test]
    fn programmatic_close_never_prompts() {
        let mut app = app_with(&["A", "B", "C"]);
        dirty(&mut app, 0);
        dirty(&mut app, 1);
        // engine.execute / control channel: JSON parameters run the command as before (discard).
        app.run("close", json!({ "index": 0 })).unwrap();
        assert!(app.closing.is_none());
        assert_eq!(titles(&app), ["B", "C"]);
        app.run("closeall", json!({})).unwrap();
        assert!(app.closing.is_none());
        assert!(app.session.docs.is_empty());
    }
}
