//! The Hatch and Gradient dialog's Custom Patterns tab: the hatch patterns of the `.pat` files in
//! a folder the user names, listed with a preview of the one selected.
//!
//! The folder is read when it changes or on Reload, not every frame. A pattern used for a hatch is
//! loaded into the session's pattern list from its file when OK is pressed (HATCH.PATTERNS), so
//! the hatch keeps its line families like any pattern loaded from a file.

use std::cell::RefCell;

use cadcraft_doc::library::PatternLine;
use cadcraft_doc::library::files::parse_pat;
use egui::{Rect, Sense, Stroke, pos2, vec2};

use crate::theme::Tokens;

/// Most `.pat` files read from the folder, and most patterns listed.
const MAX_FILES: usize = 500;
const MAX_PATTERNS: usize = 5000;
/// Largest `.pat` file read.
const MAX_FILE_BYTES: u64 = 4 << 20;

/// A pattern found in the folder.
#[derive(Clone, Debug, PartialEq)]
pub struct Found {
    /// The `.pat` file it is in.
    pub file: String,
    pub name: String,
    pub description: String,
    pub lines: Vec<PatternLine>,
}

/// What the folder held when last read.
#[derive(Clone, Debug, Default)]
struct Listing {
    folder: String,
    generation: u64,
    patterns: std::rc::Rc<Vec<Found>>,
    problem: Option<String>,
}

thread_local! {
    static LISTING: RefCell<Listing> = RefCell::new(Listing { generation: u64::MAX, ..Listing::default() });
    static GENERATION: RefCell<u64> = const { RefCell::new(0) };
}

/// Read the folder again on the next frame (files were added or changed).
pub fn reload() {
    GENERATION.with(|g| *g.borrow_mut() += 1);
}

/// The patterns of every `.pat` file in `folder` (not its subfolders), by file then name.
pub fn scan(folder: &str) -> (Vec<Found>, Option<String>) {
    let dir = folder.trim();
    if dir.is_empty() {
        return (Vec::new(), None);
    }
    let rd = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (Vec::new(), Some(crate::tl!("This folder does not exist").to_string())),
        // There but not readable (permissions, not a folder…): say why.
        Err(e) => return (Vec::new(), Some(format!("{}: {e}", crate::tl!("The folder can't be read")))),
    };
    let mut files: Vec<std::path::PathBuf> = Vec::new();
    for entry in rd.take(MAX_FILES * 20) {
        // An entry that can't be read (removed meanwhile, no permission) is left out.
        let Ok(entry) = entry else { continue };
        // `file_type` does not follow symbolic links: a link to a file elsewhere is not read.
        let path = entry.path();
        if entry.file_type().is_ok_and(|t| t.is_file()) && path.extension().is_some_and(|x| x.eq_ignore_ascii_case("pat")) {
            files.push(path);
        }
    }
    // The same files every time: sorted, then the first ones kept.
    files.sort();
    files.truncate(MAX_FILES);
    let mut out = Vec::new();
    'files: for path in files {
        if std::fs::metadata(&path).map(|m| m.len()).unwrap_or(u64::MAX) > MAX_FILE_BYTES {
            continue;
        }
        let Ok(bytes) = std::fs::read(&path) else { continue };
        // Pattern files are ASCII, often written in a Windows code page: decode leniently.
        let text = String::from_utf8_lossy(&bytes);
        for p in parse_pat(&text).items {
            if out.len() >= MAX_PATTERNS {
                break 'files;
            }
            out.push(Found { file: path.to_string_lossy().to_string(), name: p.name, description: p.description, lines: p.lines });
        }
    }
    let problem = out.is_empty().then(|| crate::tl!("No hatch patterns (.pat files) in this folder").to_string());
    (out, problem)
}

/// The folder's patterns, read again when the folder or the reload count changed; while
/// `typing` the folder, the last listing stays (a folder isn't read on every keystroke).
/// Shared, not copied: the tab asks for it every frame and a folder may hold thousands.
fn listing(folder: &str, typing: bool) -> (std::rc::Rc<Vec<Found>>, Option<String>) {
    let generation = GENERATION.with(|g| *g.borrow());
    LISTING.with(|cell| {
        let mut l = cell.borrow_mut();
        let stale = l.folder != folder || l.generation != generation;
        // The first listing, or a reload, is read even while typing.
        if stale && (!typing || l.generation == u64::MAX || l.folder == folder) {
            let (patterns, problem) = scan(folder);
            *l = Listing { folder: folder.to_string(), generation, patterns: std::rc::Rc::new(patterns), problem };
        }
        (l.patterns.clone(), l.problem.clone())
    })
}

/// Draw a pattern's preview in `r`: its lines over a few repeats, or "SOLID" filled.
pub fn preview(p: &egui::Painter, r: Rect, lines: &[PatternLine]) {
    let t = Tokens::get();
    p.rect_filled(r, 3.0, t.field);
    let inner = r.shrink(4.0);
    let runs = cadcraft_engine::render::pattern_swatch(lines, 1.0);
    let at = |v: &cadcraft_engine::geom::Vec2| pos2(inner.min.x + v.x as f32 * inner.width(), inner.max.y - v.y as f32 * inner.height());
    let clip = p.with_clip_rect(inner);
    for run in &runs {
        match run.as_slice() {
            [] => {}
            [dot] => {
                clip.circle_filled(at(dot), 0.9, t.text);
            }
            pts => {
                clip.add(egui::Shape::line(pts.iter().map(at).collect(), Stroke::new(1.0, t.text)));
            }
        }
    }
    p.rect_stroke(r, 3.0, Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
}

/// What the tab asks the dialog to do.
pub enum TabAction {
    None,
    /// Pick the folder with the system's folder dialog.
    Browse,
}

/// The tab: the folder, the list of its patterns and the preview of the chosen one. A click sets
/// `pattern` and `file`.
pub fn tab_ui(ui: &mut egui::Ui, folder: &mut String, pattern: &mut String, file: &mut Option<String>, can_browse: bool) -> TabAction {
    let mut action = TabAction::None;
    let mut typing = false;
    ui.label(crate::tl!("Folder with hatch pattern files (.pat):"));
    ui.horizontal(|ui| {
        let w = ui.available_width() - if can_browse { 150.0 } else { 70.0 };
        typing = ui.add(egui::TextEdit::singleline(folder).desired_width(w.max(120.0)).hint_text(crate::tl!("Folder path"))).has_focus();
        if can_browse && ui.button(crate::tl!("Browse...")).clicked() {
            action = TabAction::Browse;
        }
        if ui.button(crate::tl!("Reload")).on_hover_text(crate::tl!("Read the folder again")).clicked() {
            reload();
        }
    });
    let (found, problem) = listing(folder, typing);
    if let Some(p) = &problem {
        ui.colored_label(Tokens::get().text_dim, p);
    }
    ui.add_space(4.0);
    let selected = found.iter().find(|f| f.name.eq_ignore_ascii_case(pattern) && file.as_deref() == Some(f.file.as_str()));
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(190.0);
            egui::Frame::NONE.stroke(Stroke::new(1.0, Tokens::get().border)).inner_margin(2.0).show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("custom_patterns_list")
                    .auto_shrink([false, false])
                    .max_height(200.0)
                    .min_scrolled_height(200.0)
                    .show(ui, |ui| {
                        ui.set_width(182.0);
                        for f in found.iter() {
                            let on = selected.is_some_and(|s| s.name == f.name && s.file == f.file);
                            let tip = if f.description.is_empty() { f.file.clone() } else { format!("{}\n{}", f.description, f.file) };
                            if ui.selectable_label(on, &f.name).on_hover_text(tip).clicked() {
                                *pattern = f.name.clone();
                                *file = Some(f.file.clone());
                            }
                        }
                    });
            });
        });
        ui.vertical(|ui| {
            let (r, _) = ui.allocate_exact_size(vec2(170.0, 170.0), Sense::hover());
            match selected {
                Some(f) => {
                    preview(ui.painter(), r, &f.lines);
                    ui.label(egui::RichText::new(&f.name).strong());
                    if !f.description.is_empty() {
                        ui.add(egui::Label::new(egui::RichText::new(&f.description).small()).wrap());
                    }
                }
                None => {
                    let t = Tokens::get();
                    ui.painter().rect_stroke(r, 3.0, Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
                    ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, crate::tl!("Preview"), crate::theme::small(), t.text_faint);
                }
            }
        });
    });
    action
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_folder_of_pat_files_lists_their_patterns() {
        let dir = std::env::temp_dir().join(format!("cadcraft-custom-pat-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("mine.pat"), "*MYLINES, my lines\n0, 0,0, 0,1\n*MYGRID, grid\n0, 0,0, 0,1\n90, 0,0, 0,1\n").unwrap();
        std::fs::write(dir.join("notes.txt"), "not a pattern").unwrap();
        let (found, problem) = super::scan(&dir.to_string_lossy());
        assert!(problem.is_none());
        let names: Vec<&str> = found.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["MYLINES", "MYGRID"]);
        assert_eq!(found.get(1).map(|f| f.lines.len()), Some(2));
        let (none, problem) = super::scan(&dir.join("missing").to_string_lossy());
        assert!(none.is_empty() && problem.is_some());
        // A symbolic link to a pattern file elsewhere is not read.
        #[cfg(unix)]
        {
            let outside = std::env::temp_dir().join(format!("cadcraft-outside-{}.pat", std::process::id()));
            std::fs::write(&outside, "*OUTSIDE, x\n0, 0,0, 0,1\n").unwrap();
            let _ = std::os::unix::fs::symlink(&outside, dir.join("link.pat"));
            let (found, _) = super::scan(&dir.to_string_lossy());
            assert!(!found.iter().any(|f| f.name == "OUTSIDE"), "a link is skipped");
            let _ = std::fs::remove_file(outside);
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    /// While the folder is being typed it isn't read on every keystroke; once typing stops
    /// it is.
    #[test]
    fn a_folder_being_typed_is_read_once_typing_stops() {
        let dir = std::env::temp_dir().join(format!("cadcraft-typing-pat-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.pat"), "*ONE, one\n0, 0,0, 0,1\n").unwrap();
        let full = dir.to_string_lossy().to_string();
        let (first, _) = super::listing("", false);
        assert!(first.is_empty());
        let (typing, _) = super::listing(&full, true);
        assert!(typing.is_empty(), "the last listing stays while typing");
        let (done, _) = super::listing(&full, false);
        assert_eq!(done.len(), 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// The pattern limit holds across files, and a path that is a file is not "missing".
    #[test]
    fn the_pattern_limit_counts_every_file() {
        let dir = std::env::temp_dir().join(format!("cadcraft-many-pat-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for f in ["a", "b", "c"] {
            let many: String = (0..2000).map(|i| format!("*{f}{i}, p\n0, 0,0, 0,1\n")).collect();
            std::fs::write(dir.join(format!("{f}.pat")), &many).unwrap();
        }
        let (found, _) = super::scan(&dir.to_string_lossy());
        assert_eq!(found.len(), super::MAX_PATTERNS);
        assert!(!found.iter().any(|p| p.name.starts_with('C') && p.name[1..].parse::<usize>().is_ok_and(|n| n >= 1000)));
        let (none, problem) = super::scan(&dir.join("a.pat").to_string_lossy());
        assert!(none.is_empty());
        let problem = problem.unwrap_or_default();
        assert!(!problem.contains("does not exist"), "{problem}");
        let _ = std::fs::remove_dir_all(dir);
    }
}
