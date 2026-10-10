//! Array commands: ARRAY, ARRAYRECT and ARRAYPOLAR (ARRAYPATH lives in `modify2`).
//!
//! Arrays are created as separate copies of the source objects (non-associative). The interactive
//! forms select objects, then edit the array in an option loop with a live preview, and create
//! the copies on eXit (or Enter).

use cadcraft_doc::{Drawing, EntityKind, Handle};
use cadcraft_geom::{Bounds2, Mat3, TAU, Vec2};
use serde_json::{Value, json};

use super::helpers::line;
use super::machines::{SelOutcome, SelectPhase, number};
use super::modify::transform_entities;
use super::*;
use crate::{Accept, EngineError, Input, Interactive, Prompt, Result, Session, Step};

/// Most preview entities drawn while the array is edited.
const PREVIEW_LIMIT: usize = 2000;
/// Keyword-only prompts: each typed token is matched on its own (no free text).
const KEYWORDS: Accept = Accept { point: false, number: false, text: false, select: false, enter: true };

const NOT_ASSOCIATIVE: &str = "Associative arrays are not available yet; the array is created as separate objects.";
const NO_LEVELS: &str = "Levels (3D arrays) are not available yet.";

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("array", "Array", run_array)
            .alias(&["ar"])
            .params("{type?: \"rectangular\" (default) | \"polar\" | \"path\", ...the parameters of arrayrect, arraypolar or arraypath}")
            .interactive(|_| Ok(Box::new(ArrayM::default()))),
        CommandSpec::new("arrayrect", "Rectangular Array", run_arrayrect)
            .menu(&["Modify", "Array", "Rectangular Array"])
            .params("{handles?, rows, cols, rowSpacing, colSpacing}")
            .interactive(|_| Ok(Box::new(RectM::default()))),
        CommandSpec::new("arraypolar", "Polar Array", run_arraypolar)
            .menu(&["Modify", "Array", "Polar Array"])
            .params("{handles?, center, count, angle? (degrees, default 360), rotate?: bool}")
            .interactive(|_| Ok(Box::new(PolarM::default()))),
    ]
}

// ---------------- geometry ----------------

/// Translations of the copies in a rectangular array (the source cell is skipped).
fn rect_mats(rows: u64, cols: u64, rs: f64, cs: f64) -> impl Iterator<Item = Mat3> {
    (0..rows)
        .flat_map(move |r| (0..cols).map(move |c| (r, c)))
        .filter(|rc| *rc != (0, 0))
        .map(move |(r, c)| Mat3::translate(Vec2::new(cs * c as f64, rs * r as f64)))
}

/// Transforms of the copies 1..n of a polar array filling `fill` degrees (positive = ccw).
/// Without `rotate` each copy is moved so that `reference` travels around the circle.
fn polar_mats(center: Vec2, n: u64, fill: f64, rotate: bool, reference: Vec2) -> impl Iterator<Item = Mat3> {
    let total = fill.to_radians();
    let full = (total.abs() - TAU).abs() < 1e-9;
    let step = if full { total / n.max(1) as f64 } else { total / n.saturating_sub(1).max(1) as f64 };
    (1..n).map(move |k| {
        let a = step * k as f64;
        if rotate { Mat3::rotate_about(center, a) } else { Mat3::translate(reference.rotate_about(center, a) - reference) }
    })
}

fn bounds(d: &Drawing, hs: &[Handle]) -> Bounds2 {
    hs.iter().filter_map(|h| d.entity(*h).map(|e| cadcraft_doc::entity_bounds(d, e, 0))).fold(Bounds2::EMPTY, |a, b| a.union(&b))
}

/// The centre of the first object (the point a non-rotated polar copy is moved by).
fn first_center(d: &Drawing, hs: &[Handle]) -> Option<Vec2> {
    hs.first().and_then(|h| d.entity(*h)).map(|e| cadcraft_doc::entity_bounds(d, e, 0).center()).filter(|c| c.is_finite())
}

/// MAXARRAY: refuse an array of `items` copies of `objs` objects that would exceed the limit
/// (instead of creating fewer copies than asked for).
pub(crate) fn check_size(s: &Session, items: u64, objs: usize) -> Result<()> {
    let max = s.settings.maxarray;
    let total = items.saturating_mul(objs.max(1) as u64);
    if total > max {
        return Err(other(format!(
            "The array would create {total} objects; the limit is {max} (MAXARRAY). Use fewer items or select fewer objects."
        )));
    }
    Ok(())
}

/// The most items a count prompt accepts when the rest of the array is `others` items of `objs` objects.
fn max_count(s: &Session, others: u64, objs: usize) -> u64 {
    (s.settings.maxarray / others.max(1).saturating_mul(objs.max(1) as u64)).max(1)
}

fn copy_all(s: &mut Session, hs: &[Handle], mats: impl Iterator<Item = Mat3>) -> Result<usize> {
    let mut n = 0;
    for m in mats {
        n += transform_entities(s, hs, &m, true)?.len();
    }
    Ok(n)
}

/// The objects transformed by each matrix, for the preview.
fn preview_copies(s: &Session, hs: &[Handle], mats: impl Iterator<Item = Mat3>) -> Vec<EntityKind> {
    let Ok(d) = s.doc() else { return Vec::new() };
    let src: Vec<EntityKind> = hs.iter().take(PREVIEW_LIMIT).filter_map(|h| d.entity(*h)).map(|e| e.kind.clone()).collect();
    if src.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for m in mats {
        for k in &src {
            if out.len() >= PREVIEW_LIMIT {
                return out;
            }
            let mut k = k.clone();
            k.transform(&m);
            out.push(k);
        }
    }
    out
}

fn other(m: impl Into<String>) -> EngineError {
    EngineError::Other(m.into())
}

fn fmt(v: f64) -> String {
    format!("{v:.4}")
}

/// A count typed at a prompt (Enter keeps `cur`); `None` for inputs the prompt ignores.
fn count(i: &Input, cur: u64, max: u64) -> Result<Option<u64>> {
    match i {
        Input::Enter => Ok(Some(cur)),
        Input::Text(t) => t
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|v| v.fract() == 0.0 && *v >= 1.0 && *v <= max as f64)
            .map(|v| Some(v as u64))
            .ok_or_else(|| other(format!("Requires an integer between 1 and {max} (MAXARRAY limits the size of the array)."))),
        _ => Ok(None),
    }
}

/// An angle in degrees typed at a prompt (Enter keeps `cur`); `None` for inputs the prompt ignores.
fn angle(i: &Input, cur: f64) -> Result<Option<f64>> {
    match i {
        Input::Enter => Ok(Some(cur)),
        Input::Text(t) => crate::units::parse_angle(t).map(|a| Some(a.to_degrees())).ok_or_else(|| other("Requires a valid angle in degrees.")),
        _ => Ok(None),
    }
}

fn yes_no(i: &Input, cur: bool) -> Option<bool> {
    match i {
        Input::Enter => Some(cur),
        Input::Keyword(k) | Input::Text(k) => match k.trim().to_ascii_lowercase().chars().next() {
            Some('y') => Some(true),
            Some('n') => Some(false),
            _ => None,
        },
        _ => None,
    }
}

// ---------------- JSON ----------------

fn run_array(s: &mut Session, p: &Value) -> Result<Value> {
    let id = match str_param(p, "type").unwrap_or("rectangular").to_ascii_lowercase().as_str() {
        "rectangular" | "rect" | "r" => "arrayrect",
        "polar" | "po" => "arraypolar",
        "path" | "pa" => "arraypath",
        _ => return Err(bad("array", "`type` must be \"rectangular\", \"polar\" or \"path\"")),
    };
    let spec = find_command(id).ok_or_else(|| EngineError::UnknownCommand(id.into()))?;
    (spec.run)(s, p)
}

fn run_arrayrect(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    let rows = p.get("rows").and_then(Value::as_u64).unwrap_or(3).max(1);
    let cols = p.get("cols").and_then(Value::as_u64).unwrap_or(4).max(1);
    check_size(s, rows.saturating_mul(cols), hs.len())?;
    let rs = f64_or(p, "rowSpacing", 1.0);
    let cs = f64_or(p, "colSpacing", 1.0);
    let n = copy_all(s, &hs, rect_mats(rows, cols, rs, cs))?;
    Ok(json!({ "created": n }))
}

fn run_arraypolar(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    let c = point_req("arraypolar", p, "center")?;
    let n = p.get("count").and_then(Value::as_u64).unwrap_or(6).max(1);
    check_size(s, n, hs.len())?;
    let fill = f64_or(p, "angle", 360.0);
    let rotate = bool_or(p, "rotate", true);
    let reference = first_center(s.doc()?, &hs).unwrap_or(c);
    let n = copy_all(s, &hs, polar_mats(c, n, fill, rotate, reference))?;
    Ok(json!({ "created": n }))
}

// ---------------- ARRAY ----------------

/// ARRAY: select objects, choose the array type, then continue as ARRAYRECT, ARRAYPATH or
/// ARRAYPOLAR with those objects.
#[derive(Default)]
struct ArrayM {
    sel: SelectPhase,
    objs: Vec<Handle>,
    inner: Option<Box<dyn Interactive>>,
}

impl Interactive for ArrayM {
    fn name(&self) -> &'static str {
        "ARRAY"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        self.sel = SelectPhase::begin(s);
        if self.sel.done {
            self.objs = self.sel.picked.clone();
        }
        Ok(Step::Continue)
    }
    fn prompt(&self, s: &Session) -> Prompt {
        match &self.inner {
            Some(m) => m.prompt(s),
            None if !self.sel.done => self.sel.prompt(),
            None => Prompt::new("Enter array type", KEYWORDS).kw(&["Rectangular", "PAth", "POlar"]).default("Rectangular"),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if let Some(m) = &mut self.inner {
            return m.input(s, i);
        }
        if !self.sel.done {
            return match self.sel.feed(s, &i)? {
                SelOutcome::More => Ok(Step::Continue),
                SelOutcome::Empty => Ok(Step::Done),
                SelOutcome::Done(hs) => {
                    self.objs = hs;
                    Ok(Step::Continue)
                }
            };
        }
        let kind = match &i {
            Input::Enter => "Rectangular",
            Input::Keyword(k) => k.as_str(),
            _ => return Err(other("Invalid option keyword.")),
        };
        let objs = self.objs.clone();
        let (m, step): (Box<dyn Interactive>, Step) = match kind {
            "PAth" => (Box::new(super::modify2::PathArrayM::with_objects(objs)), Step::Continue),
            "POlar" => {
                let mut m = PolarM::with_objects(objs);
                let step = m.ready(s);
                (Box::new(m), step)
            }
            _ => {
                let mut m = RectM::with_objects(objs);
                let step = m.ready(s)?;
                (Box::new(m), step)
            }
        };
        self.inner = Some(m);
        Ok(step)
    }
    fn preview(&self, s: &Session, cursor: Vec2) -> Vec<EntityKind> {
        self.inner.as_ref().map(|m| m.preview(s, cursor)).unwrap_or_default()
    }
}

// ---------------- ARRAYRECT ----------------

#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum RStage {
    #[default]
    Main,
    Base,
    /// COUnt: columns, then rows.
    CountCols,
    CountRows,
    /// Spacing: between columns (or a unit cell), then between rows.
    SpaceCols,
    SpaceRows,
    UnitCell,
    /// COLumns: count, then spacing (or the total).
    ColsCount,
    ColsSpace,
    ColsTotal,
    /// Rows: count, then spacing (or the total), then the elevation increment.
    RowsCount,
    RowsSpace,
    RowsTotal,
    RowsElev,
}

/// ARRAYRECT: select objects, then edit columns/rows/spacing in a loop; eXit creates the copies.
#[derive(Default)]
struct RectM {
    sel: SelectPhase,
    objs: Vec<Handle>,
    stage: RStage,
    cols: u64,
    rows: u64,
    cs: f64,
    rs: f64,
    /// Array base point (distances picked with a point are measured from it).
    base: Vec2,
    centroid: Vec2,
    /// Unit cell: the first corner, once picked.
    corner: Option<Vec2>,
}

impl RectM {
    fn with_objects(objs: Vec<Handle>) -> Self {
        RectM { sel: SelectPhase { picked: objs.clone(), done: true, ..SelectPhase::default() }, objs, ..RectM::default() }
    }

    /// Objects are selected: set the defaults (4 columns × 3 rows, 1.5 × the selection size apart).
    fn ready(&mut self, s: &mut Session) -> Result<Step> {
        let b = bounds(s.doc()?, &self.objs);
        let (w, h) = (b.width(), b.height());
        let size = |v: f64, o: f64| match (v.is_finite() && v > 1e-9, o.is_finite() && o > 1e-9) {
            (true, _) => v * 1.5,
            (false, true) => o * 1.5,
            _ => 1.0,
        };
        (self.cols, self.rows, self.cs, self.rs) = (4, 3, size(w, h), size(h, w));
        self.centroid = if b.center().is_finite() { b.center() } else { Vec2::ZERO };
        self.base = self.centroid;
        self.stage = RStage::Main;
        s.echo("Type = Rectangular  Associative = No");
        Ok(Step::Continue)
    }

    /// A distance typed or picked (from the base point); Enter keeps `cur`.
    fn distance(&self, i: &Input, cur: f64) -> Result<Option<f64>> {
        let d = match i {
            Input::Enter => return Ok(Some(cur)),
            Input::Text(t) => number(t),
            Input::Point(p) => Some(self.base.dist(*p)),
            _ => return Ok(None),
        };
        d.filter(|d| d.is_finite() && d.abs() > 1e-12).map(Some).ok_or_else(|| other("Requires a nonzero distance."))
    }

    fn finish(&mut self, s: &mut Session) -> Result<Step> {
        check_size(s, self.rows.saturating_mul(self.cols), self.objs.len())?;
        copy_all(s, &self.objs, rect_mats(self.rows, self.cols, self.rs, self.cs))?;
        s.set_selection(Vec::new());
        Ok(Step::Done)
    }

    fn main(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        let k = match i {
            Input::Enter => return self.finish(s),
            Input::Keyword(k) => k,
            Input::Point(_) => return Err(other("Editing the array with grips is not available yet; choose an option.")),
            _ => return Err(other("Invalid option keyword.")),
        };
        self.stage = match k.as_str() {
            "eXit" => return self.finish(s),
            "Base point" => RStage::Base,
            "COUnt" => RStage::CountCols,
            "Spacing" => RStage::SpaceCols,
            "COLumns" => RStage::ColsCount,
            "Rows" => RStage::RowsCount,
            "Levels" => {
                s.echo(NO_LEVELS);
                RStage::Main
            }
            _ => {
                s.echo(NOT_ASSOCIATIVE);
                RStage::Main
            }
        };
        Ok(Step::Continue)
    }
}

impl Interactive for RectM {
    fn name(&self) -> &'static str {
        "ARRAYRECT"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        self.sel = SelectPhase::begin(s);
        if self.sel.done {
            self.objs = self.sel.picked.clone();
            return self.ready(s);
        }
        Ok(Step::Continue)
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        use RStage::*;
        if !self.sel.done {
            return self.sel.prompt();
        }
        let cols = || Prompt::new("Enter the number of columns", Accept::NUMBER).default(self.cols.to_string());
        let rows = || Prompt::new("Enter the number of rows", Accept::NUMBER).default(self.rows.to_string());
        let dist = |m: &str, d: f64| Prompt::new(m, Accept::POINT_OR_NUMBER).default(fmt(d)).base(self.base);
        match self.stage {
            Main => Prompt::new("Select grip to edit array", Accept::POINT)
                .kw(&["ASsociative", "Base point", "COUnt", "Spacing", "COLumns", "Rows", "Levels", "eXit"])
                .default("eXit"),
            Base => Prompt::new("Specify base point", Accept::POINT).kw(&["Key point"]).default("centroid"),
            CountCols | ColsCount => cols(),
            CountRows | RowsCount => rows(),
            SpaceCols => dist("Specify the distance between columns", self.cs).kw(&["Unit cell"]),
            SpaceRows => dist("Specify the distance between rows", self.rs),
            UnitCell => match self.corner {
                None => Prompt::new("Specify first corner of the unit cell", Accept::POINT).base(self.base),
                Some(c) => Prompt::new("Specify other corner", Accept::POINT).base(c),
            },
            ColsSpace => dist("Specify the distance between columns", self.cs).kw(&["Total"]),
            ColsTotal => dist("Specify the distance between the first and last column", self.cs * self.cols.saturating_sub(1) as f64),
            RowsSpace => dist("Specify the distance between rows", self.rs).kw(&["Total"]),
            RowsTotal => dist("Specify the distance between the first and last row", self.rs * self.rows.saturating_sub(1) as f64),
            RowsElev => Prompt::new("Specify the incrementing elevation between rows", Accept::NUMBER).default("0"),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        use RStage::*;
        if !self.sel.done {
            return match self.sel.feed(s, &i)? {
                SelOutcome::More => Ok(Step::Continue),
                SelOutcome::Empty => Ok(Step::Done),
                SelOutcome::Done(hs) => {
                    self.objs = hs;
                    self.ready(s)
                }
            };
        }
        let stage = self.stage;
        let next = match (stage, &i) {
            (Main, _) => return self.main(s, i),
            (Base, Input::Point(p)) => {
                self.base = *p;
                Main
            }
            (Base, Input::Enter) => {
                self.base = self.centroid;
                Main
            }
            (Base, Input::Keyword(_)) => {
                s.echo("Key points are not available yet; specify a point.");
                Base
            }
            (CountCols | ColsCount, _) => match count(&i, self.cols, max_count(s, self.rows, self.objs.len()))? {
                Some(n) => {
                    self.cols = n;
                    if stage == CountCols { CountRows } else { ColsSpace }
                }
                None => stage,
            },
            (CountRows | RowsCount, _) => match count(&i, self.rows, max_count(s, self.cols, self.objs.len()))? {
                Some(n) => {
                    self.rows = n;
                    if stage == CountRows { Main } else { RowsSpace }
                }
                None => stage,
            },
            (SpaceCols, Input::Keyword(_)) => {
                self.corner = None;
                UnitCell
            }
            (ColsSpace, Input::Keyword(_)) => ColsTotal,
            (RowsSpace, Input::Keyword(_)) => RowsTotal,
            (SpaceCols | ColsSpace, _) => match self.distance(&i, self.cs)? {
                Some(d) => {
                    self.cs = d;
                    if stage == SpaceCols { SpaceRows } else { Main }
                }
                None => stage,
            },
            (SpaceRows | RowsSpace, _) => match self.distance(&i, self.rs)? {
                Some(d) => {
                    self.rs = d;
                    if stage == SpaceRows { Main } else { RowsElev }
                }
                None => stage,
            },
            (ColsTotal, _) => match self.distance(&i, self.cs * self.cols.saturating_sub(1) as f64)? {
                Some(t) => {
                    self.cs = t / self.cols.saturating_sub(1).max(1) as f64;
                    Main
                }
                None => stage,
            },
            (RowsTotal, _) => match self.distance(&i, self.rs * self.rows.saturating_sub(1) as f64)? {
                Some(t) => {
                    self.rs = t / self.rows.saturating_sub(1).max(1) as f64;
                    RowsElev
                }
                None => stage,
            },
            (RowsElev, Input::Enter) => Main,
            (RowsElev, Input::Text(t)) => match number(t) {
                Some(z) if z.is_finite() => {
                    if z.abs() > 1e-12 {
                        s.echo("Row elevation increments (3D) are not available yet; the rows stay at the same elevation.");
                    }
                    Main
                }
                _ => return Err(other("Requires a number.")),
            },
            (UnitCell, Input::Point(p)) => match self.corner {
                None => {
                    self.corner = Some(*p);
                    UnitCell
                }
                Some(c) => {
                    let (dx, dy) = (p.x - c.x, p.y - c.y);
                    if dx.abs() <= 1e-12 || dy.abs() <= 1e-12 {
                        return Err(other("The unit cell must have a width and a height."));
                    }
                    (self.cs, self.rs, self.corner) = (dx, dy, None);
                    Main
                }
            },
            (UnitCell, Input::Enter) => {
                self.corner = None;
                Main
            }
            _ => stage,
        };
        self.stage = next;
        Ok(Step::Continue)
    }
    fn preview(&self, s: &Session, cursor: Vec2) -> Vec<EntityKind> {
        if !self.sel.done {
            return Vec::new();
        }
        let mut out = preview_copies(s, &self.objs, rect_mats(self.rows, self.cols, self.rs, self.cs));
        match (self.stage, self.corner) {
            (RStage::UnitCell, Some(c)) => out.push(line(c, cursor)),
            (RStage::SpaceCols | RStage::SpaceRows | RStage::ColsSpace | RStage::ColsTotal | RStage::RowsSpace | RStage::RowsTotal, _) => {
                out.push(line(self.base, cursor))
            }
            _ => {}
        }
        out
    }
}

// ---------------- ARRAYPOLAR ----------------

#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum PStage {
    #[default]
    Center,
    /// Base point asked from the center prompt (returns there).
    CenterBase,
    Main,
    Base,
    Items,
    Between,
    Fill,
    Rotate,
}

/// ARRAYPOLAR: select objects, specify the center, then edit items/angles in a loop; eXit creates
/// the copies.
struct PolarM {
    sel: SelectPhase,
    objs: Vec<Handle>,
    stage: PStage,
    center: Vec2,
    items: u64,
    /// Fill angle in degrees (positive = counterclockwise).
    fill: f64,
    rotate: bool,
    /// Base point: the point non-rotated copies are moved by (default: the first object's centre).
    base: Option<Vec2>,
}

impl Default for PolarM {
    fn default() -> Self {
        PolarM {
            sel: SelectPhase::default(),
            objs: Vec::new(),
            stage: PStage::Center,
            center: Vec2::ZERO,
            items: 6,
            fill: 360.0,
            rotate: true,
            base: None,
        }
    }
}

impl PolarM {
    fn with_objects(objs: Vec<Handle>) -> Self {
        PolarM { sel: SelectPhase { picked: objs.clone(), done: true, ..SelectPhase::default() }, objs, ..PolarM::default() }
    }

    fn ready(&mut self, s: &mut Session) -> Step {
        self.stage = PStage::Center;
        s.echo("Type = Polar  Associative = No");
        Step::Continue
    }

    fn full(&self) -> bool {
        (self.fill.abs() - 360.0).abs() < 1e-9
    }

    /// The angle between items, derived from the item count and the fill angle.
    fn between(&self) -> f64 {
        match self.items {
            0 | 1 => self.fill,
            n if self.full() => self.fill / n as f64,
            n => self.fill / (n - 1) as f64,
        }
    }

    fn mats(&self, s: &Session, center: Vec2) -> impl Iterator<Item = Mat3> {
        let reference = self.base.or_else(|| s.doc().ok().and_then(|d| first_center(d, &self.objs))).unwrap_or(center);
        polar_mats(center, self.items, self.fill, self.rotate, reference)
    }

    fn finish(&mut self, s: &mut Session) -> Result<Step> {
        check_size(s, self.items, self.objs.len())?;
        let mats: Vec<Mat3> = self.mats(s, self.center).collect();
        copy_all(s, &self.objs, mats.into_iter())?;
        s.set_selection(Vec::new());
        Ok(Step::Done)
    }

    fn main(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        let k = match i {
            Input::Enter => return self.finish(s),
            Input::Keyword(k) => k,
            Input::Point(_) => return Err(other("Editing the array with grips is not available yet; choose an option.")),
            _ => return Err(other("Invalid option keyword.")),
        };
        self.stage = match k.as_str() {
            "eXit" => return self.finish(s),
            "Base point" => PStage::Base,
            "Items" => PStage::Items,
            "Angle between" => PStage::Between,
            "Fill angle" => PStage::Fill,
            "ROTate items" => PStage::Rotate,
            "ROWs" => {
                s.echo("Rows in polar arrays are not available yet.");
                PStage::Main
            }
            "Levels" => {
                s.echo(NO_LEVELS);
                PStage::Main
            }
            _ => {
                s.echo(NOT_ASSOCIATIVE);
                PStage::Main
            }
        };
        Ok(Step::Continue)
    }
}

impl Interactive for PolarM {
    fn name(&self) -> &'static str {
        "ARRAYPOLAR"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        self.sel = SelectPhase::begin(s);
        if self.sel.done {
            self.objs = self.sel.picked.clone();
            return Ok(self.ready(s));
        }
        Ok(Step::Continue)
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        use PStage::*;
        if !self.sel.done {
            return self.sel.prompt();
        }
        match self.stage {
            Center => Prompt::new("Specify center point of array", Accept::POINT).kw(&["Base point", "Axis of rotation"]),
            CenterBase | Base => Prompt::new("Specify base point", Accept::POINT).kw(&["Key point"]).default("centroid"),
            Main => Prompt::new("Select grip to edit array", Accept::POINT)
                .kw(&["ASsociative", "Base point", "Items", "Angle between", "Fill angle", "ROWs", "Levels", "ROTate items", "eXit"])
                .default("eXit"),
            Items => Prompt::new("Enter number of items in array", Accept::NUMBER).default(self.items.to_string()),
            Between => Prompt::new("Specify the angle between items", Accept::NUMBER).default(fmt(self.between())),
            Fill => Prompt::new("Specify the angle to fill (+=ccw, -=cw)", Accept::NUMBER).default(fmt(self.fill)),
            Rotate => Prompt::new("Rotate arrayed items?", KEYWORDS).kw(&["Yes", "No"]).default(if self.rotate { "Yes" } else { "No" }),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        use PStage::*;
        if !self.sel.done {
            return match self.sel.feed(s, &i)? {
                SelOutcome::More => Ok(Step::Continue),
                SelOutcome::Empty => Ok(Step::Done),
                SelOutcome::Done(hs) => {
                    self.objs = hs;
                    Ok(self.ready(s))
                }
            };
        }
        let stage = self.stage;
        let next = match (stage, &i) {
            (Main, _) => return self.main(s, i),
            (Center, Input::Point(p)) => {
                self.center = *p;
                Main
            }
            (Center, Input::Keyword(k)) if k == "Base point" => CenterBase,
            (Center, Input::Keyword(_)) => {
                s.echo("A rotation axis (3D) is not available yet; specify the center point.");
                Center
            }
            (Center, _) => return Err(other("Requires a point or an option keyword.")),
            (CenterBase | Base, Input::Point(p)) => {
                self.base = Some(*p);
                if stage == CenterBase { Center } else { Main }
            }
            (CenterBase | Base, Input::Enter) => {
                self.base = None;
                if stage == CenterBase { Center } else { Main }
            }
            (CenterBase | Base, Input::Keyword(_)) => {
                s.echo("Key points are not available yet; specify a point.");
                stage
            }
            (Items, _) => match count(&i, self.items, max_count(s, 1, self.objs.len()))? {
                Some(n) => {
                    self.items = n;
                    Main
                }
                None => stage,
            },
            (Between, Input::Enter) => Main,
            (Between, _) => match angle(&i, self.between())? {
                Some(b) => {
                    if b.abs() < 1e-9 {
                        return Err(other("Requires a nonzero angle."));
                    }
                    let n = self.items as f64;
                    let fill = if (b.abs() * n - 360.0).abs() < 1e-9 { 360.0 * b.signum() } else { b * (n - 1.0).max(1.0) };
                    if fill.abs() > 360.0 + 1e-9 {
                        return Err(other("The items would span more than 360 degrees; enter a smaller angle or fewer items."));
                    }
                    self.fill = fill;
                    Main
                }
                None => stage,
            },
            (Fill, _) => match angle(&i, self.fill)? {
                Some(f) => {
                    if f.abs() < 1e-9 || f.abs() > 360.0 + 1e-9 {
                        return Err(other("Requires a fill angle between -360 and 360 degrees (not 0)."));
                    }
                    self.fill = f.clamp(-360.0, 360.0);
                    Main
                }
                None => stage,
            },
            (Rotate, _) => match yes_no(&i, self.rotate) {
                Some(r) => {
                    self.rotate = r;
                    Main
                }
                None => return Err(other("Requires Yes or No.")),
            },
            _ => stage,
        };
        self.stage = next;
        Ok(Step::Continue)
    }
    fn preview(&self, s: &Session, cursor: Vec2) -> Vec<EntityKind> {
        if !self.sel.done {
            return Vec::new();
        }
        let center = match self.stage {
            PStage::Center | PStage::CenterBase => cursor,
            _ => self.center,
        };
        preview_copies(s, &self.objs, self.mats(s, center))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn polar_angles() {
        // Full circle: 4 items 90° apart; partial: the last item sits at the fill angle.
        let a: Vec<Vec2> = polar_mats(Vec2::ZERO, 4, 360.0, true, Vec2::X).map(|m| m.apply(Vec2::X)).collect();
        assert_eq!(a.len(), 3);
        assert!(a[0].near(Vec2::new(0.0, 1.0), 1e-9) && a[2].near(Vec2::new(0.0, -1.0), 1e-9));
        let b: Vec<Vec2> = polar_mats(Vec2::ZERO, 3, -180.0, true, Vec2::X).map(|m| m.apply(Vec2::X)).collect();
        assert!(b[0].near(Vec2::new(0.0, -1.0), 1e-9) && b[1].near(Vec2::new(-1.0, 0.0), 1e-9));
        // A full clockwise circle does not put the last item on top of the source.
        let c: Vec<Vec2> = polar_mats(Vec2::ZERO, 4, -360.0, true, Vec2::X).map(|m| m.apply(Vec2::X)).collect();
        assert!(c[2].near(Vec2::new(0.0, 1.0), 1e-9));
    }
}
