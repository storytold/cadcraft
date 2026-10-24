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
