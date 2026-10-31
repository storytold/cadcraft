//! UCS an UCSICON: the current user coordinate system ([`crate::ucs`]) and its icon.
//!
//! UCS sets the current UCS of the space commands act on: by an origin and points on the new X
//! axis and XY plane (the default option), World, Previous, NAmed (Restore, Save, Delete, `?`),
//! OBject (lined up with an object), View and Z (a turn about Z). X, Y, ZAxis and Face tilt the
//! UCS out of the XY plane, which CADCraft doesn't draw in yet; they say so. A UCS change is part
//! of the command's undo step, as an edit is.

use cadcraft_doc::{Drawing, EntityKind};
use cadcraft_geom::{Vec2, Vec3};
use serde_json::{Value, json};

use super::{CommandSpec, bad, str_param};
use crate::prompt::{Accept, Input, Interactive, Prompt, Step};
use crate::ucs::{UcsFrame, named};
use crate::{EngineError, Result, Session};

/// The UCS command's options, as typed.
const OPTIONS: &[&str] = &["Face", "NAmed", "OBject", "Previous", "View", "World", "X", "Y", "Z", "ZAxis"];

/// Characters a UCS name can't hold.
const BAD_NAME_CHARS: &[char] = &['<', '>', '/', '\\', '"', ':', ';', '?', '*', '|', ',', '=', '`'];

/// Longest UCS name.
const MAX_NAME: usize = 255;

/// Most UCS table records a drawing keeps (a hostile script can't grow it without bound).
const MAX_NAMED: usize = 10_000;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("ucs", "UCS", run_ucs)
            .params(
                "{option?: origin|world|previous|named|object|view|z|x|y|zaxis|face (default origin), origin?: [x,y(,z)], xPoint?, xyPoint? \
                 (origin: the new origin, and points on its positive X axis and on the positive-Y side of its XY plane; without them \
                 the axes stay), angle? (z: degrees counterclockwise about Z, default UCSAXISANG), action?: restore|save|delete|list \
                 (named), name? (named; delete and list take wildcards and commas), replace?: bool (save over an existing name), \
                 handle?, at? (object: the object and the point it is picked at)} → {ucs: {name, origin, xAxis, yAxis}}. Points \
                 are world coordinates. x, y, zaxis and face tilt the UCS out of the XY plane and are not available yet.",
            )
            .interactive(|_| Ok(Box::new(UcsM::default()))),
        CommandSpec::new("ucs.world", "World", |s, p| run_ucs(s, &with_option(p, "world")))
            .menu(&["Tools", "New UCS", "World"])
            .params("{} (the World UCS)"),
        CommandSpec::new("ucs.origin", "Origin", |s, p| run_ucs(s, &with_option(p, "origin")))
            .menu(&["Tools", "New UCS", "Origin"])
            .params("{origin: [x,y(,z)]} (moves the UCS, keeping its axes)")
            .interactive(|_| Ok(Box::new(UcsM::starting(Stage::OriginOnly)))),
        CommandSpec::new("ucs.3point", "3 Point", |s, p| run_ucs(s, &with_option(p, "origin")))
            .menu(&["Tools", "New UCS", "3 Point"])
            .params("{origin, xPoint?, xyPoint?} (as `ucs` origin)")
            .interactive(|_| Ok(Box::new(UcsM::starting(Stage::ThreePoint)))),
        CommandSpec::new("ucs.view", "View", |s, p| run_ucs(s, &with_option(p, "view")))
            .menu(&["Tools", "New UCS", "View"])
            .params("{} (XY plane square to the view, origin kept)"),
        CommandSpec::new("ucs.z", "Z", |s, p| run_ucs(s, &with_option(p, "z")))
            .menu(&["Tools", "New UCS", "Z"])
            .params("{angle?} (degrees counterclockwise about Z, default UCSAXISANG)")
            .interactive(|_| Ok(Box::new(UcsM::starting(Stage::ZAngle)))),
        CommandSpec::new("ucsicon", "UCS Icon", run_ucsicon)
            .params("{on?: bool, origin?: bool} (UCSICON: the icon shown, and at the UCS origin when that is on screen) → {on, origin}")
            // Undoable like SETVAR: UCSICON lives in the drawing, so without a step of its own an
            // undo of the edit before it would put the old value back.
            .interactive(|_| Ok(Box::new(UcsIconM))),
    ]
}

fn with_option(p: &Value, option: &str) -> Value {
    let mut p = if p.is_object() { p.clone() } else { json!({}) };
    if let Some(o) = p.as_object_mut() {
        o.insert("option".into(), json!(option));
    }
    p
}

/// A JSON point with an optional Z: `[x, y]`, `[x, y, z]` or `{x, y, z?}`.
fn point3(v: &Value) -> Option<Vec3> {
    let p = if let Some(a) = v.as_array() {
        Vec3::new(a.first()?.as_f64()?, a.get(1)?.as_f64()?, a.get(2).and_then(Value::as_f64).unwrap_or(0.0))
    } else {
        Vec3::new(v.get("x")?.as_f64()?, v.get("y")?.as_f64()?, v.get("z").and_then(Value::as_f64).unwrap_or(0.0))
    };
    p.is_finite().then_some(p)
}

fn point3_param(cmd: &str, p: &Value, key: &str) -> Result<Option<Vec3>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => point3(v).map(Some).ok_or_else(|| bad(cmd, format!("`{key}` must be a finite point [x, y] or [x, y, z]"))),
    }
}

/// `UCS` as JSON: the result is the new current UCS.
fn run_ucs(s: &mut Session, p: &Value) -> Result<Value> {
    let option = str_param(p, "option").unwrap_or("origin").trim().to_ascii_lowercase();
    match option.as_str() {
        "origin" | "3point" => {
            let origin = point3_param("ucs", p, "origin")?.ok_or_else(|| bad("ucs", "`origin` ([x, y] or [x, y, z]) is required"))?;
            let x = point3_param("ucs", p, "xPoint")?;
            let xy = point3_param("ucs", p, "xyPoint")?;
            if xy.is_some() && x.is_none() {
                return Err(bad("ucs", "`xyPoint` needs `xPoint`"));
            }
            set_by_points(s, origin, x, xy)?;
        }
        "world" | "w" => s.set_ucs(UcsFrame::WORLD, "")?,
        "previous" | "p" => {
            if !s.ucs_previous()? {
                return Err(bad("ucs", "there is no previous UCS"));
            }
        }
        "view" | "v" => s.set_ucs(view_ucs(s), "")?,
        "z" => {
            let deg = match p.get("angle") {
                None | Some(Value::Null) => axis_angle(s),
                Some(v) => v.as_f64().filter(|a| a.is_finite()).ok_or_else(|| bad("ucs", "`angle` must be a finite number of degrees"))?,
            };
            rotate_z(s, deg.to_radians())?;
        }
        "object" | "ob" => {
            let h = super::targets(s, p)?.first().copied().ok_or_else(|| bad("ucs", "`handle` (the object) is required"))?;
            let at = p.get("at").and_then(super::point_value);
            set_by_object(s, h, at)?;
        }
        "named" | "na" => {
            let action = str_param(p, "action").unwrap_or("restore").trim().to_ascii_lowercase();
            let name = str_param(p, "name").map(str::trim).unwrap_or_default().to_string();
            match action.as_str() {
                "restore" | "r" => restore(s, &name).map_err(|e| bad("ucs", e))?,
                "save" | "s" => save(s, &name, p.get("replace").and_then(Value::as_bool).unwrap_or(false)).map_err(|e| bad("ucs", e))?,
                "delete" | "d" => {
                    let n = delete(s, if name.is_empty() { "" } else { &name })?;
                    return Ok(json!({ "deleted": n, "ucs": ucs_json(s) }));
                }
                "list" | "?" => return Ok(json!({ "list": list(s, if name.is_empty() { "*" } else { &name }), "ucs": ucs_json(s) })),
                other => return Err(bad("ucs", format!("unknown named action `{other}` (restore, save, delete or list)"))),
            }
        }
        "x" | "y" | "zaxis" | "za" | "face" | "f" => return Err(bad("ucs", tilted_message(&option))),
        other => return Err(bad("ucs", format!("unknown option `{other}`"))),
    }
    Ok(json!({ "ucs": ucs_json(s) }))
}

fn ucs_json(s: &Session) -> Value {
    let (u, name) = s.ucs_named();
    let p = |v: Vec3| json!([v.x, v.y, v.z]);
    json!({ "name": name, "origin": p(u.origin), "xAxis": p(u.x), "yAxis": p(u.y), "world": u.is_world() })
}

fn tilted_message(option: &str) -> String {
    let label = match option {
        "x" => "UCS X",
        "y" => "UCS Y",
        "zaxis" | "za" => "UCS ZAxis",
        _ => "UCS Face",
    };
    format!("{label} tilts the UCS out of the XY plane, which is not available yet.")
}

/// The UCSAXISANG default for Z, in degrees.
fn axis_angle(s: &Session) -> f64 {
    s.doc().map(|d| d.header.i64("UCSAXISANG", 90)).unwrap_or(90) as f64
}

/// A 2D point of the current UCS's XY plane as a 3D world point.
fn on_plane(s: &Session, p: Vec2) -> Vec3 {
    p.to3(s.ucs().origin.z)
}

/// The UCS through `origin`, with points on its X axis and in its XY plane (world points).
fn set_by_points(s: &mut Session, origin: Vec3, x: Option<Vec3>, xy: Option<Vec3>) -> Result<()> {
    let f = s.ucs().from_points(origin, x, xy).ok_or_else(|| EngineError::Other("Those points don't define a UCS: they lie on one line.".into()))?;
    if !f.is_planar() {
        return Err(EngineError::Other("That UCS is tilted out of the XY plane (or turned over), which is not available yet.".into()));
    }
    s.set_ucs(f, "")
}

/// UCS View: the XY plane square to the view (a plan view, so the World UCS's axes), at the
/// current origin.
fn view_ucs(s: &Session) -> UcsFrame {
    UcsFrame::WORLD.with_origin(s.ucs().origin)
}

fn rotate_z(s: &mut Session, angle: f64) -> Result<()> {
    let f = s.ucs().rotated_z(angle);
    s.set_ucs(f, "")
}

/// UCS OBject: the UCS lined up with object `h`, picked at `at` (world) when known.
fn set_by_object(s: &mut Session, h: cadcraft_doc::Handle, at: Option<Vec2>) -> Result<()> {
    let d = s.doc()?;
    let e = d.entity(h).ok_or_else(|| EngineError::Other("No such object.".into()))?;
    let ext = e.common.extrusion;
    if ext.is_finite() && (ext.x.abs() > 1e-9 || ext.y.abs() > 1e-9 || ext.z < 0.0) {
        return Err(EngineError::Other("That object lies in a tilted plane; a UCS out of the XY plane is not available yet.".into()));
    }
    let f = object_frame(&s.ucs(), &e.kind, at)?;
    s.set_ucs(f, "")
}

/// A frame at `o` whose X axis points along `dir` in the XY plane.
fn along(o: Vec3, dir: Vec2) -> Result<UcsFrame> {
    UcsFrame::new(o, dir.to3(0.0), dir.perp().to3(0.0))
        .ok_or_else(|| EngineError::Other("That object has no direction to line the UCS up with.".into()))
}

/// The UCS OBject rules: where each kind of object puts the origin and the X axis.
fn object_frame(cur: &UcsFrame, kind: &EntityKind, at: Option<Vec2>) -> Result<UcsFrame> {
    let near = |a: Vec2, b: Vec2| at.is_none_or(|p| p.dist(a) <= p.dist(b));
    match kind {
        // The endpoint nearer the pick; X along the line.
        EntityKind::Line(l) => {
            let (o, other) = if near(l.a.xy(), l.b.xy()) { (l.a, l.b) } else { (l.b, l.a) };
            along(o, other.xy() - o.xy())
        }
        // The centre; X through the endpoint nearer the pick.
        EntityKind::Arc(a) => {
            let (e0, e1) = (Vec2::polar(a.center.xy(), a.radius, a.start), Vec2::polar(a.center.xy(), a.radius, a.end));
            let e = if near(e0, e1) { e0 } else { e1 };
            along(a.center, e - a.center.xy())
        }
        // The centre; X through the pick.
        EntityKind::Circle(c) => {
            let d = at.map(|p| p - c.center.xy()).filter(|d| d.len() > 1e-12).unwrap_or(cur.x.xy());
            along(c.center, d)
        }
        // The first vertex; X toward the next one.
        EntityKind::LwPolyline(p) => {
            let first = p.vertices.first().map(|v| v.p).ok_or_else(|| EngineError::Other("That polyline has no vertices.".into()))?;
            let next = p.vertices.iter().map(|v| v.p).find(|q| q.dist(first) > 1e-12).ok_or_else(|| EngineError::Other("That polyline has no length.".into()))?;
            along(first.to3(p.elevation), next - first)
        }
        // The point; the X axis stays as it is.
        EntityKind::Point(p) => along(p.p, cur.x.xy()),
        // The insertion point; X along the object's rotation.
        EntityKind::Text(t) => along(t.insert, Vec2::from_angle(t.rotation)),
        EntityKind::AttDef(a) => along(a.text.insert, Vec2::from_angle(a.text.rotation)),
        EntityKind::Insert(i) => along(i.insert, Vec2::from_angle(i.rotation)),
        // The first corner; X toward the second.
        EntityKind::Solid(so) | EntityKind::Trace(so) => {
            let [a, b, ..] = so.corners;
            along(a, b.xy() - a.xy())
        }
        // The first corner; X toward the second, Y toward the fourth.
        EntityKind::Face3d(f) => {
            let [a, b, _, d] = f.corners;
            let frame = UcsFrame::new(a, b - a, d - a).ok_or_else(|| EngineError::Other("That face has no area to line the UCS up with.".into()))?;
            if frame.is_planar() {
                Ok(frame)
            } else {
                Err(EngineError::Other("That face lies out of the XY plane; a UCS out of it is not available yet.".into()))
            }
        }
        // The middle of the text; the X axis stays as it is.
        EntityKind::Dimension(dm) => along(dm.text_mid, cur.x.xy()),
        _ => Err(EngineError::Other("The UCS can't be lined up with that kind of object (lines, arcs, circles, polylines, points, text, blocks, 2D solids, 3D faces and dimensions can).".into())),
    }
}

/// A usable UCS name, or why not.
fn check_name(name: &str) -> std::result::Result<String, String> {
    let n = name.trim();
    if n.is_empty() {
        return Err("A UCS name is required.".into());
    }
    if n.chars().count() > MAX_NAME {
        return Err(format!("A UCS name has at most {MAX_NAME} characters."));
    }
    if n.contains(BAD_NAME_CHARS) || n.chars().any(char::is_control) {
        return Err(format!("\"{n}\" is not a valid UCS name."));
    }
    if n.eq_ignore_ascii_case("*WORLD*") || n.eq_ignore_ascii_case("WORLD") {
        return Err("\"World\" is the World UCS; save under another name.".into());
    }
    Ok(n.to_string())
}

fn restore(s: &mut Session, name: &str) -> std::result::Result<(), String> {
    let d = s.doc().map_err(|e| e.to_string())?;
    let (f, n) = named(d, name).ok_or_else(|| format!("Cannot find UCS \"{}\".", name.trim()))?;
    if !f.is_planar() {
        // Kept and saved as read, but not current until tilted UCSs are available.
        return Err(format!("UCS \"{n}\" is tilted out of the XY plane, which is not available yet."));
    }
    s.set_ucs(f, &n).map_err(|e| e.to_string())
}

fn exists(d: &Drawing, name: &str) -> bool {
    d.ucss.iter().any(|u| u.name.eq_ignore_ascii_case(name))
}

/// Save the current UCS as `name` (replacing a UCS of that name only with `replace`); it is then
/// the current UCS's name.
fn save(s: &mut Session, name: &str, replace: bool) -> std::result::Result<(), String> {
    let n = check_name(name)?;
    let frame = s.ucs();
    let d = s.doc_mut().map_err(|e| e.to_string())?;
    match d.ucss.iter_mut().find(|u| u.name.eq_ignore_ascii_case(&n)) {
        Some(_) if !replace => return Err(format!("UCS \"{n}\" already exists.")),
        Some(u) => *u = frame.record(&u.name.clone()),
        None => {
            if d.ucss.len() >= MAX_NAMED {
                return Err(format!("A drawing keeps at most {MAX_NAMED} named UCSs."));
            }
            d.ucss.push(frame.record(&n));
        }
    }
    let kept = d.ucss.iter().find(|u| u.name.eq_ignore_ascii_case(&n)).map(|u| u.name.clone()).unwrap_or(n);
    s.set_ucs(frame, &kept).map_err(|e| e.to_string())
}

/// Whether `name` matches one of the comma-separated wildcard patterns.
fn matches(patterns: &str, name: &str) -> bool {
    patterns
        .split(',')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .any(|p| super::qselect::wildcard(&p.to_ascii_uppercase(), &name.to_ascii_uppercase()))
}

/// Delete the named UCSs matching `patterns`; the current UCS keeps its axes but loses a deleted
/// name. Returns how many went.
fn delete(s: &mut Session, patterns: &str) -> Result<usize> {
    let gone: Vec<String> = s.doc()?.ucss.iter().filter(|u| matches(patterns, &u.name)).map(|u| u.name.clone()).collect();
    if gone.is_empty() {
        return Ok(0);
    }
    let (frame, current) = s.ucs_named();
    s.doc_mut()?.ucss.retain(|u| !gone.iter().any(|g| g == &u.name));
    if gone.iter().any(|g| g.eq_ignore_ascii_case(&current)) {
        s.set_ucs(frame, "")?;
    }
    Ok(gone.len())
}

/// How the current UCS is named in messages.
fn shown_name(s: &Session) -> String {
    let (u, name) = s.ucs_named();
    if !name.is_empty() {
        name
    } else if u.is_world() {
        "*WORLD*".into()
    } else {
        "*NO NAME*".into()
    }
}

/// `?`: the named UCSs matching `patterns`, their origins and axes relative to the current UCS.
fn list(s: &Session, patterns: &str) -> String {
    let Ok(d) = s.doc() else { return String::new() };
    let cur = s.input_ucs();
    let (lu, lp) = (d.header.i64("LUNITS", 2), d.header.i64("LUPREC", 4));
    let f = |v: f64| crate::units::format_distance(v, lu, lp);
    let pt = |v: Vec3| format!("<{},{},{}>", f(v.x), f(v.y), f(v.z));
    let axis = |o: Vec3, a: Vec3| cur.to_ucs3(o + a) - cur.to_ucs3(o);
    let mut out = format!("Current ucs name:  {}\nSaved coordinate systems:", shown_name(s));
    let mut any = false;
    for u in d.ucss.iter().filter(|u| matches(patterns, &u.name)) {
        any = true;
        let Some(fr) = UcsFrame::of(u) else { continue };
        out.push_str(&format!(
            "\n\n{}\n    Origin = {}, X Axis = {}\n    Y Axis = {}, Z Axis = {}",
            u.name,
            pt(cur.to_ucs3(fr.origin)),
            pt(axis(fr.origin, fr.x)),
            pt(axis(fr.origin, fr.y)),
            pt(axis(fr.origin, fr.z()))
        ));
    }
    if !any {
        out.push_str("\n\n(none)");
    }
    out
}

/// Where the interactive UCS is.
#[derive(Clone, Debug, Default, PartialEq)]
enum Stage {
    #[default]
    Main,
    /// Tools > New UCS > Origin: only the origin.
    OriginOnly,
    /// Tools > New UCS > 3 Point: the origin first, then the axes.
    ThreePoint,
    XPoint(Vec3),
    XyPoint(Vec3, Vec3),
    Named,
    Restore,
    Save,
    Replace(String),
    Delete,
    List,
    Object,
    ZAngle,
}

#[derive(Default)]
struct UcsM {
    stage: Stage,
}

impl UcsM {
    fn starting(stage: Stage) -> Self {
        UcsM { stage }
    }
}

fn other(msg: impl Into<String>) -> EngineError {
    EngineError::Other(msg.into())
}

impl Interactive for UcsM {
    fn name(&self) -> &'static str {
        "UCS"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        s.echo(format!("Current ucs name:  {}", shown_name(s)));
        Ok(Step::Continue)
    }
    fn prompt(&self, s: &Session) -> Prompt {
        match &self.stage {
            Stage::Main => Prompt::new("Specify origin of UCS", Accept::POINT).kw(OPTIONS).default("World"),
            Stage::OriginOnly | Stage::ThreePoint => {
                let o = s.ucs().origin;
                Prompt::new("Specify new origin point", Accept::POINT).default(format!("{},{},{}", o.x, o.y, o.z))
            }
            Stage::XPoint(o) => Prompt::new("Specify point on X-axis", Accept::POINT).default("Accept").base(o.xy()),
            Stage::XyPoint(o, _) => Prompt::new("Specify point on the XY plane", Accept::POINT).default("Accept").base(o.xy()),
            Stage::Named => Prompt::new("Enter an option", Accept::TEXT).kw(&["Restore", "Save", "Delete", "?"]),
            Stage::Restore => Prompt::new("Enter name of UCS to restore", Accept::TEXT).kw(&["?"]),
            Stage::Save => Prompt::new("Enter name to save current UCS", Accept::TEXT).kw(&["?"]),
            Stage::Replace(n) => Prompt::new(format!("UCS \"{n}\" already exists. Replace it?"), Accept::TEXT).kw(&["Yes", "No"]).default("No"),
            Stage::Delete => Prompt::new("Enter UCS name(s) to delete", Accept::TEXT).default("none"),
            Stage::List => Prompt::new("Enter UCS name(s) to list", Accept::TEXT).default("*"),
            Stage::Object => Prompt::new("Select object to align UCS", Accept::POINT),
            Stage::ZAngle => Prompt::new("Specify rotation angle about Z axis", Accept::NUMBER).default(format!("{}", axis_angle(s))),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if i == Input::Cancel {
            return Ok(Step::Cancel);
        }
        match (self.stage.clone(), i) {
            (Stage::Main, Input::Enter) => {
                s.set_ucs(UcsFrame::WORLD, "")?;
                Ok(Step::Done)
            }
            (Stage::Main, Input::Keyword(k)) => match k.as_str() {
                "World" => {
                    s.set_ucs(UcsFrame::WORLD, "")?;
                    Ok(Step::Done)
                }
                "Previous" => {
                    if !s.ucs_previous()? {
                        s.echo("There is no previous UCS.");
                    }
                    Ok(Step::Done)
                }
                "View" => {
                    let f = view_ucs(s);
                    s.set_ucs(f, "")?;
                    Ok(Step::Done)
                }
                "NAmed" => {
                    self.stage = Stage::Named;
                    Ok(Step::Continue)
                }
                "OBject" => {
                    self.stage = Stage::Object;
                    Ok(Step::Continue)
                }
                "Z" => {
                    self.stage = Stage::ZAngle;
                    Ok(Step::Continue)
                }
                "X" | "Y" | "ZAxis" | "Face" => {
                    s.echo(tilted_message(&k.to_ascii_lowercase()));
                    Ok(Step::Continue)
                }
                _ => Ok(Step::Continue),
            },
            (Stage::Main | Stage::ThreePoint, Input::Point(p)) => {
                self.stage = Stage::XPoint(on_plane(s, p));
                Ok(Step::Continue)
            }
            (Stage::OriginOnly, Input::Point(p)) => {
                let f = s.ucs().with_origin(on_plane(s, p));
                s.set_ucs(f, "")?;
                Ok(Step::Done)
            }
            (Stage::OriginOnly | Stage::ThreePoint, Input::Enter) => Ok(Step::Done),
            (Stage::XPoint(o), Input::Enter) => {
                set_by_points(s, o, None, None)?;
                Ok(Step::Done)
            }
            (Stage::XPoint(o), Input::Point(p)) => {
                let x = on_plane(s, p);
                if x.xy().dist(o.xy()) <= 1e-12 {
                    return Err(other("The point on the X axis must differ from the origin."));
                }
                self.stage = Stage::XyPoint(o, x);
                Ok(Step::Continue)
            }
            (Stage::XyPoint(o, x), Input::Enter) => {
                set_by_points(s, o, Some(x), None)?;
                Ok(Step::Done)
            }
            (Stage::XyPoint(o, x), Input::Point(p)) => {
                set_by_points(s, o, Some(x), Some(on_plane(s, p)))?;
                Ok(Step::Done)
            }
            (Stage::Named, Input::Keyword(k)) => {
                self.stage = match k.as_str() {
                    "Restore" => Stage::Restore,
                    "Save" => Stage::Save,
                    "Delete" => Stage::Delete,
                    _ => Stage::List,
                };
                Ok(Step::Continue)
            }
            (Stage::Named, Input::Enter) => Ok(Step::Done),
            (Stage::Restore | Stage::Save, Input::Keyword(_)) => {
                let l = list(s, "*");
                s.echo(l);
                Ok(Step::Continue)
            }
            (Stage::Restore, Input::Text(t)) => {
                restore(s, &t).map_err(other)?;
                Ok(Step::Done)
            }
            (Stage::Save, Input::Text(t)) => {
                let n = check_name(&t).map_err(other)?;
                if s.doc().is_ok_and(|d| exists(d, &n)) {
                    self.stage = Stage::Replace(n);
                    return Ok(Step::Continue);
                }
                save(s, &n, false).map_err(other)?;
                Ok(Step::Done)
            }
            (Stage::Replace(n), Input::Keyword(k)) => {
                if k == "Yes" {
                    save(s, &n, true).map_err(other)?;
                }
                Ok(Step::Done)
            }
            (Stage::Replace(_), Input::Enter) | (Stage::Restore | Stage::Save | Stage::Delete, Input::Enter) => Ok(Step::Done),
            (Stage::Delete, Input::Text(t)) => {
                let n = delete(s, &t)?;
                s.echo(format!("{n} UCS name(s) deleted."));
                Ok(Step::Done)
            }
            (Stage::List, Input::Enter) => {
                let l = list(s, "*");
                s.echo(l);
                Ok(Step::Done)
            }
            (Stage::List, Input::Text(t)) => {
                let l = list(s, &t);
                s.echo(l);
                Ok(Step::Done)
            }
            (Stage::Object, Input::Point(p)) => {
                let h = super::curves::pick_at(s, p).ok_or_else(|| other("No object found there; select an object."))?;
                set_by_object(s, h, Some(p))?;
                Ok(Step::Done)
            }
            (Stage::Object, Input::Enter) => Ok(Step::Done),
            (Stage::ZAngle, Input::Enter) => {
                let a = axis_angle(s).to_radians();
                rotate_z(s, a)?;
                Ok(Step::Done)
            }
            (Stage::ZAngle, Input::Text(t)) => {
                let a = s.angle_settings().amount(&t).filter(|a| a.is_finite()).ok_or_else(|| other("Requires a valid angle."))?;
                rotate_z(s, a)?;
                Ok(Step::Done)
            }
            (Stage::Main, Input::Text(_)) => Err(other("Requires a point or an option keyword.")),
            _ => Err(other("Invalid input.")),
        }
    }
}

fn set_ucsicon(s: &mut Session, on: Option<bool>, origin: Option<bool>) -> Result<Value> {
    let mut bits = s.ucsicon_bits();
    if let Some(on) = on {
        bits = if on { bits | 1 } else { bits & !1 };
    }
    if let Some(o) = origin {
        bits = if o { bits | 2 } else { bits & !2 };
    }
    s.doc_mut()?.header.set_i64("UCSICON", bits);
    Ok(json!({ "on": bits & 1 != 0, "origin": bits & 2 != 0 }))
}

fn run_ucsicon(s: &mut Session, p: &Value) -> Result<Value> {
    let flag = |k: &str| -> Result<Option<bool>> {
        match p.get(k) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::Bool(b)) => Ok(Some(*b)),
            Some(v) => Err(bad("ucsicon", format!("`{k}` must be true or false, got {v}"))),
        }
    };
    let (on, origin) = (flag("on")?, flag("origin")?);
    set_ucsicon(s, on, origin)
}

struct UcsIconM;

impl Interactive for UcsIconM {
    fn name(&self) -> &'static str {
        "UCSICON"
    }
    fn prompt(&self, s: &Session) -> Prompt {
        let on = s.ucsicon_bits() & 1 != 0;
        Prompt::new("Enter an option", Accept::TEXT).kw(&["ON", "OFF", "All", "Noorigin", "ORigin", "Selectable", "Properties"]).default(if on {
            "ON"
        } else {
            "OFF"
        })
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match i {
            Input::Keyword(k) => {
                match k.as_str() {
                    "ON" => set_ucsicon(s, Some(true), None).map(|_| ())?,
                    "OFF" => set_ucsicon(s, Some(false), None).map(|_| ())?,
                    "Noorigin" => set_ucsicon(s, None, Some(false)).map(|_| ())?,
                    "ORigin" => set_ucsicon(s, None, Some(true)).map(|_| ())?,
                    other => {
                        s.echo(format!("UCSICON {other} is not available yet."));
                        return Ok(Step::Continue);
                    }
                }
                Ok(Step::Done)
            }
            Input::Enter | Input::Cancel => Ok(Step::Done),
            _ => Err(other("Requires an option keyword.")),
        }
    }
}
