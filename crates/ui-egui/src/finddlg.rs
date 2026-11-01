//! Find and Replace: the window behind the FIND command.
//!
//! FIND used to be a command line that asked for its parameters and then printed a count. The
//! parameters are still what the engine takes; this window is the surface where a user can see
//! *what* matched, jump to it and replace one object at a time. It stays open while the drawing
//! is edited — find-and-replace in a drawing is a loop (look, locate, edit, look again), so a
//! window that closed on the first result would be closed and reopened all afternoon.
//!
//! Everything the engine cannot answer is left out rather than faked: the search covers the
//! current space, and the note under the switches says so in as many words.

use egui::{Color32, RichText, Sense, TextFormat};
use serde_json::{Value, json};

use crate::CadApp;
use crate::theme::{self, Tokens};

/// One object the search hit, as the engine reports it.
#[derive(Clone, Default)]
struct Row {
    handle: String,
    kind: String,
    count: u64,
    /// The text around the first hit, and where the hit sits inside it (characters, not bytes).
    snippet: String,
    hit: usize,
    hit_len: usize,
}

/// A `u64` from the reply as a `usize`, or 0 when it does not fit: an offset past the end of a
/// snippet is a bad reply, not a reason to fail the search that produced it.
fn as_index(v: Option<&Value>) -> usize {
    v.and_then(Value::as_u64).and_then(|n| usize::try_from(n).ok()).unwrap_or(0)
}

impl Row {
    /// Read one `matches` entry; anything missing keeps its default so a partially understood
    /// reply still lists something.
    fn from_json(v: &Value) -> Self {
        Self {
            handle: v.get("handle").and_then(Value::as_str).unwrap_or_default().to_string(),
            kind: v.get("type").and_then(Value::as_str).unwrap_or_default().to_string(),
            count: v.get("count").and_then(Value::as_u64).unwrap_or(0),
            snippet: v.get("snippet").and_then(Value::as_str).unwrap_or_default().to_string(),
            hit: as_index(v.get("hit")),
            hit_len: as_index(v.get("hitLen")),
        }
    }

    /// The snippet in three pieces: before the hit, the hit, after it. Every slice is by
    /// character and every range is clamped, so a reply with a nonsensical offset still draws.
    fn pieces(&self) -> (String, String, String) {
        let n = self.snippet.chars().count();
        let at = self.hit.min(n);
        let end = at.saturating_add(self.hit_len).min(n);
        let before: String = self.snippet.chars().take(at).collect();
        let hit: String = self.snippet.chars().skip(at).take(end.saturating_sub(at)).collect();
        let after: String = self.snippet.chars().skip(end).collect();
        (before, hit, after)
    }
}

/// The window's state between frames: kept in egui's temporary store, like the other dialogs,
/// so it lives exactly as long as the window does and never reaches the preferences file.
#[derive(Default, Clone)]
struct State {
    find: String,
    replace: String,
    match_case: bool,
    whole_word: bool,
    /// The `doc_epoch` and space the rows were found in. A row's handle is only meaningful in
    /// the drawing and the space it was found in, so when either moves the rows must go.
    epoch: u64,
    space: cadcraft_doc::Space,
    rows: Vec<Row>,
    /// The row Enter and Replace act on.
    current: Option<usize>,
    message: String,
}

/// Run FIND and keep the parsed matches, or the engine's complaint in `message`.
fn search(app: &mut CadApp, st: &mut State) {
    if st.find.trim().is_empty() {
        st.rows.clear();
        st.current = None;
        st.message = crate::i18n::t("Type something to find").to_string();
        return;
    }
    let params = json!({ "find": st.find, "matchCase": st.match_case, "wholeWord": st.whole_word });
    match app.run("find", params) {
        Ok(v) => {
            st.rows = v.get("matches").and_then(Value::as_array).map(|a| a.iter().map(Row::from_json).collect()).unwrap_or_default();
            st.current = if st.rows.is_empty() { None } else { Some(0) };
            let total = v.get("total").and_then(Value::as_u64).unwrap_or(0);
            let objects = v.get("matches").and_then(Value::as_array).map_or(0, Vec::len);
            st.message = format!("{total} match(es) in {objects} object(s)");
        }
        Err(e) => {
            st.rows.clear();
            st.current = None;
            st.message = e;
        }
    }
}

/// Select `row` in the drawing and zoom to it: the result list's rows navigate, they do not just
/// report.
fn reveal(app: &mut CadApp, st: &State, index: usize) {
    let Some(row) = st.rows.get(index) else { return };
    let Some(h) = cadcraft_doc::Handle::parse_hex(&row.handle) else { return };
    app.session.set_selection(vec![h]);
    let _ = app.run("zoom", json!({ "mode": "object" }));
}

/// Replace inside the objects the result list already holds. `only` limits it to one row, so
/// Replace touches the object the user is looking at and Replace All touches the whole list.
fn replace(app: &mut CadApp, st: &mut State, only: Option<usize>) {
    if st.find.trim().is_empty() {
        return;
    }
    let handles: Vec<String> = match only {
        Some(i) => st.rows.get(i).map(|r| r.handle.clone()).into_iter().collect(),
        None => st.rows.iter().map(|r| r.handle.clone()).collect(),
    };
    if handles.is_empty() {
        return;
    }
    let params = json!({
        "find": st.find,
        "replace": st.replace,
        "matchCase": st.match_case,
        "wholeWord": st.whole_word,
        "handles": handles,
    });
    match app.run("find", params) {
        Ok(v) => {
            // The engine's reply reports what the replacement did; the re-search below would
            // overwrite that message with a match count, so the replacement report is what
            // the user ends up reading.
            let replaced = v.get("message").and_then(Value::as_str).unwrap_or_default().to_string();
            search(app, st);
            if !replaced.is_empty() {
                st.message = replaced;
            }
        }
        Err(e) => st.message = e,
    }
}

/// The Find and Replace window.
pub fn dialog(app: &mut CadApp, ctx: &egui::Context, open: &mut bool) {
    let t = Tokens::get();
    let sid = egui::Id::unique("find_state");
    let mut st: State = ctx.data_mut(|d| d.get_temp(sid)).unwrap_or_default();
    // A handle from an earlier drawing or space now names whatever object took its place; if
    // Replace All ran on such a row it would edit an object the search never matched. Clearing
    // the rows on an epoch or space change costs a re-click on Find All, which is the cheap
    // side of that trade.
    if *open && (st.epoch != app.session.doc_epoch || st.space != app.session.space()) {
        st.epoch = app.session.doc_epoch;
        st.space = app.session.space();
        st.rows.clear();
        st.current = None;
        st.message.clear();
    }
    // The rows answer one question — this text, with these switches, in this drawing. Editing
    // any of them makes the rows stale: Replace All would rewrite with the old switches while
    // the user is looking at the new ones, so the rows go and a fresh search is owed.
    let criteria = (st.find.clone(), st.match_case, st.whole_word);
    let mut do_search = false;
    let mut do_replace: Option<Option<usize>> = None;
    let mut do_select_all = false;
    let mut pick: Option<usize> = None;
    egui::Window::new(crate::tl!("Find and Replace"))
        .id(egui::Id::unique("Find and Replace"))
        .open(open)
        .collapsible(false)
        .resizable(true)
        .default_width(480.0)
        .default_height(400.0)
        .show(ctx, |ui| {
            egui::Grid::new("find_fields").num_columns(2).spacing(egui::vec2(10.0, 6.0)).show(ui, |ui| {
                ui.label(crate::tl!("Find what:"));
                let r = ui.add(egui::TextEdit::singleline(&mut st.find).desired_width(f32::INFINITY).hint_text(crate::tl!("Text to find")));
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    do_search = true;
                }
                ui.end_row();
                ui.label(crate::tl!("Replace with:"));
                ui.add(egui::TextEdit::singleline(&mut st.replace).desired_width(f32::INFINITY).hint_text(crate::tl!("Replacement text")));
                ui.end_row();
            });
            ui.horizontal(|ui| {
                ui.checkbox(&mut st.match_case, crate::tl!("Match case"));
                ui.checkbox(&mut st.whole_word, crate::tl!("Whole words"));
            });
            ui.label(RichText::new(crate::tl!("Searches the current space.")).color(t.text_faint).small());
            ui.add_space(2.0);
            ui.separator();
            let found = if st.rows.is_empty() { crate::tl!("Nothing found").to_string() } else { st.rows.len().to_string() };
            ui.label(RichText::new(format!("{} · {found}", crate::tl!("Results"))).color(t.text_dim).small());
            let body_h = (ui.available_height() - 76.0).clamp(80.0, 280.0);
            egui::Frame::NONE.fill(t.field).corner_radius(3).inner_margin(egui::Margin::same(2)).show(ui, |ui| {
                egui::ScrollArea::vertical().max_height(body_h).auto_shrink([false, false]).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    for (i, row) in st.rows.iter().enumerate() {
                        let bg = if st.current == Some(i) { t.select_bg } else { Color32::TRANSPARENT };
                        let inner = egui::Frame::NONE.fill(bg).corner_radius(2).inner_margin(egui::Margin::symmetric(6, 3)).show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(crate::i18n::t(&row.kind)).color(t.text_dim).small());
                                ui.label(RichText::new(row.handle.as_str()).color(t.text_faint).small().monospace());
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    ui.label(RichText::new(format!("×{}", row.count)).color(t.text_faint).small());
                                });
                            });
                            let (before, hit, after) = row.pieces();
                            let mut job = egui::text::LayoutJob::default();
                            let plain = TextFormat { font_id: theme::small(), color: t.text, ..Default::default() };
                            let marked = TextFormat { font_id: theme::mono(), color: accent(), ..Default::default() };
                            job.append(&before, 0.0, plain.clone());
                            job.append(&hit, 0.0, marked);
                            job.append(&after, 0.0, plain);
                            ui.add(egui::Label::new(job).wrap());
                        });
                        let click = ui.interact(inner.response.rect, ui.scope_id().with(("find_row", i)), Sense::click());
                        if click.clicked() {
                            st.current = Some(i);
                            pick = Some(i);
                        }
                    }
                });
            });
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if ui.button(crate::tl!("Find All")).clicked() {
                    do_search = true;
                }
                let has = !st.rows.is_empty();
                if ui.add_enabled(has, egui::Button::new(crate::tl!("Select All"))).clicked() {
                    do_select_all = true;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // An empty replacement is deleting the matched text, which is a legitimate
                    // replacement, so the buttons care only about having something to act on.
                    if ui.add_enabled(has, egui::Button::new(crate::tl!("Replace All"))).clicked() {
                        do_replace = Some(None);
                    }
                    if ui.add_enabled(st.current.is_some(), egui::Button::new(crate::tl!("Replace"))).clicked() {
                        do_replace = Some(st.current);
                    }
                });
            });
            if !st.message.is_empty() {
                ui.label(RichText::new(st.message.as_str()).color(t.text_faint).small());
            }
        });
    if let Some(i) = pick {
        reveal(app, &st, i);
    }
    if criteria != (st.find.clone(), st.match_case, st.whole_word) {
        st.rows.clear();
        st.current = None;
        if !do_search {
            st.message.clear();
        }
    }
    if do_search {
        search(app, &mut st);
    }
    if do_select_all {
        let hs: Vec<cadcraft_doc::Handle> = st.rows.iter().filter_map(|r| cadcraft_doc::Handle::parse_hex(&r.handle)).collect();
        if !hs.is_empty() {
            app.session.set_selection(hs);
            st.message = format!("{} object(s) selected", st.rows.len());
        }
    }
    if let Some(only) = do_replace {
        replace(app, &mut st, only);
    }
    // The state lives exactly as long as the window does: a closed window leaves nothing behind,
    // so reopening it reads the drawing instead of a stale result list.
    if *open {
        ctx.data_mut(|d| d.insert_temp(sid, st));
    } else {
        ctx.data_mut(|d| d.remove_temp::<State>(sid));
    }
}

/// The colour a hit is drawn in: the theme's point colour, the same orange the drawing uses for
/// points and grips, so the highlight belongs to the drawing rather than to the interface.
fn accent() -> Color32 {
    Tokens::get().icon_point
}
