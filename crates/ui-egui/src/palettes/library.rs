//! The Library palette: a folder of DXF/DWG block files shown as one collapsible group per
//! sub-folder, with a search box. Clicking a file starts INSERT with `name=path` already given at
//! its block name prompt (the same path `-INSERT name=file` takes), so the next prompt asks for
//! the insertion point. The folder is a preference (`libraryDir`); until one is chosen the
//! palette offers `CADCRAFT_LIBRARY_DIR` and then the first of [`DEFAULT_ROOTS`] that exists.
//!
//! Only the files directly in the root and in its immediate sub-folders are listed (no deeper
//! walk); the scan stops at [`MAX_FILES`] files so a huge folder cannot stall a frame.

use std::path::Path;

use egui::{RichText, vec2};
use serde_json::{Value, json};

use crate::CadApp;
use crate::theme::Tokens;

/// Folders offered when no library folder has been chosen, first existing one wins. `~` stands
/// for the user's home folder. A deployment sets its own list here; an empty list is fine.
pub const DEFAULT_ROOTS: &[&str] = &[];
/// Files listed at most, over all groups.
pub const MAX_FILES: usize = 5_000;
/// Sub-folders listed at most.
pub const MAX_GROUPS: usize = 500;

/// A block file the palette can insert.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    /// The block name: the file name without its extension.
    pub label: String,
    /// The file's full path.
    pub path: String,
}

/// One collapsible group: the root folder itself or one of its sub-folders.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Group {
    pub name: String,
    pub items: Vec<Item>,
}

/// Whether `path` is a drawing file the palette lists.
fn is_drawing(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("dxf") || e.eq_ignore_ascii_case("dwg"))
}

/// A name's pieces for natural ordering: `75PFC` before `100PFC`, `150UB14` before `150UB18`.
#[derive(PartialEq, Eq, PartialOrd, Ord)]
enum Chunk {
    Num(u64),
    Text(String),
}

fn natural_key(s: &str) -> Vec<Chunk> {
    let mut out = Vec::new();
    let mut digits = String::new();
    let mut text = String::new();
    for c in s.chars() {
        if c.is_ascii_digit() {
            if !text.is_empty() {
                out.push(Chunk::Text(std::mem::take(&mut text)));
            }
            digits.push(c);
        } else {
            if !digits.is_empty() {
                out.push(Chunk::Num(std::mem::take(&mut digits).parse().unwrap_or(u64::MAX)));
            }
            text.extend(c.to_lowercase());
        }
    }
    if !digits.is_empty() {
        out.push(Chunk::Num(digits.parse().unwrap_or(u64::MAX)));
    }
    if !text.is_empty() {
        out.push(Chunk::Text(text));
    }
    out
}

/// The drawing files directly in `dir`, in natural order, at most `budget` of them.
#[cfg(not(target_arch = "wasm32"))]
fn files_in(dir: &Path, budget: usize) -> Vec<Item> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut items: Vec<Item> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .map(|e| e.path())
        .filter(|p| is_drawing(p))
        .filter_map(|p| {
            let label = p.file_stem()?.to_str()?.to_string();
            // `=` is INSERT's name/path separator and `*` means "exploded": those names can't be typed.
            if label.is_empty() || label.starts_with('.') || label.contains(['=', '*']) {
                return None;
            }
            Some(Item { label, path: p.to_str()?.to_string() })
        })
        .take(budget)
        .collect();
    items.sort_by_cached_key(|i| natural_key(&i.label));
    items
}

/// Scan `root`: its own drawing files (as a group named after the folder) and one group per
/// sub-folder, in natural order. A missing or unreadable folder gives an empty list; nothing here
/// can fail loudly.
#[cfg(not(target_arch = "wasm32"))]
pub fn scan(root: &Path) -> Vec<Group> {
    let mut budget = MAX_FILES;
    let mut groups = Vec::new();
    let own = files_in(root, budget);
    if !own.is_empty() {
        budget = budget.saturating_sub(own.len());
        let name = root.file_name().and_then(|n| n.to_str()).filter(|n| !n.is_empty()).unwrap_or("Library").to_string();
        groups.push(Group { name, items: own });
    }
    let Ok(entries) = std::fs::read_dir(root) else { return groups };
    let mut subs: Vec<(String, std::path::PathBuf)> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter_map(|e| {
            let name = e.file_name().to_str()?.to_string();
            (!name.starts_with('.')).then_some((name, e.path()))
        })
        .take(MAX_GROUPS)
        .collect();
    subs.sort_by_cached_key(|(n, _)| natural_key(n));
    for (name, dir) in subs {
        if budget == 0 {
            break;
        }
        let items = files_in(&dir, budget);
        if items.is_empty() {
            continue;
        }
        budget = budget.saturating_sub(items.len());
        groups.push(Group { name, items });
    }
    groups
}

/// The web has no folders to scan.
#[cfg(target_arch = "wasm32")]
pub fn scan(_root: &Path) -> Vec<Group> {
    Vec::new()
}

/// `~` at the start of a default root is the user's home folder.
#[cfg(not(target_arch = "wasm32"))]
fn expand_home(p: &str) -> Option<String> {
    let Some(rest) = p.strip_prefix('~') else { return Some(p.to_string()) };
    let home = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")).ok().filter(|h| !h.is_empty())?;
    Some(format!("{home}{rest}"))
}

/// The folder offered when none was chosen: `CADCRAFT_LIBRARY_DIR`, else the first of
/// [`DEFAULT_ROOTS`] that exists.
#[cfg(not(target_arch = "wasm32"))]
pub fn default_root() -> Option<String> {
    if let Some(d) = std::env::var("CADCRAFT_LIBRARY_DIR").ok().filter(|d| !d.trim().is_empty()) {
        return Some(d);
    }
    DEFAULT_ROOTS.iter().filter_map(|r| expand_home(r)).find(|r| Path::new(r).is_dir())
}

#[cfg(target_arch = "wasm32")]
pub fn default_root() -> Option<String> {
    None
}

/// The folder the palette shows: the preference, else [`default_root`].
pub fn root(app: &CadApp) -> Option<String> {
    app.ui.library_dir.clone().filter(|d| !d.trim().is_empty()).or_else(default_root)
}

/// The scan the palette shows, kept across frames; `root` says which folder it is of.
#[derive(Clone, Default)]
struct Cache {
    root: String,
    groups: Vec<Group>,
}

fn cache_id() -> egui::Id {
    egui::Id::unique("cadcraft-library-cache")
}
fn search_id() -> egui::Id {
    egui::Id::unique("cadcraft-library-search")
}
fn folder_edit_id() -> egui::Id {
    egui::Id::unique("cadcraft-library-folder-edit")
}

/// Start INSERT with the file given at its block name prompt (`name=path`), so the next prompt
/// asks for the insertion point.
pub fn insert(app: &mut CadApp, item: &Item) {
    crate::dialogs::insert_block(app, &format!("{}={}", item.label, item.path));
}

/// Close only the Library when present; otherwise restore it through the normal open path.
pub(crate) fn toggle(app: &mut CadApp) -> Result<Value, String> {
    let panel = crate::docking::Panel::Library;
    if app.ui.docking.layout.contains(&panel) {
        crate::docking::command(app, false, &json!({ "operation": "close", "panel": panel.id() }))
    } else {
        open(app)
    }
}

/// Open the palette. The first time it goes under the Tool Sets (the docking's home for it);
/// afterwards it returns where it was closed.
pub fn open(app: &mut CadApp) -> Result<Value, String> {
    let panel = crate::docking::Panel::Library;
    crate::docking::command(app, false, &json!({ "operation": "open", "panel": panel.id() }))
}

/// The palette body (inside the docked panel).
pub fn show(app: &mut CadApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    let root = root(app);
    let can_browse = app.services.pick_folder.is_some();
    let no_fs = app.services.no_file_system;
    let mut cache: Cache = ui.ctx().data(|d| d.get_temp(cache_id())).unwrap_or_default();
    let mut rescan = false;
    if let Some(r) = &root
        && cache.root != *r
    {
        rescan = true;
    }
    let mut search: String = ui.ctx().data(|d| d.get_temp(search_id())).unwrap_or_default();
    let mut pick: Option<Item> = None;
    let mut new_root: Option<String> = None;
    let mut browse = false;

    egui::ScrollArea::vertical().id_salt("dock-library").auto_shrink([false, false]).show(ui, |ui| {
        ui.add_space(6.0);
        ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
        // The folder, edited as text; the change applies when the field loses focus.
        ui.horizontal(|ui| {
            ui.add_space(6.0);
            let buttons = if can_browse { 56.0 } else { 30.0 };
            let w = (ui.available_width() - buttons - 6.0).max(60.0);
            let current = root.clone().unwrap_or_default();
            let mut text = ui.data(|d| d.get_temp::<String>(folder_edit_id())).unwrap_or_else(|| current.clone());
            let r = ui.add(egui::TextEdit::singleline(&mut text).hint_text(crate::tl!("Folder of block files")).desired_width(w));
            if r.changed() {
                ui.data_mut(|d| d.insert_temp(folder_edit_id(), text.clone()));
            }
            if r.lost_focus() {
                ui.data_mut(|d| d.remove::<String>(folder_edit_id()));
                if text.trim() != current {
                    new_root = Some(text.trim().to_string());
                }
            }
            if can_browse {
                let b = ui.button("…").on_hover_text(crate::tl!("Choose folder"));
                crate::control::record_widget(ui, &b, "Choose folder", "button", None);
                if b.clicked() {
                    browse = true;
                }
            }
            let b = ui.button("⟳").on_hover_text(crate::tl!("Rescan folder"));
            crate::control::record_widget(ui, &b, "Rescan folder", "button", None);
            if b.clicked() {
                rescan = true;
            }
        });
        ui.horizontal(|ui| {
            ui.add_space(6.0);
            let w = (ui.available_width() - 12.0).max(60.0);
            let r = ui.add(egui::TextEdit::singleline(&mut search).hint_text(crate::tl!("Search")).desired_width(w));
            if r.changed() {
                ui.ctx().data_mut(|d| d.insert_temp(search_id(), search.clone()));
            }
        });
        ui.add_space(2.0);
        if rescan {
            cache = match &root {
                Some(r) => Cache { root: r.clone(), groups: scan(Path::new(r)) },
                None => Cache::default(),
            };
            ui.ctx().data_mut(|d| d.insert_temp(cache_id(), cache.clone()));
        }
        let needle = search.trim().to_lowercase();
        let hint = |ui: &mut egui::Ui, text: &str| {
            ui.horizontal_wrapped(|ui| {
                ui.add_space(6.0);
                ui.label(RichText::new(text).color(t.text_dim).small());
            });
        };
        if no_fs {
            hint(ui, crate::tl!("The Library is not available on the web."));
            return;
        }
        let Some(r) = &root else {
            hint(ui, crate::tl!("Choose a folder of DXF or DWG block files: one group per sub-folder."));
            return;
        };
        if cache.groups.is_empty() {
            hint(ui, &crate::tf!("No DXF or DWG files in {folder}.", folder = r));
            return;
        }
        let mut shown = 0usize;
        for (gi, g) in cache.groups.iter().enumerate() {
            let items: Vec<&Item> = g.items.iter().filter(|i| needle.is_empty() || i.label.to_lowercase().contains(&needle)).collect();
            if items.is_empty() {
                continue;
            }
            shown += items.len();
            let title = format!("{} ({})", g.name, items.len());
            let header = egui::CollapsingHeader::new(RichText::new(title).color(t.text))
                .id_salt(("library-group", gi))
                .default_open(gi == 0)
                .open(if needle.is_empty() { None } else { Some(true) });
            header.show(ui, |ui| {
                ui.spacing_mut().item_spacing = vec2(2.0, 2.0);
                for item in items {
                    let b = ui.add_sized([ui.available_width() - 8.0, 20.0], egui::Button::new(&item.label)).on_hover_text(&item.path);
                    crate::control::record_widget(ui, &b, &item.label, "button", None);
                    if b.clicked() {
                        pick = Some(item.clone());
                    }
                }
            });
        }
        if shown == 0 {
            hint(ui, crate::tl!("No block matches the search."));
        }
    });

    if browse
        && let Some(pick_folder) = app.services.pick_folder.as_ref()
        && let Some(dir) = pick_folder(root.as_deref().unwrap_or(""))
    {
        new_root = Some(dir);
    }
    if let Some(dir) = new_root {
        app.ui.library_dir = if dir.is_empty() { None } else { Some(dir) };
        ui.ctx().data_mut(|d| d.remove::<Cache>(cache_id()));
    }
    if let Some(item) = pick {
        insert(app, &item);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("cadcraft-library-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn touch(p: &Path) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, b"0\nEOF\n").unwrap();
    }

    #[test]
    fn scan_groups_by_sub_folder_in_natural_order() {
        let root = temp_root("groups");
        for f in ["UB/150UB14.dxf", "UB/610UB125.dxf", "UB/310UB40.DWG", "UB/notes.txt", "PFC/100PFC.dxf", "PFC/75PFC.dxf", "empty/readme.md"] {
            touch(&root.join(f));
        }
        touch(&root.join("loose.dxf"));
        touch(&root.join(".hidden/secret.dxf"));
        let groups = scan(&root);
        let names: Vec<&str> = groups.iter().map(|g| g.name.as_str()).collect();
        let root_name = root.file_name().unwrap().to_str().unwrap();
        assert_eq!(names, vec![root_name, "PFC", "UB"], "root files first, then sub-folders; empty and hidden folders skipped");
        let labels = |g: &Group| g.items.iter().map(|i| i.label.clone()).collect::<Vec<_>>();
        assert_eq!(labels(&groups[0]), vec!["loose"]);
        assert_eq!(labels(&groups[1]), vec!["75PFC", "100PFC"], "natural order, not alphabetical");
        assert_eq!(labels(&groups[2]), vec!["150UB14", "310UB40", "610UB125"], "DWG counts too; .txt does not");
        assert!(groups[2].items.iter().all(|i| Path::new(&i.path).is_file()));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn scan_of_a_missing_folder_is_empty() {
        let root = temp_root("missing");
        let _ = std::fs::remove_dir_all(&root);
        assert!(scan(&root).is_empty());
        assert!(scan(Path::new("")).is_empty());
    }

    #[test]
    fn scan_skips_names_insert_cannot_take() {
        let root = temp_root("names");
        // Windows refuses `*` in a file name; the scan must still skip it where it can exist.
        for f in ["a=b.dxf", "*star.dxf", "ok.dxf"] {
            let _ = std::fs::write(
                root.join(f),
                b"0
EOF
",
            );
        }
        let groups = scan(&root);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].items.iter().map(|i| i.label.as_str()).collect::<Vec<_>>(), vec!["ok"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn natural_order_puts_numbers_before_letters_and_by_value() {
        let mut v = vec!["310UB40", "150UB14", "150UB18", "75PFC", "100PFC", "100x100x10.0SHS", "100x100x2.5SHS", "zz"];
        v.sort_by_cached_key(|s| natural_key(s));
        assert_eq!(v, vec!["75PFC", "100PFC", "100x100x2.5SHS", "100x100x10.0SHS", "150UB14", "150UB18", "310UB40", "zz"]);
    }

    #[test]
    fn root_prefers_the_preference_over_the_default() {
        let mut app = CadApp::new(cadcraft_engine::Session::new(), crate::Services::default());
        app.ui.library_dir = Some("C:/blocks".into());
        assert_eq!(root(&app), Some("C:/blocks".into()));
        app.ui.library_dir = Some("  ".into());
        assert_eq!(root(&app), default_root());
    }
}
