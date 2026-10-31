//! Synchronous thumbnail preflight. Input, expansion/evaluation work and retained output have
//! separate units. Bounds sampling is charged before bounds; rendering is planned at thumbnail
//! tolerance before allocating curve buffers. No drawing/block names affect these limits.
use super::{Arc, Bounds2, Drawing, EntityKind, Preview, Vec2};
use cadcraft_doc::{Entity, Insert};
use cadcraft_geom::{Polyline, Spline};

// Preserve the existing input/expansion caps. Curve work now counts basis-loop steps instead
// of fractions of a hypothetical 4000-point tessellation. Four million steps admit hundreds
// of ordinary cubic curves while bounding high degrees independently of their input length.
const MAX_INPUT: usize = 32_768;
const MAX_WORK: usize = 4_000_000;
const MAX_PRIMITIVES: usize = 32_768;
const MAX_VERTICES: usize = 262_144;
// Largest tile has a 90-pixel-high drawing area; 320 samples across its fitted extent also
// cover larger UI scaling factors without retaining full-canvas curve detail.
const REFERENCE_PIXELS: f64 = 320.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PreviewFailure {
    Input,
    Computation,
    Output,
    Invalid,
    MissingReference,
}
impl PreviewFailure {
    /// The text shown in the palette for this refusal.
    pub(super) fn message(self) -> &'static str {
        match self {
            Self::Input => crate::tl!("Excessive preview input"),
            Self::Computation => crate::tl!("Excessive preview computation"),
            Self::Output => crate::tl!("Excessive preview output"),
            Self::Invalid => crate::tl!("Invalid preview data"),
            Self::MissingReference => crate::tl!("Missing block reference"),
        }
    }
}
#[derive(Clone)]
pub(super) enum PreviewResult {
    Ready(Preview),
    Empty,
    Unavailable(PreviewFailure),
}
type Checked<T> = Result<T, PreviewFailure>;

#[derive(Clone, Copy, Default, Debug)]
struct Budget {
    input_items: usize,
    text_bytes: usize,
    metadata_bytes: usize,
    expanded: usize,
    computation_steps: usize,
    vertices: usize,
}
/// Adds `b` to `a` with checked arithmetic, failing with `failure` on overflow or past `limit`.
fn add(a: &mut usize, b: usize, limit: usize, failure: PreviewFailure) -> Checked<()> {
    *a = a.checked_add(b).filter(|v| *v <= limit).ok_or(failure)?;
    Ok(())
}
impl Budget {
    /// Charges `n` input items.
    fn items(&mut self, n: usize) -> Checked<()> {
        add(&mut self.input_items, n, MAX_INPUT, PreviewFailure::Input)
    }
    /// Charges `n` bytes of text.
    fn text(&mut self, n: usize) -> Checked<()> {
        add(&mut self.text_bytes, n, MAX_INPUT, PreviewFailure::Input)
    }
    /// Charges `n` bytes of metadata (style names, column heights).
    fn meta(&mut self, n: usize) -> Checked<()> {
        if n > MAX_INPUT {
            return Err(PreviewFailure::Input);
        }
        add(&mut self.metadata_bytes, n, 1024 * 1024, PreviewFailure::Input)
    }
    /// Charges `n` computation steps.
    fn work(&mut self, n: usize) -> Checked<()> {
        add(&mut self.computation_steps, n, MAX_WORK, PreviewFailure::Computation)
    }
    /// Charges `n` output vertices.
    fn points(&mut self, n: usize) -> Checked<()> {
        add(&mut self.vertices, n, MAX_VERTICES, PreviewFailure::Output)
    }
    /// Charges `n` expanded primitives.
    fn expand(&mut self, n: usize) -> Checked<()> {
        add(&mut self.expanded, n, MAX_PRIMITIVES, PreviewFailure::Computation)
    }
    /// Multiplies what was charged since `before` by `copies`, as if the same content were repeated.
    fn copies(&mut self, before: Self, copies: usize) -> Checked<()> {
        let extra = copies.checked_sub(1).ok_or(PreviewFailure::Invalid)?;
        let delta = |after: usize, prior: usize| after.checked_sub(prior).and_then(|v| v.checked_mul(extra)).ok_or(PreviewFailure::Computation);
        self.items(delta(self.input_items, before.input_items)?)?;
        self.text(delta(self.text_bytes, before.text_bytes)?)?;
        let metadata = delta(self.metadata_bytes, before.metadata_bytes)?;
        add(&mut self.metadata_bytes, metadata, 1024 * 1024, PreviewFailure::Input)?;
        self.work(delta(self.computation_steps, before.computation_steps)?)?;
        self.points(delta(self.vertices, before.vertices)?)?;
        self.expand(delta(self.expanded, before.expanded)?)
    }
}
/// Fails with `Invalid` unless every value is finite.
fn finite(values: impl IntoIterator<Item = f64>) -> Checked<()> {
    if values.into_iter().all(f64::is_finite) { Ok(()) } else { Err(PreviewFailure::Invalid) }
}
/// Charges the input size of an MText (contents, style, column metadata).
fn mtext_input(m: &cadcraft_doc::MText, b: &mut Budget) -> Checked<()> {
    b.text(m.contents.len())?;
    b.meta(m.style.len())?;
    if let Some(c) = &m.columns {
        b.items(c.heights.len())?;
        b.items(usize::from(c.count))?;
        finite(c.heights.iter().copied())?;
    }
    finite([m.insert.x, m.insert.y, m.height, m.width, m.rotation])
}
/// Charges the input size of an INSERT (attributes and array metadata).
fn insert_input(ins: &Insert, b: &mut Budget) -> Checked<()> {
    b.meta(ins.block.len())?;
    b.items(ins.attribs.len())?;
    finite([ins.insert.x, ins.insert.y, ins.scale.x, ins.scale.y, ins.rotation, ins.col_spacing, ins.row_spacing])?;
    for a in &ins.attribs {
        b.meta(a.tag.len())?;
        b.meta(a.prompt.len())?;
        b.text(a.text.value.len())?;
        b.meta(a.text.style.len())?;
        b.meta(a.props.layer.as_ref().map_or(0, String::len))?;
        b.meta(a.props.linetype.as_ref().map_or(0, String::len))?;
        if let Some(m) = &a.multiline {
            b.meta(m.read_as.len())?;
            mtext_input(&m.mtext, b)?;
        }
    }
    Ok(())
}
/// Charges the cost of a spline, following the geometry sampling plan at tolerance `tol`.
fn spline_cost(s: &Spline, tol: Option<f64>, b: &mut Budget) -> Checked<()> {
    for n in [s.control.len(), s.fit.len(), s.knots.len(), s.weights.len()] {
        b.items(n)?;
    }
    finite(s.control.iter().chain(&s.fit).flat_map(|v| [v.x, v.y]).chain(s.knots.iter().copied()).chain(s.weights.iter().copied()))?;
    if !s.is_valid() {
        // The renderer draws an invalid spline as its control polygon (no evaluation).
        if tol.is_some() {
            b.points(s.control.len())?;
        }
        return Ok(());
    }
    let (lo, hi) = s.domain();
    if hi <= lo || !(hi - lo).is_finite() {
        return Err(PreviewFailure::Invalid);
    }
    let samples = if let Some(tol) = tol { s.sample_count(tol).checked_add(1).ok_or(PreviewFailure::Computation)? } else { s.bounds_sample_count() };
    // Basis recursion is triangular in degree, then degree+1 weighted controls; span search
    // on a validated monotone knot vector is logarithmic. Include input scans separately.
    let degree = s.degree;
    let per_eval = degree
        .checked_mul(degree + 1)
        .and_then(|v| v.checked_div(2))
        .and_then(|v| v.checked_add(degree + 1))
        .and_then(|v| v.checked_add(usize::BITS as usize - s.control.len().leading_zeros() as usize + 1))
        .ok_or(PreviewFailure::Computation)?;
    b.work(samples.checked_mul(per_eval).ok_or(PreviewFailure::Computation)?)?;
    b.work(s.knots.len().checked_add(s.control.len()).ok_or(PreviewFailure::Computation)?)?;
    if tol.is_some() {
        b.points(samples)?;
    }
    Ok(())
}
/// Charges the cost of a polyline, counting arc and wide-segment expansion like the renderer.
fn polyline_cost(vertices: &[cadcraft_geom::PolyVertex], closed: bool, wide: bool, tol: Option<f64>, b: &mut Budget) -> Checked<()> {
    b.items(vertices.len())?;
    finite(vertices.iter().flat_map(|v| [v.p.x, v.p.y, v.bulge, v.start_width, v.end_width]))?;
    if let Some(tol) = tol {
        // Input is bounded before cloning/segment conversion. Segment uses arc_segments, exactly
        // as the renderer; count both ends conservatively (the joined line drops duplicates).
        let p = Polyline { vertices: vertices.to_vec(), closed };
        for seg in p.segments() {
            let points = seg.tessellation_points(tol);
            b.work(points.checked_mul(if wide { 2 } else { 1 }).ok_or(PreviewFailure::Computation)?)?;
            b.points(points.checked_mul(if wide { 7 } else { 1 }).ok_or(PreviewFailure::Output)?)?;
        }
    }
    Ok(())
}
/// Charges the input, computation and output cost of one entity before it is built.
fn entity_cost(e: &Entity, tol: Option<f64>, b: &mut Budget) -> Checked<()> {
    b.expand(1)?;
    b.meta(e.common.layer.len())?;
    b.meta(e.common.linetype.len())?;
    b.meta(e.common.plot_style.len())?;
    b.items(e.common.xdata.len())?;
    b.items(e.common.anno_scales.len())?;
    for scale in &e.common.anno_scales {
        b.meta(scale.name.len())?;
    }
    for t in &e.common.xdata {
        b.meta(t.value.len())?;
    }
    match &e.kind {
        EntityKind::Spline(s) => spline_cost(s, tol, b)?,
        EntityKind::LwPolyline(p) => {
            if p.vertices.len() > MAX_INPUT {
                return Err(PreviewFailure::Input);
            }
            finite([p.const_width, p.elevation])?;
            polyline_cost(&p.vertices, p.closed, p.const_width > 0.0 || p.vertices.iter().any(|v| v.start_width > 0.0 || v.end_width > 0.0), tol, b)?;
        }
        EntityKind::Hatch(h) => {
            b.items(h.loops.len())?;
            b.items(h.pattern_lines.len())?;
            b.meta(h.pattern.len())?;
            for l in &h.pattern_lines {
                b.items(l.dashes.len())?;
            }
            for l in &h.loops {
                b.items(l.sources.len())?;
                polyline_cost(&l.vertices, true, false, tol, b)?;
            }
        }
        EntityKind::Text(t) => {
            b.text(t.value.len())?;
            b.meta(t.style.len())?;
            finite([t.insert.x, t.insert.y, t.height, t.rotation])?;
        }
        EntityKind::MText(m) => mtext_input(m, b)?,
        EntityKind::AttDef(a) => {
            b.meta(a.tag.len())?;
            b.meta(a.prompt.len())?;
            b.text(a.text.value.len())?;
            b.meta(a.text.style.len())?;
            if let Some(m) = &a.multiline {
                b.meta(m.read_as.len())?;
                mtext_input(&m.mtext, b)?;
            }
        }
        EntityKind::Insert(i) => insert_input(i, b)?,
        EntityKind::Polyline3d(p) => {
            b.items(p.points.len())?;
            finite(p.points.iter().flat_map(|v| [v.x, v.y, v.z]))?;
            if tol.is_some() {
                b.points(p.points.len())?;
            }
        }
        EntityKind::Leader(l) => {
            b.items(l.vertices.len())?;
            b.meta(l.style.len())?;
            if tol.is_some() {
                b.points(l.vertices.len())?;
            }
        }
        EntityKind::Wipeout(w) => {
            b.items(w.boundary.len())?;
            // Slab triangulation can revisit every edge per vertex and crossing cut. Preserve
            // a conservative pre-allocation bound; these are not tolerance-dependent arcs.
            let n = w.boundary.len();
            b.work(n.checked_mul(n).and_then(|v| v.checked_mul(16)).ok_or(PreviewFailure::Computation)?)?;
            if tol.is_some() {
                b.points(n.checked_mul(n).and_then(|v| v.checked_mul(6)).ok_or(PreviewFailure::Output)?)?;
            }
        }
        EntityKind::MLeader(m) => {
            b.items(m.leaders.len())?;
            for l in &m.leaders {
                b.items(l.len())?;
                if m.spline && l.len() > 1 {
                    // from_fit_points solves a banded fit before its fixed-tolerance tessellation.
                    // Keep its previous 4000 sample bound until that path exposes a plan.
                    b.work(l.len().checked_mul(l.len()).and_then(|v| v.checked_add(4_001 * 16)).ok_or(PreviewFailure::Computation)?)?;
                    if tol.is_some() {
                        b.points(4_001)?;
                    }
                } else if tol.is_some() {
                    b.points(l.len().checked_add(1).ok_or(PreviewFailure::Output)?)?;
                }
            }
            if let Some(t) = &m.text {
                mtext_input(t, b)?;
            }
            if let Some(i) = &m.block {
                insert_input(i, b)?;
            }
        }
        EntityKind::Table(t) => table_cost(t, tol, b)?,
        EntityKind::Dimension(dm) => {
            b.text(dm.text.len())?;
            b.meta(dm.style.len())?;
            b.items(dm.overrides.len())?;
            b.items(dm.assoc.len())?;
            b.items(dm.marks.breaks.len())?;
            for (key, value) in &dm.overrides {
                b.text(key.len())?;
                json_input(value, b, 0)?;
            }
            if let Some(inspect) = &dm.marks.inspect {
                b.text(inspect.label.len())?;
                b.text(inspect.rate.len())?;
            }
        }
        EntityKind::MLine(m) => {
            b.items(m.vertices.len())?;
            b.items(m.def.elements.len())?;
            b.work(m.vertices.len().checked_mul(m.def.elements.len()).and_then(|v| v.checked_mul(4097)).ok_or(PreviewFailure::Computation)?)?;
        }
        EntityKind::Unknown(u) => {
            b.items(u.tags.len())?;
            for t in &u.tags {
                b.text(t.value.len())?;
            }
        }
        EntityKind::Tolerance(t) => {
            b.text(t.text.len())?;
            b.meta(t.style.len())?;
        }
        EntityKind::Region(r) => {
            b.items(r.loops.len())?;
            for l in &r.loops {
                b.items(l.sources.len())?;
                polyline_cost(&l.vertices, true, false, tol, b)?;
            }
        }
        EntityKind::Line(l) => finite([l.a.x, l.a.y, l.a.z, l.b.x, l.b.y, l.b.z])?,
        EntityKind::Circle(c) => {
            finite([c.center.x, c.center.y, c.radius])?;
            if let Some(tol) = tol {
                let n = cadcraft_geom::arc_segments(c.radius, cadcraft_geom::TAU, tol) + 1;
                b.work(n)?;
                b.points(n)?;
            }
        }
        EntityKind::Arc(a) => {
            finite([a.center.x, a.center.y, a.radius, a.start, a.end])?;
            if let Some(tol) = tol {
                let n = cadcraft_geom::arc_segments(a.radius, cadcraft_geom::ccw_sweep(a.start, a.end), tol) + 1;
                b.work(n)?;
                b.points(n)?;
            }
        }
        EntityKind::Ellipse(e) => {
            finite([e.center.x, e.center.y, e.major.x, e.major.y, e.ratio, e.start, e.end])?;
            if let Some(tol) = tol {
                let n = cadcraft_geom::arc_segments(e.major.xy().len(), cadcraft_geom::ccw_sweep(e.start, e.end), tol).max(8) + 1;
                b.work(n)?;
                b.points(n)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// Charges the cost of a table: its cells, text and grid lines.
fn table_cost(t: &cadcraft_doc::Table, tol: Option<f64>, b: &mut Budget) -> Checked<()> {
    b.items(t.row_heights.len())?;
    b.items(t.col_widths.len())?;
    b.items(t.cells.len())?;
    let cells = t.row_heights.len().checked_mul(t.col_widths.len()).ok_or(PreviewFailure::Input)?;
    b.items(cells)?;
    b.meta(t.style.len())?;
    for s in &t.row_styles {
        b.text(s.len())?;
    }
    finite(t.row_heights.iter().chain(&t.col_widths).copied())?;
    for row in &t.cells {
        b.items(row.len())?;
        for c in row {
            b.text(c.text.len())?;
            b.meta(c.style.len())?;
            b.text(c.formula.as_ref().map_or(0, String::len))?;
            if let Some((r, col)) = c.merged {
                b.work(
                    (r as usize).min(t.row_heights.len()).checked_mul((col as usize).min(t.col_widths.len())).ok_or(PreviewFailure::Computation)?,
                )?;
            }
        }
    }
    if let Some(br) = &t.breaks {
        b.items(br.heights.len())?;
        b.items(br.offsets.len())?;
    }
    if let Some(link) = &t.data_link {
        b.meta(link.path.len())?;
    }
    // Planning visits each source row and at most three repeated label rows per part. Bound
    // that small allocation before calling the document's actual table-breaking rules.
    b.work(t.row_heights.len().checked_add(3 * cadcraft_doc::MAX_TABLE_PARTS).ok_or(PreviewFailure::Computation)?)?;
    let mut drawn_cells = 0usize;
    for part in t.parts() {
        for row in part.rows {
            drawn_cells = drawn_cells.checked_add(t.col_widths.len()).ok_or(PreviewFailure::Computation)?;
            if let Some(row) = t.cells.get(row) {
                for cell in row {
                    b.work(cell.text.len())?;
                }
            }
        }
    }
    b.work(drawn_cells.checked_mul(8).ok_or(PreviewFailure::Computation)?)?;
    if tol.is_some() {
        b.points(drawn_cells.checked_mul(14).ok_or(PreviewFailure::Output)?)?;
    }

    Ok(())
}

/// Charges the size of a JSON value, bounding its nesting at `depth`.
fn json_input(value: &serde_json::Value, b: &mut Budget, depth: usize) -> Checked<()> {
    if depth >= cadcraft_doc::MAX_BLOCK_DEPTH {
        return Err(PreviewFailure::Input);
    }
    b.items(1)?;
    match value {
        serde_json::Value::String(s) => b.text(s.len())?,
        serde_json::Value::Array(a) => {
            b.items(a.len())?;
            for v in a {
                json_input(v, b, depth + 1)?;
            }
        }
        serde_json::Value::Object(o) => {
            b.items(o.len())?;
            for (k, v) in o {
                b.text(k.len())?;
                json_input(v, b, depth + 1)?;
            }
        }
        _ => {}
    }
    Ok(())
}

// Each pass visits at most MAX_PRIMITIVES entities/copies, even without memoization. Arrays
// measure one child then reserve the remaining copies' costs before any expansion happens.
/// Bounds of block `name` including nested inserts; `stack` detects cycles and limits depth.
fn preview_block_bounds(d: &Drawing, name: &str, stack: &mut Vec<String>, tol: Option<f64>, budget: &mut Budget) -> Checked<Bounds2> {
    if stack.len() >= cadcraft_doc::MAX_BLOCK_DEPTH || stack.iter().any(|n| n.eq_ignore_ascii_case(name)) {
        return Err(PreviewFailure::Computation);
    }
    let block = d.block(name).ok_or(PreviewFailure::MissingReference)?;
    budget.meta(name.len())?;
    budget.meta(block.description.len())?;
    budget.items(block.entities.len())?;
    stack.push(name.to_string());
    let mut bounds = Bounds2::EMPTY;
    for e in block.entities.iter() {
        entity_cost(e, tol, budget)?;
        let mut local = match &e.kind {
            EntityKind::Insert(i) => insert_bounds(d, i, stack, tol, budget)?,
            EntityKind::Dimension(dm) => {
                let child = match &dm.block {
                    Some(n) => preview_block_bounds(d, n, stack, tol, budget)?,
                    None => Bounds2::EMPTY,
                };
                if tol.is_some() {
                    child
                } else {
                    let mut dm = dm.clone();
                    dm.block = None;
                    child.union(&cadcraft_doc::entity_bounds(d, &Entity { kind: EntityKind::Dimension(dm), ..(**e).clone() }, stack.len()))
                }
            }
            EntityKind::MLeader(ml) => {
                let child = match &ml.block {
                    Some(i) => insert_bounds(d, i, stack, tol, budget)?,
                    None => Bounds2::EMPTY,
                };
                if tol.is_some() {
                    child
                } else {
                    let mut ml = ml.clone();
                    ml.block = None;
                    child.union(&cadcraft_doc::entity_bounds(d, &Entity { kind: EntityKind::MLeader(ml), ..(**e).clone() }, stack.len()))
                }
            }
            _ if tol.is_some() => Bounds2::EMPTY,
            _ => cadcraft_doc::entity_bounds(d, e, stack.len()),
        };
        if !local.is_empty() {
            finite([local.min.x, local.min.y, local.max.x, local.max.y, local.width(), local.height()])?;
            // Match hidden geometry in the renderer; preflight still accounts for its input.
            if !e.common.visible || d.is_hidden_object(e.handle) {
                local = Bounds2::EMPTY;
            }
            bounds = bounds.union(&local);
        }
    }
    stack.pop();
    Ok(bounds)
}
/// Bounds of one INSERT, covering every copy of an array.
fn insert_bounds(d: &Drawing, i: &Insert, stack: &mut Vec<String>, tol: Option<f64>, b: &mut Budget) -> Checked<Bounds2> {
    let copies = (i.cols.max(1) as usize).checked_mul(i.rows.max(1) as usize).ok_or(PreviewFailure::Computation)?;
    b.expand(copies)?;
    let target = d.block(&i.block).ok_or(PreviewFailure::MissingReference)?;
    // Rendering searches the definition for each attribute tag before applying its properties.
    b.work(i.attribs.len().checked_mul(target.entities.len()).ok_or(PreviewFailure::Computation)?)?;
    let base = target.base.xy();
    finite([base.x, base.y])?;
    let xf = i.transform(base);
    let scale = xf.scale_factor();
    if !scale.is_finite() || scale <= 0.0 {
        return Err(PreviewFailure::Invalid);
    }
    let before = *b;
    let inner = preview_block_bounds(d, &i.block, stack, tol.map(|t| t / scale.max(1e-12)), b)?;
    b.copies(before, copies)?;
    let mut bounds = if inner.is_empty() { inner } else { Bounds2::from_points(inner.corners().map(|p| xf.apply(p))) };
    if !bounds.is_empty() {
        // All four array corner offsets, including mixed signs/rotated arrays.
        for (c, r) in [(i.cols.saturating_sub(1), 0), (0, i.rows.saturating_sub(1)), (i.cols.saturating_sub(1), i.rows.saturating_sub(1))] {
            let offset = Vec2::new(i.col_spacing * f64::from(c), i.row_spacing * f64::from(r)).rotate(i.rotation);
            bounds = bounds.union(&Bounds2::from_points(inner.corners().map(|p| xf.apply(p) + offset)));
        }
    }
    if tol.is_none() && !i.attribs.is_empty() {
        let mut attrs = i.clone();
        attrs.block.clear();
        let e = Entity::new(cadcraft_doc::Handle(0), EntityKind::Insert(attrs));
        bounds = bounds.union(&cadcraft_doc::entity_bounds(d, &e, stack.len()));
    }
    Ok(bounds)
}

/// Builds the preview of block `name`; never panics, refusals are returned as `Unavailable`.
pub(super) fn build(d: &Drawing, name: &str) -> PreviewResult {
    match build_checked(d, name) {
        Ok(v) => v,
        Err(e) => PreviewResult::Unavailable(e),
    }
}
/// The fallible body of [`build`].
fn build_checked(d: &Drawing, name: &str) -> Checked<PreviewResult> {
    let block = d.block(name).ok_or(PreviewFailure::MissingReference)?;
    let mut budget = Budget::default();
    let bounds = preview_block_bounds(d, name, &mut Vec::new(), None, &mut budget)?;
    if bounds.is_empty() {
        return Ok(PreviewResult::Empty);
    }
    let size = bounds.width().max(bounds.height()).max(1e-9);
    let opts = cadcraft_render::Options {
        tolerance: size / REFERENCE_PIXELS,
        min_dash: size / (REFERENCE_PIXELS / 2.0),
        view_height: size,
        hatch_budget: Some(Arc::new(cadcraft_render::HatchBudget::new(250_000))),
        ..Default::default()
    };
    // Keep bounds and render work in one allowance, but do not charge the same input twice.
    budget.metadata_bytes = 0;
    budget.input_items = 0;
    budget.text_bytes = 0;
    budget.expanded = 0;
    preview_block_bounds(d, name, &mut Vec::new(), Some(opts.tolerance), &mut budget)?;
    let list = cadcraft_render::build_entities_limited(d, block.entities.iter().map(|e| &**e), &opts, MAX_PRIMITIVES, MAX_VERTICES)
        .ok_or(PreviewFailure::Output)?;
    if opts.hatch_budget.as_ref().is_some_and(|b| b.failed()) {
        return Err(PreviewFailure::Computation);
    }
    finite(list.verts.iter().chain(&list.tris).flat_map(|v| [v.x, v.y]))?;
    if list.prims.is_empty() { Ok(PreviewResult::Empty) } else { Ok(PreviewResult::Ready(Preview { list: Arc::new(list), bounds })) }
}

#[cfg(test)]
mod tests;
