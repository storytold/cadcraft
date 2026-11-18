//! The OS cursor is hidden only over the drawing area (#1701): over the menu bar, a menu and the
//! docked palettes the normal pointer comes back, so the user can see what they are pointing at.

use cadcraft_engine::Session;
use cadcraft_geom::Vec2;
use cadcraft_ui_egui::{CadApp, Services};
use egui::accesskit::Role;
use egui::vec2;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

/// A 1280×760 app with the in-window menu bar shown, stepped a few frames so menus and palettes
/// have laid out before the test hovers anything.
fn harness() -> Harness<'static, CadApp> {
    let mut app = CadApp::new(Session::new(), Services::default());
    app.ui.in_window_menu = true;
    let mut harness = Harness::builder().with_size(vec2(1280.0, 760.0)).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        },
        app,
    );
    harness.run_steps(5);
    harness
}

/// The cursor icon the app asks the platform for in the last frame.
fn cursor(harness: &Harness<'_, CadApp>) -> egui::CursorIcon {
    harness.output().platform_output.cursor_icon
}

/// Point at the middle of the drawing area and settle the frames it takes the canvas to react, so
/// `CursorIcon::None` there means the crosshair really is hiding the OS cursor.
fn hover_canvas(harness: &mut Harness<'_, CadApp>) {
    harness.hover_at(egui::pos2(640.0, 400.0));
    harness.run_steps(3);
}

/// The canvas hides the OS cursor; moving onto a menu title, which sits on a higher layer over no
/// drawing, must bring the plain arrow back and must not leave `CursorIcon::None` behind.
#[test]
fn the_menu_bar_brings_the_os_cursor_back() {
    let mut harness = harness();
    hover_canvas(&mut harness);
    assert_eq!(cursor(&harness), egui::CursorIcon::None, "the drawn crosshair replaces the OS cursor on the canvas");

    // Pointing at a menu title: the crosshair (and the hidden OS cursor) must not leak out of the
    // drawing area.
    harness.get_by_role_and_label(Role::Button, "File").hover();
    harness.run_steps(3);
    assert_ne!(cursor(&harness), egui::CursorIcon::None, "the menu bar keeps a visible OS cursor");
    assert_eq!(cursor(&harness), egui::CursorIcon::Default);
}

/// Both parts of an open menu keep the OS cursor visible over the drawing: its items, and the
/// frame between them and the border, where no item is hovered. Guards the layer check against
/// `hovered()` alone, which still counts the canvas underneath the popup.
#[test]
fn an_open_menu_keeps_the_os_cursor_visible_over_the_drawing() {
    let mut harness = harness();
    let title = harness.get_by_role_and_label(Role::Button, "View");
    title.click();
    harness.run_steps(3);

    // A menu item of the open popup lies over the drawing area.
    harness.get_by_label("Regen All").hover();
    harness.run_steps(3);
    assert_ne!(cursor(&harness), egui::CursorIcon::None, "a menu item over the canvas shows the pointer");

    // The menu's frame (between its items and its border) is over the canvas too, but it is no
    // menu item: the drawing's crosshair must stay under it rather than replace the OS cursor.
    let popup = harness
        .ctx
        .memory(|m| {
            m.areas()
                .visible_layer_ids()
                .into_iter()
                .filter(|layer| layer.order != egui::Order::Background)
                .filter_map(|layer| m.area_rect(layer.id).map(|rect| (layer, rect)))
                .max_by(|(_, a), (_, b)| (a.width() * a.height()).total_cmp(&(b.width() * b.height())))
        })
        .expect("the open menu is an area");
    let frame = egui::pos2(popup.1.right() - 2.0, popup.1.bottom() - 2.0);
    assert!(harness.state().canvas.rect.is_some_and(|canvas| canvas.contains(frame)), "the menu's frame overlaps the drawing area");
    assert_eq!(harness.ctx.layer_id_at(frame), Some(popup.0), "the frame point is over the menu, not the drawing");
    harness.hover_at(frame);
    harness.run_steps(3);
    assert_ne!(cursor(&harness), egui::CursorIcon::None, "the menu's frame keeps the OS cursor visible");
}

/// The docked palettes are ordinary (non-floating) panels, still above the canvas in the layer
/// stack: leaving the drawing for one restores the OS cursor.
#[test]
fn docked_palettes_keep_the_os_cursor_visible() {
    let mut harness = harness();
    hover_canvas(&mut harness);
    assert_eq!(cursor(&harness), egui::CursorIcon::None);

    // The Tool Sets palette docked left of the drawing.
    harness.hover_at(egui::pos2(100.0, 300.0));
    harness.run_steps(3);
    assert_ne!(cursor(&harness), egui::CursorIcon::None, "moving to a palette restores the OS cursor");
}

/// With the assistive-technology fallback (`system_cursor`, #39) the canvas asks for the crosshair
/// instead of hiding the pointer; off the canvas that request must not follow, so the menu bar
/// gets the default arrow rather than a drafting crosshair.
#[test]
fn show_system_cursor_restores_the_arrow_off_the_canvas() {
    let mut harness = harness();
    harness.state_mut().ui.system_cursor = true;
    hover_canvas(&mut harness);
    assert_eq!(cursor(&harness), egui::CursorIcon::Crosshair);

    harness.get_by_role_and_label(Role::Button, "File").hover();
    harness.run_steps(3);
    assert_eq!(cursor(&harness), egui::CursorIcon::Default, "not the drafting crosshair over the menu bar");
}

/// Two overlapping lines (SELECTIONCYCLING 2), zoomed to fit: a click where they overlap opens the
/// selection cycling list beside it.
fn cycle_harness() -> Harness<'static, CadApp> {
    let mut session = Session::new();
    session.execute("line", &json!({ "points": [[0, 0], [10, 0]] })).unwrap();
    session.execute("line", &json!({ "points": [[0, 0], [20, 0]] })).unwrap();
    session.execute("setvar", &json!({ "name": "SELECTIONCYCLING", "value": 2 })).unwrap();
    session.set_selection(Vec::new());
    let mut app = CadApp::new(session, Services::default());
    app.ui.in_window_menu = true;
    app.canvas.zoom_pending = true;
    let mut harness = Harness::builder().with_size(vec2(1280.0, 760.0)).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        },
        app,
    );
    harness.run_steps(5);
    harness
}

/// A press and release of the primary button at `at`.
fn click(harness: &mut Harness<'_, CadApp>, at: egui::Pos2) {
    harness.event(egui::Event::PointerMoved(at));
    harness.run_steps(1);
    harness.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::NONE });
    harness.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: false, modifiers: egui::Modifiers::NONE });
    harness.run_steps(2);
}

/// Selection cycling's rows are painted on the canvas, so the layer under them is the drawing, but
/// they are a menu: the OS cursor stays visible over a row, while the drawing around it still
/// hides the cursor under the crosshair (#1701).
#[test]
fn a_selection_cycling_row_keeps_the_os_cursor_visible() {
    let mut harness = cycle_harness();
    let at = harness.state().canvas.xf.expect("the canvas is laid out").to_screen(Vec2::new(5.0, 0.0));
    click(&mut harness, at);
    let rows = harness.state().canvas.cycle_rows();
    assert_eq!(rows.len(), 2, "the click over both lines lists them");
    let row = rows[1].0;
    assert!(harness.state().canvas.rect.is_some_and(|canvas| canvas.contains(row.center())), "the list lies over the drawing");

    // The drawing beside the list still hides the OS cursor.
    let beside = egui::pos2(row.left() - 4.0, row.center().y);
    harness.hover_at(beside);
    harness.run_steps(3);
    assert_eq!(cursor(&harness), egui::CursorIcon::None, "the canvas beside the list hides the OS cursor");

    // Over the row itself the pointer must come back, and the row keeps highlighting its object.
    harness.hover_at(row.center());
    harness.run_steps(3);
    assert_ne!(cursor(&harness), egui::CursorIcon::None, "a selection cycling row is a menu and keeps a visible OS cursor");
    assert_eq!(harness.state().canvas.hover, Some(rows[1].1));
    assert_eq!(harness.state().canvas.cycle_rows().len(), 2, "the list stays open under the pointer");
}

/// Dynamic Input's options menu is painted on the canvas too: over one of its rows the OS cursor
/// stays visible (and the menu stays open, so a click can still pick the row).
#[test]
fn a_dynamic_input_options_row_keeps_the_os_cursor_visible() {
    let mut harness = harness();
    hover_canvas(&mut harness);
    assert_eq!(cursor(&harness), egui::CursorIcon::None, "the canvas hides the OS cursor");

    harness.state_mut().cmdline("circle");
    harness.run_steps(3);
    harness.key_press(egui::Key::ArrowDown);
    harness.run_steps(3);
    let rows = harness.state().cmd.option_rows.clone();
    assert!(harness.state().cmd.options.is_some() && rows.len() >= 2, "Down Arrow lists the prompt's options over the canvas: {rows:?}");
    let row = rows[0].0;
    assert!(harness.state().canvas.rect.is_some_and(|canvas| canvas.contains(row.center())), "the options menu lies over the drawing");

    harness.hover_at(row.center());
    harness.run_steps(3);
    assert_ne!(cursor(&harness), egui::CursorIcon::None, "a Dynamic Input options row is a menu and keeps a visible OS cursor");
    assert!(harness.state().cmd.options.is_some(), "the menu stays open under the pointer");
    assert!(harness.state().cmd.option_rows.iter().any(|(r, _)| r.contains(row.center())), "the row is still drawn for clicks");
    assert!(harness.state().canvas.cursor.is_some(), "the canvas keeps tracking the pointer under the menu, so the value boxes stay put");
}
