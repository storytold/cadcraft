//! IMPORTGIS (MAPIMPORT, GEOIMPORT): GIS vector data (ESRI Shapefile with its `.dbf` and `.prj`,
//! GeoJSON) into the current space through the host's GIS reader ([`super::super::file::gis_reader`]).

use serde_json::{Value, json};

use super::super::file::{base64_decode, gis_reader};
use super::super::{CommandSpec, bad, str_param};
use super::{import_drawing, placement, read_file};
use crate::{Result, Session};

const CMD: &str = "importgis";

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new(CMD, "Import GIS Data...", run)
            .menu(&["Insert", "Import GIS Data..."])
            .alias(&["mapimport", "geoimport"])
            .params("{path: \"parcels.shp\" (its .dbf and .prj beside it are read too) or \"site.geojson\" | data: base64, name: \"x.shp\"|\"x.geojson\", dbf?: base64, prj?: WKT text}, layerField?: attribute whose value names the layer, labels?: true (polygons labelled with their name/label/number/code/id), projection?: auto|mercator|none (auto: longitude/latitude data to Web Mercator), at?: [x,y], scale?"),
    ]
}

/// Whether IMPORT hands `name` to IMPORTGIS.
pub(super) fn is_gis_name(name: &str) -> bool {
    matches!(super::extension(name).as_str(), "shp" | "geojson")
}

/// The file beside `path` with the same stem and extension `ext` (either case), if it exists.
#[cfg(not(target_arch = "wasm32"))]
fn companion(path: &str, ext: &str) -> Result<Option<Vec<u8>>> {
    let p = std::path::Path::new(path);
    for e in [ext.to_ascii_lowercase(), ext.to_ascii_uppercase()] {
        let c = p.with_extension(e);
        if c.is_file() {
            return read_file(CMD, &c.to_string_lossy()).map(Some);
        }
    }
    Ok(None)
}

#[cfg(target_arch = "wasm32")]
fn companion(_path: &str, _ext: &str) -> Result<Option<Vec<u8>>> {
    Ok(None)
}

pub(super) fn run(s: &mut Session, p: &Value) -> Result<Value> {
    let reader = gis_reader().ok_or_else(|| bad(CMD, "GIS import is not available in this build"))?;
    let (at, scale) = placement(CMD, p)?;
    let (bytes, name, dbf, prj) = if let Some(path) = str_param(p, "path") {
        let bytes = read_file(CMD, path)?;
        let (dbf, prj) = if super::extension(path) == "shp" { (companion(path, "dbf")?, companion(path, "prj")?) } else { (None, None) };
        (bytes, path.to_string(), dbf, prj.map(|b| String::from_utf8_lossy(&b).into_owned()))
    } else {
        let data = str_param(p, "data").ok_or_else(|| bad(CMD, "`path` or `data` (base64) is required"))?;
        let bytes = base64_decode(data).ok_or_else(|| bad(CMD, "invalid base64"))?;
        let dbf = match str_param(p, "dbf") {
            Some(d) => Some(base64_decode(d).ok_or_else(|| bad(CMD, "`dbf` is not valid base64"))?),
            None => None,
        };
        (bytes, str_param(p, "name").unwrap_or("import.geojson").to_string(), dbf, str_param(p, "prj").map(str::to_string))
    };
    let src = reader(&name, &bytes, dbf.as_deref(), prj.as_deref(), p).map_err(|e| bad(CMD, e))?;
    let file = std::path::Path::new(&name).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let mut r = import_drawing(s, &src, at, scale, &file)?;
    for note in &src.read_notes {
        s.echo(note.clone());
    }
    r["notes"] = json!(src.read_notes);
    Ok(r)
}
