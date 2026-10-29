use super::common::{DimText, add_dim, circle_of, dim, pick_at, text_at};
use super::{Accept, DimKind, EntityKind, Input, Interactive, Prompt, Result, Session, Step, Value, Vec2, bad, json, point_param, targets};

fn radial(s: &mut Session, p: &Value, diameter: bool) -> Result<Value> {
    let id = if diameter { "dimdiameter" } else { "dimradius" };
    let (c, pt, at) = if let (Some(c), Some(pt)) = (point_param(p, "center"), point_param(p, "point")) {
        (c, pt, None)
    } else {
        let h = targets(s, p)?.first().copied().ok_or_else(|| bad(id, "`handle` of a circle/arc, or {center, point}"))?;
        let (c, r) = circle_of(s, h).ok_or_else(|| bad(id, "object is not a circle or arc"))?;
        let at = point_param(p, "at");
        let dir = at.unwrap_or(c + Vec2::from_angle(std::f64::consts::FRAC_PI_4));
        (c, c + (dir - c).normalized() * r, at)
    };
    if c.near(pt, 1e-12) {
        return Err(bad(id, "`point` must differ from `center`"));
    }
    let k = if diameter {
        dim(s, DimKind::Diameter, c + (c - pt), Vec2::ZERO, Vec2::ZERO, pt, "")
    } else {
        dim(s, DimKind::Radius, c, Vec2::ZERO, Vec2::ZERO, pt, "")
    };
    let k = match at {
        Some(at) => text_at(k, at),
        None => k,
    };
    Ok(json!({ "handle": add_dim(s, k)?.hex() }))
}

pub(super) fn run_radius(s: &mut Session, p: &Value) -> Result<Value> {
    radial(s, p, false)
}

pub(super) fn run_diameter(s: &mut Session, p: &Value) -> Result<Value> {
    radial(s, p, true)
}

pub(super) struct RadialM {
    diameter: bool,
    target: Option<(Vec2, f64)>,
    text: DimText,
}

impl RadialM {
    pub(super) fn new(diameter: bool) -> Self {
        RadialM { diameter, target: None, text: DimText::default() }
    }
    fn kind(&self, s: &Session, c: Vec2, r: f64, at: Vec2) -> EntityKind {
        let pt = c + (at - c).normalized() * r;
        let k = self.text.apply(if self.diameter {
            dim(s, DimKind::Diameter, c + (c - pt), Vec2::ZERO, Vec2::ZERO, pt, "")
        } else {
            dim(s, DimKind::Radius, c, Vec2::ZERO, Vec2::ZERO, pt, "")
        });
        text_at(k, at)
    }
}

impl Interactive for RadialM {
    fn name(&self) -> &'static str {
        if self.diameter { "DIMDIAMETER" } else { "DIMRADIUS" }
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        if let Some(p) = self.text.prompt() {
            return p;
        }
        match self.target {
            None => Prompt::new("Select arc or circle", Accept::POINT),
            Some((c, _)) => Prompt::new("Specify dimension line location", Accept::POINT).kw(&["Mtext", "Text", "Angle"]).base(c),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if self.text.answer(s, &i) {
            return Ok(Step::Continue);
        }
        match (self.target, i) {
            (None, Input::Point(p)) => {
                match pick_at(s, p).and_then(|h| circle_of(s, h)) {
                    Some(t) => self.target = Some(t),
                    None => s.echo("Object selected is not a circle or arc."),
                }
                Ok(Step::Continue)
            }
            (Some((c, r)), Input::Point(at)) => {
                let k = self.kind(s, c, r, at);
                add_dim(s, k)?;
                Ok(Step::Done)
            }
            (Some(_), Input::Keyword(k)) => {
                self.text.keyword(&k);
                Ok(Step::Continue)
            }
            (_, Input::Enter) => Ok(Step::Cancel),
            _ => Ok(Step::Continue),
        }
    }
    fn preview(&self, s: &Session, c: Vec2) -> Vec<EntityKind> {
        self.target.map(|(ctr, r)| vec![self.kind(s, ctr, r, c)]).unwrap_or_default()
    }
}
