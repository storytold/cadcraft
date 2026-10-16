# GEOMAP: online background maps

## Run and test on Windows

Install Rust 1.95 or later and the Visual Studio C++ build tools, then open PowerShell:

```powershell
cd C:\Users\guilherme.barbosa\Music\cadmapfix
git switch map-fix
cargo run -p cadcraft -- --sample
```

In the CADCraft command line, type `GEOMAP`. Choose **OpenStreetMap Standard** and enter a familiar latitude/longitude. Click **Use current view center as CAD point**, leave **Metres per CAD unit** at `1` if the drawing is in metres, then click **Show map**. Pan and zoom; the tiles should stay aligned behind the lines. Reopen `GEOMAP` and switch to **NASA Blue Marble** to check the no-key satellite overview. Use **Hide map** to turn it off. For a quick programmatic check, enter `geomap {"source":"osm","latitude":-23.55,"longitude":-46.63,"x":0,"y":0,"metresPerUnit":1}`. The reference point should show São Paulo at CAD coordinate `(0,0)`.

Validation commands:

```powershell
cargo fmt --check
cargo check -p cadcraft-ui-egui
cargo test -p cadcraft-ui-egui geomap --lib
```

Type `GEOMAP` in the command line, or choose **View → Online Map...**. Select a source, enter the latitude and longitude of a known point (WGS84), the corresponding CAD X/Y point, and the number of ground metres per drawing unit. For example, a drawing in metres uses `1`; a drawing in millimetres uses `0.001`. Choose **Show map**. The map appears behind the model-space entities and follows pan and zoom. Open `GEOMAP` again to change the source or hide it.

Included sources without keys are OpenStreetMap Standard, OpenTopoMap, and NASA Blue Marble satellite imagery (a low-resolution global overview, not parcel-level aerial photography). CARTO Voyager, Positron, and Dark Matter require your own API key. **Custom XYZ** accepts an HTTPS raster tile URL with `{z}`, `{x}`, and `{y}` placeholders, an optional `{key}` placeholder, and mandatory visible attribution. Thus providers not in the built-in list can be used if they offer compatible XYZ tiles and their terms allow it. Provider attribution is displayed on the canvas; preserve it when sharing screenshots.

Google Maps and Google Hybrid are not available without a Google Maps Platform API key and Map Tiles API session. Free Bing Maps Basic tile access has been retired; Microsoft recommends Azure Maps, which also requires an account/key. URLs copied from browser traffic or unofficial QGIS presets are not built in because they are not supported public tile APIs. This implementation does not claim their imagery or resolution as a substitute for NASA's.

The map is a live background for the current desktop session. It is not embedded in or exported with the DXF/DWG, and it is disabled when switching drawings. Built-in OpenStreetMap, OpenTopoMap, and NASA tiles are cached in `%LOCALAPPDATA%\CADCraft\geomap-cache` on Windows so returning to a viewed area does not download them again. Custom and key-based sources are held in memory only. Only visible tiles are requested, with at most four concurrent requests. Selecting a map sends tile coordinates and your IP address to that provider. CADCraft sends an identifying User-Agent for these requests.

Alignment is local: the reference point is mapped to Web Mercator with a latitude-adjusted scale. The current implementation assumes CAD X points east and Y points north. It does not parse EPSG codes, reproject arbitrary drawings, rotate a surveyed grid, or account for projection distortion far from the reference point. For accurate placement of a large UTM or state-plane drawing, a full CRS transform and multiple control points are still needed.

Programmatic form: `geomap {"source":"osm","latitude":-23.55,"longitude":-46.63,"x":0,"y":0,"metresPerUnit":1}`. Use `geomap {"on":false}` to hide it. Source IDs: `osm`, `opentopo`, `nasa-blue-marble`, `carto-voyager`, `carto-positron`, `carto-dark`, `custom`. For `custom`, also supply `url` and `attribution`; for CARTO, supply `key`.

Provider policies and credits: [OpenStreetMap tile usage](https://operations.osmfoundation.org/policies/tiles/), [OpenTopoMap](https://www.opentopomap.org/about), [NASA GIBS](https://nasa-gibs.github.io/gibs-api-docs/access-basics/), [CARTO basemaps](https://carto.com/basemaps/index.html), [Google Map Tiles API](https://developers.google.com/maps/documentation/tile/overview), [Bing Maps retirement](https://learn.microsoft.com/en-us/bingmaps/rest-services/directly-accessing-the-bing-maps-tiles).
