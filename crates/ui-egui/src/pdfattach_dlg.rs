//! The PDF Underlay dialog (PDFATTACH from a menu, the tool bar or the command line): the file,
//! its pages as thumbnails to pick one or several, how the path is stored, and the insertion
//! point, scale (1 = the page at its size on paper) and rotation, each given here or specified
//! on screen afterwards. Calls with parameters never open it.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use cadcraft_engine::cmd::underlay::{AttachPreset, pdf, read_pdf, start_attach, stored_path, underlay_file};
use cadcraft_geom::Vec2;
use egui::{TextureHandle, TextureOptions, vec2};

use crate::CadApp;

/// Thumbnails: longest side in pixels, and most pages shown.
const THUMB_PX: u32 = 160;
const MAX_THUMBS: u32 = 60;

/// The dialog's state between frames (egui temporary data).
#[derive(Clone)]
struct State {
    path: String,
    /// The file's bytes, read once for the page count and every thumbnail (empty if unreadable).
    bytes: Arc<Vec<u8>>,
    pages: Result<usize, String>,
    selected: BTreeSet<u32>,
    thumbs: BTreeMap<u32, Option<TextureHandle>>,
    path_type: &'static str,
    at_on_screen: bool,
    at: [f64; 2],
    scale_on_screen: bool,
    scale: f64,
    rotation_on_screen: bool,
    rotation: f64,
}

impl State {
    fn new(path: String) -> State {
        let read = pdf().ok_or_else(|| "PDF underlays are not available in this build".to_string()).and_then(|hooks| {
            let bytes = read_pdf(&path).map_err(|e| e.to_string())?;
            let pages = (hooks.page)(&bytes, 1).map(|(n, ..)| n).map_err(|e| format!("{path}: {e}"))?;
            Ok((bytes, pages))
        });
        let (bytes, pages) = match read {
            Ok((bytes, n)) => (bytes, Ok(n)),
            Err(e) => (Vec::new(), Err(e)),
        };
        State {
            path,
            bytes: Arc::new(bytes),
            pages,
            selected: BTreeSet::from([1]),
            thumbs: BTreeMap::new(),
            path_type: "full",
            at_on_screen: true,
            at: [0.0, 0.0],
            scale_on_screen: true,
            scale: 1.0,
            rotation_on_screen: false,
            rotation: 0.0,
        }
    }
}

fn id() -> egui::Id {
    egui::Id::unique("cc_pdfattach_dialog")
}

/// The dialog, opened as `pdfattach:<path>` on the PDF the user chose.
pub fn dialog(app: &mut CadApp, ctx: &egui::Context, chosen: &str, open: &mut bool) {
    // The state kept from the last frame, if it is this file's (a new PDFATTACH starts afresh).
    let mut st = ctx.data(|d| d.get_temp::<State>(id())).filter(|s| s.path == chosen).unwrap_or_else(|| State::new(chosen.to_string()));
    let document = app.session.state().ok().and_then(|s| s.path.clone());
    // One thumbnail drawn per frame, so the dialog shows at once and fills in.
    if let Ok(n) = st.pages {
        let missing = (1..=(n as u32).min(MAX_THUMBS)).find(|p| !st.thumbs.contains_key(p));
        if let Some(p) = missing {
            let tex = pdf().and_then(|hooks| (hooks.render)(&st.bytes, p, THUMB_PX).ok()).and_then(|(w, h, mut rgba)| {
                // On white: the page's paper is transparent.
                for [r, g, b, a] in rgba.as_chunks_mut::<4>().0 {
                    let k = f32::from(*a) / 255.0;
                    for c in [r, g, b] {
                        *c = (f32::from(*c) * k + 255.0 * (1.0 - k)).round() as u8;
                    }
                    *a = 255;
                }
                let size = [w as usize, h as usize];
                (size[0].checked_mul(size[1]).and_then(|n| n.checked_mul(4)) == Some(rgba.len())).then(|| {
                    ctx.load_texture(format!("pdf-thumb {p}"), egui::ColorImage::from_rgba_unmultiplied(size, &rgba), TextureOptions::LINEAR)
                })
            });
            st.thumbs.insert(p, tex);
            ctx.request_repaint();
        }
    }
    let (mut ok, mut cancel, mut browse) = (false, false, false);
    egui::Window::new(crate::tl!("Attach PDF Underlay")).id(id()).open(open).collapsible(false).resizable(false).show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.label(crate::tl!("File"));
            let name = std::path::Path::new(&st.path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            ui.strong(name);
            browse = ui.button(crate::tl!("Browse...")).clicked();
        });
        match &st.pages {
            Err(e) => {
                ui.colored_label(ui.visuals().error_fg_color, e);
            }
            Ok(n) => {
                ui.label(crate::tf!("Pages to attach ({count} in the file)", count = n));
                egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        for p in 1..=(*n as u32).min(MAX_THUMBS) {
                            let on = st.selected.contains(&p);
                            let resp = ui
                                .vertical(|ui| {
                                    let frame = egui::Frame::new()
                                        .stroke(egui::Stroke::new(
                                            if on { 3.0 } else { 1.0 },
                                            if on { ui.visuals().selection.bg_fill } else { ui.visuals().widgets.noninteractive.bg_stroke.color },
                                        ))
                                        .inner_margin(2.0);
                                    frame
                                        .show(ui, |ui| match st.thumbs.get(&p) {
                                            Some(Some(t)) => {
                                                let size = t.size_vec2();
                                                let k = 120.0 / size.x.max(size.y).max(1.0);
                                                ui.add(egui::Image::new((t.id(), size * k)).sense(egui::Sense::click()))
                                            }
                                            _ => ui.add_sized(vec2(120.0, 85.0), egui::Label::new("…").sense(egui::Sense::click())),
                                        })
                                        .inner
                                        .union(ui.label(p.to_string()))
                                })
                                .inner;
                            if resp.clicked() {
                                // Ctrl or Shift adds to the pages chosen; a plain click picks one.
                                if ui.input(|i| i.modifiers.command || i.modifiers.shift) {
                                    if !st.selected.remove(&p) {
                                        st.selected.insert(p);
                                    }
                                } else {
                                    st.selected = BTreeSet::from([p]);
                                }
                            }
                        }
                    });
                });
                if *n as u32 > MAX_THUMBS {
                    ui.label(crate::tf!("Only the first {count} pages are shown.", count = MAX_THUMBS));
                }
            }
        }
        ui.separator();
        ui.label(crate::tl!("Path type"));
        ui.horizontal(|ui| {
            let saved = document.is_some();
            ui.radio_value(&mut st.path_type, "full", crate::tl!("Full path"));
            ui.add_enabled_ui(saved, |ui| {
                ui.radio_value(&mut st.path_type, "relative", crate::tl!("Relative path"));
                ui.radio_value(&mut st.path_type, "none", crate::tl!("File name only"));
            })
            .response
            .on_disabled_hover_text(crate::tl!("Save the drawing first"));
        });
        ui.separator();
        egui::Grid::new("pdfattach-place").num_columns(3).show(ui, |ui| {
            ui.label(crate::tl!("Insertion point"));
            ui.checkbox(&mut st.at_on_screen, crate::tl!("Specify on screen"));
            ui.add_enabled_ui(!st.at_on_screen, |ui| {
                ui.horizontal(|ui| {
                    ui.label("X");
                    ui.add(egui::DragValue::new(&mut st.at[0]).speed(1.0));
                    ui.label("Y");
                    ui.add(egui::DragValue::new(&mut st.at[1]).speed(1.0));
                });
            });
            ui.end_row();
            ui.label(crate::tl!("Scale (1 = size on paper)"));
            ui.checkbox(&mut st.scale_on_screen, crate::tl!("Specify on screen"));
            ui.add_enabled_ui(!st.scale_on_screen, |ui| ui.add(egui::DragValue::new(&mut st.scale).speed(0.01).range(1e-6..=1e6)));
            ui.end_row();
            ui.label(crate::tl!("Rotation"));
            ui.checkbox(&mut st.rotation_on_screen, crate::tl!("Specify on screen"));
            ui.add_enabled_ui(!st.rotation_on_screen, |ui| ui.add(egui::DragValue::new(&mut st.rotation).speed(1.0).suffix("°")));
            ui.end_row();
        });
        ui.collapsing(crate::tl!("Details"), |ui| {
            let found = underlay_file(&st.path, None).map(|p| p.to_string_lossy().to_string()).unwrap_or_else(|e| e.to_string());
            ui.label(crate::tf!("Found at: {path}", path = found));
            let stored = stored_path(&st.path, document.as_deref(), st.path_type).unwrap_or_else(|e| e);
            ui.label(crate::tf!("Saved in the drawing as: {path}", path = stored));
        });
        ui.separator();
        ui.horizontal(|ui| {
            let can = st.pages.is_ok() && !st.selected.is_empty();
            ok = ui.add_enabled(can, egui::Button::new(crate::tl!("OK"))).clicked();
            cancel = ui.button(crate::tl!("Cancel")).clicked();
        });
    });
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        cancel = true;
    }
    if browse
        && let Some(pick) = app.services.pick_pdf.as_ref()
        && let Some(path) = pick()
    {
        // The dialog now shows that file (its state is kept under the new file's name).
        app.ui.dialog = Some(format!("pdfattach:{path}"));
        st = State::new(path);
    }
    if ok {
        let preset = AttachPreset {
            path: st.path.clone(),
            pages: st.selected.iter().copied().collect(),
            at: (!st.at_on_screen).then(|| Vec2::new(st.at[0], st.at[1])),
            scale: (!st.scale_on_screen).then_some(st.scale),
            rotation: (!st.rotation_on_screen).then_some(st.rotation),
            path_type: st.path_type.to_string(),
        };
        if let Err(e) = start_attach(&mut app.session, preset) {
            app.session.echo(e.to_string());
        }
    }
    // Closed by OK, Cancel, Escape or the title bar's close button: its state goes with it.
    if ok || cancel || !*open {
        *open = false;
        ctx.data_mut(|d| d.remove::<State>(id()));
    } else {
        ctx.data_mut(|d| d.insert_temp(id(), st));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Services;
    use cadcraft_engine::Session;

    /// One frame of the dialog on `chosen`, `open` as the window's close button leaves it; the
    /// file kept in its state afterwards.
    fn frame(app: &mut CadApp, ctx: &egui::Context, chosen: &str, mut open: bool) -> Option<String> {
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| dialog(app, ui.ctx(), chosen, &mut open));
        output.textures_delta.clear();
        ctx.data(|d| d.get_temp::<State>(id())).map(|s| s.path)
    }

    #[test]
    fn a_closed_dialog_forgets_its_file_and_a_new_pick_starts_afresh() {
        let mut app = CadApp::new(Session::new(), Services::default());
        let ctx = egui::Context::default();
        assert_eq!(frame(&mut app, &ctx, "/plans/a.pdf", true).as_deref(), Some("/plans/a.pdf"));
        // Another PDFATTACH on another file while a's state is kept: b is shown.
        assert_eq!(frame(&mut app, &ctx, "/plans/b.pdf", true).as_deref(), Some("/plans/b.pdf"));
        // The title bar's close button: nothing kept for the next PDFATTACH.
        assert_eq!(frame(&mut app, &ctx, "/plans/b.pdf", false), None);
    }
}
