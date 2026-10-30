//! Bounded, one-shot window screenshots for `ui.screenshot`.
//!
//! The request record owns the output path, reply, deadline, document/focus identity and the
//! geometry it was admitted against. The shared `craft_ui::capture::CaptureSlot` owns the strong
//! `egui::Context`; its screenshot callback holds only a weak reference and never writes a file.
//! Files are encoded and written only from the logic drain, after the deadline and every identity
//! check have passed.

use std::sync::mpsc::Sender;

use craft_ui::capture::CaptureSlot;
use egui::ViewportCommand;
use serde_json::{Value, json};
use web_time::{Duration, Instant};

use crate::CadApp;

/// At most this many queued, in-flight or completed-but-not-yet-drained requests.
pub(crate) const MAX_REQUESTS: usize = 4;
/// CADCraft keeps its stricter 64 MiB per-image bound (the suite default is 128 MiB).
pub(crate) const MAX_IMAGE_BYTES: usize = 64 * 1024 * 1024;
/// CADCraft keeps the aggregate reservation at the same 64 MiB bound. Completed but undrained
/// images still own their reservation, so they count against this limit.
pub(crate) const MAX_TOTAL_BYTES: usize = 64 * 1024 * 1024;

const READY_DELAY: Duration = Duration::from_millis(120);
/// Accepted settled passes (never discarded passes or hidden logic ticks).
const SETTLE_PASSES: u32 = 3;
const DEADLINE: Duration = Duration::from_secs(8);
const REBUILD_WAIT: Duration = Duration::from_secs(6);
const REPAINT_INTERVAL: Duration = Duration::from_millis(16);

/// The host surface as reported by one `RawInput`. `RawInput` is the fresh source on hidden
/// logic-only ticks; `Context::input`/`viewport_rect` describe the last UI pass and are stale
/// there.
///
/// The equality used for the capture epoch is deliberately strict: OS focus and visibility
/// (`window_focused`, `minimized`, `occluded`) are part of it even though they are not geometry.
/// A focus or visibility transition can change what the presented frame contains (menu state,
/// active-window styling, a hidden surface), so a capture admitted before it is rejected rather
/// than risking an image from another epoch. Do not narrow this to size/DPI/zoom without a
/// replacement visibility check and a measured acceptance path.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SurfaceGeometry {
    /// Logical content size in points. A minimized window keeps its last known size so a forced
    /// hidden paint can still be requested; `None` only before any size was ever reported.
    viewport: Option<egui::Vec2>,
    /// Native pixels per point times egui's interface zoom.
    pixels_per_point: f32,
    max_texture_side: usize,
    minimized: bool,
    occluded: bool,
    window_focused: bool,
}

impl SurfaceGeometry {
    /// `Some(size)` only when both axes are finite and strictly positive; rejects degenerate viewport sizes before they enter a `SurfaceGeometry` snapshot.
    fn valid_size(size: egui::Vec2) -> Option<egui::Vec2> {
        (size.x.is_finite() && size.y.is_finite() && size.x > 0.0 && size.y > 0.0).then_some(size)
    }

    /// The fresh raw hook's surface. Called from `raw_input_hook`, so it runs on both UI and
    /// hidden logic-only callbacks and never reads the stale `InputState` rectangle.
    fn from_raw(raw: &egui::RawInput, ctx: &egui::Context, previous: Option<Self>) -> Option<Self> {
        let info = raw.viewports.get(&raw.viewport_id).cloned().unwrap_or_default();
        // `InputState::viewport_rect` is `RawInput::screen_rect`, falling back to the previous
        // pass's rect; the pass snapshot uses the same source so the two agree bit for bit. The
        // screenshot is of the UI content rect (what kittest renders and the plan binds to), so
        // remove the safe-area insets here as `InputState::content_rect` does.
        let insets = raw.safe_area_insets.unwrap_or_else(|| ctx.input(|input| input.safe_area_insets()));
        let rect = raw.screen_rect.map(|rect| rect - insets);
        let viewport = rect.and_then(|rect| Self::valid_size(rect.size())).or_else(|| previous.and_then(|previous| previous.viewport));
        let zoom = ctx.zoom_factor();
        if !zoom.is_finite() || zoom <= 0.0 {
            return None;
        }
        let pixels_per_point = info
            .native_pixels_per_point
            .map(|native| native * zoom)
            .filter(|pixels| pixels.is_finite() && *pixels > 0.0)
            .or_else(|| previous.map(|previous| previous.pixels_per_point))
            .unwrap_or_else(|| ctx.pixels_per_point());
        if !pixels_per_point.is_finite() || pixels_per_point <= 0.0 {
            return None;
        }
        let max_texture_side = raw.max_texture_side.filter(|side| *side > 0).unwrap_or_else(|| ctx.input(|input| input.max_texture_side));
        let minimized = info.minimized.unwrap_or_else(|| previous.is_some_and(|previous| previous.minimized));
        let occluded = info.occluded.unwrap_or_else(|| previous.is_some_and(|previous| previous.occluded));
        Some(Self { viewport, pixels_per_point, max_texture_side, minimized, occluded, window_focused: raw.focused })
    }

    /// The settled surface of a UI pass: the content rect and ppp this pass renders (and is
    /// presented) with. The saved interface zoom takes effect at the start of a pass, so this is
    /// the only scale the presented frame can be expected to have.
    fn from_pass(ctx: &egui::Context, previous: Option<Self>) -> Option<Self> {
        let content = ctx.input(|input| input.content_rect().size());
        let viewport = Self::valid_size(content).or_else(|| previous.and_then(|previous| previous.viewport));
        let pixels_per_point = ctx.pixels_per_point();
        if !pixels_per_point.is_finite() || pixels_per_point <= 0.0 {
            return None;
        }
        let max_texture_side = ctx.input(|input| input.max_texture_side);
        let minimized = ctx.input(|input| input.viewport().minimized).unwrap_or_else(|| previous.is_some_and(|previous| previous.minimized));
        let occluded = ctx.input(|input| input.viewport().occluded).unwrap_or_else(|| previous.is_some_and(|previous| previous.occluded));
        let window_focused = ctx.input(|input| input.focused);
        Some(Self { viewport, pixels_per_point, max_texture_side, minimized, occluded, window_focused })
    }

    /// The pixel dimensions this surface would capture at: viewport points × pixels-per-point, rounded, each axis clamped to `max_texture_side` (0 means unlimited).
    /// `None` when the viewport is unknown/non-finite or an axis overflows `usize`.
    fn expected_size(self) -> Option<[usize; 2]> {
        let viewport = self.viewport?;
        if !viewport.x.is_finite() || !viewport.y.is_finite() || viewport.x <= 0.0 || viewport.y <= 0.0 {
            return None;
        }
        let max = if self.max_texture_side == 0 { usize::MAX } else { self.max_texture_side };
        let axis = |points: f32| -> Option<usize> {
            let pixels = (f64::from(points) * f64::from(self.pixels_per_point)).round();
            if !pixels.is_finite() || pixels <= 0.0 || pixels > usize::MAX as f64 {
                return None;
            }
            Some((pixels as usize).min(max))
        };
        Some([axis(viewport.x)?, axis(viewport.y)?])
    }

    /// Raw `Color32` bytes for `expected_size()` (4 bytes per pixel), with checked multiplication; `None` on overflow.
    fn expected_bytes(self) -> Option<usize> {
        let [width, height] = self.expected_size()?;
        width.checked_mul(height)?.checked_mul(std::mem::size_of::<egui::Color32>())
    }
}

/// The immutable document and widget-focus identity a capture is bound to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CaptureIdentity {
    document: Option<u64>,
    /// Monotonic count of drawing switches in the engine, so A→B→A is still a change.
    document_epoch: u64,
    focus: Option<egui::Id>,
}

/// Viewport geometry plus the immutable document and widget-focus identity a capture is bound to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CaptureGeometry {
    surface: SurfaceGeometry,
    identity: CaptureIdentity,
}

impl CaptureGeometry {
    /// Pixel dimensions of the bound surface, or `None` when it cannot be estimated.
    fn expected_size(self) -> Option<[usize; 2]> {
        self.surface.expected_size()
    }

    /// Raw image byte count of the bound surface, or `None` when it cannot be estimated.
    fn expected_bytes(self) -> Option<usize> {
        self.surface.expected_bytes()
    }
}

struct Shot {
    token: u64,
    path: Option<String>,
    reply: Sender<Value>,
    deadline: Instant,
    ready_at: Instant,
    frames: u32,
    issued: bool,
    identity: CaptureIdentity,
    identity_epoch: u64,
    /// The surface epoch at issue; a later change invalidates the in-flight image.
    surface_epoch: u64,
    geometry: SurfaceGeometry,
    expected: [usize; 2],
    reserved: usize,
    slot: Option<CaptureSlot>,
    callback: Option<egui::ScreenshotCallback>,
}

/// All admitted screenshots. `requests` includes queued, issued and completed-but-undrained
/// captures; nothing is removed before it has been replied to or timed out.
#[derive(Default)]
pub(crate) struct Shots {
    token: u64,
    requests: Vec<Shot>,
}

impl Shots {
    /// True when no queued, in-flight or completed-but-undrained request remains.
    pub(crate) fn is_empty(&self) -> bool {
        self.requests.is_empty()
    }

    /// True when at least one admitted request has not been issued yet; drives the hidden-window repaint request.
    pub(crate) fn has_unissued(&self) -> bool {
        self.requests.iter().any(|shot| !shot.issued)
    }

    /// Saturating sum of the byte reservations of all outstanding requests.
    fn reserved(&self) -> usize {
        self.requests.iter().fold(0usize, |sum, shot| sum.saturating_add(shot.reserved))
    }

    /// Test-only count of outstanding request records.
    #[cfg(test)]
    fn len(&self) -> usize {
        self.requests.len()
    }
}

/// Advance the per-app request token, wrapping and skipping 0.
fn next_token(shots: &mut Shots) -> u64 {
    let mut token = shots.token.wrapping_add(1);
    if token == 0 {
        token = 1;
    }
    shots.token = token;
    token
}

/// The JSON rejection envelope `{ok:false,error,request}` used by admission, issuance, drain and cancellation rejections; `control::save_screenshot` builds an identical envelope for its own failures.
fn error_reply(token: u64, message: impl std::fmt::Display) -> Value {
    json!({"ok": false, "error": message.to_string(), "request": token})
}

/// Send an `error_reply` to `reply`; a closed receiver is ignored so admission never blocks.
fn reject(reply: Sender<Value>, token: u64, message: impl std::fmt::Display) {
    let _ = reply.send(error_reply(token, message));
}

/// UID of the active drawing, or `None` when no drawing is open.
fn active_document(app: &CadApp) -> Option<u64> {
    app.session.docs.get(app.session.active).map(|doc| doc.uid)
}

/// The widget egui currently reports as keyboard-focused, if any.
fn current_focus(ctx: &egui::Context) -> Option<egui::Id> {
    ctx.memory(|memory| memory.focused())
}

/// Snapshot the active drawing UID, the monotonic document epoch and the focused widget together.
fn observed_identity(app: &CadApp, ctx: &egui::Context) -> CaptureIdentity {
    CaptureIdentity { document: active_document(app), document_epoch: app.session.doc_epoch, focus: current_focus(ctx) }
}

/// Store a fresh observation. The surface and identity epochs are independent: a surface change
/// re-baselines a queued request at issue, while any identity change rejects it, including one
/// that returns to the same document or focused widget.
fn observe(app: &mut CadApp, surface: SurfaceGeometry, identity: CaptureIdentity) {
    let previous = app.capture_geometry;
    let surface_changed = previous.is_none_or(|previous| previous.surface != surface);
    let identity_changed = previous.is_none_or(|previous| previous.identity != identity);
    if surface_changed || identity_changed {
        app.capture_geometry = Some(CaptureGeometry { surface, identity });
    }
    if surface_changed {
        app.capture_epoch = app.capture_epoch.wrapping_add(1);
    }
    if identity_changed {
        app.capture_identity_epoch = app.capture_identity_epoch.wrapping_add(1);
    }
}

/// Refresh the fresh raw-input surface/identity snapshot. Called from `raw_input_hook`, so it runs
/// on both UI and hidden logic-only callbacks.
pub(crate) fn note_raw_geometry(app: &mut CadApp, ctx: &egui::Context, raw: &egui::RawInput) {
    let previous = app.capture_geometry.map(|geometry| geometry.surface);
    let Some(surface) = SurfaceGeometry::from_raw(raw, ctx, previous) else {
        if app.capture_geometry.is_some() {
            app.capture_geometry = None;
            app.capture_epoch = app.capture_epoch.wrapping_add(1);
            app.capture_identity_epoch = app.capture_identity_epoch.wrapping_add(1);
        }
        return;
    };
    let identity = observed_identity(app, ctx);
    observe(app, surface, identity);
}

/// Refresh the settled geometry of a UI pass (content rect and ppp), from every pass so a pass
/// that applies the saved interface zoom is the one admission and issuance use.
///
/// An unusable pass snapshot invalidates the retained surface exactly like the raw path: the
/// previous geometry must not survive a pass whose pixels-per-point is invalid, or a later
/// admission could be bound to a surface no current pass renders. Both epochs grow so queued and
/// in-flight captures are re-based or rejected.
pub(crate) fn note_pass_geometry(app: &mut CadApp, ctx: &egui::Context) {
    let previous = app.capture_geometry.map(|geometry| geometry.surface);
    let Some(surface) = SurfaceGeometry::from_pass(ctx, previous) else {
        if app.capture_geometry.is_some() {
            app.capture_geometry = None;
            app.capture_epoch = app.capture_epoch.wrapping_add(1);
            app.capture_identity_epoch = app.capture_identity_epoch.wrapping_add(1);
        }
        return;
    };
    let identity = observed_identity(app, ctx);
    observe(app, surface, identity);
}

/// Refresh only the binding identity. Call after anything that can change the drawing or the
/// keyboard focus within one callback (each control command, opening completion), so an
/// intervening A→B→A transition is still seen.
pub(crate) fn note_identity(app: &mut CadApp, ctx: &egui::Context) {
    let Some(geometry) = app.capture_geometry else { return };
    let identity = observed_identity(app, ctx);
    observe(app, geometry.surface, identity);
}

/// Admit one `ui.screenshot` request without issuing it. Counts queued, in-flight and
/// completed-but-undrained requests and reserves the expected raw image bytes before any capture.
/// The reservation is the admission estimate; issuance re-bases it on the settled pass.
pub(crate) fn admit(app: &mut CadApp, ctx: &egui::Context, path: Option<String>, reply: Sender<Value>) {
    note_identity(app, ctx);
    let token = next_token(&mut app.shots);
    let Some(geometry) = app.capture_geometry else {
        return reject(reply, token, "screenshot failed: the window geometry is not known yet");
    };
    let Some(expected) = geometry.expected_size() else {
        return reject(reply, token, "screenshot failed: the window has no capturable size yet");
    };
    let Some(reserved) = geometry.expected_bytes() else {
        return reject(reply, token, "screenshot failed: the window size overflows the screenshot budget");
    };
    if reserved > MAX_IMAGE_BYTES {
        return reject(
            reply,
            token,
            format!(
                "screenshot budget exceeded: {reserved} bytes for one {}×{} image, over the {MAX_IMAGE_BYTES}-byte per-image limit",
                expected[0], expected[1]
            ),
        );
    }
    if app.shots.requests.len() >= MAX_REQUESTS {
        return reject(reply, token, format!("screenshot budget exceeded: {MAX_REQUESTS} requests are already outstanding"));
    }
    let total = app.shots.reserved().saturating_add(reserved);
    if total > MAX_TOTAL_BYTES {
        return reject(
            reply,
            token,
            format!("screenshot budget exceeded: {total} bytes reserved by outstanding requests, over the {MAX_TOTAL_BYTES}-byte limit"),
        );
    }
    let (slot, callback) = CaptureSlot::new(ctx, ctx.viewport_id(), MAX_IMAGE_BYTES);
    let now = Instant::now();
    app.shots.requests.push(Shot {
        token,
        path,
        reply,
        deadline: now + DEADLINE,
        ready_at: now + READY_DELAY,
        frames: 0,
        issued: false,
        identity: geometry.identity,
        identity_epoch: app.capture_identity_epoch,
        surface_epoch: app.capture_epoch,
        geometry: geometry.surface,
        expected,
        reserved,
        slot: Some(slot),
        callback: Some(callback),
    });
    ctx.request_repaint();
}

/// Human-readable reason when the live document, focus or document epoch differs from the shot's admission identity; `None` while all still match.
fn identity_reason(app: &CadApp, ctx: &egui::Context, shot: &Shot) -> Option<&'static str> {
    if active_document(app) != shot.identity.document {
        Some("the document changed while the capture was queued")
    } else if current_focus(ctx) != shot.identity.focus {
        Some("the keyboard focus changed while the capture was queued")
    } else if app.session.doc_epoch != shot.identity.document_epoch {
        Some("the drawing changed while the capture was queued")
    } else {
        None
    }
}

/// Any observed identity change since admission, including a transition that returned to the
/// same document or widget (`capture_identity_epoch` only ever grows).
fn identity_change(app: &CadApp, ctx: &egui::Context, shot: &Shot) -> Option<&'static str> {
    if app.capture_identity_epoch == shot.identity_epoch {
        return None;
    }
    Some(identity_reason(app, ctx, shot).unwrap_or("the drawing or keyboard focus changed while the capture was queued"))
}

/// Issue settled requests from the first non-discarded UI pass.
///
/// egui discards a pass only when `will_discard()` says so, and the final pass of a multi-pass
/// frame reports false; a hidden `run_logic` tick never reaches `issue` at all.
pub(crate) fn issue(app: &mut CadApp, ctx: &egui::Context) {
    if ctx.will_discard() {
        return;
    }
    // The draw may have moved the focus or switched drawings: observe that before judging.
    note_identity(app, ctx);
    let now = Instant::now();
    let building = app.canvas.building();
    let mut failures: Vec<(usize, String)> = Vec::new();
    for i in 0..app.shots.requests.len() {
        if app.shots.requests[i].issued {
            continue;
        }
        app.shots.requests[i].frames = app.shots.requests[i].frames.saturating_add(1);
        let shot = &app.shots.requests[i];
        if now < shot.ready_at || shot.frames < SETTLE_PASSES || (building && now < shot.ready_at + REBUILD_WAIT) {
            continue;
        }
        if let Some(reason) = identity_change(app, ctx, shot) {
            failures.push((i, reason.to_string()));
            continue;
        }
        // Re-base the expected image on the settled pass. The admission estimate can predate the
        // saved interface zoom, which egui applies at the start of a later pass.
        let Some(current) = app.capture_geometry else {
            failures.push((i, "screenshot failed: the window geometry is no longer known".into()));
            continue;
        };
        let Some(expected) = current.expected_size() else {
            failures.push((i, "screenshot failed: the window has no capturable size".into()));
            continue;
        };
        let Some(reserved) = current.expected_bytes() else {
            failures.push((i, "screenshot failed: the window size overflows the screenshot budget".into()));
            continue;
        };
        if reserved > MAX_IMAGE_BYTES {
            failures.push((
                i,
                format!(
                    "screenshot budget exceeded: {reserved} bytes for one {}×{} image, over the {MAX_IMAGE_BYTES}-byte per-image limit",
                    expected[0], expected[1]
                ),
            ));
            continue;
        }
        let total = app.shots.reserved().saturating_sub(shot.reserved).saturating_add(reserved);
        if total > MAX_TOTAL_BYTES {
            failures.push((
                i,
                format!("screenshot budget exceeded: {total} bytes reserved by outstanding requests, over the {MAX_TOTAL_BYTES}-byte limit"),
            ));
            continue;
        }
        let shot = &mut app.shots.requests[i];
        let Some(callback) = shot.callback.take() else {
            failures.push((i, "screenshot failed: the capture callback was already consumed".into()));
            continue;
        };
        ctx.send_viewport_cmd_to(ctx.viewport_id(), ViewportCommand::Screenshot(callback));
        shot.issued = true;
        shot.identity = current.identity;
        shot.geometry = current.surface;
        shot.expected = expected;
        shot.reserved = reserved;
        shot.surface_epoch = app.capture_epoch;
    }
    for (i, message) in failures.into_iter().rev() {
        let shot = app.shots.requests.remove(i);
        if let Some(slot) = shot.slot {
            slot.cancel();
        }
        reject(shot.reply, shot.token, message);
    }
    if !app.shots.is_empty() {
        ctx.request_repaint_after(REPAINT_INTERVAL);
    }
}

/// Drain completed slots and expire timed-out requests. Call from every logic callback (including
/// hidden ones). The deadline is checked before taking or writing an image.
pub(crate) fn drain(app: &mut CadApp, ctx: &egui::Context) {
    note_identity(app, ctx);
    let mut i = 0;
    while i < app.shots.requests.len() {
        let now = Instant::now();
        if now >= app.shots.requests[i].deadline {
            let shot = app.shots.requests.remove(i);
            if let Some(slot) = shot.slot {
                slot.cancel();
            }
            reject(shot.reply, shot.token, "no frame was presented before the screenshot timeout");
            continue;
        }
        if let Some(reason) = identity_change(app, ctx, &app.shots.requests[i]) {
            let shot = app.shots.requests.remove(i);
            if let Some(slot) = shot.slot {
                slot.cancel();
            }
            reject(shot.reply, shot.token, format!("screenshot failed: {reason}"));
            continue;
        }
        if !app.shots.requests[i].issued {
            // A queued request re-bases its geometry on the settled pass at issue.
            i += 1;
            continue;
        }
        if app.capture_epoch != app.shots.requests[i].surface_epoch
            || app.capture_geometry.map(|geometry| geometry.surface) != Some(app.shots.requests[i].geometry)
        {
            let shot = app.shots.requests.remove(i);
            if let Some(slot) = shot.slot {
                slot.cancel();
            }
            reject(shot.reply, shot.token, "screenshot failed: the window geometry changed while the capture was queued");
            continue;
        }
        let Some(result) = app.shots.requests[i].slot.as_ref().and_then(CaptureSlot::take) else {
            i += 1;
            continue;
        };
        let shot = app.shots.requests.remove(i);
        if let Some(slot) = shot.slot {
            slot.cancel();
        }
        match result {
            Ok(image) => {
                let reply = if image.size != shot.expected {
                    error_reply(
                        shot.token,
                        format!(
                            "screenshot is {}×{} pixels, expected {}×{}: the window changed size while it was captured",
                            image.size[0], image.size[1], shot.expected[0], shot.expected[1]
                        ),
                    )
                } else {
                    crate::control::save_screenshot(&image, shot.path.as_deref(), shot.token)
                };
                let _ = shot.reply.send(reply);
            }
            Err(error) => {
                let _ = shot.reply.send(error_reply(shot.token, error));
            }
        }
    }
    if !app.shots.is_empty() {
        ctx.request_repaint_after(REPAINT_INTERVAL);
    }
}

/// Cancel every queued/in-flight capture (app exit). Late callbacks see a dropped slot and write
/// nothing; clients receive one error reply instead of hanging past the transport timeout.
pub(crate) fn cancel_all(app: &mut CadApp) {
    for shot in app.shots.requests.drain(..) {
        if let Some(slot) = shot.slot {
            slot.cancel();
        }
        let _ = shot.reply.send(error_reply(shot.token, "screenshot cancelled: the app is closing"));
    }
}

/// Ask for a painted frame while a request is still queued, and on hidden logic ticks when a
/// capture is waiting for settlement. Native eframe honours `RequestPaintWhileHidden`; the web
/// runner currently logs it as unhandled, so web only schedules a repaint and hidden web captures
/// still reach their timeout (a documented limitation, not a success path).
pub(crate) fn request_hidden_paint(app: &CadApp, ctx: &egui::Context) {
    if app.shots.has_unissued() {
        #[cfg(not(target_arch = "wasm32"))]
        ctx.send_viewport_cmd(ViewportCommand::RequestPaintWhileHidden);
        ctx.request_repaint();
    }
}

#[cfg(test)]
mod tests;
