//! The floating command line: history, the active prompt with clickable keywords, typed input
//! (keyboard focus stays on the drawing — typing anywhere goes here) and AutoComplete.

use cadcraft_engine::Input;
use egui::{Key, Pos2, Rect, Sense, Stroke, pos2, vec2};

use crate::CadApp;
use crate::theme::Tokens;

#[derive(Default)]
pub struct CmdLine {
    pub buffer: String,
    pub history: Vec<String>,
    pub history_pos: Option<usize>,
    pub expanded: bool,
}

/// Matching command names/aliases for AutoComplete.
fn suggestions(prefix: &str) -> Vec<(String, &'static str)> {
    if prefix.is_empty() || prefix.contains(' ') {
        return Vec::new();
    }
    let p = prefix.to_ascii_lowercase();
    let mut v: Vec<(String, &'static str)> = cadcraft_engine::command_specs()
        .iter()
        .filter(|c| !c.id.contains('.') && (c.id.starts_with(&p) || c.aliases.iter().any(|a| *a == p)))
        .map(|c| (c.id.to_ascii_uppercase(), c.label))
        .collect();
    v.sort_by_key(|(id, _)| (id.len(), id.clone()));
    v.truncate(8);
    v
}

/// Handle keyboard input destined for the command line (when no text field has focus).
pub fn keyboard(app: &mut CadApp, ctx: &egui::Context) {
    if ctx.egui_wants_keyboard_input() {
        return;
    }
    let events = ctx.input(|i| i.events.clone());
    // A hot grip takes Space/Enter (cycle mode, or apply a typed point) and Escape.
    if app.canvas.hot_grip.is_some() {
        for ev in &events {
            match ev {
                egui::Event::Text(t) if t != " " => app.cmd.buffer.push_str(t),
                egui::Event::Key { key: Key::Backspace, pressed: true, .. } => {
                    app.cmd.buffer.pop();
                }
                egui::Event::Key { key: Key::Escape, pressed: true, .. } => {
                    app.canvas.hot_grip = None;
                    app.cmd.buffer.clear();
                    app.session.echo("*Cancel*");
                }
                egui::Event::Key { key: Key::Enter | Key::Space, pressed: true, .. } => {
                    let typed = std::mem::take(&mut app.cmd.buffer);
                    if typed.trim().is_empty() {
                        if let Some(g) = app.canvas.hot_grip.as_mut() {
                            g.next_mode();
                            let l = g.label();
                            app.session.echo(l);
                        }
                    } else {
                        let base = app.canvas.hot_grip.map(|g| g.base).unwrap_or_default();
                        match cadcraft_engine::prompt::parse_point(&typed, base) {
                            Some(p) => crate::canvas::apply_hot_grip(app, p),
                            None => app.session.echo("Requires a point (x,y, @dx,dy or @d<a)."),
                        }
                    }
                }
                _ => {}
            }
        }
        return;
    }
    let text_prompt = app.session.current_prompt().is_some_and(|p| p.accept.text && !p.accept.point && !p.accept.number);
    for ev in events {
        match ev {
            egui::Event::Text(t) => {
                if t == " " && !text_prompt {
                    submit(app);
                } else {
                    app.cmd.buffer.push_str(&t);
                }
            }
            egui::Event::Key { key, pressed: true, modifiers, .. } => match key {
                Key::Enter => submit(app),
                Key::Escape => {
                    app.cmd.buffer.clear();
                    app.session.cancel();
                }
                Key::Backspace => {
                    app.cmd.buffer.pop();
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
                    if let Some(p) = app.cmd.history_pos {
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
                Key::Tab => {
                    if let Some((id, _)) = suggestions(&app.cmd.buffer).first() {
                        app.cmd.buffer = id.clone();
                    }
                }
                Key::Delete if app.cmd.buffer.is_empty() && app.session.running.is_none() => {
                    let _ = app.run("erase.selection", serde_json::json!({}));
                }
                _ => {}
            },
            _ => {}
        }
    }
}

pub fn submit(app: &mut CadApp) {
    let text = std::mem::take(&mut app.cmd.buffer);
    app.cmd.history_pos = None;
    if !text.trim().is_empty() && app.session.running.is_none() {
        app.cmd.history.push(text.clone());
        if app.cmd.history.len() > 200 {
            app.cmd.history.remove(0);
        }
    }
    if text.is_empty() {
        if let Err(e) = app.session.input(Input::Enter) {
            app.session.echo(e.to_string());
        }
        return;
    }
    app.cmdline(&text);
}

pub fn show(app: &mut CadApp, ui: &mut egui::Ui, canvas: Rect) {
    let t = Tokens::get();
    keyboard(app, ui.ctx());
    let w = (canvas.width() * 0.48).clamp(360.0, 760.0);
    let h = 24.0;
    let bar = Rect::from_min_size(pos2(canvas.center().x - w / 2.0, canvas.bottom() - h - 10.0), vec2(w, h));
    let p = ui.painter_at(canvas);
    // History lines above the bar.
    let n = app.ui.history_lines.min(12);
    let lines: Vec<String> = app.session.log.iter().rev().take(n).rev().cloned().collect();
    if !lines.is_empty() {
        let lh = 16.0;
        let hr = Rect::from_min_size(pos2(bar.left(), bar.top() - lh * lines.len() as f32 - 4.0), vec2(w, lh * lines.len() as f32 + 2.0));
        p.rect_filled(hr, 3.0, t.cmd_history);
        for (i, l) in lines.iter().enumerate() {
            p.text(pos2(hr.left() + 8.0, hr.top() + 1.0 + lh * i as f32), egui::Align2::LEFT_TOP, l, crate::theme::small(), t.text_dim);
        }
    }
    p.rect_filled(bar, 3.0, t.cmd_bg);
    p.rect_stroke(bar, 3.0, Stroke::new(1.0, t.cmd_border), egui::StrokeKind::Inside);
    // Prompt glyph.
    p.text(pos2(bar.left() + 8.0, bar.center().y), egui::Align2::LEFT_CENTER, ">_", crate::theme::mono(), t.text_dim);
    let mut x = bar.left() + 30.0;
    let prompt = app.session.current_prompt();
    let font = crate::theme::body();
    if let Some(g) = app.canvas.hot_grip {
        let gl = p.layout_no_wrap(g.label().to_string(), font.clone(), t.text);
        let gw = gl.size().x;
        p.galley(pos2(x, bar.center().y - gl.size().y / 2.0), gl, t.text);
        x += gw + 6.0;
    } else {
        match &prompt {
            Some(pr) => {
                let name = app.session.running.as_ref().map(|r| r.id.to_ascii_uppercase()).unwrap_or_default();
                let g = p.layout_no_wrap(format!("{name} "), font.clone(), t.text_faint);
                let gw = g.size().x;
                p.galley(pos2(x, bar.center().y - g.size().y / 2.0), g, t.text_faint);
                x += gw;
                let g = p.layout_no_wrap(pr.message.clone(), font.clone(), t.text);
                let gw = g.size().x;
                p.galley(pos2(x, bar.center().y - g.size().y / 2.0), g, t.text);
                x += gw;
                if !pr.keywords.is_empty() {
                    let g = p.layout_no_wrap(if pr.message.is_empty() { " [".into() } else { " or [".into() }, font.clone(), t.text);
                    let gw = g.size().x;
                    p.galley(pos2(x, bar.center().y - g.size().y / 2.0), g, t.text);
                    x += gw;
                    let kws = pr.keywords.clone();
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
                if let Some(d) = &pr.default {
                    let g = p.layout_no_wrap(format!(" <{d}>"), font.clone(), t.text);
                    let gw = g.size().x;
                    p.galley(pos2(x, bar.center().y - g.size().y / 2.0), g, t.text);
                    x += gw;
                }
                let g = p.layout_no_wrap(": ".into(), font.clone(), t.text);
                let gw = g.size().x;
                p.galley(pos2(x, bar.center().y - g.size().y / 2.0), g, t.text);
                x += gw;
            }
            None => {
                if app.cmd.buffer.is_empty() {
                    p.text(pos2(x, bar.center().y), egui::Align2::LEFT_CENTER, "Type a command", egui::FontId::proportional(12.5), t.text_faint);
                }
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
        crate::icons::Icon::ChevronDown,
        false,
    );
    // AutoComplete list.
    if app.session.running.is_none() {
        let sug = suggestions(&app.cmd.buffer);
        if !sug.is_empty() {
            let rh = 20.0;
            let lr = Rect::from_min_size(
                pos2(
                    bar.left() + 24.0,
                    bar.top() - 6.0 - rh * sug.len() as f32 - (if lines.is_empty() { 0.0 } else { 16.0 * lines.len() as f32 + 6.0 }),
                ),
                vec2(300.0, rh * sug.len() as f32),
            );
            p.rect_filled(lr, 3.0, t.list_bg);
            p.rect_stroke(lr, 3.0, Stroke::new(1.0, t.cmd_border), egui::StrokeKind::Inside);
            for (i, (id, label)) in sug.iter().enumerate() {
                let r = Rect::from_min_size(pos2(lr.left(), lr.top() + rh * i as f32), vec2(lr.width(), rh));
                let resp = ui.interact(r, ui.id().with(("sug", i)), Sense::click());
                if resp.hovered() || i == 0 {
                    p.rect_filled(r.shrink(1.0), 2.0, if resp.hovered() { t.cmd_keyword_hover } else { t.list_row });
                }
                p.text(Pos2::new(r.left() + 8.0, r.center().y), egui::Align2::LEFT_CENTER, id, crate::theme::body(), t.text);
                p.text(Pos2::new(r.right() - 8.0, r.center().y), egui::Align2::RIGHT_CENTER, *label, crate::theme::small(), t.text_faint);
                if resp.clicked() {
                    app.cmd.buffer.clear();
                    app.start(&id.to_ascii_lowercase());
                }
            }
        }
    }
}
