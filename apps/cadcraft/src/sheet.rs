//! macOS file dialogs as sheets. Shown in line, or from a helper thread that rfd hands to the main
//! thread's modal loop, a panel runs AppKit's modal event loop; resizing it sends window events
//! back into winit's event handler and winit aborts the app (#1527). A sheet returns at once and
//! the window keeps running; a thread of its own waits for the answer.

use std::sync::Arc;

use cadcraft_ui_egui::{Pick, StartPick};

type Answer = std::pin::Pin<Box<dyn std::future::Future<Output = Option<String>> + Send>>;

/// `Services::start_pick`: shows `pick` as a sheet on `window`; `ui` is woken when it answers.
pub fn start_pick(window: Arc<winit::window::Window>, ui: egui::Context) -> StartPick {
    Box::new(move |pick, done| {
        let (sheet_tx, sheet_rx) = std::sync::mpsc::channel::<Answer>();
        let ui = ui.clone();
        // The thread first, so the answer of a sheet once shown always has somewhere to go. If it
        // cannot start, `done` is dropped unanswered and the app says the dialog could not be
        // shown.
        let spawned = std::thread::Builder::new().name("file-dialog".into()).spawn(move || {
            if let Ok(answer) = sheet_rx.recv() {
                done(pollster::block_on(answer));
                ui.request_repaint();
            }
        });
        if let Err(e) = spawned {
            log::warn!("file dialog: {e}");
            return;
        }
        // rfd builds the panel and begins the sheet here, on the main thread (as it must).
        let dialog =
            crate::filters(&pick).iter().fold(rfd::AsyncFileDialog::new().set_parent(&*window), |d, (label, exts)| d.add_filter(*label, exts));
        let path = |f: rfd::FileHandle| f.path().to_string_lossy().to_string();
        let answer: Answer = match pick {
            Pick::Save(name) => {
                let picked = dialog.set_file_name(name).save_file();
                Box::pin(async move { picked.await.map(path) })
            }
            Pick::Lisp(dir) | Pick::Pgp(dir) => {
                let dialog = if dir.is_empty() { dialog } else { dialog.set_directory(dir) };
                let picked = dialog.pick_file();
                Box::pin(async move { picked.await.map(path) })
            }
            Pick::Open | Pick::Image => {
                let picked = dialog.pick_file();
                Box::pin(async move { picked.await.map(path) })
            }
        };
        let _ = sheet_tx.send(answer);
    })
}
