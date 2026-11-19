//! OMNI-NOTE: a marker and a comment on the drawing.

use cadcraft_color::Color;
use cadcraft_doc::{Common, EntityKind};
use cadcraft_geom::Vec2;
use serde_json::{Value, json};

use crate::cmd::draw::make_text;
use crate::cmd::helpers::circle;
use crate::cmd::{bad, point_req, size_param, str_param};
use crate::{Accept, Input, Interactive, Prompt, Result, Session, Step};

use super::{MAX_TEXT, NOTE_HEIGHT, NOTE_LAYER, NOTE_RADIUS};
/// OMNI-NOTE: a marker and a comment on the drawing's own note layer.
pub(super) fn run_note(s: &mut Session, p: &Value) -> Result<Value> {
    let at = point_req("omni-note", p, "at")?;
    let text = str_param(p, "text").map(str::to_string).filter(|t| !t.trim().is_empty());
    let value: String = match text {
        Some(t) => t.chars().take(MAX_TEXT).collect(),
        None => return Err(bad("omni-note", "`text` is required")),
    };
    let height = size_param("omni-note", p, "height", false)?.unwrap_or(NOTE_HEIGHT);
    let radius = size_param("omni-note", p, "radius", false)?.unwrap_or(NOTE_RADIUS);
    let layer = str_param(p, "layer").map(str::trim).filter(|l| !l.is_empty()).unwrap_or(NOTE_LAYER).to_string();
    if layer.len() > 255 || layer.chars().any(|c| "<>/\\\":;?*|,=`".contains(c)) {
        return Err(bad("omni-note", format!("\"{layer}\" is not a valid layer name")));
    }
    // Both objects are built before the drawing is borrowed mutably (the text style comes from it).
    let marker = circle(at, radius);
    let label = make_text(s, at, &value, height, 0.0, (cadcraft_doc::HAlign::Left, cadcraft_doc::VAlign::Baseline));
    let space = s.space();
    let common = Common { layer: layer.clone(), ..Common::default() };
    let d = s.doc_mut()?;
    if d.ensure_layer(&layer) {
        // A new OMNI_NOTES layer is red, as OMNI-NOTE's original made it.
        if let Some(l) = d.layer_mut(&layer) {
            l.color = Color::Index(1);
        }
    }
    let handle = d.add(&space, common.clone(), marker)?;
    let text_handle = d.add(&space, common, label)?;
    s.echo(format!("[OMNI] Note added on layer \"{layer}\"."));
    Ok(json!({ "handle": handle.hex(), "textHandle": text_handle.hex(), "layer": layer, "at": [at.x, at.y] }))
}

/// OMNI-NOTE typed at the command line: the point, then the comment.
#[derive(Default)]
pub(super) struct NoteM {
    at: Option<Vec2>,
}

impl Interactive for NoteM {
    fn name(&self) -> &'static str {
        "OMNI-NOTE"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match self.at {
            None => Prompt::new("Specify the point to comment on", Accept::POINT),
            Some(_) => Prompt::new("Enter the comment", Accept::TEXT),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match (self.at, i) {
            (None, Input::Point(p)) => self.at = Some(p),
            (Some(at), Input::Text(t)) if !t.trim().is_empty() => {
                run_note(s, &json!({ "at": [at.x, at.y], "text": t }))?;
                return Ok(Step::Done);
            }
            (_, Input::Cancel) => return Ok(Step::Cancel),
            (Some(_), Input::Enter) => return Ok(Step::Cancel),
            _ => {}
        }
        Ok(Step::Continue)
    }
    fn preview(&self, _s: &Session, cursor: Vec2) -> Vec<EntityKind> {
        match self.at {
            None => vec![circle(cursor, NOTE_RADIUS)],
            Some(_) => Vec::new(),
        }
    }
}
