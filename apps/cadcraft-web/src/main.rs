//! CADCraft in the browser.
//!
//! Runs the same [`cadcraft_ui_egui::CadApp`] as the desktop app through eframe's web runner
//! (wgpu: WebGPU where available, WebGL2 otherwise). Build with `trunk build --release` from this
//! directory (output in `dist/web`). URL flags: `?webgl` forces WebGL2, `?cpu` draws the drawing
//! with egui shapes instead of the GPU canvas, `?sample` opens the sample drawing. Saving downloads
//! the drawing through the browser (`download.rs`). With `?host=parent` (inside an `<iframe>`), a
//! same-origin parent page sends the drawing to open and stores what CADCraft saves instead
//! (`host.rs`).
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

#[cfg(target_arch = "wasm32")]
pub mod download;
#[cfg(target_arch = "wasm32")]
mod host;

#[cfg(any(target_arch = "wasm32", test))]
mod panel_layout;

#[cfg(target_arch = "wasm32")]
mod web {
    use cadcraft_engine::Session;
    use cadcraft_ui_egui::{CadApp, Services};
    use wasm_bindgen::JsCast as _;

    const CANVAS_ID: &str = "cadcraft_canvas";
    const LOADING_ID: &str = "cadcraft_loading";

    /// The app, the files File > Open… and drag and drop read for it in the background, the
    /// page embedding it (`?host=parent`), the browser's `prefers-reduced-motion` (it stays
    /// live, so View ▸ Accessibility ▸ Animation Effects ▸ Use System Setting follows a change),
    /// and the document title last written to the page.
    struct Shell(
        CadApp,
        crate::upload::Inbox,
        Option<(crate::host::Bridge, crate::host::DirtyReport)>,
        Option<web_sys::MediaQueryList>,
        Option<String>,
    );

    impl eframe::App for Shell {
        /// Runs one frame: takes in files dropped on or picked for the page, drains the background
        /// uploads, follows the browser's reduced-motion setting, runs the app, writes the browser
        /// document title when it changed (so the tab and an embedding page name the open
        /// drawing), and reports unsaved changes to that page.
        fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
            // Background reads are consume-based and run on hidden ticks too. Dropped files are
            // input and are read from `ui` only.
            self.1.drain(&mut self.0);
            self.0.system_animations = self.3.as_ref().map(|reduced| !reduced.matches());
            self.0.logic(ctx);
            let drawing = self.0.session.state().ok();
            crate::push_title(&mut self.4, drawing.map(|d| d.title.as_str()), drawing.is_some_and(|d| d.is_dirty()), set_page_title);
            // `DirtyReport` suppresses duplicates, so reporting after every logic callback
            // (including a hidden tab's) is idempotent and keeps the parent's guard current.
            if let Some((bridge, reported)) = &self.2 {
                bridge.report_dirty(&self.0, reported);
            }
        }
        /// Forward raw input to the web `CadApp` and capture Tab at the command line.
        fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
            self.0.raw_input_hook(ctx, raw);
            cadcraft_ui_egui::cmdline::capture_tab(ctx, raw);
        }
        /// Preferences and validated panel placement survive reloads; egui memory is not kept.
        fn save(&mut self, storage: &mut dyn eframe::Storage) {
            storage.set_string(cadcraft_ui_egui::options::PROFILE_KEY, self.0.profile_json());
            storage.set_string(cadcraft_ui_egui::PREFS_KEY, self.0.prefs_json());
            match crate::panel_layout::save(&self.0.ui) {
                Ok(layout) => storage.set_string(crate::panel_layout::KEY, layout),
                Err(error) => log::warn!("panel layout was not saved: {error}"),
            }
        }
        fn persist_egui_memory(&self) -> bool {
            false
        }
        /// On the first pass consume files dropped on the page, then delegate drawing to `CadApp::ui`.
        fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
            let first_pass = ui.ctx().current_pass_index() == 0;
            if first_pass {
                // `take_dropped` consumes the raw list; a second pass or hidden tick cannot
                // re-read the same file.
                self.1.take_dropped(ui.ctx());
            }
            self.0.ui(ui);
        }
    }

    /// Writes the browser's document title, which names the tab and is what an embedding page
    /// (`?host=parent`) reads. eframe's web runner ignores [`egui::ViewportCommand::Title`], so
    /// the title has to reach the DOM directly.
    fn set_page_title(title: &str) {
        match web_sys::window().and_then(|window| window.document()) {
            Some(document) => document.set_title(title),
            None => log::warn!("cannot set the page title to {title:?}: there is no document"),
        }
    }

    fn query() -> String {
        web_sys::window().and_then(|w| w.location().search().ok()).unwrap_or_default()
    }

    fn query_param(key: &str) -> Option<String> {
        web_sys::UrlSearchParams::new_with_str(&query()).ok()?.get(key)
    }

    /// No file picker on the web: Save / Save As from the menu (and the close prompt) download the
    /// drawing under its current name; `saveas {"path": "name.dxf"}` picks another name. There is
    /// no file system either, so a dropped file is opened from its contents and APPLOAD says to
    /// drop a `.lsp` file rather than asking for a name it cannot read.
    fn services() -> Services {
        Services { pick_save: Some(Box::new(|name: &str| Some(name.to_string()))), no_file_system: true, ..Services::default() }
    }

    pub fn start() {
        eframe::WebLogger::init(log::LevelFilter::Info).ok();
        cadcraft_engine::cmd::file::set_io(cadcraft_engine::cmd::file::IoHooks {
            read: |b, name| cadcraft_io::read(b, name).map_err(|e| e.to_string()),
            write: |d, name| cadcraft_io::write(d, name).map_err(|e| e.to_string()),
            plot: Some(|d, space, opts| cadcraft_io::plotter::plot(d, space, opts).map_err(|e| e.to_string())),
        });
        cadcraft_engine::cmd::file::set_write_as(|d, name, o| {
            cadcraft_io::write_with(d, name, &cadcraft_io::SaveOptions { version: o.version, binary: o.binary }).map_err(|e| e.to_string())
        });
        // AutoLISP's entget, entmake and entmod see entities as DXF groups.
        cadcraft_engine::cmd::file::set_entity_codec(cadcraft_engine::cmd::file::EntityCodec {
            groups: cadcraft_io::entity_groups,
            entity: cadcraft_io::entity_from_groups,
        });
        // `?host=parent`: a same-origin page framing CADCraft (such as a Nextcloud app) opens
        // drawings in it and stores what it saves. Otherwise there is no file system here: saved
        // files reach the user as browser downloads.
        let bridge = if query_param("host").as_deref() == Some("parent") { crate::host::Bridge::new() } else { None };
        match &bridge {
            Some(bridge) => {
                bridge.install();
                cadcraft_engine::cmd::file::set_deliver(crate::host::deliver);
            }
            None => cadcraft_engine::cmd::file::set_deliver(crate::download::download),
        }
        let app_bridge = bridge.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let Some(document) = web_sys::window().and_then(|w| w.document()) else { return };
            let Some(canvas) = document.get_element_by_id(CANVAS_ID).and_then(|e| e.dyn_into::<web_sys::HtmlCanvasElement>().ok()) else {
                log::error!("missing <canvas id=\"{CANVAS_ID}\">");
                return;
            };
            let mut options = eframe::WebOptions::default();
            if query().contains("webgl")
                && let eframe::egui_wgpu::WgpuSetup::CreateNew(create) = &mut options.wgpu_options.wgpu_setup
            {
                create.instance_descriptor.backends = eframe::wgpu::Backends::GL;
            }
            let result = eframe::WebRunner::new()
                .start(
                    canvas,
                    options,
                    Box::new(move |cc| {
                        // Embedded, the parent sends the drawing: start on the Start tab, not Drawing1.
                        let session = if app_bridge.is_some() { Session::empty() } else { Session::new() };
                        let mut app = CadApp::new(session, services());
                        app.system_languages =
                            web_sys::window().map(|w| w.navigator().languages().iter().filter_map(|v| v.as_string()).collect()).unwrap_or_default();
                        if let Some(profile) = cc.storage.and_then(|s| s.get_string(cadcraft_ui_egui::options::PROFILE_KEY)) {
                            app.load_profile(&profile);
                        }
                        if let Some(prefs) = cc.storage.and_then(|s| s.get_string(cadcraft_ui_egui::PREFS_KEY)) {
                            app.load_prefs(&prefs);
                        }
                        if let Some(layout) = cc.storage.and_then(|s| s.get_string(crate::panel_layout::KEY))
                            && let Err(error) = crate::panel_layout::restore(&mut app.ui, &layout)
                        {
                            log::warn!("panel layout was not restored: {error}");
                        }
                        // `?cpu`: no render state for the app, so the canvas draws with egui shapes.
                        if !crate::wants_cpu(&query())
                            && let Some(rs) = &cc.wgpu_render_state
                        {
                            // eframe's web runner has no stencil buffer: clipped layout
                            // viewports stay in the layout's display list.
                            app.set_wgpu(rs, None);
                        }
                        if query().contains("sample") {
                            let _ = app.run("ui.sample", serde_json::json!({}));
                        }
                        // No paths on the web: OPEN asks the browser's file picker, which reads the
                        // chosen file in the background (`upload.rs`), so the picker returns none.
                        let inbox = crate::upload::Inbox::new(&cc.egui_ctx);
                        let picker = inbox.clone();
                        app.services.pick_open = Some(Box::new(move || {
                            picker.pick();
                            None
                        }));
                        let host = app_bridge.map(|bridge| {
                            bridge.listen(inbox.clone());
                            bridge.post_ready();
                            (bridge, crate::host::DirtyReport::default())
                        });
                        let reduced_motion = web_sys::window().and_then(|w| w.match_media("(prefers-reduced-motion: reduce)").ok().flatten());
                        Ok(Box::new(Shell(app, inbox, host, reduced_motion, None)))
                    }),
                )
                .await;
            if let (Err(e), Some(bridge)) = (&result, &bridge) {
                bridge.post_failed(&format!("{e:?}"));
            }
            if let Some(el) = document.get_element_by_id(LOADING_ID) {
                match result {
                    Ok(()) => el.remove(),
                    Err(e) => {
                        el.set_inner_html(&format!("<p>CADCraft failed to start: {e:?}</p><p>A browser with WebGPU or WebGL2 is required.</p>"))
                    }
                }
            }
        });
    }
}

/// True when the URL query (`?cpu&sample`) has a `cpu` flag: the CPU canvas path is requested.
#[cfg(any(target_arch = "wasm32", test))]
fn wants_cpu(query: &str) -> bool {
    query.trim_start_matches('?').split('&').any(|p| p == "cpu" || p.starts_with("cpu="))
}

/// The browser tab title: the open drawing's file name, a bullet once it has unsaved changes,
/// and the app name (`drawing.dwg • — CADCraft`), or just the app name with no drawing. An
/// embedding page (`?host=parent`) reads it to tell which drawing opened and whether it is dirty.
#[cfg(any(target_arch = "wasm32", test))]
fn document_title(name: Option<&str>, dirty: bool) -> String {
    match name.filter(|n| !n.trim().is_empty()) {
        Some(name) => format!("{name}{} — CADCraft", if dirty { " •" } else { "" }),
        None => "CADCraft".to_string(),
    }
}

/// The title to write this frame, or `None` while the page already shows it: the browser needs
/// the document title only when it changes, not on every frame.
#[cfg(any(target_arch = "wasm32", test))]
fn title_to_send(sent: &mut Option<String>, name: Option<&str>, dirty: bool) -> Option<String> {
    let title = document_title(name, dirty);
    if sent.as_deref() == Some(title.as_str()) {
        return None;
    }
    *sent = Some(title.clone());
    Some(title)
}

/// Writes a changed title through `write` (on the web, the browser's document title) and reports
/// whether it wrote one. Split from [`title_to_send`] so the change decision and the write are
/// both testable: eframe's web backend ignores `egui::ViewportCommand::Title`, so a title that is
/// computed but not written leaves the tab on `CADCraft`.
#[cfg(any(target_arch = "wasm32", test))]
fn push_title(sent: &mut Option<String>, name: Option<&str>, dirty: bool, write: impl FnOnce(&str)) -> bool {
    match title_to_send(sent, name, dirty) {
        Some(title) => {
            write(&title);
            true
        }
        None => false,
    }
}

#[cfg(target_arch = "wasm32")]
mod upload;

#[cfg(target_arch = "wasm32")]
fn main() {
    web::start();
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    eprintln!("cadcraft-web only runs in the browser: build it with `trunk build --release` in apps/cadcraft-web");
}

#[cfg(test)]
mod tests {
    use super::{document_title, push_title, title_to_send, wants_cpu};

    /// Guards the `?cpu` URL flag: only a whole `cpu` query parameter (bare or with a value)
    /// selects the CPU canvas, so `?sample` and `?gpucpu` stay on the GPU path.
    #[test]
    fn cpu_flag_is_detected() {
        assert!(wants_cpu("?cpu"));
        assert!(wants_cpu("?cpu&sample"));
        assert!(wants_cpu("?sample&cpu"));
        assert!(!wants_cpu(""));
        assert!(!wants_cpu("?sample"));
        assert!(!wants_cpu("?webgl&sample"));
        assert!(!wants_cpu("?gpucpu"));
    }

    /// Guards the tab title text: a drawing contributes its name and, when dirty, a trailing
    /// bullet; a blank or missing name leaves the app name alone and never shows a bullet.
    #[test]
    fn viewport_title_names_the_document_and_marks_unsaved_changes() {
        assert_eq!(document_title(Some("drawing.dwg"), false), "drawing.dwg — CADCraft");
        assert_eq!(document_title(Some("drawing.dwg"), true), "drawing.dwg • — CADCraft");
        assert_eq!(document_title(Some("Drawing1"), false), "Drawing1 — CADCraft");
        assert_eq!(document_title(None, false), "CADCraft");
        assert_eq!(document_title(None, true), "CADCraft");
        assert_eq!(document_title(Some("   "), true), "CADCraft");
    }

    /// Guards the change signal: a title is sent on open, rename, dirty change and close, but
    /// never twice in a row, so an idle tab is not repainted by a repeated title command.
    #[test]
    fn viewport_title_follows_open_rename_and_dirty_changes_only() {
        let mut sent = None;
        assert_eq!(title_to_send(&mut sent, Some("drawing.dwg"), false).as_deref(), Some("drawing.dwg — CADCraft"));
        // The page already has it: send nothing.
        assert_eq!(title_to_send(&mut sent, Some("drawing.dwg"), false), None);
        // Unsaved changes add the bullet.
        assert_eq!(title_to_send(&mut sent, Some("drawing.dwg"), true).as_deref(), Some("drawing.dwg • — CADCraft"));
        // A rename shows the new name.
        assert_eq!(title_to_send(&mut sent, Some("plan.dwg"), true).as_deref(), Some("plan.dwg • — CADCraft"));
        // Saving clears it again.
        assert_eq!(title_to_send(&mut sent, Some("plan.dwg"), false).as_deref(), Some("plan.dwg — CADCraft"));
        // Closing the last drawing leaves the app name only.
        assert_eq!(title_to_send(&mut sent, None, false).as_deref(), Some("CADCraft"));
        assert_eq!(title_to_send(&mut sent, None, false), None);
    }

    /// Guards the title write: every changed title reaches the writer (on the web, the browser
    /// document title) exactly once, and an unchanged one is not written again. Without the write
    /// the tab and an embedding page stay on `CADCraft` however the drawing changes.
    #[test]
    fn changed_title_reaches_the_document_title_writer_once() {
        let mut sent = None;
        let mut written: Vec<String> = Vec::new();
        assert!(push_title(&mut sent, Some("drawing.dwg"), false, |title| written.push(title.to_owned())));
        // Unchanged: nothing is written again.
        assert!(!push_title(&mut sent, Some("drawing.dwg"), false, |title| written.push(title.to_owned())));
        // Unsaved changes are written.
        assert!(push_title(&mut sent, Some("drawing.dwg"), true, |title| written.push(title.to_owned())));
        // Closing the drawing writes the app name.
        assert!(push_title(&mut sent, None, false, |title| written.push(title.to_owned())));
        assert_eq!(written, ["drawing.dwg — CADCraft", "drawing.dwg • — CADCraft", "CADCraft"]);
    }
}
