//! Host-independent 3D mouse preferences and navigation. Device I/O lives in the desktop app.

use cadcraft_engine::View;
use cadcraft_geom::Vec2;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::CadApp;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub enabled: bool,
    /// Visible extents per second at full deflection.
    pub pan_speed: f64,
    /// Exponential zoom rate per second at full deflection.
    pub zoom_speed: f64,
    pub reverse_zoom: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self { enabled: true, pan_speed: 0.8, zoom_speed: 1.6, reverse_zoom: false }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Status {
    #[default]
    Unavailable,
    Searching,
    Calibrating,
    Connected,
    OpenFailed,
    Disconnected,
    Error,
}

impl Status {
    pub fn message(self) -> &'static str {
        match self {
            Self::Unavailable => "3D mouse input is unavailable on this platform",
            Self::Searching => "No supported 3D mouse found",
            Self::Calibrating => "Release the 3D mouse to calibrate its resting position",
            Self::Connected => "3D mouse connected",
            Self::OpenFailed => "3D mouse busy, possibly 3DxWare or missing device permissions",
            Self::Disconnected => "3D mouse disconnected",
            Self::Error => "3D mouse input could not be started",
        }
    }
}

#[derive(Default)]
pub struct State {
    pub settings: Settings,
    pub status: Status,
    pub device: Option<String>,
    pub detail: Option<String>,
    /// The native host consumes this request; no device I/O runs on the UI thread.
    pub calibrate_requested: bool,
}

impl State {
    pub fn load_settings(&mut self, value: &Value) -> Result<(), String> {
        let settings: Settings = serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
        if !settings.pan_speed.is_finite()
            || !(0.1..=3.0).contains(&settings.pan_speed)
            || !settings.zoom_speed.is_finite()
            || !(0.1..=3.0).contains(&settings.zoom_speed)
        {
            return Err("3D mouse sensitivity must be between 0.1 and 3.0".into());
        }
        self.settings = settings;
        Ok(())
    }

    fn summary(&self) -> Value {
        json!({"settings": self.settings, "status": self.status, "device": self.device, "detail": self.detail})
    }
}

pub fn route(app: &mut CadApp, id: &str, params: &Value) -> Option<Result<Value, String>> {
    match id {
        "ui.spacemouse" => {
            if params.is_null() {
                app.ui.dialog = Some("spacemouse".into());
            } else if let Some(settings) = params.get("settings")
                && let Err(error) = app.spacemouse.load_settings(settings)
            {
                return Some(Err(error));
            }
            Some(Ok(app.spacemouse.summary()))
        }
        "ui.spacemouse.calibrate" => {
            if !matches!(app.spacemouse.status, Status::Connected | Status::Calibrating) {
                return Some(Err("Connect a 3D mouse before calibrating".into()));
            }
            app.spacemouse.calibrate_requested = true;
            Some(Ok(Value::Null))
        }
        "ui.spacemouse.navigate" => Some(navigate(app, params)),
        _ => None,
    }
}

/// One frame of fractional pan and magnification, shared by device input and the control API.
/// Quiet view updates keep model/paper/tiled/floating viewport handling in the engine and do
/// not flood ZOOM Previous's history with one entry per frame.
fn navigate(app: &mut CadApp, params: &Value) -> Result<Value, String> {
    let pan = params.get("pan").and_then(Value::as_array).filter(|p| p.len() == 2);
    let x = pan.and_then(|p| p.first()).and_then(Value::as_f64);
    let y = pan.and_then(|p| p.get(1)).and_then(Value::as_f64);
    let zoom = params.get("zoom").and_then(Value::as_f64);
    let (Some(x), Some(y), Some(zoom)) = (x, y, zoom) else { return Err("Expected {pan: [x, y], zoom: factor}".into()) };
    if !x.is_finite() || !y.is_finite() || x.abs() > 0.25 || y.abs() > 0.25 || !zoom.is_finite() || !(0.5..=2.0).contains(&zoom) {
        return Err("3D mouse navigation frame is outside its limits".into());
    }
    if !app.spacemouse.settings.enabled {
        return Ok(Value::Null);
    }
    let (width, height) = app.session.viewport_px;
    let aspect = width / height.max(1.0);
    let state = app.session.state_mut().map_err(|e| e.to_string())?;
    let view = state.view();
    let center = view.center - Vec2::new(x * view.height * aspect, y * view.height);
    let height = (view.height / zoom).clamp(1e-9, 1e12);
    if !center.is_finite() || !height.is_finite() {
        return Err("3D mouse navigation would produce an invalid view".into());
    }
    state.set_view_quiet(View { center, height });
    Ok(json!({"center": [center.x, center.y], "height": height}))
}

/// 3D Mouse window showing connection status, detected device/detail and the navigation settings, with a Calibrate action.
pub fn dialog(app: &mut CadApp, ctx: &egui::Context, open: &mut bool) {
    egui::Window::new(crate::tl!("3D Mouse")).id(egui::Id::unique("spacemouse")).open(open).resizable(false).show(ctx, |ui| {
        ui.label(crate::i18n::t(app.spacemouse.status.message()));
        if let Some(device) = &app.spacemouse.device {
            ui.label(device);
        }
        if let Some(detail) = &app.spacemouse.detail {
            ui.label(detail);
        }
        ui.separator();
        let mut settings = app.spacemouse.settings;
        let mut changed = ui.checkbox(&mut settings.enabled, crate::tl!("Enable 3D mouse navigation")).changed();
        changed |= ui.add(egui::Slider::new(&mut settings.pan_speed, 0.1..=3.0).text(crate::tl!("Pan sensitivity"))).changed();
        changed |= ui.add(egui::Slider::new(&mut settings.zoom_speed, 0.1..=3.0).text(crate::tl!("Zoom sensitivity"))).changed();
        changed |= ui.checkbox(&mut settings.reverse_zoom, crate::tl!("Reverse zoom direction")).changed();
        if changed {
            let _ = app.run("ui.spacemouse", json!({"settings": settings}));
        }
        if ui
            .add_enabled(matches!(app.spacemouse.status, Status::Connected | Status::Calibrating), egui::Button::new(crate::tl!("Calibrate")))
            .clicked()
        {
            let _ = app.run("ui.spacemouse.calibrate", Value::Null);
        }
        ui.label(crate::tl!("Move sideways to pan; push/pull or twist to zoom."));
        ui.label(crate::tl!("Left device button: zoom extents. Right device button: calibrate."));
    });
}
