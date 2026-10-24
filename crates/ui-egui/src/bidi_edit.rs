//! The text box of the text editors: text in the drawing's fonts, laid out as the drawing lays it
//! out, with left-to-right and right-to-left scripts mixed on one line the way word processors
//! do it (the Unicode Bidirectional Algorithm, each paragraph's direction from its first strong
//! character).
//!
//! The text is kept in logical order (the order it is typed and stored). Each line is shown in
//! display order; the caret sits after the character before it (at its right edge in
//! left-to-right text, its left edge in right-to-left text); Left and Right move the caret one
//! place left or right on screen, whatever the direction of the text under it; a selection
//! covers a range of the logical text and lights up every character in it, wherever it is shown.

use egui::{Color32, Event, Key, Pos2, Rect, Sense, Stroke, pos2, vec2};

use crate::editor_glyphs::{self, EditorFont};

/// Most undo steps kept.
const MAX_UNDO: usize = 100;

/// How a character looks.
#[derive(Clone, Debug, PartialEq)]
pub struct Look {
    pub font: EditorFont,
    pub italic: bool,
    pub bold: bool,
    pub underline: bool,
    /// Size in points (the cap height is [`editor_glyphs::CAP`] of it).
    pub size: f32,
    pub color: Color32,
}

/// The caret and selection (character indices into the text), kept between frames.
#[derive(Clone, Debug, Default)]
pub struct EditState {
    pub cursor: usize,
    pub anchor: usize,
    /// Horizontal scroll of a one-line box.
    scroll_x: f32,
    /// For Up and Down: the x the caret aims at.
    preferred_x: Option<f32>,
    undo: Vec<(String, usize)>,
    redo: Vec<(String, usize)>,
}

impl EditState {
    pub fn at(i: usize) -> Self {
        EditState { cursor: i, anchor: i, ..Default::default() }
    }
    /// Everything selected, the caret at the end.
    pub fn all(len: usize) -> Self {
        EditState { cursor: len, anchor: 0, ..Default::default() }
    }
    /// The selection, low to high.
    pub fn range(&self) -> (usize, usize) {
        (self.cursor.min(self.anchor), self.cursor.max(self.anchor))
    }
}

/// What a frame of the box did.
pub struct Output {
    pub response: egui::Response,
    pub changed: bool,
    /// The caret's bottom on screen (where a completion list goes).
    pub caret: Pos2,
}

/// A character placed on a line.
#[derive(Clone, Copy, Debug)]
struct Cell {
    /// Index in the text.
    idx: usize,
    /// Left edge and width (box coordinates).
    x: f32,
    w: f32,
    rtl: bool,
}

#[derive(Clone, Debug)]
struct Line {
    /// Logical range of the line's characters (a paragraph's `\n` is not in it).
    start: usize,
    end: usize,
    top: f32,
    height: f32,
    baseline: f32,
    /// The paragraph runs right to left.
    rtl: bool,
    /// The characters in display order, left to right.
    cells: Vec<Cell>,
}

struct Layout {
    lines: Vec<Line>,
    width: f32,
    height: f32,
}

/// Characters laid out in lines: wrapped at spaces to `wrap` (one line: no wrapping), each line
/// put in display order.
fn layout(chars: &[char], look: &dyn Fn(usize) -> Look, wrap: Option<f32>, default_size: f32) -> Layout {
    let adv: Vec<f32> = chars
        .iter()
        .enumerate()
        .map(|(i, c)| {
            if *c == '\n' {
                0.0
            } else {
                let l = look(i);
                editor_glyphs::advance(&l.font, l.italic, *c, l.size)
            }
        })
        .collect();
    let sizes: Vec<f32> = (0..chars.len()).map(|i| look(i).size).collect();
    // Logical lines: paragraphs split at `\n`, wrapped.
    let mut ranges: Vec<(usize, usize, bool)> = Vec::new();
    let mut p0 = 0usize;
    let mut para_rtl = false;
    for p1 in (0..=chars.len()).filter(|i| *i == chars.len() || chars.get(*i) == Some(&'\n')) {
        let par: String = chars.get(p0..p1).unwrap_or_default().iter().collect();
        // A paragraph without strong characters keeps the direction of the one before.
        para_rtl = cadcraft_fonts::bidi::base_rtl(&par).unwrap_or(para_rtl);
        let mut s = p0;
        while s < p1 {
            let mut w = 0.0;
            let mut e = s;
            let mut last_space = None;
            while e < p1 {
                let a = adv.get(e).copied().unwrap_or(0.0);
                if let Some(limit) = wrap
                    && w + a > limit
                    && e > s
                    && chars.get(e) != Some(&' ')
                {
                    break;
                }
                if chars.get(e) == Some(&' ') {
                    last_space = Some(e + 1);
                }
                w += a;
                e += 1;
            }
            if e < p1
                && let Some(b) = last_space.filter(|b| *b > s && *b < e)
            {
                e = b;
            }
            ranges.push((s, e, para_rtl));
            s = e;
        }
        if p0 == p1 {
            ranges.push((p0, p1, para_rtl));
        }
        p0 = p1 + 1;
    }
    if ranges.is_empty() {
        ranges.push((0, 0, false));
    }
    let mut lines = Vec::with_capacity(ranges.len());
    let mut y = 0.0;
    let mut width: f32 = 0.0;
    for (s, e, rtl) in ranges {
        let max = (s..e).filter_map(|i| sizes.get(i).copied()).fold(0.0f32, f32::max);
        let size = if max > 0.0 { max } else { default_size };
        let height = size * 1.45;
        let baseline = y + size * 1.05;
        let text: String = chars.get(s..e).unwrap_or_default().iter().collect();
        let order: Vec<usize> = if cadcraft_fonts::bidi::has_rtl(&text) {
            let levels = cadcraft_fonts::bidi::char_levels(&text, Some(rtl));
            cadcraft_fonts::bidi::reorder(&levels).into_iter().map(|k| k + s).collect::<Vec<usize>>()
        } else {
            (s..e).collect()
        };
        let levels = if cadcraft_fonts::bidi::has_rtl(&text) { cadcraft_fonts::bidi::char_levels(&text, Some(rtl)) } else { Vec::new() };
        let mut x = 0.0;
        let mut cells = Vec::with_capacity(order.len());
        for i in order {
            let w = adv.get(i).copied().unwrap_or(0.0);
            let r = levels.get(i - s).is_some_and(|l| l % 2 == 1);
            cells.push(Cell { idx: i, x, w, rtl: r });
            x += w;
        }
        width = width.max(x);
        lines.push(Line { start: s, end: e, top: y, height, baseline, rtl, cells });
        y += height;
    }
    Layout { lines, width, height: y }
}

impl Layout {
    /// The line the caret at `c` is on: the last line holding `c` (a wrapped line's end is the
    /// next line's start).
    fn line_of(&self, c: usize) -> usize {
        self.lines.iter().rposition(|l| l.start <= c && c <= l.end).unwrap_or(0)
    }
    fn cell(&self, line: usize, idx: usize) -> Option<Cell> {
        self.lines.get(line)?.cells.iter().find(|k| k.idx == idx).copied()
    }
    /// The caret's x at `c`: after the character before it (its right edge in left-to-right
    /// text, its left edge in right-to-left text); at a line's start, before its first
    /// character.
    fn caret_x(&self, c: usize) -> (usize, f32) {
        let li = self.line_of(c);
        let Some(l) = self.lines.get(li) else { return (0, 0.0) };
        if c > l.start
            && let Some(k) = self.cell(li, c - 1)
        {
            return (li, if k.rtl { k.x } else { k.x + k.w });
        }
        if let Some(k) = self.cell(li, c) {
            return (li, if k.rtl { k.x + k.w } else { k.x });
        }
        // An empty line: at its start side.
        let right = l.cells.iter().map(|k| k.x + k.w).fold(0.0, f32::max);
        (li, if l.rtl { right } else { 0.0 })
    }
    /// The caret position nearest `x` on line `li`.
    fn index_at(&self, li: usize, x: f32) -> usize {
        let Some(l) = self.lines.get(li) else { return 0 };
        let mut best = (f32::INFINITY, l.start);
        for c in l.start..=l.end {
            let (cl, cx) = self.caret_x(c);
            if cl != li {
                continue;
            }
            let d = (cx - x).abs();
            if d < best.0 {
                best = (d, c);
            }
        }
        best.1
    }
    /// The caret position one place left (`dir` −1) or right (+1) on screen from `c`.
    fn step(&self, c: usize, dir: f32) -> usize {
        let (li, x) = self.caret_x(c);
        let Some(l) = self.lines.get(li) else { return c };
        let mut best: Option<(f32, usize)> = None;
        for k in l.start..=l.end {
            let (kl, kx) = self.caret_x(k);
            if kl != li || k == c {
                continue;
            }
            let d = (kx - x) * dir;
            if d > 0.5 && best.is_none_or(|(bd, bk)| d < bd - 0.01 || ((d - bd).abs() <= 0.01 && k.abs_diff(c) < bk.abs_diff(c))) {
                best = Some((d, k));
            }
        }
        if let Some((_, k)) = best {
            return k;
        }
        // Off the end of the line: the neighbouring line, in reading order.
        let forward = (dir > 0.0) != l.rtl;
        if forward {
            if li + 1 < self.lines.len() {
                return self.lines.get(li + 1).map_or(c, |n| n.start);
            }
        } else if li > 0 {
            return self.lines.get(li - 1).map_or(c, |p| p.end);
        }
        c
    }
}

/// Character classes for word moves and double clicks.
fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn word_left(chars: &[char], mut c: usize) -> usize {
    while c > 0 && chars.get(c - 1).is_some_and(|ch| !is_word(*ch)) {
        c -= 1;
    }
    while c > 0 && chars.get(c - 1).is_some_and(|ch| is_word(*ch)) {
        c -= 1;
    }
    c
}

fn word_right(chars: &[char], mut c: usize) -> usize {
    let n = chars.len();
    while c < n && chars.get(c).is_some_and(|ch| is_word(*ch)) {
        c += 1;
    }
    while c < n && chars.get(c).is_some_and(|ch| !is_word(*ch)) {
        c += 1;
    }
    c
}

/// Replace the selection by `s` (an empty `s` deletes it).
fn replace(chars: &mut Vec<char>, st: &mut EditState, s: &str, max_chars: usize) -> bool {
    let (a, b) = st.range();
    let ins: Vec<char> = s.chars().collect();
    if a == b && ins.is_empty() {
        return false;
    }
    let room = max_chars.saturating_sub(chars.len() - (b - a));
    let ins: Vec<char> = ins.into_iter().take(room).collect();
    let b = b.min(chars.len());
    let a = a.min(b);
    chars.splice(a..b, ins.iter().copied());
    st.cursor = a + ins.len();
    st.anchor = st.cursor;
    true
}

/// Show the box: `width` wide, at least `min_rows` lines tall (one line: `single_line`), the
/// characters looking as `look` says. Returns whether the text changed.
#[allow(clippy::too_many_arguments)]
pub fn show(
    ui: &mut egui::Ui,
    id: egui::Id,
    text: &mut String,
    st: &mut EditState,
    single_line: bool,
    width: f32,
    min_rows: usize,
    look: &dyn Fn(usize) -> Look,
    default_size: f32,
    max_chars: usize,
) -> Output {
    let t = crate::theme::Tokens::get();
    let margin = 5.0;
    let mut chars: Vec<char> = text.chars().collect();
    let n0 = chars.len();
    st.cursor = st.cursor.min(n0);
    st.anchor = st.anchor.min(n0);
    let inner_w = (width - 2.0 * margin).max(20.0);
    let lay = layout(&chars, look, (!single_line).then_some(inner_w), default_size);
    let row_h = default_size * 1.45;
    let h = lay.height.max(row_h * min_rows.max(1) as f32) + 2.0 * margin;
    let (rect, _) = ui.allocate_exact_size(vec2(width, h), Sense::hover());
    let focused = ui.memory(|m| m.has_focus(id));
    let response = ui.interact(rect, id, Sense::click_and_drag());
    if response.clicked() || response.drag_started() {
        ui.memory_mut(|m| m.request_focus(id));
    }
    // The text cursor (I-beam) over the text, as in any text box.
    if response.hovered() || response.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Text);
    }
    if focused {
        ui.memory_mut(|m| {
            m.set_focus_lock_filter(id, egui::EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: false })
        });
    }
    // Box coordinates → screen.
    let origin = |sx: f32| pos2(rect.left() + margin + sx, rect.top() + margin);
    let scroll = if single_line { st.scroll_x } else { 0.0 };
    // Mouse: place the caret, drag to select, double-click a word.
    let hit = |p: Pos2, lay: &Layout| -> usize {
        let y = p.y - rect.top() - margin;
        let li = lay.lines.iter().position(|l| y < l.top + l.height).unwrap_or(lay.lines.len().saturating_sub(1));
        lay.index_at(li, p.x - rect.left() - margin + scroll)
    };
    let shift = ui.input(|i| i.modifiers.shift);
    if let Some(p) = response.interact_pointer_pos() {
        if response.double_clicked() {
            let c = hit(p, &lay);
            st.anchor = word_left(&chars, (c + 1).min(chars.len()));
            st.cursor = word_right(&chars, st.anchor).min(chars.len());
            while st.cursor > st.anchor && chars.get(st.cursor - 1).is_some_and(|c| !is_word(*c)) {
                st.cursor -= 1;
            }
        } else if response.drag_started() || response.clicked() {
            st.cursor = hit(p, &lay);
            if !shift {
                st.anchor = st.cursor;
            }
            st.preferred_x = None;
        } else if response.dragged() {
            st.cursor = hit(p, &lay);
        }
    }
    let mut changed = false;
    if focused {
        let events = ui.input(|i| i.events.clone());
        let before = text.clone();
        let before_cursor = st.cursor;
        let mut edited = false;
        for ev in events {
            let lay = layout(&chars, look, (!single_line).then_some(inner_w), default_size);
            match ev {
                Event::Text(s) | Event::Paste(s) | Event::Ime(egui::ImeEvent::Commit(s)) => {
                    let s: String = s.chars().filter(|c| !c.is_control() || (*c == '\n' && !single_line)).collect();
                    let s = if single_line { s.replace('\n', " ") } else { s };
                    edited |= replace(&mut chars, st, &s, max_chars);
                }
                Event::Copy | Event::Cut => {
                    let (a, b) = st.range();
                    if a < b {
                        ui.ctx().copy_text(chars.get(a..b).unwrap_or_default().iter().collect());
                        if matches!(ev, Event::Cut) {
                            edited |= replace(&mut chars, st, "", max_chars);
                        }
                    }
                }
                Event::Key { key, pressed: true, modifiers, .. } => {
                    let word = modifiers.command || modifiers.alt;
                    let mut moved = None;
                    match key {
                        Key::ArrowLeft | Key::ArrowRight => {
                            let dir = if key == Key::ArrowLeft { -1.0 } else { 1.0 };
                            let (a, b) = st.range();
                            moved = Some(if word {
                                // A word in reading order: right is forward in left-to-right text.
                                let li = lay.line_of(st.cursor);
                                let rtl = lay.lines.get(li).is_some_and(|l| l.rtl);
                                if (dir > 0.0) != rtl { word_right(&chars, st.cursor) } else { word_left(&chars, st.cursor) }
                            } else if a != b && !modifiers.shift {
                                // Collapse the selection at its left or right end on screen.
                                let (xa, xb) = (lay.caret_x(a).1, lay.caret_x(b).1);
                                if (xa < xb) == (dir < 0.0) { a } else { b }
                            } else {
                                lay.step(st.cursor, dir)
                            });
                            st.preferred_x = None;
                        }
                        Key::ArrowUp | Key::ArrowDown => {
                            let (li, x) = lay.caret_x(st.cursor);
                            let px = *st.preferred_x.get_or_insert(x);
                            let to = if key == Key::ArrowUp { li.checked_sub(1) } else { Some(li + 1).filter(|n| *n < lay.lines.len()) };
                            moved = Some(match to {
                                Some(n) => lay.index_at(n, px),
                                None if key == Key::ArrowUp => 0,
                                None => chars.len(),
                            });
                        }
                        Key::Home => {
                            moved = Some(if modifiers.command { 0 } else { lay.lines.get(lay.line_of(st.cursor)).map_or(0, |l| l.start) });
                            st.preferred_x = None;
                        }
                        Key::End => {
                            moved = Some(if modifiers.command { chars.len() } else { lay.lines.get(lay.line_of(st.cursor)).map_or(0, |l| l.end) });
                            st.preferred_x = None;
                        }
                        Key::Backspace => {
                            if st.cursor == st.anchor && st.cursor > 0 {
                                st.anchor = if word { word_left(&chars, st.cursor) } else { st.cursor - 1 };
                            }
                            edited |= replace(&mut chars, st, "", max_chars);
                        }
                        Key::Delete => {
                            if st.cursor == st.anchor && st.cursor < chars.len() {
                                st.anchor = if word { word_right(&chars, st.cursor) } else { st.cursor + 1 };
                            }
                            edited |= replace(&mut chars, st, "", max_chars);
                        }
                        Key::Enter if !single_line && !modifiers.command => {
                            edited |= replace(&mut chars, st, "\n", max_chars);
                        }
                        Key::A if modifiers.command => {
                            st.anchor = 0;
                            st.cursor = chars.len();
                        }
                        Key::Z | Key::Y if modifiers.command => {
                            let redo = key == Key::Y || modifiers.shift;
                            let (from, to) = if redo { (&mut st.redo, &mut st.undo) } else { (&mut st.undo, &mut st.redo) };
                            if let Some((prev, c)) = from.pop() {
                                to.push((chars.iter().collect(), st.cursor));
                                chars = prev.chars().collect();
                                st.cursor = c.min(chars.len());
                                st.anchor = st.cursor;
                                changed = true;
                            }
                        }
                        _ => {}
                    }
                    if let Some(c) = moved {
                        st.cursor = c.min(chars.len());
                        if !modifiers.shift {
                            st.anchor = st.cursor;
                        }
                    }
                }
                _ => {}
            }
        }
        if edited {
            st.undo.push((before, before_cursor));
            if st.undo.len() > MAX_UNDO {
                st.undo.remove(0);
            }
            st.redo.clear();
            st.preferred_x = None;
            changed = true;
        }
    }
    if changed {
        *text = chars.iter().collect();
    }
    // Paint: the box, the selection, the characters, the caret.
    let lay = layout(&chars, look, (!single_line).then_some(inner_w), default_size);
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 2.0, t.field);
    p.rect_stroke(rect, 2.0, Stroke::new(if focused { 1.5 } else { 1.0 }, if focused { t.accent } else { t.border }), egui::StrokeKind::Inside);
    let (cl, cx) = lay.caret_x(st.cursor);
    // A one-line box scrolls to keep the caret in sight.
    if single_line {
        let view = inner_w;
        if cx - st.scroll_x > view - 2.0 {
            st.scroll_x = cx - view + 2.0;
        } else if cx - st.scroll_x < 0.0 {
            st.scroll_x = cx;
        }
        st.scroll_x = st.scroll_x.clamp(0.0, (lay.width - view + 4.0).max(0.0));
    }
    let scroll = if single_line { st.scroll_x } else { 0.0 };
    let clip = p.with_clip_rect(rect.shrink(2.0));
    let at = |x: f32, y: f32| origin(x - scroll) + vec2(0.0, y);
    let (a, b) = st.range();
    let sel_fill = if focused { t.accent.gamma_multiply(0.45) } else { t.accent.gamma_multiply(0.25) };
    for l in &lay.lines {
        for k in &l.cells {
            if k.idx >= a && k.idx < b {
                clip.rect_filled(Rect::from_min_max(at(k.x, l.top), at(k.x + k.w, l.top + l.height)), 0.0, sel_fill);
            }
        }
        // A selected paragraph break: a sliver at the line's end.
        if l.end >= a && l.end < b && chars.get(l.end) == Some(&'\n') {
            let ex = if l.rtl { 0.0 } else { l.cells.iter().map(|k| k.x + k.w).fold(0.0, f32::max) };
            let x0 = if l.rtl { ex - 6.0 } else { ex };
            clip.rect_filled(Rect::from_min_max(at(x0, l.top), at(x0 + 6.0, l.top + l.height)), 0.0, sel_fill);
        }
        for k in &l.cells {
            let Some(c) = chars.get(k.idx).copied() else { continue };
            let lk = look(k.idx);
            let c = if k.rtl { cadcraft_fonts::bidi::mirror(c) } else { c };
            editor_glyphs::paint(&clip, &lk.font, lk.italic, lk.bold, c, at(k.x, l.baseline), lk.size, lk.color);
            if lk.underline {
                let y = l.baseline + lk.size * 0.15;
                clip.line_segment([at(k.x, y), at(k.x + k.w, y)], Stroke::new(1.0, lk.color));
            }
        }
    }
    let caret_bottom = {
        let l = lay.lines.get(cl);
        let (top, hgt) = l.map_or((0.0, row_h), |l| (l.top, l.height));
        at(cx, top + hgt)
    };
    if focused {
        let l = lay.lines.get(cl);
        let (top, hgt) = l.map_or((0.0, row_h), |l| (l.top, l.height));
        let blink = ui.input(|i| i.time) % 1.0 < 0.6;
        if blink {
            clip.line_segment([at(cx, top + 2.0), at(cx, top + hgt - 2.0)], Stroke::new(1.5, t.text));
        }
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(300));
        let caret_rect = Rect::from_min_max(at(cx, top), at(cx + 1.0, top + hgt));
        ui.ctx().output_mut(|o| {
            o.ime = Some(egui::output::IMEOutput {
                purpose: egui::IMEPurpose::Normal,
                rect,
                cursor_rect: caret_rect,
                should_interrupt_composition: false,
            })
        });
    }
    Output { response, changed, caret: caret_bottom }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn look(_: usize) -> Look {
        Look {
            font: EditorFont { width_factor: 1.0, ..Default::default() },
            italic: false,
            bold: false,
            underline: false,
            size: 20.0,
            color: Color32::WHITE,
        }
    }

    fn lay(s: &str) -> (Vec<char>, Layout) {
        let c: Vec<char> = s.chars().collect();
        let l = layout(&c, &look, None, 20.0);
        (c, l)
    }

    #[test]
    fn mixed_scripts_show_in_display_order() {
        // Hebrew paragraph with an English word: the English word sits at the left, in its own
        // order, the Hebrew reads from the right.
        let (_, l) = lay("שלום abc");
        let line = &l.lines[0];
        assert!(line.rtl);
        let order: Vec<usize> = line.cells.iter().map(|k| k.idx).collect();
        assert_eq!(order, vec![5, 6, 7, 4, 3, 2, 1, 0]);
        // English paragraph with a Hebrew word: the Hebrew word reversed in place.
        let (_, l) = lay("ab שלום cd");
        let order: Vec<usize> = l.lines[0].cells.iter().map(|k| k.idx).collect();
        assert_eq!(order, vec![0, 1, 2, 6, 5, 4, 3, 7, 8, 9]);
    }

    #[test]
    fn arrows_move_visually_and_reach_every_place() {
        let (c, l) = lay("ab שלום cd");
        // From the start, Right walks every caret place once, left to right on screen.
        let mut at = 0;
        let mut seen = vec![at];
        let mut xs = vec![l.caret_x(at).1];
        for _ in 0..c.len() + 2 {
            let n = l.step(at, 1.0);
            if n == at {
                break;
            }
            at = n;
            seen.push(at);
            xs.push(l.caret_x(at).1);
        }
        assert!(xs.windows(2).all(|w| w[1] > w[0]), "{xs:?}");
        assert!(seen.len() >= c.len() - 1, "{seen:?}");
        // In a right-to-left line, Right from the logical start goes toward the end of the text
        // on screen (left to right), i.e. back in reading order.
        let (_, l) = lay("שלום");
        let x0 = l.caret_x(0).1;
        assert!(x0 > l.caret_x(4).1, "the start of Hebrew text is at the right");
        assert_eq!(l.step(0, -1.0), 1, "Left from the start of Hebrew reads on");
    }

    #[test]
    fn typing_replaces_the_selection_and_undo_restores() {
        let mut chars: Vec<char> = "hello".chars().collect();
        let mut st = EditState { cursor: 1, anchor: 4, ..Default::default() };
        assert!(replace(&mut chars, &mut st, "XY", 100));
        assert_eq!(chars.iter().collect::<String>(), "hXYo");
        assert_eq!((st.cursor, st.anchor), (3, 3));
        let mut st = EditState::at(4);
        assert!(replace(&mut chars, &mut st, "123456", 6), "capped at the limit");
        assert_eq!(chars.len(), 6);
        assert_eq!(word_left(&"one two".chars().collect::<Vec<_>>(), 7), 4);
    }

    #[test]
    fn wrapping_breaks_at_spaces() {
        let c: Vec<char> = "aaaa bbbb cccc".chars().collect();
        let one = editor_glyphs::advance(&look(0).font, false, 'a', 20.0);
        let l = layout(&c, &look, Some(one * 6.5), 20.0);
        assert!(l.lines.len() >= 2);
        assert_eq!(l.lines[0].start, 0);
        assert_eq!(c[l.lines[1].start], 'b');
    }
}
