use super::{
    Accept, EntityKind, Input, Interactive, PolyVertex, Prompt, Result, Session, Step, Value, Vec2, added, bad, bool_or, curves, f64_or, lwpoly,
    number, other, point_param, point_req, size_param,
};

// =====================================================================================
// RECTANG with Area / Dimensions / Rotation / Width / Elevation / Thickness
// =====================================================================================

/// A rectangle's rotation (radians counterclockwise from +X) as AutoCAD 2024 shows it in RECTANG's
/// modes and rotation prompt: from ANGBASE in the ANGDIR sense, in the drawing's angle units to
/// two places ("Rotation=120.00" for 30 typed with ANGBASE 90 and ANGDIR 1, "30d30'" in
/// degrees/minutes/seconds).
fn shown_rotation(s: &Session, rotation: f64) -> String {
    let au = s.angle_settings();
    crate::units::format_angle(au.shown(rotation), au.aunits, 2)
}

/// RECTANG's corner options, remembered between invocations as AutoCAD remembers them
/// ("Current rectangle modes:  Fillet=2.0000", "Elevation=7.5000  Thickness=2.2500  Rotation=30.96").
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RectangModes {
    pub chamfer: (f64, f64),
    pub fillet: f64,
    pub width: f64,
    pub elevation: f64,
    pub thickness: f64,
    /// Radians.
    pub rotation: f64,
}

/// The vertices of the rectangle from the origin to the opposite corner `(w, h)` (signed), as
/// AutoCAD lists them: from the first corner along the first axis, so counterclockwise when `w`
/// and `h` share a sign and clockwise otherwise. A fillet or chamfer replaces each corner by the
/// point on the edge into it (where the fillet arc starts, or the first chamfer distance) and
/// the point on the edge out of it (the second chamfer distance). Corners too large for the
/// shorter side leave each corner as two coincident vertices.
fn rect_corner_vertices((w, h): (f64, f64), fillet: f64, chamfer: (f64, f64)) -> Vec<PolyVertex> {
    let corners = [Vec2::ZERO, Vec2::new(w, 0.0), Vec2::new(w, h), Vec2::new(0.0, h)];
    let Some((d_in, d_out)) = corner_cuts(fillet, chamfer) else { return corners.into_iter().map(PolyVertex::new).collect() };
    // A fillet's quarter circle bulges the way the rectangle runs; a chamfer is straight.
    let quarter = cadcraft_geom::arc_to_bulge(std::f64::consts::FRAC_PI_2);
    let bulge = match (fillet > 0.0, w * h < 0.0) {
        (false, _) => 0.0,
        (true, false) => quarter,
        (true, true) => -quarter,
    };
    let fits = corners_fit((w, h), fillet, chamfer);
    let mut out = Vec::with_capacity(8);
    for (i, c) in corners.iter().enumerate() {
        let (Some(prev), Some(next)) = (corners.get((i + 3) % 4), corners.get((i + 1) % 4)) else { continue };
        if fits {
            out.push(PolyVertex::with_bulge(*c - (*c - *prev).normalized() * d_in, bulge));
            out.push(PolyVertex::new(*c + (*next - *c).normalized() * d_out));
        } else {
            out.extend([PolyVertex::new(*c), PolyVertex::new(*c)]);
        }
    }
    out
}

/// How far a fillet or chamfer cuts into the edges into and out of each corner, if one is set.
fn corner_cuts(fillet: f64, chamfer: (f64, f64)) -> Option<(f64, f64)> {
    if fillet > 0.0 {
        Some((fillet, fillet))
    } else if chamfer.0 > 0.0 && chamfer.1 > 0.0 {
        Some(chamfer)
    } else {
        None
    }
}

/// Whether the fillet or chamfer fits the shorter side of a `w` × `h` rectangle (else its
/// corners stay square).
fn corners_fit((w, h): (f64, f64), fillet: f64, chamfer: (f64, f64)) -> bool {
    corner_cuts(fillet, chamfer).is_none_or(|(d_in, d_out)| d_in + d_out <= w.abs().min(h.abs()) * (1.0 + 1e-12))
}

/// The area the rounded or cut corners take off the full rectangle: AutoCAD's Area option sizes
/// the rectangle so its outline, corners included, encloses the area asked for.
fn corner_cut(fillet: f64, chamfer: (f64, f64)) -> f64 {
    if fillet > 0.0 {
        (4.0 - std::f64::consts::PI) * fillet * fillet
    } else if chamfer.0 > 0.0 && chamfer.1 > 0.0 {
        2.0 * chamfer.0 * chamfer.1
    } else {
        0.0
    }
}

/// The sides of the rectangle of `area` whose length (or, with `breadth`, width) is `side`: the
/// corners' cut is added back so the outline encloses the area, unless the corners then don't fit
/// and stay square, when the area is the full rectangle's.
fn area_sides(area: f64, side: f64, breadth: bool, fillet: f64, chamfer: (f64, f64)) -> (f64, f64) {
    let sides = |a: f64| if breadth { (a / side, side) } else { (side, a / side) };
    let cut = sides(area + corner_cut(fillet, chamfer));
    if corners_fit(cut, fillet, chamfer) { cut } else { sides(area) }
}

fn rect_kind(first: Vec2, (w, h): (f64, f64), rot: f64, fillet: f64, chamfer: (f64, f64), width: f64, elevation: f64) -> EntityKind {
    let local = rect_corner_vertices((w, h), fillet, chamfer);
    let vs = local.into_iter().map(|v| PolyVertex { p: first + v.p.rotate(rot), ..v }).collect();
    let mut k = lwpoly(vs, true);
    if let EntityKind::LwPolyline(pl) = &mut k {
        pl.const_width = width.max(0.0);
        pl.elevation = elevation;
    }
    k
}

/// The width and height of a rectangle from its `dimensions`, its `p2` corner, its `area` with
/// `length` or `breadth` (allowing for the `fillet` or `chamfer` corners), or its second corner
/// `p2`, rotated by `rot` about `a`.
fn rect_size(p: &Value, a: Vec2, rot: f64, centered: bool, fillet: f64, chamfer: (f64, f64)) -> Result<(f64, f64)> {
    Ok(if let Some(d) = p.get("dimensions").and_then(Value::as_array) {
        let l = d.first().and_then(Value::as_f64).filter(|v| v.is_finite()).ok_or_else(|| bad("rectang", "dimensions: [length, width]"))?;
        let wd = d.get(1).and_then(Value::as_f64).filter(|v| v.is_finite()).ok_or_else(|| bad("rectang", "dimensions: [length, width]"))?;
        let (sx, sy) = match point_param(p, "p2") {
            Some(q) => {
                let loc = (q - a).rotate(-rot);
                (if loc.x < 0.0 { -1.0 } else { 1.0 }, if loc.y < 0.0 { -1.0 } else { 1.0 })
            }
            None => (1.0, 1.0),
        };
        (l.abs() * sx, wd.abs() * sy)
    } else if let Some(area) = p.get("area").and_then(Value::as_f64).filter(|v| v.is_finite()) {
        if area <= 0.0 {
            return Err(bad("rectang", "area must be positive"));
        }
        match (p.get("length").and_then(Value::as_f64), p.get("breadth").and_then(Value::as_f64)) {
            (Some(l), _) if l.is_finite() && l > 0.0 => area_sides(area, l, false, fillet, chamfer),
            (_, Some(b)) if b.is_finite() && b > 0.0 => area_sides(area, b, true, fillet, chamfer),
            _ => return Err(bad("rectang", "area needs `length` or `breadth`")),
        }
    } else {
        let b = point_req("rectang", p, "p2")?;
        let loc = (b - a).rotate(-rot);
        // In centre mode the picked point is one corner, mirrored across the centre.
        if centered { (2.0 * loc.x, 2.0 * loc.y) } else { (loc.x, loc.y) }
    })
}

/// Draws a rectangle from the JSON form: `p1` is a corner, or the centre when `center` is
/// true, and the size comes from `p2`, `dimensions` or `area` plus `length`/`breadth`.
pub(crate) fn run_rectang2(s: &mut Session, p: &Value) -> Result<Value> {
    let a = point_req("rectang", p, "p1")?;
    // Centre mode: `p1` is the rectangle's centre, so the rectangle spans it evenly.
    let centered = bool_or(p, "center", false);
    let rot = f64_or(p, "rotation", 0.0).to_radians();
    let ch = p
        .get("chamfer")
        .and_then(Value::as_array)
        .map(|c| (c.first().and_then(Value::as_f64).unwrap_or(0.0), c.get(1).and_then(Value::as_f64).unwrap_or(0.0)))
        .map(|(x, y)| (if x.is_finite() { x } else { 0.0 }, if y.is_finite() { y } else { 0.0 }))
        .unwrap_or((0.0, 0.0));
    if ch.0 < 0.0 || ch.1 < 0.0 {
        return Err(bad("rectang", "`chamfer` distances must be zero or positive"));
    }
    let fillet = size_param("rectang", p, "fillet", true)?.unwrap_or(0.0);
    let width = size_param("rectang", p, "width", true)?.unwrap_or(0.0);
    let (w, h) = rect_size(p, a, rot, centered, fillet, ch)?;
    if w.abs() < 1e-12 || h.abs() < 1e-12 || !w.is_finite() || !h.is_finite() {
        return Err(bad("rectang", "rectangle has no area"));
    }
    let first = if centered { a - Vec2::new(w / 2.0, h / 2.0).rotate(rot) } else { a };
    let k = rect_kind(first, (w, h), rot, fillet, ch, width, f64_or(p, "elevation", 0.0));
    let hd = s.add_entity(k)?;
    // A rectangle has its own thickness option and does not take THICKNESS.
    let th = f64_or(p, "thickness", 0.0);
    s.doc_mut()?.modify_entity(hd, |e| e.common.thickness = th)?;
    added(hd)
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum RAsk {
    Chamfer1,
    Chamfer2,
    Fillet,
    Width,
    Elevation,
    Thickness,
    Area,
    AreaBasis,
    AreaLength,
    AreaWidth,
    DimL,
    DimW,
    Rotation,
    RotPick1,
    RotPick2(Vec2),
}

#[derive(Default)]
pub(crate) struct RectM2 {
    first: Option<Vec2>,
    /// The first point is the rectangle's centre (the `Center` option), not a corner.
    center: bool,
    fillet: f64,
    chamfer: (f64, f64),
    width: f64,
    elevation: f64,
    thickness: f64,
    rotation: f64,
    dims: Option<(f64, f64)>,
    area: f64,
    asking: Option<RAsk>,
}

impl RectM2 {
    /// Adds the rectangle of full size `w` × `h` with the collected options, drawing it from
    /// the centre when centre mode is on, then applies the requested thickness.
    fn make(&self, s: &mut Session, w: f64, h: f64) -> Result<Step> {
        let Some(a) = self.first else { return Ok(Step::Cancel) };
        if w.abs() < 1e-12 || h.abs() < 1e-12 {
            return Err(other("The rectangle has no area."));
        }
        let first = self.origin(w, h).unwrap_or(a);
        let hd = s.add_entity(rect_kind(first, (w, h), self.rotation, self.fillet, self.chamfer, self.width, self.elevation))?;
        // A rectangle has its own thickness option and does not take THICKNESS.
        let t = self.thickness;
        s.doc_mut()?.modify_entity(hd, |e| e.common.thickness = t)?;
        self.remember(s);
        Ok(Step::Done)
    }
    /// Keeps the corner options for the next RECTANG.
    fn remember(&self, s: &mut Session) {
        s.last_used.rectang = RectangModes {
            chamfer: self.chamfer,
            fillet: self.fillet,
            width: self.width,
            elevation: self.elevation,
            thickness: self.thickness,
            rotation: self.rotation,
        };
    }
    /// The rectangle's local origin (the corner `rect_kind` draws from) for a size: the first
    /// point itself as a corner, or the first point less half the size around the centre.
    fn origin(&self, w: f64, h: f64) -> Option<Vec2> {
        let a = self.first?;
        Some(if self.center { a - Vec2::new(w / 2.0, h / 2.0).rotate(self.rotation) } else { a })
    }
    /// The rectangle's signed full size implied by the picked point `p`: explicit `dims` only
    /// take their quadrant from `p`, otherwise `p` is a corner, doubled about the centre in
    /// centre mode.
    fn other_corner(&self, p: Vec2) -> Option<(f64, f64)> {
        let a = self.first?;
        let loc = (p - a).rotate(-self.rotation);
        Some(match self.dims {
            Some((l, w)) => (if loc.x < 0.0 { -l } else { l }, if loc.y < 0.0 { -w } else { w }),
            // In centre mode the picked point is one corner, mirrored across the centre.
            None if self.center => (2.0 * loc.x, 2.0 * loc.y),
            None => (loc.x, loc.y),
        })
    }
}

impl Interactive for RectM2 {
    fn name(&self) -> &'static str {
        "RECTANG"
    }
    /// Starts from the corner options the last RECTANG used, and says which are on, as AutoCAD
    /// does ("Current rectangle modes:  Chamfer=1.6574 x 2.8497  Rotation=50.50").
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        let m = s.last_used.rectang;
        self.chamfer = m.chamfer;
        self.fillet = m.fillet;
        self.width = m.width;
        self.elevation = m.elevation;
        self.thickness = m.thickness;
        self.rotation = m.rotation;
        let mut on = Vec::new();
        if m.chamfer.0 > 0.0 || m.chamfer.1 > 0.0 {
            on.push(format!("Chamfer={:.4} x {:.4}", m.chamfer.0, m.chamfer.1));
        }
        if m.elevation != 0.0 {
            on.push(format!("Elevation={:.4}", m.elevation));
        }
        if m.fillet > 0.0 {
            on.push(format!("Fillet={:.4}", m.fillet));
        }
        if m.thickness != 0.0 {
            on.push(format!("Thickness={:.4}", m.thickness));
        }
        if m.width > 0.0 {
            on.push(format!("Width={:.4}", m.width));
        }
        let rotation = m.rotation.to_degrees().rem_euclid(360.0);
        if rotation > 1e-9 && rotation < 360.0 - 1e-9 {
            on.push(format!("Rotation={}", shown_rotation(s, m.rotation)));
        }
        if !on.is_empty() {
            s.echo(format!("Current rectangle modes:  {}", on.join("  ")));
        }
        Ok(Step::Continue)
    }
    /// Asks for the pending option's value, or for the first and second points; in centre mode
    /// the first prompt asks for the centre instead of a corner.
    fn prompt(&self, _s: &Session) -> Prompt {
        let num = |m: &str, d: f64| Prompt::new(m, Accept::NUMBER).default(format!("{d:.4}"));
        if let Some(a) = self.asking {
            return match a {
                RAsk::Chamfer1 => num("Specify first chamfer distance for rectangles", self.chamfer.0),
                RAsk::Chamfer2 => num("Specify second chamfer distance for rectangles", self.chamfer.1),
                RAsk::Fillet => num("Specify fillet radius for rectangles", self.fillet),
                RAsk::Width => num("Specify line width for rectangles", self.width),
                RAsk::Elevation => num("Specify the elevation for rectangles", self.elevation),
                RAsk::Thickness => num("Specify thickness for rectangles", self.thickness),
                RAsk::Area => num("Enter area of rectangle in current units", if self.area > 0.0 { self.area } else { 100.0 }),
                RAsk::AreaBasis => Prompt::new("Calculate rectangle dimensions based on", curves::KW).kw(&["Length", "Width"]).default("Length"),
                RAsk::AreaLength => num("Enter rectangle length", 10.0),
                RAsk::AreaWidth => num("Enter rectangle width", 10.0),
                RAsk::DimL => num("Specify length for rectangles", self.dims.map(|d| d.0).unwrap_or(10.0)),
                RAsk::DimW => num("Specify width for rectangles", self.dims.map(|d| d.1).unwrap_or(10.0)),
                RAsk::Rotation => Prompt::new("Specify rotation angle or", Accept::POINT_OR_NUMBER)
                    .kw(&["Pick points"])
                    .default(shown_rotation(_s, self.rotation))
                    .base_opt(self.first),
                RAsk::RotPick1 => Prompt::new("Specify first point", Accept::POINT),
                RAsk::RotPick2(b) => Prompt::new("Specify second point", Accept::POINT).base(b),
            };
        }
        match self.first {
            None => {
                let msg = if self.center { "Specify center point for rectangle" } else { "Specify first corner point" };
                Prompt::new(msg, Accept::POINT).kw(&["Chamfer", "CEnter", "Elevation", "Fillet", "Thickness", "Width"])
            }
            Some(p) => Prompt::new("Specify other corner point", Accept::POINT).kw(&["Area", "Dimensions", "Rotation"]).corner(p),
        }
    }
    /// Consumes one input: option keywords (`Center` only switches the first point's meaning
    /// to the centre and asks nothing), option values, and the points that size the rectangle.
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if let Some(a) = self.asking {
            let val = |i: &Input, default: f64| -> Result<f64> {
                match i {
                    Input::Text(t) => number(t).ok_or_else(|| other("Requires a number.")),
                    Input::Enter => Ok(default),
                    _ => Err(other("Requires a number.")),
                }
            };
            self.asking = None;
            match a {
                RAsk::Chamfer1 => {
                    self.chamfer.0 = val(&i, self.chamfer.0)?.max(0.0);
                    self.chamfer.1 = self.chamfer.0;
                    self.asking = Some(RAsk::Chamfer2);
                }
                // A rectangle's corners are chamfered or filleted: the option set last wins.
                RAsk::Chamfer2 => {
                    self.chamfer.1 = val(&i, self.chamfer.1)?.max(0.0);
                    self.fillet = 0.0;
                }
                RAsk::Fillet => {
                    self.fillet = val(&i, self.fillet)?.max(0.0);
                    self.chamfer = (0.0, 0.0);
                }
                RAsk::Width => self.width = val(&i, self.width)?.max(0.0),
                RAsk::Elevation => self.elevation = val(&i, self.elevation)?,
                RAsk::Thickness => self.thickness = val(&i, self.thickness)?,
                RAsk::Area => {
                    let v = val(&i, if self.area > 0.0 { self.area } else { 100.0 })?;
                    if v <= 0.0 {
                        self.asking = Some(RAsk::Area);
                        return Err(other("Value must be positive."));
                    }
                    self.area = v;
                    self.asking = Some(RAsk::AreaBasis);
                }
                RAsk::AreaBasis => {
                    let w = matches!(&i, Input::Keyword(k) | Input::Text(k) if k.to_ascii_lowercase().starts_with('w'));
                    self.asking = Some(if w { RAsk::AreaWidth } else { RAsk::AreaLength });
                }
                RAsk::AreaLength | RAsk::AreaWidth => {
                    let v = val(&i, 10.0)?;
                    if v <= 0.0 {
                        self.asking = Some(a);
                        return Err(other("Value must be positive."));
                    }
                    let (w, h) = area_sides(self.area, v, a == RAsk::AreaWidth, self.fillet, self.chamfer);
                    return self.make(s, w, h);
                }
                RAsk::DimL => {
                    let l = val(&i, self.dims.map(|d| d.0).unwrap_or(10.0))?.abs();
                    self.dims = Some((l, self.dims.map(|d| d.1).unwrap_or(10.0)));
                    self.asking = Some(RAsk::DimW);
                }
                RAsk::DimW => {
                    let w = val(&i, self.dims.map(|d| d.1).unwrap_or(10.0))?.abs();
                    self.dims = Some((self.dims.map(|d| d.0).unwrap_or(10.0), w));
                }
                RAsk::Rotation => match i {
                    Input::Keyword(_) => self.asking = Some(RAsk::RotPick1),
                    Input::Point(p) => self.rotation = self.first.map(|f| f.angle_to(p)).unwrap_or(0.0),
                    // An amount turned in the ANGDIR sense from +X, ANGBASE aside, as AutoCAD takes it.
                    Input::Text(t) => self.rotation = s.angle_settings().rotation(&t).ok_or_else(|| other("Requires a valid angle."))?,
                    _ => {}
                },
                RAsk::RotPick1 => {
                    if let Input::Point(p) = i {
                        self.asking = Some(RAsk::RotPick2(p));
                    }
                }
                RAsk::RotPick2(b) => {
                    if let Input::Point(p) = i {
                        self.rotation = b.angle_to(p);
                    }
                }
            }
            self.remember(s);
            return Ok(Step::Continue);
        }
        match i {
            Input::Keyword(k) => {
                // `Center` only switches the first point's meaning; it asks nothing.
                if k == "CEnter" {
                    self.center = true;
                    return Ok(Step::Continue);
                }
                self.asking = Some(match k.as_str() {
                    "Chamfer" => RAsk::Chamfer1,
                    "Elevation" => RAsk::Elevation,
                    "Fillet" => RAsk::Fillet,
                    "Thickness" => RAsk::Thickness,
                    "Width" => RAsk::Width,
                    "Area" => RAsk::Area,
                    "Dimensions" => RAsk::DimL,
                    _ => RAsk::Rotation,
                });
                Ok(Step::Continue)
            }
            Input::Point(p) => match self.first {
                None => {
                    self.first = Some(p);
                    Ok(Step::Continue)
                }
                Some(_) => {
                    let (w, h) = self.other_corner(p).ok_or_else(|| other("No first corner."))?;
                    self.make(s, w, h)
                }
            },
            Input::Enter => Ok(Step::Cancel),
            _ => Ok(Step::Continue),
        }
    }
    /// The rubber-band rectangle for the cursor at `c`, mapped through the same size-and-origin
    /// rules as the committed entity so centre mode previews where it will land.
    fn preview(&self, _s: &Session, c: Vec2) -> Vec<EntityKind> {
        if self.asking.is_some() {
            return Vec::new();
        }
        match self.other_corner(c).and_then(|(w, h)| self.origin(w, h).map(|a| (a, w, h))) {
            Some((a, w, h)) if w.abs() > 1e-12 && h.abs() > 1e-12 => {
                vec![rect_kind(a, (w, h), self.rotation, self.fillet, self.chamfer, self.width, self.elevation)]
            }
            _ => Vec::new(),
        }
    }
}
