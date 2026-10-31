//! The 3D model-space view: 3DORBIT, VIEW3D (angles or a preset) and PLAN (back to the flat 2D
//! view). The camera is session state ([`Session::view3d`]), not part of the drawing, so none of
//! these commands is undoable; the canvas, PNG export and the CLI/MCP renderers read it.
//!
//! Angles are degrees in JSON and radians in [`View3d`]. Azimuth turns around Z (0 looks from +X,
//! -90 from -Y, the front); elevation is above the XY plane (90 looks straight down).

use cadcraft_render::depth3d::Scene;
use cadcraft_render::view3d::{Camera3d, Projection3d, Render3dOptions, VisualStyle3d, build_3d_with_options};
use cadcraft_render::{Options, raster};
use serde_json::{Value, json};

use super::{CommandSpec, always, bad};
use crate::{Result, Session, View3d};

/// Elevation of a true isometric view: atan(1 / sqrt 2), in degrees.
const ISO_ELEVATION: f64 = 35.264_389_682_754_654;

/// The standard views as (name, azimuth, elevation) in degrees.
const PRESETS: &[(&str, f64, f64)] = &[
    ("swiso", -135.0, ISO_ELEVATION),
    ("seiso", -45.0, ISO_ELEVATION),
    ("neiso", 45.0, ISO_ELEVATION),
    ("nwiso", 135.0, ISO_ELEVATION),
    ("top", -90.0, 90.0),
    ("bottom", -90.0, -90.0),
    ("front", -90.0, 0.0),
    ("back", 90.0, 0.0),
    ("right", 0.0, 0.0),
    ("left", 180.0, 0.0),
];

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("3dorbit", "Orbit", run_orbit)
            .menu(&["View", "Orbit", "Constrained Orbit"])
            .alias(&["3do", "orbit"])
            .params("{azimuth?: degrees, elevation?: degrees (-90..90), stop?: bool} → {azimuth, elevation, orbiting, azimuthRadians, elevationRadians}: shows model space in 3D (default azimuth -45, elevation 30) and lets a primary-button drag in the canvas turn the camera; `stop` ends the dragging and keeps the view; PLAN returns to the flat view")
            .enabled(always)
            .noundo(),
        CommandSpec::new("view3d", "3D Views", run_view3d)
            .interactive(|_| Ok(Box::new(PresetM)))
            .params("{azimuth?, elevation? (-90..90), orbit?} or {preset: swiso|seiso|neiso|nwiso|top|bottom|front|back|left|right}, with optional projection: orthographic|perspective, fov: 10..120 degrees, style: shaded|flat|realistic|wireframe, lightDirection: [x,y,z], lightIntensity: 0..4, ambient: 0..1, specular: 0..1, shininess: 1..256, materialColor: [r,g,b] or null. A settings-only call preserves the current camera.")
            .enabled(always)
            .noundo(),
        CommandSpec::new("plan", "Plan View", run_plan)
            .params("{} → {view3d: false}: leaves the 3D view and shows model space flat (plan view) again")
            .enabled(always)
            .noundo(),
    ]
}

fn report(v: View3d, preset: Option<&str>, render: Render3dOptions) -> Value {
    let mut r = json!({
        "azimuth": v.azimuth.to_degrees(),
        "elevation": v.elevation.to_degrees(),
        "azimuthRadians": v.azimuth,
        "elevationRadians": v.elevation,
        "orbiting": v.orbiting,
        "projection": match render.projection { Projection3d::Orthographic => "orthographic", Projection3d::Perspective => "perspective" },
        "fov": render.field_of_view,
        "style": match render.style { VisualStyle3d::Shaded => "shaded", VisualStyle3d::Flat => "flat", VisualStyle3d::Realistic => "realistic", VisualStyle3d::Wireframe => "wireframe" },
        "lightDirection": [render.light_direction.x, render.light_direction.y, render.light_direction.z],
        "lightIntensity": render.light_intensity,
        "ambient": render.ambient,
        "specular": render.specular,
        "shininess": render.shininess,
        "materialColor": render.material_color.map(|c| [c.0, c.1, c.2]),
        "message": format!("3D view: azimuth {:.1}, elevation {:.1} degrees", v.azimuth.to_degrees(), v.elevation.to_degrees()),
    });
    if let (Some(name), Some(o)) = (preset, r.as_object_mut()) {
        o.insert("preset".into(), json!(name));
    }
    r
}

fn render_options(s: &Session, p: &Value) -> Result<Render3dOptions> {
    let mut o = s.render3d;
    if let Some(v) = p.get("projection").filter(|v| !v.is_null()) {
        o.projection = match v.as_str().map(str::trim).map(str::to_ascii_lowercase).as_deref() {
            Some("orthographic") | Some("ortho") => Projection3d::Orthographic,
            Some("perspective") | Some("persp") => Projection3d::Perspective,
            _ => return Err(bad("view3d", "`projection` must be `orthographic` or `perspective`")),
        };
    }
    if let Some(v) = p.get("style").filter(|v| !v.is_null()) {
        o.style = match v.as_str().map(str::trim).map(str::to_ascii_lowercase).as_deref() {
            Some("shaded") => VisualStyle3d::Shaded,
            Some("flat") => VisualStyle3d::Flat,
            Some("realistic") => VisualStyle3d::Realistic,
            Some("wireframe") | Some("wire") => VisualStyle3d::Wireframe,
            _ => return Err(bad("view3d", "`style` must be `shaded`, `flat`, `realistic` or `wireframe`")),
        };
    }
    let number = |key: &str| -> Result<Option<f64>> {
        match p.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(v) => v.as_f64().filter(|n| n.is_finite()).map(Some).ok_or_else(|| bad("view3d", format!("`{key}` must be a finite number"))),
        }
    };
    if let Some(n) = number("fov")? {
        o.field_of_view = n;
    }
    if let Some(n) = number("lightIntensity")? {
        o.light_intensity = n;
    }
    if let Some(n) = number("ambient")? {
        o.ambient = n;
    }
    if let Some(n) = number("specular")? {
        o.specular = n;
    }
    if let Some(n) = number("shininess")? {
        o.shininess = n;
    }
    if let Some(v) = p.get("lightDirection").filter(|v| !v.is_null()) {
        let a = v.as_array().filter(|a| a.len() == 3).ok_or_else(|| bad("view3d", "`lightDirection` must be [x,y,z]"))?;
        let mut parts = [0.0; 3];
        for (i, slot) in parts.iter_mut().enumerate() {
            *slot = a
                .get(i)
                .and_then(Value::as_f64)
                .filter(|n| n.is_finite())
                .ok_or_else(|| bad("view3d", "`lightDirection` needs three finite numbers"))?;
        }
        o.light_direction = cadcraft_geom::Vec3::new(parts[0], parts[1], parts[2]);
    }
    if let Some(v) = p.get("materialColor") {
        o.material_color = if v.is_null() {
            None
        } else {
            let a = v.as_array().filter(|a| a.len() == 3).ok_or_else(|| bad("view3d", "`materialColor` must be [r,g,b] or null"))?;
            let mut parts = [0u8; 3];
            for (i, slot) in parts.iter_mut().enumerate() {
                *slot = a
                    .get(i)
                    .and_then(Value::as_u64)
                    .and_then(|n| u8::try_from(n).ok())
                    .ok_or_else(|| bad("view3d", "`materialColor` channels must be whole numbers from 0 to 255"))?;
            }
            Some(cadcraft_color::Rgb(parts[0], parts[1], parts[2]))
        };
    }
    if !o.is_valid() {
        return Err(bad("view3d", "render settings are outside their supported ranges"));
    }
    Ok(o)
}

/// An optional angle in degrees: `None` when absent or null, else a finite number within
/// `limit` (the elevation limit is 90; the azimuth is any finite angle).
fn angle(cmd: &str, p: &Value, key: &str, limit: Option<f64>) -> Result<Option<f64>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => match v.as_f64().filter(|x| x.is_finite()) {
            Some(x) if limit.is_none_or(|l| x.abs() <= l) => Ok(Some(x)),
            Some(_) => Err(bad(cmd, format!("`{key}` must be between -90 and 90 degrees"))),
            None => Err(bad(cmd, format!("`{key}` must be a finite number of degrees"))),
        },
    }
}

/// Wrap an azimuth in degrees to (-180, 180] so repeated turns stay small.
fn wrap_degrees(a: f64) -> f64 {
    let w = a.rem_euclid(360.0);
    if w > 180.0 { w - 360.0 } else { w }
}

fn run_orbit(s: &mut Session, p: &Value) -> Result<Value> {
    let azimuth = angle("3dorbit", p, "azimuth", None)?;
    let elevation = angle("3dorbit", p, "elevation", Some(90.0))?;
    let stop = p.get("stop").and_then(Value::as_bool).unwrap_or(false);
    let render = render_options(s, p)?;
    let mut v = s.view3d.unwrap_or_default();
    if let Some(a) = azimuth {
        v.azimuth = wrap_degrees(a).to_radians();
    }
    if let Some(e) = elevation {
        v.elevation = e.to_radians();
    }
    v.orbiting = !stop;
    s.view3d = Some(v);
    s.render3d = render;
    Ok(report(v, None, render))
}

fn run_view3d(s: &mut Session, p: &Value) -> Result<Value> {
    let render = render_options(s, p)?;
    let orbit = match p.get("orbit") {
        None | Some(Value::Null) => None,
        Some(v) => Some(v.as_bool().ok_or_else(|| bad("view3d", "`orbit` must be true or false"))?),
    };
    let (azimuth, elevation, preset) = match p.get("preset") {
        None | Some(Value::Null) => {
            let current = s.view3d.unwrap_or_default();
            let a = angle("view3d", p, "azimuth", None)?;
            let e = angle("view3d", p, "elevation", Some(90.0))?;
            if a.is_none() && e.is_none() && !has_render_settings(p) {
                return Err(bad("view3d", "give camera angles, a `preset`, or a render setting"));
            }
            (a.map_or(current.azimuth, |a| wrap_degrees(a).to_radians()), e.map_or(current.elevation, f64::to_radians), None)
        }
        Some(name) => {
            let name = name.as_str().ok_or_else(|| bad("view3d", "`preset` must be a name"))?.trim().to_ascii_lowercase();
            let list = PRESETS.iter().map(|(n, ..)| *n).collect::<Vec<_>>().join(", ");
            let (n, a, e) =
                PRESETS.iter().find(|(n, ..)| *n == name).ok_or_else(|| bad("view3d", format!("unknown preset `{name}`; use one of {list}")))?;
            (a.to_radians(), e.to_radians(), Some(*n))
        }
    };
    let orbiting = orbit.unwrap_or_else(|| s.view3d.is_some_and(|v| v.orbiting) && preset.is_none());
    let v = View3d { azimuth, elevation, orbiting };
    s.view3d = Some(v);
    s.render3d = render;
    Ok(report(v, preset, render))
}

fn has_render_settings(p: &Value) -> bool {
    ["projection", "fov", "style", "lightDirection", "lightIntensity", "ambient", "specular", "shininess", "materialColor"]
        .iter()
        .any(|key| p.get(*key).is_some())
}

fn run_plan(s: &mut Session, _: &Value) -> Result<Value> {
    s.view3d = None;
    Ok(json!({ "view3d": false, "message": "Plan view" }))
}

/// Longest side of a 3D picture: the depth buffer ([`Scene::render`]) holds this many pixels at most.
pub const MAX_3D_SIDE: u32 = 2048;

/// A rendered 3D picture and its size in pixels (which can be smaller than asked for).
pub struct Render3d {
    pub png: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// The size a `width` x `height` request is drawn at: itself, or scaled down to keep the aspect
/// when a side passes [`MAX_3D_SIDE`]. Zero sizes are refused.
pub fn fit_size(width: u32, height: u32) -> Result<(u32, u32)> {
    if width == 0 || height == 0 {
        return Err(bad("render", "the image needs a width and height of at least 1 pixel"));
    }
    let longest = width.max(height);
    if longest <= MAX_3D_SIDE {
        return Ok((width, height));
    }
    let k = f64::from(MAX_3D_SIDE) / f64::from(longest);
    let scaled = |n: u32| ((f64::from(n) * k).round() as u32).clamp(1, MAX_3D_SIDE);
    Ok((scaled(width), scaled(height)))
}

/// Model space of `d` seen through `v`, drawn with a per-pixel depth test (faces hide what is
/// behind them) over the usual dark background and fitted to the projected drawing with a 5%
/// margin. Sizes over [`MAX_3D_SIDE`] are scaled down, keeping the aspect; the result says the
/// size it has. Only the PNG is raster: the SVG of a 3D view stays the vector painter's output.
pub fn render_png_of(d: &cadcraft_doc::Drawing, v: View3d, width: u32, height: u32) -> Result<Render3d> {
    render_png_of_with_options(d, v, Render3dOptions::default(), width, height)
}

pub fn render_png_of_with_options(d: &cadcraft_doc::Drawing, v: View3d, options: Render3dOptions, width: u32, height: u32) -> Result<Render3d> {
    let (w, h) = fit_size(width, height)?;
    let camera = Camera3d { azimuth: v.azimuth, elevation: v.elevation };
    if !options.is_valid() {
        return Err(bad("render", "invalid 3D render settings"));
    }
    if !camera.azimuth.is_finite() || !camera.elevation.is_finite() {
        return Err(bad("render", "invalid 3D camera angles"));
    }
    let bounds = build_3d_with_options(d, &cadcraft_doc::Space::Model, &Options::default(), camera, options).bounds;
    let view = raster::View::fit(&bounds, w, h, 0.05);
    let image = Scene::new_with_options(d, camera, options).render(&view).ok_or_else(|| bad("render", "the 3D picture could not be drawn"))?;
    // The background comes from drawing nothing; the depth picture goes over it where it has pixels.
    let mut pixmap = raster::render(&cadcraft_render::DisplayList::default(), &view, &raster::RasterOptions::default())
        .ok_or_else(|| bad("render", "the 3D picture could not be drawn"))?;
    for (dst, src) in pixmap.data_mut().chunks_exact_mut(4).zip(image.rgba.chunks_exact(4)) {
        if src.get(3).is_some_and(|a| *a == 255) {
            dst.copy_from_slice(src);
        }
    }
    let png = pixmap.encode_png().map_err(|e| bad("render", format!("PNG: {e}")))?;
    Ok(Render3d { png, width: w, height: h })
}

/// A PNG of model space seen through the session's 3D camera (see [`render_png_of`]); `None` when
/// the session shows the plan view (the caller renders it flat).
pub(super) fn render_png(s: &Session, width: u32, height: u32) -> Result<Option<Vec<u8>>> {
    let Some(v) = s.view3d else { return Ok(None) };
    Ok(Some(render_png_of_with_options(s.doc()?, v, s.render3d, width, height)?.png))
}

/// VIEW3D typed or picked: asks for a standard view by keyword (Enter takes SE isometric).
struct PresetM;

const KW: crate::Accept = crate::Accept { point: false, number: false, text: false, select: false, enter: true };

impl crate::Interactive for PresetM {
    fn name(&self) -> &'static str {
        "VIEW3D"
    }
    fn prompt(&self, _s: &Session) -> crate::Prompt {
        crate::Prompt::new("Enter a standard view", KW)
            .kw(&["Top", "Bottom", "Front", "Back", "Left", "Right", "SWiso", "SEiso", "NEiso", "NWiso"])
            .default("SEiso")
    }
    fn input(&mut self, s: &mut Session, i: crate::Input) -> Result<crate::Step> {
        let name = match i {
            crate::Input::Keyword(k) | crate::Input::Text(k) => k,
            crate::Input::Enter => "seiso".into(),
            crate::Input::Cancel => return Ok(crate::Step::Cancel),
            _ => return Ok(crate::Step::Continue),
        };
        let r = run_view3d(s, &json!({ "preset": name.trim() }))?;
        if let Some(m) = r.get("message").and_then(Value::as_str) {
            s.echo(m.to_string());
        }
        Ok(crate::Step::Done)
    }
}

#[cfg(test)]
mod tests {
    use std::f64::consts::{FRAC_PI_4, FRAC_PI_6};

    use super::*;
    use cadcraft_render::view3d::build_3d;
    use serde_json::json;

    fn run(s: &mut Session, id: &str, p: Value) -> Result<Value> {
        s.execute(id, &p)
    }

    #[test]
    fn orbit_sets_the_default_view_and_keeps_angles_afterwards() {
        let mut s = Session::new();
        assert_eq!(s.view3d, None);
        run(&mut s, "3dorbit", json!({})).unwrap();
        assert_eq!(s.view3d, Some(View3d { azimuth: -FRAC_PI_4, elevation: FRAC_PI_6, orbiting: true }));
        run(&mut s, "view3d", json!({ "azimuth": 20, "elevation": 10, "orbit": false })).unwrap();
        run(&mut s, "3dorbit", Value::Null).unwrap();
        let v = s.view3d.unwrap();
        assert!(v.orbiting && (v.azimuth - 20f64.to_radians()).abs() < 1e-12 && (v.elevation - 10f64.to_radians()).abs() < 1e-12);
        let r = run(&mut s, "3dorbit", json!({ "stop": true })).unwrap();
        assert_eq!(r["orbiting"], false);
        assert!(s.view3d.is_some());
        assert!(run(&mut s, "3dorbit", json!({ "elevation": 120 })).is_err());
        assert!(run(&mut s, "3dorbit", json!({ "azimuth": "x" })).is_err());
    }

    #[test]
    fn render_settings_are_command_driven_and_invalid_updates_are_atomic() {
        let mut s = Session::new();
        run(&mut s, "view3d", json!({ "preset": "neiso" })).unwrap();
        let camera = s.view3d;
        let result = run(
            &mut s,
            "view3d",
            json!({
                "projection": "perspective", "fov": 72, "style": "realistic",
                "lightDirection": [1, -2, 3], "lightIntensity": 1.4,
                "ambient": 0.2, "specular": 0.7, "shininess": 96,
                "materialColor": [40, 120, 220]
            }),
        )
        .unwrap();
        assert_eq!(s.view3d, camera, "a settings-only call preserves the camera");
        assert_eq!(result["projection"], "perspective");
        assert_eq!(result["style"], "realistic");
        assert_eq!(result["materialColor"], json!([40, 120, 220]));
        let saved = s.render3d;
        assert!(run(&mut s, "view3d", json!({ "style": "realistic", "fov": 900 })).is_err());
        assert!(run(&mut s, "view3d", json!({ "lightDirection": [1.7e308, 1.7e308, 1.7e308] })).is_err());
        assert_eq!(s.render3d, saved, "invalid values do not partially update session options");
        assert_eq!(s.view3d, camera);
    }

    #[test]
    fn typed_at_the_command_line_and_started_like_the_palette() {
        let mut s = Session::new();
        s.start("3dorbit").unwrap();
        assert!(s.view3d.is_some_and(|v| v.orbiting));
        s.start("plan").unwrap();
        assert_eq!(s.view3d, None);
        s.cmdline("3do").unwrap();
        assert!(s.view3d.is_some());
    }

    #[test]
    fn view3d_sets_angles_and_presets_and_refuses_bad_input() {
        let mut s = Session::new();
        let r = run(&mut s, "view3d", json!({ "azimuth": 30, "elevation": 45 })).unwrap();
        assert!((r["azimuth"].as_f64().unwrap() - 30.0).abs() < 1e-9 && (r["elevation"].as_f64().unwrap() - 45.0).abs() < 1e-9);
        assert_eq!(r["orbiting"], false);
        let v = s.view3d.unwrap();
        assert!((v.azimuth - 30f64.to_radians()).abs() < 1e-12);
        let r = run(&mut s, "view3d", json!({ "preset": "TOP" })).unwrap();
        assert_eq!(r["preset"], "top");
        assert!((s.view3d.unwrap().elevation - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
        let r = run(&mut s, "view3d", json!({ "preset": "swiso" })).unwrap();
        assert!((r["azimuth"].as_f64().unwrap() + 135.0).abs() < 1e-9);
        for bad in [
            json!({}),
            json!({ "azimuth": 10, "elevation": 91 }),
            json!({ "azimuth": 10, "elevation": -90.5 }),
            json!({ "azimuth": 10, "elevation": "high" }),
            json!({ "azimuth": null, "elevation": null }),
            json!({ "azimuth": "ten", "elevation": 5 }),
            json!({ "preset": "sideways" }),
            json!({ "preset": 3 }),
            json!({ "azimuth": 1, "elevation": 1, "orbit": "yes" }),
        ] {
            assert!(run(&mut s, "view3d", bad.clone()).is_err(), "{bad}");
        }
        // A refused call leaves the last good view alone.
        assert!((s.view3d.unwrap().azimuth + 135f64.to_radians()).abs() < 1e-12);
    }

    #[test]
    fn zoom_extents_fits_the_projection() {
        let mut s = Session::new();
        s.cmdline("line 0,0 100,0").unwrap();
        s.cmdline("").unwrap();
        // A wall 500 units tall: invisible to the plan extents, dominant from the front.
        let p = |x: f64, z: f64| cadcraft_geom::Vec3::new(x, 0.0, z);
        let kind =
            cadcraft_doc::EntityKind::Face3d(cadcraft_doc::Face3d { corners: [p(0., 0.), p(100., 0.), p(100., 500.), p(0., 500.)], hidden_edges: 0 });
        let space = s.space();
        s.doc_mut().unwrap().add(&space, cadcraft_doc::Common::default(), kind).unwrap();
        s.zoom_extents().unwrap();
        let plan_h = s.state().unwrap().view().height;
        assert!(plan_h < 500.0, "the plan extents ignore Z: {plan_h}");
        run(&mut s, "view3d", json!({ "preset": "front" })).unwrap();
        s.zoom_extents().unwrap();
        let st = s.state().unwrap();
        assert!(st.view().height >= 500.0, "front view fits the wall: {}", st.view().height);
        assert!((st.view().center.y - 250.0).abs() < 1.0, "centred on the projected wall: {:?}", st.view().center);
    }

    #[test]
    fn zoom_extents_fits_translated_perspective_geometry() {
        let mut s = Session::new();
        run(&mut s, "box", json!({"p1": [10,20,30], "p2": [14,23,32]})).unwrap();
        run(&mut s, "view3d", json!({"preset": "seiso", "projection": "perspective", "fov": 35})).unwrap();
        s.zoom_extents().unwrap();
        let st = s.state().unwrap();
        let bounds = build_3d_with_options(
            &st.doc,
            &st.space,
            &Options::default(),
            Camera3d { azimuth: s.view3d.unwrap().azimuth, elevation: s.view3d.unwrap().elevation },
            s.render3d,
        )
        .bounds;
        let view = st.view();
        assert!((view.center - bounds.center()).len() < 1e-10, "perspective bounds must determine the zoom center: {view:?}, {bounds:?}");
        assert!(view.height >= bounds.height());
        let (w, h) = s.viewport_px;
        assert!(view.height * st.zoom_aspect(w / h.max(1.0)) >= bounds.width());
    }

    #[test]
    fn the_camera_belongs_to_the_drawing() {
        let mut s = Session::new();
        run(&mut s, "view3d", json!({ "preset": "nwiso" })).unwrap();
        let first = s.view3d;
        assert!(first.is_some());
        s.execute("new", &json!({})).unwrap();
        assert_eq!(s.view3d, None, "a new drawing starts in plan view");
        run(&mut s, "view3d", json!({ "azimuth": 10, "elevation": 20 })).unwrap();
        let second = s.view3d;
        s.execute("document.switch", &json!({ "index": 0 })).unwrap();
        assert_eq!(s.view3d, first);
        s.execute("document.switch", &json!({ "index": 1 })).unwrap();
        assert_eq!(s.view3d, second);
        s.execute("close", &json!({ "index": 1 })).unwrap();
        assert_eq!(s.view3d, first, "closing the drawing brings back the other one's camera");
        s.execute("plan", &json!({})).unwrap();
        s.execute("closeall", &json!({})).unwrap();
        assert_eq!(s.view3d, None);
    }

    /// A unit-ish cube of 40 units made of six quad 3D faces.
    fn cube(s: &mut Session) {
        let p = |x: f64, y: f64, z: f64| cadcraft_geom::Vec3::new(x * 40.0, y * 40.0, z * 40.0);
        let quads = [
            [p(0., 0., 0.), p(1., 0., 0.), p(1., 1., 0.), p(0., 1., 0.)],
            [p(0., 0., 1.), p(1., 0., 1.), p(1., 1., 1.), p(0., 1., 1.)],
            [p(0., 0., 0.), p(1., 0., 0.), p(1., 0., 1.), p(0., 0., 1.)],
            [p(0., 1., 0.), p(1., 1., 0.), p(1., 1., 1.), p(0., 1., 1.)],
            [p(0., 0., 0.), p(0., 1., 0.), p(0., 1., 1.), p(0., 0., 1.)],
            [p(1., 0., 0.), p(1., 1., 0.), p(1., 1., 1.), p(1., 0., 1.)],
        ];
        let space = s.space();
        for corners in quads {
            let kind = cadcraft_doc::EntityKind::Face3d(cadcraft_doc::Face3d { corners, hidden_edges: 0 });
            s.doc_mut().unwrap().add(&space, cadcraft_doc::Common::default(), kind).unwrap();
        }
    }

    fn png_size(png: &[u8]) -> (u32, u32) {
        let n = |i: usize| u32::from_be_bytes([png[i], png[i + 1], png[i + 2], png[i + 3]]);
        (n(16), n(20))
    }

    #[test]
    fn the_3d_picture_has_depth_and_bounded_size() {
        let mut s = Session::new();
        cube(&mut s);
        let v = View3d { azimuth: -FRAC_PI_4, elevation: FRAC_PI_6, orbiting: false };
        let d = s.doc().unwrap();
        // The cube fills the middle of the picture; the corners stay background.
        let camera = Camera3d { azimuth: v.azimuth, elevation: v.elevation };
        let bounds = build_3d(d, &cadcraft_doc::Space::Model, &Options::default(), camera).bounds;
        let view = raster::View::fit(&bounds, 200, 200, 0.05);
        let img = Scene::new(d, camera).render(&view).unwrap();
        let at = |x: u32, y: u32| img.rgba.get(((y * 200 + x) * 4 + 3) as usize).copied();
        assert_eq!(at(100, 100), Some(255), "the cube covers the centre");
        assert_eq!(at(0, 0), Some(0), "the corner is empty");
        // The picture differs from the plan view's and has the asked-for size.
        let png = render_png_of(d, v, 300, 200).unwrap();
        assert_eq!(png_size(&png.png), (300, 200));
        let flat = render_png_of(d, View3d { elevation: std::f64::consts::FRAC_PI_2, ..v }, 300, 200).unwrap();
        assert_ne!(png.png, flat.png);
        let perspective = render_png_of_with_options(
            d,
            v,
            Render3dOptions { projection: Projection3d::Perspective, style: VisualStyle3d::Realistic, ..Default::default() },
            300,
            200,
        )
        .unwrap();
        assert_ne!(png.png, perspective.png, "projection and visual style affect exported pixels");
        // A huge request is scaled down with its aspect kept, never empty or refused silently.
        let big = render_png_of(d, v, 10_000, 5_000).unwrap();
        assert_eq!((big.width, big.height), (2048, 1024));
        assert_eq!(png_size(&big.png), (2048, 1024));
        let tall = render_png_of(d, v, 1, 100_000).unwrap();
        assert_eq!((tall.width, tall.height), (1, 2048));
        assert!(render_png_of(d, v, 0, 100).is_err());
        assert!(render_png_of(d, View3d { azimuth: f64::NAN, ..v }, 32, 32).is_err());
        assert_eq!(fit_size(2048, 2048).unwrap(), (2048, 2048));
        // Through EXPORT (an engine-side png) the camera is honoured without any io hooks.
        run(&mut s, "view3d", json!({ "preset": "seiso" })).unwrap();
        assert!(render_png(&s, 2400, 1600).unwrap().is_some());
    }

    #[test]
    fn view3d_typed_asks_for_a_standard_view() {
        let mut s = Session::new();
        s.cmdline("view3d").unwrap();
        s.cmdline("front").unwrap();
        let v = s.view3d.unwrap();
        assert!((v.azimuth + std::f64::consts::FRAC_PI_2).abs() < 1e-12 && v.elevation.abs() < 1e-12);
        s.cmdline("view3d").unwrap();
        s.cmdline("").unwrap();
        assert!((s.view3d.unwrap().azimuth + FRAC_PI_4).abs() < 1e-12, "Enter takes SE isometric");
        assert!(s.running.is_none());
    }

    #[test]
    fn plan_restores_the_flat_view() {
        let mut s = Session::new();
        run(&mut s, "3dorbit", json!({})).unwrap();
        let r = run(&mut s, "plan", json!({})).unwrap();
        assert_eq!(r["view3d"], false);
        assert_eq!(s.view3d, None);
        run(&mut s, "plan", json!({})).unwrap();
    }

    #[test]
    fn view_commands_are_not_undoable_and_png_follows_the_camera() {
        for id in ["3dorbit", "view3d", "plan"] {
            assert!(!crate::cmd::find_command(id).unwrap().undoable, "{id}");
        }
        let mut s = Session::new();
        s.cmdline("line 0,0 100,0").unwrap();
        s.cmdline("").unwrap();
        assert!(render_png(&s, 200, 100).unwrap().is_none());
        run(&mut s, "view3d", json!({ "preset": "swiso" })).unwrap();
        let png = render_png(&s, 200, 100).unwrap().unwrap();
        assert_eq!(&png[1..4], b"PNG");
    }
}
