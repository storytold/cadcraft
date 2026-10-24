//! Tool Sets can float, pin to the left, join a group, close, or turn into a ribbon under the
//! menu bar; its right-click menu offers every place but the current one.

use cadcraft_engine::Session;
use cadcraft_ui_egui::docking::{Panel, ToolSetsAction, ToolSetsPlace, apply_toolsets, toolsets_place};
use cadcraft_ui_egui::{CadApp, Services};
use egui::vec2;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

fn harness() -> Harness<'static, CadApp> {
    let mut h = Harness::builder().with_size(vec2(1600.0, 1000.0)).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        CadApp::new(Session::new(), Services::default()),
    );
    h.run_steps(3);
    h
}

fn act(h: &mut Harness<'_, CadApp>, a: ToolSetsAction) {
    let ctx = h.ctx.clone();
    apply_toolsets(h.state_mut(), a, &ctx).unwrap();
    h.run_steps(3);
}

#[test]
fn tool_sets_moves_between_ribbon_floating_pinned_and_grouped() {
    let mut h = harness();
    assert_eq!(toolsets_place(h.state()), ToolSetsPlace::Pinned);

    act(&mut h, ToolSetsAction::Ribbon);
    assert_eq!(toolsets_place(h.state()), ToolSetsPlace::Ribbon);
    assert!(!h.state().ui.docking.layout.contains(&Panel::ToolSets), "not a palette while it is the ribbon");
    // The ribbon's tabs and its Draw tools show.
    assert!(h.query_by_label("Annotate").is_some());
    assert!(h.query_all_by_label("Rectangle").count() >= 1);

    act(&mut h, ToolSetsAction::Float);
    assert_eq!(toolsets_place(h.state()), ToolSetsPlace::Floating);
    assert!(!h.state().ui.toolsets_ribbon);

    act(&mut h, ToolSetsAction::Group(Panel::Layers));
    assert_eq!(toolsets_place(h.state()), ToolSetsPlace::Grouped);

    act(&mut h, ToolSetsAction::PinLeft);
    assert_eq!(toolsets_place(h.state()), ToolSetsPlace::Pinned);

    act(&mut h, ToolSetsAction::Close);
    assert_eq!(toolsets_place(h.state()), ToolSetsPlace::Closed);
    assert!(!h.state().ui.docking.layout.contains(&Panel::ToolSets));
}
