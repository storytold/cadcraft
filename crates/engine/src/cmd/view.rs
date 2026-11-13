//! View menu: ZOOM (with Realtime and Dynamic), PAN, REGEN, REDRAW, named views (VIEW), the view
//! twist (DVIEW TWist), layout switching.

use cadcraft_doc::{Space, entity_bounds};
use cadcraft_geom::{Bounds2, Vec2};
use serde_json::{Value, json};

use super::machines::{SelectRun, number};
use super::{CommandSpec, bad, bool_or, f64_or, f64_req, ok, point_param, point_req, str_param, targets};

use crate::{Accept, Input, Interactive, Prompt, Result, Session, Step, View};

mod dview;
mod named_views;
mod vports;

pub(super) use vports::{config_names, config_tiles, model_vports, tile_model};

macro_rules! camera_presets_doc {
    () => {
        "top|bottom|front|back|left|right (aliases: plan|south|s|north|n|west|w|east|e) | \
         edges: front_top|front_bottom|front_right|front_left|back_top|back_bottom|back_right|back_left|right_top|right_bottom|left_top|left_bottom (aliases: top_front|bottom_front|top_back|bottom_back|top_right|bottom_right|top_left|bottom_left) | \
         corners: top_front_right|top_front_left|top_back_right|top_back_left|bottom_front_right|bottom_front_left|bottom_back_right|bottom_back_left (aliases: se_isometric|iso_se|seiso|home|iso|sw_isometric|iso_sw|swiso|ne_isometric|iso_ne|neiso|nw_isometric|iso_nw|nwiso)"
    };
}

pub fn specs() -> Vec<CommandSpec> {
    let mut v = vec![
        CommandSpec::new("zoom", "Zoom", run_zoom)
            .alias(&["z"])
            .params("{mode: extents|all|window|previous|in|out|center|dynamic|scale|realtime|object, p1?, p2?, center?, height?, factor?, xp?: bool (scale relative to paper space: in a viewport, `factor` paper units per model unit), limits?: bool (scale relative to the limits)} (center/dynamic: `center` + `height`, or `factor` with `xp`/`limits`; realtime: `factor` about the view centre; typed: Enter at the first prompt starts Realtime)")
            .noundo()
            .transparent()
            .interactive(|_| Ok(Box::new(ZoomM::default()))),
        CommandSpec::new("zoom.realtime", "Zoom Realtime", |s, p| zoom(s, &with_mode(p.clone(), "realtime", None)))
            .menu(&["View", "Zoom", "Realtime"])
            .params("{factor} (> 1 zooms in, about the view centre); interactive: drag up to zoom in, down to zoom out, Esc or Enter ends")
            .noundo()
            .transparent()
            .interactive(|_| Ok(Box::new(ZoomM { start: ZoomStart::Realtime, ..Default::default() }))),
        CommandSpec::new("zoom.dynamic", "Zoom Dynamic", |s, p| zoom(s, &with_mode(p.clone(), "dynamic", None)))
            .menu(&["View", "Zoom", "Dynamic"])
            .params("{center, height}; interactive: a view box over the drawing extents, a point places it, the next sizes it, Enter accepts")
            .noundo()
            .transparent()
            .interactive(|_| Ok(Box::new(ZoomM { start: ZoomStart::Dynamic, ..Default::default() }))),
        CommandSpec::new("zoom.previous", "Zoom Previous", |s, _| zoom(s, &json!({"mode":"previous"})))
            .menu(&["View", "Zoom", "Previous"])
            .noundo()
            .transparent(),
        CommandSpec::new("zoom.window", "Zoom Window", run_zoom)
            .menu(&["View", "Zoom", "Window"])
            .params("{p1, p2}")
            .noundo()
            .transparent()
            .interactive(|_| Ok(Box::new(ZoomM { window: true, ..Default::default() }))),
        CommandSpec::new("zoom.in", "Zoom In", |s, _| zoom(s, &json!({"mode":"in"}))).menu(&["View", "Zoom", "In"]).noundo().transparent(),
        CommandSpec::new("zoom.out", "Zoom Out", |s, _| zoom(s, &json!({"mode":"out"}))).menu(&["View", "Zoom", "Out"]).noundo().transparent(),
        CommandSpec::new("zoom.all", "Zoom All", |s, _| zoom(s, &json!({"mode":"all"}))).menu(&["View", "Zoom", "All"]).noundo().transparent(),
        CommandSpec::new("zoom.extents", "Zoom Extents", |s, _| zoom(s, &json!({"mode":"extents"})))
            .menu(&["View", "Zoom", "Extents"])
            .noundo()
            .transparent(),
        CommandSpec::new("zoom.object", "Zoom Object", |s, p| {
            zoom(s, &json!({"mode":"object", "handles": p.get("handles").cloned().unwrap_or(Value::Null)}))
        })
        .menu(&["View", "Zoom", "Object"])
        .params("{handles?}")
        .noundo()
        .transparent()
        .interactive(|_| Ok(Box::new(SelectRun::new("zoom.object", "ZOOM")))),
        CommandSpec::new("pan", "Pan", run_pan)
            .menu(&["View", "Pan", "Realtime"])
            .alias(&["p"])
            .params("{delta: [dx,dy]} | {from, to} | {center}")
            .noundo()
            .transparent()
            .interactive(|_| Ok(Box::new(PanM::default()))),
        CommandSpec::new("pan.left", "Pan Left", |s, _| pan_frac(s, Vec2::new(-0.25, 0.0))).menu(&["View", "Pan", "Left"]).noundo().transparent(),
        CommandSpec::new("pan.right", "Pan Right", |s, _| pan_frac(s, Vec2::new(0.25, 0.0))).menu(&["View", "Pan", "Right"]).noundo().transparent(),
        CommandSpec::new("pan.up", "Pan Up", |s, _| pan_frac(s, Vec2::new(0.0, 0.25))).menu(&["View", "Pan", "Up"]).noundo().transparent(),
        CommandSpec::new("pan.down", "Pan Down", |s, _| pan_frac(s, Vec2::new(0.0, -0.25))).menu(&["View", "Pan", "Down"]).noundo().transparent(),
        CommandSpec::new("regen", "Regen", |s, _| {
            // Table formulas are recalculated (as on UPDATEFIELD).
            super::table::recalc(s, None)?;
            s.touch();
            Ok(json!({"message": "Regenerating model."}))
        })
        .menu(&["View", "Regen"])
        .alias(&["re"])
        .noundo(),
        CommandSpec::new("regenall", "Regen All", |s, _| {
            s.touch();
            Ok(json!({"message": "Regenerating all viewports."}))
        })
        .menu(&["View", "Regen All"])
        .alias(&["rea"])
        .noundo(),
        CommandSpec::new("redraw", "Redraw", |s, _| {
            s.touch();
            ok()
        })
        .menu(&["View", "Redraw"])
        .alias(&["r"])
        .noundo()
        .transparent(),
        dview::spec(),
        CommandSpec::new("view.set", "Set View", run_view_set)
            .params(concat!(
                "{center?: [x,y], height?: f64 (center and height must be given together), twist?: degrees (requires center and height; default: keep the current twist; in a layout viewport it is the viewport's twist, an undoable edit; not in Paper space), preset?: string (",
                camera_presets_doc!(),
                "), neighbor?: string (top|bottom|left|right), dir?: [x,y,z] (non-zero), up?: [x,y,z] (non-zero, requires dir)} (requires center+height and/or one of preset|dir|neighbor, which are mutually exclusive)"
            ))
            .noundo(),
        CommandSpec::new("3dorbit", "3D Orbit", run_3dorbit)
            .menu(&["View", "3D Orbit"])
            .alias(&["orbit", "3do"])
            .params(concat!(
                "{daz?: f64 (deg, default 0), del?: f64 (deg, default 0), clamp?: bool (default true: snap within 2° of principal axes), preset?: string (",
                camera_presets_doc!(),
                ")}"
            ))
            .noundo()
            .transparent(),
        CommandSpec::new("view.roll", "Roll View", run_view_roll)
            .params("{angle?: f64 (deg, default 90; when inPlane is false, snaps world-Z compass heading to the next multiple of max(90, |angle|) in the sign direction of angle; when inPlane is true, rolls by the exact angle), inPlane?: bool (default false: world-Z cardinal step roll; true: continuous in-plane roll around the viewing axis)}")
            .noundo()
            .transparent(),
        CommandSpec::new("plan", "Plan View", |s, _| {
            s.state_mut()?.camera = cadcraft_geom::Camera3D::TOP;
            ok()
        })
        .menu(&["View", "3D Views", "Plan View"])
        .noundo()
        .transparent(),
        CommandSpec::new("view.get", "Get View", run_view_get)
            .params("{} → {center: [x,y], height: f64, twist: degrees, viewportPx: [w,h], viewName: string (Top|Bottom|Front|Back|Left|Right|SW Isometric|SE Isometric|NE Isometric|NW Isometric|3D), dir: [x,y,z] (camera forward vector), up: [x,y,z] (camera up vector)}")
            .noundo(),
        CommandSpec::new("layout.set", "Switch Layout", run_layout_set).params("{name: \"Model\" | layout name}").noundo(),
    ];
    v.extend(named_views::specs());
    v.extend(vports::specs());
    v
}

fn ext_bounds(s: &Session) -> Result<Bounds2> {
    let space = s.space();
    Ok(s.doc()?.extents(&space))
}

fn fit(s: &mut Session, b: Bounds2) -> Result<()> {
    if b.is_empty() {
        return Ok(());
    }
    let (w, h) = s.viewport_px;
    let st = s.state_mut()?;
    let aspect = st.zoom_aspect(w / h.max(1.0));
    // A twisted view fits the box the bounds make on screen.
    let twist = st.zoom_frame().twist;
    let (center, bw, bh) = View::twisted_box(b, twist);
    let height = bh.max(bw / aspect.max(1e-6)).max(1e-9);
    st.set_zoom_frame(View { center, height, twist });
    Ok(())
}

pub(crate) fn zoom(s: &mut Session, p: &Value) -> Result<Value> {
    let mode = str_param(p, "mode").unwrap_or("extents").to_ascii_lowercase();
    match mode.as_str() {
        "extents" | "e" => s.zoom_extents()?,
        "all" | "a" => {
            let b = ext_bounds(s)?.union(&s.zoom_limits()?);
            fit(s, b.expand(b.height().max(b.width()) * 0.02))?;
        }
        "window" | "w" => {
            let a = point_req("zoom", p, "p1")?;
            let b = point_req("zoom", p, "p2")?;
            fit(s, Bounds2::new(a, b))?;
        }
        "previous" | "p" => {
            let st = s.state_mut()?;
            if let Some(v) = st.view_history.pop() {
                st.set_view_quiet(v);
            } else {
                return Ok(json!({"message": "No previous view saved."}));
            }
        }
        "in" | "out" => {
            let f = if mode == "in" { 2.0 } else { 0.5 };
            let st = s.state_mut()?;
            let v = st.zoom_frame();
            st.set_zoom_frame(View { height: v.height / f, ..v });
        }
        "center" | "c" | "dynamic" | "d" => {
            let c = point_req("zoom", p, "center")?;
            let cur = s.state()?.zoom_frame();
            let h = if p.get("factor").is_some() { factor_height(s, p, cur.height)? } else { f64_or(p, "height", cur.height) };
            s.state_mut()?.set_zoom_frame(View { center: c, height: h.max(1e-9), ..cur });
        }
        "scale" | "s" => {
            let v = s.state()?.zoom_frame();
            let height = factor_height(s, p, v.height)?;
            s.state_mut()?.set_zoom_frame(View { height, ..v });
        }
        "realtime" => {
            let v = s.state()?.zoom_frame();
            let f = f64_req("zoom", p, "factor")?;
            if f <= 0.0 {
                return Err(bad("zoom", "factor must be positive"));
            }
            s.state_mut()?.set_zoom_frame(View { height: (v.height / f).clamp(1e-9, 1e12), ..v });
        }
        "object" | "o" => {
            let hs = targets(s, p)?;
            let d = s.doc()?;
            let b = hs.iter().filter_map(|h| d.entity(*h)).fold(Bounds2::EMPTY, |a, e| a.union(&entity_bounds(d, e, 0)));
            fit(s, b.expand(b.height().max(b.width()) * 0.05))?;
        }
        _ => return Err(bad("zoom", "unknown mode")),
    }
    // No `touch`: a zoom changes the view, not the drawing. The canvas follows the view by
    // itself and regenerates in the background (AutoCAD redraws instead of regenerating);
    // zooming an unlocked viewport changes the drawing, which shows on its own.
    Ok(Value::Null)
}

/// The view height for a zoom `factor`: relative to the current height `cur` (nX), to paper space
/// with `xp` (nXP; in a viewport `factor` paper units per model unit, elsewhere like nX), or to
/// the limits with `limits` (a plain n: 1 fits the limits).
fn factor_height(s: &Session, p: &Value, cur: f64) -> Result<f64> {
    let f = f64_req("zoom", p, "factor")?;
    if f <= 0.0 {
        return Err(bad("zoom", "factor must be positive"));
    }
    let st = s.state()?;
    if bool_or(p, "xp", false)
        && let Some(vp) = st.zoom_viewport()
    {
        return Ok(vp.height / f);
    }
    if bool_or(p, "limits", false) {
        let lim = s.zoom_limits()?;
        let (w, h) = s.viewport_px;
        let aspect = st.zoom_aspect(w / h.max(1.0)).max(1e-6);
        return Ok(lim.height().max(lim.width() / aspect).max(1e-9) / f);
    }
    Ok(cur / f)
}

/// A typed zoom factor: `n` (relative to the limits), `nX` (to the current view) or `nXP` (to
/// paper space), as `zoom` scale parameters.
fn typed_factor(t: &str) -> Option<Value> {
    let tl = t.trim().to_ascii_lowercase();
    let (num, rel) = if let Some(n) = tl.strip_suffix("xp") {
        (n, "xp")
    } else if let Some(n) = tl.strip_suffix('x') {
        (n, "x")
    } else {
        (tl.as_str(), "limits")
    };
    let f = number(num.trim()).filter(|f| *f > 0.0)?;
    Some(json!({ "factor": f, "xp": rel == "xp", "limits": rel == "limits" }))
}

fn with_mode(mut p: Value, mode: &str, center: Option<Vec2>) -> Value {
    if let Some(o) = p.as_object_mut() {
        o.insert("mode".into(), json!(mode));
        if let Some(c) = center {
            o.insert("center".into(), json!([c.x, c.y]));
        }
    }
    p
}

fn run_zoom(s: &mut Session, p: &Value) -> Result<Value> {
    zoom(s, if p.is_null() { &Value::Null } else { p })
}

fn pan_frac(s: &mut Session, f: Vec2) -> Result<Value> {
    let (w, h) = s.viewport_px;
    let st = s.state_mut()?;
    let v = st.view();
    let ww = v.height * w / h.max(1.0);
    // Screen directions: in a twisted view the drawing is turned by the twist.
    st.set_view(View { center: v.center + Vec2::new(f.x * ww, f.y * v.height).rotate(-v.twist), ..v });
    ok()
}

fn run_pan(s: &mut Session, p: &Value) -> Result<Value> {
    let st = s.state_mut()?;
    let v = st.view();
    let c = if let Some(c) = point_param(p, "center") {
        c
    } else if let Some(d) = point_param(p, "delta") {
        v.center - d
    } else {
        let a = point_req("pan", p, "from")?;
        let b = point_req("pan", p, "to")?;
        v.center - (b - a)
    };
    st.set_view(View { center: c, ..v });
    ok()
}

/// Whether JSON parameter `key` is present and not `null`.
fn has_param(p: &Value, key: &str) -> bool {
    p.get(key).is_some_and(|v| !v.is_null())
}

/// Parse an optional finite `f64` parameter, returning `default` when absent or `null` and an error for invalid/non-finite values.
fn f64_opt(cmd: &str, p: &Value, key: &str, default: f64) -> Result<f64> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(v) => v.as_f64().filter(|x| x.is_finite()).ok_or_else(|| bad(cmd, format!("`{key}` must be a finite number"))),
    }
}

/// Parse an optional non-zero 3D vector `[x, y, z]` from JSON parameter `key`.
fn vec3_nonzero(cmd: &str, p: &Value, key: &str) -> Result<Option<cadcraft_geom::Vec3>> {
    let Some(v) = p.get(key).filter(|v| !v.is_null()) else {
        return Ok(None);
    };
    let parsed = (|| {
        let a = v.as_array().filter(|a| a.len() == 3)?;
        let x = a.first()?.as_f64()?;
        let y = a.get(1)?.as_f64()?;
        let z = a.get(2)?.as_f64()?;
        let vec = cadcraft_geom::Vec3::new(x, y, z);
        (x.is_finite() && y.is_finite() && z.is_finite() && vec.normalized() != cadcraft_geom::Vec3::ZERO).then_some(vec)
    })();
    parsed.map(Some).ok_or_else(|| bad(cmd, format!("`{key}` must be a non-zero [x, y, z] vector")))
}

/// Execute `view.get`: return the 2D view state (`center`, `height`, `twist`, `viewportPx`) and
/// 3D camera classification/frame (`viewName`, `dir` = camera forward vector `Camera3D::z`,
/// `up` = camera up vector `Camera3D::y`).
fn run_view_get(s: &mut Session, _: &Value) -> Result<Value> {
    let st = s.state()?;
    let v = st.view();
    let cam = st.camera;
    Ok(json!({
        "center": [v.center.x, v.center.y],
        "height": v.height,
        "twist": v.twist.to_degrees(),
        "viewportPx": [s.viewport_px.0, s.viewport_px.1],
        "viewName": cam.view_name(),
        "dir": [cam.z.x, cam.z.y, cam.z.z],
        "up": [cam.y.x, cam.y.y, cam.y.z],
    }))
}

/// Execute `3dorbit` (`{daz?, del?, clamp?, preset?}`): orbit the 3D camera or jump to a
/// [`cadcraft_geom::Camera3D::preset`].
fn run_3dorbit(s: &mut Session, p: &Value) -> Result<Value> {
    if has_param(p, "preset") {
        let preset = str_param(p, "preset").ok_or_else(|| bad("3dorbit", "unknown camera preset"))?;
        let cam = cadcraft_geom::Camera3D::preset(preset).ok_or_else(|| bad("3dorbit", "unknown camera preset"))?;
        s.state_mut()?.camera = cam;
        return ok();
    }
    let daz = f64_opt("3dorbit", p, "daz", 0.0)?;
    let del = f64_opt("3dorbit", p, "del", 0.0)?;
    let clamp = bool_or(p, "clamp", true);
    let st = s.state_mut()?;
    let base_cam = st.camera.with_twist(st.view().twist);
    st.camera = base_cam.orbit(daz, del, clamp);
    ok()
}

/// Execute `view.roll` (`{angle?, inPlane?}`): snap world-Z heading to `max(90, |angle|)` steps (`inPlane = false`) or roll in-plane by exact `angle` (`inPlane = true`).
fn run_view_roll(s: &mut Session, p: &Value) -> Result<Value> {
    let angle = f64_opt("view.roll", p, "angle", 90.0)?;
    let in_plane = bool_or(p, "inPlane", false);
    let st = s.state_mut()?;
    let base_cam = st.camera.with_twist(st.view().twist);
    st.camera = if in_plane {
        base_cam.roll_in_plane(angle)
    } else {
        base_cam.roll_world_z(angle)
    };
    ok()
}

/// Execute `view.set`: set a [`cadcraft_geom::Camera3D::preset`], orthogonal neighbor, custom
/// direction/up, or 2D center/height/twist.
fn run_view_set(s: &mut Session, p: &Value) -> Result<Value> {
    let has_center = has_param(p, "center");
    let has_height = has_param(p, "height");
    if has_center != has_height {
        return Err(bad("view.set", "center and height must be given together"));
    }
    if has_param(p, "twist") && !has_center {
        return Err(bad("view.set", "`twist` requires `center` and `height`"));
    }
    let center_height = if has_center {
        let c = point_req("view.set", p, "center")?;
        let h = f64_req("view.set", p, "height")?;
        if h <= 0.0 {
            return Err(bad("view.set", "height must be positive"));
        }
        Some((c, h))
    } else {
        None
    };
    let has_neighbor = has_param(p, "neighbor");
    let has_preset = has_param(p, "preset");
    let has_dir = has_param(p, "dir");
    if usize::from(has_neighbor) + usize::from(has_preset) + usize::from(has_dir) > 1 {
        return Err(bad("view.set", "`preset`, `dir`, and `neighbor` are mutually exclusive"));
    }
    if has_param(p, "up") && !has_dir {
        return Err(bad("view.set", "`up` requires `dir`"));
    }
    let new_cam = if has_neighbor {
        let neighbor = str_param(p, "neighbor").ok_or_else(|| bad("view.set", "unknown neighbor direction"))?;
        let dir = match neighbor.trim().to_ascii_lowercase().as_str() {
            "top" | "up" | "n" => "top",
            "bottom" | "down" | "s" => "bottom",
            "left" | "w" => "left",
            "right" | "e" => "right",
            _ => return Err(bad("view.set", "unknown neighbor direction")),
        };
        let st = s.state()?;
        Some(st.camera.with_twist(st.view().twist).orthogonal_neighbor(dir))
    } else if has_preset {
        let preset = str_param(p, "preset").ok_or_else(|| bad("view.set", "unknown camera preset"))?;
        Some(cadcraft_geom::Camera3D::preset(preset).ok_or_else(|| bad("view.set", "unknown camera preset"))?)
    } else if let Some(dir) = vec3_nonzero("view.set", p, "dir")? {
        let up = vec3_nonzero("view.set", p, "up")?.unwrap_or(cadcraft_geom::Vec3::Z);
        Some(cadcraft_geom::Camera3D::from_dir_up(dir, up))
    } else {
        None
    };
    let Some((c, h)) = center_height else {
        if let Some(cam) = new_cam {
            s.state_mut()?.camera = cam;
            return ok();
        }
        return Err(bad("view.set", "center and height, or a camera (preset, dir, neighbor), is required"));
    };
    let twist = match p.get("twist") {
        Some(t) => t.as_f64().filter(|t| t.is_finite()).ok_or_else(|| bad("view.set", "twist must be a number of degrees"))?.to_radians(),
        None => s.state()?.view().twist,
    };
    // As DVIEW TWist: the Model tab's view or the active layout viewport turns; a layout sheet
    // (paper space) is never twisted.
    let target = match dview::target(s) {
        Ok(t) => Some(t),
        Err(_) if twist == 0.0 => None,
        Err(e) => return Err(e),
    };
    let before = (s.state()?.doc.clone(), s.state()?.selection.clone());
    let twist = match target {
        Some(t @ dview::Target::Viewport(_)) => {
            dview::set_twist(s, t, twist)?;
            s.state()?.view().twist
        }
        Some(dview::Target::Model) => cadcraft_geom::norm_angle(twist),
        None => 0.0,
    };
    if !s.state_mut()?.set_view(View { center: c, height: h, twist }) {
        s.state_mut()?.doc = before.0;
        return Err(bad("view.set", "the view is too large to show. Nothing was changed"));
    }
    if let Some(dview::Target::Viewport(h)) = target {
        dview::record_viewport_edit(s, before, h)?;
    }
    if let Some(cam) = new_cam {
        s.state_mut()?.camera = cam;
    }
    ok()
}

fn run_layout_set(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").unwrap_or("Model");
    let space = if name.eq_ignore_ascii_case("model") {
        Space::Model
    } else {
        let l = s.doc()?.layout(name).ok_or_else(|| bad("layout.set", format!("no layout `{name}`")))?;
        Space::Paper(l.name.clone())
    };
    s.pending_window = None;
    let st = s.state_mut()?;
    st.space = space.clone();
    st.mspace = None;
    st.selection.clear();
    if let Space::Paper(n) = &space
        && !st.views.iter().any(|(sp, _)| sp == &space)
    {
        // Default paper view: the sheet.
        let page = st.doc.layout(n).map(|l| l.page.clone()).unwrap_or_default();
        let (w, h) = if page.landscape { (page.height_mm, page.width_mm) } else { (page.width_mm, page.height_mm) };
        let unit = if st.doc.header.i64("MEASUREMENT", 0) == 1 { 1.0 } else { 1.0 / 25.4 };
        st.set_view_quiet(View { center: Vec2::new(w * unit / 2.0, h * unit / 2.0), height: h * unit * 1.15, twist: 0.0 });
    }
    s.touch();
    ok()
}

#[derive(Default)]
struct ZoomM {
    window: bool,
    first: Option<Vec2>,
    stage: ZoomStage,
    /// Where the command starts (View > Zoom > Realtime / Dynamic skip the first prompt).
    start: ZoomStart,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum ZoomStart {
    #[default]
    Prompt,
    Realtime,
    Dynamic,
}

/// The Center, Scale, Realtime and Dynamic options' own prompts.
#[derive(Clone, Copy, Default)]
enum ZoomStage {
    #[default]
    Main,
    Center,
    CenterHeight(Vec2),
    Scale,
    Realtime,
    Dynamic(DynBox),
}

/// The prompt of Zoom Realtime; [`Session::realtime_zoom_active`] recognises the mode by it.
const REALTIME_PROMPT: &str = "Drag up to zoom in or down to zoom out; press Esc or Enter to exit";

/// Zoom Dynamic: the view box shown over the drawing extents. A point places the box, the next
/// one sizes it (its left edge stays), and so on; Enter shows what the box holds.
#[derive(Clone, Copy)]
struct DynBox {
    /// The screen view and the zoom frame before the overview.
    screen: View,
    frame: View,
    /// The drawing extents (the limits when empty).
    extents: Bounds2,
    center: Vec2,
    height: f64,
    /// Width / height of the area being zoomed.
    aspect: f64,
    /// The next point sizes the box (else it places it).
    sizing: bool,
}

impl DynBox {
    fn rect(c: Vec2, h: f64, aspect: f64) -> Bounds2 {
        let half = Vec2::new(h * aspect / 2.0, h / 2.0);
        Bounds2::new(c - half, c + half)
    }
    /// The box a point `p` makes: centred on it, or (sizing) reaching it from the left edge.
    fn moved(&self, p: Vec2) -> (Vec2, f64) {
        if !self.sizing {
            return (p, self.height);
        }
        let left = self.center.x - self.height * self.aspect / 2.0;
        let w = (p.x - left).max(self.height * self.aspect * 1e-3).max(1e-9);
        (Vec2::new(left + w / 2.0, self.center.y), w / self.aspect)
    }
}

impl ZoomM {
    /// Start Realtime: one view history entry for the whole drag session (Zoom Previous).
    fn realtime(&mut self, s: &mut Session) -> Result<Step> {
        let st = s.state_mut()?;
        let v = st.view();
        st.view_history.push(v);
        if st.view_history.len() > 50 {
            st.view_history.remove(0);
        }
        self.stage = ZoomStage::Realtime;
        Ok(Step::Continue)
    }
    /// Start Dynamic: zoom out to show the extents and the current view, and put the view box
    /// where the view was. (Esc leaves the overview; Zoom Previous returns.)
    fn dynamic(&mut self, s: &mut Session) -> Result<Step> {
        let (w, h) = s.viewport_px;
        let st = s.state()?;
        let (screen, frame) = (st.view(), st.zoom_frame());
        let aspect = st.zoom_aspect(w / h.max(1.0)).max(1e-6);
        let ext = ext_bounds(s)?;
        let extents = if ext.is_empty() { s.zoom_limits()? } else { ext };
        let all = extents.union(&DynBox::rect(frame.center, frame.height, aspect));
        fit(s, all.expand(all.height().max(all.width()) * 0.05))?;
        self.stage = ZoomStage::Dynamic(DynBox { screen, frame, extents, center: frame.center, height: frame.height, aspect, sizing: false });
        Ok(Step::Continue)
    }
}

impl Interactive for ZoomM {
    fn name(&self) -> &'static str {
        "ZOOM"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        match self.start {
            ZoomStart::Prompt => Ok(Step::Continue),
            ZoomStart::Realtime => self.realtime(s),
            ZoomStart::Dynamic => self.dynamic(s),
        }
    }
    fn prompt(&self, s: &Session) -> Prompt {
        match self.stage {
            ZoomStage::Main => {}
            ZoomStage::Realtime => return Prompt::new(REALTIME_PROMPT, Accept { enter: true, ..Accept::default() }),
            ZoomStage::Dynamic(b) if b.sizing => {
                let left = Vec2::new(b.center.x - b.height * b.aspect / 2.0, b.center.y);
                return Prompt::new("Specify size of view box", Accept::POINT).default("accept").base(left);
            }
            ZoomStage::Dynamic(_) => return Prompt::new("Specify center of view box", Accept::POINT).default("accept"),
            ZoomStage::Center => return Prompt::new("Specify center point", Accept::POINT),
            ZoomStage::CenterHeight(_) => {
                let h = s.state().map(|st| st.zoom_frame().height).unwrap_or(1.0);
                return Prompt::new("Enter magnification or height", Accept::NUMBER).default(format!("{h:.4}"));
            }
            ZoomStage::Scale => return Prompt::new("Enter a scale factor (nX or nXP)", Accept::NUMBER),
        }
        match (self.window, self.first) {
            (false, None) => Prompt::new("Specify corner of window, enter a scale factor (nX or nXP)", Accept::POINT_OR_NUMBER)
                .kw(&["All", "Center", "Dynamic", "Extents", "Previous", "Scale", "Window", "Object"])
                .default("real time"),
            (true, None) => Prompt::new("Specify first corner", Accept::POINT),
            (_, Some(a)) => Prompt::new("Specify opposite corner", Accept::POINT).corner(a),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        let bad_factor = || crate::EngineError::Other("Requires a scale factor (n, nX or nXP).".into());
        match (self.stage, &i) {
            (_, Input::Cancel) | (ZoomStage::Main, _) => {}
            (ZoomStage::Realtime, Input::Enter) => return Ok(Step::Done),
            (ZoomStage::Realtime, _) => return Ok(Step::Continue),
            (ZoomStage::Dynamic(b), Input::Enter) => {
                // Back to the view before the overview without a history entry, then zoom from it.
                let st = s.state_mut()?;
                st.set_view_quiet(b.screen);
                if st.view_history.last() == Some(&b.screen) {
                    st.view_history.pop();
                }
                st.set_zoom_frame(View { center: b.center, height: b.height, twist: b.frame.twist });
                return Ok(Step::Done);
            }
            (ZoomStage::Dynamic(mut b), Input::Point(p)) => {
                (b.center, b.height) = b.moved(*p);
                b.sizing = !b.sizing;
                self.stage = ZoomStage::Dynamic(b);
                return Ok(Step::Continue);
            }
            (ZoomStage::Dynamic(_), _) => return Err(crate::EngineError::Other("Requires a point, or Enter to accept the view.".into())),
            (ZoomStage::Center, Input::Point(c)) => {
                self.stage = ZoomStage::CenterHeight(*c);
                return Ok(Step::Continue);
            }
            (ZoomStage::Center, Input::Enter) => return Ok(Step::Done),
            (ZoomStage::Center, _) => return Err(crate::EngineError::Other("Requires a point.".into())),
            (ZoomStage::CenterHeight(c), Input::Enter) => {
                zoom(s, &json!({ "mode": "center", "center": [c.x, c.y] }))?;
                return Ok(Step::Done);
            }
            (ZoomStage::CenterHeight(c), Input::Text(t)) => {
                let tl = t.trim().to_ascii_lowercase();
                // A plain number is the new view height; nX / nXP a magnification.
                let p = if tl.ends_with('x') || tl.ends_with("xp") {
                    typed_factor(&tl).ok_or_else(bad_factor)?
                } else {
                    json!({ "height": number(&tl).filter(|h| *h > 0.0).ok_or_else(bad_factor)? })
                };
                zoom(s, &with_mode(p, "center", Some(c)))?;
                return Ok(Step::Done);
            }
            (ZoomStage::Scale, Input::Text(t)) => {
                zoom(s, &with_mode(typed_factor(t).ok_or_else(bad_factor)?, "scale", None))?;
                return Ok(Step::Done);
            }
            (ZoomStage::Scale, Input::Enter) => return Ok(Step::Done),
            (_, _) => return Err(bad_factor()),
        }
        match i {
            Input::Keyword(k) => {
                let mode = match k.as_str() {
                    "All" => "all",
                    "Extents" => "extents",
                    "Previous" => "previous",
                    "Dynamic" => return self.dynamic(s),
                    "Window" => {
                        self.window = true;
                        return Ok(Step::Continue);
                    }
                    "Center" => {
                        self.stage = ZoomStage::Center;
                        return Ok(Step::Continue);
                    }
                    "Scale" => {
                        self.stage = ZoomStage::Scale;
                        return Ok(Step::Continue);
                    }
                    "Object" => "object",
                    _ => return Err(crate::EngineError::Other("Invalid option keyword.".into())),
                };
                zoom(s, &json!({ "mode": mode }))?;
                Ok(Step::Done)
            }
            Input::Point(p) => match self.first {
                None => {
                    self.first = Some(p);
                    Ok(Step::Continue)
                }
                Some(a) => {
                    fit(s, Bounds2::new(a, p))?;
                    Ok(Step::Done)
                }
            },
            Input::Text(t) => {
                // nX: relative to the current view; nXP: to paper space; n: to the limits.
                let p = typed_factor(&t).ok_or_else(|| crate::EngineError::Other("Requires a point, scale factor or option keyword.".into()))?;
                zoom(s, &with_mode(p, "scale", None))?;
                Ok(Step::Done)
            }
            // Enter at the first prompt: Realtime (the default), as in AutoCAD.
            Input::Enter if !self.window && self.first.is_none() => self.realtime(s),
            Input::Enter => Ok(Step::Done),
            _ => Ok(Step::Continue),
        }
    }
    fn preview(&self, _s: &Session, c: Vec2) -> Vec<cadcraft_doc::EntityKind> {
        use super::helpers::{line, lwpoly, rect_vertices};
        let rect = |r: Bounds2| lwpoly(rect_vertices(r.min, r.max), true);
        if let ZoomStage::Dynamic(b) = self.stage {
            // The extents, the view before, and the box: where it is and where the cursor puts it.
            let (center, height) = b.moved(c);
            let next = DynBox::rect(center, height, b.aspect);
            let mut out = vec![
                rect(b.extents),
                rect(DynBox::rect(b.frame.center, b.frame.height, b.aspect)),
                rect(DynBox::rect(b.center, b.height, b.aspect)),
                rect(next),
            ];
            if !b.sizing {
                out.push(line(next.min, next.max));
                out.push(line(Vec2::new(next.min.x, next.max.y), Vec2::new(next.max.x, next.min.y)));
            }
            return out;
        }
        self.first.map(|a| vec![rect(Bounds2::new(a, c))]).unwrap_or_default()
    }
}

impl Session {
    /// ZOOM is in its Realtime mode: the UI turns vertical drags into [`Session::realtime_zoom`].
    pub fn realtime_zoom_active(&self) -> bool {
        self.running.as_ref().is_some_and(|r| r.id.starts_with("zoom")) && self.current_prompt().is_some_and(|p| p.message == REALTIME_PROMPT)
    }
    /// One step of a Realtime zoom drag of `dy` pixels (down is positive): dragging up half the
    /// drawing area's height zooms in 2x, down zooms out, about the centre of the view. Does
    /// nothing outside Realtime.
    pub fn realtime_zoom(&mut self, dy: f64) -> Result<()> {
        if !self.realtime_zoom_active() || !dy.is_finite() {
            return Ok(());
        }
        let half = (self.viewport_px.1 / 2.0).max(1.0);
        let f = 2f64.powf((-dy / half).clamp(-16.0, 16.0));
        let st = self.state_mut()?;
        let v = st.view();
        st.set_view_quiet(View { height: (v.height / f).clamp(1e-9, 1e12), ..v });
        self.touch();
        Ok(())
    }
    /// The zoom of a mouse wheel scroll by `delta` points (up zooms in): ZOOMFACTOR scales it
    /// (60, the default, zooms about 18% per wheel step).
    pub fn wheel_zoom_factor(&self, delta: f64) -> f64 {
        if !delta.is_finite() {
            return 1.0;
        }
        let k = f64::from(self.settings.zoomfactor.clamp(3, 100)) / 60.0;
        (delta / 300.0 * k).clamp(-40.0, 40.0).exp()
    }
}

#[derive(Default)]
struct PanM {
    first: Option<Vec2>,
}

impl Interactive for PanM {
    fn name(&self) -> &'static str {
        "PAN"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match self.first {
            None => Prompt::new("Specify base point or displacement (drag with the middle mouse button for realtime pan)", Accept::POINT),
            Some(a) => Prompt::new("Specify second point", Accept::POINT).base(a),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match (self.first, i) {
            (None, Input::Point(p)) => {
                self.first = Some(p);
                Ok(Step::Continue)
            }
            (Some(a), Input::Point(b)) => {
                run_pan(s, &json!({ "from": [a.x, a.y], "to": [b.x, b.y] }))?;
                Ok(Step::Done)
            }
            (_, Input::Enter) => Ok(Step::Done),
            _ => Ok(Step::Continue),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn view_set_rejects_zero_dir_and_zero_up() {
        let mut s = Session::new();
        assert!(s.execute("view.set", &json!({ "preset": "front" })).is_ok());
        let before = s.state().ok().map(|st| st.camera);

        assert!(s.execute("view.set", &json!({ "dir": [0, 0, 0] })).is_err());
        assert!(s.execute("view.set", &json!({ "dir": [1, 0, 0], "up": [0, 0, 0] })).is_err());
        assert!(s.execute("view.set", &json!({ "up": [0, 0, 1] })).is_err());
        assert_eq!(s.state().ok().map(|st| st.camera), before);
    }

    #[test]
    fn view_set_rejects_mutually_exclusive_camera_selectors() {
        let mut s = Session::new();
        assert!(s.execute("view.set", &json!({ "preset": "front", "dir": [0, 1, 0] })).is_err());
        assert!(s.execute("view.set", &json!({ "preset": "front", "neighbor": "top" })).is_err());
        assert!(s.execute("view.set", &json!({ "dir": [0, 1, 0], "neighbor": "top" })).is_err());
        assert!(s.execute("view.set", &json!({ "preset": "front", "dir": [0, 1, 0], "neighbor": "top" })).is_err());
    }

    #[test]
    fn view_set_requires_center_and_height_for_twist() {
        let mut s = Session::new();
        assert!(s.execute("view.set", &json!({ "twist": 30 })).is_err());
        assert!(s.execute("view.set", &json!({ "preset": "front", "twist": 30 })).is_err());
        assert_eq!(s.state().ok().map(|st| st.camera), Some(cadcraft_geom::Camera3D::TOP));
    }

    #[test]
    fn view_set_requires_exact_three_element_vectors() {
        let mut s = Session::new();
        assert!(s.execute("view.set", &json!({ "dir": [1, 0] })).is_err());
        assert!(s.execute("view.set", &json!({ "dir": [1, 0, 0, 0] })).is_err());
        assert!(s.execute("view.set", &json!({ "dir": [1, 0, 0], "up": [0, 1] })).is_err());
        assert!(s.execute("view.set", &json!({ "dir": [1, 0, 0], "up": [0, 0, 1, 0] })).is_err());
        assert!(s.execute("view.set", &json!({ "dir": [1, 0, 0], "up": [0, 0, 1] })).is_ok());
    }

    #[test]
    fn orbit_validates_numeric_and_preset_params() {
        let mut s = Session::new();
        let before = s.state().ok().map(|st| st.camera);
        assert!(s.execute("3dorbit", &json!({ "daz": "bad" })).is_err());
        assert!(s.execute("3dorbit", &json!({ "del": "bad" })).is_err());
        assert!(s.execute("3dorbit", &json!({ "preset": 123 })).is_err());
        assert!(s.execute("3dorbit", &json!({ "preset": "does_not_exist" })).is_err());
        assert_eq!(s.state().ok().map(|st| st.camera), before);
    }

    #[test]
    fn view_roll_modes_and_validation() {
        let mut s = Session::new();
        assert!(s.execute("view.roll", &json!({ "angle": "bad" })).is_err());

        // Default world-Z roll snaps to 90° steps even for angle: 30.
        assert!(s.execute("view.set", &json!({ "preset": "front" })).is_ok());
        assert!(s.execute("view.roll", &json!({ "angle": 30 })).is_ok());
        let world_z_rolled = s.state().ok().map(|st| st.camera.view_name());
        assert_eq!(world_z_rolled, Some("Left"));

        // inPlane: true rolls continuously by the exact angle in-plane.
        assert!(s.execute("view.set", &json!({ "preset": "front" })).is_ok());
        assert!(s.execute("view.roll", &json!({ "angle": 30, "inPlane": true })).is_ok());
        let expected_y = cadcraft_geom::Camera3D::preset("front").map(|c| c.roll_in_plane(30.0).y);
        assert_eq!(s.state().ok().map(|st| st.camera.y), expected_y);
    }

    type ViewSnapshot = (cadcraft_geom::Vec2, f64, f64, cadcraft_geom::Camera3D);

    fn snapshot(s: &Session) -> Option<ViewSnapshot> {
        let st = s.state().ok()?;
        let v = st.view();
        Some((v.center, v.height, v.twist, st.camera))
    }

    fn seeded_session() -> Session {
        let mut s = Session::new();
        let _ = s.execute(
            "view.set",
            &json!({ "center": [12.0, -4.0], "height": 42.0, "twist": 15.0, "preset": "sw_isometric" }),
        );
        s
    }

    fn preset_cam(name: &str) -> cadcraft_geom::Camera3D {
        cadcraft_geom::Camera3D::preset(name).unwrap_or_default()
    }

    fn json_vec3(v: &Value, key: &str) -> Option<[f64; 3]> {
        let a = v.get(key)?.as_array()?;
        if a.len() != 3 {
            return None;
        }
        Some([a.first()?.as_f64()?, a.get(1)?.as_f64()?, a.get(2)?.as_f64()?])
    }

    fn assert_arr3_near(actual: Option<[f64; 3]>, expected: cadcraft_geom::Vec3, label: &str) {
        let Some([x, y, z]) = actual else {
            assert!(actual.is_some(), "missing vec3 for {label}");
            return;
        };
        assert!(
            (x - expected.x).abs() < 1e-9 && (y - expected.y).abs() < 1e-9 && (z - expected.z).abs() < 1e-9,
            "{label}: got [{x}, {y}, {z}], want {:?}",
            expected
        );
    }

    #[test]
    fn view_set_branches_update_camera_and_view_table() {
        let cases: Vec<(&str, Value, Value, (f64, f64, f64, f64), cadcraft_geom::Camera3D)> = vec![
            (
                "preset_front_keeps_view",
                json!({ "center": [5.0, 6.0], "height": 20.0, "twist": 10.0, "preset": "top" }),
                json!({ "preset": "front" }),
                (5.0, 6.0, 20.0, 10.0_f64.to_radians()),
                preset_cam("front"),
            ),
            (
                "neighbor_down_from_top",
                json!({ "center": [5.0, 6.0], "height": 20.0, "twist": 0.0, "preset": "top" }),
                json!({ "neighbor": "down" }),
                (5.0, 6.0, 20.0, 0.0),
                cadcraft_geom::Camera3D::TOP.orthogonal_neighbor("bottom"),
            ),
            (
                "neighbor_west_with_twist",
                json!({ "center": [0.0, 0.0], "height": 10.0, "twist": 90.0, "preset": "top" }),
                json!({ "neighbor": "w" }),
                (0.0, 0.0, 10.0, 90.0_f64.to_radians()),
                cadcraft_geom::Camera3D::TOP.with_twist(90.0_f64.to_radians()).orthogonal_neighbor("left"),
            ),
            (
                "dir_only",
                json!({ "preset": "top" }),
                json!({ "dir": [0.0, -1.0, 0.0] }),
                (0.0, 0.0, 0.0, 0.0),
                cadcraft_geom::Camera3D::from_dir_up(cadcraft_geom::Vec3::new(0.0, -1.0, 0.0), cadcraft_geom::Vec3::Z),
            ),
            (
                "dir_and_up",
                json!({ "preset": "top" }),
                json!({ "dir": [1.0, 1.0, 1.0], "up": [0.0, 0.0, 1.0] }),
                (0.0, 0.0, 0.0, 0.0),
                cadcraft_geom::Camera3D::from_dir_up(cadcraft_geom::Vec3::new(1.0, 1.0, 1.0), cadcraft_geom::Vec3::Z),
            ),
            (
                "combined_center_height_twist_and_preset",
                json!({ "preset": "top" }),
                json!({ "center": [10.0, 25.0], "height": 50.0, "twist": 30.0, "preset": "sw_isometric" }),
                (10.0, 25.0, 50.0, 30.0_f64.to_radians()),
                preset_cam("sw_isometric"),
            ),
            (
                "combined_center_height_and_dir_preserves_twist",
                json!({ "center": [1.0, 2.0], "height": 15.0, "twist": 22.5, "preset": "front" }),
                json!({ "center": [-3.0, 7.0], "height": 80.0, "dir": [0.0, 0.0, -1.0], "up": [0.0, 1.0, 0.0] }),
                (-3.0, 7.0, 80.0, 22.5_f64.to_radians()),
                cadcraft_geom::Camera3D::from_dir_up(cadcraft_geom::Vec3::new(0.0, 0.0, -1.0), cadcraft_geom::Vec3::new(0.0, 1.0, 0.0)),
            ),
        ];

        for (label, setup, args, expected_view, expected_cam) in cases {
            let mut s = Session::new();
            assert!(s.execute("view.set", &setup).is_ok(), "{label}: setup failed");
            let base_snap = snapshot(&s);
            assert!(s.execute("view.set", &args).is_ok(), "{label}: command failed");
            let got = snapshot(&s);
            let want_view = if args.get("center").is_some() || setup.get("center").is_some() {
                (cadcraft_geom::Vec2::new(expected_view.0, expected_view.1), expected_view.2, expected_view.3)
            } else if let Some((c, h, t, _)) = base_snap {
                (c, h, t)
            } else {
                (cadcraft_geom::Vec2::ZERO, 0.0, 0.0)
            };
            assert_eq!(got, Some((want_view.0, want_view.1, want_view.2, expected_cam)), "{label}: state mismatch");
        }
    }

    #[test]
    fn view_set_errors_do_not_mutate_state_table() {
        let invalid_cases = [
            ("empty", json!({})),
            ("center_without_height", json!({ "center": [1.0, 2.0] })),
            ("height_without_center", json!({ "height": 10.0 })),
            ("twist_without_center_height", json!({ "twist": 20.0 })),
            ("preset_and_twist_only", json!({ "preset": "front", "twist": 20.0 })),
            ("zero_height", json!({ "center": [1.0, 2.0], "height": 0.0 })),
            ("negative_height", json!({ "center": [1.0, 2.0], "height": -5.0 })),
            ("bad_center_array", json!({ "center": [1.0], "height": 10.0 })),
            ("bad_twist_with_valid_center_height", json!({ "center": [1.0, 2.0], "height": 10.0, "twist": "bad" })),
            ("preset_and_dir", json!({ "preset": "front", "dir": [0.0, 1.0, 0.0] })),
            ("preset_and_neighbor", json!({ "preset": "front", "neighbor": "top" })),
            ("dir_and_neighbor", json!({ "dir": [0.0, 1.0, 0.0], "neighbor": "top" })),
            ("all_three_selectors", json!({ "preset": "front", "dir": [0.0, 1.0, 0.0], "neighbor": "top" })),
            ("up_without_dir", json!({ "up": [0.0, 0.0, 1.0] })),
            ("unknown_preset", json!({ "preset": "bogus" })),
            ("unknown_neighbor", json!({ "neighbor": "diagonal" })),
            ("zero_dir", json!({ "dir": [0.0, 0.0, 0.0] })),
            ("zero_up", json!({ "dir": [1.0, 0.0, 0.0], "up": [0.0, 0.0, 0.0] })),
            ("two_elem_dir", json!({ "dir": [1.0, 0.0] })),
            ("four_elem_up", json!({ "dir": [1.0, 0.0, 0.0], "up": [0.0, 0.0, 1.0, 0.0] })),
            ("valid_view_with_invalid_preset", json!({ "center": [99.0, 99.0], "height": 100.0, "preset": "bogus" })),
            ("invalid_height_with_valid_preset", json!({ "center": [99.0, 99.0], "height": -1.0, "preset": "front" })),
        ];

        for (label, args) in invalid_cases {
            let mut s = seeded_session();
            let before = snapshot(&s);
            assert!(s.execute("view.set", &args).is_err(), "{label}: expected error");
            assert_eq!(snapshot(&s), before, "{label}: state mutated on error");
        }
    }

    #[test]
    fn orbit_valid_and_error_cases_table() {
        let valid_cases = [
            (
                "orbit_default_clamp",
                json!({ "preset": "top" }),
                json!({ "daz": 35.0, "del": -25.0 }),
                cadcraft_geom::Camera3D::TOP.orbit(35.0, -25.0, true),
            ),
            (
                "orbit_unclamped",
                json!({ "preset": "front" }),
                json!({ "daz": 0.0, "del": 120.0, "clamp": false }),
                preset_cam("front").orbit(0.0, 120.0, false),
            ),
            (
                "orbit_from_twisted_top",
                json!({ "center": [0.0, 0.0], "height": 10.0, "twist": 45.0, "preset": "top" }),
                json!({ "daz": 15.0, "del": -10.0 }),
                cadcraft_geom::Camera3D::TOP.with_twist(45.0_f64.to_radians()).orbit(15.0, -10.0, true),
            ),
            ("orbit_preset", json!({ "preset": "top" }), json!({ "preset": "se_isometric" }), preset_cam("se_isometric")),
        ];

        for (label, setup, args, expected_cam) in valid_cases {
            let mut s = Session::new();
            assert!(s.execute("view.set", &setup).is_ok(), "{label}: setup failed");
            assert!(s.execute("3dorbit", &args).is_ok(), "{label}: 3dorbit failed");
            assert_eq!(s.state().ok().map(|st| st.camera), Some(expected_cam), "{label}: camera mismatch");
        }

        let invalid_cases = [
            ("unknown_preset", json!({ "preset": "nope" })),
            ("non_string_preset", json!({ "preset": 42 })),
            ("bad_daz_string", json!({ "daz": "nan" })),
            ("bad_del_array", json!({ "del": [1] })),
            ("bad_daz_bool", json!({ "daz": true })),
        ];

        for (label, args) in invalid_cases {
            let mut s = seeded_session();
            let before = snapshot(&s);
            assert!(s.execute("3dorbit", &args).is_err(), "{label}: expected error");
            assert_eq!(snapshot(&s), before, "{label}: state mutated on error");
        }
    }

    #[test]
    fn view_roll_valid_and_error_cases_table() {
        let valid_cases = [
            ("world_z_plus_90", json!({ "preset": "front" }), json!({ "angle": 90.0 }), preset_cam("front").roll_world_z(90.0)),
            ("world_z_minus_90", json!({ "preset": "front" }), json!({ "angle": -90.0 }), preset_cam("front").roll_world_z(-90.0)),
            ("world_z_snaps_30_to_90", json!({ "preset": "front" }), json!({ "angle": 30.0 }), preset_cam("front").roll_world_z(30.0)),
            ("world_z_180", json!({ "preset": "front" }), json!({ "angle": 180.0 }), preset_cam("front").roll_world_z(180.0)),
            (
                "in_plane_continuous_30",
                json!({ "preset": "front" }),
                json!({ "angle": 30.0, "inPlane": true }),
                preset_cam("front").roll_in_plane(30.0),
            ),
            (
                "in_plane_continuous_neg45",
                json!({ "preset": "right" }),
                json!({ "angle": -45.0, "inPlane": true }),
                preset_cam("right").roll_in_plane(-45.0),
            ),
            (
                "twisted_top_world_z",
                json!({ "center": [0.0, 0.0], "height": 10.0, "twist": 90.0, "preset": "top" }),
                json!({ "angle": 90.0 }),
                cadcraft_geom::Camera3D::TOP.with_twist(90.0_f64.to_radians()).roll_world_z(90.0),
            ),
        ];

        for (label, setup, args, expected_cam) in valid_cases {
            let mut s = Session::new();
            assert!(s.execute("view.set", &setup).is_ok(), "{label}: setup failed");
            assert!(s.execute("view.roll", &args).is_ok(), "{label}: view.roll failed");
            assert_eq!(s.state().ok().map(|st| st.camera), Some(expected_cam), "{label}: camera mismatch");
        }

        let invalid_cases = [("bad_angle_string", json!({ "angle": "bad" })), ("bad_angle_bool", json!({ "angle": false }))];
        for (label, args) in invalid_cases {
            let mut s = seeded_session();
            let before = snapshot(&s);
            assert!(s.execute("view.roll", &args).is_err(), "{label}: expected error");
            assert_eq!(snapshot(&s), before, "{label}: state mutated on error");
        }
    }

    #[test]
    fn plan_resets_camera_to_top_while_preserving_center_and_height() {
        let mut s = Session::new();
        assert!(
            s.execute("view.set", &json!({ "center": [24.0, -12.0], "height": 64.0, "preset": "sw_isometric" }))
                .is_ok()
        );
        assert!(s.execute("3dorbit", &json!({ "daz": 15.0, "del": -10.0 })).is_ok());
        let before_view = s.state().ok().map(|st| (st.view().center, st.view().height));

        assert!(s.execute("plan", &json!({})).is_ok());
        assert_eq!(s.state().ok().map(|st| st.camera), Some(cadcraft_geom::Camera3D::TOP));
        assert_eq!(s.state().ok().map(|st| (st.view().center, st.view().height)), before_view);

        let info = s.execute("view.get", &json!({}));
        assert!(info.is_ok());
        if let Ok(v) = info {
            assert_eq!(v.get("viewName").and_then(Value::as_str), Some("Top"));
            assert_arr3_near(json_vec3(&v, "dir"), cadcraft_geom::Camera3D::TOP.z, "plan dir");
            assert_arr3_near(json_vec3(&v, "up"), cadcraft_geom::Camera3D::TOP.y, "plan up");
        }
    }

    #[test]
    fn view_get_reports_name_dir_and_up_after_preset_and_orbit() {
        let mut s = Session::new();
        let steps: Vec<(&str, &str, Value)> = vec![
            ("initial_top", "view.get", json!({})),
            ("preset_front", "view.set", json!({ "preset": "front" })),
            ("preset_right", "view.set", json!({ "preset": "right" })),
            ("preset_sw_iso", "view.set", json!({ "preset": "sw_isometric" })),
            ("orbit_custom", "3dorbit", json!({ "daz": 25.0, "del": -15.0 })),
        ];

        for (label, cmd, args) in steps {
            if cmd != "view.get" {
                assert!(s.execute(cmd, &args).is_ok(), "{label}: {cmd} failed");
            }
            let cam = s.state().ok().map(|st| st.camera).unwrap_or_default();
            let got = s.execute("view.get", &json!({}));
            assert!(got.is_ok(), "{label}: view.get failed");
            if let Ok(v) = got {
                assert_eq!(v.get("viewName").and_then(Value::as_str), Some(cam.view_name()), "{label}: viewName");
                assert_arr3_near(json_vec3(&v, "dir"), cam.z, label);
                assert_arr3_near(json_vec3(&v, "up"), cam.y, label);
            }
        }
    }
}
