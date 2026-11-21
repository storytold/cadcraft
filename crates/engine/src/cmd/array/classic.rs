//! -ARRAY: the classic command-line array, as AutoCAD 2024 runs it from AutoLISP's `command`
//! (accoreconsole, every copy read back with `entget`). It makes separate copies, never an
//! associative array:
//! - "Enter the type of array [Rectangular/Polar] <R>": the default is the type used last;
//! - Rectangular asks "Enter the number of rows (---) <1>" and "Enter the number of columns (|||)
//!   <1>", then "Enter the distance between rows or specify unit cell (---)" only for more than one
//!   row (two corners give both distances) and "Specify the distance between columns (|||)" only for
//!   more than one column. The copy in row r and column c moves by (c·columns, r·rows): 2 rows and
//!   2 columns at −12.57 and −18.01 put the copies below and to the left;
//! - Polar asks "Specify center point of array or [Base]", "Enter the number of items in the
//!   array", "Specify the angle to fill (+=ccw, -=cw) <360>" and "Rotate arrayed objects?
//!   [Yes/No] <Y>". Items are 360/n apart around a full circle, else fill/(n − 1) apart (6 items in
//!   270°: 54°; −90° turns clockwise). Not rotated, each copy moves as the DXF group 10 point of the
//!   object selected last turns about the centre (a polyline's first vertex, an arc's centre), or
//!   as the Base point does.

use cadcraft_doc::Handle;
use cadcraft_geom::{Mat3, Vec2};
use serde_json::{Value, json};

use super::super::machines::{SelOutcome, SelectPhase, number};
use super::super::{bad, bool_or, f64_or, point_param, point_req, str_param, targets};
use super::{KEYWORDS, angle, check_size, copy_all, count, count_or, max_count, other, yes_no};
use crate::{Accept, Input, Interactive, Prompt, Result, Session, Step};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum Stage {
    #[default]
    Select,
    Kind,
    Rows,
    Columns,
    RowDistance,
    /// The unit cell's other corner, after the first one.
    Corner(Vec2),
    ColumnDistance,
    Center,
    Base,
    Items,
    Fill,
    Rotate,
}

#[derive(Default)]
pub(super) struct ClassicM {
    sel: SelectPhase,
    objs: Vec<Handle>,
    stage: Stage,
    rows: u64,
    columns: u64,
    row_distance: f64,
    center: Vec2,
    base: Option<Vec2>,
    items: u64,
    fill: f64,
}

/// The moves of a rectangular array's copies (the objects themselves stay in row 0, column 0).
fn rect_mats(rows: u64, columns: u64, row_distance: f64, column_distance: f64) -> Vec<Mat3> {
    (0..rows)
        .flat_map(|r| (0..columns).map(move |c| (r, c)))
        .filter(|rc| *rc != (0, 0))
        .map(|(r, c)| Mat3::translate(Vec2::new(c as f64 * column_distance, r as f64 * row_distance)))
        .collect()
}

/// The transforms of a polar array's copies: `items` around `center` filling `fill` degrees,
/// turned with it or moved as `base` turns.
fn polar_mats(center: Vec2, items: u64, fill: f64, rotate: bool, base: Vec2) -> Vec<Mat3> {
    let full = (fill.abs() - 360.0).abs() < 1e-9;
    let gaps = if full { items } else { items.saturating_sub(1) }.max(1);
    let step = fill.to_radians() / gaps as f64;
    (1..items)
        .map(|k| {
            let a = step * k as f64;
            if rotate { Mat3::rotate_about(center, a) } else { Mat3::translate(base.rotate_about(center, a) - base) }
        })
        .collect()
}

/// The DXF group 10 point of the last of `hs`, which a polar array that doesn't rotate its copies
/// moves around the centre.
fn last_point(s: &Session, hs: &[Handle]) -> Option<Vec2> {
    let d = s.doc().ok()?;
    hs.last().and_then(|h| d.entity(*h)).and_then(|e| crate::session::group_10(d, e))
}

/// -ARRAY's JSON form.
pub(super) fn run_classic(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    let polar = match str_param(p, "type").map(str::to_ascii_lowercase).as_deref() {
        None | Some("rectangular") => false,
        Some("polar") => true,
        Some(_) => return Err(bad("-array", "type must be \"rectangular\" or \"polar\"")),
    };
    // The size is checked before the copies' transforms are made.
    let mats = if polar {
        let center = point_req("-array", p, "center")?;
        let items = count_or("-array", p, "count", 1)?;
        check_size(s, items, hs.len())?;
        let base = point_param(p, "base").or_else(|| last_point(s, &hs)).unwrap_or(center);
        polar_mats(center, items, f64_or(p, "angle", 360.0), bool_or(p, "rotate", true), base)
    } else {
        let rows = count_or("-array", p, "rows", 1)?;
        let columns = count_or("-array", p, "cols", 1)?;
        check_size(s, rows.saturating_mul(columns), hs.len())?;
        rect_mats(rows, columns, f64_or(p, "rowSpacing", 0.0), f64_or(p, "colSpacing", 0.0))
    };
    let created = copy_all(s, &hs, mats.into_iter())?;
    Ok(json!({ "created": created }))
}

impl ClassicM {
    /// Make the copies.
    fn finish(&mut self, s: &mut Session, mats: Vec<Mat3>) -> Result<Step> {
        check_size(s, mats.len() as u64 + 1, self.objs.len())?;
        copy_all(s, &self.objs, mats.into_iter())?;
        s.set_selection(Vec::new());
        Ok(Step::Done)
    }

    /// After the rows and columns: the distances still needed, or the copies.
    fn after_counts(&mut self, s: &mut Session) -> Result<Step> {
        if self.rows > 1 {
            self.stage = Stage::RowDistance;
        } else if self.columns > 1 {
            self.stage = Stage::ColumnDistance;
        } else {
            return self.finish(s, Vec::new());
        }
        Ok(Step::Continue)
    }

    fn rect(&mut self, s: &mut Session, column_distance: f64) -> Result<Step> {
        let mats = rect_mats(self.rows, self.columns, self.row_distance, column_distance);
        self.finish(s, mats)
    }
}

impl Interactive for ClassicM {
    fn name(&self) -> &'static str {
        "-ARRAY"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        self.sel = SelectPhase::begin(s);
        if self.sel.done {
            self.objs = self.sel.picked.clone();
            self.stage = Stage::Kind;
        }
        Ok(Step::Continue)
    }
    fn prompt(&self, s: &Session) -> Prompt {
        match self.stage {
            Stage::Select => self.sel.prompt(),
            Stage::Kind => Prompt::new("Enter the type of array", KEYWORDS)
                .kw(&["Rectangular", "Polar"])
                .default(if s.last_used.classic_array_polar { "P" } else { "R" }),
            Stage::Rows => Prompt::new("Enter the number of rows (---)", Accept::NUMBER).default("1"),
            Stage::Columns => Prompt::new("Enter the number of columns (|||)", Accept::NUMBER).default("1"),
            Stage::RowDistance => Prompt::new("Enter the distance between rows or specify unit cell (---)", Accept::POINT_OR_NUMBER),
            Stage::Corner(a) => Prompt::new("Specify opposite corner", Accept::POINT).corner(a),
            Stage::ColumnDistance => Prompt::new("Specify the distance between columns (|||)", Accept::NUMBER),
            Stage::Center => Prompt::new("Specify center point of array", Accept::POINT).kw(&["Base"]),
            Stage::Base => Prompt::new("Specify base point of objects", Accept::POINT),
            Stage::Items => Prompt::new("Enter the number of items in the array", Accept::NUMBER),
            Stage::Fill => Prompt::new("Specify the angle to fill (+=ccw, -=cw)", Accept::NUMBER).default("360"),
            Stage::Rotate => Prompt::new("Rotate arrayed objects?", KEYWORDS).kw(&["Yes", "No"]).default("Y"),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        let max = max_count(s, 1, self.objs.len());
        match (self.stage, i) {
            (Stage::Select, i) => match self.sel.feed(s, &i)? {
                SelOutcome::More => {}
                SelOutcome::Done(hs) => {
                    self.objs = hs;
                    self.stage = Stage::Kind;
                }
                SelOutcome::Empty => return Ok(Step::Done),
            },
            (Stage::Kind, i) => {
                let polar = match &i {
                    Input::Enter => s.last_used.classic_array_polar,
                    Input::Keyword(k) | Input::Text(k) => match k.trim().trim_start_matches('_').to_ascii_lowercase().chars().next() {
                        Some('p') => true,
                        Some('r') => false,
                        _ => return Err(other("Invalid option keyword.")),
                    },
                    _ => return Ok(Step::Continue),
                };
                s.last_used.classic_array_polar = polar;
                self.stage = if polar { Stage::Center } else { Stage::Rows };
            }
            (Stage::Rows, i) => {
                if let Some(n) = count(&i, 1, max)? {
                    self.rows = n;
                    self.stage = Stage::Columns;
                }
            }
            // No more columns than MAXARRAY allows with the rows.
            (Stage::Columns, i) => {
                if let Some(n) = count(&i, 1, max_count(s, self.rows, self.objs.len()))? {
                    self.columns = n;
                    return self.after_counts(s);
                }
            }
            (Stage::RowDistance, Input::Point(a)) => self.stage = Stage::Corner(a),
            (Stage::RowDistance, Input::Text(t)) => {
                self.row_distance = number(&t).filter(|v| v.is_finite()).ok_or_else(|| other("Requires a distance."))?;
                if self.columns <= 1 {
                    return self.rect(s, 0.0);
                }
                self.stage = Stage::ColumnDistance;
            }
            // A unit cell gives both distances.
            (Stage::Corner(a), Input::Point(b)) => {
                self.row_distance = b.y - a.y;
                return self.rect(s, b.x - a.x);
            }
            (Stage::ColumnDistance, Input::Text(t)) => {
                let d = number(&t).filter(|v| v.is_finite()).ok_or_else(|| other("Requires a distance."))?;
                return self.rect(s, d);
            }
            (Stage::Center, Input::Point(c)) => {
                self.center = c;
                self.stage = Stage::Items;
            }
            (Stage::Center, Input::Keyword(k)) if k == "Base" => self.stage = Stage::Base,
            (Stage::Base, Input::Point(b)) => {
                self.base = Some(b);
                self.stage = Stage::Center;
            }
            (Stage::Items, i @ Input::Text(_)) => {
                if let Some(n) = count(&i, 1, max)? {
                    self.items = n;
                    self.stage = Stage::Fill;
                }
            }
            (Stage::Fill, i) => {
                if let Some(a) = angle(&i, 360.0, &s.angle_settings())? {
                    self.fill = a;
                    self.stage = Stage::Rotate;
                }
            }
            (Stage::Rotate, i) => {
                let Some(rotate) = yes_no(&i, true) else { return Err(other("Invalid option keyword.")) };
                let base = self.base.or_else(|| last_point(s, &self.objs)).unwrap_or(self.center);
                let mats = polar_mats(self.center, self.items, self.fill, rotate, base);
                return self.finish(s, mats);
            }
            _ => {}
        }
        Ok(Step::Continue)
    }
}
