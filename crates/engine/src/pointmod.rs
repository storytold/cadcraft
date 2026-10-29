//! Point modifiers typed at a point prompt, as in AutoCAD: `FROM` (a base point and an offset),
//! `M2P`/`MTP` (the midpoint between two points), the point filters `.x`, `.y`, `.z`, `.xy`,
//! `.xz`, `.yz`, the angle override `<a`, the object snap overrides (`END`, `MID` … `NON`), the
//! temporary tracking point `TT` and tracking `TK`.
//!
//! A modifier pushes a [`Frame`] onto [`Session::point_mods`]. While frames are pending the
//! session shows their prompt ("Base point:", "<Offset>:", "First point of mid:", ".X of",
//! "(need YZ):") instead of the command's, collects the points they need and finally hands one
//! point to the command. Every point prompt of every command gets them this way, from the command
//! line and in scripts alike, and they nest (`FROM` then `M2P`).

use cadcraft_doc::{Drawing, Space};
use cadcraft_geom::Vec2;

use crate::prompt::{Accept, Input, Prompt};
use crate::snap::{SnapHit, mode};
use crate::{Result, Session};

/// The most modifiers pending at once (bounds hostile scripts such as `FROM FROM FROM …`).
const MAX_FRAMES: usize = 32;

/// The coordinates a point filter takes from its first point.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Axes {
    pub x: bool,
    pub y: bool,
    pub z: bool,
}

impl Axes {
    /// `.x`, `.y`, `.z`, `.xy`, `.xz` or `.yz` (case-insensitive).
    pub fn parse(t: &str) -> Option<Axes> {
        let a = match t.to_ascii_lowercase().as_str() {
            ".x" => (true, false, false),
            ".y" => (false, true, false),
            ".z" => (false, false, true),
            ".xy" => (true, true, false),
            ".xz" => (true, false, true),
            ".yz" => (false, true, true),
            _ => return None,
        };
        Some(Axes { x: a.0, y: a.1, z: a.2 })
    }

    fn letters(self, keep: bool) -> String {
        [(self.x, 'X'), (self.y, 'Y'), (self.z, 'Z')].iter().filter(|(on, _)| *on == keep).map(|(_, c)| *c).collect()
    }

    /// The filter as typed: ".XY".
    pub fn name(self) -> String {
        format!(".{}", self.letters(true))
    }

    /// The coordinates still needed: "YZ".
    pub fn need(self) -> String {
        self.letters(false)
    }

    /// The filtered coordinates of `first` with the rest from `second` (Z is not stored: there
    /// is no 3D yet, so a Z taken from either point is dropped).
    pub fn combine(self, first: Vec2, second: Vec2) -> Vec2 {
        Vec2::new(if self.x { first.x } else { second.x }, if self.y { first.y } else { second.y })
    }
}

/// A pending point modifier.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Frame {
    /// `FROM`: the base point once given, then the offset point.
    From { base: Option<Vec2> },
    /// `M2P`/`MTP`: the first point once given, then the second.
    Mid { first: Option<Vec2> },
    /// A point filter: the point to take `keep` from once given, then the rest.
    Filter { keep: Axes, first: Option<Vec2> },
    /// `<a`: the next point lies on the line through `base` in direction `dir` (a unit vector).
    Angle { base: Vec2, dir: Vec2 },
    /// An object snap override for the next point only: one [`mode`] bit, or 0 for `NON` (no
    /// object snap).
    Snap { mode: u32 },
    /// `TT`: the point given is acquired for object snap tracking (it sends alignment paths) and
    /// the prompt below asks again.
    TempTrack,
    /// `TK` (tracking): a run of points, each straight across or up from the one before; Enter
    /// ends it and hands down the last.
    Track { last: Option<Vec2> },
}

/// The object snap overrides and the words they are typed as: any start of a word of three
/// letters or more (`END`, `ENDP`, `endpoint`).
const SNAP_WORDS: [(u32, &[&str]); 15] = [
    (mode::END, &["endpoint"]),
    (mode::MID, &["midpoint"]),
    (mode::CEN, &["center", "centre"]),
    (mode::GCEN, &["gcenter", "gcentre"]),
    (mode::NOD, &["node"]),
    (mode::QUA, &["quadrant"]),
    (mode::INT, &["intersection"]),
    (mode::APP, &["apparent", "appint"]),
    (mode::EXT, &["extension"]),
    (mode::INS, &["insertion"]),
    (mode::PER, &["perpendicular"]),
    (mode::TAN, &["tangent"]),
    (mode::NEA, &["nearest"]),
    (mode::PAR, &["parallel"]),
    (0, &["none"]),
];

/// The object snap override `t` names (`_endp`, `MID`, `non` …): its mode bit, 0 for `NON`.
pub fn parse_snap(t: &str) -> Option<u32> {
    let b = bare(t).to_ascii_lowercase();
    if b.len() < 3 {
        return None;
    }
    SNAP_WORDS.iter().find(|(_, words)| words.iter().any(|w| w.starts_with(&b))).map(|(m, _)| *m)
}

/// The name of a snap mode ("Endpoint"), "None" for 0.
pub fn snap_name(m: u32) -> &'static str {
    mode::ALL.iter().find(|(b, _)| *b == m).map_or("None", |(_, n)| *n)
}

/// The candidate snaps near `cursor` for Tab cycling: the snap [`crate::snap::osnap`] picks
/// first, then the closest snap of each other mode in `osmode`, nearest first, without repeats.
pub fn snap_candidates(d: &Drawing, space: &Space, cursor: Vec2, aperture: f64, osmode: u32, base: Option<Vec2>, deferred: bool) -> Vec<SnapHit> {
    snap_candidates_in_orbit(d, space, cadcraft_render::Orbit3d::default(), cursor, aperture, osmode, base, deferred)
}

/// The candidate snaps near the 2D view-plane `view_cursor` under `orbit` for Tab cycling: the
/// snap [`crate::snap::osnap_in_orbit`] picks first, then the closest snap of each other mode in
/// `osmode` (sorted by projected 2D distance to `view_cursor`), without repeats.
#[allow(clippy::too_many_arguments)]
pub fn snap_candidates_in_orbit(
    d: &Drawing,
    space: &Space,
    orbit: cadcraft_render::Orbit3d,
    view_cursor: Vec2,
    aperture: f64,
    osmode: u32,
    base: Option<Vec2>,
    deferred: bool,
) -> Vec<SnapHit> {
    if osmode & !mode::HATCH == 0 || osmode & mode::OFF != 0 {
        return Vec::new();
    }
    let osnap = |m: u32| crate::snap::osnap_in_orbit_with_proj(d, space, orbit, view_cursor, aperture, m, base, deferred);
    let extra = osmode & mode::HATCH;
    let mut per_mode: Vec<(SnapHit, Vec2)> = mode::ALL.iter().filter(|(b, _)| osmode & b != 0).filter_map(|(b, _)| osnap(*b | extra)).collect();
    per_mode.sort_by(|a, b| a.1.dist(view_cursor).total_cmp(&b.1.dist(view_cursor)));
    let mut out: Vec<SnapHit> = osnap(osmode).map(|(h, _)| h).into_iter().collect();
    for (h, _) in per_mode {
        if !out.iter().any(|o| o.mode == h.mode && o.point.near(h.point, aperture * 1e-6)) {
            out.push(h);
        }
    }
    out
}

/// Strip the international (`_`) and transparent (`'`) prefixes a modifier may be typed with.
fn bare(t: &str) -> &str {
    t.trim().trim_start_matches(['\'', '_'])
}

/// Whether point modifiers apply at this prompt: it asks for a point that is not an object pick.
pub fn accepts(prompt: &Prompt) -> bool {
    prompt.accept.point && !prompt.picks_objects()
}

impl Session {
    /// The prompt shown while modifiers are pending, built over the command's own prompt.
    pub(crate) fn overlay_prompt(&self, cmd: Prompt) -> Prompt {
        let point = Accept::POINT;
        self.point_mods.iter().fold(cmd, |p, f| match *f {
            Frame::From { base: None } => Prompt::new("Base point", point),
            Frame::From { base: Some(b) } => Prompt::new("<Offset>", point).base(b),
            Frame::Mid { first: None } => Prompt::new("First point of mid", point),
            Frame::Mid { first: Some(f) } => Prompt::new("Second point of mid", point).base(f),
            Frame::Filter { keep, first: None } => Prompt::new(format!("{} of", keep.name()), point).base_opt(p.base),
            Frame::Filter { keep, first: Some(_) } => Prompt::new(format!("(need {})", keep.need()), point).base_opt(p.base),
            Frame::TempTrack => Prompt::new("Specify temporary OTRACK point", point),
            Frame::Track { last: None } => Prompt::new("First tracking point", point),
            Frame::Track { last: Some(l) } => Prompt::new("Next point (Press ENTER to end tracking)", point).base(l),
            Frame::Angle { .. } | Frame::Snap { mode: 0 } => p,
            // "Endpoint of:", keeping the base and deferral PER/TAN need.
            Frame::Snap { mode } => Prompt { message: format!("{} of", snap_name(mode)), keywords: Vec::new(), default: None, ..p },
        })
    }

    /// The object snap override for the next pick ([`Frame::Snap`]): `Some(0)` after `NON`,
    /// `None` when the running object snaps apply. The UI snaps the cursor with it.
    pub fn snap_override(&self) -> Option<u32> {
        match self.point_mods.last() {
            Some(Frame::Snap { mode }) if self.running.is_some() => Some(*mode),
            _ => None,
        }
    }

    /// The modifier `t` names at `prompt`, if any.
    fn parse_modifier(&self, prompt: &Prompt, t: &str) -> Option<Frame> {
        // The command's keywords come first (`cen` is ZOOM's or ARC's Center, `ext` ZOOM's
        // Extents); the `_` and `'` forms (`_cen`) always name the modifier.
        if prompt.match_keyword(t).is_some() {
            return None;
        }
        let b = bare(t);
        if let Some(a) = b.strip_prefix('<') {
            // `<a`, `<<a`, `<<<a`, read like the angle of `@d<a`.
            let angles = self.angle_settings();
            let ang = if a.starts_with('<') { angles.direction(&format!("<{a}"))? } else { angles.direction(a)? };
            let dir = Vec2::from_angle(ang);
            let base = prompt.base.unwrap_or(self.last_point);
            return (dir.is_finite() && base.is_finite()).then_some(Frame::Angle { base, dir });
        }
        if let Some(keep) = Axes::parse(b) {
            return Some(Frame::Filter { keep, first: None });
        }
        if let Some(mode) = parse_snap(b) {
            return Some(Frame::Snap { mode });
        }
        let b = b.to_ascii_lowercase();
        match b.as_str() {
            "from" | "fro" => Some(Frame::From { base: None }),
            "m2p" | "mtp" => Some(Frame::Mid { first: None }),
            "tt" => Some(Frame::TempTrack),
            // TK, or TRACKING from three letters (TRA, TRACK).
            "tk" => Some(Frame::Track { last: None }),
            _ if b.len() >= 3 && "tracking".starts_with(&b) => Some(Frame::Track { last: None }),
            _ => None,
        }
    }

    /// A token typed at a point prompt that a modifier handles: a modifier name, or a distance
    /// along an angle override. `None` leaves the token to the usual parsing.
    pub(crate) fn typed_modifier(&mut self, prompt: &Prompt, t: &str) -> Option<Result<()>> {
        if !accepts(prompt) {
            return None;
        }
        if let Some(f) = self.parse_modifier(prompt, t) {
            if self.point_mods.len() >= MAX_FRAMES {
                self.echo("Too many point modifiers.");
                return Some(Ok(()));
            }
            match f {
                Frame::Angle { .. } => self.echo(format!("Angle Override: {}", bare(t).trim_start_matches('<'))),
                // A second snap override replaces the first.
                Frame::Snap { .. } if matches!(self.point_mods.last(), Some(Frame::Snap { .. })) => {
                    self.point_mods.pop();
                }
                _ => {}
            }
            self.point_mods.push(f);
            return Some(Ok(()));
        }
        // A distance at "Next point" of TK: straight across or up from the last tracking point,
        // the way the cursor lies from it.
        if let Some(Frame::Track { last: Some(l) }) = self.point_mods.last().copied()
            && crate::prompt::parse_point_with(t, self.last_point, &self.angle_settings()).is_none()
            && let Some(d) = crate::units::parse_distance(t)
        {
            let dir = (crate::snap::ortho(l, self.cursor) - l).normalized();
            let dir = if dir == Vec2::ZERO || !dir.is_finite() { Vec2::X } else { dir };
            return Some(self.modifier_point(l + dir * d));
        }
        if let Some(Frame::Angle { base, dir }) = self.point_mods.last().copied() {
            // Typed coordinates are explicit: they win over the override.
            if crate::prompt::parse_point_with(t, self.last_point, &self.angle_settings()).is_some() {
                self.point_mods.pop();
                return None;
            }
            if let Some(d) = crate::units::parse_distance(t) {
                self.point_mods.pop();
                return Some(self.modifier_point(base + dir * d));
            }
        }
        None
    }

    /// An input while modifiers are pending.
    pub(crate) fn modifier_input(&mut self, input: Input) -> Result<()> {
        // An angle override only constrains a point: anything else goes to the prompt below it.
        if let Some(Frame::Angle { .. }) = self.point_mods.last()
            && !matches!(input, Input::Point(_) | Input::Deferred(_))
        {
            self.point_mods.pop();
            return self.input(input);
        }
        match input {
            Input::Point(p) => self.modifier_point(p),
            // A deferred tangent/perpendicular picked under a TAN/PER override.
            Input::Deferred(d) if matches!(self.point_mods.as_slice(), [Frame::Snap { .. }]) => {
                self.point_mods.clear();
                self.input(Input::Deferred(d))
            }
            Input::Deferred(d) => self.modifier_point(d.at),
            // Enter ends tracking with its last point; otherwise it drops the modifiers and
            // returns to the command's prompt.
            Input::Enter => match self.point_mods.last().copied() {
                Some(Frame::Track { last: Some(l) }) => {
                    self.point_mods.pop();
                    self.modifier_point(l)
                }
                _ => {
                    self.point_mods.clear();
                    Ok(())
                }
            },
            Input::Cancel => {
                self.point_mods.clear();
                self.input(Input::Cancel)
            }
            Input::Keyword(_) | Input::Text(_) | Input::Pick(_) => {
                self.echo("Invalid point.");
                Ok(())
            }
        }
    }

    /// A point given while modifiers are pending: complete the top frame, and when it has its
    /// point, hand that to the frame (or command) below.
    pub(crate) fn modifier_point(&mut self, mut p: Vec2) -> Result<()> {
        if !p.is_finite() {
            self.echo("Invalid point.");
            return Ok(());
        }
        while let Some(top) = self.point_mods.pop() {
            match top {
                Frame::From { base: None } => {
                    self.point_mods.push(Frame::From { base: Some(p) });
                    // `@dx,dy` at "<Offset>:" is measured from the base point.
                    self.last_point = p;
                    return Ok(());
                }
                Frame::Mid { first: None } => {
                    self.point_mods.push(Frame::Mid { first: Some(p) });
                    self.last_point = p;
                    return Ok(());
                }
                Frame::Filter { keep, first: None } => {
                    self.point_mods.push(Frame::Filter { keep, first: Some(p) });
                    return Ok(());
                }
                Frame::TempTrack => {
                    self.tracking.acquire_temporary(p);
                    self.last_point = p;
                    return Ok(());
                }
                Frame::Track { last } => {
                    // Each point after the first goes straight across or up from the last one.
                    let q = last.map_or(p, |l| crate::snap::ortho(l, p));
                    self.point_mods.push(Frame::Track { last: Some(q) });
                    self.last_point = q;
                    return Ok(());
                }
                // The offset point is already measured from the base (`@` reads the last point).
                Frame::From { base: Some(_) } => {}
                Frame::Mid { first: Some(f) } => p = (f + p) * 0.5,
                Frame::Filter { keep, first: Some(f) } => p = keep.combine(f, p),
                Frame::Angle { base, dir } => p = base + dir * (p - base).dot(dir),
                Frame::Snap { mode: 0 } => {}
                Frame::Snap { mode } => {
                    // Snap the point with this mode alone, within the aperture.
                    let prompt = self.current_prompt();
                    let space = self.space();
                    let orbit = self.edit_orbit();
                    let aperture = self.pixel_size() * self.settings.aperture.max(1.0);
                    let hatch = if self.settings.osnaphatch { mode::HATCH } else { 0 };
                    let base = prompt.as_ref().and_then(|q| q.base);
                    let deferred = prompt.as_ref().is_some_and(|q| q.deferred);
                    let view_p = if orbit.is_top() { p } else { orbit.project(p.to3(0.0)) };
                    let hit = crate::snap::osnap_in_orbit(self.doc()?, &space, orbit, view_p, aperture, mode | hatch, base, deferred);
                    match hit {
                        Some(h) if self.point_mods.is_empty() => return self.input_snapped(h),
                        Some(h) => p = h.point,
                        None => {
                            self.echo(format!("No {} found for specified point.", snap_name(mode)));
                            return Ok(());
                        }
                    }
                }
            }
        }
        self.point_mods.clear();
        self.feed(Some(Input::Point(p)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters() {
        let a = Axes::parse(".X").unwrap();
        assert_eq!((a.name().as_str(), a.need().as_str()), (".X", "YZ"));
        assert_eq!(a.combine(Vec2::new(1.0, 2.0), Vec2::new(3.0, 4.0)), Vec2::new(1.0, 4.0));
        let a = Axes::parse(".yz").unwrap();
        assert_eq!((a.name().as_str(), a.need().as_str()), (".YZ", "X"));
        assert_eq!(a.combine(Vec2::new(1.0, 2.0), Vec2::new(3.0, 4.0)), Vec2::new(3.0, 2.0));
        assert!(Axes::parse(".5").is_none() && Axes::parse(".w").is_none());
    }

    #[test]
    fn snap_words() {
        for (t, m) in [("END", mode::END), ("_endp", mode::END), ("'_mid", mode::MID), ("cen", mode::CEN), ("gcen", mode::GCEN)] {
            assert_eq!(parse_snap(t), Some(m), "{t}");
        }
        for (t, m) in [("perp", mode::PER), ("Insert", mode::INS), ("appint", mode::APP), ("NON", 0), ("none", 0)] {
            assert_eq!(parse_snap(t), Some(m), "{t}");
        }
        for t in ["en", "endx", "line", "x", "no"] {
            assert_eq!(parse_snap(t), None, "{t}");
        }
    }
}
