//! Move or Copy (a layout tab's menu): the layout goes before the layout picked in the list, or
//! to the end; with Create a Copy a copy goes there and the layout stays. OK runs `layout.move`.

use serde_json::{Value, json};

use super::{set_temp, temp};
use crate::CadApp;

/// The dialog's name in `app.ui.dialog` is this followed by the layout's name.
pub(super) const DIALOG: &str = "layoutmove:";
/// What the dialog shows: the place picked (`None`: the end) and Create a Copy.
const STATE: &str = "layoutmove_state";

#[derive(Clone, Default)]
struct State {
    /// The layout the dialog is open for (a new one starts afresh).
    name: String,
    before: Option<String>,
    copy: bool,
}

/// Open the dialog for layout `name`.
pub fn open(app: &mut CadApp, name: &str) {
    app.ui.dialog = Some(format!("{DIALOG}{name}"));
}

/// Show the dialog; returns the command to run.
pub(super) fn dialog(app: &mut CadApp, ctx: &egui::Context, dialog: &str, open: &mut bool) -> Option<(&'static str, Value)> {
    let name = dialog.strip_prefix(DIALOG).unwrap_or_default();
    let Some(layouts) = app.session.doc().ok().map(|d| {
        let mut ls: Vec<(u32, String)> = d.layouts.iter().map(|l| (l.tab_order, l.name.clone())).collect();
        ls.sort_by_key(|l| l.0);
        ls.into_iter().map(|l| l.1).collect::<Vec<_>>()
    }) else {
        *open = false;
        return None;
    };
    // The layout went away (undone, deleted): nothing to move.
    if !layouts.iter().any(|l| l == name) {
        *open = false;
        return None;
    }
    let mut st = temp::<State>(ctx, STATE).filter(|s| s.name == name).unwrap_or_else(|| State { name: name.to_string(), ..State::default() });
    st.before = st.before.filter(|b| layouts.contains(b));
    let (mut ok, mut cancel) = (false, false);
    let center = ctx.content_rect().center();
    egui::Window::new(crate::tl!("Move or Copy"))
        .id(egui::Id::unique("layout_move_dialog"))
        .open(open)
        .collapsible(false)
        .resizable(false)
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(center)
        .show(ctx, |ui| {
            ui.label(crate::tf!("Move or copy {name}", name = name));
            let label = ui.label(crate::tl!("Before layout:"));
            let list = egui::Frame::group(ui.style()).show(ui, |ui| {
                egui::ScrollArea::vertical().id_salt("layout_move_list").max_height(180.0).show(ui, |ui| {
                    ui.set_min_width(220.0);
                    for l in &layouts {
                        let r = ui.selectable_label(st.before.as_deref() == Some(l.as_str()), l);
                        if r.clicked() {
                            st.before = Some(l.clone());
                        }
                        if r.double_clicked() {
                            ok = true;
                        }
                    }
                    let end = ui.selectable_label(st.before.is_none(), crate::tl!("(move to end)"));
                    if end.clicked() {
                        st.before = None;
                    }
                    if end.double_clicked() {
                        ok = true;
                    }
                });
            });
            list.response.labelled_by(label.id);
            ui.checkbox(&mut st.copy, crate::tl!("Create a copy"));
            ui.separator();
            ui.horizontal(|ui| {
                ok |= ui.button(crate::tl!("OK")).clicked();
                cancel = ui.button(crate::tl!("Cancel")).clicked();
            });
        });
    if ok || cancel || !*open {
        *open = false;
        ctx.data_mut(|d| d.remove_temp::<State>(egui::Id::unique(STATE)));
    } else {
        set_temp(ctx, STATE, st.clone());
    }
    if !ok {
        return None;
    }
    let mut p = json!({ "name": name, "copy": st.copy });
    if let (Some(b), Some(o)) = (st.before, p.as_object_mut()) {
        o.insert("before".into(), json!(b));
    }
    Some(("layout.move", p))
}
