//! Embeds the fonts of the optional craft-fonts build input (`CRAFT_FONTS_DIR`) as `CRAFT_FONTS`.
//! The recipe is craft-fonts' `docs/integration.md` (as in the other Crafting Apps); unset or
//! empty, `CRAFT_FONTS` is empty and the build is unchanged. It only reads the local checkout
//! (no network). A bad checkout is a warning, or an error with `CRAFT_FONTS_REQUIRED=1`.
//!
//! On wasm32 it embeds only BIZ UDPGothic Regular and the `Arab` faces (about 5.5 MB), to keep the
//! web build within hosting limits (Cloudflare Pages: 25 MiB per file). The web build has no
//! system fonts, so these are its only fallback for Japanese and Arabic-script text.

use std::fmt::Write as _;
use std::path::PathBuf;

fn main() {
    println!("cargo::rerun-if-env-changed=CRAFT_FONTS_DIR");
    println!("cargo::rerun-if-env-changed=CRAFT_FONTS_REQUIRED");
    let mut src = String::from("pub static CRAFT_FONTS: &[CraftFont] = &[\n");
    if let Some(dir) = std::env::var_os("CRAFT_FONTS_DIR").filter(|d| !d.is_empty()).map(PathBuf::from) {
        match craft_fonts(&dir) {
            Ok(entries) => src.push_str(&entries),
            Err(e) if std::env::var_os("CRAFT_FONTS_REQUIRED").is_some() => {
                println!("cargo::error=CRAFT_FONTS_DIR={}: {}", one_line(&dir.display().to_string()), one_line(&e));
            }
            Err(e) => {
                println!("cargo::warning=building without craft-fonts: CRAFT_FONTS_DIR={}: {}", one_line(&dir.display().to_string()), one_line(&e))
            }
        }
    }
    src.push_str("];\n");
    let out = PathBuf::from(std::env::var_os("OUT_DIR").unwrap_or_default()).join("craft_fonts.rs");
    if let Err(e) = std::fs::write(&out, src) {
        println!("cargo::error=writing {}: {e}", out.display());
    }
}

/// Family-name prefixes AGENTS.md bars (no Adobe fonts, ever): Adobe's Source families and Noto
/// CJK, which is Source Han under another name. `src/craft_fonts.rs` tests the same list.
const BARRED_FAMILIES: [&str; 5] = ["Source Han", "Source Serif", "Source Sans", "Noto Sans CJK", "Noto Serif CJK"];

/// One `CraftFont { .. }` initialiser per manifest line, in manifest order.
fn craft_fonts(dir: &std::path::Path) -> Result<String, String> {
    let manifest = dir.join("fonts/manifest.txt");
    rerun_if_changed(&manifest)?;
    // Resolve the checkout root once so every manifest file can be confined inside it.
    let root = dir.canonicalize().map_err(|e| format!("{}: {e}", dir.display()))?;
    let text = std::fs::read_to_string(&manifest).map_err(|e| format!("{}: {e}", manifest.display()))?;
    let wasm = std::env::var("CARGO_CFG_TARGET_ARCH").is_ok_and(|a| a == "wasm32");
    let mut out = String::new();
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
        let f: Vec<&str> = line.split(" | ").map(str::trim).collect();
        let [family, style, file, scripts, ..] = f.as_slice() else {
            return Err(format!("malformed manifest line: {line}"));
        };
        if BARRED_FAMILIES.iter().any(|barred| family.starts_with(barred)) {
            println!("cargo::warning=craft-fonts: not embedding {family} {style} (AGENTS.md: no Adobe fonts)");
            continue;
        }
        let arabic = scripts.split(',').any(|s| s.trim() == "Arab");
        let web_face = (*family == "BIZ UDPGothic" && *style == "Regular") || arabic;
        if wasm && !web_face {
            continue;
        }
        let path = resolve_font(&root, file)?;
        rerun_if_changed(&path)?;
        let scripts: Vec<String> = scripts.split(',').map(|s| format!("{:?}", s.trim())).collect();
        let _ = writeln!(
            out,
            "    CraftFont {{ family: {family:?}, style: {style:?}, scripts: &[{}], bytes: include_bytes!({:?}) }},",
            scripts.join(", "),
            path.display().to_string(),
        );
    }
    Ok(out)
}

/// Resolve a manifest `file` field to a real path *confined to the checkout* `root` (CWE-22).
///
/// The field is rejected if it is absolute or contains a parent (`..`) component, and the
/// canonicalised result must still live inside `root`, so a crafted manifest or a symlink in the
/// checkout cannot make the generated `include_bytes!` read an arbitrary file on the build host.
fn resolve_font(root: &std::path::Path, file: &str) -> Result<PathBuf, String> {
    let rel = std::path::Path::new(file);
    if rel.is_absolute() || rel.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
        return Err(format!("{file}: manifest paths must be relative and stay inside the checkout"));
    }
    let path = root.join(rel).canonicalize().map_err(|e| format!("{file}: {e}"))?;
    if !path.starts_with(root) {
        return Err(format!("{file}: resolves outside the craft-fonts checkout"));
    }
    Ok(path)
}

/// Collapse CR/LF in a diagnostic derived from checkout contents to a single space, so it cannot
/// introduce an additional Cargo directive on the newline-delimited build-script stream (CWE-74).
fn one_line(s: &str) -> String {
    s.replace(['\n', '\r'], " ")
}

/// Emit a `cargo::rerun-if-changed` directive for `path`, refusing paths that contain a line
/// separator (CWE-74): Cargo's build-script protocol is newline-delimited, so a path with an
/// embedded CR/LF could smuggle an extra directive such as `cargo::rustc-env=OUT_DIR=…`.
fn rerun_if_changed(path: &std::path::Path) -> Result<(), String> {
    let shown = path.display().to_string();
    if shown.contains(['\n', '\r']) {
        return Err(format!("{shown:?}: path contains a line separator"));
    }
    println!("cargo::rerun-if-changed={shown}");
    Ok(())
}
