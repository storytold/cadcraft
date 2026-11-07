//! The Command Aliases dialog (ALIASEDIT typed or from a menu): the user's aliases next to the
//! built-in ones, Add, Edit and Remove, then OK or Apply. Built-in aliases are read-only; Edit on
//! one adds a user alias of the same name, which wins over it. The dialog edits a copy and sends
//! only the differences to ALIASEDIT, so it never forgets the alias file REINIT reads.

use std::collections::BTreeMap;

use egui::{RichText, vec2};
use serde_json::{Map, Value, json};

use crate::CadApp;
use crate::theme::Tokens;

/// The Add or Edit form.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Form {
    pub alias: String,
    pub command: String,
    /// The user alias Edit was opened on (`None` when adding, or overriding a built-in).
    pub editing: Option<String>,
}

/// The dialog while open.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AliasDlgState {
    /// The user aliases as edited: lower-case alias → command id.
    pub user: BTreeMap<String, String>,
    /// The user aliases ALIASEDIT holds now (OK and Apply send the difference).
    pub applied: BTreeMap<String, String>,
    /// The alias file loaded last, which REINIT reads again.
    pub file: Option<String>,
    pub selected: Option<String>,
    pub filter: String,
    pub form: Option<Form>,
    pub error: Option<String>,
    /// The warnings the last imported file reported (skipped lines, external commands, unknown
    /// commands); the first few are shown in the dialog and all of them on the command line.
    pub warnings: Vec<String>,
    /// ALIASEDIT typed again while open: bring the window to the front on the next frame.
    pub raise: bool,
    /// Import… was clicked over aliases of the user's own: the dialog asks before a file replaces them.
    pub confirm_import: bool,
}

/// Whether Import… asks first: the file would replace the user's aliases or the edits not applied yet.
pub fn import_replaces_something(st: &AliasDlgState) -> bool {
    !st.user.is_empty() || !st.applied.is_empty()
}

/// The user aliases and alias file ALIASEDIT holds now.
fn current(app: &mut CadApp) -> (BTreeMap<String, String>, Option<String>) {
    let listing = app.session.execute("aliasedit", &json!({})).unwrap_or_default();
    let aliases = listing
        .get("aliases")
        .and_then(Value::as_object)
        .map(|m| m.iter().filter_map(|(a, c)| Some((a.clone(), c.as_str()?.to_string()))).collect())
        .unwrap_or_default();
    (aliases, listing.get("file").and_then(Value::as_str).map(str::to_string))
}

/// The built-in aliases: lower-case alias → command id, the first command naming it winning as
/// it does when typed. Aliases that are also command names never reach their command and are left out.
fn built_in() -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    for c in cadcraft_engine::command_specs() {
        for a in c.aliases {
            let a = a.to_ascii_lowercase();
            if cadcraft_engine::find_command(&a).is_none() {
                m.entry(a).or_insert_with(|| c.id.to_string());
            }
        }
    }
    m
}

/// Open the dialog on the aliases ALIASEDIT holds now.
pub fn open(app: &mut CadApp) {
    // Already open: keep the edits not applied yet.
    if app.ui.dialog.as_deref() == Some("aliasedit")
        && let Some(st) = app.alias_dlg.as_mut()
    {
        st.raise = true;
        return;
    }
    let (user, file) = current(app);
    app.alias_dlg = Some(AliasDlgState { applied: user.clone(), user, file, ..Default::default() });
    app.ui.dialog = Some("aliasedit".into());
}

/// The alias and command a form names, with the rules ALIASEDIT applies, or why not.
fn check(form: &Form, user: &BTreeMap<String, String>) -> Result<(String, String), String> {
    if form.alias.trim().is_empty() {
        return Err(crate::tl!("Type an alias name.").into());
    }
    let name = cadcraft_engine::cmd::file::alias_name(&form.alias)?;
    if form.command.trim().trim_start_matches('*').trim().is_empty() {
        return Err(crate::tl!("Choose the command the alias starts.").into());
    }
    let Some(id) = cadcraft_engine::cmd::file::command_id(&form.command) else {
        return Err(crate::tf!("{command} is an unknown command.", command = form.command.trim().to_ascii_uppercase()));
    };
    let renamed_from = form.editing.as_ref().filter(|old| **old != name);
    if user.contains_key(&name) && form.editing.as_deref() != Some(name.as_str()) {
        return Err(crate::tf!("{alias} is already defined; select it and use Edit.", alias = name.to_ascii_uppercase()));
    }
    let after = user.len().saturating_add(usize::from(!user.contains_key(&name))).saturating_sub(usize::from(renamed_from.is_some()));
    if after > cadcraft_engine::cmd::file::MAX_ALIASES {
        return Err(crate::tf!("More than {n} aliases.", n = cadcraft_engine::cmd::file::MAX_ALIASES));
    }
    Ok((name, id))
}

/// Save alias: put the form's alias in the list, or say why not (the form stays open).
pub fn save_form(st: &mut AliasDlgState) {
    let Some(form) = st.form.clone() else { return };
    match check(&form, &st.user) {
        Ok((name, id)) => {
            if let Some(old) = &form.editing {
                st.user.remove(old);
            }
            st.user.insert(name.clone(), id);
            st.selected = Some(name);
            st.form = None;
            st.error = None;
        }
        Err(e) => st.error = Some(e),
    }
}

/// Send the changes to ALIASEDIT; true when it took them all. ALIASEDIT applies aliases one by
/// one, so after a refusal the dialog reads back what it holds instead of assuming nothing changed.
pub fn apply(app: &mut CadApp) -> bool {
    let Some(st) = app.alias_dlg.as_ref() else { return true };
    let set: Map<String, Value> = st.user.iter().filter(|(a, c)| st.applied.get(*a) != Some(*c)).map(|(a, c)| (a.clone(), json!(c))).collect();
    let remove: Vec<&String> = st.applied.keys().filter(|a| !st.user.contains_key(*a)).collect();
    if set.is_empty() && remove.is_empty() {
        return true;
    }
    let r = app.run("aliasedit", json!({ "set": set, "remove": remove }));
    let (applied, file) = current(app);
    let Some(st) = app.alias_dlg.as_mut() else { return r.is_ok() };
    st.applied = applied;
    st.file = file;
    match r {
        Ok(_) => {
            st.error = None;
            true
        }
        Err(e) => {
            st.error = Some(e);
            false
        }
    }
}

/// Whether `path` names a `.pgp` file, whatever its case.
fn is_pgp(path: &str) -> bool {
    std::path::Path::new(path).extension().is_some_and(|e| e.eq_ignore_ascii_case("pgp"))
}

/// Report a failed import in the dialog (when it is open) and on the command line; returns it so
/// the caller can too.
fn refused(app: &mut CadApp, msg: String) -> String {
    app.session.echo(msg.clone());
    if let Some(st) = app.alias_dlg.as_mut() {
        st.error = Some(msg.clone());
        st.warnings.clear();
    }
    msg
}

/// Load the `.pgp` alias file at `path` through ALIASEDIT, replacing the user aliases as typing
/// `ALIASEDIT {file}` does. The dialog refreshes to what ALIASEDIT holds now, so OK and Apply see
/// no pending difference; the file's warnings are kept for display. Non-`.pgp` and unreadable
/// files are ordinary errors, never a panic.
pub fn import(app: &mut CadApp, path: &str) -> Result<(), String> {
    if !is_pgp(path) {
        return Err(refused(app, format!("Import: {path} is not a .pgp alias file")));
    }
    match app.run("aliasedit", json!({ "file": path })) {
        Ok(v) => {
            let aliases: BTreeMap<String, String> = v
                .get("aliases")
                .and_then(Value::as_object)
                .map(|m| m.iter().filter_map(|(a, c)| Some((a.clone(), c.as_str()?.to_string()))).collect())
                .unwrap_or_default();
            let file = v.get("file").and_then(Value::as_str).unwrap_or(path).to_string();
            let loaded = v.get("loaded").and_then(Value::as_u64).unwrap_or(0);
            let warnings: Vec<String> = v
                .get("warnings")
                .and_then(Value::as_array)
                .map(|w| w.iter().filter_map(Value::as_str).map(str::to_string).collect())
                .unwrap_or_default();
            if let Some(dir) = std::path::Path::new(path).parent().filter(|d| !d.as_os_str().is_empty()) {
                app.ui.last_pgp_dir = Some(dir.to_string_lossy().to_string());
            }
            app.session.echo(format!("{loaded} command alias(es) imported from {path}."));
            for w in &warnings {
                app.session.echo(format!("Import {path}: {w}"));
            }
            if let Some(st) = app.alias_dlg.as_mut() {
                st.user = aliases.clone();
                st.applied = aliases;
                st.file = Some(file);
                st.form = None;
                st.selected = None;
                st.error = None;
                st.warnings = warnings;
            }
            Ok(())
        }
        Err(e) => Err(refused(app, e)),
    }
}

/// Import…: ask the host for a `.pgp` file (starting in the folder the last import came from),
/// then load it. A cancelled picker is a no-op; without a native picker (the web) the error names
/// the ALIASEDIT `text` form, since the web has no file system to read a path from.
pub fn pick_and_import(app: &mut CadApp) -> Result<(), String> {
    let dir = app.ui.last_pgp_dir.clone();
    let picked = app.services.pick_pgp.as_ref().map(|pick| pick(dir.as_deref()));
    match picked {
        None => Err(refused(
            app,
            crate::tl!("Import: there is no file dialog here; run ALIASEDIT with the file's contents using its text parameter.").to_string(),
        )),
        Some(None) => Ok(()),
        Some(Some(path)) => import(app, &path),
    }
}

/// Command Aliases window that edits the in-memory alias set in `app.alias_dlg`; OK/Apply send the differences to ALIASEDIT, read the resulting aliases and source file back, and show any error.
pub fn dialog(app: &mut CadApp, ctx: &egui::Context, open: &mut bool) {
    if app.alias_dlg.is_none() {
        crate::aliasdlg::open(app);
    }
    let built = built_in();
    let warn = Tokens::get().warn;
    let (mut ok, mut cancel, mut apply_clicked, mut import_clicked) = (false, false, false, false);
    // Without a native file picker (the web) Import… can't open one; ALIASEDIT's `text` form loads a file there.
    let can_pick = app.services.pick_pgp.is_some();
    let Some(st) = app.alias_dlg.as_mut() else { return };
    let id = egui::Id::unique("aliasedit_dialog");
    if std::mem::take(&mut st.raise) {
        ctx.move_to_top(egui::LayerId::new(egui::Order::Middle, id));
    }
    egui::Window::new(crate::tl!("Command Aliases")).id(id).open(open).collapsible(false).default_size(vec2(480.0, 460.0)).show(ctx, |ui| {
        match &st.file {
            Some(f) => ui.label(crate::tf!("Alias file: {file}", file = f)),
            None => ui.label(crate::tl!("No alias file loaded. REINIT replaces these aliases with a file's.")),
        };
        ui.label(RichText::new(crate::tl!("Aliases are saved in your CADCraft profile and survive restarts.")).small());
        ui.horizontal(|ui| {
            let label = ui.label(crate::tl!("Filter"));
            ui.text_edit_singleline(&mut st.filter).labelled_by(label.id);
        });
        let f = st.filter.trim().to_ascii_lowercase();
        let mut names: Vec<&String> = st.user.keys().chain(built.keys().filter(|a| !st.user.contains_key(*a))).collect();
        names.sort();
        // Edit and Remove act only on a selected row the filter shows.
        let mut selected_shown = false;
        egui::ScrollArea::vertical().max_height(240.0).auto_shrink([false, true]).show(ui, |ui| {
            egui::Grid::new("alias_rows").striped(true).num_columns(3).show(ui, |ui| {
                ui.strong(crate::tl!("Alias"));
                ui.strong(crate::tl!("Command"));
                ui.label("");
                ui.end_row();
                for name in names {
                    let (command, mine) = match st.user.get(name) {
                        Some(c) => (c, true),
                        None => (built.get(name).unwrap_or(name), false),
                    };
                    if !f.is_empty() && !name.contains(&f) && !command.contains(&f) {
                        continue;
                    }
                    let selected = st.selected.as_deref() == Some(name.as_str());
                    selected_shown |= selected;
                    let text = RichText::new(name.to_ascii_uppercase());
                    if ui.selectable_label(selected, if mine { text } else { text.weak() }).clicked() {
                        st.selected = Some(name.clone());
                    }
                    ui.label(if mine { RichText::new(command.to_ascii_uppercase()) } else { RichText::new(command.to_ascii_uppercase()).weak() });
                    let note = match (mine, built.contains_key(name)) {
                        (true, true) => crate::tl!("overrides built-in"),
                        (true, false) => "",
                        (false, _) => crate::tl!("built-in"),
                    };
                    ui.label(RichText::new(note).small());
                    ui.end_row();
                }
            });
        });
        let sel = st.selected.clone().filter(|_| selected_shown);
        let sel_mine = sel.as_ref().is_some_and(|s| st.user.contains_key(s));
        let sel_known = sel.as_ref().is_some_and(|s| sel_mine || built.contains_key(s));
        ui.horizontal(|ui| {
            if ui.button(crate::tl!("Add…")).clicked() {
                st.form = Some(Form::default());
                st.error = None;
            }
            if ui.add_enabled(sel_known, egui::Button::new(crate::tl!("Edit…"))).clicked()
                && let Some(s) = &sel
            {
                let command = st.user.get(s).or_else(|| built.get(s)).cloned().unwrap_or_default();
                st.form = Some(Form { alias: s.to_ascii_uppercase(), command: command.to_ascii_uppercase(), editing: sel_mine.then(|| s.clone()) });
                st.error = None;
            }
            if ui.add_enabled(sel_mine, egui::Button::new(crate::tl!("Remove"))).clicked()
                && let Some(s) = &sel
            {
                st.user.remove(s);
            }
            let import = ui
                .add_enabled(can_pick, egui::Button::new(crate::tl!("Import…")))
                .on_disabled_hover_text(crate::tl!("Run ALIASEDIT with the file's contents using its text parameter."));
            if import.clicked() {
                if import_replaces_something(st) {
                    st.confirm_import = true;
                } else {
                    import_clicked = true;
                }
            }
        });
        if st.confirm_import {
            ui.group(|ui| {
                ui.label(crate::tf!("Importing a file replaces your {n} aliases, and any changes not applied yet.", n = st.user.len()));
                ui.horizontal(|ui| {
                    if ui.button(crate::tl!("Import and replace")).clicked() {
                        st.confirm_import = false;
                        import_clicked = true;
                    }
                    if ui.button(crate::tl!("Keep my aliases")).clicked() {
                        st.confirm_import = false;
                    }
                });
            });
        }
        let mut save = false;
        let mut discard = false;
        if let Some(form) = st.form.as_mut() {
            ui.group(|ui| {
                egui::Grid::new("alias_form").num_columns(2).show(ui, |ui| {
                    let alias_label = ui.label(crate::tl!("Alias"));
                    ui.text_edit_singleline(&mut form.alias).labelled_by(alias_label.id);
                    ui.end_row();
                    let command_label = ui.label(crate::tl!("Command"));
                    ui.text_edit_singleline(&mut form.command).labelled_by(command_label.id);
                    ui.end_row();
                });
                // A few commands starting with what was typed, to pick from.
                let typed = form.command.trim().trim_start_matches('*').to_ascii_lowercase();
                if !typed.is_empty() && cadcraft_engine::find_command(&typed).is_none() {
                    ui.horizontal_wrapped(|ui| {
                        for c in cadcraft_engine::command_specs().iter().filter(|c| c.id.starts_with(&typed)).take(8) {
                            if ui.small_button(c.id.to_ascii_uppercase()).clicked() {
                                form.command = c.id.to_ascii_uppercase();
                            }
                        }
                    });
                }
                ui.horizontal(|ui| {
                    save = ui.button(crate::tl!("Save alias")).clicked();
                    discard = ui.button(crate::tl!("Discard")).clicked();
                });
            });
        }
        if save {
            save_form(st);
        }
        if discard {
            st.form = None;
            st.error = None;
        }
        if let Some(e) = &st.error {
            ui.colored_label(warn, e.as_str());
        }
        if !st.warnings.is_empty() {
            for w in st.warnings.iter().take(3) {
                ui.colored_label(warn, w.as_str());
            }
            let more = st.warnings.len().saturating_sub(3);
            if more > 0 {
                ui.label(crate::tf!("{n} more warnings; see the command line.", n = more));
            }
        }
        ui.separator();
        ui.horizontal(|ui| {
            ok = ui.button(crate::tl!("OK")).clicked();
            cancel = ui.button(crate::tl!("Cancel")).clicked();
            apply_clicked = ui.button(crate::tl!("Apply")).clicked();
        });
    });
    if import_clicked {
        let _ = pick_and_import(app);
    }
    if (ok && apply(app)) || cancel {
        *open = false;
    } else if apply_clicked {
        apply(app);
    }
    if !*open {
        app.alias_dlg = None;
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::path::PathBuf;
    use std::rc::Rc;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::{Mutex, MutexGuard, PoisonError};

    use cadcraft_engine::Session;
    use serde_json::json;

    use crate::{CadApp, Services};

    use super::{open, pick_and_import};

    /// The alias store is process-wide, so the tests that load files take turns and clear it.
    fn alone() -> MutexGuard<'static, ()> {
        static LOCK: Mutex<()> = Mutex::new(());
        LOCK.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn temp_dir(name: &str) -> PathBuf {
        static N: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!("cadcraft-alias-{}-{}-{name}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    /// An app whose Import picker answers `answer` and records the folder it was asked to start in.
    fn app_with_pick(answer: Option<String>) -> (CadApp, Rc<RefCell<Vec<Option<String>>>>) {
        let asked = Rc::new(RefCell::new(Vec::new()));
        let log = Rc::clone(&asked);
        let answer = Rc::new(RefCell::new(answer));
        let services = Services {
            pick_pgp: Some(Box::new(move |dir: Option<&str>| {
                log.borrow_mut().push(dir.map(str::to_string));
                answer.borrow().clone()
            })),
            ..Services::default()
        };
        (CadApp::new(Session::new(), services), asked)
    }

    fn clear(app: &mut CadApp) {
        let _ = app.run("aliasedit", json!({ "clear": true }));
    }

    #[test]
    fn import_loads_the_aliases_shows_the_path_and_warnings() {
        let _guard = alone();
        let dir = temp_dir("good");
        let path = dir.join("aliases.pgp");
        std::fs::write(&path, "; our own aliases\nXXL, *LINE\nXXC, *CIRCLE\nNOPE, *NOTACOMMAND\nEXT, notepad.exe\n").unwrap_or_default();
        let picked = path.to_string_lossy().to_string();
        let (mut app, asked) = app_with_pick(Some(picked.clone()));
        open(&mut app);

        pick_and_import(&mut app).expect("the file loads");

        let st = app.alias_dlg.as_ref().expect("the dialog stays open");
        assert_eq!(st.user.get("xxl").map(String::as_str), Some("line"));
        assert_eq!(st.user.get("xxc").map(String::as_str), Some("circle"));
        assert_eq!(st.file.as_deref(), Some(picked.as_str()), "the imported path is shown");
        assert_eq!(st.applied, st.user, "an import leaves nothing for Apply to resend");
        assert!(st.error.is_none());
        assert!(st.warnings.iter().any(|w| w.contains("unknown command")), "unknown commands are reported: {:?}", st.warnings);
        assert!(st.warnings.iter().any(|w| w.contains("external command")), "external commands are reported: {:?}", st.warnings);
        assert!(app.session.log.iter().any(|l| l.contains("unknown command")), "the command line shows the full warnings");
        assert_eq!(app.ui.last_pgp_dir.as_deref(), Some(dir.to_string_lossy().as_ref()), "the import folder is remembered");
        assert_eq!(asked.borrow().as_slice(), [None], "the first import has no folder yet");

        // The next import starts where the last file was.
        open(&mut app);
        let _ = pick_and_import(&mut app);
        assert_eq!(asked.borrow().as_slice(), [None, Some(dir.to_string_lossy().to_string())]);

        clear(&mut app);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Import… asks first when a file would replace aliases of the user's own or unapplied edits;
    /// with neither it opens the picker at once.
    #[test]
    fn import_asks_before_replacing_the_users_aliases() {
        let mut st = super::AliasDlgState::default();
        assert!(!super::import_replaces_something(&st));
        st.user.insert("xxl".into(), "line".into());
        assert!(super::import_replaces_something(&st), "an edit not applied yet");
        st.applied = st.user.clone();
        assert!(super::import_replaces_something(&st), "an applied user alias");
        st.user.clear();
        assert!(super::import_replaces_something(&st), "a removal not applied yet");
    }

    #[test]
    fn a_cancelled_picker_changes_nothing() {
        let _guard = alone();
        let (mut app, asked) = app_with_pick(None);
        open(&mut app);
        let before = app.alias_dlg.clone();

        pick_and_import(&mut app).expect("cancelling is not an error");

        assert_eq!(asked.borrow().len(), 1, "the picker was offered");
        assert_eq!(app.alias_dlg, before, "a cancelled picker leaves the aliases, file and errors alone");
        clear(&mut app);
    }

    #[test]
    fn non_pgp_and_malformed_files_are_ordinary_errors() {
        let _guard = alone();
        let dir = temp_dir("bad");
        let text = dir.join("aliases.txt");
        std::fs::write(&text, "XXL, *LINE\n").unwrap_or_default();
        let (mut app, _) = app_with_pick(Some(text.to_string_lossy().to_string()));
        open(&mut app);
        let before = app.alias_dlg.clone().unwrap_or_default();

        let e = pick_and_import(&mut app).expect_err("a .txt file is not an alias file");

        assert!(e.contains(".pgp"), "{e}");
        let st = app.alias_dlg.as_ref().expect("the dialog stays open");
        assert_eq!(st.user, before.user, "the refused file changes no alias");
        assert_eq!(st.file, before.file);
        assert!(st.error.as_deref().is_some_and(|e| e.contains(".pgp")), "{:?}", st.error);

        // Malformed lines are warnings with the aliases that did parse, never a panic.
        let broken = dir.join("broken.pgp");
        std::fs::write(&broken, "not an alias line\nNOSUCH, *NOTACOMMAND\n; comment\nXXL, *LINE\n").unwrap_or_default();
        let (mut app, _) = app_with_pick(Some(broken.to_string_lossy().to_string()));
        open(&mut app);
        pick_and_import(&mut app).expect("malformed lines are warnings");
        let st = app.alias_dlg.as_ref().expect("the dialog stays open");
        assert_eq!(st.user.get("xxl").map(String::as_str), Some("line"));
        assert!(st.warnings.len() >= 2, "{:?}", st.warnings);
        assert!(st.error.is_none(), "{:?}", st.error);

        // A file that cannot be read is an error, reported in the dialog.
        let missing = dir.join("missing.pgp");
        let (mut app, _) = app_with_pick(Some(missing.to_string_lossy().to_string()));
        open(&mut app);
        let e = pick_and_import(&mut app).expect_err("a missing file is an ordinary error");
        assert!(e.contains("missing.pgp"), "{e}");
        assert!(app.alias_dlg.as_ref().is_some_and(|st| st.error.is_some()));

        clear(&mut app);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_last_import_folder_round_trips_through_prefs() {
        let mut app = CadApp::new(Session::new(), Services::default());
        app.ui.last_pgp_dir = Some("/tmp/our-aliases".into());
        let saved = app.prefs_json();
        assert!(saved.contains("lastPgpDir"), "{saved}");

        let mut restored = CadApp::new(Session::new(), Services::default());
        restored.load_prefs(&saved);
        assert_eq!(restored.ui.last_pgp_dir.as_deref(), Some("/tmp/our-aliases"));

        restored.load_prefs(r#"{"lastPgpDir": 3}"#);
        assert_eq!(restored.ui.last_pgp_dir.as_deref(), Some("/tmp/our-aliases"), "malformed values are ignored");
        restored.load_prefs(r#"{"lastPgpDir": ""}"#);
        assert_eq!(restored.ui.last_pgp_dir.as_deref(), Some("/tmp/our-aliases"), "an empty value is ignored");
    }

    #[test]
    fn no_picker_points_at_aliasedit_text_without_changing_aliases() {
        let mut app = CadApp::new(Session::new(), Services::default());
        open(&mut app);
        let before = app.alias_dlg.clone();
        let e = pick_and_import(&mut app).expect_err("the web has no native picker");
        assert!(e.contains("ALIASEDIT"), "{e}");
        assert!(e.contains("contents"), "the web is told to give the file's contents: {e}");
        assert!(e.contains("text"), "the web is told which parameter carries the contents: {e}");
        assert!(!e.contains("path"), "the web has no file system, so a path is not the supported form: {e}");
        let st = app.alias_dlg.as_ref().expect("the dialog stays open");
        assert_eq!(st.user, before.as_ref().map(|b| b.user.clone()).unwrap_or_default());
        assert!(st.error.as_deref().is_some_and(|e| e.contains("ALIASEDIT")));
    }
}
