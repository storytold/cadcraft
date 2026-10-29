//! The user's keyboard shortcuts (Tools ▸ Customize ▸ Keyboard Shortcuts...): any command can get
//! one, and a shortcut the user gives wins over CADCraft's own; the command that had it loses it,
//! and the dialog says so. They are kept with the other preferences (`shortcuts`).
//!
//! Recording a shortcut (click, press the keys, Esc to cancel) and the conflict rules come from
//! the storey shortcuts of #1703 (@Alex-BB44).

use std::collections::BTreeMap;

use egui::{Key, KeyboardShortcut, Modifiers, RichText, vec2};
use serde_json::{Value, json};

use crate::CadApp;
use crate::theme::Tokens;

/// The user's shortcuts: command id → registry shortcut ("Cmd+Shift+K"). Empty: the command has
/// none (its built-in one is removed).
pub type UserKeys = BTreeMap<String, String>;

/// At most this many user shortcuts are kept; longer shortcut strings are refused (preferences
/// and JSON calls are hostile input).
pub const MAX_KEYS: usize = 1000;
const MAX_LEN: usize = 64;

/// Commands whose built-in key is handled outside the shortcut table and stays as it is: the
/// clipboard keys, Shift ones included, arrive as clipboard events, Delete belongs to the command
/// line, Cmd+Q to the macOS app menu.
const FIXED: &[&str] = &["copyclip", "cutclip", "pasteclip", "copybase", "pasteblock", "erase.selection", "ui.quit"];

/// The dialog name in `UiState::dialog`.
pub const DIALOG: &str = "shortcuts";

fn same(a: &KeyboardShortcut, b: &KeyboardShortcut) -> bool {
    a.logical_key == b.logical_key && a.modifiers == b.modifiers
}

/// A registry shortcut string ("Cmd+Shift+G", "Alt+4", "F3") as a shortcut; `None` when empty or
/// not understood. Cmd is the command key (Ctrl on Windows and Linux); Ctrl alone is the Control
/// key of macOS, and Cmd+Ctrl both macOS keys.
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
            _ if key.is_none() => key = Some(Key::from_name(part)?),
            _ => return None,
        }
    }
    if m.command && m.ctrl {
        m.mac_cmd = true;
    }
    key.map(|k| KeyboardShortcut::new(m, k))
}

/// A shortcut as a registry string ("Cmd+Shift+G"), the same on every platform: Cmd for the
/// command key, Ctrl for a Control that isn't it (macOS), so [`parse`] gives it back.
pub fn format(m: Modifiers, k: Key) -> String {
    let mut s = String::new();
    if m.command || m.mac_cmd {
        s.push_str("Cmd+");
    }
    if m.ctrl && (m.mac_cmd || !m.command) {
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

/// Whether `s` can be a user shortcut: with Cmd, Ctrl or Alt, or a function or paging key alone.
/// The keys the drawing area and the command line live on (letters as typing, Space, Enter, Tab,
/// Esc, Delete, Backspace, the arrows), the clipboard keys and Cmd+Q never are.
pub fn allowed(s: &KeyboardShortcut) -> bool {
    let k = s.logical_key;
    let never =
        [Key::Space, Key::Enter, Key::Tab, Key::Escape, Key::Delete, Key::Backspace, Key::ArrowUp, Key::ArrowDown, Key::ArrowLeft, Key::ArrowRight];
    if never.contains(&k) {
        return false;
    }
    let m = s.modifiers;
    let command = m.command || m.ctrl || m.mac_cmd;
    // Cmd+C, X and V arrive as clipboard events whatever else is held (Shift+Cmd+C is Copy with
    // Base Point, Shift+Cmd+V Paste as Block).
    let reserved = command && ([Key::C, Key::X, Key::V].contains(&k) || k == Key::Q && !m.shift && !m.alt);
    let alone_ok = matches!(k, Key::PageUp | Key::PageDown | Key::Home | Key::End) || k.name().starts_with('F') && k.name().len() > 1;
    !reserved && (command || m.alt || alone_ok)
}

/// Shortcuts are tried most specific first (egui lets a shortcut match with Shift or Alt held on
/// top), so Cmd+Shift+G reaches its own command before Cmd+G does.
pub fn by_specificity<T>(pairs: &mut [(KeyboardShortcut, T)]) {
    let weight = |s: &KeyboardShortcut| {
        let m = s.modifiers;
        u8::from(m.shift) + u8::from(m.alt) + u8::from(m.ctrl && (m.mac_cmd || !m.command)) + u8::from(m.command || m.mac_cmd)
    };
    pairs.sort_by_key(|(s, _)| std::cmp::Reverse(weight(s)));
}

/// CADCraft's own shortcuts: the shortcut table, plus Cmd+8 (QuickCalc), which the shortcut
/// handler reads before a focused text field can take it.
fn defaults() -> Vec<(KeyboardShortcut, &'static str)> {
    let mut v = crate::menus::builtin_shortcuts();
    v.push((KeyboardShortcut::new(Modifiers::COMMAND, Key::Num8), "ui.toggle.quickcalc"));
    v
}

/// The command a built-in shortcut belongs to in the editor: the table runs some commands' UI
/// form (`ui.open` for OPEN), which is the same command for the user.
fn owner(id: &str) -> &str {
    id.strip_prefix("ui.").filter(|c| cadcraft_engine::find_command(c).is_some()).unwrap_or(id)
}

fn known(id: &str) -> bool {
    cadcraft_engine::find_command(id).is_some() || crate::menus::is_ui_command(id)
}

/// The shortcuts in force: the user's first, then CADCraft's own unless the user gave that
/// command another shortcut or gave the key to another command. Most specific first.
pub fn effective(user: &UserKeys) -> Vec<(KeyboardShortcut, String)> {
    let mut out: Vec<(KeyboardShortcut, String)> = Vec::new();
    for (id, key) in user {
        if let Some(s) = parse(key).filter(allowed)
            && !out.iter().any(|(o, _)| same(o, &s))
        {
            out.push((s, id.clone()));
        }
    }
    for (s, id) in defaults() {
        if !user.contains_key(owner(id)) && !out.iter().any(|(o, _)| same(o, &s)) {
            out.push((s, id.to_string()));
        }
    }
    by_specificity(&mut out);
    out
}

/// [`effective`], worked out again only when the user's shortcuts change (the shortcut handler
/// asks every frame).
pub fn effective_cached(ctx: &egui::Context, user: &UserKeys) -> Vec<(KeyboardShortcut, String)> {
    let id = egui::Id::unique("user_keys_effective");
    if let Some((k, v)) = ctx.data(|m| m.get_temp::<(UserKeys, Vec<(KeyboardShortcut, String)>)>(id))
        && k == *user
    {
        return v;
    }
    let v = effective(user);
    ctx.data_mut(|m| m.insert_temp(id, (user.clone(), v.clone())));
    v
}

/// The shortcut a menu or the editor shows for `id`, whose built-in one is `default`: the user's,
/// or the built-in one unless the user gave that key to another command.
pub fn shown(user: &UserKeys, id: &str, default: Option<&str>) -> Option<String> {
    if let Some(k) = user.get(id) {
        return (!k.is_empty()).then(|| k.clone());
    }
    let d = default?;
    let s = parse(d);
    let taken = s.is_some_and(|s| user.iter().any(|(o, k)| o != id && parse(k).is_some_and(|u| same(&u, &s))));
    (!taken).then(|| d.to_string())
}

/// The built-in shortcut a command shows (its registry `key`, or a UI command's).
pub fn default_key(id: &str) -> Option<&'static str> {
    cadcraft_engine::find_command(id).and_then(|c| c.shortcut).or_else(|| crate::menus::UI_COMMANDS.iter().find(|c| c.0 == id).and_then(|c| c.3))
}

/// Give `id` the shortcut `key` ("" removes its shortcut). The commands that had the key, built
/// in or the user's, lose it; they are returned so the caller can say so.
pub fn assign(user: &mut UserKeys, id: &str, key: &str) -> Result<Vec<String>, String> {
    if !known(id) {
        return Err(format!("{id}: unknown command"));
    }
    if FIXED.contains(&id) {
        return Err(format!("{id}: this shortcut can't be changed"));
    }
    let key = key.trim();
    if key.len() > MAX_LEN {
        return Err("shortcut too long".into());
    }
    if user.len() >= MAX_KEYS && !user.contains_key(id) {
        return Err(format!("more than {MAX_KEYS} shortcuts"));
    }
    if key.is_empty() {
        user.insert(id.to_string(), String::new());
        return Ok(Vec::new());
    }
    let s = parse(key).ok_or_else(|| format!("{key}: not a shortcut"))?;
    if !allowed(&s) {
        return Err(format!("{key}: needs Ctrl, Alt or a function key"));
    }
    let mut replaced = Vec::new();
    for (other, k) in user.iter_mut() {
        if other != id && parse(k).is_some_and(|o| same(&o, &s)) {
            k.clear();
            replaced.push(other.clone());
        }
    }
    // A built-in owner already lost the key to the user command that had it.
    let held = !replaced.is_empty();
    for (o, other) in defaults() {
        let other = owner(other);
        if !held && other != id && same(&o, &s) && !user.contains_key(other) && !replaced.iter().any(|r| r == other) {
            replaced.push(other.to_string());
        }
    }
    user.insert(id.to_string(), format(s.modifiers, s.logical_key));
    Ok(replaced)
}

/// The user's shortcuts from saved preferences: unknown commands, bad strings, a shortcut an
/// earlier command already has (it would show on both, run one) and anything past [`MAX_KEYS`]
/// are dropped.
pub fn from_prefs(v: &Value) -> UserKeys {
    let Some(m) = v.as_object() else { return UserKeys::new() };
    let mut out = UserKeys::new();
    let mut taken: Vec<KeyboardShortcut> = Vec::new();
    for (id, k) in m.iter().filter_map(|(id, k)| Some((id, k.as_str()?))) {
        if out.len() >= MAX_KEYS {
            break;
        }
        if id.len() > MAX_LEN || k.len() > MAX_LEN || !known(id) || FIXED.contains(&id.as_str()) {
            continue;
        }
        if !k.is_empty() {
            let Some(s) = parse(k).filter(allowed) else { continue };
            if taken.iter().any(|o| same(o, &s)) {
                continue;
            }
            taken.push(s);
        }
        out.insert(id.clone(), k.to_string());
    }
    out
}

/// `ui.shortcuts`: from a menu or typed, the Keyboard Shortcuts dialog; with parameters
/// `{set?: {command: shortcut or ""}, reset?: bool}` the JSON form, which answers the user's
/// shortcuts and the commands that lost theirs.
pub fn command(app: &mut CadApp, params: &Value) -> Result<Value, String> {
    if params.is_null() {
        app.ui.dialog = Some(DIALOG.into());
        return Ok(Value::Null);
    }
    let mut keys = app.ui.shortcuts.clone();
    if params.get("reset").and_then(Value::as_bool) == Some(true) {
        keys.clear();
    }
    let mut replaced = Vec::new();
    if let Some(set) = params.get("set") {
        let set = set.as_object().ok_or("`set` must be an object of command: shortcut")?;
        if set.len() > MAX_KEYS {
            return Err(format!("more than {MAX_KEYS} shortcuts"));
        }
        for (id, key) in set {
            let key = key.as_str().ok_or_else(|| format!("{id}: the shortcut must be a string"))?;
            replaced.extend(assign(&mut keys, &id.to_ascii_lowercase(), key)?);
        }
    }
    app.ui.shortcuts = keys;
    Ok(json!({ "shortcuts": app.ui.shortcuts, "replaced": replaced }))
}

const RECORDING: &str = "user_keys_recording";

/// Whether a shortcut is being recorded (every shortcut stays quiet meanwhile).
pub fn recording(ctx: &egui::Context) -> bool {
    ctx.data(|m| m.get_temp::<String>(egui::Id::unique(RECORDING))).is_some()
}

pub fn stop_recording(ctx: &egui::Context) {
    ctx.data_mut(|m| m.remove::<String>(egui::Id::unique(RECORDING)));
}

/// The commands the editor lists, sorted by the label shown: (id, label, what tells same labels
/// apart: the command name to type, or the menu an interface command sits in).
fn rows() -> Vec<(&'static str, String, String)> {
    let mut v: Vec<(&'static str, String, String)> = Vec::new();
    for c in cadcraft_engine::command_specs() {
        if !c.label.is_empty() && !v.iter().any(|(id, ..)| *id == c.id) {
            v.push((c.id, crate::i18n::t(c.label).to_string(), c.id.to_ascii_uppercase()));
        }
    }
    for (id, label, menu, _) in crate::menus::UI_COMMANDS {
        if !label.is_empty() && !menu.is_empty() && owner(id) == *id && !v.iter().any(|(i, ..)| i == id) {
            let path: Vec<&str> = menu.iter().take(menu.len().saturating_sub(1)).map(|m| crate::i18n::menu(m)).collect();
            v.push((id, crate::i18n::t(label).to_string(), path.join(" ▸ ")));
        }
    }
    v.sort_by(|a, b| a.1.to_lowercase().cmp(&b.1.to_lowercase()).then(a.0.cmp(b.0)));
    v
}

fn label_of(id: &str) -> String {
    cadcraft_engine::find_command(id)
        .map(|c| crate::i18n::t(c.label).to_string())
        .or_else(|| crate::menus::UI_COMMANDS.iter().find(|c| c.0 == id).map(|c| crate::i18n::t(c.1).to_string()))
        .unwrap_or_else(|| id.to_string())
}

/// The Keyboard Shortcuts dialog: every command with its shortcut, a filter, a click to record a
/// new one. Changes apply at once.
pub fn dialog(app: &mut CadApp, ctx: &egui::Context, open: &mut bool) {
    let t = Tokens::get();
    let mac = ctx.os().is_mac();
    let rec_id = egui::Id::unique(RECORDING);
    let filter_id = egui::Id::unique("user_keys_filter");
    let note_id = egui::Id::unique("user_keys_note");
    let mut recording: Option<String> = ctx.data(|m| m.get_temp(rec_id));
    let mut filter: String = ctx.data(|m| m.get_temp(filter_id)).unwrap_or_default();
    let mut note: Option<(String, bool)> = ctx.data(|m| m.get_temp(note_id));
    // A key pressed while recording becomes that command's shortcut; Esc stops recording.
    if let Some(id) = recording.clone() {
        let pressed = ctx.input(|inp| {
            inp.events.iter().find_map(|e| match e {
                egui::Event::Key { key, pressed: true, modifiers, .. } => Some((*key, *modifiers)),
                _ => None,
            })
        });
        if let Some((k, m)) = pressed {
            if k != Key::Escape {
                note = Some(match assign(&mut app.ui.shortcuts, &id, &format(m, k)) {
                    Ok(lost) if lost.is_empty() => (String::new(), false),
                    Ok(lost) => {
                        let names: Vec<String> = lost.iter().map(|l| label_of(l)).collect();
                        (crate::tf!("Replaces the shortcut of {commands}", commands = names.join(", ")), false)
                    }
                    Err(_) => (crate::tl!("Needs Ctrl, Alt or a function key").to_string(), true),
                });
            }
            recording = None;
            ctx.input_mut(|inp| inp.events.clear());
        }
    }
    let mut reset_all = false;
    egui::Window::new(crate::tl!("Keyboard Shortcuts"))
        .id(egui::Id::unique("shortcuts_dialog"))
        .open(open)
        .collapsible(false)
        .default_size(vec2(600.0, 520.0))
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(crate::tl!("Filter"));
                ui.text_edit_singleline(&mut filter);
            });
            let f = filter.trim().to_lowercase();
            let rows: Vec<(&'static str, String, String)> = rows()
                .into_iter()
                .filter(|(_, label, detail)| f.is_empty() || label.to_lowercase().contains(&f) || detail.to_lowercase().contains(&f))
                .collect();
            let row_h = ui.spacing().interact_size.y + 4.0;
            egui::ScrollArea::vertical().max_height(360.0).auto_shrink([false, true]).show_rows(ui, row_h, rows.len(), |ui, range| {
                for (id, label, detail) in rows.get(range).unwrap_or_default() {
                    ui.horizontal(|ui| {
                        let cell = |ui: &mut egui::Ui, w: f32, text: RichText| {
                            ui.allocate_ui_with_layout(vec2(w, row_h - 4.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                ui.set_width(w);
                                ui.add(egui::Label::new(text).truncate());
                            });
                        };
                        cell(ui, 190.0, RichText::new(label.as_str()));
                        cell(ui, 140.0, RichText::new(detail.as_str()).color(t.text_dim).small());
                        let fixed = FIXED.contains(id);
                        let key = shown(&app.ui.shortcuts, id, default_key(id));
                        let text = if recording.as_deref() == Some(*id) {
                            crate::tl!("Press the keys").to_string()
                        } else {
                            key.as_deref().map_or_else(|| crate::tl!("None").to_string(), |k| crate::menus::shortcut_label(k, mac))
                        };
                        let b = egui::Button::new(text).selected(recording.as_deref() == Some(*id)).min_size(vec2(120.0, 0.0));
                        if ui.add_enabled(!fixed, b).clicked() {
                            recording = Some(id.to_string());
                            note = None;
                        }
                        if !fixed && key.is_some() && ui.small_button(crate::tl!("Clear")).clicked() {
                            let _ = assign(&mut app.ui.shortcuts, id, "");
                        }
                        if app.ui.shortcuts.contains_key(*id)
                            && ui.small_button(crate::tl!("Default")).on_hover_text(crate::tl!("Back to CADCraft's shortcut")).clicked()
                        {
                            app.ui.shortcuts.remove(*id);
                        }
                    });
                }
            });
            match &note {
                Some((n, true)) => {
                    ui.colored_label(t.warn, n.as_str());
                }
                Some((n, false)) if !n.is_empty() => {
                    ui.label(RichText::new(n.as_str()).color(t.text_dim));
                }
                _ => {}
            }
            ui.separator();
            ui.horizontal(|ui| {
                reset_all = ui.button(crate::tl!("Reset to Defaults")).clicked();
            });
            ui.label(
                RichText::new(crate::tl!("A shortcut you give wins over CADCraft's own. Changes apply at once and are kept for next time."))
                    .color(t.text_dim)
                    .small(),
            );
        });
    if reset_all {
        app.ui.shortcuts.clear();
        note = None;
    }
    if !*open {
        recording = None;
        note = None;
    }
    ctx.data_mut(|m| {
        match &recording {
            Some(r) => {
                m.insert_temp(rec_id, r.clone());
            }
            None => {
                m.remove::<String>(rec_id);
            }
        }
        m.insert_temp(filter_id, filter);
        match note {
            Some(n) => {
                m.insert_temp(note_id, n);
            }
            None => {
                m.remove::<(String, bool)>(note_id);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadcraft_engine::Session;

    fn app() -> CadApp {
        CadApp::new(Session::new(), crate::Services::default())
    }

    fn key(s: &str) -> KeyboardShortcut {
        parse(s).unwrap_or_else(|| panic!("{s}"))
    }

    fn runs(user: &UserKeys, s: &str) -> Option<String> {
        effective(user).into_iter().find(|(o, _)| same(o, &key(s))).map(|(_, id)| id)
    }

    #[test]
    fn shortcut_strings_round_trip() {
        for s in ["Cmd+Shift+G", "Cmd+4", "F3", "Alt+PageUp", "Cmd+Plus", "Cmd+Minus", "Ctrl+K", "Cmd+Ctrl+K"] {
            let k = key(s);
            assert_eq!(format(k.modifiers, k.logical_key), s);
        }
        // Keys as egui reports them: Ctrl on Linux and Windows is the command key; macOS Control
        // and Command+Control keep their Control.
        let linux_ctrl = Modifiers { ctrl: true, command: true, ..Modifiers::NONE };
        let mac_ctrl = Modifiers { ctrl: true, ..Modifiers::NONE };
        let mac_cmd_ctrl = Modifiers { ctrl: true, command: true, mac_cmd: true, ..Modifiers::NONE };
        assert_eq!(format(linux_ctrl, Key::K), "Cmd+K");
        assert_eq!(format(mac_ctrl, Key::K), "Ctrl+K");
        assert_eq!(format(mac_cmd_ctrl, Key::K), "Cmd+Ctrl+K");
        assert!(parse("").is_none() && parse("Cmd+").is_none() && parse("Cmd+G+H").is_none() && parse("Hyper+G").is_none());
    }

    #[test]
    fn typing_keys_clipboard_keys_and_quit_are_never_user_shortcuts() {
        for s in [
            "L",
            "Shift+L",
            "Space",
            "Enter",
            "Escape",
            "Cmd+C",
            "Cmd+V",
            "Cmd+X",
            "Cmd+Shift+C",
            "Cmd+Shift+V",
            "Cmd+Alt+C",
            "Cmd+Alt+X",
            "Cmd+Alt+V",
            "Cmd+Q",
            "ArrowUp",
            "Delete",
        ] {
            assert!(!allowed(&key(s)), "{s}");
        }
        for s in ["Cmd+L", "Alt+L", "F4", "PageDown", "Cmd+Shift+Q", "Ctrl+K"] {
            assert!(allowed(&key(s)), "{s}");
        }
    }

    #[test]
    fn any_command_gets_a_shortcut_and_it_runs_it() {
        let mut user = UserKeys::new();
        assert_eq!(assign(&mut user, "line", "Cmd+L"), Ok(vec![]));
        assert_eq!(runs(&user, "Cmd+L").as_deref(), Some("line"));
        // The built-in ones are untouched.
        assert_eq!(runs(&user, "Cmd+S").as_deref(), Some("qsave"));
        assert_eq!(runs(&UserKeys::new(), "Cmd+L"), None);
    }

    #[test]
    fn control_and_command_control_shortcuts_keep_their_control() {
        let mut user = UserKeys::new();
        assert_eq!(assign(&mut user, "line", "Ctrl+K"), Ok(vec![]));
        assert_eq!(assign(&mut user, "circle", "Cmd+Ctrl+K"), Ok(vec![]));
        assert_eq!(user.get("line").map(String::as_str), Some("Ctrl+K"));
        assert_eq!(user.get("circle").map(String::as_str), Some("Cmd+Ctrl+K"));
        let eff = effective(&user);
        let of = |id: &str| eff.iter().find(|(_, i)| i == id).map(|(s, _)| s.modifiers);
        assert_eq!(of("line"), Some(Modifiers::CTRL));
        assert_eq!(of("circle"), Some(Modifiers { ctrl: true, command: true, mac_cmd: true, ..Modifiers::NONE }));
        // Command+Control+K is tried before Control+K, which it holds too.
        let at = |id: &str| eff.iter().position(|(_, i)| i == id);
        assert!(at("circle") < at("line"));
    }

    #[test]
    fn a_user_shortcut_wins_over_a_built_in_one_and_says_which() {
        let mut user = UserKeys::new();
        assert_eq!(assign(&mut user, "line", "Cmd+P"), Ok(vec!["plot".to_string()]));
        assert_eq!(runs(&user, "Cmd+P").as_deref(), Some("line"));
        assert_eq!(shown(&user, "plot", default_key("plot")), None, "Plot no longer shows Cmd+P");
        assert_eq!(shown(&user, "line", default_key("line")).as_deref(), Some("Cmd+P"));
        // Given to a third command, the first user one loses it too.
        assert_eq!(assign(&mut user, "circle", "Cmd+P"), Ok(vec!["line".to_string()]));
        assert_eq!(runs(&user, "Cmd+P").as_deref(), Some("circle"));
        assert_eq!(shown(&user, "line", default_key("line")), None);
        // A table entry that runs a command's UI form belongs to that command (Cmd+O, OPEN).
        assert_eq!(assign(&mut user, "line", "Cmd+O"), Ok(vec!["open".to_string()]));
        assert_eq!(assign(&mut user, "line", "Cmd+8"), Ok(vec!["ui.toggle.quickcalc".to_string()]));
    }

    #[test]
    fn a_new_shortcut_replaces_the_commands_own_and_clear_removes_it() {
        let mut user = UserKeys::new();
        assert_eq!(assign(&mut user, "qsave", "Cmd+Shift+K"), Ok(vec![]));
        assert_eq!(runs(&user, "Cmd+S"), None, "Save's own Cmd+S is replaced");
        assert_eq!(runs(&user, "Cmd+Shift+K").as_deref(), Some("qsave"));
        assert_eq!(assign(&mut user, "plot", ""), Ok(vec![]));
        assert_eq!(runs(&user, "Cmd+P"), None);
        assert_eq!(shown(&user, "plot", default_key("plot")), None);
        // OPEN through its UI form: its Cmd+O goes when OPEN gets another key.
        assert_eq!(assign(&mut user, "open", "F4"), Ok(vec![]));
        assert_eq!(runs(&user, "Cmd+O"), None);
    }

    #[test]
    fn bad_shortcuts_and_commands_are_refused() {
        let mut user = UserKeys::new();
        assert!(assign(&mut user, "nosuchcommand", "Cmd+L").is_err());
        assert!(assign(&mut user, "line", "L").is_err());
        assert!(assign(&mut user, "line", "Cmd+Nothing").is_err());
        assert!(assign(&mut user, "line", &"Cmd+".repeat(1000)).is_err());
        assert!(assign(&mut user, "copyclip", "Cmd+K").is_err(), "the clipboard keys stay");
        assert!(assign(&mut user, "pasteblock", "Cmd+K").is_err(), "Shift+Cmd+V is a clipboard event too");
        assert!(user.is_empty());
    }

    #[test]
    fn the_json_form_sets_resets_and_never_opens_the_dialog() {
        let mut app = app();
        let v = app.run("ui.shortcuts", json!({"set": {"LINE": "Cmd+P"}})).unwrap();
        assert_eq!(v["shortcuts"], json!({"line": "Cmd+P"}));
        assert_eq!(v["replaced"], json!(["plot"]));
        assert_eq!(app.ui.dialog, None);
        assert!(app.run("ui.shortcuts", json!({"set": {"line": 3}})).is_err());
        assert!(app.run("ui.shortcuts", json!({"set": "Cmd+P"})).is_err());
        assert_eq!(app.ui.shortcuts.get("line").map(String::as_str), Some("Cmd+P"), "a refused call changes nothing");
        let v = app.run("ui.shortcuts", json!({"reset": true})).unwrap();
        assert_eq!(v["shortcuts"], json!({}));
        app.start("ui.shortcuts");
        assert_eq!(app.ui.dialog.as_deref(), Some(DIALOG));
    }

    #[test]
    fn shortcuts_are_kept_with_the_preferences_and_hostile_ones_dropped() {
        let mut a = app();
        let _ = a.run("ui.shortcuts", json!({"set": {"line": "Cmd+L", "plot": ""}}));
        let mut b = app();
        b.load_prefs(&a.prefs_json());
        assert_eq!(b.ui.shortcuts, a.ui.shortcuts);
        let hostile =
            json!({"shortcuts": {"line": "L", "nosuch": "Cmd+K", "copyclip": "Cmd+K", "circle": 5, "arc": "Cmd+K", "x".repeat(5000): "Cmd+J"}});
        let mut c = app();
        c.load_prefs(&hostile.to_string());
        assert_eq!(c.ui.shortcuts, UserKeys::from([("arc".to_string(), "Cmd+K".to_string())]));
        let many: serde_json::Map<String, Value> = (0..5000).map(|i| (format!("c{i}"), json!("Cmd+K"))).collect();
        c.load_prefs(&json!({ "shortcuts": many }).to_string());
        assert!(c.ui.shortcuts.len() <= MAX_KEYS);
        c.load_prefs(r#"{"shortcuts": "Cmd+K"}"#);
        // One key for two commands (an edited file): the first keeps it, the other is dropped.
        c.load_prefs(r#"{"shortcuts": {"arc": "Cmd+K", "line": "Cmd+K", "circle": "Cmd+J"}}"#);
        assert_eq!(c.ui.shortcuts, UserKeys::from([("arc".to_string(), "Cmd+K".to_string()), ("circle".to_string(), "Cmd+J".to_string())]));
    }

    #[test]
    fn the_menus_show_the_shortcut_in_force() {
        let mut app = app();
        let _ = app.run("ui.shortcuts", json!({"set": {"line": "Cmd+P"}}));
        let mut found = (None, None);
        fn walk(es: &[crate::menus::Entry], found: &mut (Option<Option<String>>, Option<Option<String>>)) {
            for e in es {
                match e {
                    crate::menus::Entry::Item { id, shortcut, .. } if id == "line" => found.0 = Some(shortcut.clone()),
                    crate::menus::Entry::Item { id, shortcut, .. } if id == "plot" => found.1 = Some(shortcut.clone()),
                    crate::menus::Entry::Sub { children, .. } => walk(children, found),
                    _ => {}
                }
            }
        }
        for (_, es) in crate::menus::tree(&app) {
            walk(&es, &mut found);
        }
        assert_eq!(found, (Some(Some("Cmd+P".into())), Some(None)));
    }

    #[test]
    fn a_recorded_key_runs_its_command_and_quiets_shortcuts_while_recording() {
        let ctx = egui::Context::default();
        let mut app = app();
        let _ = app.run("ui.shortcuts", json!({"set": {"line": "F4"}}));
        let press = |key: Key| egui::RawInput {
            events: vec![egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }],
            ..Default::default()
        };
        let mut out = ctx.run_ui(press(Key::F4), |ui| crate::menus::shortcuts(&mut app, ui.ctx()));
        out.textures_delta.clear();
        assert_eq!(app.session.summary()["running"], json!("line"));
        app.session.cancel();
        // Recording in the dialog: F4 becomes the recorded key, nothing runs.
        app.ui.dialog = Some(DIALOG.into());
        ctx.data_mut(|m| m.insert_temp(egui::Id::unique(RECORDING), "circle".to_string()));
        let mut out = ctx.run_ui(press(Key::F6), |ui| {
            crate::menus::shortcuts(&mut app, ui.ctx());
            let mut open = true;
            dialog(&mut app, ui.ctx(), &mut open);
        });
        out.textures_delta.clear();
        assert_eq!(app.session.summary()["running"], Value::Null);
        assert_eq!(app.ui.shortcuts.get("circle").map(String::as_str), Some("F6"));
        assert!(!recording(&ctx));
    }
}
