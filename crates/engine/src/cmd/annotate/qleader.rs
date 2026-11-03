//! QLEADER at the command line, as AutoCAD 2024 runs it with its default settings (accoreconsole,
//! synthetic drawings): up to three points, a text width, then lines of text.
//!
//! The leader is LEADER's, with an arrow and a hook line DIMASZ long when its last segment is more
//! than 15° off horizontal and text follows. The multiline text sits DIMGAP past the leader's end,
//! as wide as the width given: with the middle of its first line level with the end when the
//! leader runs right, and of its last line when it runs left, whatever DIMTAD. Enter at the first
//! line of text leaves the leader without text. The Settings dialog isn't available yet.

use serde_json::{Value, json};

use super::leader::{Annotation, add_leader};
use super::{Accept, Input, Interactive, Prompt, Result, Session, Step, Vec2, bad, points_param, str_param};
use crate::cmd::size_param;

/// QLEADER's default number of leader points.
const MAX_POINTS: usize = 3;

#[derive(Clone, Copy, Default, PartialEq)]
enum Ask {
    #[default]
    Points,
    Width,
    FirstLine,
    NextLine,
}

#[derive(Default)]
pub(super) struct QLeaderM {
    ask: Ask,
    pts: Vec<Vec2>,
    width: f64,
    lines: Vec<String>,
}

impl Interactive for QLeaderM {
    fn name(&self) -> &'static str {
        "QLEADER"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        let last = self.pts.last().copied();
        match self.ask {
            Ask::Points if self.pts.is_empty() => Prompt::new("Specify first leader point,", Accept::POINT).kw(&["Settings"]).default("Settings"),
            Ask::Points => Prompt::new("Specify next point", Accept::POINT).base_opt(last),
            // The width isn't remembered: AutoCAD 2024 offers 0 again after a width of 20.
            Ask::Width => Prompt::new("Specify text width", Accept::POINT_OR_NUMBER).distance().default("0").base_opt(last),
            Ask::FirstLine => Prompt::new("Enter first line of annotation text", Accept::TEXT).default("Mtext"),
            Ask::NextLine => Prompt::new("Enter next line of annotation text", Accept::TEXT),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match self.ask {
            Ask::Points => match i {
                Input::Point(p) => {
                    self.pts.push(p);
                    if self.pts.len() == MAX_POINTS {
                        self.ask = Ask::Width;
                    }
                }
                Input::Enter | Input::Keyword(_) if self.pts.is_empty() => s.echo("The QLEADER Settings dialog is not available yet."),
                // Enter after one point: no leader.
                Input::Enter if self.pts.len() < 2 => return Ok(Step::Done),
                Input::Enter => self.ask = Ask::Width,
                _ => {}
            },
            Ask::Width => {
                // A point gives the width as its distance from the leader's end; a negative
                // width is none.
                self.width = match i {
                    Input::Point(p) => self.pts.last().map_or(0.0, |e| e.dist(p)),
                    Input::Text(t) => crate::units::parse_distance(&t)
                        .filter(|w| w.is_finite())
                        .ok_or_else(|| crate::EngineError::Other("Requires numeric distance or two points.".into()))?
                        .max(0.0),
                    Input::Enter => 0.0,
                    _ => return Ok(Step::Continue),
                };
                self.ask = Ask::FirstLine;
            }
            // Only typed text and Enter: a click in the drawing is no line of text, as at TEXT's.
            Ask::FirstLine | Ask::NextLine => match i {
                Input::Text(t) if !t.is_empty() => {
                    self.lines.push(t);
                    self.ask = Ask::NextLine;
                }
                Input::Text(_) | Input::Enter => {
                    let lines = std::mem::take(&mut self.lines);
                    let ann = if lines.is_empty() { Annotation::None } else { Annotation::Quick { lines, width: self.width } };
                    add_leader(s, &std::mem::take(&mut self.pts), false, true, ann)?;
                    return Ok(Step::Done);
                }
                _ => {}
            },
        }
        Ok(Step::Continue)
    }
}

/// QLEADER's JSON form: a leader through 2 or 3 `points`, with `text` (one line per line of it) as wide
/// as `width`, placed as the command line places them. The handles of the leader and its text.
pub(super) fn run_qleader(s: &mut Session, p: &Value) -> Result<Value> {
    let pts = points_param(p, "points").ok_or_else(|| bad("qleader", "`points` is required"))?;
    if !(2..=MAX_POINTS).contains(&pts.len()) {
        return Err(bad("qleader", "`points` takes 2 or 3 points"));
    }
    let width = size_param("qleader", p, "width", true)?.unwrap_or(0.0);
    if p.get("text").is_some_and(|t| !t.is_null() && !t.is_string()) {
        return Err(bad("qleader", "`text` must be a string"));
    }
    // Empty lines at the end are dropped, as the command line ends at one: a left-running leader
    // would otherwise sit level with a blank last line. Text with nothing left is no text.
    let mut lines: Vec<String> = str_param(p, "text").map(|t| t.lines().map(str::to_string).collect()).unwrap_or_default();
    while lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    let ann = if lines.is_empty() { Annotation::None } else { Annotation::Quick { lines, width } };
    let hs = add_leader(s, &pts, false, true, ann)?;
    Ok(json!({ "handles": hs.iter().map(|h| h.hex()).collect::<Vec<_>>() }))
}
