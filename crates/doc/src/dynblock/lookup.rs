//! Lookup tables (BLOOKUPTABLE): a lookup action's rows of input values (a distance, a position,
//! a visibility state) and the lookup values each row gives, written as AutoCAD writes its own
//! (observed in files it saves, and in its Property Lookup Table dialog):
//!
//! - The table lists its input columns, then its lookup columns. Each column names its parameter
//!   and the value it stands for (`UpdatedDistance`, `UpdatedX`, `UpdatedY`, `VisibilityState`,
//!   `lookupString`), that value's type and unit, what a reference shows when no row matches
//!   ("Custom") and, for a lookup column, whether setting it on a reference sets the row's inputs
//!   ("Allow reverse lookup") or it is read only.
//! - Cells are text: numbers in block units, visibility states by name.
//! - Two rows with the same inputs are refused ("Duplicate row of input values found."); a lookup
//!   column whose values repeat can't be reversed (AutoCAD sets it back to read only).
//! - The graph has an edge each way between the action and each parameter it reads (its kind is
//!   how many of the parameter's values the table has), then between the action and its lookup
//!   parameters, each pair flagged 4 and naming the other; a table with no inputs has only the
//!   edge from the action to its lookup parameter.

use super::write::{Graph, dict_entry, tag};
use super::{ActionKind, DynamicBlock, ParameterKind, dynamic_block};
use crate::{Drawing, Handle, RawTag};

/// Most rows one lookup table is written with.
pub const MAX_LOOKUP_ROWS: usize = 10_000;

/// Most columns one lookup table is written with.
const MAX_COLUMNS: usize = 1_000;

/// A lookup table column as it is written: the parameter's graph id and the value it stands for,
/// whether it is a lookup column, what references show when no row matches, and whether a lookup
/// column can be set on a reference (reverse lookup).
#[derive(Clone, Debug, PartialEq)]
pub struct TableColumn {
    pub node: i32,
    pub property: String,
    pub lookup: bool,
    pub unmatched: String,
    pub reverse: bool,
}

/// The value type (95) and unit (96) AutoCAD writes for a column standing for `property`: text,
/// an angle, a flip state or a distance.
fn value_type(property: &str) -> (i32, i32) {
    match property.to_ascii_lowercase().as_str() {
        "lookupstring" | "visibilitystate" => (1, 0),
        "updatedangle" => (40, 1),
        "updatedflip" => (70, 0),
        _ => (40, 2),
    }
}

/// A lookup action's own section: rows and columns, the cells row by row, then each column.
pub(super) fn table_tags(columns: &[TableColumn], rows: &[Vec<String>]) -> Vec<RawTag> {
    let mut out = vec![tag(100, "AcDbBlockLookupAction"), tag(92, rows.len()), tag(93, columns.len()), tag(301, "")];
    out.extend(rows.iter().flatten().map(|c| tag(302, c)));
    for c in columns {
        let (ty, unit) = value_type(&c.property);
        out.extend([tag(303, ""), tag(94, c.node), tag(95, ty), tag(96, unit), tag(282, i32::from(c.lookup))]);
        out.extend([tag(305, &c.unmatched), tag(281, i32::from(c.lookup && c.reverse)), tag(304, &c.property)]);
    }
    out.push(tag(280, 1));
    out
}

/// The lookup action whose table gives lookup parameter `node`'s value, if one does.
pub(super) fn table_of(m: &DynamicBlock, node: i32) -> Option<i32> {
    m.actions
        .iter()
        .find(|a| matches!(&a.kind, ActionKind::Lookup { columns, .. } if columns.iter().any(|c| c.lookup && c.node == node)))
        .map(|a| a.node)
}

/// Set the lookup action a lookup parameter names (`94`): `id`, or 0 for none.
pub(super) fn set_action_of(d: &mut Drawing, param: Handle, id: i32) {
    let f = std::sync::Arc::make_mut(&mut d.foreign);
    let Some(o) = f.objects.iter_mut().find(|o| o.handle == param) else { return };
    let Some(at) = o.tags.iter().position(|t| t.code == 100 && t.value.eq_ignore_ascii_case("AcDbBlockLookUpParameter")) else { return };
    if let Some(t) = o.tags.iter_mut().skip(at + 1).take_while(|t| t.code != 100).find(|t| t.code == 94) {
        *t = tag(94, id);
    }
}

/// The input properties block `m`'s lookup tables can read, as the Add Parameter Properties
/// dialog lists them: (name, parameter graph id, value). A distance, a position's X and Y, a
/// visibility state.
pub fn input_properties(m: &DynamicBlock) -> Vec<(String, i32, String)> {
    let mut out = Vec::new();
    for p in &m.parameters {
        match &p.kind {
            ParameterKind::Linear { label, .. } => out.push((label.clone(), p.node, "UpdatedDistance".to_string())),
            ParameterKind::Point { label, .. } => {
                out.push((format!("{label} X"), p.node, "UpdatedX".to_string()));
                out.push((format!("{label} Y"), p.node, "UpdatedY".to_string()));
            }
            ParameterKind::Visibility { label, .. } => out.push((label.clone(), p.node, "VisibilityState".to_string())),
            _ => {}
        }
    }
    out
}

/// The name a lookup table column shows: an input property's name ("Position1 X"), a lookup
/// parameter's property name, or the property itself when the parameter is unknown.
pub fn column_property(m: &DynamicBlock, node: i32, property: &str) -> String {
    if let Some((n, ..)) = input_properties(m).into_iter().find(|(_, id, prop)| *id == node && prop.eq_ignore_ascii_case(property)) {
        return n;
    }
    match m.parameters.iter().find(|p| p.node == node).map(|p| &p.kind) {
        Some(ParameterKind::Lookup { label, .. }) => label.clone(),
        Some(k) => k.label().map_or_else(|| property.to_string(), |l| format!("{l} {property}")),
        None => property.to_string(),
    }
}

/// Whether table cells `a` and `b` read the same (numbers compare as numbers, text in any case).
fn same(a: &str, b: &str) -> bool {
    match (a.trim().parse::<f64>(), b.trim().parse::<f64>()) {
        (Ok(x), Ok(y)) => (x - y).abs() <= 1e-9 * x.abs().max(y.abs()).max(1.0),
        _ => a.trim().eq_ignore_ascii_case(b.trim()),
    }
}

/// Fill lookup action `action` of block `block` (any case) with `columns` and `rows` (each row
/// one cell per column), as the Property Lookup Table dialog's OK does. Input columns are written
/// before lookup columns; rows with every cell empty are left out. Refused, with AutoCAD's reason,
/// when a column can't be read or set, a cell isn't a value of its column, or two rows have the
/// same inputs.
pub fn set_lookup_table(d: &mut Drawing, block: &str, action: i32, columns: &[TableColumn], rows: &[Vec<String>]) -> Result<(), String> {
    write_table(d, block, action, columns, rows, false)
}

/// [`set_lookup_table`]; with `dedupe`, a row whose inputs repeat an earlier row's is left out
/// instead of refused (a column was taken out from under it).
fn write_table(d: &mut Drawing, block: &str, action: i32, columns: &[TableColumn], rows: &[Vec<String>], dedupe: bool) -> Result<(), String> {
    if columns.len() > MAX_COLUMNS {
        return Err(format!("A lookup table has at most {MAX_COLUMNS} columns."));
    }
    if rows.len() > MAX_LOOKUP_ROWS {
        return Err(format!("A lookup table has at most {MAX_LOOKUP_ROWS} rows."));
    }
    let m = dynamic_block(d, block).ok_or("The block has no parameters or actions.")?;
    let a = m.actions.iter().find(|a| a.node == action).ok_or_else(|| format!("No action {action} in the block."))?;
    let ActionKind::Lookup { columns: old, .. } = &a.kind else { return Err(format!("{} is not a lookup action.", a.name)) };
    let own = old.iter().find(|c| c.lookup).map(|c| c.node);
    let (action_handle, name) = (a.handle, a.name.clone());

    // The columns, checked and spelled as AutoCAD spells them; inputs first.
    let mut cols: Vec<(usize, TableColumn)> = Vec::new();
    for (i, c) in columns.iter().enumerate() {
        let p = m.parameters.iter().find(|p| p.node == c.node).ok_or_else(|| format!("No parameter {} in the block.", c.node))?;
        let property = match (&p.kind, c.property.trim().to_ascii_lowercase().as_str(), c.lookup) {
            (ParameterKind::Lookup { .. }, "lookupstring" | "", true) => "lookupString",
            (ParameterKind::Lookup { label, .. }, _, false) => return Err(format!("{label} is a lookup parameter; it can only be a lookup column.")),
            (_, _, true) => return Err(format!("{} is not a lookup parameter.", p.element)),
            (ParameterKind::Linear { .. }, "updateddistance" | "distance", _) => "UpdatedDistance",
            (ParameterKind::Point { .. }, "updatedx" | "x", _) => "UpdatedX",
            (ParameterKind::Point { .. }, "updatedy" | "y", _) => "UpdatedY",
            (ParameterKind::Visibility { .. }, "visibilitystate" | "", _) => "VisibilityState",
            (_, other, _) => return Err(format!("A lookup table can't read {other} of {}.", p.element)),
        };
        if c.lookup && table_of(&m, c.node).is_some_and(|t| t != action) {
            return Err(format!("{} already has a lookup table.", column_property(&m, c.node, property)));
        }
        if cols.iter().any(|(_, o)| o.node == c.node && o.property == property) {
            return Err(format!("{} is in the table twice.", column_property(&m, c.node, property)));
        }
        let unmatched = if c.unmatched.trim().is_empty() { "Custom".to_string() } else { c.unmatched.trim().to_string() };
        cols.push((i, TableColumn { node: c.node, property: property.to_string(), lookup: c.lookup, unmatched, reverse: c.reverse }));
    }
    if let Some(own) = own
        && !cols.iter().any(|(_, c)| c.lookup && c.node == own)
    {
        return Err(format!("{name} gives {}; its lookup column can't be removed.", column_property(&m, own, "lookupString")));
    }
    cols.sort_by_key(|(_, c)| c.lookup);

    // The cells, in the columns' new order.
    let states = |node: i32| match m.parameters.iter().find(|p| p.node == node).map(|p| &p.kind) {
        Some(ParameterKind::Visibility { states, .. }) => states.iter().map(|s| s.name.clone()).collect(),
        _ => Vec::new(),
    };
    let mut table: Vec<Vec<String>> = Vec::new();
    for (r, row) in rows.iter().enumerate() {
        if row.len() != columns.len() {
            return Err(format!("Row {} has {} cells; the table has {} columns.", r + 1, row.len(), columns.len()));
        }
        if row.iter().all(|c| c.trim().is_empty()) {
            continue;
        }
        let mut out = Vec::with_capacity(cols.len());
        for (i, c) in &cols {
            let cell = row.get(*i).map(|c| c.trim()).unwrap_or_default();
            let shown = column_property(&m, c.node, &c.property);
            if cell.is_empty() {
                return Err(format!("Row {}: {shown} needs a value.", r + 1));
            }
            out.push(match c.property.as_str() {
                "VisibilityState" => states(c.node)
                    .into_iter()
                    .find(|s| s.eq_ignore_ascii_case(cell))
                    .ok_or_else(|| format!("Row {}: \"{cell}\" is not a visibility state of {shown}.", r + 1))?,
                "lookupString" => cell.to_string(),
                _ => {
                    let v = cell
                        .parse::<f64>()
                        .ok()
                        .filter(|v| v.is_finite())
                        .ok_or_else(|| format!("Row {}: {shown} needs a number, not \"{cell}\".", r + 1))?;
                    if c.property == "UpdatedDistance" && v < 0.0 {
                        return Err(format!("Row {}: {shown} can't be negative.", r + 1));
                    }
                    format!("{v}")
                }
            });
        }
        table.push(out);
    }
    let inputs = cols.iter().filter(|(_, c)| !c.lookup).count();
    let same_inputs = |a: &[String], b: &[String]| a.iter().zip(b).take(inputs).all(|(x, y)| same(x, y));
    let mut kept: Vec<Vec<String>> = Vec::new();
    for row in table {
        if inputs > 0 && kept.iter().any(|k| same_inputs(k, &row)) {
            if dedupe {
                continue;
            }
            return Err("Duplicate row of input values found.".into());
        }
        kept.push(row);
    }
    // A lookup column whose values repeat (or that has none) can't be reversed.
    let mut cols: Vec<TableColumn> = cols.into_iter().map(|(_, c)| c).collect();
    for (j, c) in cols.iter_mut().enumerate() {
        let values: Vec<&String> = kept.iter().filter_map(|r| r.get(j)).collect();
        let repeats = values.iter().enumerate().any(|(i, v)| values.iter().take(i).any(|w| w.eq_ignore_ascii_case(v)));
        c.reverse = c.lookup && c.reverse && !values.is_empty() && !repeats;
    }

    // The action's object: its table, and the flag AutoCAD changes once the table has inputs.
    let f = std::sync::Arc::make_mut(&mut d.foreign);
    let o = f.objects.iter_mut().find(|o| o.handle == action_handle).ok_or("The lookup action is missing.")?;
    let at = o
        .tags
        .iter()
        .position(|t| t.code == 100 && t.value.eq_ignore_ascii_case("AcDbBlockLookupAction"))
        .ok_or("The lookup action can't be changed.")?;
    // Its section is the object's last; anything after it would be a layout this doesn't know.
    if o.tags.iter().skip(at + 1).any(|t| t.code == 100) {
        return Err("The lookup action can't be changed.".into());
    }
    o.tags.truncate(at);
    o.tags.extend(table_tags(&cols, &kept));
    if let Some(el) = o.tags.iter().position(|t| t.code == 100 && t.value.eq_ignore_ascii_case("AcDbBlockElement"))
        && let Some(t) = o.tags.iter_mut().skip(el + 1).take_while(|t| t.code != 100).find(|t| t.code == 1071)
    {
        *t = tag(1071, if inputs > 0 { 0 } else { 8 });
    }

    // The edges: the action's own, made again for its columns.
    let key = ("BLOCK_RECORD".to_string(), block.trim().to_ascii_uppercase());
    let gh = d
        .foreign
        .records
        .get(&key)
        .and_then(|x| x.xdict)
        .and_then(|x| dict_entry(d, x, "ACAD_ENHANCEDBLOCK"))
        .ok_or("The block's evaluation graph is missing.")?;
    let mut g = d.foreign.objects.iter().find(|o| o.handle == gh).and_then(Graph::read).ok_or("The block's evaluation graph can't be changed.")?;
    let index = |id: i32| g.nodes.iter().find(|n| n.id == id).map(|n| n.index);
    let ai = index(action).ok_or("The lookup action is missing from the graph.")?;
    let mut edges = g.kept_edges(|e| e.from == ai || e.to == ai, Some);
    let pair = |edges: &mut Vec<(i32, i32, i32, i32, i32)>, from: i32, to: i32, kind: i32, both: bool| {
        let at = i32::try_from(edges.len()).unwrap_or(i32::MAX);
        // AutoCAD flags a pair of edges 4 (without it, it reads the block as a static one).
        if both {
            edges.push((from, to, kind, 4, at.saturating_add(1)));
            edges.push((to, from, kind, 4, at));
        } else {
            edges.push((from, to, kind, 0, -1));
        }
    };
    let mut seen: Vec<i32> = Vec::new();
    for c in cols.iter().filter(|c| !c.lookup) {
        if seen.contains(&c.node) {
            continue;
        }
        seen.push(c.node);
        let kind = i32::try_from(cols.iter().filter(|o| !o.lookup && o.node == c.node).count()).unwrap_or(1);
        let pi = index(c.node).ok_or("A parameter of the table is missing from the graph.")?;
        pair(&mut edges, pi, ai, kind, true);
    }
    for c in cols.iter().filter(|c| c.lookup) {
        let pi = index(c.node).ok_or("A parameter of the table is missing from the graph.")?;
        pair(&mut edges, ai, pi, 1, inputs > 0);
    }
    g.relink(&edges);
    let tags = g.tags();
    if let Some(o) = std::sync::Arc::make_mut(&mut d.foreign).objects.iter_mut().find(|o| o.handle == gh) {
        o.tags = tags;
    }

    // Lookup parameters name the action giving their value; one left out names none.
    for p in &m.parameters {
        if !matches!(p.kind, ParameterKind::Lookup { .. }) {
            continue;
        }
        if cols.iter().any(|c| c.lookup && c.node == p.node) {
            set_action_of(d, p.handle, action);
        } else if old.iter().any(|c| c.lookup && c.node == p.node) {
            set_action_of(d, p.handle, 0);
        }
    }
    Ok(())
}

/// Before parameters `erased` of block `block` go: every lookup table reading them (or giving
/// one of them that isn't its own) loses those columns, and rows that then repeat another's
/// inputs. Returns the lookup actions whose own parameter goes (they go with it).
pub(super) fn drop_columns(d: &mut Drawing, block: &str, erased: &[i32]) -> Result<Vec<i32>, String> {
    let Some(m) = dynamic_block(d, block) else { return Ok(Vec::new()) };
    let mut gone = Vec::new();
    for a in &m.actions {
        let ActionKind::Lookup { columns, rows } = &a.kind else { continue };
        if columns.iter().find(|c| c.lookup).is_some_and(|c| erased.contains(&c.node)) {
            gone.push(a.node);
            continue;
        }
        if !columns.iter().any(|c| erased.contains(&c.node)) {
            continue;
        }
        let keep: Vec<usize> = (0..columns.len()).filter(|i| columns.get(*i).is_some_and(|c| !erased.contains(&c.node))).collect();
        let cols: Vec<TableColumn> = keep
            .iter()
            .filter_map(|i| columns.get(*i))
            .map(|c| TableColumn { node: c.node, property: c.property.clone(), lookup: c.lookup, unmatched: c.unmatched.clone(), reverse: c.reverse })
            .collect();
        let rows: Vec<Vec<String>> = rows.iter().map(|r| keep.iter().map(|i| r.get(*i).cloned().unwrap_or_default()).collect()).collect();
        write_table(d, block, a.node, &cols, &rows, true)?;
    }
    Ok(gone)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dynblock::{ActionSpec, ParamPoint, ParameterSpec, VisibilityState, add_action, add_parameter, erase_elements, set_visibility};
    use cadcraft_geom::Vec3;

    fn v(x: f64, y: f64) -> Vec3 {
        Vec3::new(x, y, 0.0)
    }

    fn col(node: i32, property: &str, lookup: bool) -> TableColumn {
        TableColumn { node, property: property.into(), lookup, unmatched: String::new(), reverse: lookup }
    }

    fn row(cells: &[&str]) -> Vec<String> {
        cells.iter().map(|c| c.to_string()).collect()
    }

    /// Block "B" as AutoCAD's sample is made: a linear parameter (Distance1) with a move, a point
    /// (Position1), a visibility parameter with states VisibilityState0 and Small, a lookup
    /// parameter (Size) and its lookup action. Returns (linear, point, visibility, lookup,
    /// action).
    fn block() -> (Drawing, [i32; 5]) {
        let mut d = Drawing::default();
        let lin =
            add_parameter(&mut d, "B", &ParameterSpec::Linear { base: v(0.0, 0.0), end: v(10.0, 0.0), label: None, label_offset: -1.0, grips: 1 })
                .unwrap();
        add_action(&mut d, "B", &ActionSpec::Move { parameter: lin, point: ParamPoint::End, selection: vec![Handle(0x40)], at: v(12.0, 0.0) })
            .unwrap();
        let pt = add_parameter(&mut d, "B", &ParameterSpec::Point { at: v(5.0, 2.0), label: None, label_at: v(5.0, 3.0), grips: 1 }).unwrap();
        let vis =
            add_parameter(&mut d, "B", &ParameterSpec::Visibility { at: v(-2.0, 5.0), label: None, objects: vec![Handle(0x40)], grips: 1 }).unwrap();
        let states = [
            VisibilityState { name: "VisibilityState0".into(), ..Default::default() },
            VisibilityState { name: "Small".into(), ..Default::default() },
        ];
        set_visibility(&mut d, "B", &[Handle(0x40)], &states).unwrap();
        let spec =
            ParameterSpec::Lookup { at: v(-2.0, 7.0), label: Some("Size".into()), name: None, description: String::new(), show: true, grips: 1 };
        let lk = add_parameter(&mut d, "B", &spec).unwrap();
        let act = add_action(&mut d, "B", &ActionSpec::Lookup { parameter: lk, at: v(-2.0, 8.0) }).unwrap();
        (d, [lin, pt, vis, lk, act])
    }

    fn graph(d: &Drawing) -> Graph {
        d.foreign.objects.iter().find(|o| o.kind == "ACAD_EVALUATION_GRAPH").and_then(Graph::read).unwrap()
    }

    /// The graph's edges by node ids: (from, to, kind, the pair edge's from and to).
    fn edges(g: &Graph) -> Vec<(i32, i32, i32, Option<(i32, i32)>)> {
        let id = |i: i32| g.nodes.iter().find(|n| n.index == i).unwrap().id;
        g.edges
            .iter()
            .map(|e| {
                let pair = (e.links[4] >= 0).then(|| g.edges.iter().find(|p| p.index == e.links[4]).map(|p| (id(p.from), id(p.to))).unwrap());
                (id(e.from), id(e.to), e.kind, pair)
            })
            .collect()
    }

    fn of(g: &Graph, node: i32) -> Vec<(i32, i32, i32, Option<(i32, i32)>)> {
        edges(g).into_iter().filter(|(f, t, ..)| *f == node || *t == node).collect()
    }

    fn object<'a>(d: &'a Drawing, kind: &str) -> &'a crate::foreign::ForeignObject {
        d.foreign.objects.iter().find(|o| o.kind == kind).unwrap()
    }

    fn group(o: &crate::foreign::ForeignObject, marker: &str, code: i32) -> String {
        let at = o.tags.iter().position(|t| t.code == 100 && t.value == marker).unwrap();
        o.tags.iter().skip(at + 1).find(|t| t.code == code).unwrap().value.clone()
    }

    fn table(d: &Drawing, act: i32) -> (Vec<super::super::LookupColumn>, Vec<Vec<String>>) {
        let m = dynamic_block(d, "B").unwrap();
        match &m.actions.iter().find(|a| a.node == act).unwrap().kind {
            ActionKind::Lookup { columns, rows } => (columns.clone(), rows.clone()),
            k => panic!("{k:?}"),
        }
    }

    #[test]
    fn a_new_lookup_action_gives_its_parameter_and_reads_nothing() {
        let (d, [.., lk, act]) = block();
        let m = dynamic_block(&d, "B").unwrap();
        let p = m.parameters.iter().find(|p| p.node == lk).unwrap();
        assert!(matches!(&p.kind, ParameterKind::Lookup { label, .. } if label == "Size"));
        assert_eq!((p.element.as_str(), p.grips.len()), ("Lookup", 1));
        assert_eq!(m.actions.iter().find(|a| a.node == act).unwrap().name, "Lookup1");
        let (columns, rows) = table(&d, act);
        assert!(rows.is_empty());
        assert_eq!(columns.len(), 1);
        let c = &columns[0];
        assert_eq!((c.node, c.property.as_str(), c.lookup, c.unmatched.as_str(), c.reverse), (lk, "lookupString", true, "Custom", false));
        assert_eq!(group(object(&d, "BLOCKLOOKUPPARAMETER"), "AcDbBlockLookUpParameter", 94), act.to_string(), "the parameter names its table");
        assert_eq!(group(object(&d, "BLOCKLOOKUPACTION"), "AcDbBlockElement", 1071), "8");
        assert_eq!(object(&d, "BLOCKLOOKUPGRIP").tags.last().unwrap().value, "AcDbBlockLookUpGrip");
        // Only the edge from the action to its parameter.
        let g = graph(&d);
        assert_eq!(of(&g, act), [(act, lk, 1, None)]);
        // The lookup grip is linked before its parameter's location components, as visibility's.
        let grip = p.grips[0];
        assert_eq!(of(&g, lk).first(), Some(&(grip, lk, 1, None)));
        let again = add_action(&mut d.clone(), "B", &ActionSpec::Lookup { parameter: lk, at: v(0.0, 0.0) });
        assert!(again.unwrap_err().contains("already associated"));
    }

    #[test]
    fn a_table_is_written_as_autocad_writes_it() {
        let (mut d, [lin, pt, vis, lk, act]) = block();
        let cols = [
            col(lin, "UpdatedDistance", false),
            col(pt, "UpdatedX", false),
            col(pt, "UpdatedY", false),
            col(vis, "VisibilityState", false),
            col(lk, "lookupString", true),
        ];
        let rows = [row(&["10", "5", "2", "visibilitystate0", "Big"]), row(&["6.0", "3", "2", "Small", "Small"]), row(&["", "", "", "", ""])];
        set_lookup_table(&mut d, "B", act, &cols, &rows).unwrap();
        let (columns, rows) = table(&d, act);
        assert_eq!(rows, [row(&["10", "5", "2", "VisibilityState0", "Big"]), row(&["6", "3", "2", "Small", "Small"])]);
        let props: Vec<(i32, &str, bool, bool)> = columns.iter().map(|c| (c.node, c.property.as_str(), c.lookup, c.reverse)).collect();
        assert_eq!(
            props,
            [
                (lin, "UpdatedDistance", false, false),
                (pt, "UpdatedX", false, false),
                (pt, "UpdatedY", false, false),
                (vis, "VisibilityState", false, false),
                (lk, "lookupString", true, true)
            ]
        );
        // Types and units as AutoCAD writes them.
        let o = object(&d, "BLOCKLOOKUPACTION");
        let types: Vec<&str> = o.tags.iter().filter(|t| t.code == 95 || t.code == 96).map(|t| t.value.as_str()).collect();
        assert_eq!(types, ["40", "2", "40", "2", "40", "2", "1", "0", "1", "0"]);
        assert_eq!(group(o, "AcDbBlockElement", 1071), "0");
        assert_eq!(o.tags.last().map(|t| (t.code, t.value.as_str())), Some((280, "1")));
        // An edge each way per parameter read (the point's two columns: kind 2), then the lookup.
        let lk_pair = Some((lk, act));
        assert_eq!(
            of(&graph(&d), act),
            [
                (lin, act, 1, Some((act, lin))),
                (act, lin, 1, Some((lin, act))),
                (pt, act, 2, Some((act, pt))),
                (act, pt, 2, Some((pt, act))),
                (vis, act, 1, Some((act, vis))),
                (act, vis, 1, Some((vis, act))),
                (act, lk, 1, lk_pair),
                (lk, act, 1, Some((act, lk))),
            ]
        );
        let g = graph(&d);
        let ai = g.nodes.iter().find(|n| n.id == act).unwrap().index;
        assert!(g.edges.iter().filter(|e| e.from == ai || e.to == ai).all(|e| e.flags == 4), "a pair is flagged 4");
        // Filled again, the same edges, not more.
        set_lookup_table(&mut d, "B", act, &cols, &[row(&["10", "5", "2", "Small", "Big"])]).unwrap();
        assert_eq!(of(&graph(&d), act).len(), 8);
        // Emptied, it is back to its first form.
        set_lookup_table(&mut d, "B", act, &[col(lk, "lookupString", true)], &[]).unwrap();
        assert_eq!(of(&graph(&d), act), [(act, lk, 1, None)]);
        assert!(graph(&d).edges.iter().all(|e| e.flags == 0), "an edge on its own isn't flagged");
        assert_eq!(group(object(&d, "BLOCKLOOKUPACTION"), "AcDbBlockElement", 1071), "8");
    }

    #[test]
    fn the_dialogs_rules_refuse_what_autocad_refuses() {
        let (d, [lin, pt, vis, lk, act]) = block();
        let cols = [col(lin, "UpdatedDistance", false), col(vis, "VisibilityState", false), col(lk, "", true)];
        let set = |rows: &[Vec<String>]| set_lookup_table(&mut d.clone(), "B", act, &cols, rows);
        assert_eq!(set(&[row(&["10", "Small", "A"]), row(&["10.0", "small", "B"])]).unwrap_err(), "Duplicate row of input values found.");
        assert!(set(&[row(&["10", "Nope", "A"])]).unwrap_err().contains("not a visibility state"));
        assert!(set(&[row(&["ten", "Small", "A"])]).unwrap_err().contains("needs a number"));
        assert!(set(&[row(&["10", "Small", ""])]).unwrap_err().contains("needs a value"));
        assert!(set(&[row(&["10", "Small"])]).unwrap_err().contains("cells"));
        // Repeated lookup values: read only.
        let mut d2 = d.clone();
        set_lookup_table(&mut d2, "B", act, &cols, &[row(&["10", "Small", "A"]), row(&["5", "Small", "a"])]).unwrap();
        assert!(!table(&d2, act).0[2].reverse);
        // Columns that can't be: a lookup parameter as an input, the wrong value, twice, without
        // the action's own lookup column, an input as a lookup column.
        let bad = |cols: &[TableColumn]| set_lookup_table(&mut d.clone(), "B", act, cols, &[]).unwrap_err();
        assert!(bad(&[col(lk, "lookupString", false), col(lk, "lookupString", true)]).contains("only be a lookup column"));
        assert!(bad(&[col(pt, "UpdatedDistance", false), col(lk, "", true)]).contains("can't read"));
        assert!(bad(&[col(lin, "distance", false), col(lin, "UpdatedDistance", false), col(lk, "", true)]).contains("twice"));
        assert!(bad(&[col(lin, "UpdatedDistance", false)]).contains("can't be removed"));
        assert!(bad(&[col(lin, "UpdatedDistance", true), col(lk, "", true)]).contains("not a lookup parameter"));
        assert!(set_lookup_table(&mut d.clone(), "B", lin, &cols, &[]).is_err(), "not an action");
    }

    #[test]
    fn erasing_a_parameter_takes_its_columns_and_a_lookup_parameter_its_table() {
        let (mut d, [lin, pt, vis, lk, act]) = block();
        let cols = [col(lin, "UpdatedDistance", false), col(pt, "UpdatedX", false), col(vis, "VisibilityState", false), col(lk, "", true)];
        set_lookup_table(&mut d, "B", act, &cols, &[row(&["10", "5", "Small", "Big"]), row(&["6", "5", "Small", "Small"])]).unwrap();
        erase_elements(&mut d, "B", &[lin]).unwrap();
        let (columns, rows) = table(&d, act);
        assert_eq!(columns.iter().map(|c| c.node).collect::<Vec<_>>(), [pt, vis, lk]);
        assert_eq!(rows, [row(&["5", "Small", "Big"])], "the row that now repeats the first's inputs goes");
        assert!(!dynamic_block(&d, "B").unwrap().actions.iter().any(|a| a.name == "Move"), "the distance's move went with it");
        let g = graph(&d);
        assert!(
            g.edges.iter().all(|e| e.links[4] < 0 || g.edges.iter().any(|p| p.index == e.links[4] && p.from == e.to && p.to == e.from)),
            "pairs kept"
        );
        assert_eq!(erase_elements(&mut d, "B", &[lk]).unwrap(), [lk, act]);
        assert!(dynamic_block(&d, "B").unwrap().actions.is_empty());
        assert!(graph(&d).edges.iter().all(|e| e.links[4] < 0));
    }

    #[test]
    fn erasing_a_lookup_action_leaves_its_parameter_without_a_table() {
        let (mut d, [.., lk, act]) = block();
        erase_elements(&mut d, "B", &[act]).unwrap();
        assert_eq!(group(object(&d, "BLOCKLOOKUPPARAMETER"), "AcDbBlockLookUpParameter", 94), "0");
        add_action(&mut d, "B", &ActionSpec::Lookup { parameter: lk, at: v(0.0, 0.0) }).unwrap();
    }

    #[test]
    fn hostile_tables_are_refused() {
        let (d, [lin, .., lk, act]) = block();
        let cols = [col(lin, "UpdatedDistance", false), col(lk, "", true)];
        let rows = vec![row(&["1", "a"]); MAX_LOOKUP_ROWS + 1];
        assert!(set_lookup_table(&mut d.clone(), "B", act, &cols, &rows).is_err());
        let many = vec![col(lk, "", true); MAX_COLUMNS + 1];
        assert!(set_lookup_table(&mut d.clone(), "B", act, &many, &[]).is_err());
        for cell in ["NaN", "inf", "-1", "1e400"] {
            assert!(set_lookup_table(&mut d.clone(), "B", act, &cols, &[row(&[cell, "a"])]).is_err(), "{cell}");
        }
        assert!(set_lookup_table(&mut d.clone(), "Nope", act, &cols, &[]).is_err());
        assert!(set_lookup_table(&mut d.clone(), "B", 9999, &cols, &[]).is_err());
    }
}
