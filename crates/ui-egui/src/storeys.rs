//! Storeys in the interface: the storey navigator in the Layers palette, the storey and blueprint
//! choosers, and the storey shortcuts the user sets.
//!
//! The navigator lists the storeys top to bottom. A click goes to a storey; the blueprint mark in
//! front of a storey shows (and switches) the one drawn beneath the current storey; a right-click
//! offers the rest (blueprint, new storey above or below, move, delete). Everything runs the
//! engine's `storey.*` commands, like the Layers section runs `layer.*`.
//!
//! Shortcuts are the user's: [`StoreyKeys`] is kept with the interface preferences. A shortcut
//! CADCraft already uses is never taken over: it is shown as taken and stays inactive.

use egui::{Key, KeyboardShortcut, Modifiers, RichText, vec2};
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

// ---------------- shortcuts ----------------

/// The storey shortcuts, as registry strings ("Cmd+PageUp"; empty: none).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct StoreyKeys {
    pub up: String,
    pub down: String,
    pub go_to: String,
    pub blueprint: String,
    pub choose_blueprint: String,
}

impl Default for StoreyKeys {
    fn default() -> Self {
        StoreyKeys {
            up: "Cmd+PageUp".into(),
            down: "Cmd+PageDown".into(),
            go_to: String::new(),
            blueprint: "Cmd+Shift+G".into(),
            choose_blueprint: "Cmd+G".into(),
        }
    }
}

impl StoreyKeys {
    /// Storey keys on the number keys: Ctrl+4 go to storey, Ctrl+5 down, Ctrl+6 up, Ctrl+7 choose
    /// the blueprint, Ctrl+0 blueprint on/off (Ctrl+8 is QuickCalc's).
    pub fn number_keys() -> Self {
        StoreyKeys { up: "Cmd+6".into(), down: "Cmd+5".into(), go_to: "Cmd+4".into(), blueprint: "Cmd+0".into(), choose_blueprint: "Cmd+7".into() }
    }

    /// (action label, command, the key) in the order the dialog shows them.
    fn actions(&self) -> [(&'static str, &'static str, &String); 5] {
        [
            ("Storey Above", "storey.up", &self.up),
            ("Storey Below", "storey.down", &self.down),
            ("Go to Storey...", "ui.dialog.storeygoto", &self.go_to),
            ("Blueprint On/Off", "storey.ghost.toggle", &self.blueprint),
            ("Choose Blueprint...", "ui.dialog.storeyghost", &self.choose_blueprint),
        ]
    }

    fn slot(&mut self, i: usize) -> Option<&mut String> {
        match i {
            0 => Some(&mut self.up),
            1 => Some(&mut self.down),
            2 => Some(&mut self.go_to),
            3 => Some(&mut self.blueprint),
            4 => Some(&mut self.choose_blueprint),
            _ => None,
        }
    }
}

/// A registry shortcut string ("Cmd+Shift+G", "Cmd+4", "F3") as a shortcut; `None` when empty or
/// not understood.
pub fn parse(s: &str) -> Option<KeyboardShortcut> {
    let mut m = Modifiers::NONE;
    let mut key = None;
    for part in s.split('+').map(str::trim).filter(|p| !p.is_empty()) {
        match part.to_ascii_lowercase().as_str() {
            "cmd" | "command" => m |= Modifiers::COMMAND,
            // Control on macOS (on Windows and Linux, Ctrl is written Cmd: it is the command key).
            "ctrl" => m |= Modifiers::CTRL,
            "shift" => m |= Modifiers::SHIFT,
            "alt" | "option" => m |= Modifiers::ALT,
            _ if key.is_none() => key = Key::from_name(part),
            _ => return None,
        }
    }
    key.map(|k| KeyboardShortcut::new(m, k))
}

/// A shortcut as a registry string: "Cmd+Shift+G". `mac`: Command is Cmd and Control is Ctrl;
/// elsewhere Control is the command key, Cmd.
fn format(m: Modifiers, k: Key, mac: bool) -> String {
    let mut s = String::new();
    if (mac && m.mac_cmd) || (!mac && (m.command || m.ctrl)) {
        s.push_str("Cmd+");
    }
    if mac && m.ctrl {
        s.push_str("Ctrl+");
    }
    if m.shift {
        s.push_str("Shift+");
    }
    if m.alt {
        s.push_str("Alt+");
    }
    // "+" and "-" would read as separators.
    s.push_str(match k {
        Key::Plus => "Plus",
        Key::Minus => "Minus",
        k => k.name(),
    });
    s
}

/// Whether `s` can be a storey shortcut at all: with Cmd, Ctrl or Alt, or a function or paging
/// key alone. Keys the drawing area and the command line live on (Space, Enter, Tab, Esc, Delete,
/// Backspace, the arrows, letters as typing) and the clipboard keys (they arrive as clipboard
/// events, not key presses) are never storey keys.
pub fn allowed(s: &KeyboardShortcut) -> bool {
    let k = s.logical_key;
    let never =
        [Key::Space, Key::Enter, Key::Tab, Key::Escape, Key::Delete, Key::Backspace, Key::ArrowUp, Key::ArrowDown, Key::ArrowLeft, Key::ArrowRight];
    if never.contains(&k) {
        return false;
    }
    let m = s.modifiers;
    let clipboard = (m.command || m.ctrl || m.mac_cmd) && !m.shift && !m.alt && [Key::C, Key::X, Key::V].contains(&k);
    let alone_ok = matches!(k, Key::PageUp | Key::PageDown | Key::Home | Key::End) || k.name().starts_with('F') && k.name().len() > 1;
    !clipboard && (m.command || m.ctrl || m.mac_cmd || m.alt || alone_ok)
}

/// Shortcuts are tried most specific first (egui lets a shortcut match with Shift or Alt held on
/// top), so Cmd+Shift+G reaches its own action before Cmd+G does.
pub fn by_specificity(pairs: &mut [(KeyboardShortcut, &'static str)]) {
    let weight = |s: &KeyboardShortcut| {
        let m = s.modifiers;
        u8::from(m.shift) + u8::from(m.alt) + u8::from(m.ctrl && !m.command) + u8::from(m.command || m.mac_cmd)
    };
    pairs.sort_by_key(|(s, _)| std::cmp::Reverse(weight(s)));
}

/// The command CADCraft itself has on `s`, if any (built-in shortcuts, menu shortcuts, command
/// shortcuts).
pub fn taken_by(s: &KeyboardShortcut) -> Option<String> {
    let same = |o: &KeyboardShortcut| o.logical_key == s.logical_key && o.modifiers == s.modifiers;
    if let Some((_, id)) = crate::menus::builtin_shortcuts().into_iter().find(|(o, _)| same(o)) {
        return Some(label_of(id));
    }
    for (id, label, _, key) in crate::menus::UI_COMMANDS {
        if key.and_then(parse).is_some_and(|o| same(&o)) {
            return Some(crate::i18n::t(label).to_string());
        }
        let _ = id;
    }
    cadcraft_engine::command_specs()
        .iter()
        .filter(|c| !matches!(c.id, "storey.up" | "storey.down" | "storey.ghost.toggle"))
        .find(|c| c.shortcut.and_then(parse).is_some_and(|o| same(&o)))
        .map(|c| crate::i18n::t(c.label).to_string())
}

fn label_of(id: &str) -> String {
    cadcraft_engine::cmd::find_command(id)
        .map(|c| crate::i18n::t(c.label).to_string())
        .or_else(|| crate::menus::UI_COMMANDS.iter().find(|c| c.0 == id).map(|c| crate::i18n::t(c.1).to_string()))
        .unwrap_or_else(|| id.to_string())
}

/// The storey shortcuts that are in force: set, understood, not one CADCraft uses, and not one an
/// earlier storey action already has.
pub fn active(keys: &StoreyKeys) -> Vec<(KeyboardShortcut, &'static str)> {
    let mut out: Vec<(KeyboardShortcut, &'static str)> = Vec::new();
    for (_, cmd, key) in keys.actions() {
        let Some(s) = parse(key) else { continue };
        if allowed(&s) && taken_by(&s).is_none() && !out.iter().any(|(o, _)| o.logical_key == s.logical_key && o.modifiers == s.modifiers) {
            out.push((s, cmd));
        }
    }
    out
}

/// [`active`], worked out again only when the keys change (the shortcut handler asks every frame).
pub fn active_cached(ctx: &egui::Context, keys: &StoreyKeys) -> Vec<(KeyboardShortcut, &'static str)> {
    let id = egui::Id::unique("storey_keys_active");
    if let Some((k, v)) = ctx.data(|m| m.get_temp::<(StoreyKeys, Vec<(KeyboardShortcut, &'static str)>)>(id))
        && k == *keys
    {
        return v;
    }
    let v = active(keys);
    ctx.data_mut(|m| m.insert_temp(id, (keys.clone(), v.clone())));
    v
}

/// Whether a storey shortcut is being recorded (every shortcut stays quiet meanwhile).
pub fn stop_recording(ctx: &egui::Context) {
    ctx.data_mut(|m| m.remove::<usize>(egui::Id::unique("storey_keys_recording")));
}

pub fn recording(ctx: &egui::Context) -> bool {
    ctx.data(|m| m.get_temp::<usize>(egui::Id::unique("storey_keys_recording"))).is_some()
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

/// The Storeys tab of Options: the start setup, then the storey shortcuts.
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
    shortcuts_ui(app, ui);
}

/// The storey shortcuts: each action's keys, set by pressing them; taken ones are marked.
fn shortcuts_ui(app: &mut CadApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    let ctx = ui.ctx().clone();
    let rec_id = egui::Id::unique("storey_keys_recording");
    let mut recording: Option<usize> = ctx.data(|m| m.get_temp(rec_id));
    // A key pressed while recording becomes that action's shortcut (Esc stops recording).
    if let Some(i) = recording {
        let pressed = ctx.input(|inp| {
            inp.events.iter().find_map(|e| match e {
                egui::Event::Key { key, pressed: true, modifiers, .. } => Some((*key, *modifiers)),
                _ => None,
            })
        });
        if let Some((k, m)) = pressed {
            if k != Key::Escape
                && let Some(slot) = app.ui.storey_keys.slot(i)
            {
                *slot = format(m, k, ctx.os().is_mac());
            }
            recording = None;
            ctx.input_mut(|inp| inp.events.clear());
        }
    }
    let keys = app.ui.storey_keys.clone();
    let mut set: Option<StoreyKeys> = None;
    let mac = ctx.os().is_mac();
    egui::Grid::new("storey_keys_grid").num_columns(4).spacing(vec2(10.0, 6.0)).show(ui, |ui| {
        let mut seen: Vec<KeyboardShortcut> = Vec::new();
        for (i, (label, _, key)) in keys.actions().into_iter().enumerate() {
            ui.label(w(label));
            let shown = if recording == Some(i) {
                crate::tl!("Press the keys").to_string()
            } else if key.is_empty() {
                crate::tl!("Not set").to_string()
            } else {
                crate::menus::shortcut_label(key, mac)
            };
            if ui.add(egui::Button::selectable(recording == Some(i), shown).min_size(vec2(140.0, 20.0))).clicked() {
                recording = Some(i);
            }
            if ui.small_button(crate::tl!("Clear")).clicked() {
                let mut k = keys.clone();
                if let Some(slot) = k.slot(i) {
                    slot.clear();
                }
                set = Some(k);
            }
            let note = match parse(key) {
                None if !key.is_empty() => Some(crate::tl!("Not understood").to_string()),
                None => None,
                Some(s) if !allowed(&s) => Some(crate::tl!("Needs Ctrl or Alt: not active").to_string()),
                Some(s) => match taken_by(&s) {
                    Some(cmd) => Some(crate::tf!("Taken by {command}: not active", command = cmd)),
                    None if seen.iter().any(|o| o.logical_key == s.logical_key && o.modifiers == s.modifiers) => {
                        Some(crate::tl!("Used twice: not active").to_string())
                    }
                    None => {
                        seen.push(s);
                        None
                    }
                },
            };
            match note {
                Some(n) => ui.label(RichText::new(n).color(egui::Color32::from_rgb(0xe0, 0x6c, 0x5a)).small()),
                None => ui.label(""),
            };
            ui.end_row();
        }
    });
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        if ui.button(crate::tl!("Defaults")).clicked() {
            set = Some(StoreyKeys::default());
        }
        if ui.button(crate::tl!("Number Keys Layout")).clicked() {
            set = Some(StoreyKeys::number_keys());
        }
    });
    ui.label(RichText::new(crate::tl!("A shortcut CADCraft uses is never taken over.")).color(t.text_dim).small());
    if let Some(k) = set {
        app.ui.storey_keys = k;
        recording = None;
    }
    ctx.data_mut(|m| match recording {
        Some(r) => {
            m.insert_temp(rec_id, r);
        }
        None => {
            m.remove::<usize>(rec_id);
        }
    });
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

    #[test]
    fn shortcut_strings_round_trip() {
        for s in ["Cmd+PageUp", "Cmd+Shift+G", "Cmd+4", "F3", "Cmd+Alt+Delete", "Cmd+Plus", "Cmd+Minus"] {
            let k = parse(s).unwrap_or_else(|| panic!("{s}"));
            assert_eq!(parse(&format(k.modifiers, k.logical_key, false)), Some(k), "{s}");
        }
        // macOS: Control stays Control, Command stays Command.
        let ctrl = Modifiers { ctrl: true, ..Modifiers::NONE };
        assert_eq!(format(ctrl, Key::Num5, true), "Ctrl+5");
        assert_eq!(format(Modifiers::MAC_CMD, Key::Num5, true), "Cmd+5");
        assert_eq!(format(Modifiers::CTRL, Key::Num5, false), "Cmd+5");
        assert_eq!(parse(""), None);
        assert_eq!(parse("Cmd+Nonsense"), None);
        assert_eq!(parse("Cmd+G+H"), None);
    }

    #[test]
    fn cadcraft_shortcuts_are_never_taken_over() {
        // Ctrl+8 is QuickCalc's, Ctrl+Z undo's: a storey key set to one of them stays inactive.
        assert!(taken_by(&parse("Cmd+Z").unwrap_or_else(|| panic!())).is_some());
        let taken = StoreyKeys { blueprint: "Cmd+8".into(), ..StoreyKeys::number_keys() };
        assert!(active(&taken).iter().all(|(s, c)| taken_by(s).is_none() && *c != "storey.ghost.toggle"));
        // The number keys layout: all five work.
        assert_eq!(active_count(&StoreyKeys::number_keys()), 5);
        // The defaults are all free.
        assert_eq!(active_count(&StoreyKeys::default()), 4);
    }

    fn active_count(k: &StoreyKeys) -> usize {
        active(k).len()
    }

    #[test]
    fn keys_of_the_drawing_area_are_never_storey_keys() {
        for s in ["Space", "Enter", "Tab", "Escape", "Delete", "Up", "G", "Shift+G", "Cmd+C", "Cmd+V"] {
            assert!(!parse(s).is_some_and(|k| allowed(&k)), "{s}");
        }
        for s in ["Cmd+G", "Alt+5", "F6", "PageUp", "Cmd+Shift+V"] {
            assert!(parse(s).is_some_and(|k| allowed(&k)), "{s}");
        }
    }

    #[test]
    fn the_more_specific_shortcut_is_tried_first() {
        let mut v = vec![(parse("Cmd+G").unwrap_or_else(|| panic!()), "a"), (parse("Cmd+Shift+G").unwrap_or_else(|| panic!()), "b")];
        by_specificity(&mut v);
        assert_eq!(v.first().map(|p| p.1), Some("b"));
        // Cmd+Shift+Z (redo) still comes before Cmd+Z (undo) among the built-in ones.
        let mut b = crate::menus::builtin_shortcuts();
        by_specificity(&mut b);
        let pos = |id: &str| b.iter().position(|p| p.1 == id);
        assert!(pos("redo") < pos("u"));
    }

    #[test]
    fn one_key_for_two_actions_is_active_once() {
        let k = StoreyKeys { up: "Cmd+PageUp".into(), down: "Cmd+PageUp".into(), ..StoreyKeys::default() };
        assert_eq!(active(&k).iter().filter(|(_, c)| *c == "storey.down").count(), 0);
    }
}
