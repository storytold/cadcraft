use super::common::style;
use super::{
    Accept, EngineError, EntityKind, Input, Interactive, LeaderOrder, MLeader, MLeaderOptions, MText, Prompt, Result, Session, Step, Value, Vec2,
    bad, json, points_param, str_param, v3,
};

/// A multileader from arrowhead to landing; `landing` false leaves out the landing line (dogleg).
fn mleader_kind(s: &Session, pts: &[Vec2], text: &str, landing: bool) -> Option<EntityKind> {
    let (landing_pt, arrow_pts) = pts.split_last()?;
    let d = s.doc().ok()?;
    let cur = d.header.str("CMLEADERSTYLE", "Standard");
    let st = d.mleader_styles.iter().find(|m| m.name.eq_ignore_ascii_case(&cur)).or(d.mleader_styles.first()).cloned().unwrap_or_default();
    // The style's sizes are the drawn sizes: a multileader style has no scale of its own and
    // DIMSCALE (a dimension-style setting) does not apply, as in AutoCAD.
    let th = st.text_height;
    let dir = if arrow_pts.first().is_some_and(|a| a.x > landing_pt.x) { -1.0 } else { 1.0 };
    let attach = if dir > 0.0 { 4 } else { 6 };
    let dogleg = if landing { st.dogleg } else { 0.0 };
    let tpos = *landing_pt + Vec2::new((dogleg + st.landing_gap) * dir, 0.0);
    Some(EntityKind::MLeader(MLeader {
        leaders: vec![arrow_pts.iter().map(|p| v3(*p)).collect()],
        landing: v3(*landing_pt),
        dogleg,
        text: (!text.is_empty()).then(|| MText {
            insert: v3(tpos),
            height: th,
            width: 0.0,
            attach,
            rotation: 0.0,
            style: st.text_style.clone(),
            contents: text.replace('\n', "\\P"),
            line_spacing: 1.0,
            line_spacing_exact: false,
            columns: None,
        }),
        style: st.name.clone(),
        arrow_size: st.arrow_size,
        spline: false,
        block: None,
    }))
}

pub(super) fn run_mleader(s: &mut Session, p: &Value) -> Result<Value> {
    let pts = points_param(p, "points").ok_or_else(|| bad("mleader", "`points` (arrow … landing) is required"))?;
    let content = crate::cmd::mleader_content::json_content(s, p)?;
    // Leader type None places the content at the one point given.
    if pts.len() < 2 && !(content.leader == crate::LeaderType::None && pts.len() == 1) {
        return Err(bad("mleader", "need an arrowhead and a landing point"));
    }
    let k = mleader_kind(s, &pts, str_param(p, "text").unwrap_or(""), true).ok_or_else(|| bad("mleader", "bad points"))?;
    let k = crate::cmd::mleader_content::apply(s, k, &content)?;
    Ok(json!({ "handle": s.add_entity(k)?.hex() }))
}

pub(super) fn run_leader(s: &mut Session, p: &Value) -> Result<Value> {
    let pts = points_param(p, "points").ok_or_else(|| bad("leader", "`points` is required"))?;
    if pts.len() < 2 {
        return Err(bad("leader", "need 2+ points"));
    }
    let st = style(s);
    let h = s.add_entity(EntityKind::Leader(cadcraft_doc::Leader {
        vertices: pts.iter().map(|q| v3(*q)).collect(),
        arrow: true,
        spline: false,
        style: st,
    }))?;
    let mut out = vec![h.hex()];
    if let (Some(t), Some(last)) = (str_param(p, "text"), pts.last()) {
        let th = s.doc()?.dim_style(&style(s)).map(|d| d.text_height).unwrap_or(0.18);
        out.push(
            s.add_entity(EntityKind::MText(MText {
                insert: v3(*last + Vec2::new(th * 0.5, th * 0.5)),
                height: th,
                width: 0.0,
                attach: 4,
                rotation: 0.0,
                style: "Standard".into(),
                contents: t.into(),
                line_spacing: 1.0,
                line_spacing_exact: false,
                columns: None,
            }))?
            .hex(),
        );
    }
    Ok(json!({ "handles": out }))
}

/// The MLEADER Options prompt being answered.
#[derive(Clone, Copy, PartialEq)]
enum LeaderOpt {
    Menu,
    LeaderType,
    Landing,
    ContentType,
    MaxPoints,
    FirstAngle,
    SecondAngle,
    BlockName,
    BlockAttach,
}

/// Upper bound for MLEADER Maxpoints.
const MAX_LEADER_POINTS: usize = 64;

pub(super) struct MLeaderM {
    order: LeaderOrder,
    /// Points in the order they were placed.
    pts: Vec<Vec2>,
    /// All leader points are placed.
    leader_done: bool,
    /// The leader text once entered (Content first enters it before the leader).
    text: Option<String>,
    opt: Option<LeaderOpt>,
    max_points: usize,
    /// First and second angle constraints (radians; 0 = free).
    angles: [f64; 2],
    landing: bool,
    content: bool,
    leader_type: crate::LeaderType,
    /// Block content: the block (`block_name`) instead of text.
    block: bool,
    block_name: String,
    block_center: bool,
}

impl MLeaderM {
    /// Starts with the order and Options chosen the last time (as AutoCAD does).
    pub(super) fn new(s: &Session) -> Self {
        let o = s.last_used.mleader;
        let max_points = o.max_points.clamp(2, MAX_LEADER_POINTS);
        MLeaderM {
            order: o.order,
            pts: Vec::new(),
            leader_done: false,
            text: None,
            opt: None,
            max_points,
            angles: o.angles,
            landing: o.landing,
            content: o.content,
            leader_type: o.leader_type,
            // Block content needs a block that still exists.
            block: o.block && s.doc().is_ok_and(|d| d.block(&s.last_used.mleader_block).is_some()),
            block_name: s.last_used.mleader_block.clone(),
            block_center: o.block_center,
        }
    }
    /// Keep the order and Options for the next MLEADER.
    fn remember(&self, s: &mut Session) {
        s.last_used.mleader = MLeaderOptions {
            order: self.order,
            max_points: self.max_points,
            angles: self.angles,
            landing: self.landing,
            content: self.content,
            leader_type: self.leader_type,
            block: self.block,
            block_center: self.block_center,
        };
        s.last_used.mleader_block = self.block_name.clone();
    }
    /// The leader type and block content for `mleader_content::apply`.
    fn content_spec(&self) -> crate::cmd::mleader_content::Content {
        crate::cmd::mleader_content::Content {
            leader: self.leader_type,
            block: (self.content && self.block).then(|| (self.block_name.clone(), self.block_center)),
        }
    }
    /// Leader type None: the one point placed is the content location.
    fn no_leader(&self) -> bool {
        self.leader_type == crate::LeaderType::None
    }
    /// The leader from arrowhead to landing, with `extra` placed next.
    fn leader(&self, extra: Option<Vec2>) -> Vec<Vec2> {
        let mut pts = self.pts.clone();
        pts.extend(extra);
        if self.order != LeaderOrder::Arrowhead {
            pts.reverse();
        }
        pts
    }
    /// `p` with the first/second angle constraint applied to the segment it ends (counted from the
    /// first point placed).
    fn constrained(&self, p: Vec2) -> Vec2 {
        let (Some(&prev), Some(&inc)) = (self.pts.last(), self.angles.get(self.pts.len().wrapping_sub(1))) else { return p };
        let len = prev.dist(p);
        if inc <= 1e-9 || !inc.is_finite() || len <= 1e-12 {
            return p;
        }
        Vec2::polar(prev, len, ((p - prev).angle() / inc).round() * inc)
    }
    /// The text is asked now: right after the content location (Content first) or after the leader.
    fn wants_text(&self) -> bool {
        self.content
            && !self.block
            && self.text.is_none()
            && (self.leader_done || (self.order == LeaderOrder::Content && self.pts.len() == 1 && !self.no_leader()))
    }
    fn finish(&self, s: &mut Session) -> Result<Step> {
        if self.no_leader() && !self.content {
            s.echo("A multileader needs a leader line or content; nothing was drawn.");
            return Ok(Step::Cancel);
        }
        let text = self.text.clone().unwrap_or_default();
        let k = mleader_kind(s, &self.leader(None), &text, self.landing).ok_or_else(|| EngineError::Other("bad leader".into()))?;
        let k = crate::cmd::mleader_content::apply(s, k, &self.content_spec())?;
        s.add_entity(k)?;
        Ok(Step::Done)
    }
    fn option_prompt(o: LeaderOpt) -> Prompt {
        let kw = crate::cmd::curves::KW;
        match o {
            LeaderOpt::Menu => Prompt::new("Enter an option", kw)
                .kw(&["Leader type", "leader lAnding", "Content type", "Maxpoints", "First angle", "Second angle", "eXit options"])
                .default("eXit options"),
            LeaderOpt::LeaderType => Prompt::new("Select a leader type", kw).kw(&["Straight", "sPline", "None"]).default("Straight"),
            LeaderOpt::Landing => Prompt::new("Use landing", kw).kw(&["Yes", "No"]).default("Yes"),
            LeaderOpt::ContentType => Prompt::new("Select a content type", kw).kw(&["Block", "Mtext", "None"]).default("Mtext"),
            LeaderOpt::MaxPoints => Prompt::new("Enter the maximum points for leader line", Accept::NUMBER),
            LeaderOpt::FirstAngle => Prompt::new("Enter first angle constraint", Accept::NUMBER),
            LeaderOpt::SecondAngle => Prompt::new("Enter second angle constraint", Accept::NUMBER),
            LeaderOpt::BlockName => Prompt::new("Enter block name", kw),
            LeaderOpt::BlockAttach => {
                Prompt::new("Specify block attachment", kw).kw(&["Center extents", "Insertion point"]).default("Center extents")
            }
        }
    }
    /// One answer at the Options prompts; Enter keeps the current value.
    fn option(&mut self, s: &mut Session, o: LeaderOpt, i: Input) -> Option<LeaderOpt> {
        use LeaderOpt::*;
        match (o, i) {
            (Menu, Input::Keyword(k)) => match k.as_str() {
                "Leader type" => Some(LeaderType),
                "leader lAnding" => Some(Landing),
                "Content type" => Some(ContentType),
                "Maxpoints" => Some(MaxPoints),
                "First angle" => Some(FirstAngle),
                "Second angle" => Some(SecondAngle),
                _ => None,
            },
            (Menu, Input::Enter) => None,
            (LeaderType, Input::Keyword(k)) => {
                self.leader_type = match k.as_str() {
                    "sPline" => crate::LeaderType::Spline,
                    "None" => crate::LeaderType::None,
                    _ => crate::LeaderType::Straight,
                };
                Some(Menu)
            }
            (Landing, Input::Keyword(k)) => {
                self.landing = k == "Yes";
                Some(Menu)
            }
            (ContentType, Input::Keyword(k)) => {
                self.content = k != "None";
                self.block = false;
                // Block content asks which block, then how it attaches.
                if k == "Block" { Some(BlockName) } else { Some(Menu) }
            }
            (BlockName, Input::Text(t)) => match crate::cmd::mleader_content::block_name(s, &t) {
                Ok(n) => {
                    self.block_name = n;
                    self.block = true;
                    Some(BlockAttach)
                }
                Err(e) => {
                    s.echo(e.to_string());
                    Some(BlockName)
                }
            },
            (BlockName, Input::Enter) => {
                // Enter keeps the block chosen before, if there is one.
                if crate::cmd::mleader_content::block_name(s, &self.block_name).is_ok() {
                    self.block = true;
                    Some(BlockAttach)
                } else {
                    s.echo("Requires the name of a block defined in the drawing.");
                    Some(BlockName)
                }
            }
            (BlockAttach, Input::Keyword(k)) => {
                self.block_center = k == "Center extents";
                Some(Menu)
            }
            (MaxPoints, Input::Text(t)) => match t.trim().parse::<usize>() {
                Ok(n) if (2..=MAX_LEADER_POINTS).contains(&n) => {
                    self.max_points = n;
                    Some(Menu)
                }
                _ => {
                    s.echo(format!("Requires an integer between 2 and {MAX_LEADER_POINTS}."));
                    Some(MaxPoints)
                }
            },
            (FirstAngle | SecondAngle, Input::Text(t)) => match s.angle_settings().amount(&t).filter(|a| *a >= 0.0) {
                Some(a) => {
                    self.angles[usize::from(o == SecondAngle)] = a;
                    Some(Menu)
                }
                None => {
                    s.echo("Requires a valid angle.");
                    Some(o)
                }
            },
            (Menu | LeaderType | Landing | ContentType | BlockAttach, Input::Text(_)) => {
                s.echo("Invalid option keyword.");
                Some(o)
            }
            (_, Input::Enter) => Some(Menu),
            _ => Some(o),
        }
    }
}

impl Interactive for MLeaderM {
    fn name(&self) -> &'static str {
        "MLEADER"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        if let Some(o) = self.opt {
            return Self::option_prompt(o);
        }
        if self.wants_text() {
            return Prompt::new("Enter leader text", Accept::TEXT);
        }
        if self.no_leader() && self.pts.is_empty() {
            let what = if self.content && self.block { "Specify location of the block" } else { "Specify location of the text" };
            return Prompt::new(what, Accept::POINT).kw(&["Options"]).default("Options");
        }
        let last = self.pts.len() + 1 >= self.max_points;
        match (self.pts.last(), self.order) {
            (None, LeaderOrder::Arrowhead) => Prompt::new("Specify leader arrowhead location", Accept::POINT)
                .kw(&["leader Landing first", "Content first", "Options"])
                .default("Options"),
            (None, LeaderOrder::Landing) => Prompt::new("Specify leader landing location", Accept::POINT)
                .kw(&["leader arrowHead first", "Content first", "Options"])
                .default("Options"),
            (None, LeaderOrder::Content) => Prompt::new("Specify location of the text", Accept::POINT)
                .kw(&["leader arrowHead first", "leader Landing first", "Options"])
                .default("Options"),
            (Some(&b), _) if !last => Prompt::new("Specify next point", Accept::POINT).base(b),
            (Some(&b), LeaderOrder::Arrowhead) => Prompt::new("Specify leader landing location", Accept::POINT).base(b),
            (Some(&b), _) => Prompt::new("Specify leader arrowhead location", Accept::POINT).base(b),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if let Some(o) = self.opt {
            self.opt = self.option(s, o, i);
            self.remember(s);
            return Ok(Step::Continue);
        }
        if self.wants_text() {
            self.text = Some(if let Input::Text(t) = i { t } else { String::new() });
            return if self.leader_done { self.finish(s) } else { Ok(Step::Continue) };
        }
        match i {
            Input::Point(p) => {
                let p = self.constrained(p);
                self.pts.push(p);
                if self.pts.len() >= self.max_points || self.no_leader() {
                    self.leader_done = true;
                }
            }
            Input::Keyword(k) if self.pts.is_empty() => {
                match k.as_str() {
                    "leader Landing first" => self.order = LeaderOrder::Landing,
                    "leader arrowHead first" => self.order = LeaderOrder::Arrowhead,
                    "Content first" => self.order = LeaderOrder::Content,
                    _ => self.opt = Some(LeaderOpt::Menu),
                }
                self.remember(s);
            }
            Input::Enter if self.pts.is_empty() => self.opt = Some(LeaderOpt::Menu),
            // Enter at "next point" ends the leader at the last point.
            Input::Enter if self.pts.len() >= 2 => self.leader_done = true,
            Input::Enter => return Ok(Step::Cancel),
            _ => {}
        }
        if self.leader_done && !self.wants_text() { self.finish(s) } else { Ok(Step::Continue) }
    }
    fn preview(&self, s: &Session, c: Vec2) -> Vec<EntityKind> {
        if self.opt.is_some() || self.wants_text() || self.leader_done || self.pts.is_empty() {
            return Vec::new();
        }
        let text = self.text.as_deref().unwrap_or("");
        mleader_kind(s, &self.leader(Some(self.constrained(c))), text, self.landing)
            .and_then(|k| crate::cmd::mleader_content::apply(s, k, &self.content_spec()).ok())
            .into_iter()
            .collect()
    }
}
