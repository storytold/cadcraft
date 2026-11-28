//! Draw order: DRAWORDER (Above objects / Under objects / Front / Back), TEXTTOFRONT and
//! HATCHTOBACK. The order is the order of a space's entity store, which DXF files keep as the
//! order of their entity records (`cadcraft-io` also applies a SORTENTSTABLE when it reads one).

use std::collections::{HashMap, HashSet};

use cadcraft_doc::{EntityKind, Handle};
use serde_json::{Value, json};

use super::machines::{SelOutcome, SelectPhase, SelectRun};
use super::{CommandSpec, EngineError, bad, ok, targets};

use crate::{Accept, Input, Interactive, Prompt, Result, Session, Step};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("draworder", "Draw Order", run_draworder)
            .alias(&["dr"])
            .params("{handles?, order?: \"front\"|\"back\"|\"above\"|\"under\" (default back), reference?: [hex] (above/under)} → {moved}")
            .interactive(|_| Ok(Box::new(DrawOrderM::new(None)))),
        CommandSpec::new("draworder.front", "Bring to Front", run_front)
            .menu(&["Tools", "Draw Order", "Bring to Front"])
            .params("{handles?}")
            .interactive(|_| Ok(Box::new(SelectRun::new("draworder.front", "DRAWORDER")))),
        CommandSpec::new("draworder.back", "Send to Back", run_back)
            .menu(&["Tools", "Draw Order", "Send to Back"])
            .params("{handles?}")
            .interactive(|_| Ok(Box::new(SelectRun::new("draworder.back", "DRAWORDER")))),
        CommandSpec::new("draworder.above", "Bring Above Objects", run_above)
            .menu(&["Tools", "Draw Order", "Bring Above Objects"])
            .params("{handles?, reference: [hex]}")
            .interactive(|_| Ok(Box::new(DrawOrderM::new(Some(Order::Above))))),
        CommandSpec::new("draworder.under", "Send Under Objects", run_under)
            .menu(&["Tools", "Draw Order", "Send Under Objects"])
            .params("{handles?, reference: [hex]}")
            .interactive(|_| Ok(Box::new(DrawOrderM::new(Some(Order::Under))))),
        CommandSpec::new("texttofront", "Bring Annotations to Front", run_texttofront)
            .params("{objects?: \"text\"|\"dimensions\"|\"leaders\"|\"all\" (default all)} → {moved}")
            .interactive(|_| Ok(Box::new(TextToFrontM))),
        CommandSpec::new("texttofront.text", "Text Objects Only", |s, _| annotations_to_front(s, Annot::Text))
            .menu(&["Tools", "Draw Order", "Bring Annotations to Front", "Text Objects Only"])
            .params("{} → {moved}"),
        CommandSpec::new("texttofront.dimensions", "Dimension Objects Only", |s, _| annotations_to_front(s, Annot::Dimensions))
            .menu(&["Tools", "Draw Order", "Bring Annotations to Front", "Dimension Objects Only"])
            .params("{} → {moved}"),
        CommandSpec::new("texttofront.leaders", "Leaders Only", |s, _| annotations_to_front(s, Annot::Leaders))
            .menu(&["Tools", "Draw Order", "Bring Annotations to Front", "Leaders Only"])
            .params("{} → {moved}"),
        CommandSpec::new("texttofront.all", "All Annotation Objects", |s, _| annotations_to_front(s, Annot::All))
            .menu(&["Tools", "Draw Order", "Bring Annotations to Front", "All Annotation Objects"])
            .params("{} → {moved}"),
        CommandSpec::new("hatchtoback", "Send Hatches to Back", run_hatchtoback)
            .menu(&["Tools", "Draw Order", "Send Hatches to Back"])
            .params("{} → {moved}"),
    ]
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Order {
    Front,
    Back,
    Above,
    Under,
}

/// Move `hs` in the current space's draw order, keeping their order among themselves. Above and
/// Under place them right over the topmost reference object or right under the lowest one.
/// Returns how many objects moved.
fn reorder(s: &mut Session, hs: &[Handle], order: Order, refs: &[Handle]) -> Result<usize> {
    let space = s.space();
    let d = s.doc_mut()?;
    let Some(st) = d.space_mut(&space) else { return Ok(0) };
    let index: HashMap<Handle, usize> = st.handles().into_iter().enumerate().map(|(i, h)| (h, i)).collect();
    let mut moving: Vec<(usize, Handle)> = hs.iter().filter_map(|h| index.get(h).map(|i| (*i, *h))).collect();
    moving.sort_unstable();
    moving.dedup();
    let set: HashSet<Handle> = moving.iter().map(|m| m.1).collect();
    let refs: Vec<(usize, Handle)> = refs.iter().filter(|h| !set.contains(h)).filter_map(|h| index.get(h).map(|i| (*i, *h))).collect();
    match order {
        Order::Front => {
            for (_, h) in &moving {
                st.bring_to_front(*h);
            }
        }
        Order::Back => {
            for (_, h) in moving.iter().rev() {
                st.send_to_back(*h);
            }
        }
        Order::Above => {
            let Some(&(_, mut cur)) = refs.iter().max() else { return Err(EngineError::Other("no reference objects".into())) };
            for (_, h) in &moving {
                st.move_next_to(*h, cur, true);
                cur = *h;
            }
        }
        Order::Under => {
            let Some(&(_, anchor)) = refs.iter().min() else { return Err(EngineError::Other("no reference objects".into())) };
            for (_, h) in &moving {
                st.move_next_to(*h, anchor, false);
            }
        }
    }
    Ok(moving.len())
}

fn parse_order(p: &Value) -> Result<Order> {
    match p.get("order") {
        None | Some(Value::Null) => Ok(Order::Back),
        Some(v) => match v.as_str().map(|t| t.trim().to_ascii_lowercase()).as_deref() {
            Some("front") => Ok(Order::Front),
            Some("back") => Ok(Order::Back),
            Some("above") => Ok(Order::Above),
            Some("under") => Ok(Order::Under),
            _ => Err(bad("draworder", format!("`order` must be \"front\", \"back\", \"above\" or \"under\", got {v}"))),
        },
    }
}

fn reference(s: &Session, cmd: &str, p: &Value) -> Result<Vec<Handle>> {
    match p.get("reference") {
        None | Some(Value::Null) => Err(bad(cmd, "`reference` (object handles) is required")),
        Some(v) => targets(s, &json!({ "handles": v })),
    }
}

fn run_draworder(s: &mut Session, p: &Value) -> Result<Value> {
    let order = parse_order(p)?;
    let hs = targets(s, p)?;
    let refs = if matches!(order, Order::Above | Order::Under) { reference(s, "draworder", p)? } else { Vec::new() };
    let n = reorder(s, &hs, order, &refs)?;
    Ok(json!({ "moved": n }))
}

fn run_front(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    reorder(s, &hs, Order::Front, &[])?;
    ok()
}

fn run_back(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    reorder(s, &hs, Order::Back, &[])?;
    ok()
}

fn run_above(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    let refs = reference(s, "draworder.above", p)?;
    let n = reorder(s, &hs, Order::Above, &refs)?;
    Ok(json!({ "moved": n }))
}

fn run_under(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    let refs = reference(s, "draworder.under", p)?;
    let n = reorder(s, &hs, Order::Under, &refs)?;
    Ok(json!({ "moved": n }))
}

/// TEXTTOFRONT's object classes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Annot {
    Text,
    Dimensions,
    Leaders,
    All,
}

impl Annot {
    fn matches(self, k: &EntityKind) -> bool {
        let text = matches!(k, EntityKind::Text(_) | EntityKind::MText(_));
        let dim = matches!(k, EntityKind::Dimension(_));
        let leader = matches!(k, EntityKind::Leader(_) | EntityKind::MLeader(_));
        match self {
            Annot::Text => text,
            Annot::Dimensions => dim,
            Annot::Leaders => leader,
            Annot::All => text || dim || leader,
        }
    }
}

/// Every object of the current space that `keep` accepts, in draw order.
fn of_kind(s: &Session, keep: impl Fn(&EntityKind) -> bool) -> Result<Vec<Handle>> {
    let d = s.doc()?;
    Ok(d.space(&s.space()).map(|st| st.iter().filter(|e| keep(&e.kind)).map(|e| e.handle).collect()).unwrap_or_default())
}

fn annotations_to_front(s: &mut Session, which: Annot) -> Result<Value> {
    let hs = of_kind(s, |k| which.matches(k))?;
    let n = reorder(s, &hs, Order::Front, &[])?;
    Ok(json!({ "moved": n, "message": format!("{n} object(s) brought to front.") }))
}

fn run_texttofront(s: &mut Session, p: &Value) -> Result<Value> {
    let which = match p.get("objects") {
        None | Some(Value::Null) => Annot::All,
        Some(v) => match v.as_str().map(|t| t.trim().to_ascii_lowercase()).as_deref() {
            Some("text") => Annot::Text,
            Some("dimensions") => Annot::Dimensions,
            Some("leaders") => Annot::Leaders,
            Some("all") => Annot::All,
            _ => return Err(bad("texttofront", format!("`objects` must be \"text\", \"dimensions\", \"leaders\" or \"all\", got {v}"))),
        },
    };
    annotations_to_front(s, which)
}

fn run_hatchtoback(s: &mut Session, _p: &Value) -> Result<Value> {
    let hs = of_kind(s, |k| matches!(k, EntityKind::Hatch(_)))?;
    let n = reorder(s, &hs, Order::Back, &[])?;
    Ok(json!({ "moved": n, "message": format!("{n} hatch object(s) sent to back.") }))
}

/// TEXTTOFRONT typed: "Bring to front [Text/Dimensions/Leaders/All] <All>:".
struct TextToFrontM;

impl Interactive for TextToFrontM {
    fn name(&self) -> &'static str {
        "TEXTTOFRONT"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        Prompt::new("Bring to front", Accept::TEXT).kw(&["Text", "Dimensions", "Leaders", "All"]).default("All")
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        let which = match &i {
            Input::Enter => Annot::All,
            Input::Keyword(k) | Input::Text(k) => match k.trim().to_ascii_lowercase().as_str() {
                "text" | "t" => Annot::Text,
                "dimensions" | "d" => Annot::Dimensions,
                "leaders" | "l" => Annot::Leaders,
                "all" | "a" => Annot::All,
                _ => {
                    s.echo("Invalid option keyword.");
                    return Ok(Step::Continue);
                }
            },
            _ => return Ok(Step::Continue),
        };
        let r = annotations_to_front(s, which)?;
        if let Some(m) = r.get("message").and_then(Value::as_str) {
            s.echo(m.to_string());
        }
        Ok(Step::Done)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    Select,
    Option,
    Reference,
}

/// DRAWORDER typed: "Select objects:", then "Enter object ordering option [Above objects/Under
/// objects/Front/Back] <Back>:", and for Above/Under "Select reference objects:". The Bring Above
/// / Send Under menu items start with the option chosen.
struct DrawOrderM {
    stage: Stage,
    preset: Option<Order>,
    order: Order,
    sel: SelectPhase,
    objs: Vec<Handle>,
}

impl DrawOrderM {
    fn new(preset: Option<Order>) -> Self {
        DrawOrderM { stage: Stage::Select, preset, order: preset.unwrap_or(Order::Back), sel: SelectPhase::default(), objs: Vec::new() }
    }
    /// The objects are chosen: ask for the option, or go on with the preset one.
    fn selected(&mut self, s: &mut Session, hs: Vec<Handle>) -> Result<Step> {
        self.objs = hs;
        match self.preset {
            Some(o) => self.chosen(s, o),
            None => {
                self.stage = Stage::Option;
                Ok(Step::Continue)
            }
        }
    }
    fn chosen(&mut self, s: &mut Session, o: Order) -> Result<Step> {
        self.order = o;
        match o {
            Order::Front | Order::Back => self.finish(s, &[]),
            Order::Above | Order::Under => {
                self.stage = Stage::Reference;
                self.sel = SelectPhase::default();
                Ok(Step::Continue)
            }
        }
    }
    fn finish(&mut self, s: &mut Session, refs: &[Handle]) -> Result<Step> {
        let objs = std::mem::take(&mut self.objs);
        match reorder(s, &objs, self.order, refs) {
            Ok(_) => {}
            Err(e) => s.echo(e.to_string()),
        }
        s.set_selection(Vec::new());
        Ok(Step::Done)
    }
}

impl Interactive for DrawOrderM {
    fn name(&self) -> &'static str {
        "DRAWORDER"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        self.sel = SelectPhase::begin(s);
        if self.sel.done {
            let hs = self.sel.picked.clone();
            return self.selected(s, hs);
        }
        Ok(Step::Continue)
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match self.stage {
            Stage::Select => self.sel.prompt(),
            Stage::Option => {
                Prompt::new("Enter object ordering option", Accept::TEXT).kw(&["Above objects", "Under objects", "Front", "Back"]).default("Back")
            }
            Stage::Reference if self.sel.removing => self.sel.prompt(),
            Stage::Reference => Prompt::new("Select reference objects", Accept::SELECT),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match self.stage {
            Stage::Select => match self.sel.feed(s, &i)? {
                SelOutcome::More => Ok(Step::Continue),
                SelOutcome::Empty => Ok(Step::Done),
                SelOutcome::Done(hs) => {
                    s.set_selection(Vec::new());
                    self.selected(s, hs)
                }
            },
            Stage::Option => {
                let o = match &i {
                    Input::Enter => Order::Back,
                    Input::Keyword(k) | Input::Text(k) => match k.trim().to_ascii_lowercase().as_str() {
                        "above objects" | "a" | "above" => Order::Above,
                        "under objects" | "u" | "under" => Order::Under,
                        "front" | "f" => Order::Front,
                        "back" | "b" => Order::Back,
                        _ => {
                            s.echo("Invalid option keyword.");
                            return Ok(Step::Continue);
                        }
                    },
                    _ => return Ok(Step::Continue),
                };
                self.chosen(s, o)
            }
            Stage::Reference => match self.sel.feed(s, &i)? {
                SelOutcome::More => Ok(Step::Continue),
                SelOutcome::Empty => {
                    s.set_selection(Vec::new());
                    Ok(Step::Done)
                }
                SelOutcome::Done(refs) => self.finish(s, &refs),
            },
        }
    }
}
