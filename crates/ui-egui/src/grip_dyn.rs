//! Grip dimensional input at the cursor: while one grip is stretched, the dimensions it changes
//! ([`cadcraft_engine::grip_dims`]) show as value fields beside the object in place of the pointer
//! boxes. Typing goes into the active field; Tab locks a typed value and moves to the next field;
//! locked values hold the grip; Enter places it there (the GRIP command gets a plain point).
//!
//! The command-line text is the active field's text, so Backspace and Escape behave as usual;
//! text that isn't a value (a keyword such as `C` for Copy) goes to the command as typed.
//!
//! A prompt that asks for a distance from its base ([`cadcraft_engine::Prompt::distance`]: a
//! circle's radius, a polyline arc's) shows the same way, as one length field from the base, and
//! Enter sends the typed value to the command as a number.

use cadcraft_doc::Handle;
use cadcraft_engine::Input;
use cadcraft_engine::grip_dims::{GripDim, GripDimKind, grip_dims, resolve};
use cadcraft_geom::Vec2;
use egui::{Color32, Pos2, Rect, Shape, Stroke, pos2, vec2};

use crate::CadApp;
use crate::theme::Tokens;

/// The fields' state for the grip being stretched.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GripDyn {
    /// The grip the fields belong to; another grip starts over.
    grip: Option<(Handle, usize)>,
    /// The base of the distance prompt the field belongs to; another one starts over.
    base: Option<Vec2>,
    /// The active field.
    pub active: usize,
    /// Locked fields: (field, value as typed).
    pub locked: Vec<(usize, String)>,
}

/// The base of the prompt on show when it asks for a distance from it (a circle's radius, a
/// polyline arc's): its one field is that distance.
fn distance_base(app: &CadApp) -> Option<Vec2> {
    crate::cmdline::dyn_frame(app).and_then(|_| app.session.current_prompt()).filter(|p| p.distance).and_then(|p| p.base)
}

/// The dimensions on show, after bringing the fields' state up to date with the prompt (empty when
/// no grip is stretched, no distance is asked for, or the value boxes aren't live).
pub fn dims(app: &mut CadApp) -> Vec<GripDim> {
    let grip = crate::cmdline::dyn_frame(app).and_then(|_| app.session.current_prompt()).and_then(|p| p.grip);
    let base = distance_base(app);
    let st = &mut app.canvas.grip_dyn;
    if st.grip != grip || st.base != base {
        *st = GripDyn { grip, base, ..GripDyn::default() };
    }
    if grip.is_none() {
        return base.map(|b| vec![GripDim { kind: GripDimKind::Length, anchor: b, original: 0.0 }]).unwrap_or_default();
    }
    let dims = grip_dims(&app.session);
    if app.canvas.grip_dyn.active >= dims.len() {
        app.canvas.grip_dyn.active = 0;
    }
    dims
}

/// The values that hold the grip: the locked ones, and the active field's typed value.
fn values(app: &CadApp, dims: &[GripDim]) -> Vec<(usize, f64)> {
    let st = &app.canvas.grip_dyn;
    let mut out: Vec<(usize, f64)> = st
        .locked
        .iter()
        .filter(|(i, _)| *i != st.active || app.cmd.buffer.trim().is_empty())
        .filter_map(|(i, t)| Some((*i, dims.get(*i)?.parse(&app.session, t)?)))
        .collect();
    if let Some(d) = dims.get(st.active)
        && let Some(v) = d.parse(&app.session, &app.cmd.buffer)
    {
        out.push((st.active, v));
    }
    out
}

/// The cursor held by the typed values, or `None` when nothing holds it.
pub fn constrain(app: &mut CadApp, p: Vec2) -> Option<Vec2> {
    let dims = dims(app);
    let vals = values(app, &dims);
    (!vals.is_empty()).then(|| resolve(&dims, &vals, p)).filter(|q| q.is_finite())
}

/// Tab: lock the active field's typed value and move to the next field. `false` when no grip
/// fields are on show (Tab does its usual thing).
pub fn tab(app: &mut CadApp) -> bool {
    let dims = dims(app);
    if dims.is_empty() {
        return false;
    }
    let st = &mut app.canvas.grip_dyn;
    let typed = app.cmd.buffer.trim().to_owned();
    if dims.get(st.active).and_then(|d| d.parse(&app.session, &typed)).is_some() {
        st.locked.retain(|(i, _)| *i != st.active);
        st.locked.push((st.active, typed));
        app.cmd.buffer.clear();
    }
    st.active = (st.active + 1) % dims.len();
    true
}

/// Enter: place the grip where the typed values put it. `false` when nothing was typed or the
/// text isn't a value (Enter or the text then go to the command as usual).
pub fn submit(app: &mut CadApp) -> bool {
    let dims = dims(app);
    if dims.is_empty() {
        return false;
    }
    let typed = app.cmd.buffer.trim();
    if !typed.is_empty() && dims.get(app.canvas.grip_dyn.active).and_then(|d| d.parse(&app.session, typed)).is_none() {
        return false;
    }
    // A distance prompt takes the value as typed (or as Tab locked it), so the command reads the
    // number itself: a negative arc radius, a radius of zero refused.
    if distance_base(app).is_some() {
        if typed.is_empty()
            && let Some((_, l)) = app.canvas.grip_dyn.locked.first()
        {
            app.cmd.buffer = l.clone();
        }
        app.canvas.grip_dyn = GripDyn::default();
        return false;
    }
    let vals = values(app, &dims);
    if vals.is_empty() {
        return false;
    }
    let cursor = app.canvas.cursor.unwrap_or(app.session.cursor);
    let p = resolve(&dims, &vals, cursor);
    app.cmd.buffer.clear();
    app.canvas.grip_dyn = GripDyn::default();
    if let Err(e) = app.session.input(Input::Point(p)) {
        app.session.echo(e.to_string());
    }
    true
}

fn format(kind: GripDimKind, v: f64, (lu, lp): (i64, i64)) -> String {
    if kind.is_angle() { format!("{}°", (v.to_degrees() * 10.0).round() / 10.0) } else { cadcraft_engine::units::format_distance(v, lu, lp) }
}

/// Draw the fields for the stretched grip at the cursor; `false` when there are none (the
/// pointer boxes show instead).
pub fn draw(app: &mut CadApp, p: &egui::Painter) -> bool {
    let dims = dims(app);
    let (Some(xf), Some(c)) = (app.canvas.xf, app.canvas.cursor) else { return false };
    if dims.is_empty() {
        return false;
    }
    let t = Tokens::get();
    let font = crate::theme::small();
    let line = Stroke::new(1.0, t.text_dim);
    let st = app.canvas.grip_dyn.clone();
    let cs = xf.to_screen(c);
    let units = crate::canvas::length_units(&app.session);
    let mut offset_rank = 0.0;
    for (i, d) in dims.iter().enumerate() {
        let a = xf.to_screen(d.anchor);
        let active = i == st.active;
        let locked = st.locked.iter().find(|(j, _)| *j == i).map(|(_, s)| s.as_str());
        let text = if active && !app.cmd.buffer.is_empty() {
            app.cmd.buffer.clone()
        } else if let Some(l) = locked {
            l.to_owned()
        } else {
            format(d.kind, d.value(c), units)
        };
        // Where the field sits: beside the measured length, or on the angle's arc.
        let at = if d.kind.is_angle() {
            // The change in angle on a wider arc than the angle itself.
            let wide = if d.kind == GripDimKind::AngleChange { 2.2 } else { 1.0 };
            let r = a.distance(cs).clamp(24.0, 70.0) * 0.6 * wide;
            let a0 = -(if d.kind == GripDimKind::AngleChange { d.original } else { 0.0 }) as f32;
            let a1 = -(d.anchor.angle_to(c)) as f32;
            let mut sweep = a1 - a0;
            while sweep > std::f32::consts::PI {
                sweep -= std::f32::consts::TAU;
            }
            while sweep < -std::f32::consts::PI {
                sweep += std::f32::consts::TAU;
            }
            let pts: Vec<Pos2> = (0..=24).map(|k| a + egui::Vec2::angled(a0 + sweep * k as f32 / 24.0) * r).collect();
            p.add(Shape::line(pts, line));
            a + egui::Vec2::angled(a0 + sweep / 2.0) * (r + 24.0)
        } else {
            offset_rank += 1.0;
            let along = cs - a;
            let n = if along.length() > 1e-3 { vec2(-along.y, along.x) / along.length() } else { vec2(0.0, -1.0) };
            // Each further length sits farther out and farther along, clear of the one before.
            let off = n * (24.0 * offset_rank);
            p.line_segment([a + off, cs + off], line);
            p.line_segment([a + off * 0.2, a + off * 1.1], Stroke::new(1.0, t.text_faint));
            p.line_segment([cs + off * 0.2, cs + off * 1.1], Stroke::new(1.0, t.text_faint));
            a + off + along * (0.25 + 0.25 * offset_rank.min(2.0))
        };
        let live = locked.is_none() && !(active && !app.cmd.buffer.is_empty());
        field(p, at, &text, &font, active, locked.is_some(), live);
    }
    true
}

/// One value field centred on `at`.
fn field(p: &egui::Painter, at: Pos2, text: &str, font: &egui::FontId, active: bool, locked: bool, live: bool) {
    let t = Tokens::get();
    let g = p.layout_no_wrap(text.to_owned(), font.clone(), if live { t.text_faint } else { t.text });
    let lock_w = if locked { 11.0 } else { 0.0 };
    let r = Rect::from_center_size(at, vec2((g.size().x + 10.0 + lock_w).max(40.0), g.size().y + 6.0));
    p.rect_filled(r, 2.0, t.chrome_dark);
    p.rect_stroke(r, 2.0, if active { Stroke::new(1.5, t.accent) } else { Stroke::new(1.0, t.text_faint) }, egui::StrokeKind::Inside);
    let gw = g.size().x;
    p.galley(pos2(r.left() + 5.0, r.center().y - g.size().y / 2.0), g, t.text);
    if active {
        let x = r.left() + 6.0 + gw;
        p.line_segment([pos2(x, r.top() + 3.0), pos2(x, r.bottom() - 3.0)], Stroke::new(1.0, t.text));
    }
    if locked {
        padlock(p, pos2(r.right() - 7.0, r.center().y), t.accent);
    }
}

/// A small padlock, drawn in code.
fn padlock(p: &egui::Painter, c: Pos2, color: Color32) {
    p.rect_filled(Rect::from_center_size(c + vec2(0.0, 1.5), vec2(7.0, 5.0)), 1.0, color);
    p.add(Shape::line(
        vec![pos2(c.x - 2.0, c.y - 1.0), pos2(c.x - 2.0, c.y - 4.0), pos2(c.x + 2.0, c.y - 4.0), pos2(c.x + 2.0, c.y - 1.0)],
        Stroke::new(1.2, color),
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadcraft_doc::EntityKind;
    use egui::Event;

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

    fn line_end(app: &CadApp) -> Vec2 {
        app.session
            .doc()
            .ok()
            .and_then(|d| {
                d.model.iter().find_map(|e| match &e.kind {
                    EntityKind::Line(l) => Some(l.b.xy()),
                    _ => None,
                })
            })
            .unwrap_or(Vec2::new(f64::NAN, f64::NAN))
    }

    #[test]
    fn typed_length_tab_and_enter_place_the_grip_and_keywords_pass_through() {
        let mut app = CadApp::new(cadcraft_engine::Session::new(), crate::Services::default());
        let r = app.session.execute("line", &serde_json::json!({"points": [[0, 0], [10, 0]]})).unwrap();
        let h = Handle::parse_hex(r["handles"][0].as_str().unwrap()).unwrap();
        let ctx = egui::Context::default();
        run_frame(&mut app, &ctx, Vec::new());
        let at = app.canvas.rect.map_or(pos2(700.0, 450.0), |r| r.center() + vec2(40.0, -30.0));
        run_frame(&mut app, &ctx, vec![Event::PointerMoved(at)]);
        app.session.start_grip_edit(&[(h, 2)]).unwrap();
        run_frame(&mut app, &ctx, vec![Event::PointerMoved(at)]);
        assert_eq!(dims(&mut app).len(), 2, "length and length change");

        // `20` Tab: the length locks and the next field is active; the grip sits on the circle.
        run_frame(&mut app, &ctx, vec![Event::Text("20".into())]);
        // (Tab reaches the command line as text: `cmdline::capture_tab`.)
        run_frame(&mut app, &ctx, vec![Event::Text("\t".into())]);
        assert_eq!(app.canvas.grip_dyn.locked, [(0, "20".to_owned())]);
        assert_eq!(app.canvas.grip_dyn.active, 1);
        assert!(app.cmd.buffer.is_empty());
        run_frame(&mut app, &ctx, vec![Event::PointerMoved(at + vec2(1.0, 0.0))]);
        let c = app.canvas.cursor.unwrap();
        assert!((c.len() - 20.0).abs() < 1e-9, "{c:?}");
        // Enter places it there.
        run_frame(&mut app, &ctx, key(egui::Key::Enter));
        assert!(app.session.running.is_none());
        let b = line_end(&app);
        assert!((b.len() - 20.0).abs() < 1e-9 && b.dist(c) < 1e-9, "{b:?} vs {c:?}");

        // A keyword goes to the command as typed: `C` is Copy, not a value.
        app.session.start_grip_edit(&[(h, 2)]).unwrap();
        run_frame(&mut app, &ctx, vec![Event::Text("c".into())]);
        run_frame(&mut app, &ctx, key(egui::Key::Enter));
        assert!(app.session.running.is_some());
        app.session.input(Input::Point(Vec2::new(10.0, 10.0))).unwrap();
        let lines = app.session.doc().unwrap().model.iter().filter(|e| matches!(e.kind, EntityKind::Line(_))).count();
        assert_eq!(lines, 2, "Copy mode stretched a copy");
    }

    /// CIRCLE's radius prompt shows one distance field from the centre, not the pointer boxes
    /// (X/Y, or distance and angle), and a typed value is the radius (#1135).
    #[test]
    fn circle_radius_shows_one_distance_field_and_typed_value_is_the_radius() {
        for cartesian in [true, false] {
            let mut app = CadApp::new(cadcraft_engine::Session::new(), crate::Services::default());
            app.session.settings.dynpi_cartesian = cartesian;
            // The UCS icon paints "X" and "Y" too.
            app.ui.show_ucs_icon = false;
            let ctx = egui::Context::default();
            crate::canvas::tests::painted_text(&mut app, &ctx, pos2(700.0, 450.0));
            let at = app.canvas.xf.unwrap().to_screen(Vec2::ZERO) + vec2(130.0, -70.0);
            let labels = |text: &str| text.lines().filter(|l| matches!(*l, "X" | "Y" | "<")).map(str::to_owned).collect::<Vec<_>>();

            app.session.cmdline("circle").unwrap();
            crate::canvas::tests::painted_text(&mut app, &ctx, at);
            let text = crate::canvas::tests::painted_text(&mut app, &ctx, at);
            assert_eq!(labels(&text), ["X", "Y"], "the centre is a point:\n{text}");

            app.session.input(Input::Point(Vec2::ZERO)).unwrap();
            crate::canvas::tests::painted_text(&mut app, &ctx, at);
            let text = crate::canvas::tests::painted_text(&mut app, &ctx, at);
            assert!(labels(&text).is_empty(), "pointer boxes at the radius prompt (DYNPIFORMAT {cartesian}):\n{text}");
            assert_eq!(dims(&mut app).len(), 1, "one distance field");
            let r = cadcraft_engine::units::format_distance(app.canvas.cursor.unwrap().len(), 2, 4);
            assert!(text.contains(&r), "no radius {r} in:\n{text}");
            assert!(text.contains("Specify radius of circle"), "{text}");

            run_frame(&mut app, &ctx, vec![Event::Text("25".into())]);
            run_frame(&mut app, &ctx, key(egui::Key::Enter));
            assert!(app.session.running.is_none());
            let radius = app.session.doc().unwrap().model.iter().find_map(|e| match &e.kind {
                EntityKind::Circle(ci) => Some(ci.radius),
                _ => None,
            });
            assert_eq!(radius, Some(25.0));
        }
    }

    /// The grip's length fields read the drawing's length units (LUNITS/LUPREC) (#841).
    #[test]
    fn length_fields_follow_the_drawing_units() {
        let mut app = CadApp::new(cadcraft_engine::Session::new(), crate::Services::default());
        app.session.execute("units", &serde_json::json!({"lunits": 4, "luprec": 4})).unwrap();
        let r = app.session.execute("line", &serde_json::json!({"points": [[0, 0], [10, 0]]})).unwrap();
        let h = Handle::parse_hex(r["handles"][0].as_str().unwrap()).unwrap();
        let ctx = egui::Context::default();
        crate::canvas::tests::painted_text(&mut app, &ctx, pos2(700.0, 450.0));
        let at = app.canvas.xf.unwrap().to_screen(Vec2::ZERO) + vec2(130.0, -70.0);
        app.session.start_grip_edit(&[(h, 2)]).unwrap();
        crate::canvas::tests::painted_text(&mut app, &ctx, at);
        let text = crate::canvas::tests::painted_text(&mut app, &ctx, at);
        let c = app.canvas.cursor.unwrap();
        let arch = |v: f64| cadcraft_engine::units::format_distance(v, 4, 4);
        assert!(arch(c.len()) != arch(c.x) && arch(c.len()) != arch(c.y), "{c:?}");
        assert!(text.contains(&arch(c.len())), "no {} in:\n{text}", arch(c.len()));
    }
}
