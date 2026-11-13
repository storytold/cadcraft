//! Inside a layout viewport an object on a layer frozen in that viewport is not drawn: selected,
//! it paints no grips there, and the cursor does not highlight it, also when the layer is frozen
//! while the pointer rests over the object.

use cadcraft_engine::doc::Handle;
use cadcraft_engine::geom::Vec2;
use cadcraft_engine::{Session, View};
use cadcraft_ui_egui::{CadApp, Services, theme::Tokens};
use serde_json::json;

/// What the tests draw: two model-space lines from (0, 0) to (100, 0), `shown` on layer 0 and,
/// drawn after it and so on top of it, `hidden` on the layer "Hidden"; and two viewports on
/// Layout1, `a` freezing "Hidden" and `b` freezing nothing.
struct Sheet {
    app: CadApp,
    shown: Handle,
    hidden: Handle,
    /// The viewport that freezes "Hidden".
    a: String,
    /// The viewport that freezes nothing.
    b: String,
}

/// The drawing of [`Sheet`], in paper space with nothing selected.
fn sheet() -> Sheet {
    let mut app = CadApp::new(Session::new(), Services::default());
    app.ui.start_tab = false;
    app.ui.in_window_menu = false;
    app.integrated_titlebar = true;
    // A third line gives the model a height for the viewports to fit.
    for points in [[[0, 0], [100, 0]], [[0, 0], [100, 0]], [[0, 40], [100, 40]]] {
        app.run("line", json!({ "points": points })).unwrap();
    }
    let handles = app.session.doc().unwrap().model.handles();
    assert_eq!(handles.len(), 3);
    let (shown, hidden) = (handles[0], handles[1]);
    app.run("layer.new", json!({ "name": "Hidden" })).unwrap();
    app.run("properties.set", json!({ "handles": [hidden.hex()], "layer": "Hidden" })).unwrap();
    assert_eq!(app.session.doc().unwrap().entity(hidden).unwrap().common.layer, "Hidden");
    app.run("layout.set", json!({ "name": "Layout1" })).unwrap();
    let made = app.session.execute("mview", &json!({ "count": 2, "p1": [20, 20], "p2": [220, 120] })).unwrap();
    let viewport = |i: usize| made["viewports"][i].as_str().unwrap().to_string();
    let (a, b) = (viewport(0), viewport(1));
    app.run("vplayer", json!({ "handles": [a], "freeze": ["Hidden"] })).unwrap();
    // Show the part of the sheet that holds both viewports, with room around them.
    assert!(app.session.state_mut().unwrap().set_view(View { center: Vec2::new(120.0, 70.0), height: 300.0, twist: 0.0 }));
    app.session.set_selection(Vec::new());
    Sheet { app, shown, hidden, a, b }
}

/// Makes `viewport` the active MSPACE viewport. That clears the selection.
fn enter(app: &mut CadApp, viewport: &str) {
    app.run("mspace", json!({ "handle": viewport })).unwrap();
}

/// The grip rectangles (filled with `color`) inside `canvas` among the painted shapes.
fn count_grips(shape: &egui::Shape, canvas: egui::Rect, color: egui::Color32) -> usize {
    match shape {
        egui::Shape::Rect(rect) => usize::from(rect.fill == color && canvas.contains(rect.rect.center())),
        egui::Shape::Vec(shapes) => shapes.iter().map(|shape| count_grips(shape, canvas, color)).sum(),
        _ => 0,
    }
}

/// Runs one frame of the app with the pointer at `pointer` (none: away from the window) and
/// counts the grip rectangles painted on its canvas.
fn frame(app: &mut CadApp, ctx: &egui::Context, pointer: Option<egui::Pos2>) -> usize {
    app.logic(ctx);
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 1000.0))),
        events: vec![pointer.map_or(egui::Event::PointerGone, egui::Event::PointerMoved)],
        ..Default::default()
    };
    let mut output = ctx.run_ui(input, |ui| app.ui(ui));
    output.textures_delta.clear();
    let canvas = app.canvas.rect.unwrap();
    let color = Tokens::get().grip;
    output.shapes.iter().map(|shape| count_grips(&shape.shape, canvas, color)).sum()
}

/// The grip rectangles painted once the frame has settled, with the pointer away.
fn painted_grips(app: &mut CadApp, ctx: &egui::Context) -> usize {
    frame(app, ctx, None);
    frame(app, ctx, None)
}

/// Whether every grip of `h` is on the canvas, where it would be painted.
fn grips_on_canvas(app: &CadApp, h: Handle) -> bool {
    let xf = app.canvas.xf.unwrap();
    let state = app.session.state().unwrap();
    state.doc.entity(h).unwrap().kind.grips().iter().all(|point| xf.rect.contains(xf.to_screen(*point)))
}

/// A selected line paints its grips through a viewport that draws its layer. Once the layer is
/// frozen in that viewport the grips are gone although the line is still selected, the grips of
/// the other selected line stay, and thawing the layer brings them back. The same holds from one
/// viewport to the other.
#[test]
fn a_selected_object_the_viewport_does_not_draw_paints_no_grips() {
    let Sheet { mut app, shown, hidden, a, b } = sheet();
    let ctx = egui::Context::default();
    // Settle the dock layout.
    painted_grips(&mut app, &ctx);
    enter(&mut app, &b);
    app.session.set_selection(vec![shown]);
    let one = painted_grips(&mut app, &ctx);
    assert!(grips_on_canvas(&app, shown) && grips_on_canvas(&app, hidden), "the viewport must show both lines whole");
    assert!(one >= 2, "the counter must find the grips of one selected line, got {one}");
    app.session.set_selection(vec![shown, hidden]);
    assert_eq!(painted_grips(&mut app, &ctx), 2 * one, "this viewport draws both lines");

    // The layer is frozen in the active viewport while its line is selected.
    app.run("vplayer", json!({ "handles": [b], "freeze": ["Hidden"] })).unwrap();
    assert_eq!(app.session.state().unwrap().selection, [shown, hidden], "both lines must still be selected");
    assert_eq!(painted_grips(&mut app, &ctx), one, "a line the viewport no longer draws kept its grips");
    app.run("vplayer", json!({ "handles": [b], "thaw": ["Hidden"] })).unwrap();
    assert_eq!(app.session.state().unwrap().selection, [shown, hidden]);
    assert_eq!(painted_grips(&mut app, &ctx), 2 * one, "thawed in the viewport: the grips are back");

    // The same selection through the viewport that freezes the layer, and back.
    enter(&mut app, &a);
    app.session.set_selection(vec![shown, hidden]);
    assert_eq!(painted_grips(&mut app, &ctx), one, "a line the viewport does not draw painted grips");
    assert!(grips_on_canvas(&app, hidden), "its grips would be on the canvas");
    enter(&mut app, &b);
    app.session.set_selection(vec![shown, hidden]);
    assert_eq!(painted_grips(&mut app, &ctx), 2 * one);
}

/// The cursor highlights the object a click would select: over the two lines, the one the
/// viewport draws.
#[test]
fn the_cursor_highlights_the_object_the_viewport_draws() {
    let Sheet { mut app, shown, hidden, a, b } = sheet();
    let ctx = egui::Context::default();
    painted_grips(&mut app, &ctx);
    for (viewport, under_cursor) in [(&a, shown), (&b, hidden)] {
        enter(&mut app, viewport);
        painted_grips(&mut app, &ctx);
        let on_lines = app.canvas.xf.unwrap().to_screen(Vec2::new(50.0, 0.0));
        assert!(app.canvas.rect.unwrap().contains(on_lines), "the viewport must show the lines");
        frame(&mut app, &ctx, Some(on_lines));
        frame(&mut app, &ctx, Some(on_lines));
        assert_eq!(app.canvas.hover, Some(under_cursor));
    }
}

/// The layer of the line under a resting pointer is frozen in the active viewport, then thawed:
/// the highlight goes to the line the viewport still draws and comes back, without the pointer
/// moving in between.
#[test]
fn the_highlight_follows_the_frozen_layers_under_a_resting_pointer() {
    let Sheet { mut app, shown, hidden, a: _, b } = sheet();
    let ctx = egui::Context::default();
    painted_grips(&mut app, &ctx);
    enter(&mut app, &b);
    painted_grips(&mut app, &ctx);
    let on_lines = app.canvas.xf.unwrap().to_screen(Vec2::new(50.0, 0.0));
    assert!(app.canvas.rect.unwrap().contains(on_lines), "the viewport must show the lines");
    frame(&mut app, &ctx, Some(on_lines));
    frame(&mut app, &ctx, Some(on_lines));
    assert_eq!(app.canvas.hover, Some(hidden), "this viewport draws both lines, the later one on top");

    app.run("vplayer", json!({ "handles": [b], "freeze": ["Hidden"] })).unwrap();
    frame(&mut app, &ctx, Some(on_lines));
    assert_eq!(app.canvas.hover, Some(shown), "the cursor kept the line the viewport no longer draws");
    app.run("vplayer", json!({ "handles": [b], "thaw": ["Hidden"] })).unwrap();
    frame(&mut app, &ctx, Some(on_lines));
    assert_eq!(app.canvas.hover, Some(hidden), "thawed in the viewport: the line on top is under the cursor again");
}
