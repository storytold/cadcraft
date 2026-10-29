//! Extended Entity Data Editor (`XDEDIT`): visual viewer and editor for DXF/DWG XData (1001..1071).
//! Supports Smart Key=Value editing (for `CAD_XDATA` and GIS attributes), Raw DXF Tag editing,
//! user-resizable columns (drag column header divider `⇔`), one-click clipboard copy, search
//! filtering, adding/removing Registered Applications (AppIDs), object picking/stepping, and Undo/Redo.

use cadcraft_doc::{Entity, Handle, RawTag};
use egui::{RichText, Sense, pos2, vec2};
use serde_json::{Value, json};

use crate::CadApp;
use crate::theme::Tokens;

const CODES: &[(i32, &str)] = &[
    (1000, "1000 String"),
    (1003, "1003 Layer"),
    (1005, "1005 Handle"),
    (1040, "1040 Real"),
    (1041, "1041 Distance"),
    (1042, "1042 Scale"),
    (1070, "1070 Int16"),
    (1071, "1071 Int32"),
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct XdRow {
    pub code: i32,
    pub key: String,
    pub val: String,
    pub sep: String,
    pub original_raw: Option<String>,
}

impl XdRow {
    pub fn from_tag(tag: &RawTag) -> Self {
        let raw = tag.value.trim();
        if tag.code == 1000 {
            if let Some((k, v)) = raw.split_once('=') {
                return Self {
                    code: 1000,
                    key: k.trim().to_string(),
                    val: v.trim().to_string(),
                    sep: "=".to_string(),
                    original_raw: Some(tag.value.clone()),
                };
            }
            if let Some((k, v)) = raw.split_once(": ") {
                return Self {
                    code: 1000,
                    key: k.trim().to_string(),
                    val: v.trim().to_string(),
                    sep: ": ".to_string(),
                    original_raw: Some(tag.value.clone()),
                };
            }
        }
        Self {
            code: tag.code,
            key: String::new(),
            val: raw.to_string(),
            sep: "=".to_string(),
            original_raw: Some(tag.value.clone()),
        }
    }

    pub fn to_raw_value(&self, _smart_kv: bool) -> String {
        if let Some(raw) = &self.original_raw {
            return raw.clone();
        }
        if self.code == 1000 && !self.key.trim().is_empty() {
            let s = if self.sep.is_empty() { "=" } else { self.sep.as_str() };
            format!("{}{s}{}", self.key.trim(), self.val.trim())
        } else {
            self.val.clone()
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct XdAppGroup {
    pub app: String,
    pub rows: Vec<XdRow>,
}

#[derive(Clone, Debug)]
pub struct XdEditState {
    pub handle: Option<Handle>,
    pub selection_list: Vec<Handle>,
    pub entity_type: String,
    pub layer: String,
    pub loaded_xdata: Vec<RawTag>,
    pub groups: Vec<XdAppGroup>,
    pub active_app: usize,
    pub filter: String,
    pub smart_kv: bool,
    pub key_col_width: f32,
    pub val_col_width: f32,
    pub new_app_name: String,
    pub new_code: i32,
    pub new_key: String,
    pub new_val: String,
    pub status_msg: Option<String>,
    pub dirty: bool,
    pub awaiting_pick: bool,
}

impl Default for XdEditState {
    fn default() -> Self {
        Self {
            handle: None,
            selection_list: Vec::new(),
            entity_type: String::new(),
            layer: String::new(),
            loaded_xdata: Vec::new(),
            groups: Vec::new(),
            active_app: 0,
            filter: String::new(),
            smart_kv: true,
            key_col_width: 280.0,
            val_col_width: 430.0,
            new_app_name: String::new(),
            new_code: 1000,
            new_key: String::new(),
            new_val: String::new(),
            status_msg: None,
            dirty: false,
            awaiting_pick: false,
        }
    }
}

fn is_system_app(app: &str) -> bool {
    cadcraft_engine::is_system_xdata_app(app)
}

fn has_user_xdata(e: &Entity) -> bool {
    e.common.xdata.iter().any(|t| t.code == 1001 && !is_system_app(t.value.trim()))
}

impl XdEditState {
    pub fn from_entity(e: &Entity) -> Self {
        Self::from_entity_with_list(e, vec![e.handle])
    }

    pub fn from_entity_with_list(e: &Entity, selection_list: Vec<Handle>) -> Self {
        let mut groups: Vec<XdAppGroup> = Vec::new();
        let mut cur_idx: Option<usize> = None;
        let mut max_key_len: usize = 22;
        let mut max_val_len: usize = 38;
        for tag in &e.common.xdata {
            if tag.code == 1001 {
                let app_name = tag.value.trim().to_string();
                if is_system_app(&app_name) {
                    cur_idx = None;
                    continue;
                }
                if let Some(pos) = groups.iter().position(|g| g.app.eq_ignore_ascii_case(&app_name)) {
                    cur_idx = Some(pos);
                } else {
                    groups.push(XdAppGroup { app: app_name, rows: Vec::new() });
                    cur_idx = Some(groups.len().saturating_sub(1));
                }
                continue;
            }
            if let Some(idx) = cur_idx
                && let Some(g) = groups.get_mut(idx)
            {
                let row = XdRow::from_tag(tag);
                if tag.code != 1002 {
                    max_key_len = max_key_len.max(row.key.chars().count());
                    max_val_len = max_val_len.max(row.val.chars().count());
                }
                g.rows.push(row);
            }
        }
        let auto_key_w = ((max_key_len as f32) * 8.8 + 36.0).clamp(280.0, 440.0);
        let auto_val_w = ((max_val_len as f32) * 8.2 + 36.0).clamp(420.0, 580.0);
        let list = if selection_list.contains(&e.handle) { selection_list } else { vec![e.handle] };
        Self {
            handle: Some(e.handle),
            selection_list: list,
            entity_type: e.kind.dxf_name().to_string(),
            layer: e.common.layer.clone(),
            loaded_xdata: e.common.xdata.clone(),
            groups,
            active_app: 0,
            filter: String::new(),
            smart_kv: true,
            key_col_width: auto_key_w,
            val_col_width: auto_val_w,
            new_app_name: String::new(),
            new_code: 1000,
            new_key: String::new(),
            new_val: String::new(),
            status_msg: None,
            dirty: false,
            awaiting_pick: false,
        }
    }

    pub fn reload_from_entity(&mut self, e: &Entity) {
        let kw = self.key_col_width;
        let vw = self.val_col_width;
        let skv = self.smart_kv;
        let flt = self.filter.clone();
        let active = self.active_app;
        let list = self.selection_list.clone();
        let mut next = Self::from_entity_with_list(e, list);
        next.key_col_width = next.key_col_width.max(kw);
        next.val_col_width = next.val_col_width.max(vw);
        next.smart_kv = skv;
        next.filter = flt;
        if !next.groups.is_empty() {
            next.active_app = active.min(next.groups.len() - 1);
        }
        *self = next;
    }

    pub fn to_xdata_json(&self) -> Value {
        let mut arr = Vec::new();
        for g in &self.groups {
            if g.app.trim().is_empty() {
                continue;
            }
            arr.push(json!({ "code": 1001, "value": g.app.trim() }));
            for r in &g.rows {
                arr.push(json!({ "code": r.code, "value": r.to_raw_value(self.smart_kv) }));
            }
        }
        Value::Array(arr)
    }

    pub fn format_plain_text(&self) -> String {
        let mut out = format!("Object,{}", self.entity_type);
        for g in &self.groups {
            for r in &g.rows {
                if r.code == 1002 {
                    continue;
                }
                if !r.key.trim().is_empty() {
                    out.push_str(&format!("\n{},{}", r.key.trim(), r.val.trim()));
                } else {
                    out.push_str(&format!("\n{}", r.val.trim().replace('=', ",")));
                }
            }
        }
        out
    }

    pub fn format_export_json(&self) -> String {
        let h_str = self.handle.map(|h| h.hex()).unwrap_or_default();
        let mut apps = serde_json::Map::new();
        for g in &self.groups {
            let mut app_obj = serde_json::Map::new();
            for (idx, r) in g.rows.iter().enumerate() {
                if r.code == 1002 {
                    continue;
                }
                if !r.key.trim().is_empty() {
                    app_obj.insert(r.key.trim().to_string(), json!(r.val));
                } else {
                    app_obj.insert(format!("{}_{}", r.code, idx), json!(r.val));
                }
            }
            apps.insert(g.app.clone(), Value::Object(app_obj));
        }
        let root = json!({
            "handle": h_str,
            "entityType": self.entity_type,
            "layer": self.layer,
            "xdata": apps,
        });
        serde_json::to_string_pretty(&root).unwrap_or_default()
    }
}

/// Intercept typed/menu `xdedit` or `xde` when called without JSON parameters.
pub fn route(app: &mut CadApp, id: &str, params: &Value) -> Option<Result<Value, String>> {
    if !params.is_null() || !matches!(id, "xdedit" | "xde" | "ui.dialog.xdedit") {
        return None;
    }
    let sel = app.session.selection();
    if let Some(&h) = sel.first()
        && let Ok(d) = app.session.doc()
        && let Some(e) = d.entity(h)
    {
        app.xdedit_dlg = Some(XdEditState::from_entity_with_list(e, sel));
        app.ui.dialog = Some("xdedit".into());
        return Some(Ok(Value::Null));
    }
    // No selection yet: start the interactive single-object pick and open the dialog as soon as picked.
    app.xdedit_dlg = Some(XdEditState { awaiting_pick: true, ..Default::default() });
    app.ui.dialog = None;
    None
}

/// Open the XDATA Editor directly for a given entity handle (e.g. from Properties panel or XDLIST).
pub fn open_for_handle(app: &mut CadApp, h: Handle) {
    let sel = app.session.selection();
    if let Ok(d) = app.session.doc()
        && let Some(e) = d.entity(h)
    {
        let list = if sel.contains(&h) { sel } else { vec![h] };
        app.xdedit_dlg = Some(XdEditState::from_entity_with_list(e, list));
        app.ui.dialog = Some("xdedit".into());
    }
}

/// Called every frame in `CadApp::logic`: opens the dialog once an interactive `XDEDIT` object pick finishes.
pub fn resume(app: &mut CadApp) {
    let awaiting = app.xdedit_dlg.as_ref().is_some_and(|s| s.awaiting_pick);
    if !awaiting || app.session.running.is_some() {
        return;
    }
    let prev = app.xdedit_dlg.take();
    let sel = app.session.selection();
    if let Some(&h) = sel.first()
        && let Ok(d) = app.session.doc()
        && let Some(e) = d.entity(h)
    {
        let mut next = XdEditState::from_entity_with_list(e, sel);
        if let Some(p) = prev {
            next.key_col_width = next.key_col_width.max(p.key_col_width);
            next.val_col_width = next.val_col_width.max(p.val_col_width);
            next.smart_kv = p.smart_kv;
            next.filter = p.filter;
        }
        app.xdedit_dlg = Some(next);
        app.ui.dialog = Some("xdedit".into());
    } else if let Some(mut p) = prev
        && p.handle.is_some()
    {
        // Pick was cancelled with Esc/Enter: return to editor with the previous object preserved.
        p.awaiting_pick = false;
        p.status_msg = Some(crate::tl!("Selection canceled — previous object kept.").into());
        if let Some(h) = p.handle {
            app.session.set_selection(vec![h]);
        }
        app.xdedit_dlg = Some(p);
        app.ui.dialog = Some("xdedit".into());
    }
}

/// Find the previous or next entity handle to inspect (`delta = -1` or `+1`).
/// Uses `selection_list` when multiple objects are selected; otherwise cycles through visible
/// XData-bearing entities (or all visible entities) in the current space.
fn step_entity(app: &CadApp, current: Handle, selection_list: &[Handle], delta: isize) -> Option<(Handle, usize, usize)> {
    if selection_list.len() > 1
        && let Some(pos) = selection_list.iter().position(|&h| h == current)
    {
        let len = selection_list.len();
        let next_idx = (pos as isize + delta).rem_euclid(len as isize) as usize;
        return Some((selection_list[next_idx], next_idx + 1, len));
    }
    let d = app.session.doc().ok()?;
    let space = app.session.space();
    let store = d.space(&space)?;
    let mut cands: Vec<Handle> = store
        .iter()
        .filter(|e| d.is_visible(e) && has_user_xdata(e))
        .map(|e| e.handle)
        .collect();
    if cands.is_empty() {
        cands = store.iter().filter(|e| d.is_visible(e)).map(|e| e.handle).collect();
    }
    if cands.is_empty() {
        return None;
    }
    let len = cands.len();
    let pos = cands.iter().position(|&h| h == current).unwrap_or(0);
    let next_idx = (pos as isize + delta).rem_euclid(len as isize) as usize;
    Some((cands[next_idx], next_idx + 1, len))
}

/// A column header with a draggable right-edge resize handle (`⇔`).
fn resizable_header(ui: &mut egui::Ui, id_salt: &str, title: &str, width: &mut f32, min_w: f32, max_w: f32) {
    let t = Tokens::get();
    let (rect, _) = ui.allocate_exact_size(vec2(*width, 20.0), Sense::hover());
    ui.painter().text(
        pos2(rect.left() + 2.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        title,
        crate::theme::body(),
        t.text,
    );
    let grip_rect = egui::Rect::from_min_max(pos2(rect.right() - 14.0, rect.top()), rect.max);
    let grip_resp = ui.interact(grip_rect, ui.scope_id().with(id_salt), Sense::click_and_drag());
    let grip_color = if grip_resp.hovered() || grip_resp.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        egui::Color32::from_rgb(96, 165, 250)
    } else {
        t.text_dim
    };
    ui.painter().line_segment(
        [pos2(rect.right() - 2.0, rect.top() + 2.0), pos2(rect.right() - 2.0, rect.bottom() - 2.0)],
        egui::Stroke::new(2.5, grip_color),
    );
    if grip_resp.dragged() {
        *width = (*width + grip_resp.drag_delta().x).clamp(min_w, max_w);
    }
    grip_resp.on_hover_text(crate::tl!("Drag left or right to resize this column"));
}

pub fn dialog(app: &mut CadApp, ctx: &egui::Context, open: &mut bool) {
    let t = Tokens::get();
    // Sync with selection if user clicked another entity while not dirty.
    let sel = app.session.selection();
    if let Some(&h) = sel.first()
        && app.xdedit_dlg.as_ref().is_none_or(|st| st.handle != Some(h) && !st.dirty)
        && let Ok(d) = app.session.doc()
        && let Some(e) = d.entity(h)
    {
        let prev_cfg = app
            .xdedit_dlg
            .as_ref()
            .map(|s| (s.key_col_width, s.val_col_width, s.smart_kv, s.filter.clone(), s.selection_list.clone()));
        let list = if sel.len() > 1 {
            sel
        } else if let Some((_, _, _, _, ref prev_list)) = prev_cfg
            && prev_list.contains(&h)
        {
            prev_list.clone()
        } else {
            vec![h]
        };
        let mut next = XdEditState::from_entity_with_list(e, list);
        if let Some((kw, vw, skv, flt, _)) = prev_cfg {
            next.key_col_width = next.key_col_width.max(kw);
            next.val_col_width = next.val_col_width.max(vw);
            next.smart_kv = skv;
            next.filter = flt;
        }
        app.xdedit_dlg = Some(next);
    }

    let Some(mut st) = app.xdedit_dlg.clone() else {
        *open = false;
        return;
    };

    let mut external_changed = false;
    if let Some(cur_h) = st.handle
        && let Ok(d) = app.session.doc()
        && let Some(e) = d.entity(cur_h)
        && e.common.xdata != st.loaded_xdata
    {
        if !st.dirty {
            st.reload_from_entity(e);
        } else {
            external_changed = true;
        }
    }

    let mut apply_changes = false;
    let mut revert_changes = false;
    let mut start_pick_another = false;
    let mut step_dir: Option<isize> = None;
    let mut close_clicked = false;

    egui::Window::new(crate::tl!("XDATA Editor (XDEDIT)"))
        .id(egui::Id::unique("xdedit_window_v3"))
        .open(open)
        .default_size(vec2(1100.0, 720.0))
        .min_width(860.0)
        .min_height(540.0)
        .resizable(true)
        .collapsible(false)
        .show(ctx, |ui| {
            let Some(h) = st.handle else {
                ui.vertical_centered(|ui| {
                    ui.add_space(20.0);
                    ui.label(crate::tl!("No object selected."));
                    ui.add_space(8.0);
                    if ui.button(crate::tl!("🎯 Select object in drawing")).clicked() {
                        start_pick_another = true;
                    }
                });
                return;
            };

            // 1. Header bar: Object info + Prev/Next + Pick Object + Copy All
            ui.horizontal(|ui| {
                ui.label(RichText::new(crate::tf!("Object: {kind}", kind = st.entity_type.as_str())).strong());
                ui.separator();
                ui.label(RichText::new(crate::tf!("Layer: {layer}", layer = st.layer.as_str())).color(t.text_dim));
                ui.separator();
                ui.label(RichText::new(crate::tf!("Handle: {handle}", handle = h.hex())).monospace().color(t.text_dim));
                if st.selection_list.len() > 1
                    && let Some(pos) = st.selection_list.iter().position(|&x| x == h)
                {
                    ui.separator();
                    ui.label(RichText::new(format!("({}/{})", pos + 1, st.selection_list.len())).strong());
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .button(RichText::new(crate::tl!("🎯 Select Another Object")).strong())
                        .on_hover_text(crate::tl!("Hide editor temporarily, pick another object in the drawing, and open it in XDEDIT"))
                        .clicked()
                    {
                        start_pick_another = true;
                    }
                    if ui
                        .button(crate::tl!("Next ▶"))
                        .on_hover_text(crate::tl!("Step to the next object with XData in the selection or drawing"))
                        .clicked()
                    {
                        step_dir = Some(1);
                    }
                    if ui
                        .button(crate::tl!("◀ Previous"))
                        .on_hover_text(crate::tl!("Step to the previous object with XData in the selection or drawing"))
                        .clicked()
                    {
                        step_dir = Some(-1);
                    }
                    ui.separator();
                    if ui
                        .button(crate::tl!("📋 Copy JSON"))
                        .on_hover_text(crate::tl!("Copy all XData as structured JSON to clipboard"))
                        .clicked()
                    {
                        ctx.copy_text(st.format_export_json());
                        st.status_msg = Some(crate::tl!("✓ JSON copied to clipboard").into());
                    }
                    if ui
                        .button(crate::tl!("📋 Copy Text"))
                        .on_hover_text(crate::tl!("Copy all XData as text to clipboard"))
                        .clicked()
                    {
                        ctx.copy_text(st.format_plain_text());
                        st.status_msg = Some(crate::tl!("✓ XData text copied to clipboard").into());
                    }
                });
            });
            ui.separator();

            // 2. Application (1001 AppID) Tabs & Management
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(crate::tl!("AppID (1001):")).strong());
                for (idx, g) in st.groups.iter().enumerate() {
                    let visible_count = g.rows.iter().filter(|r| r.code != 1002).count();
                    let label = format!("{} ({visible_count})", g.app);
                    if ui.selectable_label(st.active_app == idx, RichText::new(label).strong()).clicked() {
                        st.active_app = idx;
                    }
                }
                ui.separator();
                let resp = ui.add_sized(
                    [155.0, 22.0],
                    egui::TextEdit::singleline(&mut st.new_app_name)
                        .hint_text(crate::tl!("New AppID (e.g. CAD_XDATA)"))
                        .desired_width(f32::INFINITY),
                );
                let add_clicked = ui.button(crate::tl!("+ Add AppID")).clicked()
                    || (resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) && !st.new_app_name.trim().is_empty());
                if add_clicked {
                    let name = if st.new_app_name.trim().is_empty() {
                        "CAD_XDATA".to_string()
                    } else {
                        st.new_app_name.trim().to_string()
                    };
                    if let Some(pos) = st.groups.iter().position(|g| g.app.eq_ignore_ascii_case(&name)) {
                        st.active_app = pos;
                    } else {
                        st.groups.push(XdAppGroup { app: name, rows: Vec::new() });
                        st.active_app = st.groups.len().saturating_sub(1);
                        st.dirty = true;
                    }
                    st.new_app_name.clear();
                }
                if !st.groups.is_empty()
                    && ui
                        .button(crate::tl!("🗑 Delete AppID"))
                        .on_hover_text(crate::tl!("Remove this Application and its XData from the entity"))
                        .clicked()
                    && st.active_app < st.groups.len()
                {
                    st.groups.remove(st.active_app);
                    if st.active_app >= st.groups.len() && !st.groups.is_empty() {
                        st.active_app = st.groups.len() - 1;
                    }
                    st.dirty = true;
                    st.status_msg = Some(crate::tl!("Application removed (click Apply to save)").into());
                }
            });
            ui.separator();

            // 3. Filter, Column Width Controls & View Mode Bar
            ui.horizontal(|ui| {
                ui.label(crate::tl!("🔍 Filter:"));
                ui.add_sized(
                    [230.0, 22.0],
                    egui::TextEdit::singleline(&mut st.filter)
                        .hint_text(crate::tl!("Search key or value (e.g. THEMA, DIEPTE)..."))
                        .desired_width(f32::INFINITY),
                );
                if !st.filter.is_empty() && ui.small_button("✕").clicked() {
                    st.filter.clear();
                }
                ui.separator();
                ui.label(RichText::new(crate::tl!("Key width:")).color(t.text_dim));
                ui.add(egui::DragValue::new(&mut st.key_col_width).range(140.0..=600.0).speed(2.0).suffix(" px"))
                    .on_hover_text(crate::tl!("Drag this value or the column divider ⇔ to resize the Key column"));
                ui.label(RichText::new(crate::tl!("Value width:")).color(t.text_dim));
                ui.add(egui::DragValue::new(&mut st.val_col_width).range(200.0..=900.0).speed(2.0).suffix(" px"))
                    .on_hover_text(crate::tl!("Drag this value or the column divider ⇔ to resize the Value column"));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.checkbox(&mut st.smart_kv, crate::tl!("Smart Key=Value Columns"))
                        .on_hover_text(crate::tl!("Split 'KEY=VALUE' or 'Key: Value' into separate editable Key and Value fields"));
                });
            });
            ui.add_space(4.0);

            // 4. Main Editable XData Table
            if st.groups.is_empty() {
                ui.vertical_centered(|ui| {
                    ui.add_space(30.0);
                    ui.label(RichText::new(crate::tl!("This object currently has no user XData.")).color(t.text_dim));
                    ui.add_space(8.0);
                    if ui.button(crate::tl!("+ Create CAD_XDATA Application")).clicked() {
                        st.groups.push(XdAppGroup {
                            app: "CAD_XDATA".into(),
                            rows: vec![
                                XdRow { code: 1000, key: "THEMA".into(), val: String::new(), sep: "=".into(), original_raw: None },
                                XdRow { code: 1000, key: "NETBEHEERDER".into(), val: String::new(), sep: "=".into(), original_raw: None },
                                XdRow { code: 1000, key: "MATERIAAL".into(), val: String::new(), sep: "=".into(), original_raw: None },
                                XdRow { code: 1000, key: "DIEPTE".into(), val: String::new(), sep: "=".into(), original_raw: None },
                            ],
                        });
                        st.active_app = 0;
                        st.dirty = true;
                    }
                    ui.add_space(30.0);
                });
            } else {
                let active_idx = st.active_app.min(st.groups.len().saturating_sub(1));
                st.active_app = active_idx;
                let filter_low = st.filter.trim().to_ascii_lowercase();
                let smart_kv = st.smart_kv;
                let mut key_w = st.key_col_width;
                let mut val_w = st.val_col_width;
                let mut move_up: Option<usize> = None;
                let mut move_down: Option<usize> = None;
                let mut delete_row: Option<usize> = None;
                let mut copied_msg: Option<String> = None;
                let mut row_changed = false;

                if let Some(group) = st.groups.get_mut(active_idx) {
                    let row_codes: Vec<i32> = group.rows.iter().map(|r| r.code).collect();
                    let scroll_h = (ui.available_height() - 78.0).max(420.0);
                    egui::ScrollArea::vertical()
                        .id_salt("xdedit_rows_scroll")
                        .max_height(scroll_h)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            egui::Grid::new("xdedit_table_grid")
                                .striped(true)
                                .num_columns(if smart_kv { 5 } else { 4 })
                                .spacing(vec2(8.0, 6.0))
                                .show(ui, |ui| {
                                    ui.label(RichText::new("#").strong().color(t.text_dim));
                                    ui.label(RichText::new(crate::tl!("DXF Code")).strong());
                                    if smart_kv {
                                        resizable_header(ui, "xd_hdr_key", crate::tl!("Property / Key ⇔"), &mut key_w, 140.0, 600.0);
                                        resizable_header(
                                            ui,
                                            "xd_hdr_val",
                                            crate::tl!("Value (Selectable & Editable) ⇔"),
                                            &mut val_w,
                                            200.0,
                                            900.0,
                                        );
                                    } else {
                                        let mut raw_w = key_w + val_w;
                                        resizable_header(
                                            ui,
                                            "xd_hdr_raw",
                                            crate::tl!("Raw Value (Selectable & Editable) ⇔"),
                                            &mut raw_w,
                                            340.0,
                                            1200.0,
                                        );
                                        val_w = (raw_w - key_w).max(200.0);
                                    }
                                    ui.label(RichText::new(crate::tl!("Actions")).strong());
                                    ui.end_row();

                                    let mut vis_num = 0usize;
                                    for (i, r) in group.rows.iter_mut().enumerate() {
                                        if r.code == 1002 {
                                            continue;
                                        }
                                        vis_num += 1;
                                        if !filter_low.is_empty() {
                                            let match_k = r.key.to_ascii_lowercase().contains(&filter_low);
                                            let match_v = r.val.to_ascii_lowercase().contains(&filter_low);
                                            if !match_k && !match_v {
                                                continue;
                                            }
                                        }
                                        ui.label(RichText::new(format!("{vis_num}")).monospace().color(t.text_dim));

                                        let code_label = CODES
                                            .iter()
                                            .find(|(c, _)| *c == r.code)
                                            .map(|(_, l)| *l)
                                            .unwrap_or("Custom");
                                        craft_ui::ComboBox::from_id_salt(("xd_code", i))
                                            .selected_text(code_label)
                                            .width(105.0)
                                            .show_ui(ui, |ui| {
                                                for &(c, lbl) in CODES {
                                                    if ui.selectable_value(&mut r.code, c, lbl).changed() {
                                                        r.original_raw = None;
                                                        row_changed = true;
                                                    }
                                                }
                                            });

                                        if smart_kv {
                                            if ui
                                                .add_sized(
                                                    [key_w, 22.0],
                                                    egui::TextEdit::singleline(&mut r.key)
                                                        .id(egui::Id::unique(("xd_key", i)))
                                                        .desired_width(f32::INFINITY)
                                                        .hint_text("(key)"),
                                                )
                                                .changed()
                                            {
                                                r.original_raw = None;
                                                row_changed = true;
                                            }
                                            if ui
                                                .add_sized(
                                                    [val_w, 22.0],
                                                    egui::TextEdit::singleline(&mut r.val)
                                                        .id(egui::Id::unique(("xd_val", i)))
                                                        .desired_width(f32::INFINITY),
                                                )
                                                .changed()
                                            {
                                                r.original_raw = None;
                                                row_changed = true;
                                            }
                                        } else {
                                            let mut combined = r.to_raw_value(false);
                                            if ui
                                                .add_sized(
                                                    [key_w + val_w, 22.0],
                                                    egui::TextEdit::singleline(&mut combined)
                                                        .id(egui::Id::unique(("xd_raw", i)))
                                                        .desired_width(f32::INFINITY),
                                                )
                                                .changed()
                                            {
                                                r.key.clear();
                                                r.val = combined;
                                                r.original_raw = None;
                                                row_changed = true;
                                            }
                                        }

                                        ui.horizontal(|ui| {
                                            if ui.small_button("📋").on_hover_text(crate::tl!("Copy value to clipboard")).clicked() {
                                                ctx.copy_text(r.val.clone());
                                                copied_msg = Some(crate::tf!("✓ Copied: {val}", val = r.val.as_str()));
                                            }
                                            let can_up = i.checked_sub(1).is_some_and(|j| row_codes.get(j).is_some_and(|&c| c != 1002));
                                            if ui
                                                .add_enabled(can_up, egui::Button::new("⬆").small())
                                                .on_hover_text(crate::tl!("Move row up"))
                                                .clicked()
                                            {
                                                move_up = Some(i);
                                            }
                                            let can_down = row_codes.get(i + 1).is_some_and(|&c| c != 1002);
                                            if ui
                                                .add_enabled(can_down, egui::Button::new("⬇").small())
                                                .on_hover_text(crate::tl!("Move row down"))
                                                .clicked()
                                            {
                                                move_down = Some(i);
                                            }
                                            if ui.small_button("🗑").on_hover_text(crate::tl!("Delete this XData row")).clicked() {
                                                delete_row = Some(i);
                                            }
                                        });
                                        ui.end_row();
                                    }
                                });
                        });

                    st.key_col_width = key_w;
                    st.val_col_width = val_w;
                    if row_changed {
                        st.dirty = true;
                    }
                    if let Some(msg) = copied_msg {
                        st.status_msg = Some(msg);
                    }
                    if let Some(i) = move_up
                        && let Some(j) = i.checked_sub(1).filter(|j| group.rows.get(*j).is_some_and(|r| r.code != 1002))
                    {
                        group.rows.swap(j, i);
                        st.dirty = true;
                    }
                    if let Some(i) = move_down
                        && group.rows.get(i + 1).is_some_and(|r| r.code != 1002)
                    {
                        group.rows.swap(i, i + 1);
                        st.dirty = true;
                    }
                    if let Some(i) = delete_row {
                        group.rows.remove(i);
                        st.dirty = true;
                    }

                    ui.separator();
                    // 5. Add New Property Row Bar
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(crate::tl!("Add Row:")).strong());
                        let new_code_lbl = CODES.iter().find(|(c, _)| *c == st.new_code).map(|(_, l)| *l).unwrap_or("1000 String");
                        craft_ui::ComboBox::from_id_salt("xd_new_code")
                            .selected_text(new_code_lbl)
                            .width(105.0)
                            .show_ui(ui, |ui| {
                                for &(c, lbl) in CODES {
                                    ui.selectable_value(&mut st.new_code, c, lbl);
                                }
                            });
                        if st.new_code == 1000 && st.smart_kv {
                            ui.add_sized(
                                [key_w, 22.0],
                                egui::TextEdit::singleline(&mut st.new_key)
                                    .hint_text(crate::tl!("Key (e.g. OPMERKING)"))
                                    .desired_width(f32::INFINITY),
                            );
                        }
                        let val_resp = ui.add_sized(
                            [val_w.min(360.0), 22.0],
                            egui::TextEdit::singleline(&mut st.new_val)
                                .hint_text(crate::tl!("Value"))
                                .desired_width(f32::INFINITY),
                        );
                        let enter_pressed = val_resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                        if (ui.button(crate::tl!("+ Add Property")).clicked() || enter_pressed)
                            && (!st.new_key.trim().is_empty() || !st.new_val.trim().is_empty())
                        {
                            group.rows.push(XdRow {
                                code: st.new_code,
                                key: st.new_key.trim().to_string(),
                                val: st.new_val.trim().to_string(),
                                sep: "=".to_string(),
                                original_raw: None,
                            });
                            st.new_key.clear();
                            st.new_val.clear();
                            st.dirty = true;
                        }
                    });
                }
            }

            ui.separator();
            // 6. Footer Bar: Status + Select Next Object + Apply / Revert / Close
            ui.horizontal(|ui| {
                if external_changed {
                    ui.label(
                        RichText::new(crate::tl!("⚠ Entity XData changed outside editor — click Revert before saving."))
                            .color(egui::Color32::from_rgb(239, 68, 68)),
                    );
                } else if let Some(msg) = &st.status_msg {
                    ui.label(RichText::new(msg).color(t.text_dim));
                } else if st.dirty {
                    ui.label(RichText::new(crate::tl!("● Unsaved changes")).color(egui::Color32::from_rgb(245, 158, 11)));
                } else {
                    ui.label(
                        RichText::new(crate::tl!(
                            "Ready — click '🎯 Select Next Object' or '◀ Previous / Next ▶' to switch objects."
                        ))
                        .color(t.text_dim),
                    );
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(crate::tl!("Close")).clicked() {
                        close_clicked = true;
                    }
                    if ui
                        .add_enabled(st.dirty || external_changed, egui::Button::new(crate::tl!("↺ Revert")))
                        .clicked()
                    {
                        revert_changes = true;
                    }
                    if ui
                        .add_enabled(
                            st.dirty && !external_changed,
                            egui::Button::new(RichText::new(crate::tl!("✔ Apply / Save XData")).strong()),
                        )
                        .on_hover_text(crate::tl!("Write modified XData to the entity (Undoable with Ctrl+Z)"))
                        .clicked()
                    {
                        apply_changes = true;
                    }
                    let pick_label = if st.dirty {
                        crate::tl!("✔ Save & Select Next Object")
                    } else {
                        crate::tl!("🎯 Select Next Object")
                    };
                    if ui
                        .button(RichText::new(pick_label).strong())
                        .on_hover_text(crate::tl!("Save any changes and directly pick the next object in the drawing"))
                        .clicked()
                    {
                        start_pick_another = true;
                    }
                });
            });
        });

    if close_clicked {
        *open = false;
    }

    // Step to Previous / Next entity directly inside the dialog
    if let Some(dir) = step_dir
        && let Some(cur_h) = st.handle
    {
        if st.dirty {
            if external_changed {
                st.status_msg = Some(crate::tl!("⚠ Entity XData changed outside editor — click Revert before saving.").into());
                app.xdedit_dlg = Some(st);
                ctx.request_repaint();
                return;
            }
            let payload = json!({
                "handles": [cur_h.hex()],
                "xdata": st.to_xdata_json(),
            });
            if let Err(e) = app.run("xdedit", payload) {
                st.status_msg = Some(crate::tf!("Error: {err}", err = e));
                app.xdedit_dlg = Some(st);
                ctx.request_repaint();
                return;
            }
            st.dirty = false;
        }
        if let Some((next_h, idx1, total)) = step_entity(app, cur_h, &st.selection_list, dir)
            && let Ok(d) = app.session.doc()
            && let Some(e) = d.entity(next_h)
        {
            let kw = st.key_col_width;
            let vw = st.val_col_width;
            let skv = st.smart_kv;
            let flt = st.filter.clone();
            let list = st.selection_list.clone();
            let mut next = XdEditState::from_entity_with_list(e, list);
            next.key_col_width = next.key_col_width.max(kw);
            next.val_col_width = next.val_col_width.max(vw);
            next.smart_kv = skv;
            next.filter = flt;
            next.status_msg = Some(crate::tf!(
                "Object {idx1}/{total} loaded ({kind} — {handle})",
                idx1 = idx1,
                total = total,
                kind = next.entity_type.as_str(),
                handle = next_h.hex()
            ));
            app.session.set_selection(vec![next_h]);
            app.xdedit_dlg = Some(next);
            ctx.request_repaint();
            return;
        }
    }

    if start_pick_another {
        if st.dirty
            && let Some(h) = st.handle
        {
            if external_changed {
                st.status_msg = Some(crate::tl!("⚠ Entity XData changed outside editor — click Revert before saving.").into());
                app.xdedit_dlg = Some(st);
                ctx.request_repaint();
                return;
            }
            let payload = json!({
                "handles": [h.hex()],
                "xdata": st.to_xdata_json(),
            });
            match app.run("xdedit", payload) {
                Ok(_) => {
                    st.dirty = false;
                    if let Ok(d) = app.session.doc()
                        && let Some(e) = d.entity(h)
                    {
                        st.loaded_xdata = e.common.xdata.clone();
                    }
                }
                Err(e) => {
                    st.status_msg = Some(crate::tf!("Error: {err}", err = e));
                    app.xdedit_dlg = Some(st);
                    ctx.request_repaint();
                    return;
                }
            }
        }
        st.awaiting_pick = true;
        st.status_msg = None;
        app.xdedit_dlg = Some(st);
        app.ui.dialog = None;
        app.session.set_selection(Vec::new());
        if let Err(e) = app.session.start("xdedit") {
            app.session.echo(e.to_string());
        }
        ctx.request_repaint();
        return;
    }

    if revert_changes
        && let Some(h) = st.handle
        && let Ok(d) = app.session.doc()
        && let Some(e) = d.entity(h)
    {
        st.reload_from_entity(e);
        st.status_msg = Some(crate::tl!("↺ Reverted to saved entity XData").into());
    }

    if apply_changes && let Some(h) = st.handle {
        if external_changed {
            st.status_msg = Some(crate::tl!("⚠ Entity XData changed outside editor — click Revert before saving.").into());
        } else {
            let payload = json!({
                "handles": [h.hex()],
                "xdata": st.to_xdata_json(),
            });
            match app.run("xdedit", payload) {
                Ok(_) => {
                    st.dirty = false;
                    if let Ok(d) = app.session.doc()
                        && let Some(e) = d.entity(h)
                    {
                        st.loaded_xdata = e.common.xdata.clone();
                    }
                    st.status_msg = Some(crate::tl!("✔ XData saved to entity (Undoable)").into());
                }
                Err(e) => {
                    st.status_msg = Some(crate::tf!("Error: {err}", err = e));
                }
            }
        }
    }

    app.xdedit_dlg = Some(st);
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadcraft_doc::{Common, EntityKind, Line};
    use cadcraft_geom::Vec3;

    #[test]
    fn xdedit_state_preserves_1002_control_strings_and_formats_csv() {
        let common = Common {
            xdata: vec![
                RawTag { code: 1001, value: "CAD_XDATA".into() },
                RawTag { code: 1002, value: "{".into() },
                RawTag { code: 1000, value: "THEMA=Middenspanning".into() },
                RawTag { code: 1002, value: "}".into() },
            ],
            ..Default::default()
        };
        let mut e = Entity {
            handle: Handle(42),
            common,
            kind: EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(10.0, 0.0, 0.0) }),
        };
        let mut st = XdEditState::from_entity(&e);
        assert_eq!(st.loaded_xdata, e.common.xdata);
        let json_out = st.to_xdata_json();
        let arr = json_out.as_array().cloned().unwrap_or_default();
        assert_eq!(arr.len(), 4);
        assert_eq!(arr[1]["code"], 1002);
        assert_eq!(arr[1]["value"], "{");
        assert_eq!(arr[3]["code"], 1002);
        assert_eq!(arr[3]["value"], "}");

        let csv = st.format_plain_text();
        assert_eq!(csv, "Object,LINE\nTHEMA,Middenspanning");

        // External entity XData modification is detected via loaded_xdata and reloaded cleanly.
        e.common.xdata = vec![
            RawTag { code: 1001, value: "CAD_XDATA".into() },
            RawTag { code: 1000, value: "THEMA=Laagspanning".into() },
        ];
        assert_ne!(st.loaded_xdata, e.common.xdata);
        st.reload_from_entity(&e);
        assert_eq!(st.loaded_xdata, e.common.xdata);
        assert_eq!(st.format_plain_text(), "Object,LINE\nTHEMA,Laagspanning");
    }

    #[test]
    fn unchanged_rows_preserve_original_whitespace_until_edited() {
        let tag = RawTag { code: 1000, value: "  NOTE = padded value  ".into() };
        let mut row = XdRow::from_tag(&tag);
        assert_eq!(row.to_raw_value(true), "  NOTE = padded value  ");
        assert_eq!(row.to_raw_value(false), "  NOTE = padded value  ");

        // Editing in Smart Key=Value mode clears original_raw and formats cleanly.
        row.val = "updated".into();
        row.original_raw = None;
        assert_eq!(row.to_raw_value(true), "NOTE=updated");
    }
}
