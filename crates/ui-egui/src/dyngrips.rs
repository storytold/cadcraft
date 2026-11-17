//! Dynamic block grips on the canvas, as AutoCAD shows them on a selected dynamic block
//! reference: an arrow at a distance's end (drag it to change the distance), a square at a
//! position (drag it to move it) and a down-pointing triangle for a list (click it for the
//! visibility states or lookup values). Dragging runs the engine's dynamic grip edit
//! ([`cadcraft_engine::Session::start_dyn_grip`]); a list choice runs `blocks.reference.set`.

use std::sync::Arc;

use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, vec2};
use serde_json::json;

use crate::CadApp;
use crate::canvas::Xf;
use crate::theme::Tokens;
use cadcraft_engine::doc::Handle;
use cadcraft_engine::doc::dynblock::{DynGrip, GripShape};

/// Most selected references whose grips are shown.
const MAX_REFERENCES: usize = 200;

/// The dynamic grips of the selection, worked out once per drawing revision, selection and set
/// of layers frozen in the active viewport: entering another viewport changes which references
/// are shown without changing the drawing.
#[derive(Clone, Default)]
pub struct Cache {
    key: (u64, u64, Vec<Handle>, Vec<String>),
    grips: Arc<Vec<(Handle, DynGrip)>>,
}

/// The selected references' dynamic grips (cached).
pub(crate) fn of_selection(app: &mut CadApp) -> Arc<Vec<(Handle, DynGrip)>> {
    let sel = app.session.selection();
    let Ok(st) = app.session.state() else { return Arc::default() };
    let key = (st.uid, st.revision, sel.clone(), st.viewport_frozen().to_vec());
    if app.canvas.dyn_grips.key != key {
        let grips: Vec<(Handle, DynGrip)> = sel
            .iter()
            .filter(|h| matches!(st.doc.entity(**h).map(|e| &e.kind), Some(cadcraft_engine::doc::EntityKind::Insert(_))))
            .filter(|h| st.doc.entity(**h).is_some_and(|e| st.shows(e) && !st.doc.layer(&e.common.layer).is_some_and(|l| l.locked)))
            .take(MAX_REFERENCES)
            .flat_map(|h| cadcraft_engine::doc::dynblock::grips(&st.doc, *h).into_iter().map(move |g| (*h, g)))
            .collect();
        app.canvas.dyn_grips = Cache { key, grips: Arc::new(grips) };
    }
    app.canvas.dyn_grips.grips.clone()
}

/// The screen outline of grip `g` (size `gs`, half the side of a square grip).
fn outline(xf: &Xf, g: &DynGrip, gs: f32) -> Vec<Pos2> {
    let c = xf.to_screen(g.at);
    match g.shape {
        GripShape::Linear => {
            // The arrow points the way the distance grows, on screen.
            let ahead = xf.to_screen(g.at + g.dir) - c;
            let u = if ahead.length() > 1e-6 { ahead / ahead.length() } else { vec2(1.0, 0.0) };
            let n = vec2(-u.y, u.x);
            let (len, half) = (gs * 1.6, gs * 1.1);
            vec![c - u * (len * 0.5) + n * half, c + u * (len * 0.8), c - u * (len * 0.5) - n * half]
        }
        GripShape::Point => {
            vec![c + vec2(-gs, -gs), c + vec2(gs, -gs), c + vec2(gs, gs), c + vec2(-gs, gs)]
        }
        GripShape::List => {
            let w = gs * 1.3;
            vec![c + vec2(-w, -w * 0.6), c + vec2(w, -w * 0.6), c + vec2(0.0, w * 0.9)]
        }
    }
}

/// Whether `p` is on grip `g` (its box, a little larger, as for other grips).
fn hit(xf: &Xf, g: &DynGrip, gs: f32, p: Pos2) -> bool {
    let r = Rect::from_points(&outline(xf, g, gs)).expand(3.0);
    r.contains(p)
}

/// Draw the grips of the selected dynamic references; the one under the cursor warm, the one
/// being dragged hot.
pub(crate) fn draw(app: &mut CadApp, painter: &Painter, xf: &Xf, rect: Rect, hover: Option<Pos2>) {
    let t = Tokens::get();
    let gs = app.session.settings.gripsize as f32;
    let hot = app.canvas.dyn_hot;
    for (h, g) in of_selection(app).iter() {
        if !rect.contains(xf.to_screen(g.at)) {
            continue;
        }
        let pts = outline(xf, g, gs);
        let is_hot = hot.as_ref().is_some_and(|(hh, n, p)| hh == h && *n == g.node && *p == g.part);
        let warm = !is_hot && hover.is_some_and(|p| hit(xf, g, gs, p));
        let fill = if is_hot {
            t.grip_hot
        } else if warm {
            Color32::from_rgb(0xff, 0x7f, 0x9f)
        } else {
            t.grip_dyn
        };
        painter.add(Shape::convex_polygon(pts, fill, Stroke::new(1.0, Color32::from_rgb(0x10, 0x30, 0x80))));
    }
}

/// The dynamic grip under `hover`, if any: (reference, grip).
pub(crate) fn at(app: &mut CadApp, xf: &Xf, hover: Option<Pos2>) -> Option<(Handle, DynGrip)> {
    let p = hover?;
    let gs = app.session.settings.gripsize as f32;
    of_selection(app).iter().find(|(_, g)| hit(xf, g, gs, p)).cloned()
}

/// A click on dynamic grip `g` of reference `h`: drag it, or open its list.
pub(crate) fn click(app: &mut CadApp, (h, g): (Handle, DynGrip)) {
    if g.shape == GripShape::List {
        app.canvas.dyn_menu = Some((h, g));
        app.canvas.dyn_menu_fresh = true;
        return;
    }
    app.canvas.hot_grips.clear();
    match app.session.start_dyn_grip(h, g.node, g.part) {
        Ok(()) => app.canvas.dyn_hot = Some((h, g.node, g.part)),
        Err(e) => app.session.echo(e.to_string()),
    }
}

/// The open list of a list grip: its choices under the grip, the current one checked. A choice
/// sets the property (one undo step); a click elsewhere closes it.
pub(crate) fn menu(app: &mut CadApp, ctx: &egui::Context, xf: &Xf) {
    let Some((h, g)) = app.canvas.dyn_menu.clone() else { return };
    let current = app
        .session
        .state()
        .ok()
        .and_then(|st| cadcraft_engine::doc::dynblock::custom_properties(&st.doc, h))
        .and_then(|(_, ps)| ps.into_iter().find(|p| p.name.eq_ignore_ascii_case(&g.property)))
        .map(|p| p.value)
        .unwrap_or_default();
    let gs = app.session.settings.gripsize as f32;
    let at = xf.to_screen(g.at) + vec2(-gs, gs * 1.5);
    let mut chosen: Option<String> = None;
    let area = egui::Area::new(egui::Id::unique(("dyn-grip-list", h, g.node))).order(egui::Order::Foreground).fixed_pos(at).show(ctx, |ui| {
        egui::Frame::popup(ui.style()).show(ui, |ui| {
            for c in &g.choices {
                if ui.selectable_label(c.eq_ignore_ascii_case(&current), c.as_str()).clicked() {
                    chosen = Some(c.clone());
                }
            }
        });
    });
    // The click that opened the list isn't a click elsewhere.
    let fresh = std::mem::take(&mut app.canvas.dyn_menu_fresh);
    let clicked_elsewhere = !fresh && ctx.input(|i| i.pointer.any_pressed()) && !area.response.contains_pointer();
    if let Some(value) = chosen {
        app.canvas.dyn_menu = None;
        if let Err(e) = app.run("blocks.reference.set", json!({ "handle": h.hex(), "name": g.property, "value": value })) {
            app.session.echo(e);
        }
    } else if clicked_elsewhere || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        app.canvas.dyn_menu = None;
    }
}

#[cfg(test)]
mod tests {
    use cadcraft_engine::Session;
    use cadcraft_engine::doc::{Handle, Space};
    use serde_json::json;

    use super::of_selection;
    use crate::{CadApp, Services};

    /// DXF text from lines of `code value`.
    fn dxf(src: &str) -> String {
        src.lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(|l| {
                let (code, value) = l.split_once(' ').unwrap_or((l, ""));
                format!("{code}\n{value}\n")
            })
            .collect()
    }

    /// A session with the synthetic DOOR block of the io tests (Width's end grip shown in the
    /// Open state, plus the visibility grip) and a reference to it at (100, 0).
    fn door() -> (Session, Handle) {
        let grip =
            "0 BLOCKLINEARGRIP\n5 68\n330 61\n100 AcDbEvalExpr\n90 9\n98 33\n99 232\n100 AcDbBlockElement\n300 End Grip\n98 33\n99 232\n1071 0\n";
        let src = include_str!("../../io/tests/data/dynblock_door.txt")
            .replace("\r\n", "\n")
            .replace("170 4\n91 0\n91 0\n", "170 4\n91 8\n91 9\n")
            .replace("1030 0\n93 0\n170 0\n171 0\n100 AcDbBlockVisibilityParameter", "1030 0\n93 10\n170 0\n171 0\n100 AcDbBlockVisibilityParameter")
            .replace("91 5\n93 32\n95 6\n360 67", "91 5\n93 32\n95 6\n360 67\n92 -1\n92 -1\n92 -1\n92 -1\n91 6\n93 32\n95 9\n360 68")
            .replace("95 1\n333 63\n", "95 2\n333 63\n333 68\n")
            .replace("0 BLOCKFLIPGRIP", &format!("{grip}0 BLOCKFLIPGRIP"));
        let d = cadcraft_io::read_dxf(dxf(&src).as_bytes()).unwrap();
        let mut s = Session::empty();
        s.open_drawing(d, "door", None);
        let r = s.execute("insert", &json!({ "name": "Door", "at": [100, 0] })).unwrap();
        let h = Handle::parse_hex(r["handle"].as_str().unwrap()).unwrap();
        assert!(s.doc().unwrap().space_of(h) == Some(Space::Model));
        (s, h)
    }

    /// An app with the reference of [`door`] on the layer "Hidden", and two viewports on Layout1
    /// over the model: the first freezes "Hidden", the second freezes nothing. Returns the app,
    /// the reference and the two viewports, in paper space.
    fn door_in_two_viewports() -> (CadApp, Handle, String, String) {
        let (session, h) = door();
        let mut app = CadApp::new(session, Services::default());
        app.session.execute("layer.new", &json!({ "name": "Hidden" })).unwrap();
        app.session.execute("properties.set", &json!({ "handles": [h.hex()], "layer": "Hidden" })).unwrap();
        assert_eq!(app.session.doc().unwrap().entity(h).unwrap().common.layer, "Hidden");
        app.session.execute("layout.set", &json!({ "name": "Layout1" })).unwrap();
        let made = app.session.execute("mview", &json!({ "count": 2, "p1": [20, 20], "p2": [220, 120] })).unwrap();
        let viewport = |i: usize| made["viewports"][i].as_str().unwrap().to_string();
        let (a, b) = (viewport(0), viewport(1));
        app.session.execute("vplayer", &json!({ "handles": [a], "freeze": ["Hidden"] })).unwrap();
        (app, h, a, b)
    }

    /// Makes `viewport` the active MSPACE viewport and selects `h` there (entering a viewport
    /// clears the selection; Undo or another selection source can put the reference back).
    fn enter_and_select(app: &mut CadApp, viewport: &str, h: Handle) {
        app.session.execute("mspace", &json!({ "handle": viewport })).unwrap();
        app.session.set_selection(vec![h]);
    }

    /// A selected reference shows its dynamic grips through a viewport that draws its layer. Once
    /// the layer is frozen in that viewport the grips are gone although the reference is still
    /// selected, and thawing the layer there brings them back.
    #[test]
    fn a_reference_the_viewport_does_not_draw_has_no_dynamic_grips() {
        let (mut app, h, _, b) = door_in_two_viewports();
        enter_and_select(&mut app, &b, h);
        assert_eq!(of_selection(&mut app).len(), 2, "this viewport draws the reference: its two grips");
        app.session.execute("vplayer", &json!({ "handles": [b], "freeze": ["Hidden"] })).unwrap();
        assert_eq!(app.session.state().unwrap().selection, [h], "the reference must still be selected");
        assert!(of_selection(&mut app).is_empty(), "a reference the viewport no longer draws kept its dynamic grips");
        app.session.execute("vplayer", &json!({ "handles": [b], "thaw": ["Hidden"] })).unwrap();
        assert_eq!(app.session.state().unwrap().selection, [h]);
        assert_eq!(of_selection(&mut app).len(), 2, "thawed in the viewport: the grips are back");
    }

    /// Entering another viewport changes which references are drawn without changing the drawing,
    /// so the cached grips of the same selection must not be reused from one viewport to the next.
    #[test]
    fn the_dynamic_grip_cache_follows_the_active_viewport() {
        let (mut app, h, a, b) = door_in_two_viewports();
        enter_and_select(&mut app, &b, h);
        assert_eq!(of_selection(&mut app).len(), 2);
        let revision = app.session.state().unwrap().revision;
        enter_and_select(&mut app, &a, h);
        assert_eq!(app.session.state().unwrap().revision, revision, "entering a viewport must not look like a change of the drawing");
        assert!(of_selection(&mut app).is_empty(), "the grips cached in the other viewport were reused where the reference is not drawn");
        enter_and_select(&mut app, &b, h);
        assert_eq!(app.session.state().unwrap().revision, revision);
        assert_eq!(of_selection(&mut app).len(), 2, "the empty result cached in the other viewport was reused where the reference is drawn");
    }

    /// A hidden reference restored to the selection (Undo, or another selection source) shows
    /// and takes no dynamic grips, as ordinary grips are not shown for it either.
    #[test]
    fn a_hidden_reference_has_no_dynamic_grips() {
        let (session, h) = door();
        let mut app = CadApp::new(session, Services::default());
        app.session.set_selection(vec![h]);
        assert_eq!(of_selection(&mut app).len(), 2, "a visible reference shows its two grips");
        app.run("hideobjects", json!({ "handles": [h.hex()] })).unwrap();
        app.session.set_selection(vec![h]);
        let st = app.session.state().unwrap();
        assert!(!st.doc.is_visible(st.doc.entity(h).unwrap()), "the reference must be hidden");
        assert!(of_selection(&mut app).is_empty(), "a hidden reference must not offer dynamic grips");
    }

    /// BEDIT without a block name opens the editor on the one selected reference, and its dialog
    /// when there is none. A selected reference the viewport does not draw is none to start from.
    #[test]
    fn bedit_opens_its_dialog_when_the_viewport_does_not_draw_the_selected_reference() {
        let (mut app, h, a, b) = door_in_two_viewports();
        enter_and_select(&mut app, &b, h);
        assert!(crate::bedit::route(&mut app, "bedit", &serde_json::Value::Null).is_none(), "one reference is selected: no dialog");
        assert_eq!(app.ui.dialog, None);
        enter_and_select(&mut app, &a, h);
        assert!(crate::bedit::route(&mut app, "bedit", &serde_json::Value::Null).is_some(), "the dialog must open");
        assert_eq!(app.ui.dialog.as_deref(), Some("bedit"));
    }
}
