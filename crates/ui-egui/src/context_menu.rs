//! The canvas's right-click menus, after AutoCAD's shortcut menus:
//!
//! - **No command running** ([`entries`]): Repeat the last command, Recent Commands, the
//!   Clipboard and Isolate; with a selection the editing commands, Draw Order, Group, the edit
//!   commands of the selected object type, Select Similar and Deselect All; then Undo/Redo,
//!   Pan/Zoom, Quick Select, Find, Properties and Options.
//! - **While a command runs** ([`command_entries`]): a quick right-click is Enter; holding the
//!   button for [`HOLD_SECS`] opens Enter, Cancel, Recent Input, the prompt's options, Snap
//!   Overrides and Pan/Zoom instead (AutoCAD's time-sensitive right-click). With that turned off
//!   in OPTIONS (`timeSensitiveRightClick`), every right-click opens the menu.
//! - **Shift/Ctrl+right-click** ([`snap_entries`]): the object snap menu. Its items type the point
//!   modifiers and snap overrides (`_from`, `_endp`, `_non`…) at the current point prompt.
//!
//! An item for a command appears only when that command is registered, so the menus pick up
//! commands as they land. Choosing an item ([`choose`]) starts its command like the menu bar, or
//! gives the running command the same input as typing it on the command line.

use cadcraft_doc::EntityKind;
use cadcraft_engine::snap::mode;
use cadcraft_engine::{Input, find_command};

use crate::CadApp;
use crate::menus::{Entry, activate, entry_ui};

/// Clipboard ▸ (AutoCAD's order).
const CLIPBOARD: &[&str] = &["cutclip", "copyclip", "copybase", "pasteclip", "pasteblock", "pasteorig"];
/// Isolate ▸ (object isolation commands).
const ISOLATE: &[&str] = &["isolateobjects", "hideobjects", "unisolateobjects"];
/// With a selection: commands that act on it, and its label when it differs from the command's.
const EDIT: &[(&str, Option<&str>)] = &[("erase", None), ("move", None), ("copy", Some("Copy Selection")), ("scale", None), ("rotate", None)];
/// Draw Order ▸.
const DRAW_ORDER: &[&str] = &["draworder.front", "draworder.back", "draworder.above", "draworder.under"];
const GROUP: &[&str] = &["group", "ungroup"];
/// With a selection: selection tools.
const SELECT: &[&str] = &["addselected", "selectsimilar", "ai_deselect"];
const VIEW: &[&str] = &["u", "redo", "pan", "zoom"];
const TOOLS: &[&str] = &["qselect", "find"];
/// Last in the menu: Properties with a selection, Options always.
const SETTINGS: &[&str] = &["options"];

/// How many recent commands and inputs the menus list.
pub const RECENT: usize = 8;
/// How long the right button must be held while a command runs to open the command menu instead
/// of acting as Enter (AutoCAD's time-sensitive right-click; 250 ms there by default).
pub const HOLD_SECS: f64 = 0.25;

/// Item ids that are not commands: keys, typed text and snap menu items.
pub const ENTER: &str = "key:enter";
pub const CANCEL: &str = "key:cancel";
/// Types the rest at the prompt (Recent Input, Pan/Zoom).
pub const TYPE: &str = "type:";
/// Gives the running command the keyword that follows.
pub const KEYWORD: &str = "keyword:";
/// An object snap menu item ([`choose_snap`]).
pub const SNAP: &str = "snap:";

fn item(app: &CadApp, id: &str, label: Option<&str>) -> Option<Entry> {
    let spec = find_command(id)?;
    Some(Entry::Item {
        label: label.unwrap_or(spec.label).to_string(),
        id: id.to_string(),
        shortcut: spec.shortcut.map(str::to_string),
        enabled: (spec.enabled)(&app.session).is_ok(),
    })
}

fn items(app: &CadApp, ids: &[&str]) -> Vec<Entry> {
    ids.iter().filter_map(|id| item(app, id, None)).collect()
}

/// A submenu, left out when none of its commands exist.
fn sub(label: &str, children: Vec<Entry>) -> Option<Entry> {
    (!children.is_empty()).then(|| Entry::Sub { label: label.into(), children })
}

/// Push a group unless it is empty.
fn push(groups: &mut Vec<Vec<Entry>>, group: Vec<Entry>) {
    if !group.is_empty() {
        groups.push(group);
    }
}

/// The idle menu's groups of entries (separated by lines).
pub fn entries(app: &CadApp) -> Vec<Vec<Entry>> {
    let mut groups = Vec::new();
    let mut first = Vec::new();
    if let Some(last) = app.session.last_command.as_deref().and_then(find_command) {
        let label = crate::tf!("Repeat {command}", command = last.id.to_ascii_uppercase());
        first.push(Entry::Item { label, id: last.id.to_string(), shortcut: None, enabled: (last.enabled)(&app.session).is_ok() });
    }
    let recent: Vec<Entry> = app
        .session
        .recent_commands(RECENT)
        .iter()
        .filter_map(|id| find_command(id))
        .map(|c| Entry::Item { label: c.id.to_ascii_uppercase(), id: c.id.to_string(), shortcut: None, enabled: (c.enabled)(&app.session).is_ok() })
        .collect();
    first.extend(sub("Recent Commands", recent));
    push(&mut groups, first);
    // In the Block Editor: save the block or go back to the drawing.
    if app.session.state().is_ok_and(|st| st.block_edit.is_some()) {
        push(&mut groups, items(app, &["bsave", "bsaveas", "bclose"]));
    }
    let mut clip: Vec<Entry> = sub("Clipboard", items(app, CLIPBOARD)).into_iter().collect();
    clip.extend(sub("Isolate", items(app, ISOLATE)));
    push(&mut groups, clip);
    let selected = !app.session.selection().is_empty();
    if selected {
        let mut edit: Vec<Entry> = EDIT.iter().filter_map(|(id, label)| item(app, id, *label)).collect();
        edit.extend(sub("Draw Order", items(app, DRAW_ORDER)));
        edit.extend(items(app, GROUP));
        push(&mut groups, edit);
        push(&mut groups, object_entries(app));
        push(&mut groups, items(app, SELECT));
    }
    push(&mut groups, items(app, VIEW));
    push(&mut groups, items(app, TOOLS));
    let mut last = if selected { items(app, &["properties"]) } else { Vec::new() };
    last.extend(items(app, SETTINGS));
    push(&mut groups, last);
    groups
}

/// The edit commands of the selected objects' type (when they are all of one type), with their
/// labels: Edit Text for text, Block Editor and Edit Attributes for block references, Hatch Edit,
/// Polyline Edit, Spline Edit…
fn object_commands(app: &CadApp) -> Vec<(&'static str, Option<&'static str>)> {
    let Ok(d) = app.session.doc() else { return Vec::new() };
    let sel = app.session.selection();
    let ents: Vec<_> = sel.iter().filter_map(|h| d.entity(*h)).collect();
    let Some(first) = ents.first() else { return Vec::new() };
    let ty = first.kind.type_name();
    if ents.iter().any(|e| e.kind.type_name() != ty) {
        return Vec::new();
    }
    match &first.kind {
        EntityKind::Text(_) | EntityKind::MText(_) | EntityKind::AttDef(_) => vec![("textedit", None)],
        EntityKind::Insert(_) => {
            let mut v = vec![("bedit", None), ("refedit", None)];
            if ents.iter().any(|e| matches!(&e.kind, EntityKind::Insert(i) if i.attribs.iter().any(|a| !a.constant))) {
                v.push(("attedit", Some("Edit Attributes...")));
            }
            v
        }
        EntityKind::Hatch(_) => vec![("hatchedit", None)],
        EntityKind::LwPolyline(_) | EntityKind::Polyline3d(_) => vec![("pedit", Some("Polyline Edit"))],
        EntityKind::Spline(_) => vec![("splinedit", Some("Spline Edit"))],
        EntityKind::Dimension(_) => vec![("dimtedit", None)],
        EntityKind::MLeader(_) => vec![("mleaderedit", None)],
        EntityKind::Viewport(_) => vec![("vpclip", None)],
        EntityKind::Image(_) => vec![("imageclip", None)],
        _ => Vec::new(),
    }
}

/// [`object_commands`] that are registered, as menu items.
pub fn object_entries(app: &CadApp) -> Vec<Entry> {
    object_commands(app).into_iter().filter_map(|(id, label)| item(app, id, label)).collect()
}

/// The command menu while a command runs (`None` when none does): Enter and Cancel, Recent Input,
/// the prompt's options, Snap Overrides at point prompts, Pan and Zoom.
pub fn command_entries(app: &CadApp) -> Option<Vec<Vec<Entry>>> {
    let prompt = app.session.current_prompt()?;
    let typed = |label: &str, id: String| Entry::Item { label: label.into(), id, shortcut: None, enabled: true };
    let mut groups = vec![vec![typed("Enter", ENTER.into()), typed("Cancel", CANCEL.into())]];
    let recent: Vec<Entry> = app.cmd.recent_input.iter().rev().take(RECENT).map(|t| typed(t, format!("{TYPE}{t}"))).collect();
    push(&mut groups, sub("Recent Input", recent).into_iter().collect());
    push(&mut groups, prompt.keywords.iter().map(|k| typed(k, format!("{KEYWORD}{k}"))).collect());
    if cadcraft_engine::pointmod::accepts(&prompt) {
        let mut snaps = Vec::new();
        for g in snap_entries(app) {
            for e in g {
                snaps.push(prefix_ids(e, SNAP));
            }
        }
        push(&mut groups, sub("Snap Overrides", snaps).into_iter().collect());
    }
    // Typed transparently ('zoom.extents), so the command keeps running.
    let view = |label: &str, ids: &[&str]| {
        let children = ids.iter().filter_map(|id| item(app, id, None)).map(|e| prefix_ids(e, &format!("{TYPE}'"))).collect();
        sub(label, children)
    };
    let pan = view("Pan", &["pan.left", "pan.right", "pan.up", "pan.down"]);
    let zoom = view("Zoom", &["zoom.extents", "zoom.all", "zoom.previous", "zoom.in", "zoom.out"]);
    push(&mut groups, pan.into_iter().chain(zoom).collect());
    Some(groups)
}

fn prefix_ids(e: Entry, prefix: &str) -> Entry {
    match e {
        Entry::Item { label, id, shortcut, enabled } => Entry::Item { label, id: format!("{prefix}{id}"), shortcut, enabled },
        Entry::Sub { label, children } => Entry::Sub { label, children: children.into_iter().map(|c| prefix_ids(c, prefix)).collect() },
    }
}

/// Run a chosen item of any of the canvas menus.
pub fn choose(app: &mut CadApp, id: &str) {
    if id == ENTER {
        app.cmd.buffer.clear();
        app.cmdline("");
    } else if id == CANCEL {
        // Escape.
        app.cmd.buffer.clear();
        app.session.cancel();
    } else if let Some(text) = id.strip_prefix(TYPE) {
        app.cmd.buffer.clear();
        app.cmdline(text);
    } else if let Some(k) = id.strip_prefix(KEYWORD) {
        app.cmd.buffer.clear();
        if let Err(e) = app.session.input(Input::Keyword(k.to_string())) {
            app.session.echo(e.to_string());
        }
    } else if let Some(s) = id.strip_prefix(SNAP) {
        choose_snap(app, s);
    } else {
        activate(app, id);
    }
}

/// Remember a line typed at the command line for Recent Input (only lines typed at a command's
/// prompts; command names are the history's).
pub fn note_input(app: &mut CadApp, text: &str) {
    let t = text.trim();
    if !app.session.prompting() || t.is_empty() {
        return;
    }
    let recent = &mut app.cmd.recent_input;
    recent.retain(|r| r != t);
    recent.push(t.to_string());
    if recent.len() > RECENT {
        recent.remove(0);
    }
}

/// The object snap menu's groups: (label, what it types) per item; the label of a snap mode is
/// its name in [`mode::ALL`].
const SNAP_GROUPS: &[&[(u32, &str)]] = &[
    &[(mode::END, "_endp"), (mode::MID, "_mid"), (mode::INT, "_int"), (mode::APP, "_app"), (mode::EXT, "_ext")],
    &[(mode::CEN, "_cen"), (mode::GCEN, "_gcen"), (mode::QUA, "_qua"), (mode::TAN, "_tan")],
    &[(mode::PER, "_per"), (mode::PAR, "_par"), (mode::NOD, "_nod"), (mode::INS, "_ins"), (mode::NEA, "_nea")],
];
/// The point filters submenu.
const FILTERS: &[&str] = &[".X", ".Y", ".Z", ".XY", ".XZ", ".YZ"];
/// The id of the Osnap Settings item (the Drafting Settings dialog).
const OSNAP_SETTINGS: &str = "dsettings";

/// The object snap menu (Shift/Ctrl+right-click): Temporary track point, From, Mid Between 2
/// Points, Point Filters, the snap modes, None and Osnap Settings. An item's id is the text it
/// types; the modifiers are only available at a point prompt.
pub fn snap_entries(app: &CadApp) -> Vec<Vec<Entry>> {
    let on = app.session.current_prompt().is_some_and(|p| cadcraft_engine::pointmod::accepts(&p));
    let typed = |label: &str, id: &str| Entry::Item { label: label.into(), id: id.into(), shortcut: None, enabled: on };
    let name = |m: u32| mode::ALL.iter().find(|(b, _)| *b == m).map_or("", |(_, n)| *n);
    let mut groups = vec![vec![
        typed("Temporary track point", "_tt"),
        typed("From", "_from"),
        typed("Mid Between 2 Points", "_m2p"),
        Entry::Sub { label: "Point Filters".into(), children: FILTERS.iter().map(|f| typed(f, &f.to_ascii_lowercase())).collect() },
    ]];
    groups.extend(SNAP_GROUPS.iter().map(|g| g.iter().map(|(m, id)| typed(name(*m), id)).collect()));
    groups.push(vec![typed("None", "_non")]);
    groups.push(vec![Entry::Item { label: "Osnap Settings...".into(), id: OSNAP_SETTINGS.into(), shortcut: None, enabled: true }]);
    groups
}

/// Run an item of the object snap menu: type its modifier at the prompt, or open the settings.
pub fn choose_snap(app: &mut CadApp, id: &str) {
    if id == OSNAP_SETTINGS {
        activate(app, id);
    } else {
        app.cmdline(id);
    }
}

/// Lay out menu groups, separated by lines; the chosen item's id lands in `clicked`.
pub fn groups_ui(ui: &mut egui::Ui, groups: &[Vec<Entry>], clicked: &mut Option<String>) {
    for (i, group) in groups.iter().enumerate() {
        if i > 0 {
            ui.separator();
        }
        for e in group {
            entry_ui(ui, e, clicked);
        }
    }
}

/// Attach the menu to the canvas response: it opens on a right-click, as the object snap menu
/// when that click held Shift or Ctrl (`CanvasState::snap_menu`).
pub fn show(app: &mut CadApp, resp: &egui::Response) {
    let mut clicked = None;
    let snap = app.canvas.snap_menu;
    resp.context_menu(|ui| {
        let groups = if snap { snap_entries(app) } else { entries(app) };
        groups_ui(ui, &groups, &mut clicked);
    });
    match clicked {
        Some(id) if snap => choose_snap(app, &id),
        Some(id) => choose(app, &id),
        None => {}
    }
}

/// When the right button went down on the canvas: (time, position).
#[derive(Clone, Copy, Default)]
struct Press(f64, egui::Pos2);

/// The canvas's right button while a command runs (Shift/Ctrl+right-click is the snap menu's): a
/// quick click is Enter — returns `true` when the caller should send it — and a click held for
/// [`HOLD_SECS`] opens the command menu ([`command_entries`]) at the pointer (any click does with
/// the time-sensitive right-click turned off). Call it every frame
/// with whether the pointer is on the canvas and whether a right-click ended there; it also shows
/// the menu while it is open.
pub fn in_command(app: &mut CadApp, ui: &egui::Ui, inside: bool, right_clicked: bool) -> bool {
    let ctx = ui.ctx().clone();
    let press_id = egui::Id::unique("cc_command_menu_press");
    let popup_id = egui::Id::unique("cc_command_menu");
    let secondary = egui::PointerButton::Secondary;
    let (pressed, released, now, pos) =
        ctx.input(|i| (i.pointer.button_pressed(secondary), i.pointer.button_released(secondary), i.time, i.pointer.interact_pos()));
    if pressed
        && inside
        && let Some(p) = pos
    {
        ctx.data_mut(|d| d.insert_temp(press_id, Press(now, p)));
    }
    let running = app.session.prompting() && !app.canvas.snap_menu;
    let mut open = false;
    if released && let Some(Press(t0, p0)) = ctx.data_mut(|d| d.remove_temp::<Press>(press_id)) {
        // A drag isn't a click; a click held long enough asks for the menu.
        let still = pos.is_some_and(|p| p.distance(p0) <= 6.0);
        let held = now - t0 >= HOLD_SECS || !app.options.time_sensitive_right_click;
        open = running && still && held;
    }
    let enter = running && inside && right_clicked && !open && app.options.time_sensitive_right_click;
    if !running {
        return false;
    }
    let mut clicked = None;
    let popup = egui::Popup::new(popup_id, ctx, egui::PopupAnchor::PointerFixed, ui.layer_id())
        .kind(egui::PopupKind::Menu)
        .layout(egui::Layout::top_down_justified(egui::Align::Min))
        .style(egui::containers::menu::menu_style)
        .open_memory(open.then_some(egui::SetOpenCommand::Bool(true)));
    popup.show(|ui| {
        if let Some(groups) = command_entries(app) {
            groups_ui(ui, &groups, &mut clicked);
        }
    });
    if let Some(id) = clicked {
        choose(app, &id);
    }
    enter
}
