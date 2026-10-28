//! Array grips: `array.grips` lists the count and spacing grips of an associative array,
//! `array.grip` drags one to a point or gives it a value (one undo step), and
//! [`Session::start_array_grip`] starts the drag the UI's click on a grip begins, with the array
//! rebuilt under the cursor as its preview. The geometry is [`ArrayParams::grips`] and
//! [`ArrayParams::with_grip`].

use cadcraft_doc::{ArrayGripInput, ArrayGripKind, ArrayParams, Entity, EntityKind, Handle};
use cadcraft_geom::{Mat3, Vec2};
use serde_json::{Value, json};

use super::super::machines::number;
use super::super::{CommandSpec, bad, has_doc, point_param, str_param};
use super::edit::{array_of, rebuild, refuse_while_editing};
use super::{PREVIEW_LIMIT, max_count, other};
use crate::{Accept, EngineError, Input, Interactive, Prompt, Result, Running, Session, Step};

/// Arrays of more items than this show no preview while a grip is dragged (the rubber band
/// still shows).
const MAX_PREVIEW_ITEMS: u64 = 100_000;

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("array.grips", "Array Grips", run_grips)
            .params("{handle} → [{grip: columnCount|columnSpacing|rowCount|rowSpacing|rowColumnCount|itemCount|itemAngle, at: [x,y], dir: [x,y]}] (empty: not an associative array)")
            .enabled(has_doc)
            .noundo(),
        CommandSpec::new("array.grip", "Drag Array Grip", run_grip)
            .params(
                "{handle, grip (as array.grips names it), to?: [x,y] (drawing coordinates) | value?: number (a count, a distance in the array's units, or the angle between polar items in degrees)} → {handle}: count grips set the number of columns, rows or items, spacing grips the distance between them",
            )
            .enabled(has_doc),
    ]
}

/// What a grip edit is given, in drawing coordinates.
#[derive(Clone, Copy, Debug)]
enum Given {
    Point(Vec2),
    Value(f64),
}

/// The parameters of array `h` after grip `kind` gets `given`, and the transform from the array
/// block to the drawing.
fn edited(s: &Session, h: Handle, kind: ArrayGripKind, given: Given) -> Result<(ArrayParams, Mat3)> {
    let d = s.doc()?;
    let (ins, blk, p) = array_of(d, h)?;
    let w = ins.transform(blk.base.xy());
    let input = match given {
        Given::Point(at) => ArrayGripInput::Point(w.inverse().ok_or_else(|| other("The array's scale is zero."))?.apply(at)),
        Given::Value(v) => ArrayGripInput::Value(v),
    };
    let (cols, rows) = p.grid();
    let objs = d.block(&p.source).map_or(1, |b| b.entities.len());
    let levels = u64::from(p.levels);
    // The most items one direction may have with the others as they are (MAXARRAY).
    let others = match kind {
        ArrayGripKind::ColumnCount | ArrayGripKind::ItemCount | ArrayGripKind::ItemAngle => u64::from(rows).saturating_mul(levels),
        ArrayGripKind::RowCount => u64::from(cols).saturating_mul(levels),
        _ => levels,
    };
    let mut cap = max_count(s, others, objs);
    if kind == ArrayGripKind::RowColumnCount {
        // Both counts change: each gets the square root of what the limit allows.
        cap = (cap as f64).sqrt().floor().max(1.0) as u64;
    }
    let cap = u32::try_from(cap).unwrap_or(u32::MAX);
    let q = p.with_grip(kind, input, cap).ok_or_else(|| match given {
        Given::Value(_) if kind.is_count() => other("Requires a whole number of items within the array's limits."),
        Given::Value(_) if kind == ArrayGripKind::ItemAngle => other("Requires a nonzero angle that keeps the items within a whole circle."),
        Given::Value(_) => other("Requires a nonzero distance."),
        Given::Point(_) => other("The array can't be changed to that point."),
    })?;
    Ok((q, w))
}

/// Give grip `kind` of array `h` a point or a value (one change of the drawing).
fn apply(s: &mut Session, h: Handle, kind: ArrayGripKind, given: Given) -> Result<()> {
    refuse_while_editing(s)?;
    if super::super::curves::is_locked(s, h) {
        return Err(other("The array is on a locked layer."));
    }
    let (q, _) = edited(s, h, kind, given)?;
    rebuild(s, h, q, None)
}

fn run_grips(s: &mut Session, p: &Value) -> Result<Value> {
    let h = super::super::curves::handle_param(p, "handle").ok_or_else(|| bad("array.grips", "`handle` is required"))?;
    serde_json::to_value(s.doc()?.array_grips(h)).map_err(|e| bad("array.grips", e.to_string()))
}

fn run_grip(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "array.grip";
    let h = super::super::curves::handle_param(p, "handle").ok_or_else(|| bad(cmd, "`handle` (the array) is required"))?;
    let kind = str_param(p, "grip").and_then(ArrayGripKind::from_name).ok_or_else(|| {
        let names: Vec<&str> = ArrayGripKind::ALL.iter().map(|k| k.name()).collect();
        bad(cmd, format!("`grip` must be one of {}", names.join(", ")))
    })?;
    let given = match (point_param(p, "to"), p.get("value").filter(|v| !v.is_null())) {
        (Some(to), None) => Given::Point(to),
        (None, Some(v)) => Given::Value(v.as_f64().filter(|v| v.is_finite()).ok_or_else(|| bad(cmd, "`value` must be a number"))?),
        _ => return Err(bad(cmd, "give either `to` [x, y] or `value`")),
    };
    if s.doc()?.array_grips(h).iter().all(|g| g.kind != kind) {
        return Err(bad(cmd, format!("the array has no {} grip", kind.name())));
    }
    apply(s, h, kind, given)?;
    Ok(json!({ "handle": h.hex() }))
}

/// An array grip being dragged.
struct ArrayGripEdit {
    handle: Handle,
    kind: ArrayGripKind,
    /// Where the grip is (the rubber band's start).
    from: Vec2,
}

impl Interactive for ArrayGripEdit {
    fn name(&self) -> &'static str {
        "GRIP"
    }

    fn begin(&mut self, _s: &mut Session) -> Result<Step> {
        Ok(Step::Continue)
    }

    fn prompt(&self, _s: &Session) -> Prompt {
        use ArrayGripKind::*;
        let (msg, accept) = match self.kind {
            ColumnCount => ("** COLUMN COUNT ** Specify number of columns", Accept::POINT_OR_NUMBER),
            ColumnSpacing => ("** COLUMN SPACING ** Specify the distance between columns", Accept::POINT_OR_NUMBER),
            RowCount => ("** ROW COUNT ** Specify number of rows", Accept::POINT_OR_NUMBER),
            RowSpacing => ("** ROW SPACING ** Specify the distance between rows", Accept::POINT_OR_NUMBER),
            RowColumnCount => ("** ROW AND COLUMN COUNT ** Specify number of rows and columns", Accept::POINT),
            ItemCount => ("** ITEM COUNT ** Specify number of items", Accept::POINT_OR_NUMBER),
            ItemAngle => ("** ANGLE BETWEEN ITEMS ** Specify the angle between items", Accept::POINT_OR_NUMBER),
        };
        Prompt::new(msg, accept).kw(&["eXit"]).base(self.from)
    }

    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        let given = match i {
            Input::Point(p) => Given::Point(p),
            Input::Deferred(d) => Given::Point(d.at),
            Input::Text(t) if self.kind.takes_value() => Given::Value(number(&t).ok_or_else(|| other("Requires a number or a point."))?),
            Input::Keyword(k) if k == "eXit" => return Ok(Step::Done),
            // Other grip-edit keywords (Copy, which Ctrl+click sends) don't apply to an array.
            Input::Keyword(_) => return Ok(Step::Continue),
            Input::Enter | Input::Cancel => return Ok(Step::Done),
            _ => return Err(other("Requires a point.")),
        };
        apply(s, self.handle, self.kind, given)?;
        Ok(Step::Done)
    }

    /// The rubber band from the grip.
    fn preview(&self, _s: &Session, c: Vec2) -> Vec<EntityKind> {
        vec![crate::cmd::helpers::line(self.from, c)]
    }

    /// The array rebuilt for the cursor: each item's objects where they will go, with the
    /// array's properties.
    fn preview_objects(&self, s: &Session, c: Vec2) -> Vec<Entity> {
        let Ok((q, w)) = edited(s, self.handle, self.kind, Given::Point(c)) else { return Vec::new() };
        let Ok(d) = s.doc() else { return Vec::new() };
        let Some(array) = d.entity(self.handle) else { return Vec::new() };
        if q.item_count() > MAX_PREVIEW_ITEMS {
            return Vec::new();
        }
        let mut out = Vec::new();
        for it in q.items().unwrap_or_default() {
            let Some(b) = d.block(q.block_for(it.i, it.j, it.level)) else { continue };
            let m = w.then_before(Mat3::translate(it.insert.xy())).then_before(Mat3::rotate(it.rotation)).then_before(Mat3::translate(-b.base.xy()));
            for e in b.entities.iter().filter(|e| e.common.visible && !matches!(e.kind, EntityKind::AttDef(_))) {
                if out.len() >= PREVIEW_LIMIT {
                    return out;
                }
                let mut kind = e.kind.clone();
                kind.transform(&m);
                out.push(Entity { handle: Handle(0), common: crate::cmd::blocks::dyngrip::under(&e.common, &array.common), kind });
            }
        }
        out
    }
}

impl Session {
    /// Start dragging grip `kind` of associative array `handle` (the UI's click on an array
    /// grip): a point or a typed value finishes it (one undo step).
    pub fn start_array_grip(&mut self, handle: Handle, kind: ArrayGripKind) -> Result<()> {
        if self.running.is_some() {
            self.cancel();
        }
        refuse_while_editing(self)?;
        let g = self.doc()?.array_grips(handle).into_iter().find(|g| g.kind == kind).ok_or_else(|| EngineError::Other("no such grip".into()))?;
        let edit = ArrayGripEdit { handle, kind, from: g.at };
        let machine = crate::guard::Guarded::wrap(Box::new(edit));
        let st = self.state()?;
        let (before, selection_before) = (st.doc.clone(), st.selection.clone());
        self.running = Some(Running { id: "grip".into(), machine, before, selection_before });
        self.pending_window = None;
        self.point_mods.clear();
        self.echo(format!("** {} **", kind.label().to_uppercase()));
        Ok(())
    }
}
