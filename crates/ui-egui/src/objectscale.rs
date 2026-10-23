//! The Annotation Object Scale dialog (OBJECTSCALE, and the Properties palette's Annotative
//! scale row): the annotation scales the selected annotative objects support, with Add and
//! Delete. It never edits the drawing itself: OK runs one OBJECTSCALE call with what was added
//! and deleted.

use cadcraft_doc::{Drawing, Handle};
use serde_json::{Value, json};

use crate::CadApp;
use crate::theme::Tokens;

/// The dialog while open.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ObjectScaleDlg {
    /// The annotative objects it changes.
    pub handles: Vec<Handle>,
    /// "List scales common to all selected objects only" instead of every scale any supports.
    pub common_only: bool,
    /// Scales added and deleted so far (names of the drawing's scale list).
    pub added: Vec<String>,
    pub deleted: Vec<String>,
    /// The scale picked in the list.
    pub selected: Option<String>,
    /// The Add Scales to Object window while open: the scales ticked in it.
    pub adding: Option<Vec<String>>,
    /// The drawing it belongs to (`DocState::uid`).
    pub uid: u64,
    /// The last command error, shown in the dialog.
    pub error: Option<String>,
}

fn same(a: &str, b: &str) -> bool {
    a.trim().eq_ignore_ascii_case(b.trim())
}

/// The scales each of the dialog's objects supports now, before the dialog's changes.
fn scales_of(d: &Drawing, s: &ObjectScaleDlg) -> Vec<Vec<String>> {
    s.handles.iter().filter_map(|h| d.entity(*h)).map(|e| e.common.anno_scales.iter().map(|x| x.name.clone()).collect()).collect()
}

/// The scales one object would support with the dialog's changes: its own, deleted ones left out,
/// added ones in.
fn after(own: &[String], s: &ObjectScaleDlg) -> Vec<String> {
    let mut out: Vec<String> = own.iter().filter(|n| !s.deleted.iter().any(|x| same(x, n))).cloned().collect();
    for a in &s.added {
        if !out.iter().any(|n| same(n, a)) {
            out.push(a.clone());
        }
    }
    out
}

/// The Object Scale List: every scale an object supports (or only those every object
/// supports), with the dialog's changes, in the order of the drawing's scale list.
pub fn listed(d: &Drawing, s: &ObjectScaleDlg) -> Vec<String> {
    let each: Vec<Vec<String>> = scales_of(d, s).iter().map(|own| after(own, s)).collect();
    let keep = |n: &str| {
        if s.common_only { each.iter().all(|l| l.iter().any(|x| same(x, n))) } else { each.iter().any(|l| l.iter().any(|x| same(x, n))) }
    };
    let mut out: Vec<String> = d.scales.iter().map(|x| x.name.clone()).filter(|n| keep(n)).collect();
    // Scales an object supports that the list lacks come last.
    for l in &each {
        for n in l {
            if keep(n) && !out.iter().any(|x| same(x, n)) {
                out.push(n.clone());
            }
        }
    }
    out
}

/// Whether `name` can be deleted: every object keeps at least one scale.
pub fn can_delete(d: &Drawing, s: &ObjectScaleDlg, name: &str) -> bool {
    scales_of(d, s).iter().all(|own| after(own, s).iter().any(|n| !same(n, name)))
}

/// Delete `name` (an added one is just taken back).
pub fn delete(s: &mut ObjectScaleDlg, name: &str) {
    let before = s.added.len();
    s.added.retain(|n| !same(n, name));
    if s.added.len() == before && !s.deleted.iter().any(|n| same(n, name)) {
        s.deleted.push(name.to_string());
    }
    s.selected = None;
}

/// Add the scales `names` (a deleted one is just taken back).
pub fn add(s: &mut ObjectScaleDlg, names: &[String]) {
    for n in names {
        let before = s.deleted.len();
        s.deleted.retain(|x| !same(x, n));
        if s.deleted.len() == before && !s.added.iter().any(|x| same(x, n)) {
            s.added.push(n.clone());
        }
    }
}

/// The scales of the drawing's list the Add window offers: those not listed.
pub fn addable(d: &Drawing, s: &ObjectScaleDlg) -> Vec<String> {
    let shown = listed(d, &ObjectScaleDlg { common_only: false, ..s.clone() });
    d.scales.iter().filter(|x| x.is_valid() && !shown.iter().any(|n| same(n, &x.name))).map(|x| x.name.clone()).collect()
}

/// The OBJECTSCALE call OK runs; `None` when nothing changed.
pub fn params(s: &ObjectScaleDlg) -> Option<Value> {
    if s.added.is_empty() && s.deleted.is_empty() {
        return None;
    }
    let ids: Vec<String> = s.handles.iter().map(|h| h.hex()).collect();
    Some(json!({ "handles": ids, "add": s.added, "delete": s.deleted }))
}

/// "1 paper unit = 5 drawing units" for the scale `name`.
pub fn units_line(d: &Drawing, name: &str) -> Option<String> {
    let sc = d.anno_scale(name)?;
    let n = |v: f64| {
        let s = format!("{v:.4}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    };
    Some(crate::tf!("{paper} paper unit = {drawing} drawing units", paper = n(sc.paper_units), drawing = n(sc.drawing_units)).to_string())
}

/// Open the dialog on the annotative objects among `handles`; says so when there are none.
pub fn open(app: &mut CadApp, handles: &[Handle]) {
    let Ok(st) = app.session.state() else { return };
    let objs: Vec<Handle> = handles.iter().copied().filter(|h| st.doc.entity(*h).is_some_and(|e| cadcraft_doc::is_annotative(e))).collect();
    let uid = st.uid;
    if objs.is_empty() {
        app.session.echo(crate::tl!("Select annotative objects to change their annotation scales."));
        return;
    }
    app.objectscale = Some(ObjectScaleDlg { handles: objs, uid, ..ObjectScaleDlg::default() });
    app.ui.dialog = Some("objectscale".into());
}

/// Whether the dialog is up: its state and the open dialog both say so (another dialog, or a
/// control call, can replace it).
pub fn is_open(app: &CadApp) -> bool {
    app.objectscale.is_some() && app.ui.dialog.as_deref() == Some("objectscale")
}

pub fn dialog(app: &mut CadApp, ctx: &egui::Context, open: &mut bool) {
    let Some(mut s) = app.objectscale.clone() else {
        *open = false;
        return;
    };
    let Ok(st) = app.session.state() else {
        *open = false;
        return;
    };
    // Another drawing, or objects that went away: the dialog closes.
    if st.uid != s.uid || s.handles.iter().all(|h| st.doc.entity(*h).is_none()) {
        app.objectscale = None;
        *open = false;
        return;
    }
    let d = st.doc.clone();
    let t = Tokens::get();
    let mut ok = false;
    let mut cancel = false;
    // Modal: the drawing behind takes no clicks or keys while it is open (see
    // `cmdline::keyboard`), so Escape doesn't cancel or deselect there.
    egui::Modal::new(egui::Id::new("Annotation Object Scale")).show(ctx, |ui| {
        ui.heading(crate::tl!("Annotation Object Scale"));
        ui.add_space(4.0);
        ui.label(egui::RichText::new(crate::tl!("Object Scale List")).color(t.text_dim));
        let list = listed(&d, &s);
        if s.selected.as_ref().is_some_and(|n| !list.iter().any(|x| same(x, n))) {
            s.selected = None;
        }
        ui.horizontal_top(|ui| {
            egui::Frame::new().fill(t.control).inner_margin(4.0).show(ui, |ui| {
                egui::ScrollArea::vertical().max_height(160.0).min_scrolled_height(160.0).show(ui, |ui| {
                    // One scale per line, from the top.
                    ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
                        ui.set_width(220.0);
                        ui.set_min_height(160.0);
                        for n in &list {
                            let on = s.selected.as_ref().is_some_and(|x| same(x, n));
                            if ui.selectable_label(on, n).clicked() {
                                s.selected = Some(n.clone());
                            }
                        }
                    });
                });
            });
            ui.vertical(|ui| {
                if ui.button(crate::tl!("Add...")).clicked() {
                    s.adding = Some(Vec::new());
                }
                let picked = s.selected.clone().filter(|n| can_delete(&d, &s, n));
                if ui.add_enabled(picked.is_some(), egui::Button::new(crate::tl!("Delete"))).clicked()
                    && let Some(n) = picked
                {
                    delete(&mut s, &n);
                }
            });
        });
        let shown = s.selected.clone().or_else(|| list.first().cloned());
        if let Some(line) = shown.and_then(|n| units_line(&d, &n)) {
            ui.label(line);
        }
        ui.radio_value(&mut s.common_only, false, crate::tl!("List all scales for selected objects"));
        ui.radio_value(&mut s.common_only, true, crate::tl!("List scales common to all selected objects only"));
        if let Some(e) = &s.error {
            ui.colored_label(t.warn, e);
        }
        ui.separator();
        ui.horizontal(|ui| {
            ok = ui.button(crate::tl!("OK")).clicked();
            cancel = ui.button(crate::tl!("Cancel")).clicked();
        });
    });
    // Add Scales to Object: the drawing's other scales, several at a time.
    if let Some(mut picks) = s.adding.clone() {
        let mut done = false;
        egui::Modal::new(egui::Id::new("Add Scales to Object")).show(ctx, |ui| {
            ui.heading(crate::tl!("Add Scales to Object"));
            ui.add_space(4.0);
            ui.label(egui::RichText::new(crate::tl!("Scale List")).color(t.text_dim));
            egui::Frame::new().fill(t.control).inner_margin(4.0).show(ui, |ui| {
                egui::ScrollArea::vertical().max_height(220.0).min_scrolled_height(220.0).show(ui, |ui| {
                    ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
                        ui.set_width(220.0);
                        for n in addable(&d, &s) {
                            let on = picks.iter().any(|x| same(x, &n));
                            if ui.selectable_label(on, &n).clicked() {
                                if on {
                                    picks.retain(|x| !same(x, &n));
                                } else {
                                    picks.push(n.clone());
                                }
                            }
                        }
                    });
                });
            });
            ui.horizontal(|ui| {
                if ui.button(crate::tl!("OK")).clicked() {
                    add(&mut s, &picks);
                    done = true;
                }
                if ui.button(crate::tl!("Cancel")).clicked() {
                    done = true;
                }
            });
        });
        s.adding = if done { None } else { Some(picks) };
    }
    if cancel {
        *open = false;
    }
    if ok {
        match params(&s).map(|p| app.run("objectscale", p)) {
            Some(Err(e)) => s.error = Some(e.to_string()),
            _ => *open = false,
        }
    }
    app.objectscale = if *open { Some(s) } else { None };
}

#[cfg(test)]
mod tests {
    use cadcraft_engine::Session;
    use serde_json::json;

    use super::*;
    use crate::{CadApp, Services};

    fn select(app: &mut CadApp, hs: &[Handle]) {
        let ids: Vec<String> = hs.iter().map(|h| h.hex()).collect();
        app.run("select", json!({ "handles": ids })).unwrap();
    }

    /// Two annotative texts: one at 1:1 and 1:2, one at 1:2 only; and a plain one.
    fn app() -> (CadApp, Vec<Handle>) {
        let mut app = CadApp::new(Session::new(), Services::default());
        let mut hs = Vec::new();
        for y in [0, 5, 10] {
            let r = app.run("text", json!({ "at": [0, y], "text": "A", "height": 2.5 })).unwrap();
            hs.push(Handle::parse_hex(r["handle"].as_str().unwrap()).unwrap());
        }
        app.run("properties.set", json!({ "handles": [hs[0].hex(), hs[1].hex()], "annotative": true })).unwrap();
        app.run("objectscale", json!({ "handles": [hs[0].hex(), hs[1].hex()], "add": "1:2" })).unwrap();
        app.run("objectscale", json!({ "handles": [hs[1].hex()], "delete": "1:1" })).unwrap();
        (app, hs)
    }

    #[test]
    fn the_dialog_replaced_by_another_no_longer_takes_the_keys() {
        let (mut app, hs) = app();
        open(&mut app, &hs);
        assert!(is_open(&app));
        // Another dialog (or `ui.dialog.close`) replaces it: the command line gets the keys again.
        app.ui.dialog = Some("authoring".into());
        assert!(!is_open(&app));
        app.ui.dialog = None;
        assert!(!is_open(&app));
    }

    #[test]
    fn the_list_shows_all_or_common_scales_and_follows_add_and_delete() {
        let (mut app, hs) = app();
        select(&mut app, &hs);
        open(&mut app, &hs);
        assert_eq!(app.ui.dialog.as_deref(), Some("objectscale"));
        let mut s = app.objectscale.clone().unwrap();
        // The plain text isn't one of the dialog's objects.
        assert_eq!(s.handles, hs[..2]);
        let d = app.session.doc().unwrap().clone();
        assert_eq!(listed(&d, &s), ["1:1", "1:2"]);
        s.common_only = true;
        assert_eq!(listed(&d, &s), ["1:2"]);
        // 1:2 is the second text's only scale: it can't go; 1:1 can.
        assert!(!can_delete(&d, &s, "1:2") && can_delete(&d, &s, "1:1"));
        add(&mut s, &["1:4".to_string()]);
        assert_eq!(listed(&d, &s), ["1:2", "1:4"]);
        assert!(can_delete(&d, &s, "1:2"));
        delete(&mut s, "1:2");
        assert_eq!(listed(&d, &s), ["1:4"]);
        assert!(!addable(&d, &s).contains(&"1:4".to_string()) && addable(&d, &s).contains(&"1:2".to_string()));
        // Adding back what was deleted only takes the deletion back.
        add(&mut s, &["1:2".to_string()]);
        assert!(s.deleted.is_empty());
        delete(&mut s, "1:2");
        assert_eq!(params(&s), Some(json!({ "handles": [hs[0].hex(), hs[1].hex()], "add": ["1:4"], "delete": ["1:2"] })));
        assert_eq!(units_line(&d, "1:4").as_deref(), Some("1 paper unit = 4 drawing units"));
    }

    #[test]
    fn ok_runs_one_objectscale_and_nothing_changed_runs_none() {
        let (mut app, hs) = app();
        let mut s = ObjectScaleDlg { handles: hs[..2].to_vec(), ..ObjectScaleDlg::default() };
        assert_eq!(params(&s), None);
        add(&mut s, &["1:4".to_string()]);
        delete(&mut s, "1:1");
        app.run("objectscale", params(&s).unwrap()).unwrap();
        let d = app.session.doc().unwrap();
        let names = |h: Handle| d.entity(h).unwrap().common.anno_scales.iter().map(|x| x.name.clone()).collect::<Vec<_>>();
        assert_eq!(names(hs[0]), ["1:2", "1:4"]);
        assert_eq!(names(hs[1]), ["1:2", "1:4"]);
    }

    #[test]
    fn objectscale_opens_the_dialog_on_a_selection_and_asks_without_one() {
        let (mut app, hs) = app();
        app.start("objectscale");
        assert!(app.ui.dialog.is_none(), "no selection: the command line asks");
        app.session.cancel();
        select(&mut app, &hs[..1]);
        app.start("objectscale");
        assert_eq!(app.ui.dialog.as_deref(), Some("objectscale"));
        // A selection without annotative objects: the command line too.
        app.ui.dialog = None;
        app.session.cancel();
        select(&mut app, &hs[2..]);
        app.start("objectscale");
        assert!(app.ui.dialog.is_none());
        // JSON calls never open it.
        select(&mut app, &hs[..1]);
        app.run("objectscale", json!({ "handles": [hs[0].hex()], "add": "1:4" })).unwrap();
        assert!(app.ui.dialog.is_none());
    }
}
