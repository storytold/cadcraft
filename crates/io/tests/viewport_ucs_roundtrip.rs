//! A layout viewport's own UCS (UCSVP and groups 110–112, 79, 146, 345, 346) and the groups
//! CADCraft doesn't model (snap, grid, lens, plot style, visual style, lighting…) survive reading,
//! saving as DXF or DWG and copying (#1316).

use cadcraft_doc::*;
use cadcraft_geom::{Vec2, Vec3};

fn viewport() -> EntityKind {
    EntityKind::Viewport(Viewport {
        center: Vec3::new(50.0, 40.0, 0.0),
        width: 80.0,
        height: 60.0,
        view_center: Vec2::new(10.0, 10.0),
        view_height: 50.0,
        id: 2,
        ..Viewport::default()
    })
}

/// A drawing with two named UCSs and one layout viewport, saved by CADCraft.
fn base() -> (Drawing, Handle) {
    let mut d = Drawing::default();
    d.ucss.push(Ucs { name: "SITE".into(), origin: Vec3::new(100.0, 50.0, 0.0), x_axis: Vec3::new(1.0, 0.0, 0.0), y_axis: Vec3::new(0.0, 1.0, 0.0) });
    d.ucss.push(Ucs { name: "BASE".into(), origin: Vec3::ZERO, x_axis: Vec3::new(1.0, 0.0, 0.0), y_axis: Vec3::new(0.0, 1.0, 0.0) });
    let paper = Space::Paper(d.layouts[0].name.clone());
    let h = d.add(&paper, Common::default(), viewport()).unwrap();
    (d, h)
}

/// The (code, value) pairs of every `kind` record in DXF text.
fn records(text: &str, kind: &str) -> Vec<Vec<(String, String)>> {
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let mut out: Vec<Vec<(String, String)>> = Vec::new();
    let mut inside = false;
    for pair in lines.chunks(2) {
        let [code, value] = pair else { break };
        if *code == "0" {
            inside = *value == kind;
            if inside {
                out.push(Vec::new());
            }
        } else if inside && let Some(rec) = out.last_mut() {
            rec.push((code.to_string(), value.to_string()));
        }
    }
    out
}

fn group<'a>(rec: &'a [(String, String)], code: &str) -> Option<&'a str> {
    rec.iter().find(|(c, _)| c == code).map(|(_, v)| v.as_str())
}

/// The handle of the `kind` table record named `name`.
fn record_handle(text: &str, kind: &str, name: &str) -> String {
    records(text, kind).into_iter().find(|r| group(r, "2") == Some(name)).and_then(|r| group(&r, "5").map(str::to_string)).unwrap()
}

/// The viewport record with handle `h`.
fn viewport_record(text: &str, h: Handle) -> Vec<(String, String)> {
    records(text, "VIEWPORT").into_iter().find(|r| group(r, "5") == Some(h.hex().as_str())).unwrap()
}

/// `text` with `groups` put into viewport `h`'s record after its status flags (group 90).
fn inject(text: &str, h: Handle, groups: &[(i32, String)]) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let mut out: Vec<String> = Vec::new();
    let (mut in_vp, mut done) = (false, false);
    let mut i = 0;
    while i + 1 < lines.len() {
        let (code, value) = (lines[i].trim(), lines[i + 1].trim());
        if code == "0" {
            in_vp = value == "VIEWPORT";
        }
        let ours = in_vp && !done;
        out.push(lines[i].to_string());
        out.push(lines[i + 1].to_string());
        if ours && code == "5" && value != h.hex() {
            in_vp = false;
        }
        if ours && code == "90" {
            for (c, v) in groups {
                out.push(c.to_string());
                out.push(v.clone());
            }
            done = true;
        }
        i += 2;
    }
    assert!(done, "viewport {h:?} not found");
    out.join("\n") + "\n"
}

const C6: f64 = 0.994_521_895_368_273_3;
const S6: f64 = 0.104_528_463_267_653_47;

/// The groups another program writes on a viewport with a 6° UCS, after its status flags:
/// UCS, then the ones CADCraft doesn't model, with a visual style handle (`style`), plus the view
/// twist, direction and target that must not be carried raw.
fn foreign_groups(site: &str, base_ucs: &str, style: &str) -> Vec<(i32, String)> {
    let mut g: Vec<(i32, String)> = vec![
        (13, "1.0".into()),
        (23, "2.0".into()),
        (14, "0.5".into()),
        (24, "0.5".into()),
        (15, "10.0".into()),
        (25, "10.0".into()),
        (16, "0.0".into()),
        (26, "0.0".into()),
        (36, "1.0".into()),
        (17, "0.0".into()),
        (27, "0.0".into()),
        (37, "0.0".into()),
        (42, "50.0".into()),
        (43, "0.0".into()),
        (44, "0.0".into()),
        (50, "0.0".into()),
        (51, "6.0".into()),
        (72, "1000".into()),
        (1, "monochrome.ctb".into()),
        (281, "0".into()),
        (71, "1".into()),
        (74, "0".into()),
    ];
    for (c, v) in [(110, [100.0, 50.0, 0.0]), (111, [C6, S6, 0.0]), (112, [-S6, C6, 0.0])] {
        for (k, x) in v.iter().enumerate() {
            g.push((c + 10 * k as i32, format!("{x:?}")));
        }
    }
    g.extend([
        (79, "0".into()),
        (146, "2.5".into()),
        (345, site.into()),
        (346, base_ucs.into()),
        (170, "0".into()),
        (61, "5".into()),
        (348, style.into()),
        (292, "1".into()),
        (282, "1".into()),
        (141, "0.0".into()),
        (142, "0.0".into()),
        (63, "250".into()),
    ]);
    g
}

/// The codes of the unmodelled groups, in file order.
const EXTRA: &[i32] = &[13, 23, 14, 24, 15, 25, 42, 43, 44, 50, 72, 1, 281, 74, 170, 61, 348, 292, 282, 141, 142, 63];

fn the_viewport(d: &Drawing) -> (Handle, Viewport) {
    d.layouts
        .iter()
        .flat_map(|l| l.entities.iter())
        .find_map(|e| if let EntityKind::Viewport(v) = &e.kind { (v.id != 1).then(|| (e.handle, v.clone())) } else { None })
        .unwrap()
}

/// A drawing whose viewport carries another program's UCS and unmodelled groups, its visual
/// style handle pointing at layer 0's record (a record CADCraft writes itself, under a new
/// handle on save).
fn foreign() -> Drawing {
    let (d, h) = base();
    let text = cadcraft_io::write_dxf(&d);
    let (site, base_ucs, layer0) = (record_handle(&text, "UCS", "SITE"), record_handle(&text, "UCS", "BASE"), record_handle(&text, "LAYER", "0"));
    cadcraft_io::read_dxf(inject(&text, h, &foreign_groups(&site, &base_ucs, &layer0)).as_bytes()).unwrap()
}

fn expected_ucs() -> ViewportUcs {
    ViewportUcs {
        origin: Vec3::new(100.0, 50.0, 0.0),
        x_axis: Vec3::new(C6, S6, 0.0),
        y_axis: Vec3::new(-S6, C6, 0.0),
        elevation: 2.5,
        ortho: 0,
        named: Some("SITE".into()),
        base: Some("BASE".into()),
    }
}

fn num(v: &str) -> Option<f64> {
    v.trim().parse::<f64>().ok()
}

/// The unmodelled groups agree: same codes in the same order, same values (numbers compared as
/// numbers), the visual style pointing at layer 0 in each file.
fn assert_extra(text: &str, rec: &[(String, String)]) {
    let codes: Vec<i32> = rec.iter().filter_map(|(c, _)| c.parse::<i32>().ok()).filter(|c| EXTRA.contains(c)).collect();
    assert_eq!(codes, EXTRA, "{rec:?}");
    for (code, want) in foreign_groups("", "", "").into_iter().filter(|(c, _)| EXTRA.contains(c) && *c != 348) {
        let got = group(rec, &code.to_string()).unwrap();
        match (num(got), num(&want)) {
            (Some(a), Some(b)) => assert!((a - b).abs() < 1e-12, "{code}: {got} vs {want}"),
            _ => assert_eq!(got, want, "{code}"),
        }
    }
    assert_eq!(group(rec, "348"), Some(record_handle(text, "LAYER", "0").as_str()), "visual style pointer");
}

#[test]
fn another_program_s_viewport_ucs_and_settings_are_read() {
    let d = foreign();
    let (_, v) = the_viewport(&d);
    assert!(v.ucs_per_viewport);
    assert_eq!(v.ucs, Some(expected_ucs()));
    let codes: Vec<i32> = v.extra.iter().map(|t| t.code).collect();
    assert_eq!(codes, EXTRA);
}

#[test]
fn viewport_ucs_and_settings_survive_a_dxf_save() {
    let d = foreign();
    let (h, v) = the_viewport(&d);
    let text = cadcraft_io::write_dxf(&d);
    let rec = viewport_record(&text, h);
    assert_eq!(group(&rec, "71"), Some("1"));
    assert_eq!(group(&rec, "345"), Some(record_handle(&text, "UCS", "SITE").as_str()));
    assert_eq!(group(&rec, "346"), Some(record_handle(&text, "UCS", "BASE").as_str()));
    assert_eq!(num(group(&rec, "146").unwrap()), Some(2.5));
    assert_extra(&text, &rec);
    // Written once each, and the view twist, direction and target are not carried raw.
    for code in EXTRA.iter().chain(&[71, 110, 345, 346]) {
        assert_eq!(rec.iter().filter(|(c, _)| *c == code.to_string()).count(), 1, "group {code} in {rec:?}");
    }
    for code in ["51", "16", "17"] {
        assert!(group(&rec, code).is_none(), "{code} in {rec:?}");
    }
    // And it reads back the same.
    let back = cadcraft_io::read_dxf(text.as_bytes()).unwrap();
    let (_, w) = the_viewport(&back);
    assert_eq!((w.ucs, w.ucs_per_viewport), (v.ucs, v.ucs_per_viewport));
    let again = cadcraft_io::write_dxf(&back);
    assert_extra(&again, &viewport_record(&again, h));
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn viewport_ucs_and_settings_survive_a_dwg_save() {
    let d = foreign();
    let dwg = cadcraft_io::write(&d, "x.dwg").unwrap();
    let back = cadcraft_io::read(&dwg, "x.dwg").unwrap();
    let (h, v) = the_viewport(&back);
    assert!(v.ucs_per_viewport);
    let u = v.ucs.clone().unwrap();
    let want = expected_ucs();
    for (a, b) in [(u.origin, want.origin), (u.x_axis, want.x_axis), (u.y_axis, want.y_axis)] {
        assert!((a - b).len() < 1e-12, "{a:?} vs {b:?}");
    }
    assert_eq!((u.elevation, u.ortho, u.named.as_deref(), u.base.as_deref()), (2.5, 0, Some("SITE"), Some("BASE")));
    // Every group that isn't a reference comes back with its value, also those the DWG bridge
    // drops from the record (plot style sheet, shade plot, lighting: from CADCraft's xdata copy).
    // The visual style reference (348) is lost through DWG (docs/file-format-parity.md).
    let text = cadcraft_io::write_dxf(&back);
    let rec = viewport_record(&text, h);
    for (code, want) in foreign_groups("", "", "").into_iter().filter(|(c, _)| EXTRA.contains(c) && *c != 348) {
        let got = group(&rec, &code.to_string()).unwrap_or_else(|| panic!("group {code} lost through DWG: {rec:?}"));
        match (num(got), num(&want)) {
            (Some(a), Some(b)) => assert!((a - b).abs() < 1e-12, "{code}: {got} vs {want}"),
            _ => assert_eq!(got, want, "{code}"),
        }
    }
    // And a second trip through DWG changes nothing more.
    let twice = cadcraft_io::read(&cadcraft_io::write(&back, "x.dwg").unwrap(), "x.dwg").unwrap();
    let (_, w) = the_viewport(&twice);
    assert_eq!((w.ucs, w.ucs_per_viewport), (v.ucs, v.ucs_per_viewport));
    let mut a: Vec<i32> = v.extra.iter().map(|t| t.code).collect();
    let mut b: Vec<i32> = w.extra.iter().map(|t| t.code).collect();
    a.sort_unstable();
    b.sort_unstable();
    assert_eq!(a, b);
}

#[test]
fn a_copied_viewport_keeps_its_ucs_and_settings() {
    let mut d = foreign();
    let (h, v) = the_viewport(&d);
    let paper = Space::Paper(d.layouts[0].name.clone());
    let copy = d.add(&paper, Common::default(), EntityKind::Viewport(Viewport { id: 3, center: Vec3::new(150.0, 40.0, 0.0), ..v.clone() })).unwrap();
    let text = cadcraft_io::write_dxf(&d);
    for vh in [h, copy] {
        let rec = viewport_record(&text, vh);
        assert_eq!(group(&rec, "345"), Some(record_handle(&text, "UCS", "SITE").as_str()), "{vh:?}");
        assert_extra(&text, &rec);
    }
}

#[test]
fn a_viewport_without_a_ucs_is_written_as_before() {
    let (d, h) = base();
    let text = cadcraft_io::write_dxf(&d);
    let rec = viewport_record(&text, h);
    for code in ["71", "110", "111", "112", "79", "146", "345", "346"] {
        assert!(group(&rec, code).is_none(), "{code} in {rec:?}");
    }
    let (_, v) = the_viewport(&cadcraft_io::read_dxf(text.as_bytes()).unwrap());
    assert_eq!((v.ucs, v.ucs_per_viewport, v.extra.len()), (None, false, 0));
}

#[test]
fn a_deleted_named_ucs_is_not_pointed_at() {
    let mut d = foreign();
    d.ucss.retain(|u| u.name != "SITE");
    let (h, _) = the_viewport(&d);
    let text = cadcraft_io::write_dxf(&d);
    let rec = viewport_record(&text, h);
    assert!(group(&rec, "345").is_none(), "{rec:?}");
    assert_eq!(group(&rec, "346"), Some(record_handle(&text, "UCS", "BASE").as_str()));
}

/// CADCraft's `VPEXTRA` copy of a viewport's unmodelled groups can't smuggle structure into the
/// record: a record start, handles, a subclass, a group brace, an owner or xdata are dropped on
/// read, so a save writes nothing the user didn't draw.
#[test]
fn vpextra_cannot_inject_records_or_handles() {
    let (d, h) = base();
    let text = cadcraft_io::write_dxf(&d);
    let lines_before = d.model.iter().filter(|e| matches!(e.kind, EntityKind::Line(_))).count();
    let mut g: Vec<(i32, String)> = vec![(1001, "CADCRAFT".into()), (1000, "VPEXTRA".into()), (1002, "{".into())];
    for (code, value) in
        [("0", "LINE"), ("5", "1F"), ("100", "AcDbLine"), ("102", "{ACAD_REACTORS"), ("105", "2A"), ("330", "1"), ("1001", "X"), ("13", "4.5")]
    {
        g.push((1000, code.into()));
        g.push((1000, value.into()));
    }
    g.push((1002, "}".into()));
    let back = cadcraft_io::read_dxf(inject(&text, h, &g).as_bytes()).unwrap();
    let (_, v) = the_viewport(&back);
    let codes: Vec<i32> = v.extra.iter().map(|t| t.code).collect();
    assert!(codes.iter().all(|c| (1..1000).contains(c) && !matches!(c, 5 | 100 | 102 | 105 | 330)), "{codes:?}");
    assert!(v.extra.iter().any(|t| t.code == 13 && t.value.trim() == "4.5"), "a viewport's own group is still kept: {:?}", v.extra);
    // Saved and opened again: still one viewport, no LINE it never had, no stray subclass.
    let saved = cadcraft_io::write_dxf(&back);
    assert!(!saved.lines().any(|l| l.trim() == "AcDbLine"), "no injected subclass");
    let again = cadcraft_io::read_dxf(saved.as_bytes()).unwrap();
    assert_eq!(again.model.iter().filter(|e| matches!(e.kind, EntityKind::Line(_))).count(), lines_before, "no injected LINE");
    assert!(the_viewport(&again).1.extra.iter().all(|t| (1..1000).contains(&t.code)));
}

/// Structural groups inside the record's own AcDbViewport subclass (a second handle, a group
/// brace) aren't kept either: written back, the record would have two handles.
#[test]
fn a_viewport_records_own_structural_groups_are_not_kept() {
    let (d, h) = base();
    let text = cadcraft_io::write_dxf(&d);
    let g: Vec<(i32, String)> = vec![(5, "FFFFF1".into()), (105, "FFFFF2".into()), (102, "{ACAD_REACTORS".into()), (13, "4.5".into())];
    let back = cadcraft_io::read_dxf(inject(&text, h, &g).as_bytes()).unwrap();
    let (_, v) = the_viewport(&back);
    let codes: Vec<i32> = v.extra.iter().map(|t| t.code).collect();
    assert!(codes.iter().all(|c| !matches!(c, 5 | 102 | 105)), "{codes:?}");
    assert!(v.extra.iter().any(|t| t.code == 13 && t.value.trim() == "4.5"), "a viewport's own group is still kept: {:?}", v.extra);
    let saved = cadcraft_io::write_dxf(&back);
    assert!(!saved.lines().any(|l| l.trim() == "FFFFF1" || l.trim() == "FFFFF2"), "no second handle");
}

/// A viewport whose only UCS group is its base UCS handle (346) keeps that base UCS.
#[test]
fn a_base_ucs_handle_alone_is_kept() {
    let (d, h) = base();
    let text = cadcraft_io::write_dxf(&d);
    assert!(the_viewport(&d).1.ucs.is_none() && group(&viewport_record(&text, h), "110").is_none(), "no UCS groups of its own");
    let handle = record_handle(&text, "UCS", "BASE");
    let back = cadcraft_io::read_dxf(inject(&text, h, &[(346, handle)]).as_bytes()).unwrap();
    let u = the_viewport(&back).1.ucs.expect("a UCS from 346 alone");
    assert_eq!(u.base.as_deref(), Some("BASE"));
    let again = cadcraft_io::read_dxf(cadcraft_io::write_dxf(&back).as_bytes()).unwrap();
    assert_eq!(the_viewport(&again).1.ucs.and_then(|u| u.base).as_deref(), Some("BASE"), "kept through a save");
}

#[test]
fn hostile_viewport_ucs_groups_do_not_panic() {
    let (d, h) = base();
    let text = cadcraft_io::write_dxf(&d);
    let mut g: Vec<(i32, String)> = vec![
        (71, "7".into()),
        (110, "nan".into()),
        (120, "inf".into()),
        (111, "1e400".into()),
        (79, "99".into()),
        (146, "-inf".into()),
        (345, "ZZZZ".into()),
    ];
    // Far more unmodelled groups than any viewport has.
    g.extend((0..5000).map(|i| (13, i.to_string())));
    let back = cadcraft_io::read_dxf(inject(&text, h, &g).as_bytes()).unwrap();
    let (_, v) = the_viewport(&back);
    assert_eq!(v.ucs, None, "unreadable axes give no UCS");
    assert!(!v.ucs_per_viewport, "71 is 1 or 0");
    assert!(v.extra.len() <= 512, "{}", v.extra.len());
    let _ = cadcraft_io::write_dxf(&back);
    // A finite UCS with a nonsense orthographic type, elevation and named UCS.
    let g: Vec<(i32, String)> = vec![(110, "1.0".into()), (79, "99".into()), (146, "nan".into()), (345, "ZZZZ".into()), (346, "".into())];
    let back = cadcraft_io::read_dxf(inject(&text, h, &g).as_bytes()).unwrap();
    let u = the_viewport(&back).1.ucs.unwrap();
    assert_eq!((u.ortho, u.elevation, u.named, u.base), (0, 0.0, None, None));
    assert_eq!((u.x_axis, u.y_axis), (Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0)));
}
