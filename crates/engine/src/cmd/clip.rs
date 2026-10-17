//! XCLIP, IMAGECLIP and CLIP: clipping boundaries for block references, xrefs and images.
//!
//! An external reference or block reference keeps a 2D boundary polygon in its coordinate
//! space ([`cadcraft_doc::ClipBoundary`]), applied when drawing and plotting.
//! XCLIP sets, turns on/off, deletes or generates a polyline from the boundary.
//! IMAGECLIP sets, turns on/off or deletes a pixel-space boundary on raster images.
//! CLIP dispatches to XCLIP, IMAGECLIP or VPCLIP depending on what is selected.

use cadcraft_doc::{ClipBoundary, Common, Drawing, EntityKind, Handle, Image, Space};
use cadcraft_geom::{PolyVertex, Vec2};
use serde_json::{Value, json};

use super::curves::{self, handle_value};
use super::helpers::lwpoly;
use super::{CommandSpec, bad, bool_or, has_doc, points_param, str_param, targets};
use crate::{Accept, EngineError, Input, Interactive, Prompt, Result, Session, Step};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("xclip", "Clip Xref", run_xclip)
            .menu(&["Modify", "Clip", "Xref"])
            .alias(&["xc"])
            .params("{handle? (block or xref insert; default: the selected one), points?: [[x, y], …], rect?: [[x1, y1], [x2, y2]], object?: handle, on?: bool, off?: bool, delete?: bool, invert?: bool, generate_polyline?: bool} → {handle, clip?, enabled?, polyline?}")
            .enabled(has_doc)
            .interactive(|_| Ok(Box::new(XclipM::default()))),
        CommandSpec::new("imageclip", "Clip Image", run_imageclip)
            .menu(&["Modify", "Clip", "Image"])
            .alias(&["icl"])
            .params("{handle? (image; default: the selected one), points?: [[x, y], …], rect?: [[x1, y1], [x2, y2]], object?: handle, on?: bool, off?: bool, delete?: bool, generate_polyline?: bool} → {handle, clip?, enabled?, polyline?}")
            .enabled(has_doc)
            .interactive(|_| Ok(Box::new(ImageclipM::default()))),
        CommandSpec::new("clip", "Clip", run_clip)
            .alias(&["cl"])
            .params("{handle?, ... (dispatches to xclip, imageclip or vpclip based on the target entity)}")
            .enabled(has_doc)
            .interactive(|_| Ok(Box::new(ClipM::default()))),
    ]
}

// ---------------- Helpers ----------------

/// Find an object's boundary polygon in world coordinates.
fn object_boundary_points(d: &Drawing, h: Handle) -> Result<Vec<Vec2>> {
    let e = d.entity(h).ok_or_else(|| bad("clip", format!("object {} not found", h.hex())))?;
    cadcraft_render::clip::boundary_polygon(&e.kind)
        .ok_or_else(|| bad("clip", "selected object is not a closed curve (must be a closed polyline, circle, ellipse or spline with area)"))
}

/// Convert an image's pixel coordinate to world coordinates.
fn image_pixel_to_world(img: &Image, px: Vec2) -> Vec2 {
    img.insert.xy() + img.u.xy() * (px.x * img.pixel_size.x) + img.v.xy() * (px.y * img.pixel_size.y)
}

/// Convert a world coordinate to an image's pixel coordinates.
fn image_world_to_pixel(img: &Image, w: Vec2) -> Option<Vec2> {
    let rel = w - img.insert.xy();
    let ux = img.u.xy() * img.pixel_size.x;
    let vy = img.v.xy() * img.pixel_size.y;
    let det = ux.cross(vy);
    if det.abs() < 1e-12 {
        return None;
    }
    let px = rel.cross(vy) / det;
    let py = ux.cross(rel) / det;
    Some(Vec2::new(px, py))
}

fn rect_points(p0: Vec2, p1: Vec2) -> Vec<Vec2> {
    vec![p0, Vec2::new(p1.x, p0.y), p1, Vec2::new(p0.x, p1.y)]
}

fn parse_boundary_points(cmd: &str, s: &Session, p: &Value) -> Result<Vec<Vec2>> {
    if let Some(o) = p.get("object") {
        let oh = handle_value(o).ok_or_else(|| bad(cmd, "`object` must be a handle"))?;
        object_boundary_points(s.doc()?, oh)
    } else if let Some(rect) = p.get("rect") {
        let a = rect.as_array().ok_or_else(|| bad(cmd, "`rect` must be [[x1, y1], [x2, y2]]"))?;
        if a.len() != 2 {
            return Err(bad(cmd, "`rect` must contain exactly two corner points"));
        }
        let p0 = a.first().and_then(super::point_value).ok_or_else(|| bad(cmd, "invalid rect point 0"))?;
        let p1 = a.get(1).and_then(super::point_value).ok_or_else(|| bad(cmd, "invalid rect point 1"))?;
        Ok(rect_points(p0, p1))
    } else if p.get("points").is_some() {
        let pts = points_param(p, "points").ok_or_else(|| bad(cmd, "`points` must be [[x, y], …]"))?;
        if pts.len() < 2 {
            return Err(bad(cmd, "a clipping boundary needs at least two points (corners of a box) or 3+ for a polygon"));
        }
        if pts.len() == 2 {
            let (Some(p0), Some(p1)) = (pts.first(), pts.get(1)) else {
                return Err(bad(cmd, "two points expected"));
            };
            Ok(rect_points(*p0, *p1))
        } else {
            Ok(pts)
        }
    } else {
        Err(bad(cmd, "pass `delete`, `on`, `off`, `generate_polyline`, `object`, `rect` or `points`"))
    }
}

// ---------------- XCLIP ----------------

pub fn run_xclip(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "xclip";
    let d = s.doc()?;
    let ih = match str_param(p, "handle") {
        Some(h) => Handle::parse_hex(h).ok_or_else(|| bad(cmd, format!("invalid handle `{h}`")))?,
        None => targets(s, p)?
            .into_iter()
            .find(|h| d.entity(*h).is_some_and(|e| matches!(e.kind, EntityKind::Insert(_))))
            .ok_or_else(|| bad(cmd, "no block reference or xref given (pass `handle` or select one)"))?,
    };

    let Some(EntityKind::Insert(mut ins)) = d.entity(ih).map(|e| e.kind.clone()) else {
        return Err(bad(cmd, "selected object is not a block reference or external reference"));
    };
    let blk = d.block(&ins.block).cloned().ok_or_else(|| bad(cmd, format!("block \"{}\" not found", ins.block)))?;
    let to_world = ins.transform(blk.base.xy());
    let to_block = to_world.inverse().ok_or_else(|| bad(cmd, "block reference transformation is degenerate"))?;

    if bool_or(p, "delete", false) {
        if ins.clip.is_none() {
            return Err(bad(cmd, "the reference has no clipping boundary to delete"));
        }
        ins.clip = None;
        s.doc_mut()?.modify_entity(ih, |e| e.kind = EntityKind::Insert(ins))?;
        return Ok(json!({ "handle": ih.hex(), "clip": Value::Null }));
    }

    if bool_or(p, "on", false) {
        let Some(cb) = &mut ins.clip else {
            return Err(bad(cmd, "the reference has no clipping boundary"));
        };
        cb.enabled = true;
        s.doc_mut()?.modify_entity(ih, |e| e.kind = EntityKind::Insert(ins))?;
        return Ok(json!({ "handle": ih.hex(), "enabled": true }));
    }

    if bool_or(p, "off", false) {
        let Some(cb) = &mut ins.clip else {
            return Err(bad(cmd, "the reference has no clipping boundary"));
        };
        cb.enabled = false;
        s.doc_mut()?.modify_entity(ih, |e| e.kind = EntityKind::Insert(ins))?;
        return Ok(json!({ "handle": ih.hex(), "enabled": false }));
    }

    if bool_or(p, "generate_polyline", false) {
        let Some(cb) = &ins.clip else {
            return Err(bad(cmd, "the reference has no clipping boundary"));
        };
        let w_pts: Vec<Vec2> = cb.points.iter().map(|pt| to_world.apply(*pt)).collect();
        let poly = lwpoly(w_pts.iter().map(|pt| PolyVertex::new(*pt)).collect(), true);
        let space = s.doc()?.space_of(ih).unwrap_or(Space::Model);
        let ph = s.doc_mut()?.add(&space, Common::default(), poly)?;
        return Ok(json!({ "handle": ih.hex(), "polyline": ph.hex() }));
    }

    // New boundary from object, points, or rect.
    let invert = bool_or(p, "invert", false);
    let world_pts = parse_boundary_points(cmd, s, p)?;

    if world_pts.len() < 3 {
        return Err(bad(cmd, "the boundary polygon must have at least three vertices"));
    }
    let block_pts: Vec<Vec2> = world_pts.iter().map(|pt| to_block.apply(*pt)).collect();
    if cadcraft_render::clip::PolyClip::new(&block_pts).is_none() {
        return Err(bad(cmd, "the clipping boundary is degenerate or has no area"));
    }

    ins.clip = Some(ClipBoundary { points: block_pts.clone(), enabled: true, inverted: invert });
    s.doc_mut()?.modify_entity(ih, |e| e.kind = EntityKind::Insert(ins))?;
    Ok(json!({
        "handle": ih.hex(),
        "points": block_pts.iter().map(|pt| [pt.x, pt.y]).collect::<Vec<_>>(),
        "inverted": invert,
    }))
}

// ---------------- IMAGECLIP ----------------

pub fn run_imageclip(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "imageclip";
    let d = s.doc()?;
    let ih = match str_param(p, "handle") {
        Some(h) => Handle::parse_hex(h).ok_or_else(|| bad(cmd, format!("invalid handle `{h}`")))?,
        None => targets(s, p)?
            .into_iter()
            .find(|h| d.entity(*h).is_some_and(|e| matches!(e.kind, EntityKind::Image(_))))
            .ok_or_else(|| bad(cmd, "no image given (pass `handle` or select an image)"))?,
    };

    let Some(EntityKind::Image(mut img)) = d.entity(ih).map(|e| e.kind.clone()) else {
        return Err(bad(cmd, "selected object is not an image"));
    };

    if bool_or(p, "delete", false) {
        if img.clip.is_empty() && !img.clipping {
            return Err(bad(cmd, "the image has no clipping boundary to delete"));
        }
        img.clip.clear();
        img.clipping = false;
        s.doc_mut()?.modify_entity(ih, |e| e.kind = EntityKind::Image(img))?;
        return Ok(json!({ "handle": ih.hex(), "clip": Value::Null }));
    }

    if bool_or(p, "on", false) {
        if img.clip.is_empty() {
            return Err(bad(cmd, "the image has no clipping boundary"));
        }
        img.clipping = true;
        s.doc_mut()?.modify_entity(ih, |e| e.kind = EntityKind::Image(img))?;
        return Ok(json!({ "handle": ih.hex(), "enabled": true }));
    }

    if bool_or(p, "off", false) {
        if img.clip.is_empty() {
            return Err(bad(cmd, "the image has no clipping boundary"));
        }
        img.clipping = false;
        s.doc_mut()?.modify_entity(ih, |e| e.kind = EntityKind::Image(img))?;
        return Ok(json!({ "handle": ih.hex(), "enabled": false }));
    }

    if bool_or(p, "generate_polyline", false) {
        if img.clip.is_empty() {
            return Err(bad(cmd, "the image has no clipping boundary"));
        }
        let w_pts: Vec<Vec2> = img.clip.iter().map(|px| image_pixel_to_world(&img, *px)).collect();
        let poly = lwpoly(w_pts.iter().map(|pt| PolyVertex::new(*pt)).collect(), true);
        let space = s.doc()?.space_of(ih).unwrap_or(Space::Model);
        let ph = s.doc_mut()?.add(&space, Common::default(), poly)?;
        return Ok(json!({ "handle": ih.hex(), "polyline": ph.hex() }));
    }

    let world_pts = parse_boundary_points(cmd, s, p)?;

    let mut pixel_pts = Vec::with_capacity(world_pts.len());
    for w in &world_pts {
        let px = image_world_to_pixel(&img, *w).ok_or_else(|| bad(cmd, "image coordinate transform is degenerate"))?;
        pixel_pts.push(px);
    }

    img.clip = pixel_pts.clone();
    img.clipping = true;
    s.doc_mut()?.modify_entity(ih, |e| e.kind = EntityKind::Image(img))?;
    Ok(json!({
        "handle": ih.hex(),
        "points": pixel_pts.iter().map(|pt| [pt.x, pt.y]).collect::<Vec<_>>(),
    }))
}

// ---------------- CLIP ----------------

pub fn run_clip(s: &mut Session, p: &Value) -> Result<Value> {
    let d = s.doc()?;
    let h = match str_param(p, "handle") {
        Some(s) => Handle::parse_hex(s).ok_or_else(|| bad("clip", format!("invalid handle `{s}`")))?,
        None => *targets(s, p)?.first().ok_or_else(|| bad("clip", "no object given (pass `handle` or select an object)"))?,
    };
    let Some(e) = d.entity(h) else {
        return Err(bad("clip", format!("object {} not found", h.hex())));
    };
    match &e.kind {
        EntityKind::Insert(_) => run_xclip(s, p),
        EntityKind::Image(_) => run_imageclip(s, p),
        EntityKind::Viewport(_) => crate::cmd::layout::viewports::run_vpclip(s, p),
        _ => Err(bad("clip", "CLIP operates on external references, blocks, images, or viewports")),
    }
}

// ---------------- Interactive Prompt Machines ----------------

#[derive(Default)]
pub(crate) struct XclipM {
    stage: XclipStage,
}

#[derive(Default)]
enum XclipStage {
    #[default]
    Select,
    Option(Handle),
    BoundaryType(Handle, bool),
    FirstCorner(Handle, bool),
    SecondCorner(Handle, bool, Vec2),
    PolygonFirst(Handle, bool),
    PolygonNext(Handle, bool, Vec<Vec2>),
    SelectPolyline(Handle, bool),
}

impl Interactive for XclipM {
    fn name(&self) -> &'static str {
        "XCLIP"
    }

    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        let sel = s.selection();
        let d = s.doc()?;
        if let Some(h) = sel.iter().find(|h| d.entity(**h).is_some_and(|e| matches!(e.kind, EntityKind::Insert(_)))) {
            let h = *h;
            let clipped = d.entity(h).is_some_and(|e| matches!(&e.kind, EntityKind::Insert(ins) if ins.clip.is_some()));
            if clipped {
                self.stage = XclipStage::Option(h);
            } else {
                self.stage = XclipStage::BoundaryType(h, false);
            }
        }
        Ok(Step::Continue)
    }

    fn prompt(&self, s: &Session) -> Prompt {
        match &self.stage {
            XclipStage::Select => Prompt::new("Select block reference or external reference", Accept::POINT),
            XclipStage::Option(h) => {
                let on = s
                    .doc()
                    .ok()
                    .and_then(|d| d.entity(*h))
                    .is_some_and(|e| matches!(&e.kind, EntityKind::Insert(ins) if ins.clip.as_ref().is_some_and(|c| c.enabled)));
                let state_kw = if on { "OFF" } else { "ON" };
                Prompt::new("Enter clipping option", curves::KW)
                    .kw(&[state_kw, "Delete", "generate Polyline", "New boundary"])
                    .default("New boundary")
            }
            XclipStage::BoundaryType(_, invert) => {
                let inv_str = if *invert { "Invert clip (ON)" } else { "Invert clip (OFF)" };
                Prompt::new("Specify clipping boundary", Accept::POINT)
                    .kw(&["Select polyline", "Polygonal", "Rectangular", inv_str])
                    .default("Rectangular")
            }
            XclipStage::FirstCorner(..) => Prompt::new("Specify first corner", Accept::POINT),
            XclipStage::SecondCorner(_, _, first) => Prompt::new("Specify opposite corner", Accept::POINT).base(*first),
            XclipStage::PolygonFirst(..) => Prompt::new("Specify first point", Accept::POINT),
            XclipStage::PolygonNext(_, _, pts) => {
                let base = pts.last().copied();
                let mut p = Prompt::new("Specify next point or [Close/Undo]", Accept::POINT).kw(&["Close", "Undo"]);
                if let Some(b) = base {
                    p = p.base(b);
                }
                p
            }
            XclipStage::SelectPolyline(..) => Prompt::new("Select polyline", Accept::POINT),
        }
    }

    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match std::mem::take(&mut self.stage) {
            XclipStage::Select => {
                let h = match &i {
                    Input::Point(p) => curves::pick_at(s, *p)
                        .filter(|h| s.doc().is_ok_and(|d| d.entity(*h).is_some_and(|e| matches!(e.kind, EntityKind::Insert(_))))),
                    Input::Pick(hs) => {
                        hs.iter().find(|h| s.doc().is_ok_and(|d| d.entity(**h).is_some_and(|e| matches!(e.kind, EntityKind::Insert(_))))).copied()
                    }
                    _ => None,
                };
                let Some(h) = h else {
                    if matches!(i, Input::Enter) {
                        return Ok(Step::Done);
                    }
                    self.stage = XclipStage::Select;
                    s.echo("Select a block reference or xref.");
                    return Ok(Step::Continue);
                };
                let clipped = s.doc()?.entity(h).is_some_and(|e| matches!(&e.kind, EntityKind::Insert(ins) if ins.clip.is_some()));
                if clipped {
                    self.stage = XclipStage::Option(h);
                } else {
                    self.stage = XclipStage::BoundaryType(h, false);
                }
                Ok(Step::Continue)
            }
            XclipStage::Option(h) => match &i {
                Input::Keyword(k) | Input::Text(k) => match k.trim().to_ascii_lowercase().as_str() {
                    "on" => {
                        run_xclip(s, &json!({ "handle": h.hex(), "on": true }))?;
                        s.echo("Clipping turned ON.");
                        Ok(Step::Done)
                    }
                    "off" | "of" => {
                        run_xclip(s, &json!({ "handle": h.hex(), "off": true }))?;
                        s.echo("Clipping turned OFF.");
                        Ok(Step::Done)
                    }
                    "delete" | "d" => {
                        run_xclip(s, &json!({ "handle": h.hex(), "delete": true }))?;
                        s.echo("Clipping boundary deleted.");
                        Ok(Step::Done)
                    }
                    "generate polyline" | "generate" | "polyline" | "p" => {
                        run_xclip(s, &json!({ "handle": h.hex(), "generate_polyline": true }))?;
                        s.echo("Polyline generated from clipping boundary.");
                        Ok(Step::Done)
                    }
                    "new boundary" | "new" | "n" => {
                        self.stage = XclipStage::BoundaryType(h, false);
                        Ok(Step::Continue)
                    }
                    _ => {
                        self.stage = XclipStage::Option(h);
                        Err(EngineError::Other("Invalid option keyword.".into()))
                    }
                },
                Input::Enter => {
                    self.stage = XclipStage::BoundaryType(h, false);
                    Ok(Step::Continue)
                }
                _ => {
                    self.stage = XclipStage::Option(h);
                    Ok(Step::Continue)
                }
            },
            XclipStage::BoundaryType(h, mut invert) => match &i {
                Input::Point(p) => {
                    self.stage = XclipStage::SecondCorner(h, invert, *p);
                    Ok(Step::Continue)
                }
                Input::Keyword(k) | Input::Text(k) => match k.trim().to_ascii_lowercase().as_str() {
                    "invert clip" | "invert" | "i" => {
                        invert = !invert;
                        s.echo(format!("Invert clip is now {}.", if invert { "ON" } else { "OFF" }));
                        self.stage = XclipStage::BoundaryType(h, invert);
                        Ok(Step::Continue)
                    }
                    "select polyline" | "select" | "s" => {
                        self.stage = XclipStage::SelectPolyline(h, invert);
                        Ok(Step::Continue)
                    }
                    "polygonal" | "p" => {
                        self.stage = XclipStage::PolygonFirst(h, invert);
                        Ok(Step::Continue)
                    }
                    "rectangular" | "r" => {
                        self.stage = XclipStage::FirstCorner(h, invert);
                        Ok(Step::Continue)
                    }
                    _ => {
                        self.stage = XclipStage::BoundaryType(h, invert);
                        Err(EngineError::Other("Invalid boundary option.".into()))
                    }
                },
                Input::Enter => {
                    self.stage = XclipStage::FirstCorner(h, invert);
                    Ok(Step::Continue)
                }
                _ => {
                    self.stage = XclipStage::BoundaryType(h, invert);
                    Ok(Step::Continue)
                }
            },
            XclipStage::FirstCorner(h, invert) => {
                if let Input::Point(p) = i {
                    self.stage = XclipStage::SecondCorner(h, invert, p);
                    Ok(Step::Continue)
                } else {
                    Ok(Step::Done)
                }
            }
            XclipStage::SecondCorner(h, invert, p0) => {
                if let Input::Point(p1) = i {
                    run_xclip(s, &json!({ "handle": h.hex(), "rect": [[p0.x, p0.y], [p1.x, p1.y]], "invert": invert }))?;
                    s.echo("Clipping boundary applied.");
                }
                Ok(Step::Done)
            }
            XclipStage::PolygonFirst(h, invert) => {
                if let Input::Point(p) = i {
                    self.stage = XclipStage::PolygonNext(h, invert, vec![p]);
                    Ok(Step::Continue)
                } else {
                    Ok(Step::Done)
                }
            }
            XclipStage::PolygonNext(h, invert, mut pts) => match i {
                Input::Point(p) => {
                    pts.push(p);
                    self.stage = XclipStage::PolygonNext(h, invert, pts);
                    Ok(Step::Continue)
                }
                Input::Keyword(k) | Input::Text(k) if k.eq_ignore_ascii_case("close") || k.eq_ignore_ascii_case("c") => {
                    if pts.len() < 3 {
                        self.stage = XclipStage::PolygonNext(h, invert, pts);
                        s.echo("At least 3 points required.");
                        return Ok(Step::Continue);
                    }
                    let json_pts: Vec<[f64; 2]> = pts.iter().map(|p| [p.x, p.y]).collect();
                    run_xclip(s, &json!({ "handle": h.hex(), "points": json_pts, "invert": invert }))?;
                    s.echo("Polygonal clipping boundary applied.");
                    Ok(Step::Done)
                }
                Input::Keyword(k) | Input::Text(k) if k.eq_ignore_ascii_case("undo") || k.eq_ignore_ascii_case("u") => {
                    pts.pop();
                    if pts.is_empty() {
                        self.stage = XclipStage::PolygonFirst(h, invert);
                    } else {
                        self.stage = XclipStage::PolygonNext(h, invert, pts);
                    }
                    Ok(Step::Continue)
                }
                Input::Enter => {
                    if pts.len() >= 3 {
                        let json_pts: Vec<[f64; 2]> = pts.iter().map(|p| [p.x, p.y]).collect();
                        run_xclip(s, &json!({ "handle": h.hex(), "points": json_pts, "invert": invert }))?;
                        s.echo("Polygonal clipping boundary applied.");
                    }
                    Ok(Step::Done)
                }
                _ => {
                    self.stage = XclipStage::PolygonNext(h, invert, pts);
                    Ok(Step::Continue)
                }
            },
            XclipStage::SelectPolyline(h, invert) => {
                let oh = match &i {
                    Input::Point(p) => curves::pick_at(s, *p),
                    Input::Pick(hs) => hs.first().copied(),
                    _ => None,
                };
                if let Some(oh) = oh {
                    run_xclip(s, &json!({ "handle": h.hex(), "object": oh.hex(), "invert": invert }))?;
                    s.echo("Clipping boundary applied from polyline.");
                }
                Ok(Step::Done)
            }
        }
    }
}

// ---------------- IMAGECLIP Prompt Machine ----------------

#[derive(Default)]
pub(crate) struct ImageclipM {
    stage: ImageclipStage,
}

#[derive(Default)]
enum ImageclipStage {
    #[default]
    Select,
    Option(Handle),
    BoundaryType(Handle),
    FirstCorner(Handle),
    SecondCorner(Handle, Vec2),
    SelectPolyline(Handle),
}

impl Interactive for ImageclipM {
    fn name(&self) -> &'static str {
        "IMAGECLIP"
    }

    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        let sel = s.selection();
        let d = s.doc()?;
        if let Some(h) = sel.iter().find(|h| d.entity(**h).is_some_and(|e| matches!(e.kind, EntityKind::Image(_)))) {
            let h = *h;
            let clipped = d.entity(h).is_some_and(|e| matches!(&e.kind, EntityKind::Image(img) if !img.clip.is_empty()));
            if clipped {
                self.stage = ImageclipStage::Option(h);
            } else {
                self.stage = ImageclipStage::BoundaryType(h);
            }
        }
        Ok(Step::Continue)
    }

    fn prompt(&self, s: &Session) -> Prompt {
        match &self.stage {
            ImageclipStage::Select => Prompt::new("Select image to clip", Accept::POINT),
            ImageclipStage::Option(h) => {
                let on = s.doc().ok().and_then(|d| d.entity(*h)).is_some_and(|e| matches!(&e.kind, EntityKind::Image(img) if img.clipping));
                let state_kw = if on { "OFF" } else { "ON" };
                Prompt::new("Enter clipping option", curves::KW)
                    .kw(&[state_kw, "Delete", "generate Polyline", "New boundary"])
                    .default("New boundary")
            }
            ImageclipStage::BoundaryType(..) => {
                Prompt::new("Specify clipping boundary", Accept::POINT).kw(&["Select polyline", "Rectangular"]).default("Rectangular")
            }
            ImageclipStage::FirstCorner(..) => Prompt::new("Specify first corner", Accept::POINT),
            ImageclipStage::SecondCorner(_, first) => Prompt::new("Specify opposite corner", Accept::POINT).base(*first),
            ImageclipStage::SelectPolyline(..) => Prompt::new("Select polyline", Accept::POINT),
        }
    }

    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match std::mem::take(&mut self.stage) {
            ImageclipStage::Select => {
                let h = match &i {
                    Input::Point(p) => {
                        curves::pick_at(s, *p).filter(|h| s.doc().is_ok_and(|d| d.entity(*h).is_some_and(|e| matches!(e.kind, EntityKind::Image(_)))))
                    }
                    Input::Pick(hs) => {
                        hs.iter().find(|h| s.doc().is_ok_and(|d| d.entity(**h).is_some_and(|e| matches!(e.kind, EntityKind::Image(_))))).copied()
                    }
                    _ => None,
                };
                let Some(h) = h else {
                    if matches!(i, Input::Enter) {
                        return Ok(Step::Done);
                    }
                    self.stage = ImageclipStage::Select;
                    s.echo("Select an image.");
                    return Ok(Step::Continue);
                };
                let clipped = s.doc()?.entity(h).is_some_and(|e| matches!(&e.kind, EntityKind::Image(img) if !img.clip.is_empty()));
                if clipped {
                    self.stage = ImageclipStage::Option(h);
                } else {
                    self.stage = ImageclipStage::BoundaryType(h);
                }
                Ok(Step::Continue)
            }
            ImageclipStage::Option(h) => match &i {
                Input::Keyword(k) | Input::Text(k) => match k.trim().to_ascii_lowercase().as_str() {
                    "on" => {
                        run_imageclip(s, &json!({ "handle": h.hex(), "on": true }))?;
                        s.echo("Image clipping turned ON.");
                        Ok(Step::Done)
                    }
                    "off" | "of" => {
                        run_imageclip(s, &json!({ "handle": h.hex(), "off": true }))?;
                        s.echo("Image clipping turned OFF.");
                        Ok(Step::Done)
                    }
                    "delete" | "d" => {
                        run_imageclip(s, &json!({ "handle": h.hex(), "delete": true }))?;
                        s.echo("Image clipping boundary deleted.");
                        Ok(Step::Done)
                    }
                    "generate polyline" | "generate" | "polyline" | "p" => {
                        run_imageclip(s, &json!({ "handle": h.hex(), "generate_polyline": true }))?;
                        s.echo("Polyline generated from image clipping boundary.");
                        Ok(Step::Done)
                    }
                    "new boundary" | "new" | "n" => {
                        self.stage = ImageclipStage::BoundaryType(h);
                        Ok(Step::Continue)
                    }
                    _ => {
                        self.stage = ImageclipStage::Option(h);
                        Err(EngineError::Other("Invalid option keyword.".into()))
                    }
                },
                Input::Enter => {
                    self.stage = ImageclipStage::BoundaryType(h);
                    Ok(Step::Continue)
                }
                _ => {
                    self.stage = ImageclipStage::Option(h);
                    Ok(Step::Continue)
                }
            },
            ImageclipStage::BoundaryType(h) => match &i {
                Input::Point(p) => {
                    self.stage = ImageclipStage::SecondCorner(h, *p);
                    Ok(Step::Continue)
                }
                Input::Keyword(k) | Input::Text(k) => match k.trim().to_ascii_lowercase().as_str() {
                    "select polyline" | "select" | "s" => {
                        self.stage = ImageclipStage::SelectPolyline(h);
                        Ok(Step::Continue)
                    }
                    "rectangular" | "r" => {
                        self.stage = ImageclipStage::FirstCorner(h);
                        Ok(Step::Continue)
                    }
                    _ => {
                        self.stage = ImageclipStage::BoundaryType(h);
                        Err(EngineError::Other("Invalid boundary option.".into()))
                    }
                },
                Input::Enter => {
                    self.stage = ImageclipStage::FirstCorner(h);
                    Ok(Step::Continue)
                }
                _ => {
                    self.stage = ImageclipStage::BoundaryType(h);
                    Ok(Step::Continue)
                }
            },
            ImageclipStage::FirstCorner(h) => {
                if let Input::Point(p) = i {
                    self.stage = ImageclipStage::SecondCorner(h, p);
                    Ok(Step::Continue)
                } else {
                    Ok(Step::Done)
                }
            }
            ImageclipStage::SecondCorner(h, p0) => {
                if let Input::Point(p1) = i {
                    run_imageclip(s, &json!({ "handle": h.hex(), "rect": [[p0.x, p0.y], [p1.x, p1.y]] }))?;
                    s.echo("Image clipping boundary applied.");
                }
                Ok(Step::Done)
            }
            ImageclipStage::SelectPolyline(h) => {
                let oh = match &i {
                    Input::Point(p) => curves::pick_at(s, *p),
                    Input::Pick(hs) => hs.first().copied(),
                    _ => None,
                };
                if let Some(oh) = oh {
                    run_imageclip(s, &json!({ "handle": h.hex(), "object": oh.hex() }))?;
                    s.echo("Image clipping boundary applied from polyline.");
                }
                Ok(Step::Done)
            }
        }
    }
}

// ---------------- CLIP Prompt Machine ----------------

#[derive(Default)]
pub(crate) struct ClipM {
    sub: Option<Box<dyn Interactive>>,
}

impl Interactive for ClipM {
    fn name(&self) -> &'static str {
        "CLIP"
    }

    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        let sel = s.selection();
        let d = s.doc()?;
        if let Some(h) = sel.first().copied()
            && let Some(e) = d.entity(h)
        {
            match &e.kind {
                EntityKind::Insert(_) => {
                    let mut x = Box::new(XclipM::default());
                    let step = x.begin(s)?;
                    self.sub = Some(x);
                    return Ok(step);
                }
                EntityKind::Image(_) => {
                    let mut im = Box::new(ImageclipM::default());
                    let step = im.begin(s)?;
                    self.sub = Some(im);
                    return Ok(step);
                }
                EntityKind::Viewport(_) => {
                    let mut vp = Box::new(crate::cmd::layout::viewports::VpclipM::default());
                    let step = vp.begin(s)?;
                    self.sub = Some(vp);
                    return Ok(step);
                }
                _ => {}
            }
        }
        Ok(Step::Continue)
    }

    fn prompt(&self, s: &Session) -> Prompt {
        if let Some(sub) = &self.sub {
            sub.prompt(s)
        } else {
            Prompt::new("Select object to clip (block reference, xref, image, or viewport)", Accept::POINT)
        }
    }

    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if let Some(sub) = &mut self.sub {
            return sub.input(s, i);
        }
        let h = match &i {
            Input::Point(p) => curves::pick_at(s, *p),
            Input::Pick(hs) => hs.first().copied(),
            _ => None,
        };
        let Some(h) = h else {
            if matches!(i, Input::Enter) {
                return Ok(Step::Done);
            }
            s.echo("No clippable object found.");
            return Ok(Step::Continue);
        };
        let d = s.doc()?;
        let Some(e) = d.entity(h) else {
            s.echo("Object not found.");
            return Ok(Step::Continue);
        };
        match &e.kind {
            EntityKind::Insert(_) => {
                let mut x = Box::new(XclipM::default());
                x.stage = XclipStage::Select;
                let _ = x.input(s, Input::Pick(vec![h]))?;
                self.sub = Some(x);
                Ok(Step::Continue)
            }
            EntityKind::Image(_) => {
                let mut im = Box::new(ImageclipM::default());
                im.stage = ImageclipStage::Select;
                let _ = im.input(s, Input::Pick(vec![h]))?;
                self.sub = Some(im);
                Ok(Step::Continue)
            }
            EntityKind::Viewport(_) => {
                let mut vp = Box::new(crate::cmd::layout::viewports::VpclipM::default());
                let _ = vp.begin(s)?;
                let _ = vp.input(s, Input::Pick(vec![h]))?;
                self.sub = Some(vp);
                Ok(Step::Continue)
            }
            _ => {
                s.echo("Object cannot be clipped. Select an external reference, block, image or viewport.");
                Ok(Step::Continue)
            }
        }
    }
}
