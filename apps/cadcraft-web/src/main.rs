//! CADCraft in the browser.
//!
//! Runs the same [`cadcraft_ui_egui::CadApp`] as the desktop app through eframe's web runner
//! (wgpu: WebGPU where available, WebGL2 otherwise). Build with `trunk build --release` from this
//! directory (output in `dist/web`). URL flags: `?webgl` forces WebGL2, `?sample` opens the sample
//! drawing.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

#[cfg(target_arch = "wasm32")]
mod web {
    use cadcraft_engine::Session;
    use cadcraft_ui_egui::{CadApp, Services};
    use wasm_bindgen::JsCast as _;

    const CANVAS_ID: &str = "cadcraft_canvas";
    const LOADING_ID: &str = "cadcraft_loading";

    struct Shell(CadApp);

    impl eframe::App for Shell {
        fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
            self.0.logic(ctx);
        }
        fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
            self.0.raw_input_hook(raw);
            cadcraft_ui_egui::cmdline::capture_tab(ctx, raw);
        }
        fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
            self.0.ui(ui);
        }
    }

    fn query() -> String {
        web_sys::window().and_then(|w| w.location().search().ok()).unwrap_or_default()
    }

    pub fn start() {
        eframe::WebLogger::init(log::LevelFilter::Info).ok();
        cadcraft_engine::cmd::file::set_io(cadcraft_engine::cmd::file::IoHooks {
            read: |b, name| cadcraft_io::read(b, name).map_err(|e| e.to_string()),
            write: |d, name| cadcraft_io::write(d, name).map_err(|e| e.to_string()),
            plot: Some(|d, space, opts| cadcraft_io::plot(d, space, opts).map_err(|e| e.to_string())),
        });
        wasm_bindgen_futures::spawn_local(async {
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
                        let mut app = CadApp::new(Session::new(), Services::default());
                        if let Some(rs) = &cc.wgpu_render_state {
                            app.set_wgpu(rs);
                        }
                        if query().contains("sample") {
                            let _ = app.run("ui.sample", serde_json::json!({}));
                        }
                        Ok(Box::new(Shell(app)))
                    }),
                )
                .await;
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

#[cfg(target_arch = "wasm32")]
fn main() {
    web::start();
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    eprintln!("cadcraft-web only runs in the browser: build it with `trunk build --release` in apps/cadcraft-web");
}
