//! UCS frame selection and named-frame commands.

use cadcraft_geom::Vec3;
use serde_json::{Value, json};

use super::{CommandSpec, always, bad};
use crate::{Accept, Input, Interactive, Prompt, Result, Session, Step, ucs::Frame};

pub fn specs() -> Vec<CommandSpec> {
    vec![CommandSpec::new("ucs", "User Coordinate System", run)
        .menu(&["View", "Coordinates", "User Coordinate System"])
        .alias(&["ucs3d"])
        .params("{action: world|previous|set|save|named, name?, origin?, xAxis?, yAxis?} → {name, origin, xAxis, yAxis}: select or define a right-handed UCS; typed XYZ in 3D prompts is expressed in the active UCS")
        .enabled(always)
        .interactive(|_| Ok(Box::new(UcsM::default())))
        .noundo()]
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Ask {
    Action,
    Origin,
    XAxis,
    YAxis,
    Name,
}

#[derive(Default)]
struct UcsM {
    ask: Option<Ask>,
    action: String,
    points: Vec<Vec3>,
}

impl UcsM {
    fn run(&self, s: &mut Session) -> Result<Step> {
        let params = if self.action == "set" {
            let [origin, x, y] = self.points.as_slice() else { return Err(bad("ucs", "three points are required")) };
            json!({"action":"set", "origin":[origin.x,origin.y,origin.z], "xAxis":[x.x-origin.x,x.y-origin.y,x.z-origin.z], "yAxis":[y.x-origin.x,y.y-origin.y,y.z-origin.z]})
        } else {
            json!({"action": self.action})
        };
        let result = run(s, &params)?;
        if let Some(message) = result.get("message").and_then(Value::as_str) {
            s.echo(message);
        }
        Ok(Step::Done)
    }
}

impl Interactive for UcsM {
    fn name(&self) -> &'static str {
        "UCS"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match self.ask.unwrap_or(Ask::Action) {
            Ask::Action => Prompt::new("Specify UCS option", Accept::TEXT).kw(&["World", "Previous", "Origin", "3point", "Named", "Save"]),
            Ask::Origin => Prompt::new("Specify new origin", Accept::POINT).point3d(),
            Ask::XAxis => Prompt::new("Specify point on positive X axis", Accept::POINT).point3d(),
            Ask::YAxis => Prompt::new("Specify point on positive Y side of the XY plane", Accept::POINT).point3d(),
            Ask::Name => Prompt::new(if self.action == "save" { "Name for saved UCS" } else { "Name of UCS to restore" }, Accept::TEXT),
        }
    }
    fn input(&mut self, s: &mut Session, input: Input) -> Result<Step> {
        match (self.ask.unwrap_or(Ask::Action), input) {
            (_, Input::Cancel) => Ok(Step::Cancel),
            (Ask::Action, Input::Keyword(k) | Input::Text(k)) => {
                self.action = match k.trim().to_ascii_lowercase().as_str() {
                    "world" | "w" => "world".into(),
                    "previous" | "p" => "previous".into(),
                    "origin" | "o" => "origin".into(),
                    "3point" | "3" => "set".into(),
                    "named" | "n" => "named".into(),
                    "save" | "s" => "save".into(),
                    _ => return Err(bad("ucs", "choose World, Previous, Origin, 3point, Named or Save")),
                };
                match self.action.as_str() {
                    "world" | "previous" => self.run(s),
                    "origin" => {
                        self.ask = Some(Ask::Origin);
                        Ok(Step::Continue)
                    }
                    "set" => {
                        self.ask = Some(Ask::Origin);
                        Ok(Step::Continue)
                    }
                    _ => {
                        self.ask = Some(Ask::Name);
                        Ok(Step::Continue)
                    }
                }
            }
            (Ask::Origin, Input::Point3(p)) => {
                if self.action == "origin" {
                    let current = s.active_ucs();
                    let next = Frame::from_axes(p, current.x, current.y).ok_or_else(|| bad("ucs", "the new origin is invalid"))?;
                    s.set_active_ucs(next)?;
                    Ok(Step::Done)
                } else {
                    self.points.push(p);
                    self.ask = Some(Ask::XAxis);
                    Ok(Step::Continue)
                }
            }
            (Ask::Origin, Input::Point(p)) => self.input(s, Input::Point3(Vec3::new(p.x, p.y, 0.0))),
            (Ask::XAxis, Input::Point3(p)) => {
                self.points.push(p);
                self.ask = Some(Ask::YAxis);
                Ok(Step::Continue)
            }
            (Ask::XAxis, Input::Point(p)) => self.input(s, Input::Point3(Vec3::new(p.x, p.y, 0.0))),
            (Ask::YAxis, Input::Point3(p)) => {
                self.points.push(p);
                self.run(s)
            }
            (Ask::YAxis, Input::Point(p)) => self.input(s, Input::Point3(Vec3::new(p.x, p.y, 0.0))),
            (Ask::Name, Input::Text(name)) => {
                let mut p = json!({"action":self.action, "name":name.trim()});
                if self.action == "save" {
                    p["action"] = json!("save");
                }
                run(s, &p)?;
                Ok(Step::Done)
            }
            _ => Err(bad("ucs", "a point or valid UCS option is required")),
        }
    }
}

fn vec3(value: &Value) -> Option<Vec3> {
    let a = value.as_array()?;
    if a.len() != 3 {
        return None;
    }
    let [Some(x), Some(y), Some(z)] = [a[0].as_f64(), a[1].as_f64(), a[2].as_f64()] else { return None };
    let p = Vec3::new(x, y, z);
    p.is_finite().then_some(p)
}

fn report(frame: Frame, name: &str) -> Value {
    json!({ "name": name, "origin": [frame.origin.x, frame.origin.y, frame.origin.z],
        "xAxis": [frame.x.x, frame.x.y, frame.x.z], "yAxis": [frame.y.x, frame.y.y, frame.y.z],
        "zAxis": [frame.z.x, frame.z.y, frame.z.z] })
}

fn run(s: &mut Session, p: &Value) -> Result<Value> {
    let action = p.get("action").and_then(Value::as_str).unwrap_or("set").trim().to_ascii_lowercase();
    let (frame, name) = match action.as_str() {
        "world" | "wcs" => (Frame::WORLD, "WCS".to_string()),
        "previous" | "p" => {
            s.previous_ucs()?;
            return Ok(report(s.active_ucs(), "Previous"));
        }
        "named" => {
            let name = p
                .get("name")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|n| !n.is_empty())
                .ok_or_else(|| bad("ucs", "`name` is required for action `named`"))?;
            let record =
                s.doc()?.ucss.iter().find(|u| u.name.eq_ignore_ascii_case(name)).ok_or_else(|| bad("ucs", format!("no named UCS `{name}`")))?;
            let frame = Frame::from_record(record).ok_or_else(|| bad("ucs", format!("named UCS `{name}` has an invalid basis")))?;
            (frame, name.to_string())
        }
        "save" => {
            let name = p
                .get("name")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|n| !n.is_empty())
                .ok_or_else(|| bad("ucs", "`name` is required for action `save`"))?;
            if name.len() > 255 {
                return Err(bad("ucs", "`name` must be at most 255 characters"));
            }
            let frame = s.active_ucs();
            let record = frame.to_record(name.to_string());
            let d = s.doc_mut()?;
            if let Some(existing) = d.ucss.iter_mut().find(|u| u.name.eq_ignore_ascii_case(name)) {
                *existing = record;
            } else {
                if d.ucss.len() >= 4096 {
                    return Err(bad("ucs", "the drawing already has 4096 named coordinate systems"));
                }
                d.ucss.push(record);
            }
            return Ok(report(frame, name));
        }
        "set" => {
            let origin = p.get("origin").and_then(vec3).ok_or_else(|| bad("ucs", "`origin` must be three finite WCS numbers"))?;
            let x = p.get("xAxis").and_then(vec3).ok_or_else(|| bad("ucs", "`xAxis` must be three finite WCS numbers"))?;
            let y = p.get("yAxis").and_then(vec3).ok_or_else(|| bad("ucs", "`yAxis` must be three finite WCS numbers"))?;
            let frame = Frame::from_axes(origin, x, y)
                .ok_or_else(|| bad("ucs", "the UCS origin and axes must be finite and X/Y axes nonzero and independent"))?;
            let name = p.get("name").and_then(Value::as_str).map(str::trim).filter(|n| !n.is_empty()).unwrap_or("Unnamed").to_string();
            (frame, name)
        }
        _ => return Err(bad("ucs", "`action` must be world, previous, set, save or named")),
    };
    s.set_active_ucs(frame)?;
    Ok(report(frame, &name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_named_previous_and_wcs_keep_session_frame_consistent() {
        let mut s = Session::new();
        let params = json!({"action":"set", "origin":[4,5,6], "xAxis":[0,1,0], "yAxis":[0,0,1]});
        let out = run(&mut s, &params).unwrap();
        assert_eq!(out["origin"], json!([4.0, 5.0, 6.0]));
        assert!((s.active_ucs().to_world(Vec3::new(2.0, 3.0, 4.0)) - Vec3::new(8.0, 7.0, 9.0)).len() < 1e-9);
        run(&mut s, &json!({"action":"save", "name":"Fixture"})).unwrap();
        assert_eq!(s.doc().unwrap().ucss.len(), 1);
        run(&mut s, &json!({"action":"world"})).unwrap();
        assert_eq!(s.active_ucs(), Frame::WORLD);
        run(&mut s, &json!({"action":"previous"})).unwrap();
        assert_eq!(s.active_ucs().origin, Vec3::new(4.0, 5.0, 6.0));
        run(&mut s, &json!({"action":"world"})).unwrap();
        run(&mut s, &json!({"action":"named", "name":"fixture"})).unwrap();
        assert_eq!(s.active_ucs().origin, Vec3::new(4.0, 5.0, 6.0));
    }

    #[test]
    fn invalid_frame_is_refused_without_changing_current_frame() {
        let mut s = Session::new();
        let before = s.active_ucs();
        assert!(run(&mut s, &json!({"action":"set", "origin":[0,0,0], "xAxis":[1,0,0], "yAxis":[2,0,0]})).is_err());
        assert_eq!(s.active_ucs(), before);
    }

    #[test]
    fn typed_absolute_xyz_uses_active_ucs_for_a_solid_command() {
        let mut s = Session::new();
        let frame = Frame::from_axes(Vec3::new(4.0, 5.0, 6.0), Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, 0.0, 1.0)).unwrap();
        s.set_active_ucs(frame).unwrap();
        s.start("sphere").unwrap();
        s.cmdline("1,2,3").unwrap();
        s.cmdline("1").unwrap();
        let mut lo = Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY);
        let mut hi = Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
        for entity in s.doc().unwrap().model.iter() {
            if let cadcraft_doc::EntityKind::Face3d(face) = &entity.kind {
                for p in face.corners {
                    lo = Vec3::new(lo.x.min(p.x), lo.y.min(p.y), lo.z.min(p.z));
                    hi = Vec3::new(hi.x.max(p.x), hi.y.max(p.y), hi.z.max(p.z));
                }
            }
        }
        let center = (lo + hi) * 0.5;
        assert!((center - Vec3::new(7.0, 6.0, 8.0)).len() < 1e-8);
    }

    #[test]
    fn interactive_three_point_ucs_sets_a_right_handed_frame() {
        let mut s = Session::new();
        s.start("ucs").unwrap();
        s.input(Input::Keyword("3point".into())).unwrap();
        s.input(Input::Point3(Vec3::new(2.0, 3.0, 4.0))).unwrap();
        s.input(Input::Point3(Vec3::new(2.0, 5.0, 4.0))).unwrap();
        s.input(Input::Point3(Vec3::new(2.0, 3.0, 6.0))).unwrap();
        let frame = s.active_ucs();
        assert_eq!(frame.origin, Vec3::new(2.0, 3.0, 4.0));
        assert!((frame.x - Vec3::new(0.0, 1.0, 0.0)).len() < 1e-9);
        assert!((frame.y - Vec3::new(0.0, 0.0, 1.0)).len() < 1e-9);
        assert!((frame.z - Vec3::new(1.0, 0.0, 0.0)).len() < 1e-9);
    }
}
