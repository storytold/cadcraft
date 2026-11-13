//! The storey setup (start storeys, blueprint, naming) is a preference: it is kept across
//! restarts and survives Reset Palettes.

use cadcraft_engine::Session;
use cadcraft_ui_egui::storeys::{Naming, StoreySetup};
use cadcraft_ui_egui::{CadApp, Services};

fn changed() -> StoreySetup {
    StoreySetup {
        auto: false,
        template: cadcraft_doc::StoreyTemplate::blueprints(),
        blueprint_dark: [1, 2, 3],
        blueprint_light: [4, 5, 6],
        blueprint_hatches: false,
        blueprint_per_storey: false,
        naming: Naming::Blueprints,
    }
}

#[test]
fn the_storey_setup_is_kept_across_restarts() {
    let mut a = CadApp::new(Session::new(), Services::default());
    assert!(!a.prefs_json().contains("storeySetup"), "the default is not written");
    a.ui.storey_setup = changed();
    let mut b = CadApp::new(Session::new(), Services::default());
    b.load_prefs(&a.prefs_json());
    assert_eq!(b.ui.storey_setup, changed());
    // A malformed value keeps what is there.
    b.load_prefs(r#"{"storeySetup": "Blueprints"}"#);
    assert_eq!(b.ui.storey_setup, changed());
}

#[test]
fn reset_palettes_keeps_the_storey_setup() {
    let mut app = CadApp::new(Session::new(), Services::default());
    app.ui.storey_setup = changed();
    app.run("ui.resetpalettes", serde_json::json!({})).unwrap();
    assert_eq!(app.ui.storey_setup, changed());
}
