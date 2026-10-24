//! Desktop drawing saves. One worker at a time bounds concurrent serialization and ensures that
//! two saves cannot commit the same destination out of order.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::{Receiver, TryRecvError, channel},
};
use std::time::{Duration, Instant};

use cadcraft_engine::cmd::file::{CompletedSave, PreparedSave};
use serde_json::{Value, json};

use crate::theme::Tokens;

struct Job {
    id: u64,
    uid: u64,
    path: String,
    started: Instant,
    cancel: Arc<AtomicBool>,
    rx: Receiver<Result<CompletedSave, String>>,
}

#[derive(Default)]
pub struct Saving {
    next_id: u64,
    job: Option<Job>,
    last: Option<Value>,
}

impl Saving {
    pub fn busy(&self) -> bool {
        self.job.is_some()
    }

    pub fn start(&mut self, prepared: PreparedSave) -> Result<Value, String> {
        if self.busy() {
            return Err("a drawing is already being saved; wait for it to finish".into());
        }
        let id = self.next_id.saturating_add(1);
        let uid = prepared.uid;
        let path = prepared.path.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let (tx, rx) = channel();
        std::thread::Builder::new()
            .name("cadcraft-save".into())
            .spawn(move || {
                let _ = tx.send(prepared.perform(&worker_cancel));
            })
            .map_err(|e| format!("could not start save worker: {e}"))?;
        self.next_id = id;
        self.job = Some(Job { id, uid, path: path.clone(), started: Instant::now(), cancel, rx });
        self.last = None;
        Ok(json!({"jobId": id, "state": "running", "path": path}))
    }

    pub fn cancel(&self, id: u64) -> Result<Value, String> {
        let Some(job) = self.job.as_ref().filter(|j| j.id == id) else { return Err("no running save with that jobId".into()) };
        job.cancel.store(true, Ordering::Release);
        Ok(json!({"jobId": id, "state": "cancelling"}))
    }

    pub fn status(&self, id: Option<u64>) -> Value {
        if let Some(job) = &self.job
            && id.is_none_or(|id| id == job.id)
        {
            return json!({"jobId": job.id, "state": if job.cancel.load(Ordering::Acquire) { "cancelling" } else { "running" }, "path": job.path});
        }
        if let Some(last) = &self.last
            && id.is_none_or(|id| last["jobId"].as_u64() == Some(id))
        {
            return last.clone();
        }
        json!({"state": "idle"})
    }

    pub fn poll(&mut self) -> Option<(u64, String, Result<CompletedSave, String>)> {
        let job = self.job.as_ref()?;
        let result = match job.rx.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return None,
            Err(TryRecvError::Disconnected) => Err("save worker stopped before completing".into()),
        };
        let job = self.job.take()?;
        self.last = Some(match &result {
            Ok(done) => json!({"jobId": job.id, "state": "completed", "path": job.path, "bytes": done.bytes, "result": done.result}),
            Err(error) => {
                json!({"jobId": job.id, "state": if error == "save cancelled" { "cancelled" } else { "failed" }, "path": job.path, "error": error})
            }
        });
        Some((job.uid, job.path, result))
    }

    pub fn show(&self, ctx: &egui::Context) {
        let Some(job) = &self.job else { return };
        let name = std::path::Path::new(&job.path).file_name().map_or_else(|| job.path.clone(), |n| n.to_string_lossy().to_string());
        let t = Tokens::get();
        egui::Area::new(egui::Id::new("cc_saving")).order(egui::Order::Foreground).anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 64.0)).show(
            ctx,
            |ui| {
                egui::Frame::NONE
                    .fill(t.panel)
                    .stroke(egui::Stroke::new(1.0, t.accent))
                    .corner_radius(6.0)
                    .inner_margin(egui::Margin::symmetric(18, 12))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.add(egui::Spinner::new().color(t.accent));
                            ui.label(format!("Saving {name}... {:.0} s", job.started.elapsed().as_secs_f32()));
                            if ui.button("Cancel").clicked() {
                                job.cancel.store(true, Ordering::Release);
                            }
                        });
                    });
            },
        );
        ctx.request_repaint_after(Duration::from_millis(50));
    }
}
