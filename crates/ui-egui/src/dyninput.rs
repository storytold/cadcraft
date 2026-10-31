//! Dynamic Input pointer input: the value boxes beside the cursor while a command asks for a point.
//!
//! The command-line text is the single source of truth; the boxes are a view of it:
//! - `40` is typing in the first box (X, or the distance for second and next points, which
//!   default to polar as in AutoCAD; DYNPIFORMAT = 1 makes them Cartesian).
//! - `40,` locks X = 40 and moves to Y; `40<` locks the distance and moves to the angle. A locked
//!   value holds the cursor.
//! - Tab types the separator for the format on show, so `40` Tab `90` reads `40<90`; Tab after the
//!   second value locks it too (both boxes show a lock and the cursor sits on the point).
//! - Enter sends the entry with `@` in front when it is measured from the last point (relative is
//!   the default; DYNPICOORDS = 1 makes it absolute), and fills an empty box from the cursor.
//! - A plain number and Enter stays direct distance entry; `@`/`#` entries, keywords and command
//!   names go through untouched.
//!
//! - Down Arrow at a prompt with options (DYNPROMPT on) lists them at the cursor ([`OptionsMenu`]).
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
    /// The other corner of a rectangle: the boxes are its width and height (lengths beside its
    /// sides, Tab between them), applied in the direction of the cursor from the first corner.
    pub dims: bool,
}

impl Frame {
    /// The format the boxes show before a separator is typed.
    pub fn default_format(&self) -> Format {
        if self.base.is_some() && !self.cartesian && !self.dims { Format::Polar } else { Format::Cartesian }
    }
    /// Where the typed values are measured from.
    pub fn origin(&self) -> Vec2 {
        match self.base {
            Some(b) if !self.absolute || self.dims => b,
            _ => Vec2::ZERO,
        }
    }
    /// Whether values are offsets from the last point.
    pub fn relative(&self) -> bool {
        self.base.is_some() && (!self.absolute || self.dims)
    }

    /// The live (X, Y) or (distance, angle in radians) of `cursor`; a rectangle's width and
    /// height (lengths) in [`Frame::dims`].
    pub fn live(&self, format: Format, cursor: Vec2) -> (f64, f64) {
        let d = cursor - self.origin();
        match format {
            Format::Cartesian if self.dims => (d.x.abs(), d.y.abs()),
            Format::Cartesian => (d.x, d.y),
            Format::Polar => (d.len(), d.y.atan2(d.x)),
        }
    }

    /// A typed width or height as an offset: its length, toward the cursor's side.
    fn toward(&self, v: f64, cursor_offset: f64) -> f64 {
        if self.dims { if cursor_offset < 0.0 { -v.abs() } else { v.abs() } } else { v }
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

/// Whether Tab locks the second value too: both values are typed.
pub fn can_lock_second(text: &str, frame: &Frame) -> bool {
    parse(text, frame).is_some_and(|e| e.split && e.first_locked().is_some() && e.second_value().is_some())
}

/// Hold the cursor to the locked values: the first one once a separator follows it, and both
/// when Tab has locked the second one too (`second_locked`), which pins the cursor to the point.
pub fn constrain(entry: &Entry<'_>, frame: &Frame, p: Vec2, second_locked: bool) -> Vec2 {
    let Some(v) = entry.first_locked() else { return p };
    let o = frame.origin();
    if second_locked && let Some(w) = entry.second_value() {
        let q = match entry.format {
            Format::Cartesian => o + Vec2::new(frame.toward(v, p.x - o.x), frame.toward(w, p.y - o.y)),
            Format::Polar => o + Vec2::from_angle(w) * v,
        };
        if q.is_finite() {
            return q;
        }
    }
    match entry.format {
        Format::Cartesian => Vec2::new(o.x + frame.toward(v, p.x - o.x), p.y),
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
        // A rectangle's width typed alone is that width (the height follows the cursor), as the
        // width box shows, not a distance along the cursor's diagonal.
        if frame.dims && entry.format == Format::Cartesian && cadcraft_engine::units::parse_distance(entry.first).is_some() {
            return complete(&Entry { split: true, second: "", ..entry.clone() }, frame, cursor, last);
        }
        return None;
    }
    let (live_first, live_second) = frame.live(entry.format, constrain(entry, frame, cursor, false));
    let first = entry.first_locked().unwrap_or(live_first);
    let second = entry.second_value().unwrap_or(live_second);
    // A rectangle's width and height go toward the cursor's side of the first corner.
    let (first, second) = match (frame.dims, entry.format) {
        (true, Format::Cartesian) => {
            let d = cursor - frame.origin();
            (frame.toward(first, d.x), frame.toward(second, d.y))
        }
        _ => (first, second),
    };
    if frame.relative() && frame.base == Some(last) {
        return Some(match entry.format {
            Format::Cartesian => format!("@{first},{second}"),
            // `<<`: degrees from +X counterclockwise, as previewed, whatever ANGBASE/ANGDIR say.
            Format::Polar => format!("@{first}<<{}", second.to_degrees()),
        });
    }
    let p = match entry.format {
        Format::Cartesian => frame.origin() + Vec2::new(first, second),
        Format::Polar => frame.origin() + Vec2::from_angle(second) * first,
    };
    p.is_finite().then(|| format!("#{},{}", p.x, p.y))
}

/// The Dynamic Input options menu (DYNPROMPT): Down Arrow at a prompt with keywords lists them
/// at the cursor; the arrows move the highlight and Enter or a click sends the keyword.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OptionsMenu {
    /// The prompt's keywords, as they were when the menu opened.
    pub keywords: Vec<String>,
    /// The highlighted keyword.
    pub index: usize,
}

impl OptionsMenu {
    /// The menu for a prompt's keywords, or `None` when it has none.
    pub fn open(keywords: &[String]) -> Option<Self> {
        (!keywords.is_empty()).then(|| OptionsMenu { keywords: keywords.to_vec(), index: 0 })
    }
    /// Move the highlight down (`step` = 1) or up (-1), wrapping around.
    pub fn step(&mut self, step: isize) {
        let n = self.keywords.len();
        if n > 0 {
            self.index = (self.index % n).checked_add_signed(step).unwrap_or(n - 1) % n;
        }
    }
    /// The highlighted keyword.
    pub fn current(&self) -> Option<&str> {
        self.keywords.get(self.index).map(String::as_str)
    }
    /// Whether the menu still belongs to a prompt with these keywords (it closes when the prompt
    /// moves on).
    pub fn fits(&self, keywords: &[String]) -> bool {
        self.keywords == keywords
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn second_point(base: Vec2) -> Frame {
        Frame { base: Some(base), cartesian: false, absolute: false, dims: false }
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
    fn rectangle_dimensions_go_toward_the_cursor() {
        let f = Frame { base: Some(Vec2::new(10.0, 10.0)), cartesian: false, absolute: true, dims: true };
        // Width and height boxes (Cartesian), whatever DYNPIFORMAT / DYNPICOORDS say.
        assert_eq!(f.default_format(), Format::Cartesian);
        assert_eq!(f.live(Format::Cartesian, Vec2::new(4.0, 13.0)), (6.0, 3.0));
        assert_eq!(tab_separator("100", &f), Some(','));
        // Cursor down-left of the first corner: the rectangle goes that way.
        let p = sent("100,50", &f, Vec2::new(0.0, 0.0), Vec2::new(10.0, 10.0)).unwrap();
        assert!(close(Some(p), Vec2::new(-90.0, -40.0)), "{p:?}");
        let p = sent("100,50", &f, Vec2::new(20.0, 30.0), Vec2::new(10.0, 10.0)).unwrap();
        assert!(close(Some(p), Vec2::new(110.0, 60.0)), "{p:?}");
        // A typed width holds the cursor on its side.
        let e = parse("100,", &f).unwrap();
        assert!(close(Some(constrain(&e, &f, Vec2::new(0.0, 3.0), false)), Vec2::new(-90.0, 3.0)));
        // Only the width typed: the height comes from the cursor.
        let p = sent("100,", &f, Vec2::new(20.0, 4.0), Vec2::new(10.0, 10.0)).unwrap();
        assert!(close(Some(p), Vec2::new(110.0, 4.0)), "{p:?}");
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
        let f = Frame { base: Some(Vec2::new(5.0, 5.0)), cartesian: true, absolute: true, dims: false };
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
        assert!(close(Some(constrain(&e, &f, Vec2::new(1.0, 100.0), false)), Vec2::new(1.0, 6.0)));
        let e = parse("3,", &f).unwrap();
        assert_eq!(constrain(&e, &f, Vec2::new(-7.0, 9.0), false), Vec2::new(4.0, 9.0));
        // Still typing the first value: nothing is locked yet.
        let e = parse("3", &f).unwrap();
        assert_eq!(constrain(&e, &f, Vec2::new(-7.0, 9.0), false), Vec2::new(-7.0, 9.0));
    }

    #[test]
    fn tab_locks_the_second_value_and_pins_the_cursor() {
        let base = Vec2::new(1.0, 1.0);
        let f = second_point(base);
        // Only once both values are typed.
        assert!(!can_lock_second("5", &f));
        assert!(!can_lock_second("5<", &f));
        assert!(can_lock_second("5<90", &f));
        assert!(can_lock_second("3,4", &f));
        let e = parse("5<90", &f).unwrap();
        // Unlocked, the typed angle doesn't hold the cursor; locked, the cursor sits on the point.
        assert!(close(Some(constrain(&e, &f, Vec2::new(100.0, 1.0), false)), Vec2::new(6.0, 1.0)));
        assert!(close(Some(constrain(&e, &f, Vec2::new(100.0, 1.0), true)), Vec2::new(1.0, 6.0)));
        let e = parse("3,4", &f).unwrap();
        assert_eq!(constrain(&e, &f, Vec2::new(-7.0, 9.0), true), Vec2::new(4.0, 5.0));
    }

    #[test]
    fn options_menu_cycles_the_keywords() {
        assert_eq!(OptionsMenu::open(&[]), None);
        let kws: Vec<String> = ["3P", "2P", "Ttr"].iter().map(|s| s.to_string()).collect();
        let mut m = OptionsMenu::open(&kws).unwrap();
        assert_eq!(m.current(), Some("3P"));
        m.step(1);
        assert_eq!(m.current(), Some("2P"));
        m.step(1);
        m.step(1);
        assert_eq!(m.current(), Some("3P"), "wraps past the last");
        m.step(-1);
        assert_eq!(m.current(), Some("Ttr"), "wraps before the first");
        assert!(m.fits(&kws));
        assert!(!m.fits(&kws[..2]));
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

    fn run_frame(app: &mut crate::CadApp, ctx: &egui::Context, events: Vec<egui::Event>) {
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0))),
            events,
            ..Default::default()
        };
        let mut out = ctx.run_ui(raw, |ui| {
            app.logic(ui.ctx());
            app.ui(ui);
        });
        out.textures_delta.clear();
    }

    fn key(key: egui::Key) -> Vec<egui::Event> {
        let m = egui::Modifiers::NONE;
        vec![
            egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: m },
            egui::Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers: m },
        ]
    }

    #[test]
    fn down_arrow_lists_the_options_and_enter_or_a_click_picks_one() {
        let mut app = crate::CadApp::new(cadcraft_engine::Session::new(), crate::Services::default());
        let ctx = egui::Context::default();
        run_frame(&mut app, &ctx, Vec::new());
        let center = app.canvas.rect.map_or(egui::pos2(700.0, 450.0), |r| r.center());
        let hover = vec![egui::Event::PointerMoved(center)];
        app.cmdline("circle");
        let kws = app.session.current_prompt().unwrap().keywords;
        assert!(kws.len() >= 2, "{kws:?}");
        run_frame(&mut app, &ctx, hover.clone());
        run_frame(&mut app, &ctx, key(egui::Key::ArrowDown));
        assert_eq!(app.cmd.options.as_ref().and_then(|m| m.current()), kws.first().map(String::as_str));
        run_frame(&mut app, &ctx, key(egui::Key::ArrowDown));
        assert_eq!(app.cmd.options.as_ref().map(|m| m.index), Some(1));
        // Escape closes the menu and leaves the command running.
        run_frame(&mut app, &ctx, key(egui::Key::Escape));
        assert!(app.cmd.options.is_none() && app.session.running.is_some());
        // Down, Down, Enter: the second option answers the prompt.
        run_frame(&mut app, &ctx, key(egui::Key::ArrowDown));
        run_frame(&mut app, &ctx, key(egui::Key::ArrowDown));
        run_frame(&mut app, &ctx, key(egui::Key::Enter));
        assert!(app.cmd.options.is_none());
        let after = app.session.current_prompt().map(|p| p.message).unwrap_or_default();
        assert!(app.session.running.is_some(), "the option leads to its next prompt");
        assert_ne!(after, "", "{after}");

        // A click on a row picks it.
        app.session.cancel();
        app.cmdline("circle");
        run_frame(&mut app, &ctx, hover);
        run_frame(&mut app, &ctx, key(egui::Key::ArrowDown));
        run_frame(&mut app, &ctx, Vec::new());
        let (row, kw) = app.cmd.option_rows.get(1).cloned().expect("the menu is drawn at the cursor");
        assert_eq!(Some(&kw), kws.get(1));
        let at = row.center();
        let click = |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
        run_frame(&mut app, &ctx, vec![egui::Event::PointerMoved(at)]);
        run_frame(&mut app, &ctx, vec![click(true)]);
        run_frame(&mut app, &ctx, vec![click(false)]);
        assert!(app.cmd.options.is_none());
        assert_eq!(app.session.current_prompt().map(|p| p.message), Some(after), "same option, same next prompt");
        assert!(app.session.doc().unwrap().model.iter().next().is_none(), "the click placed no point");

        // DYNPROMPT = 0: Down Arrow is left to the history.
        app.session.cancel();
        app.session.settings.dynprompt = false;
        app.cmdline("circle");
        run_frame(&mut app, &ctx, key(egui::Key::ArrowDown));
        assert!(app.cmd.options.is_none());
    }

    #[test]
    fn empty_text_is_an_empty_entry() {
        let f = second_point(Vec2::ZERO);
        let e = parse("", &f).unwrap();
        assert_eq!((e.format, e.split), (Format::Polar, false));
    }
}
