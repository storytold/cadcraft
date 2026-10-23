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

#[cfg(target_arch = "wasm32")]
mod web {
    use cadcraft_engine::Session;
    use cadcraft_ui_egui::{CadApp, Services};
    use wasm_bindgen::JsCast as _;

    const CANVAS_ID: &str = "cadcraft_canvas";
    const LOADING_ID: &str = "cadcraft_loading";

    /// The app, the files File > Open… and drag and drop read for it in the background, the
    /// page embedding it (`?host=parent`), and the browser's `prefers-reduced-motion` (it stays
    /// live, so View ▸ Accessibility ▸ Animation Effects ▸ Use System Setting follows a change).
    struct Shell(CadApp, crate::upload::Inbox, Option<(crate::host::Bridge, crate::host::DirtyReport)>, Option<web_sys::MediaQueryList>);

    impl eframe::App for Shell {
        fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
            self.1.take_dropped(ctx);
            self.1.drain(&mut self.0);
            self.0.system_animations = self.3.as_ref().map(|reduced| !reduced.matches());
            self.0.logic(ctx);
            if let Some((bridge, reported)) = &self.2 {
                bridge.report_dirty(&self.0, reported);
            }
        }
        fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
            self.0.raw_input_hook(raw);
            cadcraft_ui_egui::cmdline::capture_tab(ctx, raw);
        }
        /// Preferences (interface theme and language) survive reloads; egui state is not kept.
        fn save(&mut self, storage: &mut dyn eframe::Storage) {
            storage.set_string(cadcraft_ui_egui::options::PROFILE_KEY, self.0.profile_json());
            storage.set_string(cadcraft_ui_egui::PREFS_KEY, self.0.prefs_json());
        }
        fn persist_egui_memory(&self) -> bool {
            false
        }
        fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
            self.0.ui(ui);
            if let Some((bridge, reported)) = &self.2 {
                bridge.report_dirty(&self.0, reported);
            }
        }
    }

    fn query() -> String {
        web_sys::window().and_then(|w| w.location().search().ok()).unwrap_or_default()
    }

    fn query_param(key: &str) -> Option<String> {
        web_sys::UrlSearchParams::new_with_str(&query()).ok()?.get(key)
    }

    /// No file picker on the web: Save / Save As from the menu (and the close prompt) download the
    /// drawing under its current name; `saveas {"path": "name.dxf"}` picks another name.
    fn services() -> Services {
        Services { pick_save: Some(Box::new(|name: &str| Some(name.to_string()))), ..Services::default() }
    }

    /// Start the web app: install the browser's file-format hooks (read, write, plot, save-as and
    /// export), choose the parent-frame bridge for `?host=parent` or browser downloads otherwise,
    /// and run the app on the page's canvas.
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
        cadcraft_engine::cmd::file::set_export_with(|d, name, opts| cadcraft_io::export_with(d, name, opts).map_err(|e| e.to_string()));
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
                        Ok(Box::new(Shell(app, inbox, host, reduced_motion)))
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
    use super::wants_cpu;

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
}
