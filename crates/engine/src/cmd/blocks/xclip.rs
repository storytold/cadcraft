//! XCLIP and XCLIPFRAME: clip block and xref references to a boundary.
//!
//! A boundary belongs to one reference ([`cadcraft_doc::Insert::clip`]), in the block's own
//! coordinates, so it moves, rotates and scales with the reference and other references of the
//! block stay whole. It is a rectangle, a polygon or the outline of a closed polyline, can hide
//! what is inside instead of what is outside (Invert clip), and can be turned off, deleted or
//! turned into a polyline. Clip depth (front and back clipping planes) has no meaning in a 2D
//! drawing and is ignored. XCLIPFRAME says whether the boundary is drawn and plotted.

use cadcraft_doc::{ClipBoundary, EntityKind, Handle, MAX_CLIP_VERTICES};
use cadcraft_geom::{PolyVertex, Vec2};
use serde_json::{Value, json};

use super::super::machines::{SelOutcome, SelectPhase};
use crate::cmd::{CommandSpec, bad, bool_or, has_doc, point_value, str_param, targets};
use crate::{Accept, Input, Interactive, Prompt, Result, Session, Step};

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("xclip", "Clip Xref", run)
            .menu(&["Modify", "Clip", "Xref"])
            .alias(&["xc"])
            .params("{handles? | handle? (block or xref references, default the selection), option?: new|on|off|delete|polyline|clipdepth|query (default new with a boundary, else query), rectangle?: [[x,y],[x,y]] | points?: [[x,y],…] (3 to 4096, drawing coordinates) | polyline?: handle of a closed polyline, invert?: bool (hide what is inside)} → {handles, option} (query: {clips: [{handle, boundary, enabled, inverted}]}; polyline: {polylines})")
            .enabled(has_doc)
            .interactive(|_| Ok(Box::new(XclipM::default()))),
        CommandSpec::new("xclipframe", "Xref Clip Frame", run_frame)
            .params("{value?: 0 (frames hidden) | 1 (shown and plotted) | 2 (shown, not plotted; the default)} → {XCLIPFRAME}")
            .enabled(has_doc)
            .interactive(|_| Ok(Box::new(FrameM))),
    ]
}

/// The block references among `hs` (other objects are ignored, as XCLIP's selection does).
fn references(s: &Session, hs: &[Handle]) -> Vec<Handle> {
    let Ok(d) = s.doc() else { return Vec::new() };
    hs.iter().copied().filter(|h| d.entity(*h).is_some_and(|e| matches!(e.kind, EntityKind::Insert(_)))).collect()
}

/// A rectangle's two corners as a closed outline.
fn rectangle(a: Vec2, b: Vec2) -> Vec<Vec2> {
    vec![
        Vec2::new(a.x.min(b.x), a.y.min(b.y)),
        Vec2::new(a.x.max(b.x), a.y.min(b.y)),
        Vec2::new(a.x.max(b.x), a.y.max(b.y)),
        Vec2::new(a.x.min(b.x), a.y.max(b.y)),
    ]
}

/// The boundary a JSON call gives, in drawing coordinates, and whether it is a rectangle.
fn world_boundary(s: &Session, p: &Value) -> Result<(Vec<Vec2>, bool)> {
    if let Some(r) = p.get("rectangle") {
        let pts: Vec<Vec2> = r.as_array().map(|a| a.iter().filter_map(point_value).collect()).unwrap_or_default();
        let [a, b] = pts.as_slice() else { return Err(bad("xclip", "`rectangle` needs two finite corners [[x,y],[x,y]]")) };
        if (a.x - b.x).abs() <= 1e-12 || (a.y - b.y).abs() <= 1e-12 {
            return Err(bad("xclip", "the clipping rectangle must have a width and a height"));
        }
        return Ok((vec![*a, *b], true));
    }
    if let Some(v) = p.get("polyline") {
        let h = v
            .as_str()
            .and_then(Handle::parse_hex)
            .or_else(|| v.as_u64().map(Handle))
            .ok_or_else(|| bad("xclip", "`polyline` must be an object handle"))?;
        let d = s.doc()?;
        let e = d.entity(h).ok_or_else(|| bad("xclip", format!("no object {}", h.hex())))?;
        let closed = matches!(&e.kind, EntityKind::LwPolyline(pl) if pl.closed) || matches!(&e.kind, EntityKind::Polyline3d(pl) if pl.closed);
        if !closed {
            return Err(bad("xclip", "select a closed polyline"));
        }
        let pts = cadcraft_render::clip::boundary_polygon(&e.kind).ok_or_else(|| bad("xclip", "the polyline does not enclose an area"))?;
        return Ok((pts, false));
    }
    let pts = p.get("points").and_then(Value::as_array).ok_or_else(|| bad("xclip", "a boundary is required: `rectangle`, `points` or `polyline`"))?;
    if !(3..=MAX_CLIP_VERTICES).contains(&pts.len()) {
        return Err(bad("xclip", format!("a polygonal boundary needs 3 to {MAX_CLIP_VERTICES} points")));
    }
    let pts: Vec<Vec2> =
        pts.iter().map(|v| point_value(v).ok_or_else(|| bad("xclip", "every boundary point must be a finite [x, y]"))).collect::<Result<_>>()?;
    Ok((pts, false))
}

fn run(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = references(s, &targets(s, p)?);
    if hs.is_empty() {
        return Err(bad("xclip", "select block or xref references"));
    }
    let has_boundary = ["rectangle", "points", "polyline"].iter().any(|k| p.get(*k).is_some());
    let option = str_param(p, "option").map(str::to_ascii_lowercase).unwrap_or_else(|| if has_boundary { "new".into() } else { "query".into() });
    match option.as_str() {
        "new" | "n" => {
            let (world, rect) = world_boundary(s, p)?;
            let outline = if rect { world.first().zip(world.get(1)).map(|(a, b)| rectangle(*a, *b)).unwrap_or_default() } else { world.clone() };
            if cadcraft_render::clip::PolyClip::new(&outline).is_none() {
                return Err(bad("xclip", "the clipping boundary encloses no area"));
            }
            let invert = bool_or(p, "invert", false);
            let mut clips = Vec::with_capacity(hs.len());
            {
                let d = s.doc()?;
                for h in &hs {
                    let Some(EntityKind::Insert(ins)) = d.entity(*h).map(|e| &e.kind) else { continue };
                    let base = d.block(&ins.block).map(|b| b.base.xy()).ok_or_else(|| bad("xclip", format!("block \"{}\" is missing", ins.block)))?;
                    let inv = ins.transform(base).inverse().ok_or_else(|| bad("xclip", "a reference with a zero scale can't be clipped"))?;
                    let points: Vec<Vec2> = world.iter().map(|q| inv.apply(*q)).collect();
                    // A rotated or skewed reference sees the rectangle as a polygon.
                    let r = ins.rotation.rem_euclid(std::f64::consts::FRAC_PI_2);
                    let square = r < 1e-9 || std::f64::consts::FRAC_PI_2 - r < 1e-9;
                    let points = if rect && !square { outline.iter().map(|q| inv.apply(*q)).collect() } else { points };
                    clips.push((*h, ClipBoundary { points, enabled: true, inverted: invert }));
                }
            }
            for (h, c) in clips {
                set_clip(s, h, Some(c))?;
            }
        }
        "on" | "off" => {
            let on = option == "on";
            for h in &hs {
                let mut c = clip_of(s, *h).ok_or_else(|| bad("xclip", "no clipping boundary on the selected reference"))?;
                c.enabled = on;
                set_clip(s, *h, Some(c))?;
            }
        }
        "delete" | "d" => {
            for h in &hs {
                set_clip(s, *h, None)?;
            }
        }
        "polyline" | "p" | "generate" => {
            let mut made = Vec::new();
            for h in &hs {
                let Some(world) = world_outline(s, *h) else { continue };
                let vertices = world.into_iter().map(|p| PolyVertex { p, bulge: 0.0, start_width: 0.0, end_width: 0.0 }).collect();
                let pl = cadcraft_doc::LwPolyline { vertices, closed: true, const_width: 0.0, elevation: 0.0, plinegen: false };
                made.push(s.add_entity(EntityKind::LwPolyline(pl))?.hex());
            }
            if made.is_empty() {
                return Err(bad("xclip", "no clipping boundary on the selected references"));
            }
            return Ok(json!({ "handles": hexes(&hs), "option": "polyline", "polylines": made }));
        }
        // Front and back clipping planes have no meaning in 2D: nothing changes.
        "clipdepth" | "c" => return Ok(json!({ "handles": hexes(&hs), "option": "clipdepth", "ignored": true })),
        "query" | "?" => {
            let clips: Vec<Value> = hs
                .iter()
                .map(|h| {
                    let c = clip_of(s, *h);
                    json!({
                        "handle": h.hex(),
                        "boundary": world_outline(s, *h).map(|w| w.iter().map(|p| [p.x, p.y]).collect::<Vec<_>>()),
                        "enabled": c.as_ref().is_some_and(|c| c.enabled),
                        "inverted": c.as_ref().is_some_and(|c| c.inverted),
                    })
                })
                .collect();
            return Ok(json!({ "clips": clips }));
        }
        o => return Err(bad("xclip", format!("unknown option \"{o}\" (new, on, off, delete, polyline, clipdepth or query)"))),
    }
    Ok(json!({ "handles": hexes(&hs), "option": option }))
}

fn hexes(hs: &[Handle]) -> Vec<String> {
    hs.iter().map(|h| h.hex()).collect()
}

fn clip_of(s: &Session, h: Handle) -> Option<ClipBoundary> {
    match &s.doc().ok()?.entity(h)?.kind {
        EntityKind::Insert(i) => i.clip.clone(),
        _ => None,
    }
}

/// Reference `h`'s boundary outline in drawing coordinates.
fn world_outline(s: &Session, h: Handle) -> Option<Vec<Vec2>> {
    let d = s.doc().ok()?;
    let EntityKind::Insert(i) = &d.entity(h)?.kind else { return None };
    let c = i.clip.as_ref()?;
    let m = i.transform(d.block(&i.block)?.base.xy());
    let poly = c.polygon();
    (poly.len() >= 3).then(|| poly.into_iter().map(|p| m.apply(p)).collect())
}

fn set_clip(s: &mut Session, h: Handle, c: Option<ClipBoundary>) -> Result<()> {
    s.doc_mut()?
        .modify_entity(h, |e| {
            if let EntityKind::Insert(i) = &mut e.kind {
                i.clip = c;
            }
        })
        .map_err(|e| bad("xclip", e.to_string()))
}

// ---------------- XCLIPFRAME ----------------

fn run_frame(s: &mut Session, p: &Value) -> Result<Value> {
    let v = match p.get("value").or_else(|| p.get("XCLIPFRAME")) {
        None | Some(Value::Null) => return Ok(json!({ "XCLIPFRAME": s.doc()?.header.i64("XCLIPFRAME", 2) })),
        Some(v) => v.as_i64().or_else(|| v.as_str().and_then(|t| t.trim().parse().ok())),
    };
    let v = v.filter(|v| (0..=2).contains(v)).ok_or_else(|| bad("xclipframe", "`value` must be 0, 1 or 2"))?;
    s.doc_mut()?.header.set_i64("XCLIPFRAME", v);
    Ok(json!({ "XCLIPFRAME": v }))
}

struct FrameM;

impl Interactive for FrameM {
    fn name(&self) -> &'static str {
        "XCLIPFRAME"
    }
    fn prompt(&self, s: &Session) -> Prompt {
        let cur = s.doc().map(|d| d.header.i64("XCLIPFRAME", 2)).unwrap_or(2);
        Prompt::new("Enter new value for XCLIPFRAME", Accept::TEXT).kw(&["0", "1", "2"]).default(cur.to_string())
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        let t = match i {
            Input::Cancel => return Ok(Step::Cancel),
            Input::Enter => return Ok(Step::Done),
            Input::Text(t) | Input::Keyword(t) => t,
            _ => return Ok(Step::Continue),
        };
        match run_frame(s, &json!({ "value": t.trim() })) {
            Ok(_) => Ok(Step::Done),
            Err(_) => {
                s.echo("Requires an integer between 0 and 2.");
                Ok(Step::Continue)
            }
        }
    }
}

// ---------------- interactive XCLIP ----------------

#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum Stage {
    #[default]
    Select,
    Option,
    DeleteOld,
    Boundary,
    Corner1,
    Corner2,
    Polygon,
    Polyline,
}

#[derive(Default)]
struct XclipM {
    stage: Stage,
    sel: SelectPhase,
    refs: Vec<Handle>,
    invert: bool,
    pts: Vec<Vec2>,
}

impl XclipM {
    fn selected(&mut self, s: &mut Session, hs: &[Handle]) -> Result<Step> {
        self.refs = references(s, hs);
        if self.refs.is_empty() {
            s.echo("No block or xref references selected.");
            return Ok(Step::Done);
        }
        self.stage = Stage::Option;
        Ok(Step::Continue)
    }

    fn apply(&mut self, s: &mut Session, mut p: Value) -> Result<Step> {
        p["handles"] = json!(hexes(&self.refs));
        let r = run(s, &p);
        s.set_selection(Vec::new());
        match r {
            Ok(_) => Ok(Step::Done),
            Err(e) => {
                s.echo(e.to_string());
                Ok(Step::Done)
            }
        }
    }

    fn any_clipped(&self, s: &Session) -> bool {
        self.refs.iter().any(|h| clip_of(s, *h).is_some())
    }
}

fn kw(i: &Input) -> Option<String> {
    match i {
        Input::Keyword(k) | Input::Text(k) => Some(k.trim().to_ascii_lowercase()),
        _ => None,
    }
}

impl Interactive for XclipM {
    fn name(&self) -> &'static str {
        "XCLIP"
    }

    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        self.sel = SelectPhase::begin(s);
        if self.sel.done {
            let hs = self.sel.picked.clone();
            return self.selected(s, &hs);
        }
        Ok(Step::Continue)
    }

    fn prompt(&self, _s: &Session) -> Prompt {
        match self.stage {
            Stage::Select => self.sel.prompt(),
            Stage::Option => Prompt::new("Enter clipping option", Accept::TEXT)
                .kw(&["ON", "OFF", "Clipdepth", "Delete", "generate Polyline", "New boundary"])
                .default("New"),
            Stage::DeleteOld => Prompt::new("Delete old boundary(s)?", Accept::TEXT).kw(&["Yes", "No"]).default("Yes"),
            Stage::Boundary => Prompt::new("Specify clipping boundary or select invert option", Accept::POINT)
                .kw(&["Select polyline", "Polygonal", "Rectangular", "Invert clip"])
                .default("Rectangular"),
            Stage::Corner1 => Prompt::new("Specify first corner", Accept::POINT),
            Stage::Corner2 => {
                let p = Prompt::new("Specify opposite corner", Accept::POINT);
                match self.pts.first() {
                    Some(a) => p.corner(*a),
                    None => p,
                }
            }
            Stage::Polygon => {
                let msg = if self.pts.is_empty() { "Specify first point" } else { "Specify next point" };
                let p = Prompt::new(msg, Accept::POINT).base_opt(self.pts.last().copied());
                if self.pts.is_empty() { p } else { p.kw(&["Undo"]) }
            }
            Stage::Polyline => Prompt::new("Select polyline", Accept::SELECT),
        }
    }

    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if matches!(i, Input::Cancel) {
            return Ok(Step::Cancel);
        }
        match self.stage {
            Stage::Select => match self.sel.feed(s, &i)? {
                SelOutcome::More => Ok(Step::Continue),
                SelOutcome::Empty => Ok(Step::Done),
                SelOutcome::Done(hs) => self.selected(s, &hs),
            },
            Stage::Option => {
                let k = if matches!(i, Input::Enter) { Some("new".to_string()) } else { kw(&i) };
                match k.as_deref() {
                    Some("on") => self.apply(s, json!({ "option": "on" })),
                    Some("off") => self.apply(s, json!({ "option": "off" })),
                    Some(k) if k.starts_with('c') => {
                        s.echo("Clip depth does not apply to a 2D drawing: front and back clipping planes are ignored.");
                        Ok(Step::Done)
                    }
                    Some(k) if k.starts_with('d') => self.apply(s, json!({ "option": "delete" })),
                    Some(k) if k.starts_with('p') || k.starts_with('g') => self.apply(s, json!({ "option": "polyline" })),
                    Some(k) if k.starts_with('n') => {
                        self.stage = if self.any_clipped(s) { Stage::DeleteOld } else { Stage::Boundary };
                        Ok(Step::Continue)
                    }
                    _ => {
                        s.echo("Invalid option keyword.");
                        Ok(Step::Continue)
                    }
                }
            }
            Stage::DeleteOld => match kw(&i).as_deref() {
                None if matches!(i, Input::Enter) => {
                    self.stage = Stage::Boundary;
                    Ok(Step::Continue)
                }
                Some(k) if k.starts_with('y') => {
                    self.stage = Stage::Boundary;
                    Ok(Step::Continue)
                }
                Some(k) if k.starts_with('n') => Ok(Step::Done),
                _ => Ok(Step::Continue),
            },
            Stage::Boundary => match (&i, kw(&i).as_deref()) {
                (Input::Enter, _) => {
                    self.stage = Stage::Corner1;
                    Ok(Step::Continue)
                }
                (Input::Point(p), _) => {
                    // A point starts the default rectangle.
                    self.pts = vec![*p];
                    self.stage = Stage::Corner2;
                    Ok(Step::Continue)
                }
                (_, Some(k)) if k.starts_with('i') => {
                    self.invert = !self.invert;
                    s.echo(if self.invert {
                        "Inside mode - Objects inside boundary will be hidden."
                    } else {
                        "Outside mode - Objects outside boundary will be hidden."
                    });
                    Ok(Step::Continue)
                }
                (_, Some(k)) if k.starts_with('s') => {
                    self.stage = Stage::Polyline;
                    Ok(Step::Continue)
                }
                (_, Some(k)) if k.starts_with('p') => {
                    self.pts.clear();
                    self.stage = Stage::Polygon;
                    Ok(Step::Continue)
                }
                (_, Some(k)) if k.starts_with('r') => {
                    self.stage = Stage::Corner1;
                    Ok(Step::Continue)
                }
                _ => Ok(Step::Continue),
            },
            Stage::Corner1 => match i {
                Input::Point(p) => {
                    self.pts = vec![p];
                    self.stage = Stage::Corner2;
                    Ok(Step::Continue)
                }
                _ => Ok(Step::Continue),
            },
            Stage::Corner2 => match (i, self.pts.first().copied()) {
                (Input::Point(b), Some(a)) => self.apply(s, json!({ "option": "new", "rectangle": [[a.x, a.y], [b.x, b.y]], "invert": self.invert })),
                _ => Ok(Step::Continue),
            },
            Stage::Polygon => match i {
                Input::Point(p) if self.pts.len() < MAX_CLIP_VERTICES => {
                    self.pts.push(p);
                    Ok(Step::Continue)
                }
                Input::Point(_) => {
                    s.echo(format!("A boundary has at most {MAX_CLIP_VERTICES} points."));
                    Ok(Step::Continue)
                }
                Input::Keyword(_) | Input::Text(_) => {
                    self.pts.pop();
                    Ok(Step::Continue)
                }
                Input::Enter if self.pts.len() >= 3 => {
                    let pts: Vec<[f64; 2]> = self.pts.iter().map(|p| [p.x, p.y]).collect();
                    self.apply(s, json!({ "option": "new", "points": pts, "invert": self.invert }))
                }
                Input::Enter => {
                    s.echo("A polygonal boundary needs at least three points.");
                    Ok(Step::Continue)
                }
                _ => Ok(Step::Continue),
            },
            Stage::Polyline => match i {
                Input::Pick(hs) => match hs.first() {
                    Some(h) => {
                        let h = *h;
                        self.apply(s, json!({ "option": "new", "polyline": h.hex(), "invert": self.invert }))
                    }
                    None => Ok(Step::Continue),
                },
                Input::Enter => Ok(Step::Done),
                _ => Ok(Step::Continue),
            },
        }
    }
}
