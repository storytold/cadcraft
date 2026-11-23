//! MENUBAR and TOOLBAR at the command line bring a hidden menu bar and tool bar back (#1341):
//! on Windows and Linux the in-window menu bar is the only menu, so with it hidden typing these
//! is the way back, as in AutoCAD.

use cadcraft_engine::Session;
use cadcraft_ui_egui::{CadApp, Services};
use serde_json::json;

fn app() -> CadApp {
    CadApp::new(Session::new(), Services::default())
}

#[test]
fn typed_menubar_shows_and_hides_the_menu_bar() {
    let mut app = app();
    app.ui.in_window_menu = false;
    app.cmdline("MENUBAR");
    assert_eq!(app.session.prompt_text(), "MENUBAR Enter new value for MENUBAR <0>:");
    app.cmdline("1");
    assert!(app.session.running.is_none());
    assert!(app.ui.in_window_menu);
    // 0 hides it again; anything else is refused and asked again.
    app.cmdline("MENUBAR");
    app.cmdline("2");
    assert!(app.session.running.is_some(), "still asking");
    app.cmdline("0");
    assert!(!app.ui.in_window_menu);
}

#[test]
fn typed_toolbar_shows_the_tool_bar_by_default() {
    let mut app = app();
    app.ui.show_toolbar = false;
    app.cmdline("TOOLBAR");
    assert_eq!(app.session.prompt_text(), "TOOLBAR Enter tool bar option or [Show/Hide] <Show>:");
    app.cmdline("");
    assert!(app.ui.show_toolbar);
    app.cmdline("TOOLBAR");
    app.cmdline("H");
    assert!(!app.ui.show_toolbar);
}

#[test]
fn the_json_forms_report_and_set_the_bars() {
    let mut app = app();
    app.ui.in_window_menu = true;
    app.ui.show_toolbar = false;
    app.cmdline("MENUBAR");
    app.cmdline("");
    assert_eq!(app.session.execute("menubar", &json!({})).unwrap(), json!({ "value": 1 }));
    assert_eq!(app.session.execute("toolbar", &json!({})).unwrap(), json!({ "show": false }));
    app.session.execute("toolbar", &json!({ "show": true })).unwrap();
    app.cmdline("MENUBAR");
    app.cmdline("");
    assert!(app.ui.show_toolbar && app.ui.in_window_menu);
    assert!(app.session.execute("menubar", &json!({ "value": 3 })).is_err());
}

/// MENUBAR changes only the menu bar: a tool bar change made before the request is applied
/// stays (a control-API `ui.set` can come between them).
#[test]
fn menubar_leaves_a_later_tool_bar_change() {
    let mut app = app();
    app.ui.in_window_menu = true;
    app.ui.show_toolbar = true;
    app.session.execute("menubar", &json!({ "value": 0 })).unwrap();
    app.ui.show_toolbar = false;
    app.cmdline("'REDRAW");
    assert!(!app.ui.in_window_menu, "MENUBAR applied");
    assert!(!app.ui.show_toolbar, "the tool bar keeps its later change");
}

/// A bar query through `CadApp::run` reports the bars as the interface shows them now, even when
/// a UI change came just before it with no frame drawn in between.
#[test]
fn a_bar_query_sees_a_ui_change_just_made() {
    let mut app = app();
    app.ui.show_toolbar = true;
    assert_eq!(app.run("toolbar", json!({})).unwrap(), json!({ "show": true }));
    app.ui.show_toolbar = false;
    assert_eq!(app.run("toolbar", json!({})).unwrap(), json!({ "show": false }));
    app.run("toolbar", json!({ "show": true })).unwrap();
    assert!(app.ui.show_toolbar, "the request is applied at once");
}

#[test]
fn menubar_started_from_the_ui_offers_the_bar_as_shown_now() {
    let mut app = app();
    app.ui.in_window_menu = true;
    app.cmdline("");
    app.ui.in_window_menu = false;
    app.start("menubar");
    let prompt = app.session.prompt_text();
    assert!(prompt.contains("<0>"), "{prompt}");
}

#[test]
fn enter_through_the_control_api_applies_the_bar_before_the_next_request() {
    let mut app = app();
    let ctx = egui::Context::default();
    app.ui.show_toolbar = false;
    app.start("toolbar");
    let (req, _pending) = cadcraft_ui_egui::control::ControlRequest::new("cmdline.key", serde_json::json!({ "key": "enter" }));
    cadcraft_ui_egui::control::handle(&mut app, &ctx, &req);
    assert!(app.session.running.is_none());
    assert!(app.ui.show_toolbar, "TOOLBAR's default Show is applied before the next request");
}
