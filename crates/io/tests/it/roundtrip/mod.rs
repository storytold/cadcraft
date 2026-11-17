//! In-memory preservation tests grouped by file format. DXF and DWG use the same drawing cases
//! and strict assertions; each format owns its version domain and explicitly pending regressions.

mod cases;
mod collections;
mod compare;
#[cfg(not(any(target_arch = "wasm32", all(target_os = "freebsd", target_arch = "aarch64"))))]
mod dwg;
mod dxf;
mod handle_limits;
mod unknown;
