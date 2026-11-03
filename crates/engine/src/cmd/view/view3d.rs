//! 3D views of model space: VPOINT (a viewing direction, or a preset view of View ▸ 3D Views) and
//! PLAN (back to the plan view of the WCS), both zoomed to the extents as in AutoCAD, and 3DORBIT
//! (drag to turn the view about the point at the centre of the screen).

use cadcraft_doc::Space;
use cadcraft_geom::{Mat4, Vec2, Vec3};
use serde_json::{Value, json};

use super::super::{CommandSpec, bad, str_param};
use crate::view::tiled;
use crate::{Accept, Input, Interactive, Prompt, Result, Session, Step, View};

/// The preset views: name, label (the View ▸ 3D Views menu), viewing direction (towards the viewer).
pub(crate) const PRESETS: [(&str, &str, [f64; 3]); 10] = [
    ("top", "Top", [0.0, 0.0, 1.0]),
    ("bottom", "Bottom", [0.0, 0.0, -1.0]),
    ("left", "Left", [-1.0, 0.0, 0.0]),
    ("right", "Right", [1.0, 0.0, 0.0]),
    ("front", "Front", [0.0, -1.0, 0.0]),
    ("back", "Back", [0.0, 1.0, 0.0]),
    ("swiso", "SW Isometric", [-1.0, -1.0, 1.0]),
    ("seiso", "SE Isometric", [1.0, -1.0, 1.0]),
    ("neiso", "NE Isometric", [1.0, 1.0, 1.0]),
    ("nwiso", "NW Isometric", [-1.0, 1.0, 1.0]),
];

/// Why a 3D view is refused in tiled viewports or a layout.
const NEEDS_SINGLE: &str = "3D views need model space in a single model viewport: VPORTS Single first";

/// The presets as typed VPOINT's keywords (capitals: the shortcut).
const PRESET_KEYWORDS: [&str; 10] = ["Top", "BOttom", "Left", "Right", "Front", "BAck", "SWiso", "SEiso", "NEiso", "NWiso"];

pub(super) fn specs() -> Vec<CommandSpec> {
    let mut v = vec![
        CommandSpec::new("vpoint", "Viewpoint", run_vpoint)
            .menu(&["View", "3D Views", "Viewpoint"])
            .params("{direction: [x, y, z] (from the target towards the viewer)} | {preset: top|bottom|left|right|front|back|swiso|seiso|neiso|nwiso} → {direction}; typed: x,y,z or a preset keyword")
            .noundo()
            .interactive(|_| Ok(Box::new(VpointM))),
        CommandSpec::new("plan", "Plan View", |s, _| set_direction(s, "plan", Vec3::Z))
            .menu(&["View", "3D Views", "Plan View", "World UCS"])
            .params("{} (the plan view of the WCS, zoomed to the extents)")
            .noundo(),
        CommandSpec::new("3dorbit", "Constrained Orbit", run_orbit)
            .alias(&["3do", "orbit"])
            .menu(&["View", "Orbit", "Constrained Orbit"])
            .params("{azimuth?: degrees about the Z axis, elevation?: degrees up} → {direction}; interactive: drag to orbit about the centre of the view, Enter or Esc ends")
            .noundo()
            .interactive(|_| Ok(Box::new(OrbitM))),
    ];
    macro_rules! preset {
        ($id:literal, $i:literal) => {
            CommandSpec::new($id, PRESETS[$i].1, |s, _| run_vpoint(s, &json!({ "preset": PRESETS[$i].0 })))
                .menu(&["View", "3D Views", PRESETS[$i].1])
                .noundo()
        };
    }
    v.extend([
        preset!("vpoint.top", 0),
        preset!("vpoint.bottom", 1),
        preset!("vpoint.left", 2),
        preset!("vpoint.right", 3),
        preset!("vpoint.front", 4),
        preset!("vpoint.back", 5),
        preset!("vpoint.swiso", 6),
        preset!("vpoint.seiso", 7),
        preset!("vpoint.neiso", 8),
        preset!("vpoint.nwiso", 9),
    ]);
    v
}

/// A preset's viewing direction by its name (`swiso`) or keyword (`SWiso`).
fn preset(name: &str) -> Option<Vec3> {
    let [x, y, z] = PRESETS.iter().find(|(n, _, _)| n.eq_ignore_ascii_case(name.trim()))?.2;
    Some(Vec3::new(x, y, z))
}

fn run_vpoint(s: &mut Session, p: &Value) -> Result<Value> {
    let dir = match (p.get("direction"), str_param(p, "preset")) {
        (Some(d), _) => {
            let c: Option<Vec<f64>> = d.as_array().and_then(|a| a.iter().map(Value::as_f64).collect());
            match c.as_deref() {
                Some([x, y, z]) => Vec3::new(*x, *y, *z),
                _ => return Err(bad("vpoint", "`direction` must be [x, y, z]")),
            }
        }
        (None, Some(name)) => preset(name).ok_or_else(|| bad("vpoint", format!("unknown preset `{}`", name.chars().take(40).collect::<String>())))?,
        (None, None) => return Err(bad("vpoint", "give `direction` or `preset`")),
    };
    set_direction(s, "vpoint", dir)
}

/// Look at model space from `dir` and zoom to the extents. Back to plan is always allowed; a 3D
/// view needs model space in a single viewport.
fn set_direction(s: &mut Session, cmd: &str, dir: Vec3) -> Result<Value> {
    let len = dir.len();
    if !(len.is_finite() && len > 1e-12) {
        return Err(bad(cmd, "the direction must be a non-zero vector"));
    }
    if s.layout_space() != Space::Model {
        return Err(bad(cmd, "3D views are for model space: switch to the Model tab"));
    }
    let dir = dir * (1.0 / len);
    if !Mat4::view(dir, Vec3::ZERO, 0.0).is_plan() && !s.can_view_3d() {
        return Err(bad(cmd, NEEDS_SINGLE));
    }
    s.state_mut()?.set_view_dir(dir);
    s.zoom_extents()?;
    s.touch();
    Ok(json!({ "direction": [dir.x, dir.y, dir.z] }))
}

fn run_orbit(s: &mut Session, p: &Value) -> Result<Value> {
    if !s.can_view_3d() {
        return Err(bad("3dorbit", NEEDS_SINGLE));
    }
    let deg = |k: &str| p.get(k).and_then(Value::as_f64).filter(|v| v.is_finite()).unwrap_or(0.0).to_radians();
    let (azimuth, elevation) = (deg("azimuth"), deg("elevation"));
    if azimuth != 0.0 || elevation != 0.0 {
        turn(s, azimuth, elevation)?;
    }
    let d = s.state()?.view_dir;
    Ok(json!({ "direction": [d.x, d.y, d.z] }))
}

/// Turn the view by `azimuth` about the Z axis and `elevation` up (radians), about the point at
/// the centre of the view, which stays there. The elevation stops short of straight up or down; a
/// plan view starts from the south.
fn turn(s: &mut Session, azimuth: f64, elevation: f64) -> Result<()> {
    let st = s.state_mut()?;
    let d = st.view_dir;
    let flat = Vec2::new(d.x, d.y);
    let az = if flat.len() < 1e-9 { -std::f64::consts::FRAC_PI_2 } else { d.y.atan2(d.x) } + azimuth;
    let limit = 89.9f64.to_radians();
    let el = (d.z.clamp(-1.0, 1.0).asin() + elevation).clamp(-limit, limit);
    let dir = Vec3::new(el.cos() * az.cos(), el.cos() * az.sin(), el.sin());
    let (old, new) = (Mat4::view(d, Vec3::ZERO, 0.0), Mat4::view(dir, Vec3::ZERO, 0.0));
    let v = st.view();
    // The centre of the view on the plane through the origin facing the viewer: the view is
    // orthonormal, so its transpose takes it back to the WCS.
    let m = &old.m;
    let pivot = Vec3::new(
        m[0][0] * v.center.x + m[1][0] * v.center.y,
        m[0][1] * v.center.x + m[1][1] * v.center.y,
        m[0][2] * v.center.x + m[1][2] * v.center.y,
    );
    let c = new.apply(pivot);
    st.set_view_dir(dir);
    st.set_view_quiet(View { center: Vec2::new(c.x, c.y), height: v.height });
    s.touch();
    Ok(())
}

/// Typed VPOINT: a direction `x,y,z` or a preset keyword.
struct VpointM;

impl Interactive for VpointM {
    fn name(&self) -> &'static str {
        "VPOINT"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        Prompt::new("Specify a view direction x,y,z or", Accept::TEXT).kw(&PRESET_KEYWORDS)
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        let dir = match i {
            Input::Keyword(k) => preset(&k),
            Input::Text(t) => match t.split(',').map(|c| c.trim().parse::<f64>().ok()).collect::<Option<Vec<f64>>>().as_deref() {
                Some([x, y, z]) => Some(Vec3::new(*x, *y, *z)),
                _ => None,
            },
            Input::Enter | Input::Cancel => return Ok(Step::Done),
            _ => None,
        };
        let dir = dir.ok_or_else(|| crate::EngineError::Other("Requires x,y,z or an option keyword.".into()))?;
        set_direction(s, "vpoint", dir)?;
        Ok(Step::Done)
    }
}

/// The prompt of 3DORBIT.
const ORBIT_PROMPT: &str = "Drag to orbit, Enter or Esc to end";

struct OrbitM;

impl Interactive for OrbitM {
    fn name(&self) -> &'static str {
        "3DORBIT"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        Prompt::new(ORBIT_PROMPT, Accept { enter: true, ..Accept::default() })
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        if !s.can_view_3d() {
            return Err(bad("3dorbit", NEEDS_SINGLE));
        }
        Ok(Step::Continue)
    }
    fn input(&mut self, _s: &mut Session, i: Input) -> Result<Step> {
        Ok(match i {
            Input::Enter | Input::Cancel => Step::Done,
            _ => Step::Continue,
        })
    }
}

impl Session {
    /// Model space on the Model tab in a single viewport: where a 3D view can be shown (tiled
    /// viewports and layouts are drawn in plan).
    pub fn can_view_3d(&self) -> bool {
        self.layout_space() == Space::Model && self.doc().is_ok_and(|d| !tiled(d))
    }
    /// 3DORBIT is running: the UI turns drags into [`Session::drag_view`].
    pub fn orbit_active(&self) -> bool {
        self.running.as_ref().is_some_and(|r| r.id == "3dorbit")
    }
    /// Orbit model space by a drag of (`dx`, `dy`) pixels, as 3DORBIT and dragging the ViewCube
    /// do: across the whole drawing area turns the view half a turn about the Z axis, up and down
    /// tilts it.
    pub fn drag_view(&mut self, dx: f64, dy: f64) -> Result<()> {
        if !(dx.is_finite() && dy.is_finite()) || !self.can_view_3d() {
            return Ok(());
        }
        let k = std::f64::consts::PI / self.viewport_px.0.max(1.0);
        turn(self, -dx * k, dy * k)
    }
    /// Turn a 3D view by `angle` (radians, counterclockwise) about the Z axis, as the ViewCube's
    /// compass ring does. A plan view, or one straight up the Z axis, has no azimuth to turn.
    pub fn rotate_view(&mut self, angle: f64) -> Result<()> {
        let st = self.state()?;
        if !angle.is_finite() || st.projection().is_none() || st.view_dir.z.abs() > 1.0 - 1e-12 {
            return Ok(());
        }
        turn(self, angle, 0.0)
    }
    /// The name of the model view: a preset's label, "Top" in plan view, "Custom" otherwise.
    pub fn view_name(&self) -> &'static str {
        let Ok(st) = self.state() else { return "Top" };
        if st.projection().is_none() {
            return "Top";
        }
        let d = st.view_dir;
        PRESETS
            .iter()
            .find(|(_, _, p)| {
                let p = Vec3::new(p[0], p[1], p[2]);
                (d - p * (1.0 / p.len())).len() < 1e-6
            })
            .map_or("Custom", |(_, label, _)| *label)
    }
}
