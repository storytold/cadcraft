//! Picking, moving and erasing a dynamic block's parameters and actions in the Block Editor, as
//! AutoCAD's editor treats them: they are picked like objects (the UI picks them on a click),
//! MOVE and ERASE (and Delete) act on the picked ones with the selected objects, and a picked
//! one's points have grips (a linear parameter's ends and label line, a point parameter's
//! location and label, a visibility or lookup parameter's location, an action's location).
//! Erasing a parameter erases its actions. Points are in the editor's coordinates (the block's
//! base point at the origin).

use cadcraft_doc::EntityKind;
use cadcraft_doc::dynblock::{self, ElementPoint};
use cadcraft_geom::Vec2;
use serde_json::{Value, json};

use crate::cmd::{CommandSpec, bad, point_param, str_param};
use crate::{Accept, EngineError, Input, Interactive, Prompt, Result, Running, Session, Step};

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("blocks.element.select", "Pick Parameters and Actions", run_select)
            .params("{nodes?: [graph ids] (none: pick nothing), add?: bool (add to those picked)} → {picked}")
            .enabled(editing),
        CommandSpec::new("blocks.element.move", "Move Parameters and Actions", run_move)
            .params("{nodes?: [graph ids] (default the picked ones), delta: [dx, dy]} → {moved}")
            .enabled(editing),
        CommandSpec::new("blocks.element.point", "Move a Parameter's Point", run_point)
            .params("{node: graph id, point: \"base\"|\"end\"|\"at\"|\"label\", to: [x, y]} (editor coordinates)")
            .enabled(editing),
        CommandSpec::new("blocks.element.points", "Parameter Points", run_points)
            .params("{node: graph id} → {points: [{point, at}]} (editor coordinates; read-only)")
            .enabled(editing),
        CommandSpec::new("blocks.element.erase", "Erase Parameters and Actions", run_erase)
            .params("{nodes?: [graph ids] (default the picked ones)} → {erased: [graph ids]} (a parameter's actions go with it)")
            .enabled(editing),
    ]
}

/// The Block Editor is open on a block.
fn editing(s: &Session) -> std::result::Result<(), String> {
    match s.state() {
        Ok(st) => match &st.block_edit {
            Some(be) if be.current => Err("This editor is open on the drawing itself, which has no parameters or actions.".into()),
            Some(_) => Ok(()),
            None => Err("Open a block in the Block Editor first (BEDIT).".into()),
        },
        Err(e) => Err(e.to_string()),
    }
}

/// The edited block and its base point (the editor's origin).
fn edited(s: &Session) -> Result<(String, Vec2)> {
    editing(s).map_err(EngineError::Other)?;
    let be = s.state()?.block_edit.as_ref().ok_or_else(|| EngineError::Other("The Block Editor is not open.".into()))?;
    Ok((be.name.clone(), be.base()))
}

fn nodes_param(cmd: &str, p: &Value, s: &Session) -> Result<Vec<i32>> {
    match p.get("nodes") {
        None | Some(Value::Null) => Ok(s.picked_elements()),
        Some(Value::Array(a)) if a.len() <= dynblock::MAX_ACTION_ITEMS => {
            a.iter().map(|v| v.as_i64().and_then(|n| i32::try_from(n).ok()).ok_or_else(|| bad(cmd, format!("{v} is not a graph id")))).collect()
        }
        Some(_) => Err(bad(cmd, "`nodes` is a list of graph ids")),
    }
}

fn node_param(cmd: &str, p: &Value) -> Result<i32> {
    p.get("node").and_then(Value::as_i64).and_then(|n| i32::try_from(n).ok()).ok_or_else(|| bad(cmd, "`node`: a graph id"))
}

fn point_name(p: ElementPoint) -> &'static str {
    match p {
        ElementPoint::Base => "base",
        ElementPoint::End => "end",
        ElementPoint::At => "at",
        ElementPoint::Label => "label",
    }
}

fn run_select(s: &mut Session, p: &Value) -> Result<Value> {
    let (block, _) = edited(s)?;
    let nodes = match p.get("nodes") {
        None | Some(Value::Null) => Vec::new(),
        _ => nodes_param("blocks.element.select", p, s)?,
    };
    let m = dynblock::dynamic_block(s.doc()?, &block);
    let known = |n: &i32| m.as_ref().is_some_and(|m| m.parameters.iter().any(|p| p.node == *n) || m.actions.iter().any(|a| a.node == *n));
    if let Some(n) = nodes.iter().find(|n| !known(n)) {
        return Err(bad("blocks.element.select", format!("no parameter or action {n} in the block")));
    }
    let add = p.get("add").and_then(Value::as_bool).unwrap_or(false);
    s.pick_elements(&nodes, add)?;
    Ok(json!({ "picked": s.picked_elements() }))
}

fn run_move(s: &mut Session, p: &Value) -> Result<Value> {
    let (block, _) = edited(s)?;
    let nodes = nodes_param("blocks.element.move", p, s)?;
    let d = point_param(p, "delta").ok_or_else(|| bad("blocks.element.move", "`delta`: [dx, dy]"))?;
    let mut doc = s.doc()?.clone();
    for n in &nodes {
        dynblock::move_element(&mut doc, &block, *n, d.to3(0.0)).map_err(|e| bad("blocks.element.move", e))?;
    }
    *s.doc_mut()? = doc;
    Ok(json!({ "moved": nodes.len() }))
}

fn run_point(s: &mut Session, p: &Value) -> Result<Value> {
    let (block, base) = edited(s)?;
    let node = node_param("blocks.element.point", p)?;
    let which = match str_param(p, "point").map(str::to_ascii_lowercase).as_deref() {
        Some("base") | Some("start") => ElementPoint::Base,
        Some("end") => ElementPoint::End,
        Some("at") => ElementPoint::At,
        Some("label") => ElementPoint::Label,
        _ => return Err(bad("blocks.element.point", "`point` is \"base\", \"end\", \"at\" or \"label\"")),
    };
    let to = point_param(p, "to").ok_or_else(|| bad("blocks.element.point", "`to`: [x, y]"))?;
    let mut doc = s.doc()?.clone();
    dynblock::set_element_point(&mut doc, &block, node, which, (to + base).to3(0.0)).map_err(|e| bad("blocks.element.point", e))?;
    *s.doc_mut()? = doc;
    Ok(json!({ "node": node }))
}

fn run_points(s: &mut Session, p: &Value) -> Result<Value> {
    let (block, base) = edited(s)?;
    let node = node_param("blocks.element.points", p)?;
    let pts: Vec<Value> = dynblock::element_points(s.doc()?, &block, node)
        .into_iter()
        .map(|(w, at)| json!({ "point": point_name(w), "at": [at.x - base.x, at.y - base.y] }))
        .collect();
    Ok(json!({ "points": pts }))
}

fn run_erase(s: &mut Session, p: &Value) -> Result<Value> {
    let (block, _) = edited(s)?;
    let nodes = nodes_param("blocks.element.erase", p, s)?;
    if nodes.is_empty() {
        return Ok(json!({ "erased": [] }));
    }
    let mut doc = s.doc()?.clone();
    let erased = dynblock::erase_elements(&mut doc, &block, &nodes).map_err(|e| bad("blocks.element.erase", e))?;
    *s.doc_mut()? = doc;
    let left: Vec<i32> = s.picked_elements().into_iter().filter(|n| !erased.contains(n)).collect();
    s.pick_elements(&left, false)?;
    Ok(json!({ "erased": erased }))
}

/// A picked parameter's or action's grip being dragged (click it, then click where it goes).
struct ElementGrip {
    node: i32,
    which: ElementPoint,
    /// Where the grip is, in the editor (the rubber band's start).
    from: Vec2,
}

impl Interactive for ElementGrip {
    fn name(&self) -> &'static str {
        "GRIP"
    }

    fn prompt(&self, _s: &Session) -> Prompt {
        Prompt::new("** STRETCH ** Specify stretch point", Accept::POINT).kw(&["eXit"]).base(self.from)
    }

    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        let to = match i {
            Input::Point(p) => p,
            Input::Deferred(d) => d.at,
            Input::Keyword(k) if k == "eXit" => return Ok(Step::Done),
            Input::Enter | Input::Cancel => return Ok(Step::Done),
            _ => return Err(EngineError::Other("Requires a point.".into())),
        };
        let (block, base) = edited(s)?;
        let mut doc = s.doc()?.clone();
        dynblock::set_element_point(&mut doc, &block, self.node, self.which, (to + base).to3(0.0)).map_err(EngineError::Other)?;
        *s.doc_mut()? = doc;
        Ok(Step::Done)
    }

    fn preview(&self, _s: &Session, c: Vec2) -> Vec<EntityKind> {
        vec![crate::cmd::helpers::line(self.from, c)]
    }
}

impl Session {
    /// The grips of the parameters and actions picked in the Block Editor: (graph id, point,
    /// where it is in the editor).
    pub fn element_grips(&self) -> Vec<(i32, ElementPoint, Vec2)> {
        let Ok((block, base)) = edited(self) else { return Vec::new() };
        let Ok(d) = self.doc() else { return Vec::new() };
        self.picked_elements()
            .into_iter()
            .flat_map(|n| dynblock::element_points(d, &block, n).into_iter().map(move |(w, at)| (n, w, at.xy() - base)))
            .collect()
    }

    /// Start dragging point `which` of picked parameter or action `node` (the UI's click on its
    /// grip in the Block Editor): the next point puts it there.
    pub fn start_element_grip(&mut self, node: i32, which: ElementPoint) -> Result<()> {
        if self.running.is_some() {
            self.cancel();
        }
        let from = self
            .element_grips()
            .into_iter()
            .find(|(n, w, _)| *n == node && *w == which)
            .map(|(_, _, at)| at)
            .ok_or_else(|| EngineError::Other("no such grip".into()))?;
        let machine = crate::guard::Guarded::wrap(Box::new(ElementGrip { node, which, from }));
        let st = self.state()?;
        let (before, selection_before) = (st.doc.clone(), st.selection.clone());
        self.running = Some(Running { id: "grip".into(), machine, before, selection_before });
        self.pending_window = None;
        self.point_mods.clear();
        self.echo("** STRETCH **");
        Ok(())
    }
}
