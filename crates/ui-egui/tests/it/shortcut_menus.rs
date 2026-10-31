//! Right-click menus outside the drawing area (#512): the command line, drawing tabs, layout
//! tabs, and the Layers and Properties palettes.

use cadcraft_engine::Session;
use cadcraft_ui_egui::menus::Entry;
use cadcraft_ui_egui::{CadApp, Services, shortcut_menus as sm};
use serde_json::json;

/// (label, id, enabled) of the items, with submenu items prefixed by "Sub/".
fn flatten(groups: &[Vec<Entry>]) -> Vec<(String, String, bool)> {
    fn walk(prefix: &str, es: &[Entry], out: &mut Vec<(String, String, bool)>) {
        for e in es {
            match e {
                Entry::Item { label, id, enabled, .. } => out.push((format!("{prefix}{label}"), id.clone(), *enabled)),
                Entry::Sub { label, children } => walk(&format!("{prefix}{label}/"), children, out),
            }
        }
    }
    let mut out = Vec::new();
    for g in groups {
        walk("", g, &mut out);
    }
    out
}

/// The id of the item labelled `label`, and whether it is enabled.
fn find(groups: &[Vec<Entry>], label: &str) -> Option<(String, bool)> {
    flatten(groups).into_iter().find(|(l, _, _)| l == label).map(|(_, id, e)| (id, e))
}

fn choose(app: &mut CadApp, groups: &[Vec<Entry>], label: &str) {
    let (id, enabled) = find(groups, label).unwrap_or_else(|| panic!("{label} missing from {:?}", flatten(groups)));
    assert!(enabled, "{label} is disabled");
    sm::choose(app, &egui::Context::default(), &id);
}

fn app() -> CadApp {
    CadApp::new(Session::new(), Services::default())
}

fn layout_names(app: &CadApp) -> Vec<String> {
    app.session.doc().unwrap().layouts.iter().map(|l| l.name.clone()).collect()
}

#[test]
fn command_line_menu() {
    let mut app = app();
    app.cmdline("LINE 0,0 10,0");
    app.cmdline("");
    app.cmdline("CIRCLE 0,0 2");
    let g = sm::command_line_entries(&app);
    assert_eq!(find(&g, "Recent Commands/CIRCLE").map(|f| f.0).as_deref(), Some("circle"));
    assert!(find(&g, "Recent Commands/LINE").is_some());
    assert!(find(&g, "Dynamic Input").is_some() && find(&g, "Command History").is_some());
    assert!(find(&g, "Copy History").is_some_and(|(_, e)| e));
    choose(&mut app, &g, "Lines of Command History/5");
    assert_eq!(app.ui.history_lines, 5);
    choose(&mut app, &g, "Recent Commands/LINE");
    assert_eq!(app.session.running.as_ref().map(|r| r.id.as_str()), Some("line"));
    app.session.cancel();
    choose(&mut app, &g, "Clear Command History");
    assert!(app.session.log.is_empty());
    assert!(find(&sm::command_line_entries(&app), "Copy History").is_some_and(|(_, e)| !e), "nothing to copy");
}

#[test]
fn drawing_tab_menu() {
    let mut app = app();
    app.run("new", json!({})).unwrap();
    app.run("new", json!({})).unwrap();
    assert_eq!(app.session.docs.len(), 3);
    let g = sm::drawing_tab_entries(&app, 1);
    for l in ["New Drawing...", "Open...", "Save", "Save As...", "Close", "Close All Other Drawings"] {
        assert!(find(&g, l).is_some_and(|(_, e)| e), "{l} missing or disabled in {:?}", flatten(&g));
    }
    assert!(find(&g, "Copy Full File Path").is_some_and(|(_, e)| !e), "an unsaved drawing has no path");
    let keep = app.session.docs[1].uid;
    choose(&mut app, &g, "Close All Other Drawings");
    assert_eq!(app.session.docs.iter().map(|d| d.uid).collect::<Vec<_>>(), [keep]);
    let g = sm::drawing_tab_entries(&app, 0);
    assert!(find(&g, "Close All Other Drawings").is_some_and(|(_, e)| !e), "no other drawing");
    choose(&mut app, &g, "Close");
    assert!(app.session.docs.is_empty());
}

#[test]
fn layout_tab_menu() {
    let mut app = app();
    let before = layout_names(&app);
    assert!(before.len() >= 2, "{before:?}");
    let model = sm::layout_tab_entries(&app, "Model");
    assert!(find(&model, "New Layout").is_some() && find(&model, "From Template...").is_some());
    assert!(find(&model, "Delete Layout").is_none() && find(&model, "Rename Layout").is_none(), "the model tab stays");

    // Rename asks for the new name on the command line.
    let first = before[0].clone();
    let g = sm::layout_tab_entries(&app, &first);
    choose(&mut app, &g, "Rename Layout");
    assert!(app.session.prompt_text().contains("new layout name"), "{}", app.session.prompt_text());
    app.cmdline("Sheet A");
    assert!(layout_names(&app).contains(&"Sheet A".to_string()), "{:?}", layout_names(&app));

    let second = before[1].clone();
    let g = sm::layout_tab_entries(&app, &second);
    choose(&mut app, &g, "Delete Layout");
    assert!(!layout_names(&app).contains(&second));

    let n = layout_names(&app).len();
    choose(&mut app, &model, "New Layout");
    // The next free name is taken; the part of the model to show is asked in the Model tab.
    assert!(app.session.prompt_text().contains("first corner of the model area"), "{}", app.session.prompt_text());
    assert_eq!(app.session.state().unwrap().space, cadcraft_doc::Space::Model);
    app.cmdline("");
    assert_eq!(layout_names(&app).len(), n + 1);
    assert!(matches!(app.session.state().unwrap().space, cadcraft_doc::Space::Paper(_)), "the new layout is current");

    // Esc at the area prompt creates nothing and returns to the tab it started in.
    let g = sm::layout_tab_entries(&app, "Sheet A");
    choose(&mut app, &g, "New Layout");
    assert_eq!(app.session.state().unwrap().space, cadcraft_doc::Space::Model);
    app.session.cancel();
    assert_eq!(layout_names(&app).len(), n + 1, "cancelled: no layout added");
    assert!(matches!(app.session.state().unwrap().space, cadcraft_doc::Space::Paper(_)), "back in the layout it started from");

    // Page Setup acts on the right-clicked tab.
    let g = sm::layout_tab_entries(&app, "Sheet A");
    assert!(find(&g, "Page Setup Manager...").is_some() && find(&g, "Print...").is_some());
}

#[test]
fn layer_and_properties_palette_menus() {
    let mut app = app();
    app.run("layer.new", json!({ "name": "Walls" })).unwrap();
    let g = sm::layer_entries(&app, "Walls");
    choose(&mut app, &g, "Turn Off Layer");
    let walls = |app: &CadApp| app.session.doc().unwrap().layers.iter().find(|l| l.name == "Walls").cloned().unwrap();
    assert!(!walls(&app).on);
    let g = sm::layer_entries(&app, "Walls");
    assert!(find(&g, "Turn On Layer").is_some() && find(&g, "Freeze Layer").is_some() && find(&g, "Lock Layer").is_some());
    choose(&mut app, &g, "Set Current");
    assert_eq!(app.session.doc().unwrap().header.str("CLAYER", "0"), "Walls");
    assert!(sm::layer_entries(&app, "NoSuchLayer").is_empty());

    let g = sm::properties_entries(&app);
    for l in ["Quick Select...", "Select All", "Deselect All", "Hide Palette"] {
        assert!(find(&g, l).is_some(), "{l} missing from {:?}", flatten(&g));
    }
}

#[test]
fn every_id_is_a_command() {
    let mut app = app();
    app.cmdline("LINE 0,0 10,0");
    app.cmdline("");
    let mut all = flatten(&sm::command_line_entries(&app));
    all.extend(flatten(&sm::drawing_tab_entries(&app, 0)));
    all.extend(flatten(&sm::layout_tab_entries(&app, "Model")));
    all.extend(flatten(&sm::layout_tab_entries(&app, "Layout1")));
    all.extend(flatten(&sm::layer_entries(&app, "0")));
    all.extend(flatten(&sm::properties_entries(&app)));
    for (label, id, _) in &all {
        let command = id.strip_prefix("run:").and_then(|r| r.split_once(':')).map_or(id.as_str(), |(c, _)| c);
        let special = ["drawing:", "layout:", "cmdline:"].iter().any(|p| id.starts_with(p));
        assert!(
            special || cadcraft_engine::find_command(command).is_some() || cadcraft_ui_egui::menus::is_ui_command(command),
            "{label}: {id} isn't a command"
        );
    }
}
