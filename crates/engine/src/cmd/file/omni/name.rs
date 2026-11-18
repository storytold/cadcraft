//! A version's name: the fields it carries, and the block name an overlay is given.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::OVERLAY_PREFIX;
use super::time::{iso, stamp, stamp_epoch};

/// Versions are stored gzipped; the suffix says so.
pub(super) const COMPRESSED_EXT: &str = "gz";
/// The description an automatic version — the one a save leaves behind — is filed under.
pub(super) const SAVE_DESCRIPTION: &str = "save";
/// Characters of one name part (a description, a user name) kept in a file name.
pub(super) const MAX_PART: usize = 48;
/// Snapshot files are DXF and DWG drawings, never anything else that happens to be in the folder.
pub(super) const DRAWING_EXTS: [&str; 2] = ["dxf", "dwg"];
/// The lowercase extension of a path (empty without one).
pub(super) fn ext_of(p: &Path) -> String {
    p.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default()
}

/// A file name split into its stem and its extension.
pub(super) fn split_ext(file: &str) -> (&str, &str) {
    match file.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => (stem, ext),
        _ => (file, ""),
    }
}

/// The user a snapshot is stamped with: the environment, so the same code works on every
/// platform CADCraft builds for (Windows sets `USERNAME`, everything else `USER` or `LOGNAME`).
pub(super) fn user() -> String {
    for k in ["CADCRAFT_USER", "USER", "USERNAME", "LOGNAME"] {
        if let Some(v) = std::env::var_os(k) {
            let s = v.to_string_lossy().trim().to_string();
            if !s.is_empty() {
                return s;
            }
        }
    }
    "unknown".to_string()
}

/// The user part of a version's name. `_` separates the name's fields, so it can never appear
/// inside one: a name from an `_`-carrying account (`john_doe`) would otherwise read back with the
/// user `john` and the description `doe`, and every save of that account would list as a milestone.
pub(super) fn user_part_of(me: &str) -> String {
    let s = slug(me, "-");
    if s.is_empty() { "unknown".to_string() } else { s }
}

/// A name part that is safe in a file name and in a block name: letters and digits of any script
/// are kept (so a description in Russian survives), the characters in `keep` are kept, and every
/// other character — a space, a path separator, the name's own `_` — becomes `-`. Runs collapse
/// and the ends are trimmed, so one part never holds two separators in a row.
pub(super) fn slug(text: &str, keep: &str) -> String {
    let mut out = String::new();
    let mut n = 0;
    for c in text.trim().chars() {
        let ok = c.is_alphanumeric() || keep.contains(c);
        if ok {
            out.push(c);
            n += 1;
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
            n += 1;
        }
        if n >= MAX_PART {
            break;
        }
    }
    out.trim_matches('-').to_string()
}

/// One version on disk: what its name says about it, and where it is.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Snap {
    pub(super) file: String,
    pub(super) path: PathBuf,
    /// The name without the compression suffix — what the drawing inside is called.
    pub(super) inner: String,
    /// Stored gzipped.
    pub(super) compressed: bool,
    /// When it was taken (UTC seconds), `None` when the name doesn't follow the pattern.
    pub(super) epoch: Option<i64>,
    pub(super) millis: u32,
    pub(super) user: String,
    pub(super) description: String,
    /// The drawing file it was taken from.
    pub(super) drawing: String,
    /// On disk, so a compressed version compares as what it costs.
    pub(super) bytes: u64,
}

impl Snap {
    /// What a call that takes no snapshot of its own uses: the newest.
    pub(super) fn time(&self) -> Value {
        match self.epoch {
            Some(e) => json!(iso(e, self.millis)),
            None => Value::Null,
        }
    }
    /// An automatic version (one a save left behind) or a named milestone (OMNI-SNAP).
    pub(super) fn kind(&self) -> &'static str {
        if self.description.eq_ignore_ascii_case(SAVE_DESCRIPTION) { "save" } else { "milestone" }
    }
    pub(super) fn json(&self, n: usize) -> Value {
        json!({
            "n": n,
            "file": self.file,
            "path": self.path.to_string_lossy(),
            "time": self.time(),
            "user": self.user,
            "description": self.description,
            "kind": self.kind(),
            "drawing": self.drawing,
            "bytes": self.bytes,
            "compressed": self.compressed,
        })
    }
}

/// Read a version's metadata out of its file name. The description is one `_`-free field and the
/// drawing's own name is everything after it, so the drawing is recovered exactly however many
/// separators the name parts before it had; a file that doesn't follow the pattern is still
/// listed, with what its name didn't say left empty.
pub(super) fn parse(file: &str, path: PathBuf, bytes: u64) -> Snap {
    let compressed = file.to_ascii_lowercase().ends_with(&format!(".{COMPRESSED_EXT}"));
    let inner = if compressed { &file[..file.len() - COMPRESSED_EXT.len() - 1] } else { file };
    let (stem, ext) = split_ext(inner);
    let f: Vec<&str> = stem.split('_').collect();
    let epoch = f.first().copied().and_then(stamp_epoch);
    let millis = f.get(1).and_then(|s| s.parse::<u32>().ok()).unwrap_or(0);
    let tail = f.get(4..).map(|t| t.join("_")).unwrap_or_default();
    let drawing = match (epoch.is_some(), tail.is_empty(), ext.is_empty()) {
        (true, false, false) => format!("{tail}.{ext}"),
        (true, false, true) => tail,
        _ => inner.to_string(),
    };
    Snap {
        file: file.to_string(),
        path,
        inner: inner.to_string(),
        compressed,
        epoch,
        millis,
        user: f.get(2).copied().unwrap_or_default().to_string(),
        description: f.get(3).copied().unwrap_or_default().to_string(),
        drawing,
        bytes,
    }
}

/// Whether a file name is a version's (a DXF or DWG drawing in the history folder, stored plain by
/// an older build or gzipped now), whatever case the extensions are written in.
pub(super) fn is_snapshot(file: &str) -> bool {
    let lower = file.to_ascii_lowercase();
    let plain = lower.strip_suffix(&format!(".{COMPRESSED_EXT}")).unwrap_or(&lower);
    DRAWING_EXTS.contains(&split_ext(plain).1)
}

/// The snapshot a value names: a number as listed (1 = the newest), or a file name or path.
pub(super) fn chosen(list: &[Snap], v: &Value) -> Option<Snap> {
    match v {
        Value::Number(_) => v.as_u64().map(|i| i as usize).and_then(|i| i.checked_sub(1)).and_then(|i| list.get(i)).cloned(),
        Value::String(t) => {
            let t = t.trim();
            if let Ok(i) = t.parse::<usize>() {
                return chosen(list, &json!(i));
            }
            let name = t.rsplit(['/', '\\']).next().unwrap_or(t);
            list.iter().find(|s| s.file.eq_ignore_ascii_case(name) || s.path.to_string_lossy() == t).cloned()
        }
        _ => None,
    }
}

/// The overlay reference name of a snapshot: the drawing's name and the moment, `_`-separated so
/// it is a valid block name and unique per snapshot.
///
/// The milliseconds are part of it. A save's version and an OMNI-SNAP milestone taken in the same
/// second used to get one name; the second attach then found the first overlay's block and reused
/// it, so `OMNI-DIFF snapshot=2` laid the wrong revision under the drawing. A file whose name
/// carries no stamp gets the file's own stem for the same reason.
pub(super) fn overlay_name(snap: &Snap) -> String {
    let (stem, _) = split_ext(&snap.drawing);
    let base = format!("{OVERLAY_PREFIX}{}", slug(stem, "-_"));
    match snap.epoch {
        Some(e) => format!("{base}_{}_{:03}", stamp(e).replace('.', "_"), snap.millis),
        None => format!("{base}_{}", slug(&split_ext(&snap.file).0, "-_")),
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_keep_their_fields_and_survive_any_script() {
        let s = parse("20261014.083512_482_kostyk_правка-стен_план_на_2026.dxf", PathBuf::from("x.dxf"), 12);
        assert_eq!(s.epoch, Some(1_791_966_912));
        assert_eq!(s.millis, 482);
        assert_eq!(s.user, "kostyk");
        assert_eq!(s.description, "правка-стен");
        assert_eq!(s.drawing, "план_на_2026.dxf", "everything after the description is the drawing");
        assert_eq!(s.bytes, 12);
        // A file that doesn't follow the pattern is still read, with what its name didn't say empty.
        let f = parse("notes.dxf", PathBuf::from("notes.dxf"), 0);
        assert_eq!((f.epoch, f.drawing.as_str()), (None, "notes.dxf"));
    }

    #[test]
    fn slugs_drop_separators_and_path_characters() {
        assert_eq!(slug("правка стен", "-"), "правка-стен");
        assert_eq!(slug("a/b\\c:d*e?f", "-"), "a-b-c-d-e-f");
        assert_eq!(slug("__x__", "-"), "x", "the name's own separator never survives a description");
        assert_eq!(slug("", "-"), "");
        assert_eq!(slug("  ", "-"), "");
        assert_eq!(slug("a--b", "-"), "a--b", "a separator the user typed is kept");
        assert_eq!(slug("-x-", "-"), "x");
        assert_eq!(slug("x".repeat(200).as_str(), "-").chars().count(), MAX_PART);
        // A block name keeps the `_` that separates its parts.
        assert_eq!(slug("план_2026", "-_"), "план_2026");
        assert_eq!(slug("план 2026", "-_"), "план-2026", "a space becomes a dash, even where a `_` would do");
    }

    #[test]
    fn an_overlay_is_named_after_its_snapshot() {
        let s = parse("20261014.083512_482_kostyk_правка_план.dxf", PathBuf::from("x.dxf"), 0);
        assert_eq!(overlay_name(&s), "OMNI_план_20261014_083512");
        let untimed = parse("notes.dxf", PathBuf::from("notes.dxf"), 0);
        assert_eq!(overlay_name(&untimed), "OMNI_notes");
    }

    #[test]
    fn an_account_with_an_underscore_is_still_a_save() {
        // `_` separates a version's fields, so an account named `john_doe` must lose it here:
        // otherwise the name reads back as user `john`, description `doe` — a milestone, not a save
        // — and the drawing it belongs to is never found again.
        assert_eq!(user_part_of("john_doe"), "john-doe");
        assert_eq!(user_part_of("john doe"), "john-doe");
        assert_eq!(user_part_of("___"), "unknown", "a part that is only separators still names someone");
        let name = format!("20261014.083512_482_{}_save_plan.dxf.gz", user_part_of("john_doe"));
        let s = parse(&name, PathBuf::from("h").join(&name), 9);
        assert_eq!(s.user, "john-doe");
        assert_eq!(s.description, SAVE_DESCRIPTION);
        assert_eq!(s.kind(), "save");
        assert_eq!(s.drawing, "plan.dxf");
    }

    #[test]
    fn only_drawings_are_snapshots() {
        assert!(is_snapshot("20261014.083512_482_u_d_plan.dxf"));
        assert!(is_snapshot("a.DWG"));
        assert!(!is_snapshot("desktop.ini"));
        assert!(!is_snapshot("readme.md"));
        assert!(!is_snapshot("_OMNI_HISTORY"));
    }

    #[test]
    fn a_snapshot_is_chosen_by_number_name_or_path() {
        let new = "20261014.083512_482_u_new_plan.dxf";
        let old = "20261013.083512_482_u_old_plan.dxf";
        let list = vec![parse(new, PathBuf::from("h").join(new), 3), parse(old, PathBuf::from("h").join(old), 4)];
        assert_eq!(chosen(&list, &json!(1)).map(|s| s.file), Some(new.to_string()));
        assert_eq!(chosen(&list, &json!(2)).map(|s| s.file), Some(old.to_string()));
        assert_eq!(chosen(&list, &json!(3)), None, "past the end");
        assert_eq!(chosen(&list, &json!(0)), None, "0 is cancel, not the first");
        assert_eq!(chosen(&list, &json!("2")).map(|s| s.bytes), Some(4));
        assert_eq!(chosen(&list, &json!(old)).map(|s| s.bytes), Some(4), "a file name");
        assert_eq!(chosen(&list, &json!("OLD")), None, "a name is matched whole, not by its parts");
        assert_eq!(chosen(&list, &json!("old/plan.dxf")), None, "a folder is not a snapshot");
        assert_eq!(chosen(&list, &json!(format!("h/{old}"))).map(|s| s.bytes), Some(4), "a full path");
        assert_eq!(chosen(&list, &json!(true)), None);
        assert_eq!(chosen(&[], &json!(1)), None);
    }
}
