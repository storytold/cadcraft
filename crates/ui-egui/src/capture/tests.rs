use std::sync::Arc;

use super::*;
use cadcraft_engine::Session;
use egui::{Color32, ColorImage, ViewportId, ViewportInfo};
use serde_json::json;
use std::sync::mpsc::{Receiver, channel};

use crate::Services;

/// An empty `CadApp` over a fresh `Session` for capture tests.
fn app() -> CadApp {
    CadApp::new(Session::new(), Services::default())
}

/// A visible, focused, 1-ppp surface of `size` with an 8192-pixel texture side.
fn surface(size: egui::Vec2) -> SurfaceGeometry {
    SurfaceGeometry { viewport: Some(size), pixels_per_point: 1.0, max_texture_side: 8192, minimized: false, occluded: false, window_focused: true }
}

/// Seed the admission snapshot a raw hook would have taken, then bump both epochs as an
/// observation of a change would.
fn set_geometry(app: &mut CadApp, ctx: &egui::Context, size: egui::Vec2) {
    app.capture_geometry = Some(CaptureGeometry { surface: surface(size), identity: observed_identity(app, ctx) });
    app.capture_epoch = app.capture_epoch.wrapping_add(1);
    app.capture_identity_epoch = app.capture_identity_epoch.wrapping_add(1);
}

/// Seed 800×600 geometry, admit one pathless request and return its reply receiver and token.
fn admit_one(app: &mut CadApp, ctx: &egui::Context) -> (Receiver<Value>, u64) {
    set_geometry(app, ctx, egui::vec2(800.0, 600.0));
    let (tx, rx) = channel();
    admit(app, ctx, None, tx);
    let token = app.shots.requests.first().map(|shot| shot.token).unwrap_or_default();
    (rx, token)
}

/// Focused `RawInput` with a `width`×`height` screen rect and an 8192-pixel texture side.
fn raw(width: f32, height: f32) -> egui::RawInput {
    egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, height))),
        focused: true,
        max_texture_side: Some(8192),
        ..Default::default()
    }
}

/// One real UI pass, with a presented frame the screenshot commands can be taken from.
fn ui_frame(app: &mut CadApp, ctx: &egui::Context) -> egui::FullOutput {
    ui_frame_sized(app, ctx, 800.0, 600.0)
}

/// Run one real `logic` + `ui` pass at the requested size and drop texture deltas, yielding the frame's output.
fn ui_frame_sized(app: &mut CadApp, ctx: &egui::Context, width: f32, height: f32) -> egui::FullOutput {
    let raw = raw(width, height);
    note_raw_geometry(app, ctx, &raw);
    let mut out = ctx.run_ui(raw, |ui| {
        app.logic(ui.ctx());
        app.ui(ui);
    });
    out.textures_delta.clear();
    out
}

/// Every `ViewportCommand::Screenshot` callback emitted by a frame.
fn screenshot_callbacks(out: &egui::FullOutput) -> Vec<egui::ScreenshotCallback> {
    out.viewport_output
        .values()
        .flat_map(|viewport| viewport.commands.iter())
        .filter_map(|command| match command {
            egui::ViewportCommand::Screenshot(callback) => Some(callback.clone()),
            _ => None,
        })
        .collect()
}

/// Drive the settling passes so `issue` sends the real `ViewportCommand::Screenshot`.
fn issue_after_settling(app: &mut CadApp, ctx: &egui::Context) -> (egui::FullOutput, Vec<egui::ScreenshotCallback>) {
    for shot in &mut app.shots.requests {
        // Exercise the settled-frame path without sleeping for either wall-clock delay.
        // A scale change can also start an asynchronous canvas rebuild.
        shot.ready_at = Instant::now() - REBUILD_WAIT - Duration::from_millis(1);
    }
    ui_frame(app, ctx).drop_without_applying_deltas();
    ui_frame(app, ctx).drop_without_applying_deltas();
    let out = ui_frame(app, ctx);
    let callbacks = screenshot_callbacks(&out);
    (out, callbacks)
}

/// Complete each screenshot callback with a solid red image of `size`.
fn complete(callbacks: Vec<egui::ScreenshotCallback>, size: [usize; 2]) {
    let image = Arc::new(ColorImage::filled(size, Color32::RED));
    for callback in callbacks {
        callback.complete(Arc::clone(&image));
    }
}

/// A completed-but-undrained shot still occupies a `MAX_REQUESTS` slot; the next admission is rejected as "outstanding" and no earlier token is replaced.
#[test]
fn admission_counts_completed_but_undrained_and_never_evicts() {
    let ctx = egui::Context::default();
    let mut app = app();
    let _receivers: Vec<_> = (0..MAX_REQUESTS).map(|_| admit_one(&mut app, &ctx)).collect();
    assert_eq!(app.shots.len(), MAX_REQUESTS);
    // Complete the first slot without draining it: it still owns its reservation and request.
    let image = Arc::new(ColorImage::filled([800, 600], Color32::RED));
    app.shots.requests[0].callback.take().expect("a callback").complete(image);
    let (tx, rx) = channel();
    admit(&mut app, &ctx, None, tx);
    assert_eq!(app.shots.len(), MAX_REQUESTS, "the fifth request is rejected, not admitted");
    let reply = rx.recv().expect("a rejection reply");
    assert_eq!(reply["ok"], false);
    assert!(reply["error"].as_str().unwrap_or_default().contains("outstanding"), "{reply}");
    assert!(app.shots.requests.iter().all(|shot| shot.token != reply["request"].as_u64().unwrap_or_default()));
}

/// A completed callback whose deadline has passed is rejected: the reply reports "timeout" and the request is removed from `Shots`; the test does not inspect the filesystem.
#[test]
fn deadline_is_checked_before_writing_a_completed_image() {
    let ctx = egui::Context::default();
    let mut app = app();
    let (rx, _) = admit_one(&mut app, &ctx);
    app.shots.requests[0].issued = true;
    app.shots.requests[0].surface_epoch = app.capture_epoch;
    app.shots.requests[0].deadline = Instant::now() - Duration::from_millis(1);
    let image = Arc::new(ColorImage::filled([800, 600], Color32::RED));
    app.shots.requests[0].callback.take().expect("a callback").complete(image);
    drain(&mut app, &ctx);
    let reply = rx.recv().expect("a timeout reply");
    assert_eq!(reply["ok"], false);
    assert!(reply["error"].as_str().unwrap_or_default().contains("timeout"), "{reply}");
    assert!(app.shots.is_empty());
}

/// A drawing switch before issue rejects the queued request as "document changed" and empties the queue.
#[test]
fn document_change_cancels_before_issuing() {
    let ctx = egui::Context::default();
    let mut app = app();
    let (rx, _) = admit_one(&mut app, &ctx);
    app.session.new_drawing(false);
    drain(&mut app, &ctx);
    let reply = rx.recv().expect("a document-change reply");
    assert!(reply["error"].as_str().unwrap_or_default().contains("document changed"), "{reply}");
    assert!(app.shots.is_empty());
}

/// Moving keyboard focus before issue rejects the queued capture as "focus changed" and empties the queue.
#[test]
fn focus_change_cancels_an_unissued_capture() {
    let ctx = egui::Context::default();
    let focus_a = egui::Id::unique("focus-a");
    let focus_b = egui::Id::unique("focus-b");
    ctx.memory_mut(|memory| memory.request_focus(focus_a));
    let mut app = app();
    set_geometry(&mut app, &ctx, egui::vec2(800.0, 600.0));
    let (tx, rx) = channel();
    admit(&mut app, &ctx, None, tx);
    assert_eq!(app.shots.len(), 1);
    ctx.memory_mut(|memory| memory.request_focus(focus_b));
    drain(&mut app, &ctx);
    let reply = rx.recv().expect("a focus-change reply");
    assert!(reply["error"].as_str().unwrap_or_default().contains("focus changed"), "{reply}");
    assert!(app.shots.is_empty());
}

/// Two requests issue from one settled frame; each reply carries its own token and a distinct, readable file with a PNG signature.
#[test]
fn issued_callbacks_reply_once_with_matching_tokens_and_distinct_pathless_files() {
    let ctx = egui::Context::default();
    let mut app = app();
    set_geometry(&mut app, &ctx, egui::vec2(800.0, 600.0));
    let (tx1, rx1) = channel();
    let (tx2, rx2) = channel();
    admit(&mut app, &ctx, None, tx1);
    admit(&mut app, &ctx, None, tx2);
    let tokens: Vec<_> = app.shots.requests.iter().map(|shot| shot.token).collect();
    let (_, callbacks) = issue_after_settling(&mut app, &ctx);
    assert_eq!(callbacks.len(), 2, "both requests issue in the same settled frame");
    complete(callbacks, [800, 600]);
    drain(&mut app, &ctx);
    let first = rx1.recv().expect("the first reply");
    let second = rx2.recv().expect("the second reply");
    assert_eq!((first["ok"].clone(), second["ok"].clone()), (json!(true), json!(true)));
    assert_eq!(first["request"].as_u64(), tokens.first().copied());
    assert_eq!(second["request"].as_u64(), tokens.get(1).copied());
    let a = first["result"]["path"].as_str().expect("the first file").to_string();
    let b = second["result"]["path"].as_str().expect("the second file").to_string();
    assert_ne!(a, b, "one file per request");
    for path in [&a, &b] {
        assert!(std::fs::read(path).is_ok_and(|bytes| bytes.starts_with(b"\x89PNG\r\n\x1a\n")), "{path}");
        let _ = std::fs::remove_file(path);
    }
    assert!(app.shots.is_empty());
}

/// A drawing switch while a capture is in flight rejects its completed callback as "document changed" and reports no result path.
#[test]
fn document_switch_after_issue_rejects_the_completed_callback_without_writing() {
    let ctx = egui::Context::default();
    let mut app = app();
    let (rx, token) = admit_one(&mut app, &ctx);
    let (_, callbacks) = issue_after_settling(&mut app, &ctx);
    assert_eq!(callbacks.len(), 1);
    complete(callbacks, [800, 600]);
    app.session.new_drawing(false);
    drain(&mut app, &ctx);
    let reply = rx.recv().expect("a document-change reply");
    assert_eq!(reply["request"].as_u64(), Some(token));
    assert!(reply["error"].as_str().unwrap_or_default().contains("document changed"), "{reply}");
    assert!(reply.get("result").is_none(), "a rejected capture must not report or write a file");
    assert!(app.shots.is_empty());
}

/// Once a callback has completed but the deadline has passed, drain rejects it as "timeout" with no result path.
#[test]
fn completed_callback_after_timeout_writes_nothing() {
    let ctx = egui::Context::default();
    let mut app = app();
    let (rx, _) = admit_one(&mut app, &ctx);
    let (_, callbacks) = issue_after_settling(&mut app, &ctx);
    complete(callbacks, [800, 600]);
    app.shots.requests[0].deadline = Instant::now() - Duration::from_millis(1);
    drain(&mut app, &ctx);
    let reply = rx.recv().expect("a timeout reply");
    assert!(reply["error"].as_str().unwrap_or_default().contains("timeout"), "{reply}");
    assert!(reply.get("result").is_none(), "a timed-out capture must not write a file");
    assert!(app.shots.is_empty());
}

/// Cancelling all shots replies "cancelled" and wins over an already-completed callback, which is never reported.
#[test]
fn callback_completed_before_cancel_all_is_inert() {
    let ctx = egui::Context::default();
    let mut app = app();
    let (rx, token) = admit_one(&mut app, &ctx);
    let (_, callbacks) = issue_after_settling(&mut app, &ctx);
    complete(callbacks, [800, 600]);
    cancel_all(&mut app);
    let reply = rx.recv().expect("a cancellation reply");
    assert_eq!(reply["request"].as_u64(), Some(token));
    assert!(reply["error"].as_str().unwrap_or_default().contains("cancelled"), "{reply}");
    assert!(reply.get("result").is_none(), "cancellation wins over a late completion");
    assert!(app.shots.is_empty());
}

/// A 5000×5000 image is rejected by the 64 MiB per-image bound; after two 2500×2500 admissions a third is rejected by the aggregate bound, leaving two queued.
#[test]
fn per_image_and_aggregate_budgets_reject_before_issue() {
    let ctx = egui::Context::default();
    let mut app = app();
    // 5000×5000×4 bytes = 100 MB, over the 64 MiB per-image bound.
    set_geometry(&mut app, &ctx, egui::vec2(5000.0, 5000.0));
    let (tx, rx) = channel();
    admit(&mut app, &ctx, None, tx);
    let reply = rx.recv().expect("per-image rejection");
    assert!(reply["error"].as_str().unwrap_or_default().contains("per-image"), "{reply}");
    assert!(app.shots.is_empty());

    // 2500×2500×4 bytes = 25 MB each: two fit, the third exceeds the 64 MiB aggregate bound.
    set_geometry(&mut app, &ctx, egui::vec2(2500.0, 2500.0));
    let _first = admit_one_at(&mut app, &ctx, egui::vec2(2500.0, 2500.0));
    let _second = admit_one_at(&mut app, &ctx, egui::vec2(2500.0, 2500.0));
    let (tx, rx) = channel();
    admit(&mut app, &ctx, None, tx);
    let reply = rx.recv().expect("aggregate rejection");
    assert!(reply["error"].as_str().unwrap_or_default().contains("outstanding requests"), "{reply}");
    assert_eq!(app.shots.len(), 2);
}

/// Seed an arbitrary surface size, admit one pathless request and return the newly queued request's receiver and token.
fn admit_one_at(app: &mut CadApp, ctx: &egui::Context, size: egui::Vec2) -> (Receiver<Value>, u64) {
    set_geometry(app, ctx, size);
    let (tx, rx) = channel();
    admit(app, ctx, None, tx);
    let token = app.shots.requests.last().map(|shot| shot.token).unwrap_or_default();
    (rx, token)
}

/// Every observed surface change, including A→B→A, bumps the surface epoch and retains the latest geometry.
#[test]
fn every_observed_geometry_change_bumps_the_generation() {
    let ctx = egui::Context::default();
    let mut app = app();
    note_raw_geometry(&mut app, &ctx, &raw(800.0, 600.0));
    let first = app.capture_epoch;
    note_raw_geometry(&mut app, &ctx, &raw(900.0, 600.0));
    let second = app.capture_epoch;
    note_raw_geometry(&mut app, &ctx, &raw(800.0, 600.0));
    let third = app.capture_epoch;
    assert!(first < second && second < third, "A→B→A bumps the generation at each observed change");
    assert_eq!(app.capture_geometry.and_then(|g| g.expected_size()), Some([800, 600]));
}

/// An invalid settled pass must not leave the previous surface retained: the raw path clears
/// it, and the pass path has to invalidate it identically so a later admission cannot reuse a
/// surface no current pass renders.
#[test]
fn an_invalid_pass_invalidates_the_retained_surface() {
    let ctx = egui::Context::default();
    let mut app = app();
    note_raw_geometry(&mut app, &ctx, &raw(800.0, 600.0));
    assert!(app.capture_geometry.is_some(), "the raw snapshot was retained");
    let surface_epoch = app.capture_epoch;
    let identity_epoch = app.capture_identity_epoch;
    let mut invalid = raw(800.0, 600.0);
    invalid.viewports.insert(ViewportId::ROOT, ViewportInfo { native_pixels_per_point: Some(0.0), ..Default::default() });
    let mut out = ctx.run_ui(invalid, |_| {});
    out.textures_delta.clear();
    assert_eq!(ctx.pixels_per_point(), 0.0, "the pass pixels-per-point is invalid");
    note_pass_geometry(&mut app, &ctx);
    assert!(app.capture_geometry.is_none(), "an invalid pass must not keep the previous surface");
    assert!(app.capture_epoch > surface_epoch, "the surface epoch changes");
    assert!(app.capture_identity_epoch > identity_epoch, "the identity epoch changes with the surface");
    let (tx, rx) = channel();
    admit(&mut app, &ctx, None, tx);
    let reply = rx.recv().expect("an admission rejection");
    assert_eq!(reply["ok"], false);
    assert!(reply["error"].as_str().unwrap_or_default().contains("not known"), "{reply}");
}

/// With a screen rect but no inner rect, 640×480 points at 2 ppp yields a `[1280, 960]` image and keeps the focused flag.
#[test]
fn raw_geometry_uses_screen_rect_when_inner_rect_is_absent() {
    let ctx = egui::Context::default();
    let mut app = app();
    let mut raw = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(640.0, 480.0))),
        focused: true,
        max_texture_side: Some(4096),
        ..Default::default()
    };
    raw.viewports.insert(ViewportId::ROOT, ViewportInfo { native_pixels_per_point: Some(2.0), ..Default::default() });
    note_raw_geometry(&mut app, &ctx, &raw);
    let geometry = app.capture_geometry.expect("fresh geometry");
    assert_eq!(geometry.expected_size(), Some([1280, 960]));
    assert!(geometry.surface.window_focused);
}

/// The producer regression for the pass-lifecycle defect: a first pass that egui discards must
/// not issue, and the settled second pass must issue exactly once.
#[test]
fn a_discarded_pass_does_not_issue_and_the_settled_pass_does_once() {
    let ctx = egui::Context::default();
    let mut app = app();
    let (rx, _) = admit_one(&mut app, &ctx);
    for shot in &mut app.shots.requests {
        shot.ready_at = Instant::now() - Duration::from_millis(1);
        shot.frames = SETTLE_PASSES - 1;
    }
    let raw = raw(800.0, 600.0);
    note_raw_geometry(&mut app, &ctx, &raw);
    let mut discard = true;
    let mut out = ctx.run_ui(raw, |ui| {
        app.logic(ui.ctx());
        if std::mem::take(&mut discard) {
            ui.ctx().request_discard("test: force a second pass");
        }
        app.ui(ui);
    });
    out.textures_delta.clear();
    let callbacks = screenshot_callbacks(&out);
    assert_eq!(callbacks.len(), 1, "the settled pass issues once; the discarded pass does not");
    assert_eq!(app.ui_frames, 1, "one settled frame, not one per pass");
    complete(callbacks, [800, 600]);
    drain(&mut app, &ctx);
    assert_eq!(rx.recv().expect("a reply")["ok"], true);
}

/// Issuance re-bases the expected image on the settled pass, so the saved interface zoom
/// (applied by egui at the start of a later pass) cannot leave a stale admission estimate.
#[test]
fn issuance_uses_the_settled_interface_scale_not_the_admission_estimate() {
    let ctx = egui::Context::default();
    let mut app = app();
    app.options.interface_scale = 2.0;
    let raw = raw(800.0, 600.0);
    note_raw_geometry(&mut app, &ctx, &raw);
    let (tx, rx) = channel();
    admit(&mut app, &ctx, None, tx);
    assert_eq!(app.shots.requests[0].expected, [800, 600], "the admission estimate predates the zoom");
    let (_, callbacks) = issue_after_settling(&mut app, &ctx);
    assert_eq!(callbacks.len(), 1);
    assert_eq!(app.shots.requests[0].expected, [1600, 1200], "issued against the settled pass scale");
    complete(callbacks, [1600, 1200]);
    drain(&mut app, &ctx);
    let reply = rx.recv().expect("a reply");
    assert_eq!(reply["ok"], true, "{reply}");
    let path = reply["result"]["path"].as_str().expect("a file").to_string();
    let _ = std::fs::remove_file(path);
}

/// A→B→A: a switch that returns to the same drawing still invalidates the queued request.
#[test]
fn a_document_switch_that_returns_still_rejects_the_queued_capture() {
    let ctx = egui::Context::default();
    let mut app = app();
    app.session.new_drawing(false);
    let (rx, _) = admit_one(&mut app, &ctx);
    let before = active_document(&app);
    app.run("document.switch", json!({ "index": 0 })).expect("switch to the first drawing");
    note_identity(&mut app, &ctx);
    app.run("document.switch", json!({ "index": 1 })).expect("switch back");
    note_identity(&mut app, &ctx);
    assert_eq!(active_document(&app), before, "the drawing and focus are back where they were");
    let (_, callbacks) = issue_after_settling(&mut app, &ctx);
    assert!(callbacks.is_empty(), "the request is rejected, not issued");
    let reply = rx.recv().expect("a rejection reply");
    assert!(reply["error"].as_str().unwrap_or_default().contains("changed"), "{reply}");
    assert!(reply.get("result").is_none());
}

/// A drawing created and closed inside one callback never reaches the same observed values, but
/// the engine's monotonic document epoch still records it.
#[test]
fn a_drawing_created_and_closed_between_observations_still_bumps_the_identity_epoch() {
    let ctx = egui::Context::default();
    let mut app = app();
    set_geometry(&mut app, &ctx, egui::vec2(800.0, 600.0));
    let before_epoch = app.capture_identity_epoch;
    let before_doc = active_document(&app);
    app.session.new_drawing(false);
    app.run("close", json!({})).expect("close the new drawing");
    assert_eq!(active_document(&app), before_doc, "the same drawing is active again");
    note_identity(&mut app, &ctx);
    assert!(app.capture_identity_epoch > before_epoch, "the intervening switch is still observed");
}

/// Raw-hook and pass geometry must agree, or every frame would re-baseline the surface and a
/// capture in flight could never settle.
#[test]
fn a_settled_window_does_not_keep_changing_the_surface_epoch() {
    let ctx = egui::Context::default();
    let mut app = app();
    ui_frame(&mut app, &ctx).drop_without_applying_deltas();
    let epoch = app.capture_epoch;
    let identity = app.capture_identity_epoch;
    for _ in 0..4 {
        ui_frame(&mut app, &ctx).drop_without_applying_deltas();
    }
    assert_eq!(app.capture_epoch, epoch, "the raw hook and the pass agree on the surface");
    assert_eq!(app.capture_identity_epoch, identity, "nothing changed the drawing or focus");
    assert_eq!(app.capture_geometry.and_then(|geometry| geometry.expected_size()), Some([800, 600]));
}

/// A window resized while the request is still settling is issued against the new size.
#[test]
fn a_resize_while_settling_is_absorbed_at_issue() {
    let ctx = egui::Context::default();
    let mut app = app();
    let (rx, _) = admit_one(&mut app, &ctx);
    for shot in &mut app.shots.requests {
        shot.ready_at = Instant::now() - Duration::from_millis(1);
    }
    ui_frame_sized(&mut app, &ctx, 900.0, 600.0).drop_without_applying_deltas();
    ui_frame_sized(&mut app, &ctx, 900.0, 600.0).drop_without_applying_deltas();
    let out = ui_frame_sized(&mut app, &ctx, 900.0, 600.0);
    let callbacks = screenshot_callbacks(&out);
    out.drop_without_applying_deltas();
    assert_eq!(callbacks.len(), 1);
    assert_eq!(app.shots.requests[0].expected, [900, 600], "issued against the settled size");
    complete(callbacks, [900, 600]);
    drain(&mut app, &ctx);
    let reply = rx.recv().expect("a reply");
    assert_eq!(reply["ok"], true, "{reply}");
    let path = reply["result"]["path"].as_str().expect("a file").to_string();
    let _ = std::fs::remove_file(path);
}
