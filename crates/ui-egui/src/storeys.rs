//! Storeys in the interface: the storey navigator in the Layers palette, the storey and blueprint
//! choosers, and the storey setup in Options.
//!
//! The navigator lists the storeys top to bottom. A click goes to a storey; the blueprint mark in
//! front of a storey shows (and switches) the one drawn beneath the current storey; a right-click
//! offers the rest (blueprint, new storey above or below, move, delete). Everything runs the
//! engine's `storey.*` commands, like the Layers section runs `layer.*`.
//!
//! The storey commands' shortcuts are ordinary command shortcuts: built in
//! (`menus::builtin_shortcuts`) and changed in the Keyboard Shortcuts dialog like any other.

use egui::{Key, Modifiers, RichText, vec2};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::CadApp;
use crate::icons::{self, Icon};
use crate::theme::Tokens;

// ---------------- names: storeys or blueprints ----------------

/// What the levels are called: storeys (buildings) or blueprints (any other stack: versions,
/// variants). Only the words change.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Naming {
    #[default]
    Storeys,
    Blueprints,
}

thread_local! {
    static NAMING: std::cell::Cell<Naming> = const { std::cell::Cell::new(Naming::Storeys) };
}

/// The words for storeys and their blueprint-stack counterparts. "Blueprint" itself already names
/// the level shown beneath, so a level of a blueprint stack is a "Level".
pub(crate) const TERMS: &[(&str, &str)] = &[
    ("Storeys", "Blueprints"),
    ("Storeys Palette", "Blueprints Palette"),
    ("Storey Settings...", "Blueprint Settings..."),
    ("Storey", "Level"),
    ("New Storey", "New Level"),
    ("New Storey Above", "New Level Above"),
    ("New Storey Below", "New Level Below"),
    ("Go to Storey", "Go to Level"),
    ("Go to Storey...", "Go to Level..."),
    ("Storey Above", "Level Above"),
    ("Storey Below", "Level Below"),
    ("Copy to Storey", "Copy to Level"),
    ("Move to Storey", "Move to Level"),
    ("Copy to Storey...", "Copy to Level..."),
    ("Move to Storey...", "Move to Level..."),
    ("Delete Storey", "Delete Level"),
    ("Set as Start Storey", "Set as Start Level"),
    ("Import Plan as Storey", "Import Plan as Level"),
    ("Storeys from Setup", "Levels from Setup"),
    ("Blueprint Storey Above", "Blueprint Level Above"),
    ("Blueprint Storey Below", "Blueprint Level Below"),
    ("Storey name", "Level name"),
    ("Storey number", "Level number"),
    ("Start storey (number 0)", "Start level (number 0)"),
    ("Start storey", "Start level"),
    ("Storeys, top first, one per line", "Levels, top first, one per line"),
    ("The storey to work on", "The level to work on"),
    ("The storey shown beneath the current one", "The level shown beneath the current one"),
    ("Each storey remembers its blueprint", "Each level remembers its blueprint"),
    ("Select the objects first, then choose the storey.", "Select the objects first, then choose the level."),
    ("No storeys yet: the first one takes everything drawn so far.", "No levels yet: the first one takes everything drawn so far."),
    ("New drawings, and opened drawings without storeys, get these storeys", "New drawings, and opened drawings without levels, get these levels"),
    ("The drawing has storeys already", "The drawing has levels already"),
    (
        "Show hatches in the blueprint (off: hatches on the current storey only)",
        "Show hatches in the blueprint (off: hatches on the current level only)",
    ),
    ("Everything drawn so far goes on the start storey", "Everything drawn so far goes on the start level"),
    ("Only an empty storey can be deleted", "Only an empty level can be deleted"),
    ("Above the current storey", "Above the current level"),
];

/// `source` in the words the user chose (English; translate after).
pub fn term(source: &str) -> &str {
    if NAMING.with(std::cell::Cell::get) == Naming::Blueprints
        && let Some((_, b)) = TERMS.iter().find(|(a, _)| *a == source)
    {
        return b;
    }
    source
}

/// `source` in the user's words and language.
fn w(source: &'static str) -> &'static str {
    crate::i18n::t(term(source))
}

// ---------------- settings ----------------

/// The user's storey setup: which storeys new drawings, and opened drawings without storeys, get.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct StoreySetup {
    /// Give new drawings and drawings without storeys the template's storeys.
    pub auto: bool,
    pub template: cadcraft_doc::StoreyTemplate,
    /// The blueprint colour on a dark drawing area.
    pub blueprint_dark: [u8; 3],
    /// The blueprint colour on a light drawing area.
    pub blueprint_light: [u8; 3],
    /// Hatches show in the blueprint; off, hatches show on the current storey only.
    pub blueprint_hatches: bool,
    /// Each storey remembers its blueprint.
    pub blueprint_per_storey: bool,
    /// What the levels are called in the interface.
    pub naming: Naming,
}

impl Default for StoreySetup {
    fn default() -> Self {
        let c = cadcraft_doc::DEFAULT_GHOST_COLOR;
        StoreySetup {
            auto: true,
            template: cadcraft_doc::StoreyTemplate::default(),
            blueprint_dark: [c.0, c.1, c.2],
            blueprint_light: [0xa8, 0x90, 0x60],
            blueprint_hatches: true,
            blueprint_per_storey: true,
            naming: Naming::Storeys,
        }
    }
}

/// The blueprint colour for the drawing area as it is now: light or dark, whichever theme or
/// background colour made it so.
fn blueprint_color(setup: &StoreySetup) -> cadcraft_color::Rgb {
    let light = crate::theme::luminance(Tokens::get().canvas) > 0.5;
    let [r, g, b] = if light { setup.blueprint_light } else { setup.blueprint_dark };
    cadcraft_color::Rgb(r, g, b)
}

/// The blueprint takes the user's colour for the background shown. The colour is a view, not an
/// edit: a drawing without changes stays without changes.
fn sync_blueprint(app: &mut CadApp) {
    let want = blueprint_color(&app.ui.storey_setup);
    let hatches = app.ui.storey_setup.blueprint_hatches;
    let per_storey = app.ui.storey_setup.blueprint_per_storey;
    let Ok(st) = app.session.state_mut() else { return };
    let d = &st.doc;
    if (d.ghost_color == want && d.ghost_hatches == hatches && d.ghost_per_storey == per_storey) || st.block_edit.is_some() {
        return;
    }
    let clean = !st.is_dirty();
    let d = std::sync::Arc::make_mut(&mut st.doc);
    d.ghost_color = want;
    d.ghost_hatches = hatches;
    d.ghost_per_storey = per_storey;
    if clean {
        st.saved = st.doc.clone();
    }
    st.revision += 1;
}

/// Hand the setup to the engine, which applies it as drawings are made and opened.
pub fn sync(app: &mut CadApp) {
    NAMING.with(|n| n.set(app.ui.storey_setup.naming));
    sync_blueprint(app);
    let want = app.ui.storey_setup.auto.then(|| app.ui.storey_setup.template.clone());
    if app.session.storey_template == want {
        return;
    }
    app.session.storey_template = want.clone();
    // The drawing the app opened with came before the setup was handed over: while it is untouched
    // (new, empty, never edited) it gets the setup now, as a change of nothing.
    if let (Some(t), Ok(st)) = (want, app.session.state_mut())
        && st.path.is_none()
        && st.undo.is_empty()
        && st.redo.is_empty()
        && !st.is_dirty()
        && st.doc.model.is_empty()
        && !st.doc.storeys.active()
    {
        let d = std::sync::Arc::make_mut(&mut st.doc);
        if d.apply_storey_template(&t) {
            st.saved = st.doc.clone();
            st.revision += 1;
        }
    }
}

/// The Storeys tab of Options: the start setup and the blueprint; the storey shortcuts are in the
/// Keyboard Shortcuts dialog, a click away.
pub(crate) fn options_tab(app: &mut CadApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    ui.heading(crate::tl!("Start Setup"));
    let mut setup = app.ui.storey_setup.clone();
    ui.checkbox(&mut setup.auto, w("New drawings, and opened drawings without storeys, get these storeys"));
    ui.horizontal(|ui| {
        ui.label(crate::tl!("Names"));
        ui.selectable_value(&mut setup.naming, Naming::Storeys, crate::tl!("Storeys (buildings)"));
        ui.selectable_value(&mut setup.naming, Naming::Blueprints, crate::tl!("Blueprints and levels (any other stack)"));
    });
    ui.horizontal(|ui| {
        // One name per line, the top storey first.
        let text_id = egui::Id::unique("storey_setup_text");
        let mut text: String = ui.data(|m| m.get_temp(text_id)).unwrap_or_else(|| setup.template.names.join("\n"));
        ui.vertical(|ui| {
            ui.label(RichText::new(w("Storeys, top first, one per line")).color(t.text_dim).small());
            if ui.add(egui::TextEdit::multiline(&mut text).desired_rows(7).desired_width(200.0)).changed() {
                setup.template.names = text.lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_string).collect();
            }
        });
        ui.data_mut(|m| m.insert_temp(text_id, text));
        ui.vertical(|ui| {
            ui.label(RichText::new(w("Start storey (number 0)")).color(t.text_dim).small());
            craft_ui::ComboBox::from_id_salt("storey_setup_start").selected_text(setup.template.start.clone()).show_ui(ui, |ui| {
                for n in setup.template.names.clone() {
                    ui.selectable_value(&mut setup.template.start, n.clone(), n);
                }
            });
            ui.add_space(6.0);
            let mut preset = None;
            if ui.button(crate::tl!("Building")).clicked() {
                preset = Some(cadcraft_doc::StoreyTemplate::default());
                setup.naming = Naming::Storeys;
            }
            if ui.button(crate::tl!("Neutral")).on_hover_text(crate::tl!("Drawing 0, drawings above, blueprints below")).clicked() {
                preset = Some(cadcraft_doc::StoreyTemplate::blueprints());
                setup.naming = Naming::Blueprints;
            }
            if let Some(p) = preset {
                ui.data_mut(|m| m.insert_temp(egui::Id::unique("storey_setup_text"), p.names.join("\n")));
                setup.template = p;
            }
            ui.add_space(6.0);
            let bare = app.session.doc().is_ok_and(|d| !d.storeys.active());
            if ui
                .add_enabled(bare, egui::Button::new(crate::tl!("Apply to This Drawing")))
                .on_disabled_hover_text(w("The drawing has storeys already"))
                .clicked()
            {
                let p = json!({ "names": setup.template.names, "start": setup.template.start });
                let _ = app.run("storey.template", p);
            }
        });
    });
    if !setup.template.names.iter().any(|n| cadcraft_doc::same_storey_name(n, &setup.template.start)) {
        setup.template.start = setup.template.names.first().cloned().unwrap_or_default();
    }
    if setup != app.ui.storey_setup {
        app.ui.storey_setup = setup;
    }
    ui.add_space(10.0);
    ui.heading(crate::tl!("Blueprint"));
    let mut setup = app.ui.storey_setup.clone();
    for (label, c) in
        [(crate::tl!("On a dark drawing area"), &mut setup.blueprint_dark), (crate::tl!("On a light drawing area"), &mut setup.blueprint_light)]
    {
        ui.horizontal(|ui| {
            ui.color_edit_button_srgb(c);
            ui.label(label);
        });
    }
    ui.checkbox(&mut setup.blueprint_per_storey, w("Each storey remembers its blueprint"));
    ui.checkbox(&mut setup.blueprint_hatches, w("Show hatches in the blueprint (off: hatches on the current storey only)"));
    if setup != app.ui.storey_setup {
        app.ui.storey_setup = setup;
    }
    ui.add_space(10.0);
    ui.heading(crate::tl!("Shortcuts"));
    if ui.button(crate::tl!("Keyboard Shortcuts...")).clicked() {
        app.ui.dialog = Some(crate::keys::DIALOG.into());
    }
}

// ---------------- the navigator ----------------

/// One storey row as the navigator shows it.
struct Row {
    name: String,
    /// Its number: 0 the start storey, counting up above and down below.
    number: i64,
    start: bool,
    current: bool,
    ghost: bool,
    objects: u64,
}

pub(crate) fn storeys_section(app: &mut CadApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    crate::palettes::section_header(ui, term("Storeys"));
    let Ok(st) = app.session.state() else { return };
    let d = &st.doc;
    // Objects selected on the current storey: a right-click on another storey moves or copies them there.
    let selected = st.selection.len();
    // Objects per storey, counted again only when the drawing changed.
    let key = (std::sync::Arc::as_ptr(d) as usize, st.revision);
    let count_id = ui.scope_id().with("storey_counts");
    let counts: std::sync::Arc<std::collections::HashMap<u32, u64>> = match ui.data(|m| m.get_temp::<((usize, u64), std::sync::Arc<_>)>(count_id)) {
        Some((k, c)) if k == key => c,
        _ => {
            let mut c = std::collections::HashMap::new();
            for e in d.model.iter() {
                if let Some(id) = d.storey_of(e) {
                    *c.entry(id).or_insert(0u64) += 1;
                }
            }
            let c = std::sync::Arc::new(c);
            ui.data_mut(|m| m.insert_temp(count_id, (key, c.clone())));
            c
        }
    };
    let rows: Vec<Row> = d
        .storeys
        .list
        .iter()
        .rev()
        .map(|s| Row {
            name: s.name.clone(),
            number: d.storeys.number(s.id).unwrap_or(0),
            start: s.id == d.storeys.home,
            current: s.id == d.storeys.current,
            ghost: d.ghost_storey == Some(s.id),
            objects: counts.get(&s.id).copied().unwrap_or(0),
        })
        .collect();
    let count = rows.len();
    let mut act: Option<(&str, Value)> = None;
    let mut open_dialog: Option<&str> = None;
    // The storey whose name is being typed over in place: (its name before, the text so far,
    // whether the field has had its focus and selection yet).
    let edit_id = ui.scope_id().with("storey_rename");
    let mut editing: Option<(String, String, bool)> = ui.data_mut(|d| d.get_temp(edit_id));
    // A storey made here gets its name typed at once.
    let mut name_new: Option<String> = None;
    // A storey renamed in place this frame.
    let mut rename: Option<(Value, bool)> = None;
    // A field whose storey is gone (undo, renamed elsewhere, another drawing) is dropped.
    if editing.as_ref().is_some_and(|e| !rows.iter().any(|r| r.name == e.0)) {
        editing = None;
    }
    if rows.is_empty() {
        ui.horizontal(|ui| {
            ui.add_space(10.0);
            ui.label(RichText::new(w("No storeys yet: the first one takes everything drawn so far.")).color(t.text_dim).small());
        });
        ui.horizontal(|ui| {
            ui.add_space(8.0);
            if ui.button(w("Storeys from Setup")).on_hover_text(w("Everything drawn so far goes on the start storey")).clicked() {
                // The user's setup, whether new drawings get it or not.
                let t = &app.ui.storey_setup.template;
                act = Some(("storey.template", json!({ "names": t.names, "start": t.start })));
            }
        });
    }
    for (row, r) in rows.iter().enumerate() {
        // Position from the bottom (the list shows the top first).
        let index = (count - 1 - row) as u64;
        ui.horizontal(|ui| {
            ui.add_space(6.0);
            // The blueprint mark: lit on the storey drawn beneath; a click switches it.
            if r.current {
                ui.add_space(22.0);
            } else {
                let tip = if r.ghost { "Shown as blueprint (click to hide)" } else { "Show as Blueprint" };
                let resp = ui.scope(|ui| {
                    if !r.ghost {
                        ui.multiply_opacity(0.3);
                    }
                    icons::button(ui, Icon::Transparency, 20.0, tip, r.ghost)
                });
                if resp.inner.clicked() {
                    act = Some(("storey.ghost", json!({ "name": if r.ghost { Value::Null } else { json!(r.name) } })));
                }
            }
            // The storey's number, fixed by its place (0: the start storey).
            let num = RichText::new(format!("{:>3}", r.number)).monospace().color(if r.start { t.accent } else { t.text_dim });
            ui.add_sized(vec2(26.0, 20.0), egui::Label::new(num)).on_hover_text(if r.start { w("Start storey") } else { w("Storey number") });
            let width = (ui.available_width() - 40.0).max(20.0);
            // Renaming in place: Enter or a click elsewhere keeps the name, Esc keeps the old one.
            if let Some((before, text, ready)) = editing.as_mut().filter(|e| e.0 == r.name) {
                let resp = ui.add_sized(
                    vec2(width, 20.0),
                    egui::TextEdit::singleline(text).id(egui::Id::unique(("storey_rename_field", &r.name))).hint_text(w("Storey name")),
                );
                if !*ready {
                    // Focus until the field has it (the command line takes typing from a field without focus),
                    // then the offered name selected once, so typing replaces it.
                    if !resp.has_focus() {
                        resp.request_focus();
                    } else {
                        if let Some(mut state) = egui::text_edit::TextEditState::load(ui.ctx(), resp.id) {
                            let all = egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(text.chars().count()));
                            state.cursor.set_char_range(Some(all));
                            state.store(ui.ctx(), resp.id);
                        }
                        *ready = true;
                    }
                } else if ui.input(|i| i.key_pressed(Key::Escape)) {
                    editing = None;
                } else if resp.lost_focus() {
                    let name = text.trim().to_string();
                    if !name.is_empty() && name != *before {
                        // Run before anything else this frame. Entered: the field stays until the
                        // name took; a click elsewhere never pulls the focus back.
                        let enter = ui.input(|i| i.key_pressed(Key::Enter));
                        rename = Some((json!({ "name": before.clone(), "newName": name }), enter));
                    } else {
                        editing = None;
                    }
                }
                ui.label(RichText::new(r.objects.to_string()).color(t.text_dim).small());
                return;
            }
            let label = if r.current { RichText::new(&r.name).strong() } else { RichText::new(&r.name) };
            let resp =
                ui.add_sized(vec2(width, 20.0), egui::Button::selectable(r.current, label)).on_hover_text(crate::tl!("Double-click to rename"));
            if resp.double_clicked() {
                editing = Some((r.name.clone(), r.name.clone(), false));
            } else if resp.clicked() && !r.current {
                act = Some(("storey.current", json!({ "name": r.name })));
            }
            resp.context_menu(|ui| {
                if !r.current && selected > 0 {
                    ui.label(RichText::new(format!("{selected} {}", crate::tl!("selected"))).color(t.text_dim).small());
                    if ui.button(crate::tl!("Move Selection Here")).clicked() {
                        act = Some(("storeymove", json!({ "to": r.name })));
                    }
                    if ui.button(crate::tl!("Copy Selection Here")).clicked() {
                        act = Some(("storeycopy", json!({ "to": r.name })));
                    }
                    ui.separator();
                }
                if !r.current {
                    if ui.button(w("Go to Storey")).clicked() {
                        act = Some(("storey.current", json!({ "name": r.name })));
                    }
                    let b = if r.ghost { crate::tl!("Hide Blueprint") } else { crate::tl!("Show as Blueprint") };
                    if ui.button(b).clicked() {
                        act = Some(("storey.ghost", json!({ "name": if r.ghost { Value::Null } else { json!(r.name) } })));
                    }
                    ui.separator();
                }
                if !r.start && ui.button(w("Set as Start Storey")).clicked() {
                    act = Some(("storey.home", json!({ "name": r.name })));
                }
                if ui.button(crate::tl!("Rename")).clicked() {
                    editing = Some((r.name.clone(), r.name.clone(), false));
                    ui.close();
                }
                ui.separator();
                let fresh = fresh_name(&rows);
                if ui.button(w("New Storey Above")).clicked() {
                    act = Some(("storey.new", json!({ "name": fresh, "index": index + 1 })));
                    name_new = Some(fresh.clone());
                }
                if ui.button(w("New Storey Below")).clicked() {
                    act = Some(("storey.new", json!({ "name": fresh, "index": index })));
                    name_new = Some(fresh.clone());
                }
                ui.separator();
                if ui.add_enabled(row > 0, egui::Button::new(crate::tl!("Move Up"))).clicked() {
                    act = Some(("storey.order", json!({ "name": r.name, "index": index + 1 })));
                }
                if ui.add_enabled(index > 0, egui::Button::new(crate::tl!("Move Down"))).clicked() {
                    act = Some(("storey.order", json!({ "name": r.name, "index": index - 1 })));
                }
                let deletable = r.objects == 0 || count == 1;
                if ui
                    .add_enabled(deletable, egui::Button::new(w("Delete Storey")))
                    .on_disabled_hover_text(w("Only an empty storey can be deleted"))
                    .clicked()
                {
                    act = Some(("storey.delete", json!({ "name": r.name })));
                }
                if act.is_some() {
                    ui.close();
                }
            });
            ui.label(RichText::new(r.objects.to_string()).color(t.text_dim).small());
        });
    }
    ui.horizontal_wrapped(|ui| {
        ui.add_space(8.0);
        if ui.button(w("New Storey")).on_hover_text(w("Above the current storey")).clicked() {
            let fresh = fresh_name(&rows);
            act = Some(("storey.new", json!({ "name": fresh })));
            name_new = Some(fresh);
        }
        if ui.button(crate::tl!("Settings...")).clicked() {
            open_dialog = Some("options:storeys");
        }
    });
    if let Some((p, enter)) = rename {
        match app.run("storey.rename", p) {
            // Taken or not allowed: the message is on the command line; after Enter the text stays,
            // selected, to be put right.
            Err(_) if enter => {
                if let Some(e) = editing.as_mut() {
                    e.2 = false;
                }
            }
            _ => editing = None,
        }
    }
    if let Some((id, p)) = act
        && app.run(id, p).is_ok()
        && let Some(n) = name_new
    {
        // The new storey's name, ready to be typed over ("2.OG").
        editing = Some((n.clone(), n, false));
    }
    if let Some(d) = open_dialog {
        app.ui.dialog = Some(d.to_string());
    }
    ui.data_mut(|d| match editing {
        Some(e) => {
            d.insert_temp(edit_id, e);
        }
        None => {
            d.remove::<(String, String, bool)>(edit_id);
        }
    });
}

/// "Storey 3": a name no storey has.
fn fresh_name(rows: &[Row]) -> String {
    let base = w("Storey");
    (rows.len() + 1..)
        .map(|n| format!("{base} {n}"))
        .find(|n| !rows.iter().any(|r| cadcraft_doc::same_storey_name(&r.name, n)))
        .unwrap_or_else(|| base.to_string())
}

// ---------------- the choosers ----------------

/// The storey choosers: `blueprint` picks the storey drawn beneath the current one (or none);
/// otherwise the storey to go to. Arrow keys move, Enter applies, Esc closes; a click applies.
/// What a storey chooser picks a storey for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Pick {
    /// The storey to work on.
    Go,
    /// The storey shown beneath (or none).
    Blueprint,
    /// Where the selected objects are copied to.
    Copy,
    /// Where the selected objects are moved to.
    Move,
}

pub(crate) fn chooser(app: &mut CadApp, ctx: &egui::Context, open: &mut bool, pick: Pick) {
    let blueprint = pick == Pick::Blueprint;
    // Copy and move go to another storey, like the blueprint.
    let elsewhere = pick != Pick::Go;
    let Ok(st) = app.session.state() else {
        *open = false;
        return;
    };
    let d = &st.doc;
    if !d.storeys.active() {
        app.session.echo(w("No storeys yet: the first one takes everything drawn so far."));
        *open = false;
        return;
    }
    let selected = st.selection.len();
    if matches!(pick, Pick::Copy | Pick::Move) && selected == 0 {
        app.session.echo(w("Select the objects first, then choose the storey."));
        *open = false;
        return;
    }
    // (storey id or None, name, current) top to bottom; the blueprint chooser adds None.
    let mut items: Vec<(Option<u32>, String, bool)> = d
        .storeys
        .list
        .iter()
        .rev()
        .map(|x| (Some(x.id), format!("{:>3}  {}", d.storeys.number(x.id).unwrap_or(0), x.name), x.id == d.storeys.current))
        .collect();
    if blueprint {
        items.push((None, crate::tl!("None").to_string(), false));
    }
    let marked = if blueprint { d.ghost_storey } else { Some(d.storeys.current) };
    let pick_id = egui::Id::unique(("storey_chooser", pick));
    let start =
        items.iter().position(|i| i.0 == marked && !(elsewhere && i.2)).or_else(|| items.iter().position(|i| !(elsewhere && i.2))).unwrap_or(0);
    let first_frame = ctx.data(|m| m.get_temp::<usize>(pick_id)).is_none();
    let mut sel: usize = ctx.data(|m| m.get_temp(pick_id)).unwrap_or(start).min(items.len() - 1);
    // The keys are the chooser's only while no text field (the command line) has them, and the
    // Enter that opened it (typed at the command line) doesn't choose at once.
    let (up, down, enter, esc) = if ctx.egui_wants_keyboard_input() {
        (false, false, false, false)
    } else {
        ctx.input_mut(|i| {
            (
                i.consume_key(Modifiers::NONE, Key::ArrowUp),
                i.consume_key(Modifiers::NONE, Key::ArrowDown),
                i.consume_key(Modifiers::NONE, Key::Enter) && !first_frame,
                i.consume_key(Modifiers::NONE, Key::Escape),
            )
        })
    };
    // Blueprint, copy and move pass the current storey: it can't be its own blueprint or target.
    let usable = |j: usize| items.get(j).is_some_and(|i| !(elsewhere && i.2));
    let step = |from: usize, by: isize| -> usize {
        let mut j = from;
        while let Some(n) = j.checked_add_signed(by).filter(|n| *n < items.len()) {
            j = n;
            if usable(j) {
                return j;
            }
        }
        from
    };
    if up {
        sel = step(sel, -1);
    }
    if down {
        sel = step(sel, 1);
    }
    let mut choose: Option<Option<u32>> = if enter && usable(sel) { items.get(sel).map(|i| i.0) } else { None };
    let t = Tokens::get();
    let (title, hint) = match pick {
        Pick::Blueprint => (crate::tl!("Blueprint"), w("The storey shown beneath the current one").to_string()),
        Pick::Go => (w("Go to Storey"), w("The storey to work on").to_string()),
        Pick::Copy => (w("Copy to Storey"), format!("{selected} {}", crate::tl!("selected objects, copied in place to"))),
        Pick::Move => (w("Move to Storey"), format!("{selected} {}", crate::tl!("selected objects, moved in place to"))),
    };
    egui::Window::new(title)
        .id(egui::Id::unique(("storey chooser", pick)))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0))
        .open(open)
        .show(ctx, |ui| {
            ui.label(hint);
            ui.add_space(4.0);
            for (k, (id, name, current)) in items.iter().enumerate() {
                let text =
                    if *current { RichText::new(format!("{name}  ({})", crate::tl!("current"))).color(t.text_dim) } else { RichText::new(name) };
                let r = ui.add_enabled(usable(k), egui::Button::selectable(k == sel, text).min_size(vec2(220.0, 22.0)));
                if r.clicked() {
                    choose = Some(*id);
                }
            }
            ui.add_space(4.0);
            ui.label(RichText::new(crate::tl!("Arrow keys choose, Enter applies, Esc closes")).color(t.text_dim).small());
        });
    if esc {
        *open = false;
    }
    if let Some(g) = choose {
        // By name: the label also shows the number.
        let name = g.and_then(|g| app.session.doc().ok()?.storeys.get(g).map(|x| x.name.clone()));
        let _ = match (pick, name) {
            (Pick::Blueprint, name) => app.run("storey.ghost", json!({ "name": name })),
            (Pick::Go, Some(name)) => app.run("storey.current", json!({ "name": name })),
            (Pick::Copy, Some(name)) => app.run("storeycopy", json!({ "to": name })),
            (Pick::Move, Some(name)) => app.run("storeymove", json!({ "to": name })),
            (_, None) => Ok(Value::Null),
        };
        *open = false;
    }
    if *open {
        ctx.data_mut(|m| m.insert_temp(pick_id, sel));
    } else {
        ctx.data_mut(|m| m.remove::<usize>(pick_id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_namings_are_translated_in_every_catalog() {
        for code in ["de", "uk", "es", "fr", "it", "pt-BR", "cs", "ru", "sv", "hu", "tr", "el", "ar", "ko", "zh"] {
            let catalog = crate::i18n::catalog(code).unwrap_or_else(|| panic!("{code}"));
            for (a, b) in TERMS {
                for s in [a, b] {
                    assert!(catalog.contains_key(&("", *s)), "{code}: `{s}`");
                }
            }
        }
    }

    fn key(s: &str) -> egui::KeyboardShortcut {
        crate::keys::parse(s).unwrap_or_else(|| panic!("{s}"))
    }

    /// The command a key press runs with these user shortcuts.
    fn runs(user: &crate::keys::UserKeys, s: &str) -> Option<String> {
        let k = key(s);
        crate::keys::effective(user).into_iter().find(|(o, _)| o.logical_key == k.logical_key && o.modifiers == k.modifiers).map(|(_, id)| id)
    }

    #[test]
    fn the_storey_commands_have_their_shortcuts_and_the_more_specific_one_comes_first() {
        let none = crate::keys::UserKeys::new();
        assert_eq!(runs(&none, "Cmd+PageUp").as_deref(), Some("storey.up"));
        assert_eq!(runs(&none, "Cmd+PageDown").as_deref(), Some("storey.down"));
        assert_eq!(runs(&none, "Cmd+Shift+G").as_deref(), Some("storey.ghost.toggle"));
        assert_eq!(runs(&none, "Cmd+G").as_deref(), Some("ui.dialog.storeyghost"));
        // egui lets Cmd+G match with Shift held: Cmd+Shift+G must be tried first.
        let pairs = crate::keys::effective(&none);
        let pos = |id: &str| pairs.iter().position(|(_, i)| i == id);
        assert!(pos("storey.ghost.toggle") < pos("ui.dialog.storeyghost"));
        // The menus show them.
        assert_eq!(crate::keys::default_key("storey.up"), Some("Cmd+PageUp"));
        assert_eq!(crate::keys::default_key("ui.dialog.storeyghost"), Some("Cmd+G"));
    }

    #[test]
    fn storey_shortcuts_are_set_like_any_other() {
        let mut user = crate::keys::UserKeys::new();
        let _ = crate::keys::assign(&mut user, "storey.up", "Cmd+6");
        let _ = crate::keys::assign(&mut user, "ui.dialog.storeygoto", "Cmd+4");
        assert_eq!(runs(&user, "Cmd+6").as_deref(), Some("storey.up"));
        assert_eq!(runs(&user, "Cmd+4").as_deref(), Some("ui.dialog.storeygoto"));
        assert_eq!(runs(&user, "Cmd+PageUp"), None, "its built-in key is given up");
        // Another command can take a storey key; storey.down loses it, with the dialog saying so.
        let replaced = crate::keys::assign(&mut user, "line", "Cmd+PageDown").unwrap_or_default();
        assert_eq!(replaced, vec!["storey.down".to_string()]);
        assert_eq!(runs(&user, "Cmd+PageDown").as_deref(), Some("line"));
    }

    #[test]
    fn storey_keys_saved_by_an_earlier_version_are_ignored() {
        let old = json!({ "storeyKeys": { "up": "Cmd+6", "down": "Cmd+5", "goTo": "Cmd+4", "blueprint": "Cmd+0", "chooseBlueprint": "Cmd+7" } });
        let ui = serde_json::from_value::<crate::UiState>(old.clone());
        assert!(ui.is_ok(), "{ui:?}");
        let mut app = CadApp::new(cadcraft_engine::Session::new(), crate::Services::default());
        app.load_prefs(&old.to_string());
        assert!(app.ui.shortcuts.is_empty());
    }
}
