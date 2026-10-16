//! The floating command line: history, the active prompt with clickable keywords, typed input
//! (keyboard focus stays on the drawing — typing anywhere goes here) and AutoComplete.

use std::collections::HashSet;

use cadcraft_engine::Input;
use egui::{Key, Pos2, Rect, Sense, Stroke, pos2, vec2};

use crate::CadApp;
use crate::theme::Tokens;

#[derive(Default)]
pub struct CmdLine {
    pub buffer: String,
    /// The selection as of the last frame, to notice when it changes.
    seen_selection: Vec<cadcraft_doc::Handle>,
    /// Something was typed at the command line since the selection was made.
    typed_since_selection: bool,
    pub history: Vec<String>,
    pub history_pos: Option<usize>,
    /// What was typed at command prompts (not command names), oldest first: the in-command
    /// shortcut menu's Recent Input.
    pub recent_input: Vec<String>,
    /// The expanded history window is open (F2, the chevron, or a click on the history lines).
    pub expanded: bool,
    /// The command line resizing under the pointer: what the drag started from.
    resize_from: Option<ResizeFrom>,
    /// The Dynamic Input options menu at the cursor (Down Arrow at a prompt with keywords).
    pub options: Option<crate::dyninput::OptionsMenu>,
    /// The text whose second Dynamic Input value Tab locked; the lock holds while it is unchanged.
    locked_second: Option<String>,
    /// Where the options menu's rows were drawn last frame (screen), for clicks on them.
    pub option_rows: Vec<(Rect, String)>,
    /// Where the Dynamic Input tooltip stays while the options menu is open.
    pub options_anchor: Option<Pos2>,
}

/// The most history lines the command line shows above its bar.
pub const MAX_LINES: usize = 12;
const LINE_H: f32 = 16.0;

/// The narrowest the command bar gets, in points: below this a prompt and its input line have no
/// room at all. The widest is the drawing area less [`BAR_MARGIN`].
pub const MIN_WIDTH: f32 = 360.0;

/// How much drawing area the command bar leaves beside itself at its widest, in points.
pub const BAR_MARGIN: f32 = 8.0;

/// What a command-line resize drag started from: the pointer and the size when it began, so the
/// size follows the pointer instead of accumulating frame by frame.
#[derive(Clone, Copy, Debug)]
enum ResizeFrom {
    /// Its top edge: (pointer y, history lines).
    Lines(f32, usize),
    /// Its left or right edge: (pointer x, bar width), `right` false for the left edge, whose
    /// anchor is the bar's right edge.
    Width(f32, f32, bool),
}

/// The command bar's width: the saved width ([`crate::UiState::cmdline_width`], points), or 48 %
/// of the drawing area when the user has never resized it. Clamped so the bar always fits the
/// drawing area whatever the window did since the width was saved, and never narrower than
/// [`MIN_WIDTH`] while the area can hold that (in a narrower window the bar takes the whole area).
/// A non-finite saved value (a hostile profile) falls back to the default.
pub fn bar_width(app: &CadApp, canvas: Rect) -> f32 {
    let max = (canvas.width() - BAR_MARGIN).max(0.0);
    let saved = app.ui.cmdline_width.filter(|w| w.is_finite() && *w > 0.0);
    let want = saved.unwrap_or_else(|| (canvas.width() * 0.48).clamp(MIN_WIDTH, 760.0));
    want.clamp(MIN_WIDTH.min(max), max)
}

/// The Dynamic Input frame for the prompt on screen, or `None` when the value boxes are not live
/// (Dynamic Input off, no point prompt or a pending selection window).
pub fn dyn_frame(app: &CadApp) -> Option<crate::dyninput::Frame> {
    let st = &app.session.settings;
    if !st.dynmode || app.session.pending_window.is_some() {
        return None;
    }
    let p = app.session.current_prompt().filter(cadcraft_engine::pointmod::accepts)?;
    Some(crate::dyninput::Frame { base: p.base, cartesian: st.dynpi_cartesian, absolute: st.dynpi_absolute })
}

/// Take Tab away from egui before it moves keyboard focus to the next widget (which would leave
/// the command line deaf to typing). While no widget has focus, a pressed Tab becomes a `"\t"`
/// text event, which keeps its place among the other keystrokes; [`keyboard`] handles it.
pub fn capture_tab(ctx: &egui::Context, raw: &mut egui::RawInput) {
    if ctx.egui_wants_keyboard_input() {
        return;
    }
    raw.events.retain_mut(|e| {
        let tab = match e {
            egui::Event::Key { key: Key::Tab, pressed, .. } => Some(*pressed),
            _ => None,
        };
        match tab {
            Some(true) => {
                *e = egui::Event::Text("\t".into());
                true
            }
            Some(false) => false,
            None => true,
        }
    });
}

/// Tab on the command line: cycle the object snaps under the cursor, type the Dynamic Input
/// separator (`,` or `<`) after a value, or complete a command name.
fn tab(app: &mut CadApp) {
    // Grip dimensional input: Tab locks the typed value and moves to the next field.
    if crate::grip_dyn::tab(app) {
        return;
    }
    // Over a snap marker with nothing typed, Tab cycles the candidate snaps.
    if app.cmd.buffer.is_empty() && app.canvas.snap.is_some() {
        app.canvas.snap_tab = true;
    } else if let Some(f) = dyn_frame(app)
        && let Some(sep) = crate::dyninput::tab_separator(&app.cmd.buffer, &f)
    {
        app.cmd.buffer.push(sep);
    } else if let Some(f) = dyn_frame(app)
        && crate::dyninput::can_lock_second(&app.cmd.buffer, &f)
    {
        // Both values typed: Tab locks the second one as well.
        app.cmd.locked_second = Some(app.cmd.buffer.clone());
    } else if let Some(s) = suggestions(&app.cmd.buffer).first() {
        app.cmd.buffer = s.id.clone();
    }
}

/// The most lines of prompt keywords the command bar draws above itself.
pub const MAX_KEYWORD_LINES: usize = 6;

/// How a prompt's keywords are laid out: the ones that fit after `open` on the bar's own line,
/// and the rest, one line each, above it.
pub struct KeywordLayout {
    /// The keywords drawn after the prompt.
    pub inline: usize,
    /// The keywords drawn one per line above the bar (at most [`MAX_KEYWORD_LINES`] lines).
    pub above: Vec<Vec<String>>,
    /// Keywords that fit nowhere (drawn as "+N more" at the end of the last line).
    pub hidden: usize,
}

/// Fit a prompt's keywords into `width` points after `open` points of prompt text: as many as
/// fit on the bar's own line, then the rest, one line per `line_width`, above it, up to
/// `MAX_KEYWORD_LINES` lines. `open` is the room left for the first keyword, so the caller keeps
/// its own margins; `sep_width` is the width of the "/" between keywords and `bracket` the room
/// the closing "]" needs. The first keyword is always inline (the bar clips it, as it did before
/// this layout existed). Pure, so the layout is testable without a UI frame.
pub fn keyword_layout(keywords: &[String], widths: &[f32], sep_width: f32, bracket: f32, open: f32, line_width: f32) -> KeywordLayout {
    let wid = |i: usize| widths.get(i).copied().unwrap_or(0.0);
    // The bar's own line: `open` counts the space the first keyword starts at, so a keyword only
    // has to fit from the second one on.
    let mut inline = 0;
    let mut used = 0.0f32;
    while inline < keywords.len() {
        let need = if inline == 0 { wid(0) - sep_width } else { wid(inline) + sep_width };
        if inline > 0 && used + need > open - bracket {
            break;
        }
        used += need;
        inline += 1;
    }
    // The lines above: filled left to right, one budget each.
    let mut above: Vec<Vec<String>> = Vec::new();
    let mut line: Vec<String> = Vec::new();
    let mut room = line_width;
    for (i, k) in keywords.iter().enumerate().skip(inline) {
        let need = if line.is_empty() { wid(i) } else { wid(i) + sep_width };
        if need > room && !line.is_empty() {
            above.push(std::mem::take(&mut line));
            room = line_width;
            if above.len() == MAX_KEYWORD_LINES {
                break;
            }
        }
        room = if room > need { room - need } else { 0.0 };
        line.push(k.clone());
    }
    if !line.is_empty() && above.len() < MAX_KEYWORD_LINES {
        above.push(line);
    }
    let shown: usize = above.iter().map(Vec::len).sum();
    KeywordLayout { inline, above, hidden: keywords.len() - inline - shown }
}

/// An AutoComplete suggestion for the command line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Suggestion {
    /// Command identifier (uppercase, e.g. "RECTANG"), used for buffer completion and execution.
    pub id: String,
    /// Display text shown in the AutoComplete list (e.g. "REC (RECTANG)" or "RECOVER").
    pub display: String,
    /// Human-readable command label (e.g. "Rectangle").
    pub label: &'static str,
}

/// Matching command names/aliases for AutoComplete.
fn suggestions(prefix: &str) -> Vec<Suggestion> {
    if prefix.is_empty() || prefix.contains(' ') {
        return Vec::new();
    }
    let p = prefix.to_ascii_lowercase();

    let mut out: Vec<Suggestion> = Vec::new();
    let mut seen_ids: HashSet<&'static str> = HashSet::new();

    // 1. Exact match: alias resolution or exact command name match.
    let resolved = cadcraft_engine::cmd::resolve_alias(&p);
    if let Some(spec) = cadcraft_engine::cmd::find_command(&resolved)
        && !spec.id.contains('.')
    {
        let display =
            if spec.id == p { spec.id.to_ascii_uppercase() } else { format!("{} ({})", p.to_ascii_uppercase(), spec.id.to_ascii_uppercase()) };
        out.push(Suggestion { id: spec.id.to_ascii_uppercase(), display, label: spec.label });
        seen_ids.insert(spec.id);
    }

    // 2. Command ID prefix matches (excluding already matched exact command).
    let mut id_matches: Vec<&'static cadcraft_engine::CommandSpec> =
        cadcraft_engine::command_specs().iter().filter(|c| !c.id.contains('.') && !seen_ids.contains(c.id) && c.id.starts_with(&p)).collect();
    id_matches.sort_by_key(|c| (c.id.len(), c.id));
    for c in id_matches {
        out.push(Suggestion { id: c.id.to_ascii_uppercase(), display: c.id.to_ascii_uppercase(), label: c.label });
        seen_ids.insert(c.id);
    }

    // 3. Command alias prefix matches (for commands not yet added).
    let mut alias_matches: Vec<(&'static str, &'static cadcraft_engine::CommandSpec)> = Vec::new();
    for c in cadcraft_engine::command_specs() {
        if c.id.contains('.') || seen_ids.contains(c.id) {
            continue;
        }
        let matching_alias = c.aliases.iter().copied().filter(|a| a.starts_with(&p)).min_by_key(|a| (a.len(), *a));
        if let Some(alias) = matching_alias {
            alias_matches.push((alias, c));
        }
    }
    alias_matches.sort_by_key(|(alias, c)| (alias.len(), *alias, c.id));
    for (alias, c) in alias_matches {
        if seen_ids.insert(c.id) {
            out.push(Suggestion {
                id: c.id.to_ascii_uppercase(),
                display: format!("{} ({})", alias.to_ascii_uppercase(), c.id.to_ascii_uppercase()),
                label: c.label,
            });
        }
    }

    out.truncate(8);
    out
}

impl CmdLine {
    /// Track the selection: a new selection starts with nothing typed since.
    pub fn watch_selection(&mut self, sel: &[cadcraft_doc::Handle]) {
        if self.seen_selection != sel {
            self.seen_selection = sel.to_vec();
            self.typed_since_selection = false;
        }
    }

    /// Note that the command line holds typed text.
    pub fn note_typing(&mut self) {
        if !self.buffer.is_empty() {
            self.typed_since_selection = true;
        }
    }

    /// Tab locked the second Dynamic Input value and nothing was typed since.
    pub fn second_locked(&self) -> bool {
        self.locked_second.as_deref() == Some(self.buffer.as_str())
    }

    /// The options menu row under `at` (screen), if the menu is open.
    pub fn option_at(&self, at: Pos2) -> Option<String> {
        self.options.as_ref()?;
        self.option_rows.iter().find(|(r, _)| r.contains(at)).map(|(_, k)| k.clone())
    }
}

/// Keep the options menu only while its prompt is on screen with Dynamic Input and DYNPROMPT on.
fn options_still_fit(app: &mut CadApp) {
    let st = &app.session.settings;
    let on = st.dynmode && st.dynprompt && app.canvas.hot_grips.is_empty();
    let kws = app.session.current_prompt().map(|p| p.keywords).unwrap_or_default();
    if app.cmd.options.as_ref().is_some_and(|m| !on || !m.fits(&kws)) {
        app.cmd.options = None;
    }
}

/// Down Arrow at a prompt with keywords (Dynamic Input on, DYNPROMPT on, not browsing the
/// history): open the options menu at the cursor, or move its highlight. `false` when the key is
/// left for the history.
fn options_key(app: &mut CadApp, step: isize) -> bool {
    if let Some(m) = app.cmd.options.as_mut() {
        m.step(step);
        return true;
    }
    let st = &app.session.settings;
    if step < 0 || !st.dynmode || !st.dynprompt || app.cmd.history_pos.is_some() || !app.canvas.hot_grips.is_empty() {
        return false;
    }
    let kws = app.session.current_prompt().map(|p| p.keywords).unwrap_or_default();
    app.cmd.options = crate::dyninput::OptionsMenu::open(&kws);
    app.cmd.options.is_some()
}

/// Send a keyword picked from the options menu, as a click on it at the command line does.
pub fn pick_option(app: &mut CadApp, keyword: String) {
    app.cmd.options = None;
    app.cmd.buffer.clear();
    if let Err(e) = app.session.input(Input::Keyword(keyword)) {
        app.session.echo(e.to_string());
    }
}

/// Whether Backspace should erase the selection instead of editing the command line. The key
/// labelled "delete" on a Mac sends Backspace, so on macOS it erases selected objects, but only
/// when it can't be meant for the command line: a fresh, plain press (never an auto-repeat, so
/// holding it to clear typing can't run on into the drawing, and not Option/Cmd+delete, which
/// delete a word or line of text on macOS), no command running, the command line empty, and
/// nothing typed there since the selection was made.
pub fn backspace_erases(mac: bool, press: KeyPress, buffer: &str, command_running: bool, has_selection: bool, typed_since_selection: bool) -> bool {
    mac && press.plain && !press.repeat && buffer.is_empty() && !command_running && has_selection && !typed_since_selection
}

/// How a key was pressed.
#[derive(Clone, Copy, Debug)]
pub struct KeyPress {
    /// The operating system's auto-repeat while the key is held.
    pub repeat: bool,
    /// No Option, Control or Command held (Shift doesn't matter).
    pub plain: bool,
}

/// Handle keyboard input destined for the command line (when no text field has focus).
pub fn keyboard(app: &mut CadApp, ctx: &egui::Context) {
    if ctx.egui_wants_keyboard_input() {
        return;
    }
    let events = ctx.input(|i| i.events.clone());
    let selection = app.session.selection();
    app.cmd.watch_selection(&selection);
    let mac = ctx.os() == egui::os::OperatingSystem::Mac;
    let text_prompt = app.session.current_prompt().is_some_and(|p| p.accept.text && !p.accept.point && !p.accept.number);
    options_still_fit(app);
    for ev in events {
        // The options menu takes the arrows, Enter and Escape; typing closes it.
        if app.cmd.options.is_some() {
            let space = matches!(&ev, egui::Event::Text(t) if t == " ") && !text_prompt;
            match &ev {
                _ if space => {
                    if let Some(k) = app.cmd.options.as_ref().and_then(|m| m.current()).map(str::to_owned) {
                        pick_option(app, k);
                    }
                    continue;
                }
                egui::Event::Key { key: Key::Enter, pressed: true, .. } => {
                    if let Some(k) = app.cmd.options.as_ref().and_then(|m| m.current()).map(str::to_owned) {
                        pick_option(app, k);
                    }
                    continue;
                }
                egui::Event::Key { key: Key::Escape, pressed: true, .. } => {
                    app.cmd.options = None;
                    continue;
                }
                egui::Event::Key { key: Key::ArrowUp, pressed: true, modifiers, .. } if !modifiers.any() => {
                    options_key(app, -1);
                    continue;
                }
                egui::Event::Key { key: Key::ArrowDown | Key::ArrowUp, pressed: true, .. } => {}
                // Space arrives as a key and as text; the text picks.
                egui::Event::Key { key: Key::Space, .. } => {}
                egui::Event::Text(_) | egui::Event::Key { pressed: true, .. } => app.cmd.options = None,
                _ => {}
            }
        }
        match ev {
            egui::Event::Text(t) => {
                if t == "\t" {
                    tab(app);
                } else if t == " " && !text_prompt {
                    submit(app);
                } else {
                    app.cmd.buffer.push_str(&t);
                }
            }
            egui::Event::Key { key, pressed: true, repeat, modifiers, .. } => match key {
                Key::Enter => submit(app),
                Key::Escape => {
                    app.cmd.buffer.clear();
                    app.session.cancel();
                }
                Key::Backspace => {
                    let press = KeyPress { repeat, plain: !(modifiers.alt || modifiers.ctrl || modifiers.command) };
                    let erase = backspace_erases(
                        mac,
                        press,
                        &app.cmd.buffer,
                        app.session.prompting(),
                        !app.session.selection().is_empty(),
                        app.cmd.typed_since_selection,
                    );
                    if erase {
                        let _ = app.run("erase.selection", serde_json::json!({}));
                    } else {
                        app.cmd.buffer.pop();
                    }
                }
                Key::ArrowUp if !modifiers.any() => {
                    if !app.cmd.history.is_empty() {
                        let n = app.cmd.history.len();
                        let pos = app.cmd.history_pos.map(|p| p.saturating_sub(1)).unwrap_or(n - 1);
                        app.cmd.history_pos = Some(pos);
                        app.cmd.buffer = app.cmd.history.get(pos).cloned().unwrap_or_default();
                    }
                }
                Key::ArrowDown if !modifiers.any() => {
                    if !options_key(app, 1)
                        && let Some(p) = app.cmd.history_pos
                    {
                        let np = p + 1;
                        if np >= app.cmd.history.len() {
                            app.cmd.history_pos = None;
                            app.cmd.buffer.clear();
                        } else {
                            app.cmd.history_pos = Some(np);
                            app.cmd.buffer = app.cmd.history.get(np).cloned().unwrap_or_default();
                        }
                    }
                }
                Key::Tab => tab(app),
                Key::Delete if app.cmd.buffer.is_empty() && !app.session.prompting() => {
                    let _ = app.run("erase.selection", serde_json::json!({}));
                }
                _ => {}
            },
            _ => {}
        }
        app.cmd.note_typing();
    }
}

pub fn submit(app: &mut CadApp) {
    // Grip dimensional input: the typed lengths and angles place the grip.
    if crate::grip_dyn::submit(app) {
        return;
    }
    // Dynamic Input: a split entry (`40,30`, `40<90`, `40,`) is completed from the cursor and
    // measured from the last point.
    if let Some(f) = dyn_frame(app) {
        let cursor = app.canvas.cursor.unwrap_or(app.session.cursor);
        let last = app.session.last_point;
        let done = crate::dyninput::parse(&app.cmd.buffer, &f).and_then(|e| crate::dyninput::complete(&e, &f, cursor, last));
        if let Some(text) = done {
            app.cmd.buffer = text;
        }
    }
    let text = std::mem::take(&mut app.cmd.buffer);
    app.cmd.locked_second = None;
    app.cmd.history_pos = None;
    if !text.trim().is_empty() && !app.session.prompting() {
        app.cmd.history.push(text.clone());
        if app.cmd.history.len() > 200 {
            app.cmd.history.remove(0);
        }
    }
    crate::context_menu::note_input(app, &text);
    if text.is_empty() {
        if let Err(e) = app.session.input(Input::Enter) {
            app.session.echo(e.to_string());
        }
        return;
    }
    app.cmdline(&text);
}

fn visible_lines(app: &CadApp, canvas: Rect, bar: Rect) -> usize {
    let available = ((bar.top() - canvas.top() - 8.0).max(0.0) / LINE_H) as usize;
    app.ui.history_lines.min(MAX_LINES).min(available)
}

pub fn show(app: &mut CadApp, ui: &mut egui::Ui, canvas: Rect) {
    let t = Tokens::get();
    let w = bar_width(app, canvas);
    let h = 24.0;
    let bar = Rect::from_min_size(pos2(canvas.center().x - w / 2.0, canvas.bottom() - h - 10.0), vec2(w, h));
    let p = ui.painter_at(canvas);
    history(app, ui, canvas, bar);
    // A prompt with more keywords than fit on the bar (PURGE, for one) draws the rest one line
    // each above it, like the history lines they sit over: no keyword runs off the bar, and the
    // ones on those lines are the same set read left to right.
    let font = crate::theme::body();
    let kws: Vec<String> = app.session.current_prompt().filter(|pr| !pr.raw).map(|pr| pr.keywords.clone()).unwrap_or_default();
    let sep = p.layout_no_wrap("/".into(), font.clone(), t.text).size().x;
    let bracket = p.layout_no_wrap("]".into(), font.clone(), t.text).size().x;
    let open = 30.0
        + if app.session.running.as_ref().is_some_and(|r| r.id != "grip") { 40.0 } else { 0.0 }
        + p.layout_no_wrap(
            if app.session.current_prompt().is_some_and(|pr| pr.message.is_empty()) { " [".to_string() } else { crate::tl!(" or [").into() },
            font.clone(),
            t.text,
        )
        .size()
        .x;
    let (kw_inline, kw_lines): (Option<usize>, Vec<String>) = if kws.is_empty() {
        (None, Vec::new())
    } else {
        let widths: Vec<f32> = kws.iter().map(|k| p.layout_no_wrap(k.clone(), font.clone(), t.cmd_keyword).size().x).collect();
        let l = keyword_layout(&kws, &widths, sep, bracket, w - open, w - 56.0);
        let mut rows: Vec<String> = Vec::with_capacity(l.above.len());
        for (n, row) in l.above.iter().enumerate() {
            // The marker goes on the last line that shows keywords, never in a row of its own:
            // `above` never runs past MAX_KEYWORD_LINES.
            if n + 1 == l.above.len() && l.hidden > 0 {
                rows.push(format!("{}/+{} more", row.join("/"), l.hidden));
            } else {
                rows.push(row.join("/"));
            }
        }
        (Some(l.inline), rows)
    };
    let lh = LINE_H;
    let room = ((bar.top() - canvas.top() - 8.0).max(0.0) / lh) as usize;
    let kw_shown = kw_lines.len().min(room);
    let kw_start = bar.top() - lh * kw_shown as f32 - 2.0;
    // History lines above the bar (the expanded window replaces them); the keyword lines sit
    // directly on the bar and the history above them.
    let n = if app.cmd.expanded { 0 } else { visible_lines(app, canvas, bar).saturating_sub(kw_shown) };
    let lines: Vec<String> = app.session.log.iter().rev().take(n).rev().cloned().collect();
    if !lines.is_empty() {
        let hr = Rect::from_min_size(pos2(bar.left(), kw_start - lh * lines.len() as f32 - 4.0), vec2(w, lh * lines.len() as f32 + 2.0));
        p.rect_filled(hr, 3.0, t.cmd_history);
        for (i, l) in lines.iter().enumerate() {
            p.text(pos2(hr.left() + 8.0, hr.top() + 1.0 + lh * i as f32), egui::Align2::LEFT_TOP, l, crate::theme::small(), t.text_dim);
        }
    }
    if kw_shown > 0 {
        let kr = Rect::from_min_size(pos2(bar.left(), kw_start), vec2(w, lh * kw_shown as f32 + 2.0));
        p.rect_filled(kr, 3.0, t.cmd_history);
        for (i, row) in kw_lines.iter().take(kw_shown).enumerate() {
            p.text(pos2(kr.left() + 8.0, kr.top() + 1.0 + lh * i as f32), egui::Align2::LEFT_TOP, row, font.clone(), t.cmd_keyword);
        }
    }
    p.rect_filled(bar, 3.0, t.cmd_bg);
    p.rect_stroke(bar, 3.0, Stroke::new(1.0, t.cmd_border), egui::StrokeKind::Inside);
    // Prompt glyph.
    p.text(pos2(bar.left() + 8.0, bar.center().y), egui::Align2::LEFT_CENTER, ">_", crate::theme::mono(), t.text_dim);
    let mut x = bar.left() + 30.0;
    let prompt = app.session.current_prompt();
    match &prompt {
        Some(pr) => {
            // The grip modes name themselves ("** STRETCH **…").
            let name = app.session.running.as_ref().filter(|r| r.id != "grip").map(|r| r.id.to_ascii_uppercase() + " ").unwrap_or_default();
            let g = p.layout_no_wrap(name, font.clone(), t.text_faint);
            let gw = g.size().x;
            p.galley(pos2(x, bar.center().y - g.size().y / 2.0), g, t.text_faint);
            x += gw;
            // A LISP `get…` prompt shows as the routine wrote it (no keywords, default or colon added).
            let message = if pr.raw { pr.display() } else { crate::i18n::t(&pr.message).to_string() };
            let g = p.layout_no_wrap(message, font.clone(), t.text);
            let gw = g.size().x;
            p.galley(pos2(x, bar.center().y - g.size().y / 2.0), g, t.text);
            x += gw;
            if !pr.raw && !pr.keywords.is_empty() {
                let g = p.layout_no_wrap(if pr.message.is_empty() { " [".into() } else { crate::tl!(" or [").into() }, font.clone(), t.text);
                let gw = g.size().x;
                p.galley(pos2(x, bar.center().y - g.size().y / 2.0), g, t.text);
                x += gw;
                // Only the keywords that fit: the rest are on the lines above ([`kw_inline`]).
                let upto = kw_inline.unwrap_or(0).min(pr.keywords.len());
                let kws = pr.keywords[..upto].to_vec();
                for (i, k) in kws.iter().enumerate() {
                    let g = p.layout_no_wrap(k.clone(), font.clone(), t.cmd_keyword);
                    let r = Rect::from_min_size(pos2(x, bar.top() + 3.0), vec2(g.size().x, h - 6.0));
                    let resp = ui.interact(r, ui.id().with(("kw", i)), Sense::click());
                    if resp.hovered() {
                        p.rect_filled(r, 2.0, t.cmd_keyword_hover);
                    }
                    let gw = g.size().x;
                    p.galley(pos2(x, bar.center().y - g.size().y / 2.0), g, t.cmd_keyword);
                    x += gw;
                    if resp.clicked() {
                        let _ = app.session.input(Input::Keyword(k.clone()));
                    }
                    if i + 1 < kws.len() {
                        let g = p.layout_no_wrap("/".into(), font.clone(), t.text);
                        let gw = g.size().x;
                        p.galley(pos2(x, bar.center().y - g.size().y / 2.0), g, t.text);
                        x += gw;
                    }
                }
                let g = p.layout_no_wrap("]".into(), font.clone(), t.text);
                let gw = g.size().x;
                p.galley(pos2(x, bar.center().y - g.size().y / 2.0), g, t.text);
                x += gw;
            }
            if let Some(d) = pr.default.as_ref().filter(|_| !pr.raw) {
                let g = p.layout_no_wrap(format!(" <{d}>"), font.clone(), t.text);
                let gw = g.size().x;
                p.galley(pos2(x, bar.center().y - g.size().y / 2.0), g, t.text);
                x += gw;
            }
            if !pr.raw {
                let g = p.layout_no_wrap(": ".into(), font.clone(), t.text);
                let gw = g.size().x;
                p.galley(pos2(x, bar.center().y - g.size().y / 2.0), g, t.text);
                x += gw;
            } else if !pr.message.ends_with(' ') {
                x += 4.0;
            }
        }
        None => {
            if app.cmd.buffer.is_empty() {
                p.text(
                    pos2(x, bar.center().y),
                    egui::Align2::LEFT_CENTER,
                    crate::tl!("Type a command"),
                    egui::FontId::proportional(12.5),
                    t.text_faint,
                );
            }
        }
    }
    let g = p.layout_no_wrap(app.cmd.buffer.clone(), font, t.text);
    let gw = g.size().x;
    p.galley(pos2(x, bar.center().y - g.size().y / 2.0), g, t.text);
    // Caret.
    let blink = (ui.input(|i| i.time) * 2.0).floor() as i64 % 2 == 0;
    if blink {
        p.line_segment([pos2(x + gw + 1.0, bar.top() + 5.0), pos2(x + gw + 1.0, bar.bottom() - 5.0)], Stroke::new(1.0, t.text));
    }
    // History chevron.
    crate::icons::paint(
        &p,
        Rect::from_center_size(pos2(bar.right() - 14.0, bar.center().y), vec2(14.0, 14.0)),
        if app.cmd.expanded { crate::icons::Icon::ChevronDown } else { crate::icons::Icon::ChevronUp },
        false,
    );
    // AutoComplete list.
    if !app.session.prompting() {
        let sug = suggestions(&app.cmd.buffer);
        if !sug.is_empty() {
            let rh = 20.0;
            let lr = Rect::from_min_size(
                pos2(
                    bar.left() + 24.0,
                    bar.top() - 6.0 - rh * sug.len() as f32 - (if lines.is_empty() { 0.0 } else { 16.0 * lines.len() as f32 + 6.0 }),
                ),
                vec2((bar.width() - 24.0).max(0.0), rh * sug.len() as f32),
            );
            p.rect_filled(lr, 3.0, t.list_bg);
            p.rect_stroke(lr, 3.0, Stroke::new(1.0, t.cmd_border), egui::StrokeKind::Inside);
            for (i, s) in sug.iter().enumerate() {
                let r = Rect::from_min_size(pos2(lr.left(), lr.top() + rh * i as f32), vec2(lr.width(), rh));
                let resp = ui.interact(r, ui.id().with(("sug", i)), Sense::click());
                if resp.hovered() || i == 0 {
                    p.rect_filled(r.shrink(1.0), 2.0, if resp.hovered() { t.cmd_keyword_hover } else { t.list_row });
                }
                p.text(Pos2::new(r.left() + 8.0, r.center().y), egui::Align2::LEFT_CENTER, &s.display, crate::theme::body(), t.text);
                p.text(
                    Pos2::new(r.right() - 8.0, r.center().y),
                    egui::Align2::RIGHT_CENTER,
                    crate::i18n::t(s.label),
                    crate::theme::small(),
                    t.text_faint,
                );
                if resp.clicked() {
                    app.cmd.buffer.clear();
                    app.start(&s.id.to_ascii_lowercase());
                }
            }
        }
    }
}

/// The command line's areas, laid out like [`show`] draws them.
struct Zones {
    /// The inline history lines (when there are any and the window is closed).
    lines: Option<Rect>,
    /// The expanded history window.
    window: Option<Rect>,
    chevron: Rect,
    /// The top edge that resizes the inline history.
    top_grip: Rect,
    /// The bar's left and right edges, which resize its width.
    left_grip: Rect,
    right_grip: Rect,
    /// Everything, AutoComplete list included: kept away from the drawing underneath.
    all: Rect,
}

fn zones(app: &CadApp, canvas: Rect, bar: Rect) -> Zones {
    let n = if app.cmd.expanded { 0 } else { visible_lines(app, canvas, bar).min(app.session.log.len()) };
    let lines =
        (n > 0).then(|| Rect::from_min_size(pos2(bar.left(), bar.top() - LINE_H * n as f32 - 4.0), vec2(bar.width(), LINE_H * n as f32 + 2.0)));
    let window = app.cmd.expanded.then(|| {
        // As tall as the log (at least three lines), at most half the drawing area.
        let full = LINE_H * app.session.log.len().max(3) as f32 + 4.0;
        let h = full.min((canvas.height() * 0.5).max(3.0 * LINE_H));
        Rect::from_min_size(pos2(bar.left(), bar.top() - h - 4.0), vec2(bar.width(), h + 2.0))
    });
    let top = window.or(lines).map_or(bar.top(), |r| r.top());
    let top_grip = Rect::from_min_max(pos2(bar.left(), top - 3.0), pos2(bar.right(), top + 3.0));
    // The side grips span the bar and whatever sits above it, so they never overlap its rows.
    let side = |left: bool| {
        let x = if left { bar.left() - 3.0 } else { bar.right() };
        Rect::from_min_max(pos2(x, top), pos2(x + 3.0, bar.bottom()))
    };
    let (left_grip, right_grip) = (side(true), side(false));
    let mut all = bar.union(top_grip).union(left_grip).union(right_grip);
    if !app.session.prompting() {
        let k = suggestions(&app.cmd.buffer).len();
        if k > 0 {
            let above = if n == 0 { 0.0 } else { LINE_H * n as f32 + 6.0 };
            all = all.union(Rect::from_min_size(pos2(bar.left(), bar.top() - 6.0 - 20.0 * k as f32 - above), vec2(bar.width(), 1.0)));
        }
    }
    Zones {
        lines,
        window,
        chevron: Rect::from_center_size(pos2(bar.right() - 14.0, bar.center().y), vec2(24.0, 22.0)),
        top_grip,
        left_grip,
        right_grip,
        all,
    }
}

/// The pointer on the command line: one widget under all of it keeps clicks, drags and the wheel
/// off the drawing (the keywords and AutoComplete rows sit on top of it). Clicking the history
/// lines or the chevron opens/closes the history window (`ui.toggle.history`, also F2); dragging
/// the top edge sets how many lines show (`ui.cmdline.lines`) and dragging a side edge sets how
/// wide the bar is (`ui.cmdline.width`).
fn history(app: &mut CadApp, ui: &mut egui::Ui, canvas: Rect, bar: Rect) {
    let z = zones(app, canvas, bar);
    let resp = ui.interact(z.all, ui.id().with("cmdline"), Sense::click_and_drag());
    let at = resp.hover_pos().or(resp.interact_pointer_pos());
    let on_grip = |p: Pos2| z.top_grip.contains(p) && z.window.is_none();
    // A side edge: only the grips themselves, so a click on the bar's own line still opens or
    // closes the history window as it did before.
    let on_side = |p: Pos2| (z.left_grip.contains(p) || z.right_grip.contains(p)) && z.window.is_none();
    let on_toggle = |p: Pos2| z.chevron.contains(p) || z.lines.is_some_and(|r| r.contains(p));
    if app.cmd.resize_from.is_some_and(|r| matches!(r, ResizeFrom::Width(..))) {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    } else if app.cmd.resize_from.is_some() || at.is_some_and(on_grip) {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
    } else if at.is_some_and(on_side) {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    } else if at.is_some_and(on_toggle) {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if resp.drag_started()
        && let Some(o) = ui.input(|i| i.pointer.press_origin())
    {
        if on_grip(o) {
            app.cmd.resize_from = Some(ResizeFrom::Lines(o.y, visible_lines(app, canvas, bar)));
        } else if on_side(o) {
            let middle = bar.center().x;
            app.cmd.resize_from = Some(ResizeFrom::Width(o.x, bar.width(), o.x >= middle));
        }
    }
    if let Some(from) = app.cmd.resize_from {
        if let (true, Some(p)) = (resp.dragged(), resp.interact_pointer_pos()) {
            match from {
                ResizeFrom::Lines(y0, n0) => {
                    let n = (n0 as f32 + (y0 - p.y) / LINE_H).round().clamp(0.0, MAX_LINES as f32) as usize;
                    if n != app.ui.history_lines {
                        let _ = app.run("ui.cmdline.lines", serde_json::json!({ "lines": n }));
                    }
                }
                // The dragged edge follows the pointer; the other one stays where it is.
                ResizeFrom::Width(x0, w0, right) => {
                    let w = if right { w0 + (p.x - x0) } else { w0 - (p.x - x0) };
                    let _ = app.run("ui.cmdline.width", serde_json::json!({ "width": w }));
                }
            }
        } else {
            app.cmd.resize_from = None;
        }
    }
    if resp.clicked() && resp.interact_pointer_pos().is_some_and(on_toggle) {
        let _ = app.run("ui.toggle.history", serde_json::json!({}));
    }
    crate::shortcut_menus::attach(app, &resp, crate::shortcut_menus::command_line_entries);
    if let Some(r) = zones(app, canvas, bar).window {
        history_window(app, ui, r);
    }
}

/// The expanded history: the whole command log, scrollable, newest at the bottom.
fn history_window(app: &CadApp, ui: &mut egui::Ui, r: Rect) {
    let t = Tokens::get();
    ui.painter().rect_filled(r, 3.0, t.cmd_bg);
    ui.painter().rect_stroke(r, 3.0, Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
    let inner = r.shrink2(vec2(8.0, 3.0));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(egui::Layout::top_down(egui::Align::Min)));
    child.spacing_mut().item_spacing.y = 0.0;
    let log = &app.session.log;
    egui::ScrollArea::vertical().id_salt("cmd_history").stick_to_bottom(true).auto_shrink([false, false]).show_rows(
        &mut child,
        LINE_H,
        log.len(),
        |ui, rows| {
            for l in log.get(rows).into_iter().flatten() {
                let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), LINE_H), Sense::hover());
                let p = ui.painter().with_clip_rect(row.intersect(ui.clip_rect()));
                p.text(pos2(row.left(), row.center().y), egui::Align2::LEFT_CENTER, l, crate::theme::small(), t.text_dim);
            }
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadcraft_engine::Session;
    use egui::{Event, PointerButton};

    #[test]
    fn compact_canvas_contains_history_controls_without_changing_line_preference() {
        let mut app = CadApp::new(Session::empty(), crate::Services::default());
        app.ui.history_lines = 12;
        for i in 0..20 {
            app.session.echo(format!("line {i}"));
        }
        let canvas = Rect::from_min_size(Pos2::ZERO, vec2(300.0, 180.0));
        let w = bar_width(&app, canvas);
        let bar = Rect::from_min_size(pos2(canvas.center().x - w / 2.0, canvas.bottom() - 34.0), vec2(w, 24.0));
        let z = zones(&app, canvas, bar);
        assert!(canvas.contains_rect(bar));
        assert!(canvas.contains_rect(z.chevron));
        assert!(z.lines.is_some_and(|r| canvas.contains_rect(r)));
        assert!(canvas.contains_rect(z.top_grip));
        // On a canvas this narrow the bar is as wide as it fits, and its grips are inside it too.
        assert_eq!(w, canvas.width() - BAR_MARGIN);
        assert!(canvas.contains_rect(z.left_grip));
        assert!(canvas.contains_rect(z.right_grip));
        assert_eq!(app.ui.history_lines, 12);
    }

    fn frame(app: &mut CadApp, ctx: &egui::Context, events: Vec<Event>) {
        let raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1400.0, 900.0))), events, ..Default::default() };
        let mut out = ctx.run_ui(raw, |ui| {
            app.logic(ui.ctx());
            app.ui(ui);
        });
        out.textures_delta.clear();
    }

    fn press(app: &mut CadApp, ctx: &egui::Context, at: Pos2, pressed: bool) {
        frame(app, ctx, vec![Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: egui::Modifiers::default() }]);
    }

    #[test]
    fn history_expands_by_click_chevron_f2_and_resizes_by_drag_without_touching_the_drawing() {
        let mut app = CadApp::new(Session::empty(), crate::Services::default());
        app.start("ui.sample");
        for i in 0..20 {
            app.session.echo(format!("line {i}"));
        }
        let ctx = egui::Context::default();
        frame(&mut app, &ctx, Vec::new());
        frame(&mut app, &ctx, Vec::new());
        let canvas = app.canvas.rect.unwrap_or(Rect::NOTHING);
        let w = (canvas.width() * 0.48).clamp(360.0, 760.0).min((canvas.width() - 8.0).max(0.0));
        let bar = Rect::from_min_size(pos2(canvas.center().x - w / 2.0, canvas.bottom() - 34.0), vec2(w, 24.0));
        let z = zones(&app, canvas, bar);
        let Some(lines) = z.lines else { panic!("three history lines show by default") };
        // A click on the history lines opens the history window, and doesn't start a selection.
        for (at, pressed) in [(lines.center(), true), (lines.center(), false)] {
            frame(&mut app, &ctx, vec![Event::PointerMoved(at)]);
            press(&mut app, &ctx, at, pressed);
        }
        frame(&mut app, &ctx, Vec::new());
        assert!(app.cmd.expanded);
        assert!(app.session.pending_window.is_none(), "the click stayed on the command line");
        // The chevron closes it; F2 and TEXTSCR open it.
        let chevron = z.chevron.center();
        press(&mut app, &ctx, chevron, true);
        press(&mut app, &ctx, chevron, false);
        frame(&mut app, &ctx, Vec::new());
        assert!(!app.cmd.expanded);
        let f2 = |pressed| Event::Key { key: Key::F2, physical_key: None, pressed, repeat: false, modifiers: egui::Modifiers::NONE };
        frame(&mut app, &ctx, vec![f2(true), f2(false)]);
        assert!(app.cmd.expanded);
        assert_eq!(app.run("ui.toggle.history", serde_json::json!({"on": false})), Ok(serde_json::json!({"on": false})));
        app.cmdline("textscr");
        assert!(app.cmd.expanded);
        let _ = app.run("ui.toggle.history", serde_json::json!({"on": false}));
        frame(&mut app, &ctx, Vec::new());
        // Dragging the top edge up by two lines shows two more lines (capped).
        let top = pos2(lines.center().x, lines.top() - 1.0);
        frame(&mut app, &ctx, vec![Event::PointerMoved(top)]);
        press(&mut app, &ctx, top, true);
        for dy in [8.0, 20.0, 2.0 * LINE_H] {
            frame(&mut app, &ctx, vec![Event::PointerMoved(top - vec2(0.0, dy))]);
        }
        press(&mut app, &ctx, top - vec2(0.0, 2.0 * LINE_H), false);
        frame(&mut app, &ctx, Vec::new());
        assert_eq!(app.ui.history_lines, 5);
        assert!(!app.cmd.expanded, "a drag isn't a click");
        assert!(app.session.pending_window.is_none());
        assert_eq!(app.run("ui.cmdline.lines", serde_json::json!({"lines": 99})), Ok(serde_json::json!({"lines": MAX_LINES})));
    }

    #[test]
    fn dragging_a_side_edge_resizes_the_bar_and_the_other_edge_stays_put() {
        let mut app = CadApp::new(Session::empty(), crate::Services::default());
        app.start("ui.sample");
        let ctx = egui::Context::default();
        frame(&mut app, &ctx, Vec::new());
        frame(&mut app, &ctx, Vec::new());
        let canvas = app.canvas.rect.unwrap_or(Rect::NOTHING);
        let w0 = bar_width(&app, canvas);
        let bar = Rect::from_min_size(pos2(canvas.center().x - w0 / 2.0, canvas.bottom() - 34.0), vec2(w0, 24.0));
        let z = zones(&app, canvas, bar);
        let Some(right) = z.right_grip.intersect(bar).center().into() else { panic!("the bar has a right edge") };
        // Drag the right edge 120 pt to the right: the bar widens, its left edge doesn't move.
        frame(&mut app, &ctx, vec![Event::PointerMoved(right)]);
        press(&mut app, &ctx, right, true);
        for dx in [20.0, 60.0, 120.0] {
            frame(&mut app, &ctx, vec![Event::PointerMoved(right + vec2(dx, 0.0))]);
        }
        press(&mut app, &ctx, right + vec2(120.0, 0.0), false);
        frame(&mut app, &ctx, Vec::new());
        let w1 = bar_width(&app, canvas);
        assert!((w1 - (w0 + 120.0)).abs() <= 1.0, "{w0} -> {w1}");
        let bar1 = Rect::from_min_size(pos2(canvas.center().x - w1 / 2.0, canvas.bottom() - 34.0), vec2(w1, 24.0));
        assert!(bar1.left() < bar.left(), "the left edge stays where it was");
        assert_eq!(app.session.pending_window, None, "the drag stayed on the command line");
        // A left-edge drag moves the left edge and keeps the right one: width follows the pointer.
        let z1 = zones(&app, canvas, bar1);
        let Some(left) = z1.left_grip.intersect(bar1).center().into() else { panic!("the bar has a left edge") };
        frame(&mut app, &ctx, vec![Event::PointerMoved(left)]);
        press(&mut app, &ctx, left, true);
        for dx in [10.0, 40.0, 60.0] {
            frame(&mut app, &ctx, vec![Event::PointerMoved(left + vec2(dx, 0.0))]);
        }
        press(&mut app, &ctx, left + vec2(60.0, 0.0), false);
        frame(&mut app, &ctx, Vec::new());
        let w2 = bar_width(&app, canvas);
        assert!((w2 - (w1 - 60.0)).abs() <= 1.0, "{w1} -> {w2}");
        // The command line reports the width in use, clamps to the drawing area, and null resets it.
        let reported = app.run("ui.cmdline.width", serde_json::json!({})).unwrap();
        assert!((reported["width"].as_f64().unwrap_or_default() - f64::from(w2)).abs() <= 1.0, "{reported}");
        let huge = app.run("ui.cmdline.width", serde_json::json!({"width": 99999.0})).unwrap();
        assert!(huge["width"].as_f64().unwrap_or_default() <= f64::from(canvas.width() - BAR_MARGIN) + 1.0, "{huge}");
        let reset = app.run("ui.cmdline.width", serde_json::json!({"width": null})).unwrap();
        let auto = (canvas.width() * 0.48).clamp(MIN_WIDTH, 760.0);
        assert!((reset["width"].as_f64().unwrap_or_default() - f64::from(auto)).abs() <= 1.0, "{reset}");
        assert_eq!(app.ui.cmdline_width, None);
        assert!(app.run("ui.cmdline.width", serde_json::json!({"width": "wide"})).is_err());
    }

    #[test]
    fn a_saved_width_that_no_longer_fits_clamps_to_the_canvas() {
        let mut app = CadApp::new(Session::empty(), crate::Services::default());
        app.ui.cmdline_width = Some(5_000.0);
        for width in [200.0, 3_000.0] {
            let canvas = Rect::from_min_size(Pos2::ZERO, vec2(width, 400.0));
            let w = bar_width(&app, canvas);
            // A canvas narrower than MIN_WIDTH gets the whole area: the bar never runs off it.
            let fits = canvas.width() - BAR_MARGIN;
            assert!(w <= fits, "{width}: {w}");
            assert!(w >= MIN_WIDTH.min(fits), "{width}: {w}");
        }
        // A hostile saved value falls back to the default instead of a NaN bar.
        app.ui.cmdline_width = Some(f32::NAN);
        let canvas = Rect::from_min_size(Pos2::ZERO, vec2(1_200.0, 400.0));
        assert_eq!(bar_width(&app, canvas), (1_200.0_f32 * 0.48).clamp(MIN_WIDTH, 760.0));
    }
}

#[cfg(test)]
mod delete_key_tests {
    use super::*;
    use cadcraft_doc::Handle;

    const PRESS: KeyPress = KeyPress { repeat: false, plain: true };

    #[test]
    fn mac_delete_erases_a_fresh_selection() {
        assert!(backspace_erases(true, PRESS, "", false, true, false));
        // Not on Windows/Linux, where Backspace never deletes objects.
        assert!(!backspace_erases(false, PRESS, "", false, true, false));
        // Nothing selected, a command running, or text on the command line: just edit text.
        assert!(!backspace_erases(true, PRESS, "", false, false, false));
        assert!(!backspace_erases(true, PRESS, "", true, true, false));
        assert!(!backspace_erases(true, PRESS, "m", false, true, false));
        // Option/Cmd+delete are text-editing gestures on macOS.
        assert!(!backspace_erases(true, KeyPress { plain: false, ..PRESS }, "", false, true, false));
    }

    #[test]
    fn holding_delete_to_clear_typing_never_erases() {
        // Auto-repeat after the last character is gone.
        assert!(!backspace_erases(true, KeyPress { repeat: true, ..PRESS }, "", false, true, false));
        // Typed, then cleared with separate presses: still not erased.
        assert!(!backspace_erases(true, PRESS, "", false, true, true));
    }

    #[test]
    fn typing_is_forgotten_when_the_selection_changes() {
        let mut c = CmdLine::default();
        c.watch_selection(&[Handle(1)]);
        c.buffer.push('m');
        c.note_typing();
        c.buffer.clear();
        c.watch_selection(&[Handle(1)]);
        assert!(c.typed_since_selection);
        c.watch_selection(&[Handle(1), Handle(2)]);
        assert!(!c.typed_since_selection);
    }
}

#[cfg(test)]
mod autocomplete_tests {
    use super::*;

    #[test]
    fn exact_alias_is_first_suggestion_with_alias_display() {
        let rec = suggestions("rec");
        assert!(!rec.is_empty());
        assert_eq!(rec[0].id, "RECTANG");
        assert_eq!(rec[0].display, "REC (RECTANG)");
        assert_eq!(rec[0].label, "Rectangle");
        // RECOVER should follow RECTANG, not precede it
        assert!(rec.iter().any(|s| s.id == "RECOVER"));
        assert_eq!(rec[1].id, "RECOVER");
        assert_eq!(rec[1].display, "RECOVER");

        let l = suggestions("l");
        assert_eq!(l[0].id, "LINE");
        assert_eq!(l[0].display, "L (LINE)");

        let c = suggestions("c");
        assert_eq!(c[0].id, "CIRCLE");
        assert_eq!(c[0].display, "C (CIRCLE)");

        let co = suggestions("co");
        assert_eq!(co[0].id, "COPY");
        assert_eq!(co[0].display, "CO (COPY)");
    }

    #[test]
    fn prefix_without_alias_shows_command_id() {
        let rect = suggestions("rect");
        assert_eq!(rect[0].id, "RECTANG");
        assert_eq!(rect[0].display, "RECTANG");

        let line = suggestions("line");
        assert_eq!(line[0].id, "LINE");
        assert_eq!(line[0].display, "LINE");
    }

    #[test]
    fn tab_autocompletes_alias_to_resolved_command() {
        let mut app = CadApp::new(cadcraft_engine::Session::empty(), crate::Services::default());
        app.cmd.buffer = "rec".to_string();
        tab(&mut app);
        assert_eq!(app.cmd.buffer, "RECTANG");
    }
}

#[cfg(test)]
mod keyword_layout_tests {
    use super::*;

    /// PURGE's own list, as it reaches the bar.
    fn purge() -> Vec<String> {
        [
            "Blocks",
            "DEtailviewstyles",
            "Dimstyles",
            "Groups",
            "LAyers",
            "LTypes",
            "MAterials",
            "MUltileaderstyles",
            "Plotstyles",
            "SHapes",
            "textSTyles",
            "Mlinestyles",
            "SEctionviewstyles",
            "Tablestyles",
            "Visualstyles",
            "Regapps",
            "Zero-length geometry",
            "Empty text objects",
            "Orphaned data",
            "All",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect()
    }

    /// Every keyword is 40 pt, the separator 8: 110 pt fits two per line.
    fn widths(n: usize) -> Vec<f32> {
        vec![40.0; n]
    }

    #[test]
    fn a_short_prompt_stays_on_the_bar() {
        let kws: Vec<String> = ["Close", "Undo"].iter().map(|s| (*s).to_string()).collect();
        let l = keyword_layout(&kws, &widths(2), 8.0, 8.0, 400.0, 300.0);
        assert_eq!(l.inline, 2);
        assert!(l.above.is_empty());
        assert_eq!(l.hidden, 0);
    }

    #[test]
    fn purge_puts_the_rest_on_the_lines_above_and_hides_nothing() {
        let kws = purge();
        // The bar has 110 pt for keywords, so two 40 pt ones fit; 110 pt per line above fits
        // two each.
        let l = keyword_layout(&kws, &widths(kws.len()), 8.0, 8.0, 110.0, 110.0);
        assert_eq!(l.inline, 2, "Blocks / DEtailviewstyles");
        assert_eq!(l.above.len(), MAX_KEYWORD_LINES);
        let shown: usize = l.above.iter().map(Vec::len).sum();
        assert_eq!(shown, MAX_KEYWORD_LINES * 2, "two per line");
        assert_eq!(l.hidden, kws.len() - l.inline - shown);
        // The order is the prompt's own, left to right and top to bottom.
        let flat: Vec<&String> = l.above.iter().flatten().collect();
        assert_eq!(flat.first().map(|s| s.as_str()), Some("Dimstyles"));
        assert_eq!(flat.last().map(|s| s.as_str()), Some("Tablestyles"));
    }

    #[test]
    fn nothing_is_hidden_when_every_keyword_fits_above() {
        let kws = purge();
        // Roomy: the whole list fits on the bar and nothing needs the lines above.
        let l = keyword_layout(&kws, &widths(kws.len()), 8.0, 8.0, 2000.0, 2000.0);
        assert_eq!(l.inline, kws.len());
        assert!(l.above.is_empty());
        assert_eq!(l.hidden, 0);
    }

    #[test]
    fn hidden_counts_what_no_line_can_hold() {
        let kws = purge();
        // Two per line and six lines at most: 2 inline + 12 above = 14 of 20.
        let l = keyword_layout(&kws, &widths(kws.len()), 8.0, 8.0, 110.0, 110.0);
        assert_eq!(l.inline + l.above.iter().map(Vec::len).sum::<usize>() + l.hidden, kws.len());
        assert!(l.hidden > 0);
        // A line too narrow for two keywords still takes one (it is clipped, as on the bar).
        let l = keyword_layout(&kws, &widths(kws.len()), 8.0, 8.0, 110.0, 10.0);
        assert_eq!(l.inline, 2);
        assert_eq!(l.above.iter().map(Vec::len).sum::<usize>(), MAX_KEYWORD_LINES, "one per line");
        assert_eq!(l.hidden, kws.len() - 2 - MAX_KEYWORD_LINES);
    }

    #[test]
    fn an_empty_keyword_list_lays_out_nothing() {
        let l = keyword_layout(&[], &[], 8.0, 8.0, 200.0, 110.0);
        assert_eq!(l.inline, 0);
        assert!(l.above.is_empty());
        assert_eq!(l.hidden, 0);
    }

    #[test]
    fn a_keyword_wider_than_the_bar_still_draws_inline() {
        let kws: Vec<String> = ["VeryLongKeywordIndeed", "Short"].iter().map(|s| (*s).to_string()).collect();
        let widths = vec![900.0, 40.0];
        // No room at all: the first is drawn inline anyway (the bar clips it), the rest above.
        let l = keyword_layout(&kws, &widths, 8.0, 8.0, 100.0, 500.0);
        assert_eq!(l.inline, 1);
        assert_eq!(l.above, vec![vec!["Short".to_string()]]);
        assert_eq!(l.hidden, 0);
    }
}
