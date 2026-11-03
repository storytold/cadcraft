//! Native 3Dconnexion input. The reader publishes one latest sample, never an input queue;
//! only movement starting/stopping or status/buttons wake egui. Active motion runs at 60 Hz.

#[cfg(any(target_os = "windows", target_os = "linux"))]
mod motion;

use cadcraft_ui_egui::{
    CadApp,
    spacemouse::{Status, bindings},
};
#[cfg(any(target_os = "windows", target_os = "linux"))]
use std::time::Duration;
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

#[derive(Default)]
struct Shared {
    status: Status,
    device: Option<String>,
    detail: Option<String>,
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    motion: motion::Motion,
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    last_report: Option<Instant>,
    /// Buttons that went down since the last poll (bit 0 is button 1).
    pressed: u32,
    /// Buttons down now.
    held: u32,
    recalibrate: bool,
}

pub struct SpaceMouse {
    shared: Arc<Mutex<Shared>>,
    stop: Arc<AtomicBool>,
    last_frame: Instant,
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    moving: bool,
}

impl SpaceMouse {
    pub fn start(ctx: egui::Context) -> Self {
        let shared = Arc::new(Mutex::new(Shared::default()));
        let stop = Arc::new(AtomicBool::new(false));
        #[cfg(any(target_os = "windows", target_os = "linux"))]
        {
            let (input, quit, context) = (Arc::clone(&shared), Arc::clone(&stop), ctx.clone());
            if let Err(error) = std::thread::Builder::new().name("spacemouse-hid".into()).spawn(move || read(input, quit, context)) {
                set_status(&shared, &ctx, Status::Error, None, Some(error.to_string()));
            }
        }
        #[cfg(not(any(target_os = "windows", target_os = "linux")))]
        let _ = ctx;
        Self {
            shared,
            stop,
            last_frame: Instant::now(),
            #[cfg(any(target_os = "windows", target_os = "linux"))]
            moving: false,
        }
    }

    pub fn poll(&mut self, app: &mut CadApp, ctx: &egui::Context) {
        let now = Instant::now();
        let elapsed = now.saturating_duration_since(self.last_frame).as_secs_f64().min(0.05);
        self.last_frame = now;
        let Ok(mut shared) = self.shared.lock() else { return };
        if std::mem::take(&mut app.spacemouse.calibrate_requested) {
            shared.recalibrate = true;
        }
        let changed = app.spacemouse.status != shared.status || app.spacemouse.device != shared.device || app.spacemouse.detail != shared.detail;
        app.spacemouse.status = shared.status;
        app.spacemouse.device.clone_from(&shared.device);
        app.spacemouse.detail.clone_from(&shared.detail);
        if changed && !matches!(shared.status, Status::Unavailable | Status::Searching) {
            app.set_status(cadcraft_ui_egui::i18n::tr(app.language(), shared.status.message()));
        }
        let (pressed, held) = (std::mem::take(&mut shared.pressed), shared.held);
        #[cfg(any(target_os = "windows", target_os = "linux"))]
        let motion = if shared.last_report.is_some_and(|at| now.saturating_duration_since(at) < Duration::from_millis(250)) {
            shared.motion
        } else {
            motion::Motion::default()
        };
        drop(shared);
        app.spacemouse.held = held;
        let highest = u8::try_from(u32::BITS - (held | pressed).leading_zeros()).unwrap_or(u8::MAX);
        app.spacemouse.highest_button = app.spacemouse.highest_button.max(highest);
        let allowed = app.spacemouse.settings.enabled
            && ctx.input(|i| i.focused)
            && !app.ui.start_tab
            && app.ui.dialog.is_none()
            && app.closing.is_none()
            && app.canvas.rect.is_some()
            && !egui::Popup::is_any_open(ctx);
        // Pressed edges run their bindings (recalibrating even when navigation is not allowed).
        let fired: Vec<_> = bindings::fired(&app.spacemouse.settings.buttons, pressed, allowed).into_iter().cloned().collect();
        for binding in fired {
            if let Err(error) = app.run(&binding.command, binding.run_params()) {
                app.set_status(error);
            }
        }
        // A bound `ui.spacemouse.calibrate` raised the request after the check above.
        self.forward_calibrate_request(app);
        #[cfg(any(target_os = "windows", target_os = "linux"))]
        {
            let moving = allowed && motion.active();
            if moving {
                if self.moving {
                    let settings = &app.spacemouse.settings;
                    let rate = if settings.reverse_zoom { -settings.zoom_speed } else { settings.zoom_speed };
                    let flip = |inverted: bool| if inverted { -1.0 } else { 1.0 };
                    let pan = [
                        motion.pan[0] * settings.pan_speed * elapsed * flip(settings.invert_pan_x),
                        motion.pan[1] * settings.pan_speed * elapsed * flip(settings.invert_pan_y),
                    ];
                    if let Err(error) =
                        app.run("ui.spacemouse.navigate", serde_json::json!({"pan": pan, "zoom": (motion.zoom * rate * elapsed).exp()}))
                    {
                        app.set_status(error);
                    }
                }
                ctx.request_repaint_after(Duration::from_millis(16));
            }
            self.moving = moving;
        }
        #[cfg(not(any(target_os = "windows", target_os = "linux")))]
        let _ = elapsed;
    }
}

impl SpaceMouse {
    /// Passes a pending Calibrate request (the dialog button or a bound button) to the device thread.
    fn forward_calibrate_request(&self, app: &mut CadApp) {
        if std::mem::take(&mut app.spacemouse.calibrate_requested)
            && let Ok(mut shared) = self.shared.lock()
        {
            shared.recalibrate = true;
        }
    }
}

impl Drop for SpaceMouse {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
fn set_status(shared: &Mutex<Shared>, ctx: &egui::Context, status: Status, device: Option<String>, detail: Option<String>) {
    let Ok(mut state) = shared.lock() else { return };
    let changed = state.status != status || state.device != device || state.detail != detail;
    if !changed {
        return;
    }
    if let Some(error) = &detail {
        log::warn!("3D mouse: {error}");
    }
    state.status = status;
    state.device = device;
    state.detail = detail;
    state.motion = motion::Motion::default();
    state.last_report = None;
    state.pressed = 0;
    state.held = 0;
    drop(state);
    ctx.request_repaint();
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
fn retry(stop: &AtomicBool) {
    for _ in 0..40 {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
fn read(shared: Arc<Mutex<Shared>>, stop: Arc<AtomicBool>, ctx: egui::Context) {
    while !stop.load(Ordering::Relaxed) {
        let mut api = match hidapi::HidApi::new() {
            Ok(api) => api,
            Err(error) => {
                set_status(&shared, &ctx, Status::Error, None, Some(error.to_string()));
                retry(&stop);
                continue;
            }
        };
        while !stop.load(Ordering::Relaxed) {
            if let Err(error) = api.refresh_devices() {
                set_status(&shared, &ctx, Status::Error, None, Some(error.to_string()));
                break;
            }
            let device =
                api.device_list().find(|device| matches!(device.vendor_id(), 0x046d | 0x256f) && device.usage_page() == 1 && device.usage() == 8);
            let Some(info) = device else {
                set_status(&shared, &ctx, Status::Searching, None, None);
                retry(&stop);
                continue;
            };
            let name = info.product_string().unwrap_or("3Dconnexion SpaceMouse").to_string();
            match info.open_device(&api) {
                Ok(device) => {
                    set_status(&shared, &ctx, Status::Calibrating, Some(name.clone()), None);
                    if let Err(error) = read_device(&device, &shared, &stop, &ctx, &name) {
                        set_status(&shared, &ctx, Status::Disconnected, Some(name), Some(error.to_string()));
                    }
                }
                Err(error) => set_status(&shared, &ctx, Status::OpenFailed, Some(name), Some(error.to_string())),
            }
            retry(&stop);
        }
        retry(&stop);
    }
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
fn read_device(device: &hidapi::HidDevice, shared: &Mutex<Shared>, stop: &AtomicBool, ctx: &egui::Context, name: &str) -> hidapi::HidResult<()> {
    let mut filter = motion::Filter::default();
    let mut buffer = [0; 64];
    let mut buttons = 0;
    while !stop.load(Ordering::Relaxed) {
        let recalibrate = if let Ok(mut state) = shared.lock() { std::mem::take(&mut state.recalibrate) } else { return Ok(()) };
        if recalibrate {
            filter.recalibrate();
            set_status(shared, ctx, Status::Calibrating, Some(name.into()), None);
        }
        let len = device.read_timeout(&mut buffer, 50)?;
        let now = Instant::now();
        let report = buffer.get(..len).and_then(motion::decode);
        let Ok(mut state) = shared.lock() else { return Ok(()) };
        let mut wake = false;
        match report {
            Some(motion::Report::Buttons(next)) => {
                // Which button does what is the UI thread's business (`bindings::fired`); the
                // device thread only reports edges and what is held.
                state.pressed |= next & !buttons;
                state.held = next;
                wake = next != buttons;
                buttons = next;
            }
            Some(motion::Report::Axes { values, mask }) => {
                let motion = filter.update(values, mask, now);
                wake = state.motion.active() != motion.active();
                state.motion = motion;
                state.last_report = Some(now);
                if filter.calibrated() && state.status != Status::Connected {
                    state.status = Status::Connected;
                    wake = true;
                }
            }
            None => {
                if state.motion.active() && state.last_report.is_some_and(|at| now.saturating_duration_since(at) >= Duration::from_millis(250)) {
                    state.motion = motion::Motion::default();
                    wake = true;
                }
            }
        }
        drop(state);
        if wake {
            ctx.request_repaint();
        }
    }
    Ok(())
}
