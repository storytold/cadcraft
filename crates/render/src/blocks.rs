use std::collections::HashMap;
use std::sync::Arc;

use super::draw::entity;
use super::props::{resolve, resolve_common, transparency};
use super::text_entity::{mtext, place_text_entity};
use super::{Builder, Color, Ctx, Entity, EntityKind, Ink, Lineweight, Mat3, Vec2};

/// What drawing the references of one block needs from its definition, worked out once per
/// display list rather than for every reference or MINSERT copy.
pub(crate) struct BlockPlan {
    /// The block's entities a copy draws, in drawing order: all but its attribute definitions,
    /// which only give attributes their properties.
    drawn: Vec<Arc<Entity>>,
    /// The block's first attribute definition with each tag (lower case).
    defs: HashMap<String, Arc<Entity>>,
}

impl BlockPlan {
    fn of(blk: &cadcraft_doc::Block) -> Self {
        let mut plan = BlockPlan { drawn: Vec::with_capacity(blk.entities.len()), defs: HashMap::new() };
        for be in blk.entities.iter() {
            match &be.kind {
                EntityKind::AttDef(d) => {
                    plan.defs.entry(d.tag.to_ascii_lowercase()).or_insert_with(|| be.clone());
                }
                _ => plan.drawn.push(be.clone()),
            }
        }
        plan
    }
}

/// The plans of the blocks a display list has referenced so far, by block name.
pub(crate) type Plans = HashMap<String, Arc<BlockPlan>>;

/// The plan of `blk`, worked out at its first reference in this list.
fn block_plan(b: &mut Builder, blk: &cadcraft_doc::Block) -> Arc<BlockPlan> {
    if let Some(plan) = b.blocks.get(&blk.name) {
        return plan.clone();
    }
    let plan = Arc::new(BlockPlan::of(blk));
    b.blocks.insert(blk.name.clone(), plan.clone());
    plan
}

pub(crate) fn sub_ctx<'a>(ctx: &Ctx<'a>, e: &Entity, m: Mat3) -> Ctx<'a> {
    let layer = if e.common.layer == "0" { ctx.block_layer.clone() } else { Some(e.common.layer.clone()) };
    let color = match e.common.color {
        Color::ByBlock => ctx.block_color,
        Color::ByLayer => ctx.layer_color(layer.as_deref().unwrap_or("0")).unwrap_or(Color::Index(7)),
        c => c,
    };
    let lw = match e.common.lineweight {
        Lineweight::ByBlock => ctx.block_lw,
        Lineweight::ByLayer => ctx.d.layer(layer.as_deref().unwrap_or("0")).map(|l| l.lineweight).unwrap_or(Lineweight::Default),
        x => x,
    };
    let lt = match e.common.linetype.to_ascii_lowercase().as_str() {
        "byblock" => ctx.block_ltype.clone(),
        "bylayer" => ctx.d.layer(layer.as_deref().unwrap_or("0")).map(|l| l.linetype.clone()).unwrap_or_else(|| "Continuous".into()),
        _ => e.common.linetype.clone(),
    };
    Ctx {
        d: ctx.d,
        xf: ctx.xf.then_before(m),
        block_color: color,
        block_layer: layer,
        block_lw: lw,
        block_ltype: lt,
        depth: ctx.depth + 1,
        top: ctx.top,
        frozen: ctx.frozen,
        vp_colors: ctx.vp_colors,
        // The reference's own linetype scale (group 48) multiplies that of the objects inside,
        // as LTSCALE does for the drawing (tests/block_ltscale.rs).
        lt_factor: ctx.lt_factor * if e.common.ltscale.is_finite() && e.common.ltscale > 0.0 { e.common.ltscale } else { 1.0 },
        block_pstyle: ctx.block_pstyle,
        // The reference's own, set by the caller (`Builder::transparency` while it draws).
        block_transparency: ctx.block_transparency,
    }
}

pub(crate) fn insert(b: &mut Builder, ctx: &Ctx, e: &Entity, ins: &cadcraft_doc::Insert, rgb: Ink) {
    if ctx.depth >= cadcraft_doc::MAX_BLOCK_DEPTH {
        return;
    }
    let blk = ctx.d.block(&ins.block);
    let plan = blk.map(|blk| block_plan(b, blk));
    // A block with nothing a copy draws (only attribute definitions, or nothing) has no copies to
    // make, however many a MINSERT asks for.
    if let (Some(blk), Some(plan)) = (blk, &plan)
        && !plan.drawn.is_empty()
    {
        let cols = ins.cols.clamp(1, 10_000);
        let rows = ins.rows.clamp(1, 10_000);
        'copies: for r in 0..rows {
            for c in 0..cols {
                if !b.expand() {
                    break 'copies;
                }
                let off = Vec2::new(ins.col_spacing * f64::from(c), ins.row_spacing * f64::from(r)).rotate(ins.rotation);
                let m = Mat3::translate(off).then_before(ins.transform(blk.base.xy()));
                let sub = Ctx { block_pstyle: b.named, block_transparency: b.transparency, ..sub_ctx(ctx, e, m) };
                // Only what a copy draws is visited, each charged once: attribute definitions
                // (attributes draw instead) cost no budget and no time, however many there are.
                for be in &plan.drawn {
                    if !b.expand() {
                        break 'copies;
                    }
                    entity(b, &sub, be);
                }
            }
        }
    }
    if b.opts.text {
        // ATTMODE (ATTDISP): 0 shows no attributes, 1 the visible ones, 2 all of them.
        let attmode = ctx.d.header.i64("ATTMODE", 1);
        // The reference's transparency; each attribute draws with its own.
        let reference = b.transparency;
        for a in &ins.attribs {
            if attmode == 0 || (a.invisible && attmode != 2) {
                continue;
            }
            // A multiline attribute is drawn as its multiline text.
            let multiline = a.multiline_text();
            let sh = if multiline.is_none() { Some(place_text_entity(ctx.d, &a.text, &a.text.value).0) } else { None };
            // Properties of the attribute's definition (as AutoCAD copies them to the attribute
            // on insertion), except those set on the attribute itself: ByBlock = the reference's,
            // layer 0 = the reference's layer.
            // The block's first definition with the attribute's tag, any case (from its plan).
            let def = plan.as_ref().and_then(|plan| plan.defs.get(&a.tag.to_ascii_lowercase())).map(|be| be.as_ref());
            let (ink, lw, shown, t) = match (def, a.props.is_empty()) {
                (None, true) => (rgb, resolve(ctx, e, b.plotting).1, true, reference),
                (def, _) => {
                    let c = a.props.over(&def.map(|d| d.common.clone()).unwrap_or_else(by_block));
                    let (ink, lw, _, _, shown, layer_t) = resolve_common(&sub_ctx(ctx, e, Mat3::IDENTITY), &c, b.plotting);
                    let t = if b.opts.transparency { transparency(c.transparency, layer_t, reference) } else { 0 };
                    (ink, lw, shown, t)
                }
            };
            if shown {
                b.transparency = t;
                let lw = if b.opts.lineweights { lw } else { 0.0 };
                match (&multiline, &sh) {
                    (Some(m), _) => mtext(b, ctx, m, ink, lw),
                    (None, Some(sh)) => b.shaped(ctx, ink, lw, sh),
                    (None, None) => {}
                }
            }
        }
        b.transparency = reference;
    }
}

/// Properties that follow the block reference's (an attribute without a definition).
fn by_block() -> cadcraft_doc::Common {
    cadcraft_doc::Common {
        color: Color::ByBlock,
        linetype: "ByBlock".into(),
        lineweight: Lineweight::ByBlock,
        transparency: cadcraft_doc::Transparency::ByBlock,
        ..cadcraft_doc::Common::default()
    }
}
