//! `cargo xtask parity`: compare the reference app's menu tree (feature names only, in
//! `xtask/data/menu-catalog.txt`) with the live command registry (`cadcraft-cli commands`), and
//! write `docs/parity-checklist.md` (dated; the assessment that interprets it is
//! `docs/target-app-parity.md`).

use std::path::Path;
use std::process::Command;

/// Leaf menu items with their path ("Draw > Circle > 3 Points").
pub fn leaves(catalog: &str) -> Vec<String> {
    let lines: Vec<&str> = catalog.lines().filter(|l| !l.trim().is_empty()).collect();
    let mut stack: Vec<(usize, String)> = Vec::new();
    let mut out = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        let ind = l.len() - l.trim_start().len();
        while stack.last().is_some_and(|(d, _)| *d >= ind) {
            stack.pop();
        }
        stack.push((ind, l.trim().to_string()));
        let next_ind = lines.get(i + 1).map(|n| n.len() - n.trim_start().len()).unwrap_or(0);
        if next_ind <= ind && ind > 0 {
            out.push(stack.iter().map(|(_, s)| s.as_str()).collect::<Vec<_>>().join(" > "));
        }
    }
    out
}

/// RFC-0052 hand marks for menu leaves no engine command covers: `(path, headless, reach, why)`.
/// A path ending in ` > ` marks every leaf under it; the longest matching path wins.
///
/// `headless` is `no` or `partial`; `reach` is `window` (needs a desktop window, dialog or pointer),
/// `os` (needs an OS or online service the engine doesn't wrap) or `engine-gap` (could be an engine
/// command, isn't one yet). `yes` is never written here: a leaf is `yes` only when a command from
/// `cadcraft-cli commands`, the engine registry the CLI and the MCP server load, covers it. An
/// unmarked uncovered leaf is `unknown`. [`mark_errors`] rejects marks that match nothing or only
/// hide a `yes`.
pub const HEADLESS: &[(&str, &str, &str, &str)] = &[
    ("Draw > 3D Modeling > ", "no", "engine-gap", "3D solid and surface creation could be commands"),
    ("Modify > 3D Operations > ", "no", "engine-gap", "3D operations could be commands"),
    ("Modify > Solid Editing > ", "no", "engine-gap", "solid editing could be commands"),
    ("Modify > Mesh Editing > ", "no", "engine-gap", "mesh editing could be commands"),
    ("Modify > Surface Editing > ", "no", "engine-gap", "surface editing could be commands"),
    ("View > 3D Views > ", "no", "engine-gap", "a saved view direction could be set by a command"),
    ("View > Visual Styles > ", "no", "engine-gap", "a visual style could be set by a command"),
    ("View > Render > ", "no", "engine-gap", "rendering could be a command"),
    ("View > Camera > ", "no", "engine-gap", "cameras could be created by a command"),
    ("View > Lights", "no", "engine-gap", "lights could be created by a command"),
    ("View > Hide", "no", "engine-gap", "hidden-line output could be a command"),
    ("View > Orbit > ", "no", "window", "drags the desktop view"),
    ("View > Pan > ", "no", "window", "drags the desktop view"),
    ("View > ViewCube > ", "no", "window", "a desktop view control"),
    ("View > UCS Icon > ", "no", "window", "a desktop view overlay"),
    ("View > Enter Full Screen", "no", "window", "the desktop window"),
    ("Tools > New UCS > ", "no", "engine-gap", "a coordinate system could be set by a command"),
    ("Tools > Named UCS...", "no", "engine-gap", "named coordinate systems could be commands"),
    ("Tools > Customize > ", "no", "window", "a desktop dialog"),
    ("Window > ", "no", "window", "opens or arranges a desktop window or palette"),
    ("Help > ", "no", "window", "a desktop help window or dialog"),
    ("Help > AutoCAD Help", "no", "os", "opens online help in the browser"),
    ("Help > What's New", "no", "os", "opens online help in the browser"),
    ("Help > Send Feedback", "no", "os", "an online feedback service"),
    ("Help > Download Offline Help", "no", "os", "an online download"),
    ("Help > Autodesk Assistant", "no", "os", "an online assistant service"),
    ("File > Share...", "no", "os", "an online sharing service"),
    ("File > Open Recent > ", "no", "window", "the desktop recent-files list"),
    ("File > New Sheet Set...", "no", "engine-gap", "sheet sets could be commands"),
    ("File > Open Sheet Set...", "no", "engine-gap", "sheet sets could be commands"),
    ("File > Package Drawing...", "no", "engine-gap", "packaging a drawing could be a command"),
    ("File > Export Layout to Model...", "no", "engine-gap", "the export could be a command"),
    ("File > DWG Compare...", "no", "engine-gap", "comparing drawings could be a command"),
    ("File > Markup Import...", "no", "engine-gap", "the import could be a command"),
    ("File > Attach...", "no", "engine-gap", "attaching a reference could be a command"),
    ("Edit > Undo Group of commands", "no", "engine-gap", "undo groups could be a command"),
    ("Insert > Raster Image Reference...", "no", "engine-gap", "the reference could be inserted by a command"),
    ("Insert > PDF Underlay...", "no", "engine-gap", "the underlay could be inserted by a command"),
    ("Insert > Layout > ", "no", "engine-gap", "layouts could be created by a command"),
    ("Insert > Field...", "no", "engine-gap", "fields could be inserted by a command"),
    ("Insert > Drawing Exchange Binary...", "no", "engine-gap", "the import could be a command"),
    ("Insert > ACIS File...", "no", "engine-gap", "the import could be a command"),
    ("Format > Transparency", "no", "engine-gap", "transparency could be set by a command"),
    ("Tools > Spelling", "no", "engine-gap", "a spell check could be a command"),
    ("Tools > Update Fields", "no", "engine-gap", "updating fields could be a command"),
    ("Tools > Edit Xref in Place", "no", "window", "edits a reference in the desktop view"),
    ("Tools > Import Settings", "no", "engine-gap", "importing settings could be a command"),
    ("Tools > Export Settings", "no", "engine-gap", "exporting settings could be a command"),
    ("Tools > Express Tools > Super Hatch > ", "no", "engine-gap", "image and reference hatches could be commands"),
    ("Tools > Express Tools > Super Hatch > Select Existing Object", "no", "window", "picks an object in the desktop view"),
    ("Modify > Object > External Reference > Frame", "no", "engine-gap", "the frame setting could be a command"),
    ("Modify > Object > Image > ", "no", "engine-gap", "image settings could be commands"),
    ("Modify > Clip > Image", "no", "engine-gap", "clipping an image could be a command"),
];

/// Whether a `HEADLESS` path covers a leaf.
fn mark_covers(mark: &str, leaf: &str) -> bool {
    mark == leaf || (mark.ends_with(" > ") && leaf.starts_with(mark))
}

/// A leaf's RFC-0052 `(headless, reach, why)`: `yes` when an engine command covers it, else the
/// longest mark that covers it, else `unknown`.
pub fn headless_of<'a>(leaf: &str, covered: bool, marks: &[(&'a str, &'a str, &'a str, &'a str)]) -> (&'a str, Option<&'a str>, &'a str) {
    if covered {
        return ("yes", None, "");
    }
    marks.iter().filter(|m| mark_covers(m.0, leaf)).max_by_key(|m| m.0.len()).map_or(("unknown", None, ""), |m| (m.1, Some(m.2), m.3))
}

/// Problems with `marks` against `leaves` (`(path, covered)`): bad values, repeats, a mark that
/// matches no leaf, and a mark whose leaves are all covered (it would only hide a `yes`).
pub fn mark_errors(marks: &[(&str, &str, &str, &str)], leaves: &[(String, bool)]) -> Vec<String> {
    let mut errs = Vec::new();
    for (i, (path, value, reach, _)) in marks.iter().enumerate() {
        if !matches!(*value, "no" | "partial") {
            errs.push(format!("headless mark `{path}`: `{value}` is not `no` or `partial` (`yes` comes from the registry)"));
        }
        if !matches!(*reach, "window" | "os" | "engine-gap") {
            errs.push(format!("headless mark `{path}`: reach `{reach}` is not window, os or engine-gap"));
        }
        if marks[..i].iter().any(|m| m.0 == *path) {
            errs.push(format!("headless mark `{path}` is written twice"));
        }
        let hits: Vec<bool> = leaves.iter().filter(|l| mark_covers(path, &l.0)).map(|l| l.1).collect();
        if hits.is_empty() {
            errs.push(format!("headless mark `{path}` matches no catalog leaf"));
        } else if hits.iter().all(|c| *c) {
            errs.push(format!("headless mark `{path}` is stale: a command covers every leaf it marks"));
        }
    }
    errs
}

/// The RFC-0052 summary line for `values` (`(headless, reach)` per leaf).
pub fn headless_summary(values: &[(&str, Option<&str>)]) -> String {
    let count = |v: &str| values.iter().filter(|h| h.0 == v).count();
    let no = |r: &str| values.iter().filter(|h| h.0 == "no" && h.1 == Some(r)).count();
    format!(
        "**Headless ([RFC-0052](https://github.com/storytold/craft-contrib/blob/main/rfcs/0052-headless-reach.md)): {} yes, {} no (window {}, os {}, engine-gap {}), {} partial, {} unknown.**",
        count("yes"),
        count("no"),
        no("window"),
        no("os"),
        no("engine-gap"),
        count("partial"),
        count("unknown")
    )
}

/// Where the generated checklist goes.
pub const OUT: &str = "docs/parity-checklist.md";

/// Today's UTC date as `YYYY-MM-DD` (from the system clock).
fn today() -> String {
    date(std::time::SystemTime::now())
}

/// `t` as a UTC `YYYY-MM-DD`. A clock before 1970 reads as 1970-01-01 (the `unwrap_or(0)` clamp);
/// past chrono's year 262143 range the proleptic Gregorian date is kept via the 400-year cycle
/// rather than falling back to the epoch.
fn date(t: std::time::SystemTime) -> String {
    let secs = t.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let (y, m, d) = civil_from_days(secs / 86_400);
    // `{y:04}` rather than `%Y`, which writes a `+` before years past 9999.
    format!("{y:04}-{m:02}-{d:02}")
}

/// Days since 1970-01-01 → proleptic Gregorian (year, month, day).
///
/// `date` clamps a pre-1970 clock to the epoch, so `days` is an unsigned count that can run past
/// `NaiveDate`'s ±262143-year range. The Gregorian calendar repeats every 400 years (146_097
/// days), so chrono converts the day within one cycle and whole cycles go back into the year (an
/// `i128`, then saturated into `i64`), keeping chrono as the only calendar logic and reproducing
/// the old `civil_from_days` result exactly. 1970-01-01 is `num_days_from_ce` 719_163.
fn civil_from_days(days: u64) -> (i64, u32, u32) {
    use chrono::Datelike;
    let (cycles, rest) = (days / 146_097, days % 146_097);
    // `rest + 719_163` is 719_163..865_259, always a valid CE day.
    match i32::try_from(rest + 719_163).ok().and_then(chrono::NaiveDate::from_num_days_from_ce_opt) {
        Some(d) => {
            let y = i128::from(d.year()) + i128::from(cycles) * 400;
            (i64::try_from(y).unwrap_or(i64::MAX), d.month(), d.day())
        }
        None => (1970, 1, 1), // unreachable
    }
}

pub fn run(root: &Path) -> Result<(), String> {
    let catalog = std::fs::read_to_string(root.join("xtask/data/menu-catalog.txt")).map_err(|e| format!("menu catalog: {e}"))?;
    let out = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .current_dir(root)
        .args(["run", "-q", "-p", "cadcraft-cli", "--", "commands"])
        .output()
        .map_err(|e| format!("cadcraft-cli: {e}"))?;
    let cmds: serde_json::Value = serde_json::from_slice(&out.stdout).map_err(|e| format!("commands JSON: {e}"))?;
    let paths: Vec<String> = cmds
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|c| c["menu"].as_array())
                .filter(|m| !m.is_empty())
                .map(|m| m.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(" > ").to_ascii_lowercase())
                .collect()
        })
        .unwrap_or_default();
    let labels: Vec<String> = paths.iter().filter_map(|p| p.rsplit(" > ").next().map(str::to_string)).collect();
    let ls = leaves(&catalog);
    let mut by_menu: Vec<(String, usize, usize)> = Vec::new();
    let mut missing = Vec::new();
    let mut checked: Vec<(String, bool)> = Vec::new();
    let mut hit = 0;
    for l in &ls {
        let top = l.split(" > ").next().unwrap_or("").to_string();
        let lower = l.to_ascii_lowercase();
        let leaf = lower.rsplit(" > ").next().unwrap_or("").to_string();
        let ok = paths.contains(&lower) || labels.contains(&leaf);
        checked.push((l.clone(), ok));
        if ok {
            hit += 1;
        } else {
            missing.push(l.clone());
        }
        match by_menu.iter_mut().find(|(m, ..)| *m == top) {
            Some(e) => {
                e.1 += 1;
                e.2 += usize::from(ok);
            }
            None => by_menu.push((top, 1, usize::from(ok))),
        }
    }
    let errs = mark_errors(HEADLESS, &checked);
    if !errs.is_empty() {
        return Err(format!("RFC-0052 headless marks in xtask/src/parity.rs:\n  {}", errs.join("\n  ")));
    }
    let values: Vec<(&str, Option<&str>)> = checked.iter().map(|(l, ok)| headless_of(l, *ok, HEADLESS)).map(|h| (h.0, h.1)).collect();
    let pct = |a: usize, b: usize| if b == 0 { 0.0 } else { 100.0 * a as f64 / b as f64 };
    let mut md = format!(
        "# Parity checklist\n\n> **Generated:** {} by `cargo xtask parity`. Do not edit by hand.\n\nMenu-breadth parity: a reference menu item counts as covered when a CADCraft command is registered under the same menu label. It measures breadth, not depth; see [target-app-parity.md](target-app-parity.md) for the weighted assessment.\n\n**Menu breadth: {hit} / {} items ({:.0}%)** · {} registered commands.\n\n| Menu | Items | Covered | % |\n|---|---|---|---|\n",
        today(),
        ls.len(),
        pct(hit, ls.len()),
        cmds.as_array().map(Vec::len).unwrap_or(0)
    );
    for (m, n, k) in &by_menu {
        md += &format!("| {m} | {n} | {k} | {:.0}% |\n", pct(*k, *n));
    }
    md += &format!(
        "\n{}\n\nA covered item is `headless: yes`: the command that covers it is in the engine registry that `cadcraft-cli` and the MCP server load. An item not covered carries its hand mark from `HEADLESS` in `xtask/src/parity.rs`, or `unknown`.\n",
        headless_summary(&values)
    );
    md += "\n## Not yet covered\n\n";
    for m in &missing {
        let (value, reach, why) = headless_of(m, false, HEADLESS);
        let reach = reach.map(|r| format!(", reach: {r} ({why})")).unwrap_or_default();
        md += &format!("- {m}: headless: {value}{reach}\n");
    }
    std::fs::write(root.join(OUT), md).map_err(|e| format!("{OUT}: {e}"))?;
    println!("parity: {hit}/{} menu items ({:.0}%) → {OUT}", ls.len(), pct(hit, ls.len()));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_dates() {
        let day = |d: u64| date(std::time::UNIX_EPOCH + std::time::Duration::from_secs(d * 86_400));
        assert_eq!(day(0), "1970-01-01");
        assert_eq!(day(11_016), "2000-02-29");
        assert_eq!(day(20_736), "2026-10-10");
        assert_eq!(date(std::time::UNIX_EPOCH - std::time::Duration::from_secs(86_400)), "1970-01-01");
        // Past chrono's range keeps the proleptic Gregorian date, not the epoch (old `civil_from_days`).
        // Checked on the day count: Windows' SystemTime ends in the year 30828, so building that
        // SystemTime panics there.
        assert_eq!(civil_from_days(115_740_740), (318_857, 5, 20));
        // Windows' SystemTime can't hold u64::MAX / 2 seconds, so it is only checked where it exists.
        if let Some(far) = std::time::UNIX_EPOCH.checked_add(std::time::Duration::from_secs(u64::MAX / 2)) {
            assert_eq!(date(far), "292277026596-12-04");
        }
    }

    #[test]
    fn headless_marks_pick_the_longest_path() {
        let marks = [("W > ", "no", "window", "a"), ("W > C > ", "no", "os", "b"), ("X", "partial", "engine-gap", "c")];
        assert_eq!(headless_of("W > C > M", false, &marks), ("no", Some("os"), "b"));
        assert_eq!(headless_of("W > Mix", false, &marks), ("no", Some("window"), "a"));
        assert_eq!(headless_of("W > Mix", true, &marks).0, "yes", "a covered leaf is yes whatever the mark says");
        assert_eq!(headless_of("WX", false, &marks).0, "unknown", "a prefix mark needs the separator");
        let s = headless_summary(&[("yes", None), ("no", Some("window")), ("no", Some("os")), ("unknown", None)]);
        assert!(s.contains("1 yes, 2 no (window 1, os 1, engine-gap 0), 0 partial, 1 unknown"), "{s}");
    }

    #[test]
    fn bad_headless_marks_are_refused() {
        let leaves = [("W > Mix".to_string(), false), ("X".to_string(), true)];
        let bad = [("W > ", "yes", "window", ""), ("W > ", "no", "desk", ""), ("X", "no", "os", ""), ("Z", "no", "os", "")];
        let errs = mark_errors(&bad, &leaves);
        for want in ["is not `no` or `partial`", "reach `desk`", "written twice", "`X` is stale", "`Z` matches no catalog leaf"] {
            assert!(errs.iter().any(|e| e.contains(want)), "{want}: {errs:?}");
        }
    }

    #[test]
    fn checked_in_marks_name_catalog_leaves() {
        let catalog = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/data/menu-catalog.txt")).unwrap_or_default();
        let leaves: Vec<(String, bool)> = leaves(&catalog).into_iter().map(|l| (l, false)).collect();
        assert!(!leaves.is_empty());
        let errs = mark_errors(HEADLESS, &leaves);
        assert!(errs.is_empty(), "{errs:?}");
    }

    #[test]
    fn leaves_with_paths() {
        let c = "Draw\n    Line\n    Circle\n        3 Points\n        2 Points\nModify\n    Move\n";
        assert_eq!(leaves(c), vec!["Draw > Line", "Draw > Circle > 3 Points", "Draw > Circle > 2 Points", "Modify > Move"]);
    }
}
