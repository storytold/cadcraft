//! Dynamic Input pointer input: the value boxes beside the cursor while a command asks for a point.
//!
//! The command-line text is the single source of truth; the boxes are a view of it:
//! - `40` is typing in the first box (X, or the distance for second and next points, which
//!   default to polar as in AutoCAD; DYNPIFORMAT = 1 makes them Cartesian).
//! - `40,` locks X = 40 and moves to Y; `40<` locks the distance and moves to the angle. A locked
//!   value holds the cursor.
//! - Tab types the separator for the format on show, so `40` Tab `90` reads `40<90`.
//! - Enter sends the entry with `@` in front when it is measured from the last point (relative is
//!   the default; DYNPICOORDS = 1 makes it absolute), and fills an empty box from the cursor.
//! - A plain number and Enter stays direct distance entry; `@`/`#` entries, keywords and command
//!   names go through untouched.
//!
//! Scripts, the control channel and MCP never pass through here, so they keep the plain
//! command-line rules.

use cadcraft_geom::Vec2;

/// How the boxes read the point.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Format {
    /// X and Y.
    #[default]
    Cartesian,
    /// Distance and angle.
    Polar,
}

impl Format {
    /// The character that ends the first value.
    pub fn separator(self) -> char {
        match self {
            Format::Cartesian => ',',
            Format::Polar => '<',
        }
    }
}

/// The settings and prompt the boxes are working for.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Frame {
    /// The prompt's base point (the last point), if it has one.
    pub base: Option<Vec2>,
    /// DYNPIFORMAT = 1: second and next points start Cartesian instead of polar.
    pub cartesian: bool,
    /// DYNPICOORDS = 1: second and next points are absolute instead of relative.
    pub absolute: bool,
}

impl Frame {
    /// The format the boxes show before a separator is typed.
    pub fn default_format(&self) -> Format {
        if self.base.is_some() && !self.cartesian { Format::Polar } else { Format::Cartesian }
    }
    /// Where the typed values are measured from.
    pub fn origin(&self) -> Vec2 {
        match self.base {
            Some(b) if !self.absolute => b,
            _ => Vec2::ZERO,
        }
    }
    /// Whether values are offsets from the last point.
    pub fn relative(&self) -> bool {
        self.base.is_some() && !self.absolute
    }

    /// The live (X, Y) or (distance, angle in radians) of `cursor`.
    pub fn live(&self, format: Format, cursor: Vec2) -> (f64, f64) {
        let d = cursor - self.origin();
        match format {
            Format::Cartesian => (d.x, d.y),
            Format::Polar => (d.len(), d.y.atan2(d.x)),
        }
    }
}

/// The command-line text read as a pointer-input entry.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry<'a> {
    pub format: Format,
    /// Text in the first box (X or distance).
    pub first: &'a str,
    /// A separator was typed: the first box is done and the second box is active.
    pub split: bool,
    /// Text in the second box (Y or angle).
    pub second: &'a str,
}

impl Entry<'_> {
    /// The locked first value, once a separator follows it.
    pub fn first_locked(&self) -> Option<f64> {
        if self.split { cadcraft_engine::units::parse_distance(self.first) } else { None }
    }
    /// The value typed in the second box (radians for an angle).
    pub fn second_value(&self) -> Option<f64> {
        let t = self.second.trim();
        if t.is_empty() {
            return None;
        }
        match self.format {
            Format::Cartesian => cadcraft_engine::units::parse_distance(t),
            Format::Polar => cadcraft_engine::units::parse_angle(t),
        }
    }
}

/// Read `text` as a pointer-input entry, or `None` when it is something else (an explicit
/// `@`/`#` entry, a keyword, a command name, a 3D point).
pub fn parse<'a>(text: &'a str, frame: &Frame) -> Option<Entry<'a>> {
    let t = text.trim();
    if t.starts_with('@') || t.starts_with('#') {
        return None;
    }
    let entry = if let Some((a, b)) = t.split_once(',') {
        Entry { format: Format::Cartesian, first: a.trim(), split: true, second: b.trim() }
    } else if let Some((a, b)) = t.split_once('<') {
        Entry { format: Format::Polar, first: a.trim(), split: true, second: b.trim() }
    } else {
        Entry { format: frame.default_format(), first: t, split: false, second: "" }
    };
    let number_or_empty = |s: &str| s.is_empty() || cadcraft_engine::units::parse_distance(s).is_some();
    if !number_or_empty(entry.first) || entry.second.contains([',', '<']) {
        return None;
    }
    if !entry.second.is_empty() && entry.second_value().is_none() {
        return None;
    }
    Some(entry)
}

/// What Tab types: the separator for the format on show, when the first box is still active.
pub fn tab_separator(text: &str, frame: &Frame) -> Option<char> {
    parse(text, frame).filter(|e| !e.split).map(|e| e.format.separator())
}

/// Hold the cursor to the locked first value.
pub fn constrain(entry: &Entry<'_>, frame: &Frame, p: Vec2) -> Vec2 {
    let Some(v) = entry.first_locked() else { return p };
    let o = frame.origin();
    match entry.format {
        Format::Cartesian => Vec2::new(o.x + v, p.y),
        Format::Polar => {
            let d = p - o;
            let l = d.len();
            let dir = if l > 1e-12 { d * (1.0 / l) } else { Vec2::X };
            o + dir * v
        }
    }
}

/// The command-line text to send for a finished entry, or `None` to send the text as typed
/// (nothing split yet: a plain number stays direct distance entry). Empty boxes take the cursor's
/// value. Entries measured from the last point are sent with `@` when the prompt's base is the
/// session's last point (`last`, what `@` resolves against); otherwise, and for absolute entries,
/// the finished point is sent as `#x,y`, so what was previewed is exactly what is placed.
pub fn complete(entry: &Entry<'_>, frame: &Frame, cursor: Vec2, last: Vec2) -> Option<String> {
    if !entry.split {
        return None;
    }
    let (live_first, live_second) = frame.live(entry.format, constrain(entry, frame, cursor));
    let first = entry.first_locked().unwrap_or(live_first);
    let second = entry.second_value().unwrap_or(live_second);
    if frame.relative() && frame.base == Some(last) {
        return Some(match entry.format {
            Format::Cartesian => format!("@{first},{second}"),
            Format::Polar => format!("@{first}<{}", second.to_degrees()),
        });
    }
    let p = match entry.format {
        Format::Cartesian => frame.origin() + Vec2::new(first, second),
        Format::Polar => frame.origin() + Vec2::from_angle(second) * first,
    };
    p.is_finite().then(|| format!("#{},{}", p.x, p.y))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn second_point(base: Vec2) -> Frame {
        Frame { base: Some(base), cartesian: false, absolute: false }
    }

    fn sent(text: &str, frame: &Frame, cursor: Vec2, last: Vec2) -> Option<Vec2> {
        let e = parse(text, frame)?;
        let t = complete(&e, frame, cursor, last)?;
        cadcraft_engine::prompt::parse_point(&t, last)
    }

    fn close(a: Option<Vec2>, b: Vec2) -> bool {
        a.is_some_and(|a| a.dist(b) < 1e-9)
    }

    #[test]
    fn rectangle_corner_with_comma_is_relative() {
        // Corner snapped at (0,60); `40,40` Enter -> (40,100).
        let base = Vec2::new(0.0, 60.0);
        let f = second_point(base);
        assert!(close(sent("40,40", &f, Vec2::ZERO, base), Vec2::new(40.0, 100.0)));
    }

    #[test]
    fn tab_types_the_separator_for_the_format_on_show() {
        let base = Vec2::new(10.0, 10.0);
        let polar = second_point(base);
        assert_eq!(tab_separator("40", &polar), Some('<'));
        // `40` Tab `90` Enter: 40 units straight up.
        assert!(close(sent("40<90", &polar, Vec2::ZERO, base), Vec2::new(10.0, 50.0)));
        // Already split: Tab does nothing more.
        assert_eq!(tab_separator("40<", &polar), None);
        let cart = Frame { cartesian: true, ..polar };
        assert_eq!(tab_separator("40", &cart), Some(','));
        // First point: always X then Y.
        assert_eq!(tab_separator("3", &Frame::default()), Some(','));
        // Not a number: Tab falls back to AutoComplete.
        assert_eq!(tab_separator("rec", &polar), None);
    }

    #[test]
    fn first_point_is_absolute() {
        let f = Frame::default();
        assert!(close(sent("0,60", &f, Vec2::new(9.0, 9.0), Vec2::new(5.0, 5.0)), Vec2::new(0.0, 60.0)));
    }

    #[test]
    fn absolute_setting_measures_from_the_origin() {
        let f = Frame { base: Some(Vec2::new(5.0, 5.0)), cartesian: true, absolute: true };
        assert!(close(sent("40,100", &f, Vec2::ZERO, Vec2::new(5.0, 5.0)), Vec2::new(40.0, 100.0)));
    }

    #[test]
    fn empty_box_takes_the_cursor() {
        let base = Vec2::new(1.0, 1.0);
        let f = second_point(base);
        // X locked at 3 (relative), Y from the cursor.
        assert!(close(sent("3,", &f, Vec2::new(-7.0, 9.0), base), Vec2::new(4.0, 9.0)));
        // Distance locked at 5, angle from the cursor (straight up).
        assert!(close(sent("5<", &f, Vec2::new(1.0, 100.0), base), Vec2::new(1.0, 6.0)));
    }

    #[test]
    fn locked_value_holds_the_cursor() {
        let base = Vec2::new(1.0, 1.0);
        let f = second_point(base);
        let e = parse("5<", &f).unwrap();
        assert!(close(Some(constrain(&e, &f, Vec2::new(1.0, 100.0))), Vec2::new(1.0, 6.0)));
        let e = parse("3,", &f).unwrap();
        assert_eq!(constrain(&e, &f, Vec2::new(-7.0, 9.0)), Vec2::new(4.0, 9.0));
        // Still typing the first value: nothing is locked yet.
        let e = parse("3", &f).unwrap();
        assert_eq!(constrain(&e, &f, Vec2::new(-7.0, 9.0)), Vec2::new(-7.0, 9.0));
    }

    #[test]
    fn plain_numbers_and_other_text_go_through_as_typed() {
        let f = second_point(Vec2::ZERO);
        // Direct distance entry.
        assert_eq!(parse("25", &f).and_then(|e| complete(&e, &f, Vec2::X, Vec2::ZERO)), None);
        for t in ["@40,40", "#0,0", "c", "rec", "1,2,3", "40,abc"] {
            assert!(parse(t, &f).and_then(|e| complete(&e, &f, Vec2::X, Vec2::ZERO)).is_none(), "{t}");
        }
    }

    #[test]
    fn relative_entries_use_at_only_when_the_base_is_the_last_point() {
        let base = Vec2::new(0.0, 60.0);
        let f = second_point(base);
        let e = parse("40,40", &f).unwrap();
        assert_eq!(complete(&e, &f, Vec2::ZERO, base).as_deref(), Some("@40,40"));
        // A prompt whose base is not the last point: send the finished point itself.
        let other_last = Vec2::new(500.0, 500.0);
        let t = complete(&e, &f, Vec2::ZERO, other_last).unwrap();
        assert!(t.starts_with('#'), "{t}");
        assert!(close(cadcraft_engine::prompt::parse_point(&t, other_last), Vec2::new(40.0, 100.0)));
        // Polar too.
        let e = parse("40<90", &f).unwrap();
        let t = complete(&e, &f, Vec2::ZERO, other_last).unwrap();
        assert!(close(cadcraft_engine::prompt::parse_point(&t, other_last), Vec2::new(0.0, 100.0)));
    }

    #[test]
    fn empty_text_is_an_empty_entry() {
        let f = second_point(Vec2::ZERO);
        let e = parse("", &f).unwrap();
        assert_eq!((e.format, e.split), (Format::Polar, false));
    }
}
