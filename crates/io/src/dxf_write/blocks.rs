use super::{
    Block, Color, Common, Dimension, Drawing, Entity, EntityKind, EntityStore, Handle, HashMap, Line, MText, Solid, Table, Vec2, Vec3, dxf_ext,
};

/// What happens to the anonymous `*D` blocks read from a file (#495): a dimension that still
/// names its block regenerates its geometry under that name, keeping the block's entity
/// handles, and `*D` blocks nothing else references are not written again.
pub(super) struct DimBlockPlan {
    /// Dimension (model or paper space) → the read block whose name and handles it reuses.
    pub(super) reuse: HashMap<Handle, std::sync::Arc<Block>>,
    /// Upper-case names of `*D` blocks not written as ordinary blocks.
    pub(super) skip: std::collections::HashSet<String>,
}

fn is_dim_block(name: &str) -> bool {
    name.get(..2).is_some_and(|p| p.eq_ignore_ascii_case("*D"))
}

pub(super) fn dim_block_plan(d: &Drawing) -> DimBlockPlan {
    let spaces: Vec<&EntityStore> = std::iter::once(&d.model).chain(d.layouts.iter().map(|l| &l.entities)).collect();
    // Block names other objects use: inserts and multileader contents anywhere, and dimensions
    // inside block definitions (their blocks are written as read).
    let mut used = std::collections::HashSet::new();
    let note = |e: &Entity, dims: bool, used: &mut std::collections::HashSet<String>| match &e.kind {
        EntityKind::Insert(i) => {
            used.insert(i.block.to_ascii_uppercase());
        }
        EntityKind::MLeader(m) => used.extend(m.block.as_ref().map(|b| b.block.to_ascii_uppercase())),
        EntityKind::Dimension(dm) if dims => used.extend(dm.block.as_ref().map(|n| n.to_ascii_uppercase())),
        _ => {}
    };
    for e in spaces.iter().flat_map(|s| s.iter()) {
        note(e, false, &mut used);
    }
    for e in d.blocks.values().flat_map(|b| b.entities.iter()) {
        note(e, true, &mut used);
    }
    let skip = d.blocks.keys().filter(|n| is_dim_block(n)).map(|n| n.to_ascii_uppercase()).filter(|n| !used.contains(n)).collect();
    let mut plan = DimBlockPlan { reuse: HashMap::new(), skip };
    let mut claimed = std::collections::HashSet::new();
    for e in spaces.iter().flat_map(|s| s.iter()) {
        let EntityKind::Dimension(dm) = &e.kind else { continue };
        let Some(b) = dm.block.as_deref().filter(|n| is_dim_block(n)).and_then(|n| d.block(n)) else { continue };
        let key = b.name.to_ascii_uppercase();
        if plan.skip.contains(&key) && claimed.insert(key) {
            plan.reuse.insert(e.handle, b.clone());
        }
    }
    plan
}

/// Give regenerated dimension geometry the handles of the block it replaces, in order; the rest
/// keep handle 0 and get fresh ones.
pub(super) fn reuse_handles(ents: &mut [Entity], old: &Block, taken: &mut std::collections::HashSet<Handle>) {
    let mut handles = old.entities.iter().map(|e| e.handle).filter(|h| h.0 != 0);
    for e in ents.iter_mut() {
        match handles.by_ref().find(|h| taken.insert(*h)) {
            Some(h) => e.handle = h,
            None => break,
        }
    }
}

/// Anonymous dimension blocks (`*D1`…) with the rendered geometry, as consumers expect.
pub(super) fn dim_block_entities(d: &Drawing, dm: &Dimension, layer: &str) -> Vec<Entity> {
    // The dimension as drawn: its overrides, DIMSCALE (0 = the drawing's) and text font.
    let style = d.dim_style(&dm.style).cloned().unwrap_or_default().with_overrides(&dm.overrides);
    let g = cadcraft_render::dimension_in(d, dm);
    let mut out = Vec::new();
    // Each part in its style colour (DIMCLRD, DIMCLRE, DIMCLRT), ByBlock when the style says so,
    // as AutoCAD writes them: readers that draw the block drew every part ByBlock, i.e. in the
    // dimension's own colour (#1052).
    let in_color = |color: Color| Common { layer: layer.into(), color, ..Common::default() };
    let (dim_c, ext_c, text_c) = (in_color(style.dim_line_color), in_color(style.ext_line_color), in_color(style.text_color));
    let line_entities = |i: usize, l: &[Vec2]| -> Vec<Entity> {
        let c = match g.line_roles.get(i) {
            Some(cadcraft_render::LineRole::Ext) => &ext_c,
            _ => &dim_c,
        };
        l.windows(2)
            .filter_map(|seg| match seg {
                [a, b] => Some(Entity { handle: Handle(0), common: c.clone(), kind: EntityKind::Line(Line { a: a.to3(0.0), b: b.to3(0.0) }) }),
                _ => None,
            })
            .collect()
    };
    // The text's marks (arc length symbol, DIMINSPECT frame) go after the text background.
    for (i, l) in g.lines.iter().enumerate().filter(|(i, _)| !g.text_marks.contains(i)) {
        out.extend(line_entities(i, l));
    }
    for t in &g.fills {
        if let [a, b, cc] = t.as_slice() {
            out.push(Entity {
                handle: Handle(0),
                common: dim_c.clone(),
                kind: EntityKind::Solid(Solid { corners: [a.to3(0.0), b.to3(0.0), cc.to3(0.0), cc.to3(0.0)] }),
            });
        }
    }
    // DIMTFILL 2: the text background in DIMTFILLCLR, under the text and its marks (SOLID
    // corners go 1, 2, 4, 3).
    if style.text_fill == 2
        && let [p0, p1, p2, p3] = g.text_fill.as_slice()
    {
        out.push(Entity {
            handle: Handle(0),
            common: in_color(style.text_fill_color),
            kind: EntityKind::Solid(Solid { corners: [p0.to3(0.0), p1.to3(0.0), p3.to3(0.0), p2.to3(0.0)] }),
        });
    }
    for (i, l) in g.lines.iter().enumerate().filter(|(i, _)| g.text_marks.contains(i)) {
        out.extend(line_entities(i, l));
    }
    if !g.value.is_empty() {
        out.push(Entity {
            handle: Handle(0),
            common: text_c,
            kind: EntityKind::MText(MText {
                insert: g.text_pos.to3(0.0),
                height: g.text_height,
                width: 0.0,
                attach: 5,
                rotation: g.text_angle,
                style: style.text_style.clone(),
                contents: g.mtext_contents(),
                line_spacing: 1.0,
                line_spacing_exact: false,
                columns: None,
            }),
        });
    }
    out
}

/// Rows and columns written for a table (bounded like the renderer).
pub(super) fn table_size(t: &Table) -> (usize, usize) {
    (t.row_heights.len().min(10_000), t.col_widths.len().min(10_000))
}

/// The anonymous `*T` block of a table: cell fills, borders and texts as its cell styles draw
/// them, for readers that only draw the block.
///
/// The block is relative to its own origin: the ACAD_TABLE reference adds the insertion point.
pub(crate) fn table_block_entities(d: &Drawing, t: &Table, layer: &str) -> Vec<Entity> {
    let at_origin = Table { insert: Vec3::ZERO, ..t.clone() };
    let g = cadcraft_render::table_geometry(d, &at_origin);
    let c = Common { layer: layer.into(), color: Color::ByBlock, ..Common::default() };
    let mut out = Vec::new();
    for f in &g.fills {
        let [a, b, cc, dd] = f.corners.map(|p| p.to3(0.0));
        // SOLID corners go 1, 2, 4, 3.
        out.push(Entity {
            handle: Handle(0),
            common: Common { color: f.color, ..c.clone() },
            kind: EntityKind::Solid(Solid { corners: [a, b, dd, cc] }),
        });
    }
    for l in &g.lines {
        out.push(Entity {
            handle: Handle(0),
            common: Common { color: l.color, lineweight: l.lineweight, ..c.clone() },
            kind: EntityKind::Line(Line { a: l.a.to3(0.0), b: l.b.to3(0.0) }),
        });
    }
    let mut texts = Vec::new();
    for cell in &g.texts {
        if !cell.text.is_empty() {
            let style = if d.text_style(&cell.style).is_some() { cell.style.clone() } else { "Standard".into() };
            texts.push(Entity {
                handle: Handle(0),
                common: Common { color: cell.color, ..c.clone() },
                kind: EntityKind::MText(MText {
                    insert: cell.at.to3(0.0),
                    height: cell.height,
                    width: 0.0,
                    attach: cell.attach,
                    rotation: 0.0,
                    style,
                    contents: cell.text.clone(),
                    line_spacing: 1.0,
                    line_spacing_exact: false,
                    columns: None,
                }),
            });
        }
    }
    out.extend(texts);
    out
}

/// Our own geometry for an arrowhead block: unit size, tip at the origin, pointing along +X.
pub(super) fn arrow_block_entities(kind: cadcraft_render::Arrowhead) -> Vec<Entity> {
    let g = cadcraft_render::arrowhead(kind, Vec2::ZERO, Vec2::X, 1.0);
    let c = Common { layer: "0".into(), color: Color::ByBlock, ..Common::default() };
    let mut out = Vec::new();
    for l in &g.lines {
        for seg in l.windows(2) {
            if let [a, b] = seg {
                out.push(Entity { handle: Handle(0), common: c.clone(), kind: EntityKind::Line(Line { a: a.to3(0.0), b: b.to3(0.0) }) });
            }
        }
    }
    for t in g.tris.chunks(3) {
        if let [a, b, cc] = t {
            out.push(Entity {
                handle: Handle(0),
                common: c.clone(),
                kind: EntityKind::Solid(Solid { corners: [a.to3(0.0), b.to3(0.0), cc.to3(0.0), cc.to3(0.0)] }),
            });
        }
    }
    out
}

/// Whether the entities of the block `name` are the arrowhead block CADCraft writes for that
/// standard arrow name ([`arrow_block_entities`]): the same lines and solids with the same
/// properties (layer 0, ByBlock). Any other content (AutoCAD's own `_Dot`, a user's
/// redefinition, an older CADCraft's geometry) is the file's.
pub(crate) fn is_generated_arrow(name: &str, entities: &[&Entity]) -> bool {
    let Some((_, kind)) = dxf_ext::arrow_block_name(name) else { return false };
    let ours = arrow_block_entities(kind);
    ours.len() == entities.len() && ours.iter().zip(entities).all(|(a, b)| a.kind == b.kind && a.common == b.common)
}
