//! `window.cadcraft`: the page command API (`craft-web-api`, storytold/craft-libs#413) over the app's
//! own control channel, the one the desktop control server drives (`docs/control-protocol.md`).
//!
//! The crate owns the JavaScript side (`request`, `execute`, `commands`, `docInspect`, `uiInspect`,
//! `screenshot`, as promises with a timeout). This module is CADCraft's part: a [`Request`] that
//! queues a [`ControlRequest`] for the app, which answers it on its next frame, and waits for the
//! reply. A call the page gave up on drops its [`PendingReply`], which cancels a request the app has
//! not started, so the app skips it instead of running it late.

use std::rc::Rc;
use std::sync::mpsc::{Receiver, channel};

use cadcraft_ui_egui::{ControlRequest, PendingReply};
use craft_web_api::methods::Methods;
use craft_web_api::page::{PageApi, Request};
use serde_json::{Value, json};
use wasm_bindgen_futures::JsFuture;

/// The app id in `window.<app>`.
const APP: &str = "cadcraft";

/// How often a waiting call looks for the app's reply, in milliseconds (about a frame).
const POLL_MS: i32 = 16;

/// Put `window.cadcraft` on the page and return the queue the app drains each frame (hand it to
/// `CadApp::with_control`). An existing `window.cadcraft` keeps the functions it already has.
pub fn install(ctx: egui::Context) -> Receiver<ControlRequest> {
    let (queue, requests) = channel::<ControlRequest>();
    let request: Request = Rc::new(move |method, params| {
        let (control, reply) = ControlRequest::new(method, params);
        if queue.send(control).is_err() {
            return Box::pin(async { json!({"ok": false, "error": "the app is not running"}) });
        }
        ctx.request_repaint();
        Box::pin(wait(reply))
    });
    if let Err(e) = Rc::new(PageApi::new(APP, Methods::default(), request)).install() {
        log::warn!("couldn't install window.{APP}: {e:?}");
    }
    requests
}

/// The app's reply, polled about once a frame. Dropping this future (the page's timeout) drops
/// `reply`, which cancels the request if the app has not started it.
async fn wait(reply: PendingReply) -> Value {
    loop {
        if let Some(reply) = reply.try_reply() {
            return reply;
        }
        sleep(POLL_MS).await;
    }
}

async fn sleep(ms: i32) {
    let promise = js_sys::Promise::new(&mut |resolve, _| {
        if let Some(window) = web_sys::window() {
            let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms);
        }
    });
    let _ = JsFuture::from(promise).await;
}
