use super::draw::entity;
use super::props::{resolve, resolve_common, transparency};
use super::text_entity::{mtext, place_text_entity};
use super::{Builder, Clip, Color, Ctx, Entity, EntityKind, Ink, Lineweight, Mat3, Vec2, append_clipped_keep, clip};

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
    if let Some(blk) = ctx.d.block(&ins.block) {
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
                // XCLIP: the copy is drawn on its own, then cut to the boundary.
                let clip = xclip_region(ins, &sub.xf);
                let mut part = clip.as_ref().map(|_| b.part());
                let target = part.as_mut().unwrap_or(&mut *b);
                let mut stop = false;
                for be in blk.entities.iter() {
                    // Constant/visible attribute definitions inside blocks are not drawn; attribs are.
                    if matches!(be.kind, EntityKind::AttDef(_)) {
                        continue;
                    }
                    if !target.expand() {
                        stop = true;
                        break;
                    }
                    entity(target, &sub, be);
                }
                if let (Some(part), Some((region, inverted))) = (part, &clip) {
                    b.list.expanded = part.list.expanded;
                    append_clipped_keep(&mut b.list, &part.list, &if *inverted { Clip::Outside(region) } else { Clip::Poly(region) });
                    xclip_frame(b, ctx, region, rgb);
                }
                if stop {
                    break 'copies;
                }
            }
        }
    }
    // Attributes are cut by the reference's clip too.
    let attr_clip = ctx.d.block(&ins.block).and_then(|blk| xclip_region(ins, &ctx.xf.then_before(ins.transform(blk.base.xy()))));
    let mut part = attr_clip.as_ref().map(|_| b.part());
    attributes(part.as_mut().unwrap_or(&mut *b), ctx, e, ins, rgb);
    if let (Some(part), Some((region, inverted))) = (part, &attr_clip) {
        append_clipped_keep(&mut b.list, &part.list, &if *inverted { Clip::Outside(region) } else { Clip::Poly(region) });
    }
}

/// The XCLIP region of reference `ins` drawn through `xf` (block to drawing coordinates), and
/// whether it is inverted. `None` when the reference isn't clipped (no boundary, clipping off, a
/// degenerate outline).
fn xclip_region(ins: &cadcraft_doc::Insert, xf: &Mat3) -> Option<(clip::PolyClip, bool)> {
    let c = ins.clip.as_ref().filter(|c| c.active())?;
    let world: Vec<Vec2> = c.polygon().into_iter().map(|p| xf.apply(p)).collect();
    Some((clip::PolyClip::new(&world)?, c.inverted))
}

/// The clipping boundary's frame as XCLIPFRAME says: 0 hidden, 1 shown and plotted, 2 shown but
/// not plotted (the default).
fn xclip_frame(b: &mut Builder, ctx: &Ctx, region: &clip::PolyClip, rgb: Ink) {
    let mode = ctx.d.header.i64("XCLIPFRAME", 2);
    if mode == 0 || (b.plotting && mode != 1) {
        return;
    }
    let mut frame = region.polygon().to_vec();
    if let Some(first) = frame.first().copied() {
        frame.push(first);
    }
    // The outline is in drawing coordinates already.
    let top = Ctx { xf: Mat3::IDENTITY, ..ctx.clone() };
    b.polyline(&top, rgb, 0.0, &frame);
}

/// The attributes of block reference `e` (`ins`).
fn attributes(b: &mut Builder, ctx: &Ctx, e: &Entity, ins: &cadcraft_doc::Insert, rgb: Ink) {
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
            let def = ctx.d.block(&ins.block).and_then(|blk| blk.entities.iter().find(|be| attdef_tag(be, &a.tag)));
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

/// The attribute definition with tag `tag` (any case).
fn attdef_tag(e: &Entity, tag: &str) -> bool {
    matches!(&e.kind, EntityKind::AttDef(d) if d.tag.eq_ignore_ascii_case(tag))
}
