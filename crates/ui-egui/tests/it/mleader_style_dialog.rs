//! Format > Multileader Style...: the dialog sets the style's leader type, landing, content and
//! text frame through the `mleaderstyle` command, clicked like a user would.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{MLeaderContent, MLeaderLineType, MLeaderStyle};
use cadcraft_ui_egui::{CadApp, Services, dialogs, menus};
use egui::vec2;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

fn standard(app: &CadApp) -> MLeaderStyle {
    app.session.doc().unwrap().mleader_styles.iter().find(|s| s.name == "Standard").cloned().unwrap()
}

fn dialog() -> Harness<'static, CadApp> {
    let mut app = CadApp::new(Session::default(), Services::default());
    app.run("line", json!({ "points": [[0, 0], [2, 2]] })).unwrap();
    let l = app.session.doc().unwrap().model.iter().next().map(|e| e.handle.hex()).unwrap();
    app.run("block", json!({ "name": "TAG", "base": [0, 0], "handles": [l], "keep": "delete" })).unwrap();
    menus::activate(&mut app, "mleaderstyle");
    assert_eq!(app.ui.dialog.as_deref(), Some("mleaderstyle"));
    let mut h = Harness::builder().with_size(vec2(900.0, 900.0)).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(&ui.ctx().clone());
            dialogs::show(app, ui.ctx());
        },
        app,
    );
    h.run_steps(3);
    h
}

#[test]
fn the_dialog_sets_the_styles_leader_and_content() {
    let mut h = dialog();
    assert!(!standard(h.state()).text_frame);
    h.get_by_label("Frame text").click();
    h.run_steps(3);
    assert!(standard(h.state()).text_frame, "Frame text turned on");
    h.get_by_label("Automatically include landing").click();
    h.run_steps(3);
    assert!(!standard(h.state()).landing, "no landing");
    // Leader type: open the list and pick Spline.
    h.get_by_label("Leader type").click();
    h.run_steps(3);
    h.get_by_label("Spline").click();
    h.run_steps(3);
    assert_eq!(standard(h.state()).leader_type, MLeaderLineType::Spline);
    // Block content with the drawing's block.
    h.get_by_label("Multileader type").click();
    h.run_steps(3);
    h.get_by_label("Block").click();
    h.run_steps(3);
    // Block content needs a block first: refused, the style keeps MText.
    assert_eq!(standard(h.state()).content, MLeaderContent::MText);
    h.state_mut().run("mleaderstyle", json!({ "name": "Standard", "block": "TAG", "current": false })).unwrap();
    h.run_steps(3);
    h.get_by_label("Multileader type").click();
    h.run_steps(3);
    h.get_by_label("Block").click();
    h.run_steps(3);
    let st = standard(h.state());
    assert_eq!((st.content, st.block.as_str()), (MLeaderContent::Block, "TAG"));
    assert_eq!(h.state().ui.dialog.as_deref(), Some("mleaderstyle"), "the dialog stays open");
}
