//! Grips of dynamic block references: `blocks.reference.grips` lists them, `blocks.reference.grip`
//! drags one (one undo step), and [`Session::start_dyn_grip`] starts the drag the UI's click on a
//! grip begins, with the reference re-evaluated under the cursor as its preview. Typing a number
//! at a distance's grip sets the distance.

use cadcraft_doc::color::Color;
use cadcraft_doc::dynblock::{self, GripPart, GripShape};
use cadcraft_doc::{Common, Drawing, Entity, EntityKind, Handle, Lineweight, Transparency};
use cadcraft_geom::Vec2;
use serde_json::{Value, json};

use crate::cmd::{bad, point_param, str_param};
use crate::{Accept, EngineError, Input, Interactive, Prompt, Result, Running, Session, Step};

/// Most objects one preview draws.
const MAX_PREVIEW: usize = 2000;

/// The grips of a dynamic block reference (an empty list for any other object).
pub(super) fn run_grips(s: &mut Session, p: &Value) -> Result<Value> {
    let h = super::super::curves::handle_param(p, "handle").ok_or_else(|| bad("blocks.reference.grips", "`handle` is required"))?;
    serde_json::to_value(dynblock::grips(s.doc()?, h)).map_err(|e| bad("blocks.reference.grips", e.to_string()))
}

/// Drag a grip of a dynamic block reference to a point (one undo step).
pub(super) fn run_grip(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "blocks.reference.grip";
    let h = super::super::curves::handle_param(p, "handle").ok_or_else(|| bad(cmd, "`handle` is required"))?;
    let node = p
        .get("node")
        .and_then(Value::as_i64)
        .and_then(|n| i32::try_from(n).ok())
        .ok_or_else(|| bad(cmd, "`node` (a parameter's graph id) is required"))?;
    let part = str_param(p, "part").and_then(GripPart::from_name).ok_or_else(|| bad(cmd, "`part` must be base, end or at"))?;
    let to = point_param(p, "to").ok_or_else(|| bad(cmd, "`to` [x, y] is required"))?;
    drag(s, h, node, part, to).map_err(|e| bad(cmd, e))?;
    let properties = dynblock::custom_properties(s.doc()?, h).map(|(_, p)| p).unwrap_or_default();
    Ok(json!({ "properties": properties }))
}

/// Drag grip (`node`, `part`) of reference `h` to `to` in the session's drawing.
fn drag(s: &mut Session, h: Handle, node: i32, part: GripPart, to: Vec2) -> std::result::Result<(), String> {
    if !matches!(s.doc().map_err(|e| e.to_string())?.entity(h).map(|e| &e.kind), Some(EntityKind::Insert(_))) {
        return Err("`handle` is not a block reference".into());
    }
    if super::super::curves::is_locked(s, h) {
        return Err("the reference is on a locked layer".into());
    }
    let mut d = s.doc().map_err(|e| e.to_string())?.clone();
    dynblock::drag_grip(&mut d, h, node, part, to)?;
    *s.doc_mut().map_err(|e| e.to_string())? = d;
    Ok(())
}

/// The properties an object of a block draws with in reference `r`: on layer 0 it is on the
/// reference's layer, and its ByBlock colour, linetype, lineweight and transparency are the
/// reference's (as the renderer draws a block's contents).
fn under(c: &Common, r: &Common) -> Common {
    let mut c = c.clone();
    if c.layer == "0" {
        c.layer.clone_from(&r.layer);
    }
    if c.color == Color::ByBlock {
        c.color = r.color;
    }
    if c.linetype.eq_ignore_ascii_case("ByBlock") {
        c.linetype.clone_from(&r.linetype);
    }
    if c.lineweight == Lineweight::ByBlock {
        c.lineweight = r.lineweight;
    }
    if c.transparency == Transparency::ByBlock {
        c.transparency = r.transparency;
    }
    c
}

/// The reference `h` as drawn in drawing `d`: its block's shown objects placed by it, in the
/// block's order (fills under the outline and text), and its attributes, each with the
/// properties it draws with.
fn drawn(d: &Drawing, h: Handle) -> Vec<Entity> {
    let Some(e) = d.entity(h) else { return Vec::new() };
    let EntityKind::Insert(ins) = &e.kind else { return Vec::new() };
    let Some(b) = d.block(&ins.block) else { return Vec::new() };
    let r = &e.common;
    let m = ins.transform(b.base.xy());
    let mut out: Vec<Entity> = b
        .entities
        .iter()
        .filter(|e| e.common.visible && !matches!(&e.kind, EntityKind::AttDef(_) | EntityKind::Wipeout(_)))
        .take(MAX_PREVIEW)
        .map(|e| {
            let mut kind = e.kind.clone();
            kind.transform(&m);
            Entity { handle: Handle(0), common: under(&e.common, r), kind }
        })
        .collect();
    // An attribute draws with its definition's properties, except those set on it.
    let by_block = Common { layer: "0".into(), color: Color::ByBlock, linetype: "ByBlock".into(), lineweight: Lineweight::ByBlock, ..r.clone() };
    out.extend(ins.attribs.iter().filter(|a| !a.invisible).map(|a| {
        let def = b.entities.iter().find_map(|e| match &e.kind {
            EntityKind::AttDef(ad) if ad.tag.eq_ignore_ascii_case(&a.tag) => Some(&e.common),
            _ => None,
        });
        let common = under(&a.props.over(def.unwrap_or(&by_block)), r);
        Entity { handle: Handle(0), common, kind: EntityKind::Text(a.text.clone()) }
    }));
    out
}

/// A dynamic grip being dragged.
struct DynGripEdit {
    handle: Handle,
    node: i32,
    part: GripPart,
    /// Where the grip is (the rubber band's start).
    from: Vec2,
    /// The property it changes, and whether it is a distance (a typed number sets it).
    property: String,
    distance: bool,
}

impl Interactive for DynGripEdit {
    fn name(&self) -> &'static str {
        "GRIP"
    }

    fn begin(&mut self, _s: &mut Session) -> Result<Step> {
        Ok(Step::Continue)
    }

    fn prompt(&self, _s: &Session) -> Prompt {
        let (msg, accept) = if self.distance {
            ("** STRETCH ** Specify stretch point or distance", Accept::POINT_OR_NUMBER)
        } else {
            ("** STRETCH ** Specify stretch point", Accept::POINT)
        };
        Prompt::new(msg, accept).kw(&["eXit"]).base(self.from)
    }

    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        let to = match i {
            Input::Point(p) => p,
            Input::Deferred(d) => d.at,
            Input::Text(t) if self.distance => {
                let mut d = s.doc()?.clone();
                dynblock::set_property(&mut d, self.handle, &self.property, t.trim()).map_err(EngineError::Other)?;
                *s.doc_mut()? = d;
                return Ok(Step::Done);
            }
            Input::Keyword(k) if k == "eXit" => return Ok(Step::Done),
            Input::Enter | Input::Cancel => return Ok(Step::Done),
            _ => return Err(EngineError::Other("Requires a point.".into())),
        };
        drag(s, self.handle, self.node, self.part, to).map_err(EngineError::Other)?;
        Ok(Step::Done)
    }

    /// The rubber band from the grip.
    fn preview(&self, _s: &Session, c: Vec2) -> Vec<EntityKind> {
        vec![crate::cmd::helpers::line(self.from, c)]
    }

    /// The reference re-evaluated at the cursor, drawn as it will look (its own colours, so
    /// text stays readable on a filled block).
    fn preview_objects(&self, s: &Session, c: Vec2) -> Vec<Entity> {
        let Ok(d) = s.doc() else { return Vec::new() };
        let mut d = d.clone();
        match dynblock::drag_grip(&mut d, self.handle, self.node, self.part, c) {
            Ok(()) => drawn(&d, self.handle),
            Err(_) => Vec::new(),
        }
    }
}

impl Session {
    /// Start dragging grip `part` of parameter `node` of dynamic block reference `handle` (the
    /// UI's click on a dynamic grip). A list grip (visibility, lookup) has no drag: the UI offers
    /// its choices and sets them with `blocks.reference.set`.
    pub fn start_dyn_grip(&mut self, handle: Handle, node: i32, part: GripPart) -> Result<()> {
        if self.running.is_some() {
            self.cancel();
        }
        let g = dynblock::grips(self.doc()?, handle)
            .into_iter()
            .find(|g| g.node == node && g.part == part)
            .ok_or_else(|| EngineError::Other("no such grip".into()))?;
        if g.shape == GripShape::List {
            return Err(EngineError::Other("a list grip is chosen from, not dragged".into()));
        }
        let edit = DynGripEdit { handle, node, part, from: g.at, property: g.property, distance: g.shape == GripShape::Linear };
        let machine = crate::guard::Guarded::wrap(Box::new(edit));
        let st = self.state()?;
        let (before, selection_before) = (st.doc.clone(), st.selection.clone());
        self.running = Some(Running { id: "grip".into(), machine, before, selection_before });
        self.pending_window = None;
        self.point_mods.clear();
        self.echo("** STRETCH **");
        Ok(())
    }
}
