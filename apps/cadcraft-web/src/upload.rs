//! Browser uploads: File > Open… on the web, through web-sys only. There is no file system to pick
//! a path from, so the picker is a hidden `<input type="file">`; the chosen file (or one dropped on
//! the page) is read in the background and handed to the app on its next frame, which opens it
//! with OPEN `{data, name}` ([`CadApp::open_bytes`]).
//!
//! SHX fonts (text fonts, big fonts, shape files) chosen or dropped with a drawing are the web
//! build's font folder: they are registered with the font table
//! ([`cadcraft_fonts::shx::register`]) under their file name, so the styles and linetypes that
//! name them draw with them, and the drawings are regenerated.

use std::cell::Cell;
use std::rc::Rc;

use cadcraft_ui_egui::CadApp;
use wasm_bindgen::JsCast as _;
use web_sys::js_sys;

/// The hidden file input (one at a time; a cancelled pick leaves it until the next one).
const INPUT_ID: &str = "cadcraft_open_input";
/// What the picker offers: the formats OPEN reads, and the SHX fonts drawings name.
const ACCEPT: &str = ".dxf,.dwg,.shx";

/// Most files taken from one pick.
const MAX_PICKED: u32 = 64;
/// Most bytes being read at once, all files together: a large pick must not exhaust the page's
/// memory (each file is copied once more into the app).
const MAX_READING_BYTES: u64 = 512 << 20;

/// A file read in the browser (name, contents), or why it couldn't be read.
type Upload = Result<(String, Vec<u8>), String>;

/// A drawing the hosting page sent (`cadcraft:open`): the cabinet's bytes are the document the
/// session starts from, so it opens locally (`CadApp::open_host_bytes`) instead of becoming a
/// room operation the room refuses.
#[derive(Clone)]
struct HostFile {
    name: String,
    bytes: Vec<u8>,
}

/// Files read in the background, waiting for the app's next frame.
#[derive(Clone)]
pub struct Inbox {
    files: Rc<Cell<Vec<Upload>>>,
    host_files: Rc<Cell<Vec<HostFile>>>,
    /// Bytes of the files being read now.
    reading: Rc<Cell<u64>>,
    ctx: egui::Context,
}

impl Inbox {
    pub fn new(ctx: &egui::Context) -> Self {
        Inbox { files: Rc::default(), host_files: Rc::default(), reading: Rc::default(), ctx: ctx.clone() }
    }

    /// Show the browser's file picker and return at once. A chosen file arrives through
    /// [`Self::drain`]; cancelling the picker does nothing.
    pub fn pick(&self) {
        if let Err(e) = self.show_picker() {
            self.push(Err(format!("Open: the browser's file picker is not available ({e})")));
        }
    }

    fn show_picker(&self) -> Result<(), String> {
        let document = web_sys::window().and_then(|w| w.document()).ok_or("no browser document")?;
        let body = document.body().ok_or("the page has no body")?;
        if let Some(old) = document.get_element_by_id(INPUT_ID) {
            old.remove();
        }
        let input = document
            .create_element("input")
            .map_err(js_error)?
            .dyn_into::<web_sys::HtmlInputElement>()
            .map_err(|_| "could not create a file input".to_string())?;
        input.set_type("file");
        input.set_accept(ACCEPT);
        // A drawing and the fonts it uses can be chosen together.
        input.set_multiple(true);
        input.set_id(INPUT_ID);
        input.set_attribute("style", "display:none").map_err(js_error)?;
        let (inbox, chosen) = (self.clone(), input.clone());
        // Called once, when files were chosen (browsers don't fire `change` on cancel).
        let on_change = wasm_bindgen::closure::Closure::once_into_js(move || {
            if let Some(files) = chosen.files() {
                for i in 0..files.length().min(MAX_PICKED) {
                    if let Some(file) = files.get(i) {
                        inbox.read(file);
                    }
                }
                let skipped: Vec<String> = (MAX_PICKED..files.length()).filter_map(|i| files.get(i)).map(|f| f.name()).collect();
                if !skipped.is_empty() {
                    inbox.push(Err(format!("Open: at most {MAX_PICKED} files at a time; not opened: {}", skipped.join(", "))));
                }
            }
            chosen.remove();
        });
        input.set_onchange(Some(on_change.unchecked_ref()));
        body.append_child(&input).map_err(js_error)?;
        input.click();
        Ok(())
    }

    /// Read the files dropped on the page this frame (eframe hands them over unread on the web).
    /// The list is taken, not cloned: a later pass or hidden tick must not read the same drop.
    pub fn take_dropped(&self, ctx: &egui::Context) {
        for f in ctx.input_mut(|i| std::mem::take(&mut i.raw.dropped_files)) {
            if let Some(file) = f.web_file() {
                self.read(file.clone());
            }
        }
    }

    /// Read `file` in the background; it arrives through [`Self::drain`]. A file too large to
    /// open is refused before it is read.
    fn read(&self, file: web_sys::File) {
        let name = file.name();
        // `File.size` is a whole number of bytes; anything else counts as too large.
        let size = file.size();
        let len = if size.is_finite() && size >= 0.0 { size as u64 } else { u64::MAX };
        if let Some(e) = CadApp::open_size_error(&name, len) {
            return self.push(Err(e));
        }
        let reading = self.reading.get();
        if reading.saturating_add(len) > MAX_READING_BYTES {
            return self.push(Err(format!("Open: {name} was not read: the files being read together are too large; open it on its own")));
        }
        self.reading.set(reading.saturating_add(len));
        let inbox = self.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let r = match wasm_bindgen_futures::JsFuture::from(file.array_buffer()).await {
                Ok(buf) => Ok((name, js_sys::Uint8Array::new(&buf).to_vec())),
                Err(e) => Err(format!("Open: could not read {name}: {}", js_error(e))),
            };
            inbox.reading.set(inbox.reading.get().saturating_sub(len));
            inbox.push(r);
        });
    }

    /// The drawing the hosting page sent with the session (`cadcraft:open`): it opens locally on
    /// the next frame, even while a shared session is attached, because the cabinet's bytes are
    /// how the file reaches the browser (INTEGRATION.md §5.1).
    pub fn receive_host(&self, name: String, bytes: Vec<u8>) {
        match CadApp::open_size_error(&name, bytes.len() as u64) {
            Some(e) => self.push(Err(e)),
            None => {
                let mut files = self.host_files.take();
                files.push(HostFile { name, bytes });
                self.host_files.set(files);
                self.ctx.request_repaint();
            }
        }
    }

    /// A file from elsewhere refused before it was read (`error` says why).
    pub fn refuse(&self, error: String) {
        self.push(Err(error));
    }

    fn push(&self, upload: Upload) {
        let mut files = self.files.take();
        files.push(upload);
        self.files.set(files);
        self.ctx.request_repaint();
    }

    /// Open what arrived since the last frame (or report why it couldn't be read). Fonts are
    /// registered first, so drawings opened with them draw with them.
    pub fn drain(&self, app: &mut CadApp) {
        // The host's drawing first: it is the document the session starts from, and it opens in
        // the window rather than as a room operation.
        for file in self.host_files.take() {
            app.open_host_bytes(&file.name, &file.bytes);
        }
        let (fonts, files): (Vec<Upload>, Vec<Upload>) = self.files.take().into_iter().partition(|u| u.as_ref().is_ok_and(|(n, _)| is_shx(n)));
        let mut registered = false;
        for (name, bytes) in fonts.into_iter().flatten() {
            match cadcraft_fonts::shx::register(&name, &bytes) {
                Ok(()) => registered = true,
                Err(e) => app.open_failed(format!("{name}: {e}")),
            }
        }
        if registered {
            let _ = app.run("regenall", serde_json::json!({}));
        }
        for upload in files {
            match upload {
                Ok((name, bytes)) => app.open_bytes(&name, &bytes),
                Err(e) => app.open_failed(e),
            }
        }
    }
}

/// Whether `name` is an SHX font or shape file.
fn is_shx(name: &str) -> bool {
    name.to_ascii_lowercase().ends_with(".shx")
}

fn js_error(e: wasm_bindgen::JsValue) -> String {
    e.as_string().unwrap_or_else(|| format!("{e:?}"))
}
