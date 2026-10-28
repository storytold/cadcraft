//! Array commands: ARRAY, ARRAYRECT and ARRAYPOLAR (ARRAYPATH lives in `modify2`).
//!
//! Arrays are associative by default (ARRAYASSOCIATIVITY): the source objects move into an
//! anonymous source block and the array becomes one block reference to an anonymous array block
//! holding a reference per item, with the array parameters kept on that block
//! ([`cadcraft_doc::ArrayParams`]). With ASsociative = No the items are separate copies. The
//! interactive forms select objects, then edit the array in an option loop with a live preview,
//! and create the array on eXit (or Enter).

use std::collections::HashMap;
use std::sync::Arc;

use cadcraft_doc::{ArrayKind, ArrayParams, Block, Drawing, Entity, EntityKind, Handle, Insert, MAX_BLOCK_DEPTH, PolarArray, RectArray};
use cadcraft_geom::{Bounds2, Mat3, Vec2, Vec3};
use serde_json::{Value, json};

use super::helpers::{line, v3};
use super::machines::{SelOutcome, SelectPhase, number};
use super::modify::transform_entities;
use super::{CommandSpec, bad, bool_or, f64_or, find_command, point_param, point_req, str_param, targets};

use crate::{Accept, EngineError, Input, Interactive, Prompt, Result, Session, Step};

mod edit;

/// Most preview entities drawn while the array is edited.
const PREVIEW_LIMIT: usize = 2000;
/// Keyword-only prompts: each typed token is matched on its own (no free text).
const KEYWORDS: Accept = Accept { point: false, number: false, text: false, select: false, enter: true };

const FLAT_LEVELS: &str = "Levels need an associative array; the copies are made on one level.";

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("array", "Array", run_array)
            .alias(&["ar"])
            .params("{type?: \"rectangular\" (default) | \"polar\" | \"path\", ...the parameters of arrayrect, arraypolar or arraypath}")
            .interactive(|_| Ok(Box::new(ArrayM::default()))),
        CommandSpec::new("arrayrect", "Rectangular Array", run_arrayrect)
            .menu(&["Modify", "Array", "Rectangular Array"])
            .params(
                "{handles?, rows, cols, rowSpacing, colSpacing, levels? (1), levelSpacing?, rowElevation?, axisAngle? (degrees), base? ([x, y], default the objects' centre), associative?: bool (default ARRAYASSOCIATIVITY, on)} → {handle (the array, when associative), created (copies)}",
            )
            .interactive(|_| Ok(Box::new(RectM::default()))),
        CommandSpec::new("arraypolar", "Polar Array", run_arraypolar)
            .menu(&["Modify", "Array", "Polar Array"])
            .params(
                "{handles?, center, count, angle? (degrees, default 360), rotate?: bool, rows? (1), rowSpacing?, rowElevation?, levels? (1), levelSpacing?, base? ([x, y], default the first object's centre), associative?: bool (default ARRAYASSOCIATIVITY, on)} → {handle (the array, when associative), created (copies)}",
            )
            .interactive(|_| Ok(Box::new(PolarM::default()))),
    ]
    .into_iter()
    .chain(edit::specs())
    .collect()
}

// ---------------- geometry ----------------

/// Array parameters around `kind` (the source block is named when the array is made).
fn params(kind: ArrayKind, levels: u64, level_spacing: f64) -> ArrayParams {
    ArrayParams { source: "*".into(), kind, levels: u32::try_from(levels).unwrap_or(u32::MAX), level_spacing, replaced: Vec::new(), editing: None }
}

fn count32(n: u64) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// World transforms of the items of `p` with its base point at `base`, except the first item
/// (the source objects themselves) and items above the first level (copies are flat).
fn item_mats(p: &ArrayParams, base: Vec2) -> Vec<Mat3> {
    p.items()
        .unwrap_or_default()
        .into_iter()
        .filter(|it| it.level == 0 && (it.i, it.j) != (0, 0))
        .map(|it| Mat3::translate(base + it.insert.xy()).then_before(Mat3::rotate(it.rotation)).then_before(Mat3::translate(-base)))
        .collect()
}

/// How deeply block references nest in `ents` (0 without any), at most `MAX_BLOCK_DEPTH + 1`.
fn nesting(d: &Drawing, ents: &[&Entity], memo: &mut HashMap<String, usize>, depth: usize) -> usize {
    let mut most = 0;
    for e in ents {
        let EntityKind::Insert(i) = &e.kind else { continue };
        let key = i.block.to_ascii_uppercase();
        let n = match memo.get(&key) {
            Some(n) => *n,
            None if depth > MAX_BLOCK_DEPTH => MAX_BLOCK_DEPTH + 1,
            None => {
                // Cycles end here: a block being measured counts as too deep.
                memo.insert(key.clone(), MAX_BLOCK_DEPTH + 1);
                let inner: Vec<&Entity> = d.block(&i.block).map(|b| b.entities.iter().map(|e| e.as_ref()).collect()).unwrap_or_default();
                let n = (1 + nesting(d, &inner, memo, depth + 1)).min(MAX_BLOCK_DEPTH + 1);
                memo.insert(key, n);
                n
            }
        };
        most = most.max(n);
    }
    most
}

/// ARRAYASSOCIATIVITY: whether new arrays start associative (on unless set to 0).
fn assoc_default(s: &Session) -> bool {
    s.doc().map(|d| d.header.i64("ARRAYASSOCIATIVITY", 1) != 0).unwrap_or(true)
}

/// Make `objs` an associative array with its base point at `base`: the objects move into a new
/// source block, the items into a new array block, and one block reference to the array block
/// takes their place. Returns that reference.
pub(crate) fn make_assoc(s: &mut Session, objs: &[Handle], base: Vec2, mut p: ArrayParams) -> Result<Handle> {
    let d = s.doc()?;
    let objs: Vec<Handle> = objs.iter().copied().filter(|h| !super::curves::is_locked(s, *h)).collect();
    let ents: Vec<Entity> = objs.iter().filter_map(|h| d.entity(*h).map(|e| (**e).clone())).collect();
    if ents.is_empty() {
        return Err(other("No objects to array (objects on locked layers are left out)."));
    }
    // The array adds two levels of block references (array block, then source block).
    let refs: Vec<&Entity> = ents.iter().collect();
    if nesting(d, &refs, &mut HashMap::new(), 0) + 2 > MAX_BLOCK_DEPTH {
        return Err(other(format!("The objects nest block references too deeply to array (more than {MAX_BLOCK_DEPTH} levels).")));
    }
    if !base.is_finite() {
        return Err(other("The array base point is not a valid point."));
    }
    let d = s.doc_mut()?;
    let src_name = d.anonymous_block_name("*U");
    let mut src = Block::new(&src_name);
    let to_local = Mat3::translate(-base);
    for mut e in ents {
        e.handle = d.new_handle();
        e.kind.transform(&to_local);
        src.entities.push(e);
    }
    d.blocks.insert(src_name.clone(), Arc::new(src));
    p.source = src_name;
    let items = p.item_entities(d).ok_or_else(|| other("The array parameters are not valid."))?;
    let name = d.anonymous_block_name("*U");
    let mut blk = Block::new(&name);
    for e in items {
        blk.entities.push(e);
    }
    blk.array = Some(p);
    d.blocks.insert(name.clone(), Arc::new(blk));
    for h in &objs {
        d.remove_entity(*h);
    }
    s.add_entity(EntityKind::Insert(Insert {
        block: name,
        insert: v3(base),
        scale: Vec3::new(1.0, 1.0, 1.0),
        rotation: 0.0,
        attribs: Vec::new(),
        cols: 1,
        rows: 1,
        col_spacing: 0.0,
        row_spacing: 0.0,
    }))
}

/// Create the array: associative (one block reference) or separate copies. Returns the JSON
/// result: the array's handle when associative, and the number of copies the array adds.
fn create(s: &mut Session, objs: &[Handle], base: Vec2, p: ArrayParams, assoc: bool) -> Result<Value> {
    let copies = p.item_count().saturating_sub(1).saturating_mul(objs.len() as u64);
    if assoc {
        let h = make_assoc(s, objs, base, p)?;
        return Ok(json!({ "handle": h.hex(), "created": copies, "associative": true }));
    }
    if p.levels > 1 {
        s.echo(FLAT_LEVELS);
    }
    let n = copy_all(s, objs, item_mats(&p, base).into_iter())?;
    Ok(json!({ "created": n }))
}

/// EXPLODE of an associative array: every item becomes the objects of its block (the source
/// objects, or a replacement block's reference), in world coordinates. `None` for other objects.
pub(crate) fn explode_array(d: &Drawing, e: &Entity) -> Option<Vec<Entity>> {
    let EntityKind::Insert(ins) = &e.kind else { return None };
    let p = d.array_params(e)?;
    let blk = d.block(&ins.block)?;
    let m = ins.transform(blk.base.xy());
    let mut out = Vec::new();
    for item in blk.entities.iter() {
        let item = placed(e, item, &m);
        let source = match &item.kind {
            EntityKind::Insert(i) if i.block.eq_ignore_ascii_case(&p.source) => d.block(&i.block).map(|b| (i.transform(b.base.xy()), b)),
            _ => None,
        };
        match source {
            Some((im, b)) => out.extend(b.entities.iter().map(|se| placed(&item, se, &im))),
            None => out.push(item),
        }
    }
    Some(out)
}

/// `child` of a block referenced by `parent`, moved by `m`, with layer 0 and ByBlock properties
/// taken from the reference (as EXPLODE does).
fn placed(parent: &Entity, child: &Entity, m: &Mat3) -> Entity {
    let mut ne = child.clone();
    ne.kind.transform(m);
    if ne.common.layer == "0" {
        ne.common.layer = parent.common.layer.clone();
    }
    if ne.common.color == cadcraft_color::Color::ByBlock {
        ne.common.color = parent.common.color;
    }
    if ne.common.linetype.eq_ignore_ascii_case("ByBlock") {
        ne.common.linetype = parent.common.linetype.clone();
    }
    if ne.common.lineweight == cadcraft_doc::Lineweight::ByBlock {
        ne.common.lineweight = parent.common.lineweight;
    }
    ne
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

/// An angle in degrees typed at a prompt in AUNITS (Enter keeps `cur`); `None` for inputs the
/// prompt ignores.
fn angle(i: &Input, cur: f64, au: &crate::units::AngleSettings) -> Result<Option<f64>> {
    match i {
        Input::Enter => Ok(Some(cur)),
        Input::Text(t) => au.amount(t).map(|a| Some(a.to_degrees())).ok_or_else(|| other("Requires a valid angle in degrees.")),
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

/// A count parameter (at least 1).
fn count_or(p: &Value, key: &str, default: u64) -> u64 {
    p.get(key).and_then(Value::as_u64).unwrap_or(default).max(1)
}

fn run_arrayrect(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    let rows = count_or(p, "rows", 3);
    let cols = count_or(p, "cols", 4);
    let levels = count_or(p, "levels", 1);
    check_size(s, rows.saturating_mul(cols).saturating_mul(levels), hs.len())?;
    let kind = ArrayKind::Rect(RectArray {
        cols: count32(cols),
        rows: count32(rows),
        col_spacing: f64_or(p, "colSpacing", 1.0),
        row_spacing: f64_or(p, "rowSpacing", 1.0),
        axis_angle: f64_or(p, "axisAngle", 0.0).to_radians(),
        row_elevation: f64_or(p, "rowElevation", 0.0),
    });
    let rs = f64_or(p, "rowSpacing", 1.0);
    let base = match point_param(p, "base") {
        Some(b) => b,
        None => Some(bounds(s.doc()?, &hs).center()).filter(|c| c.is_finite()).unwrap_or_default(),
    };
    let p2 = params(kind, levels, f64_or(p, "levelSpacing", rs));
    let associative = bool_or(p, "associative", assoc_default(s));
    create(s, &hs, base, p2, associative)
}

fn run_arraypolar(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    let c = point_req("arraypolar", p, "center")?;
    let n = count_or(p, "count", 6);
    let rows = count_or(p, "rows", 1);
    let levels = count_or(p, "levels", 1);
    check_size(s, n.saturating_mul(rows).saturating_mul(levels), hs.len())?;
    let base = point_param(p, "base").or_else(|| s.doc().ok().and_then(|d| first_center(d, &hs))).unwrap_or(c);
    let row_spacing = f64_or(p, "rowSpacing", 1.0);
    let kind = ArrayKind::Polar(PolarArray {
        center: c - base,
        items: count32(n),
        fill: f64_or(p, "angle", 360.0),
        rotate: bool_or(p, "rotate", true),
        rows: count32(rows),
        row_spacing,
        row_elevation: f64_or(p, "rowElevation", 0.0),
    });
    let p2 = params(kind, levels, f64_or(p, "levelSpacing", row_spacing));
    let associative = bool_or(p, "associative", assoc_default(s));
    create(s, &hs, base, p2, associative)
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
    fn drag(&self, s: &Session, cursor: Vec2) -> Option<crate::prompt::Drag> {
        self.inner.as_ref().and_then(|m| m.drag(s, cursor))
    }
}

// ---------------- ARRAYRECT ----------------

#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum RStage {
    #[default]
    Main,
    Assoc,
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
    /// Levels: count, then spacing (or the total).
    LevelsCount,
    LevelsSpace,
    LevelsTotal,
}

/// ARRAYRECT: select objects, then edit columns/rows/levels/spacing in a loop; eXit creates the
/// array.
#[derive(Default)]
struct RectM {
    sel: SelectPhase,
    objs: Vec<Handle>,
    stage: RStage,
    assoc: bool,
    cols: u64,
    rows: u64,
    levels: u64,
    cs: f64,
    rs: f64,
    /// Distance between levels and the elevation added per row.
    ls: f64,
    elev: f64,
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
        (self.cols, self.rows, self.levels, self.cs, self.rs) = (4, 3, 1, size(w, h), size(h, w));
        (self.ls, self.elev) = (self.rs, 0.0);
        self.assoc = assoc_default(s);
        self.centroid = if b.center().is_finite() { b.center() } else { Vec2::ZERO };
        self.base = self.centroid;
        self.stage = RStage::Main;
        s.echo(format!("Type = Rectangular  Associative = {}", yes(self.assoc)));
        Ok(Step::Continue)
    }

    fn params(&self) -> ArrayParams {
        let kind = ArrayKind::Rect(RectArray {
            cols: count32(self.cols),
            rows: count32(self.rows),
            col_spacing: self.cs,
            row_spacing: self.rs,
            axis_angle: 0.0,
            row_elevation: self.elev,
        });
        params(kind, if self.assoc { self.levels } else { 1 }, self.ls)
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
        let levels = if self.assoc { self.levels } else { 1 };
        check_size(s, self.rows.saturating_mul(self.cols).saturating_mul(levels), self.objs.len())?;
        if !self.assoc && self.levels > 1 {
            s.echo(FLAT_LEVELS);
        }
        create(s, &self.objs.clone(), self.base, self.params(), self.assoc)?;
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
            "ASsociative" => RStage::Assoc,
            "Base point" => RStage::Base,
            "COUnt" => RStage::CountCols,
            "Spacing" => RStage::SpaceCols,
            "COLumns" => RStage::ColsCount,
            "Rows" => RStage::RowsCount,
            "Levels" => RStage::LevelsCount,
            _ => return Err(other("Invalid option keyword.")),
        };
        Ok(Step::Continue)
    }
}

fn yes(b: bool) -> &'static str {
    if b { "Yes" } else { "No" }
}

/// The ASsociative prompt.
fn assoc_prompt(cur: bool) -> Prompt {
    Prompt::new("Create associative array", KEYWORDS).kw(&["Yes", "No"]).default(yes(cur))
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
            Assoc => assoc_prompt(self.assoc),
            Base => Prompt::new("Specify base point", Accept::POINT).kw(&["Key point"]).default("centroid"),
            CountCols | ColsCount => cols(),
            CountRows | RowsCount => rows(),
            SpaceCols => dist("Specify the distance between columns", self.cs).kw(&["Unit cell"]),
            SpaceRows => dist("Specify the distance between rows", self.rs),
            UnitCell => match self.corner {
                None => Prompt::new("Specify first corner of the unit cell", Accept::POINT).base(self.base),
                Some(c) => Prompt::new("Specify other corner", Accept::POINT).corner(c),
            },
            ColsSpace => dist("Specify the distance between columns", self.cs).kw(&["Total"]),
            ColsTotal => dist("Specify the distance between the first and last column", self.cs * self.cols.saturating_sub(1) as f64),
            RowsSpace => dist("Specify the distance between rows", self.rs).kw(&["Total"]),
            RowsTotal => dist("Specify the distance between the first and last row", self.rs * self.rows.saturating_sub(1) as f64),
            RowsElev => Prompt::new("Specify the incrementing elevation between rows", Accept::NUMBER).default(fmt(self.elev)),
            LevelsCount => Prompt::new("Enter the number of levels", Accept::NUMBER).default(self.levels.to_string()),
            LevelsSpace => dist("Specify the distance between levels", self.ls).kw(&["Total"]),
            LevelsTotal => dist("Specify the distance between the first and last level", self.ls * self.levels.saturating_sub(1) as f64),
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
        let n = self.objs.len();
        let next = match (stage, &i) {
            (Main, _) => return self.main(s, i),
            (Assoc, _) => match yes_no(&i, self.assoc) {
                Some(a) => {
                    self.assoc = a;
                    Main
                }
                None => return Err(other("Requires Yes or No.")),
            },
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
            (CountCols | ColsCount, _) => match count(&i, self.cols, max_count(s, self.rows.saturating_mul(self.levels), n))? {
                Some(c) => {
                    self.cols = c;
                    if stage == CountCols { CountRows } else { ColsSpace }
                }
                None => stage,
            },
            (CountRows | RowsCount, _) => match count(&i, self.rows, max_count(s, self.cols.saturating_mul(self.levels), n))? {
                Some(r) => {
                    self.rows = r;
                    if stage == CountRows { Main } else { RowsSpace }
                }
                None => stage,
            },
            (LevelsCount, _) => match count(&i, self.levels, max_count(s, self.cols.saturating_mul(self.rows), n))? {
                Some(l) => {
                    self.levels = l;
                    LevelsSpace
                }
                None => stage,
            },
            (SpaceCols, Input::Keyword(_)) => {
                self.corner = None;
                UnitCell
            }
            (ColsSpace, Input::Keyword(_)) => ColsTotal,
            (RowsSpace, Input::Keyword(_)) => RowsTotal,
            (LevelsSpace, Input::Keyword(_)) => LevelsTotal,
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
            (LevelsSpace, _) => match self.distance(&i, self.ls)? {
                Some(d) => {
                    self.ls = d;
                    if !self.assoc && self.levels > 1 {
                        s.echo(FLAT_LEVELS);
                    }
                    Main
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
            (LevelsTotal, _) => match self.distance(&i, self.ls * self.levels.saturating_sub(1) as f64)? {
                Some(t) => {
                    self.ls = t / self.levels.saturating_sub(1).max(1) as f64;
                    Main
                }
                None => stage,
            },
            (RowsElev, Input::Enter) => Main,
            (RowsElev, Input::Text(t)) => match number(t) {
                Some(z) if z.is_finite() => {
                    self.elev = z;
                    if !self.assoc && z.abs() > 1e-12 {
                        s.echo("Row elevations need an associative array; the copies stay at the same elevation.");
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
        let mut out = preview_copies(s, &self.objs, item_mats(&self.params(), self.base).into_iter());
        match (self.stage, self.corner) {
            (RStage::UnitCell, Some(c)) => out.push(line(c, cursor)),
            (
                RStage::SpaceCols
                | RStage::SpaceRows
                | RStage::ColsSpace
                | RStage::ColsTotal
                | RStage::RowsSpace
                | RStage::RowsTotal
                | RStage::LevelsSpace
                | RStage::LevelsTotal,
                _,
            ) => out.push(line(self.base, cursor)),
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
    Assoc,
    Base,
    Items,
    Between,
    Fill,
    Rotate,
    /// ROWs: count, then spacing (or the total), then the elevation increment.
    RowsCount,
    RowsSpace,
    RowsTotal,
    RowsElev,
    /// Levels: count, then spacing (or the total).
    LevelsCount,
    LevelsSpace,
    LevelsTotal,
}

/// ARRAYPOLAR: select objects, specify the center, then edit items/angles/rows/levels in a loop;
/// eXit creates the array.
struct PolarM {
    sel: SelectPhase,
    objs: Vec<Handle>,
    stage: PStage,
    assoc: bool,
    center: Vec2,
    items: u64,
    /// Fill angle in degrees (positive = counterclockwise).
    fill: f64,
    rotate: bool,
    rows: u64,
    /// Distance between rows (rings), the elevation added per row, and the distance between levels.
    rs: f64,
    elev: f64,
    levels: u64,
    ls: f64,
    /// Base point: the point non-rotated copies are moved by (default: the first object's centre).
    base: Option<Vec2>,
}

impl Default for PolarM {
    fn default() -> Self {
        PolarM {
            sel: SelectPhase::default(),
            objs: Vec::new(),
            stage: PStage::Center,
            assoc: true,
            center: Vec2::ZERO,
            items: 6,
            fill: 360.0,
            rotate: true,
            rows: 1,
            rs: 1.0,
            elev: 0.0,
            levels: 1,
            ls: 1.0,
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
        self.assoc = assoc_default(s);
        // Rows and levels default to 1.5 × the selection's size apart.
        if let Ok(d) = s.doc() {
            let b = bounds(d, &self.objs);
            let size = b.width().max(b.height());
            if size.is_finite() && size > 1e-9 {
                (self.rs, self.ls) = (size * 1.5, size * 1.5);
            }
        }
        s.echo(format!("Type = Polar  Associative = {}", yes(self.assoc)));
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

    /// The base point: picked, else the first object's centre, else the centre of the array.
    fn base_point(&self, s: &Session, center: Vec2) -> Vec2 {
        self.base.or_else(|| s.doc().ok().and_then(|d| first_center(d, &self.objs))).unwrap_or(center)
    }

    fn params(&self, base: Vec2, center: Vec2) -> ArrayParams {
        let kind = ArrayKind::Polar(PolarArray {
            center: center - base,
            items: count32(self.items),
            fill: self.fill,
            rotate: self.rotate,
            rows: count32(self.rows),
            row_spacing: self.rs,
            row_elevation: self.elev,
        });
        params(kind, if self.assoc { self.levels } else { 1 }, self.ls)
    }

    fn finish(&mut self, s: &mut Session) -> Result<Step> {
        let levels = if self.assoc { self.levels } else { 1 };
        check_size(s, self.items.saturating_mul(self.rows).saturating_mul(levels), self.objs.len())?;
        if !self.assoc && self.levels > 1 {
            s.echo(FLAT_LEVELS);
        }
        let base = self.base_point(s, self.center);
        create(s, &self.objs.clone(), base, self.params(base, self.center), self.assoc)?;
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
            "ASsociative" => PStage::Assoc,
            "Base point" => PStage::Base,
            "Items" => PStage::Items,
            "Angle between" => PStage::Between,
            "Fill angle" => PStage::Fill,
            "ROTate items" => PStage::Rotate,
            "ROWs" => PStage::RowsCount,
            "Levels" => PStage::LevelsCount,
            _ => return Err(other("Invalid option keyword.")),
        };
        Ok(Step::Continue)
    }

    /// A distance typed or picked (from the centre); Enter keeps `cur`.
    fn distance(&self, i: &Input, cur: f64) -> Result<Option<f64>> {
        let d = match i {
            Input::Enter => return Ok(Some(cur)),
            Input::Text(t) => number(t),
            Input::Point(p) => Some(self.center.dist(*p)),
            _ => return Ok(None),
        };
        d.filter(|d| d.is_finite() && d.abs() > 1e-12).map(Some).ok_or_else(|| other("Requires a nonzero distance."))
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
        let dist = |m: &str, d: f64| Prompt::new(m, Accept::POINT_OR_NUMBER).default(fmt(d)).base(self.center);
        match self.stage {
            Center => Prompt::new("Specify center point of array", Accept::POINT).kw(&["Base point", "Axis of rotation"]),
            CenterBase | Base => Prompt::new("Specify base point", Accept::POINT).kw(&["Key point"]).default("centroid"),
            Main => Prompt::new("Select grip to edit array", Accept::POINT)
                .kw(&["ASsociative", "Base point", "Items", "Angle between", "Fill angle", "ROWs", "Levels", "ROTate items", "eXit"])
                .default("eXit"),
            Assoc => assoc_prompt(self.assoc),
            Items => Prompt::new("Enter number of items in array", Accept::NUMBER).default(self.items.to_string()),
            Between => Prompt::new("Specify the angle between items", Accept::NUMBER).default(fmt(self.between())),
            Fill => Prompt::new("Specify the angle to fill (+=ccw, -=cw)", Accept::NUMBER).default(fmt(self.fill)),
            Rotate => Prompt::new("Rotate arrayed items?", KEYWORDS).kw(&["Yes", "No"]).default(yes(self.rotate)),
            RowsCount => Prompt::new("Enter the number of rows", Accept::NUMBER).default(self.rows.to_string()),
            RowsSpace => dist("Specify the distance between rows", self.rs).kw(&["Total"]),
            RowsTotal => dist("Specify the distance between the first and last row", self.rs * self.rows.saturating_sub(1) as f64),
            RowsElev => Prompt::new("Specify the incrementing elevation between rows", Accept::NUMBER).default(fmt(self.elev)),
            LevelsCount => Prompt::new("Enter the number of levels", Accept::NUMBER).default(self.levels.to_string()),
            LevelsSpace => dist("Specify the distance between levels", self.ls).kw(&["Total"]),
            LevelsTotal => dist("Specify the distance between the first and last level", self.ls * self.levels.saturating_sub(1) as f64),
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
        let n = self.objs.len();
        let next = match (stage, &i) {
            (Main, _) => return self.main(s, i),
            (Assoc, _) => match yes_no(&i, self.assoc) {
                Some(a) => {
                    self.assoc = a;
                    Main
                }
                None => return Err(other("Requires Yes or No.")),
            },
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
            (Items, _) => match count(&i, self.items, max_count(s, self.rows.saturating_mul(self.levels), n))? {
                Some(c) => {
                    self.items = c;
                    Main
                }
                None => stage,
            },
            (RowsCount, _) => match count(&i, self.rows, max_count(s, self.items.saturating_mul(self.levels), n))? {
                Some(c) => {
                    self.rows = c;
                    RowsSpace
                }
                None => stage,
            },
            (LevelsCount, _) => match count(&i, self.levels, max_count(s, self.items.saturating_mul(self.rows), n))? {
                Some(c) => {
                    self.levels = c;
                    LevelsSpace
                }
                None => stage,
            },
            (RowsSpace, Input::Keyword(_)) => RowsTotal,
            (LevelsSpace, Input::Keyword(_)) => LevelsTotal,
            (RowsSpace, _) => match self.distance(&i, self.rs)? {
                Some(d) => {
                    self.rs = d;
                    RowsElev
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
                    self.elev = z;
                    if !self.assoc && z.abs() > 1e-12 {
                        s.echo("Row elevations need an associative array; the copies stay at the same elevation.");
                    }
                    Main
                }
                _ => return Err(other("Requires a number.")),
            },
            (LevelsSpace, _) => match self.distance(&i, self.ls)? {
                Some(d) => {
                    self.ls = d;
                    if !self.assoc && self.levels > 1 {
                        s.echo(FLAT_LEVELS);
                    }
                    Main
                }
                None => stage,
            },
            (LevelsTotal, _) => match self.distance(&i, self.ls * self.levels.saturating_sub(1) as f64)? {
                Some(t) => {
                    self.ls = t / self.levels.saturating_sub(1).max(1) as f64;
                    Main
                }
                None => stage,
            },
            (Between, Input::Enter) => Main,
            (Between, _) => match angle(&i, self.between(), &s.angle_settings())? {
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
            (Fill, _) => match angle(&i, self.fill, &s.angle_settings())? {
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
        let base = self.base_point(s, center);
        preview_copies(s, &self.objs, item_mats(&self.params(base, center), base).into_iter())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn polar(n: u32, fill: f64) -> ArrayParams {
        let kind = ArrayKind::Polar(PolarArray { center: -Vec2::X, items: n, fill, rotate: true, rows: 1, row_spacing: 0.0, row_elevation: 0.0 });
        params(kind, 1, 0.0)
    }

    #[test]
    fn polar_angles() {
        // Base point (1,0) around the origin. Full circle: 4 items 90° apart; partial: the last
        // item sits at the fill angle.
        let at = |p: &ArrayParams| -> Vec<Vec2> { item_mats(p, Vec2::X).iter().map(|m| m.apply(Vec2::X)).collect() };
        let a = at(&polar(4, 360.0));
        assert_eq!(a.len(), 3);
        assert!(a[0].near(Vec2::new(0.0, 1.0), 1e-9) && a[2].near(Vec2::new(0.0, -1.0), 1e-9));
        let b = at(&polar(3, -180.0));
        assert!(b[0].near(Vec2::new(0.0, -1.0), 1e-9) && b[1].near(Vec2::new(-1.0, 0.0), 1e-9));
        // A full clockwise circle does not put the last item on top of the source.
        let c = at(&polar(4, -360.0));
        assert!(c[2].near(Vec2::new(0.0, 1.0), 1e-9));
    }
}
