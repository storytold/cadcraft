//! Online XYZ raster basemaps in model space. A geographic reference ties one CAD point to
//! latitude/longitude; drawing units are scaled to Web Mercator metres at that point.

use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, Sender};

use cadcraft_geom::Vec2;
use egui::{Color32, ColorImage, FontId, Pos2, Rect, TextureHandle, TextureOptions, pos2};
use serde_json::{Value, json};

use crate::{CadApp, canvas::Xf};

const HALF_WORLD: f64 = 20_037_508.342_789_244;
const MAX_TILES: usize = 64;
const MAX_IN_FLIGHT: usize = 4;

struct Source {
    id: &'static str,
    name: &'static str,
    template: &'static str,
    attribution: &'static str,
    key_required: bool,
    max_zoom: u8,
}

const SOURCES: &[Source] = &[
    Source {
        id: "osm",
        name: "OpenStreetMap Standard (no key)",
        template: "https://tile.openstreetmap.org/{z}/{x}/{y}.png",
        attribution: "© OpenStreetMap contributors",
        key_required: false,
        max_zoom: 19,
    },
    Source {
        id: "opentopo",
        name: "OpenTopoMap (no key)",
        template: "https://a.tile.opentopomap.org/{z}/{x}/{y}.png",
        attribution: "© OpenStreetMap contributors, SRTM | © OpenTopoMap (CC BY-SA)",
        key_required: false,
        max_zoom: 17,
    },
    Source {
        id: "nasa-blue-marble",
        name: "NASA Blue Marble (satellite, no key)",
        template: "https://gibs.earthdata.nasa.gov/wmts/epsg3857/best/BlueMarble_NextGeneration/default/GoogleMapsCompatible_Level8/{z}/{y}/{x}.jpeg",
        attribution: "Imagery: NASA GIBS / ESDIS",
        key_required: false,
        max_zoom: 8,
    },
    Source {
        id: "carto-voyager",
        name: "CARTO Voyager (API key)",
        template: "https://basemaps.cartocdn.com/rastertiles/voyager/{z}/{x}/{y}.png?key={key}",
        attribution: "© OpenStreetMap contributors, © CARTO",
        key_required: true,
        max_zoom: 20,
    },
    Source {
        id: "carto-positron",
        name: "CARTO Positron (API key)",
        template: "https://basemaps.cartocdn.com/rastertiles/light_all/{z}/{x}/{y}.png?key={key}",
        attribution: "© OpenStreetMap contributors, © CARTO",
        key_required: true,
        max_zoom: 20,
    },
    Source {
        id: "carto-dark",
        name: "CARTO Dark Matter (API key)",
        template: "https://basemaps.cartocdn.com/rastertiles/dark_all/{z}/{x}/{y}.png?key={key}",
        attribution: "© OpenStreetMap contributors, © CARTO",
        key_required: true,
        max_zoom: 20,
    },
];

#[derive(Clone)]
pub struct Config {
    pub source: String,
    pub key: String,
    pub template: String,
    pub attribution: String,
    pub latitude: f64,
    pub longitude: f64,
    pub anchor: Vec2,
    /// Real-world metres represented by one unit of the CAD drawing.
    pub metres_per_unit: f64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            source: "osm".into(),
            key: String::new(),
            template: String::new(),
            attribution: String::new(),
            latitude: 0.0,
            longitude: 0.0,
            anchor: Vec2::new(0.0, 0.0),
            metres_per_unit: 1.0,
        }
    }
}

impl Config {
    fn source(&self) -> Option<&Source> {
        SOURCES.iter().find(|s| s.id == self.source)
    }

    fn template(&self) -> &str {
        self.source().map_or(self.template.as_str(), |s| s.template)
    }

    fn attribution(&self) -> &str {
        self.source().map_or(self.attribution.as_str(), |s| s.attribution)
    }

    fn validate(&self) -> Result<(), String> {
        if !self.latitude.is_finite() || self.latitude.abs() > 85.051_128_78 {
            return Err("Latitude must be between -85.0511 and 85.0511 degrees".into());
        }
        if !self.longitude.is_finite() || self.longitude.abs() > 180.0 {
            return Err("Longitude must be between -180 and 180 degrees".into());
        }
        if !self.anchor.is_finite() || self.anchor.x.abs() > 1e12 || self.anchor.y.abs() > 1e12 {
            return Err("CAD anchor must be finite and within ±1e12 units".into());
        }
        if !self.metres_per_unit.is_finite() || !(1e-9..=1e9).contains(&self.metres_per_unit) {
            return Err("Metres per CAD unit must be between 1e-9 and 1e9".into());
        }
        let template = self.template();
        if !template.starts_with("https://") || !["{z}", "{x}", "{y}"].iter().all(|p| template.contains(p)) {
            return Err("XYZ URL must use HTTPS and contain {z}, {x}, and {y}".into());
        }
        if self.attribution().trim().is_empty() {
            return Err("Map attribution is required".into());
        }
        if (self.source().is_some_and(|s| s.key_required) || template.contains("{key}")) && self.key.is_empty() {
            return Err("This map source requires an API key".into());
        }
        if !self.key.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '~')) {
            return Err("API key contains characters that cannot be placed in a URL".into());
        }
        Ok(())
    }
}

#[derive(Hash, PartialEq, Eq, Clone)]
struct TileKey {
    generation: u64,
    source: String,
    z: u8,
    x: u32,
    y: u32,
}

enum Tile {
    Loading,
    Ready(TextureHandle),
    Failed(String),
}

pub struct State {
    pub config: Config,
    draft: Config,
    pub enabled: bool,
    pub error: Option<String>,
    drawing_uid: Option<u64>,
    generation: u64,
    tiles: HashMap<TileKey, Tile>,
    tx: Sender<(TileKey, Result<Vec<u8>, String>)>,
    rx: Receiver<(TileKey, Result<Vec<u8>, String>)>,
}

impl Default for State {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            config: Config::default(),
            draft: Config::default(),
            enabled: false,
            error: None,
            drawing_uid: None,
            generation: 0,
            tiles: HashMap::new(),
            tx,
            rx,
        }
    }
}

pub fn command(app: &mut CadApp, params: &Value) -> Result<Value, String> {
    if params.is_null() {
        app.geomap.draft = app.geomap.config.clone();
        app.geomap.error = None;
        app.ui.dialog = Some("geomap".into());
        return Ok(Value::Null);
    }
    if params.get("on").and_then(Value::as_bool) == Some(false) {
        app.geomap.enabled = false;
        app.geomap.generation = app.geomap.generation.wrapping_add(1);
        app.geomap.tiles.clear();
        return Ok(json!({"on": false}));
    }
    let mut next = app.geomap.config.clone();
    if let Some(s) = params.get("source").and_then(Value::as_str) {
        if s != "custom" && !SOURCES.iter().any(|p| p.id == s) {
            return Err(format!("Unknown map source: {s}"));
        }
        next.source = s.into();
    }
    for (name, field) in [("key", &mut next.key), ("url", &mut next.template), ("attribution", &mut next.attribution)] {
        if let Some(value) = params.get(name).and_then(Value::as_str) {
            *field = value.into();
        }
    }
    for (name, field) in [
        ("latitude", &mut next.latitude),
        ("longitude", &mut next.longitude),
        ("x", &mut next.anchor.x),
        ("y", &mut next.anchor.y),
        ("metresPerUnit", &mut next.metres_per_unit),
    ] {
        if let Some(value) = params.get(name).and_then(Value::as_f64) {
            *field = value;
        }
    }
    next.validate()?;
    let uid = app.session.state().map_err(|e| e.to_string())?.uid;
    app.geomap.config = next;
    app.geomap.drawing_uid = Some(uid);
    app.geomap.enabled = true;
    app.geomap.generation = app.geomap.generation.wrapping_add(1);
    app.geomap.error = None;
    app.geomap.tiles.clear();
    Ok(json!({"on": true, "source": app.geomap.config.source}))
}

pub fn dialog(app: &mut CadApp, ctx: &egui::Context, open: &mut bool) {
    let mut apply = false;
    let mut off = false;
    egui::Window::new("Online Map (GEOMAP)").open(open).resizable(true).show(ctx, |ui| {
        ui.label("Choose an XYZ basemap for the model-space background.");
        ui.small("Google Maps/Hybrid and Bing do not offer supported keyless tile access. Custom XYZ requires a provider URL you may use.");
        egui::ComboBox::from_id_salt("geomap_source")
            .selected_text(SOURCES.iter().find(|s| s.id == app.geomap.draft.source).map_or("Custom XYZ", |s| s.name))
            .show_ui(ui, |ui| {
                for source in SOURCES {
                    ui.selectable_value(&mut app.geomap.draft.source, source.id.into(), source.name);
                }
                ui.selectable_value(&mut app.geomap.draft.source, "custom".into(), "Custom XYZ");
            });
        if app.geomap.draft.source == "custom" {
            ui.label("HTTPS URL with {z}/{x}/{y} placeholders:");
            ui.text_edit_singleline(&mut app.geomap.draft.template);
            ui.label("Attribution (shown on map):");
            ui.text_edit_singleline(&mut app.geomap.draft.attribution);
        }
        if app.geomap.draft.source().is_some_and(|s| s.key_required) || app.geomap.draft.template().contains("{key}") {
            ui.label("Provider API key:");
            ui.add(egui::TextEdit::singleline(&mut app.geomap.draft.key).password(true));
        }
        ui.separator();
        ui.label("Reference point: geographic position of one CAD point (WGS84).");
        egui::Grid::new("geomap_reference").num_columns(2).show(ui, |ui| {
            ui.label("Latitude (°)");
            ui.add(egui::DragValue::new(&mut app.geomap.draft.latitude).speed(0.0001));
            ui.end_row();
            ui.label("Longitude (°)");
            ui.add(egui::DragValue::new(&mut app.geomap.draft.longitude).speed(0.0001));
            ui.end_row();
            ui.label("CAD X");
            ui.add(egui::DragValue::new(&mut app.geomap.draft.anchor.x));
            ui.end_row();
            ui.label("CAD Y");
            ui.add(egui::DragValue::new(&mut app.geomap.draft.anchor.y));
            ui.end_row();
            ui.label("Metres per CAD unit");
            ui.add(egui::DragValue::new(&mut app.geomap.draft.metres_per_unit).range(0.000001..=1e9));
            ui.end_row();
        });
        if ui.button("Use current view center as CAD point").clicked()
            && let Ok(st) = app.session.state()
        {
            app.geomap.draft.anchor = st.view().center;
        }
        ui.small(
            "The drawing must use east/right and north/up coordinates. This is a local Web Mercator alignment, not a general CRS transformation.",
        );
        if let Some(error) = &app.geomap.error {
            ui.colored_label(Color32::RED, error);
        }
        if let Some(error) = app.geomap.tiles.values().find_map(|tile| match tile {
            Tile::Failed(error) => Some(error),
            _ => None,
        }) {
            ui.colored_label(Color32::RED, format!("Map tile: {error}"));
        }
        ui.horizontal(|ui| {
            apply = ui.button("Show map").clicked();
            off = ui.button("Hide map").clicked();
        });
        ui.small(app.geomap.draft.attribution());
    });
    if apply {
        match app.geomap.draft.validate().and_then(|_| app.session.state().map(|s| s.uid).map_err(|e| e.to_string())) {
            Ok(uid) => {
                app.geomap.config = app.geomap.draft.clone();
                app.geomap.drawing_uid = Some(uid);
                app.geomap.enabled = true;
                app.geomap.generation = app.geomap.generation.wrapping_add(1);
                app.geomap.error = None;
                app.geomap.tiles.clear();
                *open = false;
            }
            Err(e) => app.geomap.error = Some(e),
        }
    }
    if off {
        app.geomap.enabled = false;
        app.geomap.generation = app.geomap.generation.wrapping_add(1);
        app.geomap.tiles.clear();
        *open = false;
    }
}

fn mercator(latitude: f64, longitude: f64) -> Vec2 {
    let lat = latitude.to_radians();
    Vec2::new(HALF_WORLD * longitude / 180.0, HALF_WORLD * ((std::f64::consts::FRAC_PI_4 + lat / 2.0).tan().ln()) / std::f64::consts::PI)
}

fn tile_range(xf: &Xf, config: &Config, zoom: u8) -> Option<(u32, u32, u32, u32)> {
    let n = 1_u32 << zoom;
    let anchor = mercator(config.latitude, config.longitude);
    let bounds = xf.world_bounds();
    let scale = projected_metres_per_unit(config);
    let mx0 = anchor.x + (bounds.min.x - config.anchor.x) * scale;
    let mx1 = anchor.x + (bounds.max.x - config.anchor.x) * scale;
    let my0 = anchor.y + (bounds.min.y - config.anchor.y) * scale;
    let my1 = anchor.y + (bounds.max.y - config.anchor.y) * scale;
    let edge = 2.0 * HALF_WORLD;
    let x0 = ((mx0 + HALF_WORLD) / edge * f64::from(n)).floor() as i64;
    let x1 = ((mx1 + HALF_WORLD) / edge * f64::from(n)).floor() as i64;
    let y0 = ((HALF_WORLD - my1) / edge * f64::from(n)).floor() as i64;
    let y1 = ((HALF_WORLD - my0) / edge * f64::from(n)).floor() as i64;
    if x1 < 0 || y1 < 0 || x0 >= i64::from(n) || y0 >= i64::from(n) {
        return None;
    }
    let clamp = |v: i64| v.clamp(0, i64::from(n) - 1) as u32;
    Some((clamp(x0), clamp(x1), clamp(y0), clamp(y1)))
}

fn projected_metres_per_unit(config: &Config) -> f64 {
    config.metres_per_unit / config.latitude.to_radians().cos()
}

pub fn paint(app: &mut CadApp, painter: &egui::Painter, xf: &Xf) {
    if !app.geomap.enabled {
        return;
    }
    let Ok(st) = app.session.state() else { return };
    if app.geomap.drawing_uid != Some(st.uid) {
        app.geomap.enabled = false;
        app.geomap.generation = app.geomap.generation.wrapping_add(1);
        app.geomap.tiles.clear();
        return;
    }
    let map = &mut app.geomap;
    while let Ok((key, result)) = map.rx.try_recv() {
        if !map.tiles.contains_key(&key) {
            continue;
        }
        let tile = result.and_then(|bytes| {
            let rgba = image::load_from_memory(&bytes).map_err(|e| e.to_string())?.into_rgba8();
            let (w, h) = rgba.dimensions();
            if w > 1024 || h > 1024 {
                return Err("Map tile exceeds 1024 pixels".into());
            }
            let image = ColorImage::from_rgba_unmultiplied([w as usize, h as usize], rgba.as_raw());
            Ok(painter.ctx().load_texture(
                format!("geomap-{}-{}-{}-{}-{}", key.generation, key.source, key.z, key.x, key.y),
                image,
                TextureOptions::LINEAR,
            ))
        });
        map.tiles.insert(
            key,
            match tile {
                Ok(texture) => Tile::Ready(texture),
                Err(e) => Tile::Failed(e),
            },
        );
        painter.ctx().request_repaint();
    }
    let config = &map.config;
    let projected_scale = projected_metres_per_unit(config);
    let desired = (2.0 * HALF_WORLD * xf.scale / (256.0 * projected_scale)).log2();
    let mut zoom = desired.round().clamp(0.0, f64::from(config.source().map_or(19, |s| s.max_zoom))) as u8;
    let range = loop {
        let Some(range) = tile_range(xf, config, zoom) else { return };
        let count = u64::from(range.1 - range.0 + 1) * u64::from(range.3 - range.2 + 1);
        if count <= MAX_TILES as u64 || zoom == 0 {
            break range;
        }
        zoom -= 1;
    };
    let anchor = mercator(config.latitude, config.longitude);
    let world_tile = 2.0 * HALF_WORLD / f64::from(1_u32 << zoom);
    let source = config.template().to_owned();
    let key_text = config.key.clone();
    let mut in_flight = map.tiles.values().filter(|v| matches!(v, Tile::Loading)).count();
    for y in range.2..=range.3 {
        for x in range.0..=range.1 {
            let key = TileKey { generation: map.generation, source: source.clone(), z: zoom, x, y };
            if !map.tiles.contains_key(&key) && in_flight < MAX_IN_FLIGHT {
                let url = source
                    .replace("{z}", &zoom.to_string())
                    .replace("{x}", &x.to_string())
                    .replace("{y}", &y.to_string())
                    .replace("{key}", &key_text);
                map.tiles.insert(key.clone(), Tile::Loading);
                let disk_cache = matches!(config.source.as_str(), "osm" | "opentopo" | "nasa-blue-marble");
                fetch(url, key.clone(), disk_cache, map.tx.clone(), painter.ctx().clone());
                in_flight += 1;
            }
            if let Some(Tile::Ready(texture)) = map.tiles.get(&key) {
                let left = (-HALF_WORLD + f64::from(x) * world_tile - anchor.x) / projected_scale + config.anchor.x;
                let right = left + world_tile / projected_scale;
                let top = (HALF_WORLD - f64::from(y) * world_tile - anchor.y) / projected_scale + config.anchor.y;
                let bottom = top - world_tile / projected_scale;
                let rect = Rect::from_two_pos(xf.to_screen(Vec2::new(left, top)), xf.to_screen(Vec2::new(right, bottom)));
                painter.image(texture.id(), rect, Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)), Color32::WHITE);
            }
        }
    }
    if map.tiles.len() > 256 {
        map.tiles
            .retain(|key, _| key.source == source && key.z == zoom && key.x >= range.0 && key.x <= range.1 && key.y >= range.2 && key.y <= range.3);
    }
}

pub fn paint_attribution(app: &CadApp, painter: &egui::Painter) {
    if !app.geomap.enabled {
        return;
    }
    let label = app.geomap.config.attribution();
    let at = painter.clip_rect().right_bottom() - egui::vec2(8.0, 8.0);
    painter.text(at + egui::vec2(1.0, 1.0), egui::Align2::RIGHT_BOTTOM, label, FontId::proportional(11.0), Color32::BLACK);
    painter.text(at, egui::Align2::RIGHT_BOTTOM, label, FontId::proportional(11.0), Color32::WHITE);
}

#[cfg(not(target_arch = "wasm32"))]
fn fetch(url: String, key: TileKey, disk_cache: bool, tx: Sender<(TileKey, Result<Vec<u8>, String>)>, ctx: egui::Context) {
    std::thread::spawn(move || {
        let result = fetch_bytes(&url, disk_cache);
        let _ = tx.send((key, result));
        ctx.request_repaint();
    });
}

#[cfg(not(target_arch = "wasm32"))]
fn fetch_bytes(url: &str, disk_cache: bool) -> Result<Vec<u8>, String> {
    use std::hash::{Hash, Hasher};
    use std::io::Read;
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    url.hash(&mut hash);
    let cache =
        std::env::var_os("LOCALAPPDATA").map(std::path::PathBuf::from).unwrap_or_else(std::env::temp_dir).join("CADCraft").join("geomap-cache");
    let path = cache.join(format!("{:016x}.tile", hash.finish()));
    if disk_cache && let Ok(bytes) = std::fs::read(&path) {
        return Ok(bytes);
    }
    let response = ureq::get(url)
        .set("User-Agent", "CADCraft-GeoMap/0.1 (+https://github.com/storytold/cadcraft)")
        .timeout(std::time::Duration::from_secs(15))
        .call()
        .map_err(|e| match e {
            ureq::Error::Status(code, _) => format!("HTTP {code} from map provider"),
            ureq::Error::Transport(_) => "Map provider network or TLS error".into(),
        })?;
    let mut bytes = Vec::new();
    response.into_reader().take(4 * 1024 * 1024).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    if bytes.is_empty() {
        return Err("Map provider returned an empty tile".into());
    }
    if disk_cache && std::fs::create_dir_all(&cache).is_ok() {
        let _ = std::fs::write(path, &bytes);
    }
    Ok(bytes)
}

#[cfg(target_arch = "wasm32")]
fn fetch(_url: String, key: TileKey, _disk_cache: bool, tx: Sender<(TileKey, Result<Vec<u8>, String>)>, ctx: egui::Context) {
    let _ = tx.send((key, Err("Online maps require the desktop build".into())));
    ctx.request_repaint();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reference_point_maps_to_expected_tile() {
        let config = Config { latitude: -23.55, longitude: -46.63, anchor: Vec2::new(1000.0, 2000.0), ..Default::default() };
        let xf = Xf { rect: Rect::from_min_max(pos2(0.0, 0.0), pos2(256.0, 256.0)), center: config.anchor, scale: 1.0 };
        let range = tile_range(&xf, &config, 12).expect("reference is inside world map");
        let projected = mercator(config.latitude, config.longitude);
        let n = 4096.0;
        let expected_x = ((projected.x + HALF_WORLD) / (2.0 * HALF_WORLD) * n).floor() as u32;
        let expected_y = ((HALF_WORLD - projected.y) / (2.0 * HALF_WORLD) * n).floor() as u32;
        assert!(range.0 <= expected_x && expected_x <= range.1);
        assert!(range.2 <= expected_y && expected_y <= range.3);
    }

    #[test]
    fn custom_source_requires_secure_xyz_and_attribution() {
        let mut config = Config { source: "custom".into(), template: "http://example.com/{z}/{x}/{y}.png".into(), ..Default::default() };
        assert!(config.validate().is_err());
        config.template = "https://example.com/{z}/{x}/{y}.png".into();
        assert!(config.validate().is_err());
        config.attribution = "Example map © provider".into();
        assert!(config.validate().is_ok());
    }
}
