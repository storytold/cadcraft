//! A rectangle's width and height fields at the cursor: while a prompt sizes a rectangle from its
//! first corner ([`cadcraft_engine::Prompt::rect`], RECTANG's other corner), Dynamic Input shows
//! the rubber-band rectangle's width and height as value fields on dimension lines along its
//! sides, in place of the pointer boxes. Typing goes into the active field; Tab locks a typed
//! length and moves to the other field (and back); a locked side keeps its length while the other
//! follows the cursor, and the cursor's side of the first corner gives each length its direction.
//! Typing in a locked field and Tab again replaces the lock. Enter places the corner the typed
//! lengths give (the command gets a plain point).
//!
//! The command-line text is the active field's text, so Backspace and Escape behave as usual;
//! text that isn't a length (`@4,2`, `6,6`, a keyword such as `D`) goes to the command as typed.

use cadcraft_engine::{Input, RectSize};
use cadcraft_geom::Vec2;
use egui::{Pos2, Stroke, vec2};

use crate::CadApp;
use crate::theme::Tokens;

/// The width field.
pub const WIDTH: usize = 0;
/// The height field.
pub const HEIGHT: usize = 1;

/// The fields' state for the rectangle being sized.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RectDyn {
    /// The rectangle the fields belong to; another one (a new first corner, a new rotation)
    /// starts over.
    rect: Option<RectSize>,
    /// The active field ([`WIDTH`] or [`HEIGHT`]).
    pub active: usize,
    /// Locked lengths as typed, by field.
    pub locked: [Option<String>; 2],
}

/// The rectangle on show, after bringing the fields' state up to date with the prompt (`None`
/// when no prompt sizes a rectangle or the value boxes aren't live).
pub fn rect(app: &mut CadApp) -> Option<RectSize> {
    let r = crate::cmdline::dyn_frame(app).and_then(|_| app.session.current_prompt()).and_then(|p| p.rect);
    let st = &mut app.canvas.rect_dyn;
    if st.rect != r {
        *st = RectDyn { rect: r, ..RectDyn::default() };
    }
    r
}

/// A typed length: a distance that isn't zero, as a positive number (the cursor gives the
/// direction).
fn length(text: &str) -> Option<f64> {
    cadcraft_engine::units::parse_distance(text.trim()).filter(|v| v.is_finite() && v.abs() > 1e-12).map(f64::abs)
}

/// The lengths that hold the sides: the active field's typed value, else the locked ones.
fn values(app: &CadApp) -> [Option<f64>; 2] {
    let st = &app.canvas.rect_dyn;
    let field = |i: usize| {
        let typed = if i == st.active { length(&app.cmd.buffer) } else { None };
        typed.or_else(|| st.locked.get(i).and_then(|l| l.as_deref()).and_then(length))
    };
    [field(WIDTH), field(HEIGHT)]
}

/// The cursor held by the typed lengths, or `None` when nothing holds it.
pub fn constrain(app: &mut CadApp, p: Vec2) -> Option<Vec2> {
    let r = rect(app)?;
    let [w, h] = values(app);
    if w.is_none() && h.is_none() {
        return None;
    }
    Some(r.corner(w, h, p)).filter(|q| q.is_finite())
}

/// Tab: lock the active field's typed length and move to the other field. `false` when no
/// rectangle fields are on show or the text isn't a length (Tab does its usual thing).
pub fn tab(app: &mut CadApp) -> bool {
    if rect(app).is_none() {
        return false;
    }
    let typed = app.cmd.buffer.trim().to_owned();
    if !typed.is_empty() && length(&typed).is_none() {
        return false;
    }
    let st = &mut app.canvas.rect_dyn;
    if !typed.is_empty()
        && let Some(slot) = st.locked.get_mut(st.active)
    {
        *slot = Some(typed);
        app.cmd.buffer.clear();
    }
    st.active = if st.active == WIDTH { HEIGHT } else { WIDTH };
    true
}

/// Enter: place the corner the typed lengths give, the cursor sizing any side left open. `false`
/// when nothing is typed or locked, or the text isn't a length (Enter or the text then go to the
/// command as usual).
pub fn submit(app: &mut CadApp) -> bool {
    let Some(r) = rect(app) else { return false };
    if !app.cmd.buffer.trim().is_empty() && length(&app.cmd.buffer).is_none() {
        return false;
    }
    let [w, h] = values(app);
    if w.is_none() && h.is_none() {
        return false;
    }
    let cursor = app.canvas.cursor.unwrap_or(app.session.cursor);
    let p = r.corner(w, h, cursor);
    if !p.is_finite() {
        return false;
    }
    app.cmd.buffer.clear();
    app.canvas.rect_dyn = RectDyn::default();
    if let Err(e) = app.session.input(Input::Point(p)) {
        app.session.echo(e.to_string());
    }
    true
}

/// Draw the width and height fields on dimension lines outside the rubber-band rectangle;
/// `false` when there are none (the pointer boxes show instead).
pub fn draw(app: &mut CadApp, p: &egui::Painter) -> bool {
    let Some(r) = rect(app) else { return false };
    let (Some(xf), Some(c)) = (app.canvas.xf, app.canvas.cursor) else { return false };
    let t = Tokens::get();
    let font = crate::theme::small();
    let (lu, lp) = crate::canvas::length_units(&app.session);
    let st = app.canvas.rect_dyn.clone();
    let (w, h) = r.size(c);
    let (o, w_end, h_end) = r.sides((w, h));
    let (so, sw, sh) = (xf.to_screen(o), xf.to_screen(w_end), xf.to_screen(h_end));
    // The rectangle's middle on screen: each dimension line sits on the far side of its edge.
    let mid = so + ((sw - so) + (sh - so)) / 2.0;
    let view = app.canvas.rect.unwrap_or_else(|| p.clip_rect());
    let line = Stroke::new(1.0, t.text_dim);
    let ext = Stroke::new(1.0, t.text_faint);
    let mut fields = Vec::with_capacity(2);
    for (i, (a, b), live) in [(WIDTH, (so, sw), w.abs()), (HEIGHT, (so, sh), h.abs())] {
        let along = b - a;
        let len = along.length();
        // A side with no length yet: lay the line out along the rectangle's own direction.
        let dir = if len > 1e-3 {
            along / len
        } else {
            let d = Vec2::from_angle(r.rotation + if i == WIDTH { 0.0 } else { std::f64::consts::FRAC_PI_2 });
            vec2(d.x as f32, -d.y as f32)
        };
        let mut n = vec2(-dir.y, dir.x);
        if n.dot((a + along / 2.0) - mid) < 0.0 {
            n = -n;
        }
        // Outside the view (an edge near the canvas border): the line goes inside the edge.
        if !view.contains(a + along / 2.0 + n * 24.0) && view.contains(a + along / 2.0 - n * 24.0) {
            n = -n;
        }
        let off = n * 24.0;
        p.line_segment([a + off, b + off], line);
        p.line_segment([a + off * 0.2, a + off * 1.25], ext);
        p.line_segment([b + off * 0.2, b + off * 1.25], ext);
        arrow(p, a + off, dir, t.text_dim);
        arrow(p, b + off, -dir, t.text_dim);
        let active = i == st.active;
        let locked = st.locked.get(i).and_then(|l| l.as_deref());
        let typing = active && !app.cmd.buffer.is_empty();
        let text = if typing {
            app.cmd.buffer.clone()
        } else if let Some(l) = locked {
            l.to_owned()
        } else {
            cadcraft_engine::units::format_distance(live, lu, lp)
        };
        let lock = locked.is_some() && !typing;
        let size = field_size(p, &text, &font, lock);
        fields.push((a + off + along / 2.0, size, text, active, lock, locked.is_none() && !typing));
    }
    if let [wf, hf] = fields.as_mut_slice() {
        let [wa, ha] = place(view, [(wf.0, wf.1), (hf.0, hf.1)]);
        (wf.0, hf.0) = (wa, ha);
    }
    for (at, _, text, active, lock, live) in &fields {
        crate::grip_dyn::field(p, *at, text, &font, *active, *lock, *live);
    }
    true
}

/// The size [`crate::grip_dyn::field`] draws a field of `text` at.
fn field_size(p: &egui::Painter, text: &str, font: &egui::FontId, locked: bool) -> egui::Vec2 {
    let g = p.layout_no_wrap(text.to_owned(), font.clone(), Tokens::get().text);
    let lock_w = if locked { 11.0 } else { 0.0 };
    vec2((g.size().x + 10.0 + lock_w).max(40.0), g.size().y + 6.0)
}

/// Where the width and height fields go, given where their dimension lines want them (centre,
/// size): each is kept inside `view`, and when the two overlap (a small rectangle, or one pressed
/// against an edge) they are pushed apart along the axis that overlaps least.
fn place(view: egui::Rect, [(wa, ws), (ha, hs)]: [(Pos2, egui::Vec2); 2]) -> [Pos2; 2] {
    const GAP: f32 = 4.0;
    let keep_in = |c: Pos2, s: egui::Vec2| {
        let half = s / 2.0 + vec2(2.0, 2.0);
        let (lo, hi) = (view.min + half, view.max - half);
        let axis = |v: f32, lo: f32, hi: f32, mid: f32| if lo <= hi { v.clamp(lo, hi) } else { mid };
        egui::pos2(axis(c.x, lo.x, hi.x, view.center().x), axis(c.y, lo.y, hi.y, view.center().y))
    };
    let (mut w, mut h) = (keep_in(wa, ws), keep_in(ha, hs));
    let box_of = |c: Pos2, s: egui::Vec2| egui::Rect::from_center_size(c, s + vec2(GAP, GAP));
    for _ in 0..2 {
        let o = box_of(w, ws).intersect(box_of(h, hs));
        if o.width() <= 0.0 || o.height() <= 0.0 {
            break;
        }
        // Away from the width field; the height field moves first, the width field takes what
        // the view's edge leaves over.
        let away = h - w;
        let push = if o.width() < o.height() {
            vec2(if away.x < 0.0 { -o.width() } else { o.width() }, 0.0)
        } else {
            vec2(0.0, if away.y < 0.0 { -o.height() } else { o.height() })
        };
        let moved = keep_in(h + push, hs);
        let left = push - (moved - h);
        h = moved;
        w = keep_in(w - left, ws);
    }
    [w, h]
}

/// A small filled arrowhead with its tip at `tip`, pointing along `dir` backwards (into the line).
fn arrow(p: &egui::Painter, tip: Pos2, dir: egui::Vec2, color: egui::Color32) {
    let n = vec2(-dir.y, dir.x);
    let back = tip + dir * 7.0;
    p.add(egui::Shape::convex_polygon(vec![tip, back + n * 2.5, back - n * 2.5], color, Stroke::NONE));
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadcraft_doc::EntityKind;
    use egui::{Event, Rect, pos2};

    fn run_frame(app: &mut CadApp, ctx: &egui::Context, events: Vec<Event>) {
        let raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1400.0, 900.0))), events, ..Default::default() };
        let mut out = ctx.run_ui(raw, |ui| {
            app.logic(ui.ctx());
            app.ui(ui);
        });
        out.textures_delta.clear();
    }

    fn key(key: egui::Key) -> Vec<Event> {
        let m = egui::Modifiers::NONE;
        vec![
            Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: m },
            Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers: m },
        ]
    }

    fn typed(app: &mut CadApp, ctx: &egui::Context, text: &str) {
        run_frame(app, ctx, vec![Event::Text(text.into())]);
    }

    /// Tab reaches the command line as text (`cmdline::capture_tab`).
    fn tab(app: &mut CadApp, ctx: &egui::Context) {
        typed(app, ctx, "\t");
    }

    /// The last rectangle's (min, max) corners.
    fn last_rect(app: &CadApp) -> (Vec2, Vec2) {
        let pts: Vec<Vec2> = app
            .session
            .doc()
            .ok()
            .and_then(|d| match &d.model.iter().last()?.kind {
                EntityKind::LwPolyline(p) => Some(p.vertices.iter().map(|v| v.p).collect()),
                _ => None,
            })
            .unwrap_or_default();
        let b = cadcraft_geom::Bounds2::from_points(pts);
        (b.min, b.max)
    }

    fn close(a: Vec2, x: f64, y: f64) -> bool {
        a.dist(Vec2::new(x, y)) < 1e-9
    }

    /// A RECTANG waiting for its other corner at (0,0), with the pointer at `at` (screen offset
    /// from the corner).
    fn rectang_at(at: egui::Vec2) -> (CadApp, egui::Context, Pos2) {
        let mut app = CadApp::new(cadcraft_engine::Session::new(), crate::Services::default());
        let ctx = egui::Context::default();
        run_frame(&mut app, &ctx, Vec::new());
        let o = app.canvas.xf.map_or(pos2(700.0, 450.0), |x| x.to_screen(Vec2::ZERO));
        app.cmdline("rectang");
        app.session.input(Input::Point(Vec2::ZERO)).unwrap();
        let pointer = o + at;
        run_frame(&mut app, &ctx, vec![Event::PointerMoved(pointer)]);
        (app, ctx, pointer)
    }

    #[test]
    fn width_tab_height_enter_draws_the_exact_rectangle() {
        let (mut app, ctx, _) = rectang_at(vec2(130.0, -70.0));
        assert!(rect(&mut app).is_some(), "width and height fields");
        typed(&mut app, &ctx, "10");
        tab(&mut app, &ctx);
        assert_eq!(app.canvas.rect_dyn.locked, [Some("10".to_owned()), None]);
        assert_eq!(app.canvas.rect_dyn.active, HEIGHT);
        assert!(app.cmd.buffer.is_empty());
        typed(&mut app, &ctx, "5");
        run_frame(&mut app, &ctx, key(egui::Key::Enter));
        assert!(app.session.running.is_none());
        let (lo, hi) = last_rect(&app);
        assert!(close(lo, 0.0, 0.0) && close(hi, 10.0, 5.0), "{lo:?} {hi:?}");
    }

    #[test]
    fn a_locked_side_holds_and_the_cursor_sizes_the_other() {
        // The first corner in the middle of the view, the pointer below and left of it: the
        // locked width runs to the left.
        let mut app = CadApp::new(cadcraft_engine::Session::new(), crate::Services::default());
        let ctx = egui::Context::default();
        run_frame(&mut app, &ctx, Vec::new());
        let (xf, mid) = (app.canvas.xf.unwrap(), app.canvas.rect.unwrap().center());
        let a = xf.to_world(mid);
        app.cmdline("rectang");
        app.session.input(Input::Point(a)).unwrap();
        run_frame(&mut app, &ctx, vec![Event::PointerMoved(mid + vec2(-130.0, 70.0))]);
        typed(&mut app, &ctx, "10");
        tab(&mut app, &ctx);
        // Move the pointer: the width stays 10, the height follows.
        let pointer = mid + vec2(-170.0, 100.0);
        run_frame(&mut app, &ctx, vec![Event::PointerMoved(pointer)]);
        let c = app.canvas.cursor.unwrap();
        assert!((c.x - (a.x - 10.0)).abs() < 1e-9, "{c:?}");
        assert!((c.y - xf.to_world(pointer).y).abs() < 1e-9, "{c:?}");
        run_frame(&mut app, &ctx, key(egui::Key::Enter));
        let (lo, hi) = last_rect(&app);
        assert!(close(lo, a.x - 10.0, c.y) && close(hi, a.x, a.y), "{lo:?} {hi:?}");
    }

    #[test]
    fn tab_back_to_a_locked_field_and_retype_changes_its_lock() {
        let (mut app, ctx, _) = rectang_at(vec2(130.0, -70.0));
        typed(&mut app, &ctx, "10");
        tab(&mut app, &ctx);
        typed(&mut app, &ctx, "5");
        tab(&mut app, &ctx);
        assert_eq!(app.canvas.rect_dyn.locked, [Some("10".to_owned()), Some("5".to_owned())]);
        assert_eq!(app.canvas.rect_dyn.active, WIDTH, "Tab goes back to the width");
        // Both locked: the cursor sits on the corner.
        let c = app.canvas.cursor.unwrap();
        assert!(close(c, 10.0, 5.0), "{c:?}");
        // Retype the width: the new value replaces the lock.
        typed(&mut app, &ctx, "7");
        tab(&mut app, &ctx);
        assert_eq!(app.canvas.rect_dyn.locked, [Some("7".to_owned()), Some("5".to_owned())]);
        run_frame(&mut app, &ctx, key(egui::Key::Enter));
        let (lo, hi) = last_rect(&app);
        assert!(close(lo, 0.0, 0.0) && close(hi, 7.0, 5.0), "{lo:?} {hi:?}");
    }

    #[test]
    fn points_and_keywords_still_answer_the_prompt() {
        let (mut app, ctx, _) = rectang_at(vec2(130.0, -70.0));
        typed(&mut app, &ctx, "@4,2");
        run_frame(&mut app, &ctx, key(egui::Key::Enter));
        let (_, hi) = last_rect(&app);
        assert!(close(hi, 4.0, 2.0), "{hi:?}");

        let (mut app, ctx, _) = rectang_at(vec2(130.0, -70.0));
        // A keyword goes to the command, even with a length locked.
        typed(&mut app, &ctx, "10");
        tab(&mut app, &ctx);
        typed(&mut app, &ctx, "r");
        run_frame(&mut app, &ctx, key(egui::Key::Enter));
        assert!(app.session.current_prompt().is_some_and(|p| p.message.starts_with("Specify rotation angle")));
        typed(&mut app, &ctx, "90");
        run_frame(&mut app, &ctx, key(egui::Key::Enter));
        // The rotation turned the fields: a new rectangle, so the locks start over.
        let r = rect(&mut app).unwrap();
        assert!((r.rotation - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
        assert_eq!(app.canvas.rect_dyn.locked, [None, None]);
        typed(&mut app, &ctx, "6");
        tab(&mut app, &ctx);
        typed(&mut app, &ctx, "2");
        run_frame(&mut app, &ctx, key(egui::Key::Enter));
        assert!(app.session.running.is_none());
        let (lo, hi) = last_rect(&app);
        // Width 6 up the Y axis, height 2 to the left or right (the pointer's side).
        assert!((hi.y - lo.y - 6.0).abs() < 1e-9 && (hi.x - lo.x - 2.0).abs() < 1e-9, "{lo:?} {hi:?}");
    }

    #[test]
    fn fields_are_drawn_with_the_live_lengths() {
        let (mut app, ctx, pointer) = rectang_at(vec2(130.0, -70.0));
        app.ui.show_ucs_icon = false;
        let text = crate::canvas::tests::painted_text(&mut app, &ctx, pointer);
        let c = app.canvas.cursor.unwrap();
        let (lu, lp) = crate::canvas::length_units(&app.session);
        for v in [c.x.abs(), c.y.abs()] {
            let s = cadcraft_engine::units::format_distance(v, lu, lp);
            assert!(text.contains(&s), "no {s} in:\n{text}");
        }
        assert!(text.contains("Specify other corner point"), "{text}");
        assert!(!text.lines().any(|l| l == "X" || l == "Y"), "no pointer boxes:\n{text}");
    }

    #[test]
    fn centre_mode_types_the_whole_width_and_height() {
        // CEnter: the first point is the centre, so the picked corner is half the size away.
        let mut app = CadApp::new(cadcraft_engine::Session::new(), crate::Services::default());
        let ctx = egui::Context::default();
        run_frame(&mut app, &ctx, Vec::new());
        let o = app.canvas.xf.map_or(pos2(700.0, 450.0), |x| x.to_screen(Vec2::ZERO));
        app.cmdline("rectang");
        app.session.input(Input::Keyword("CEnter".into())).unwrap();
        app.session.input(Input::Point(Vec2::ZERO)).unwrap();
        run_frame(&mut app, &ctx, vec![Event::PointerMoved(o + vec2(130.0, -70.0))]);
        assert!(rect(&mut app).is_some_and(|r| r.center), "centre-mode fields");
        typed(&mut app, &ctx, "10");
        tab(&mut app, &ctx);
        typed(&mut app, &ctx, "4");
        tab(&mut app, &ctx);
        // Both locked: the cursor holds the corner half the size from the centre.
        let c = app.canvas.cursor.unwrap();
        assert!(close(c, 5.0, 2.0), "{c:?}");
        run_frame(&mut app, &ctx, key(egui::Key::Enter));
        assert!(app.session.running.is_none());
        let (lo, hi) = last_rect(&app);
        assert!(close(lo, -5.0, -2.0) && close(hi, 5.0, 2.0), "{lo:?} {hi:?}");
    }

    #[test]
    fn fields_are_drawn_with_the_pointer_on_the_first_corner() {
        // Both sides have no length: the dimension lines fall back to the rectangle's own
        // directions, and both fields still show (with zero), apart from each other.
        // The first corner in the middle of the view (the origin is at the canvas's edge).
        let mut app = CadApp::new(cadcraft_engine::Session::new(), crate::Services::default());
        let ctx = egui::Context::default();
        run_frame(&mut app, &ctx, Vec::new());
        let (xf, pointer) = (app.canvas.xf.unwrap(), app.canvas.rect.unwrap().center());
        app.cmdline("rectang");
        app.session.input(Input::Point(xf.to_world(pointer))).unwrap();
        run_frame(&mut app, &ctx, vec![Event::PointerMoved(pointer)]);
        app.ui.show_ucs_icon = false;
        let text = crate::canvas::tests::painted_text(&mut app, &ctx, pointer);
        let (lu, lp) = crate::canvas::length_units(&app.session);
        let zero = cadcraft_engine::units::format_distance(0.0, lu, lp);
        assert_eq!(text.lines().filter(|l| *l == zero).count(), 2, "two {zero} fields in:\n{text}");
        assert!(!text.lines().any(|l| l == "X" || l == "Y"), "no pointer boxes:\n{text}");
    }

    #[test]
    fn fields_stay_in_the_view_and_apart() {
        let view = Rect::from_min_size(Pos2::ZERO, vec2(400.0, 300.0));
        let s = vec2(60.0, 18.0);
        let inside = |c: Pos2| view.contains_rect(Rect::from_center_size(c, s));
        // Past the right and bottom edges: pulled back in.
        let [w, h] = place(view, [(pos2(420.0, 150.0), s), (pos2(200.0, 330.0), s)]);
        assert!(inside(w) && inside(h), "{w:?} {h:?}");
        // On top of each other (a tiny rectangle): pushed apart.
        let [w, h] = place(view, [(pos2(200.0, 150.0), s), (pos2(205.0, 152.0), s)]);
        assert!(!Rect::from_center_size(w, s).intersects(Rect::from_center_size(h, s)), "{w:?} {h:?}");
        // Overlapping in a corner: apart and still inside.
        let [w, h] = place(view, [(pos2(395.0, 295.0), s), (pos2(398.0, 290.0), s)]);
        assert!(inside(w) && inside(h), "{w:?} {h:?}");
        assert!(!Rect::from_center_size(w, s).intersects(Rect::from_center_size(h, s)), "{w:?} {h:?}");
        // Already apart and inside: left where the dimension lines put them.
        let (a, b) = (pos2(100.0, 100.0), pos2(300.0, 200.0));
        assert_eq!(place(view, [(a, s), (b, s)]), [a, b]);
    }
}
