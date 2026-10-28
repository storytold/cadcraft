//! What a DWG that AutoCAD saved loses when CADCraft opens and saves it, object by object, as
//! CADCraft's own DWG reader (`cadcraft_dwg::codec`) sees both files: each object's extended data
//! (xdata), its persistent reactors and its extension dictionary, by handle. Needs the MI-201
//! corpus (`MI201_CORPUS` names its folder; not part of the repository).

use std::collections::BTreeMap;

use cadcraft_dwg::codec::{CommonData, DwgContainer, read_container, type_name};

/// Each object's type and common data by handle (objects whose common data doesn't read are
/// left out of the comparison and counted).
fn objects(c: &DwgContainer) -> (BTreeMap<u64, (String, CommonData)>, usize) {
    let mut out = BTreeMap::new();
    let mut unread = 0;
    for (r, o) in c.objects_iter() {
        let Ok(o) = o else {
            unread += 1;
            continue;
        };
        let (name, entity) = type_name(o.type_code, &c.classes);
        match o.common(c.version, entity) {
            Ok(common) => {
                out.insert(r.handle, (name, common));
            }
            Err(_) => unread += 1,
        }
    }
    (out, unread)
}

/// The losses of one file, by kind: (handle, type) of each object that lost something.
#[derive(Default, Debug)]
struct Losses {
    objects: usize,
    unread: usize,
    missing: Vec<(u64, String)>,
    xdata: Vec<(u64, String)>,
    reactors: Vec<(u64, String)>,
    xdict: Vec<(u64, String)>,
}

fn losses(before: &[u8], after: &[u8]) -> Losses {
    let (src, unread) = objects(&read_container(before).unwrap());
    let (out, _) = objects(&read_container(after).unwrap());
    let mut l = Losses { objects: src.len(), unread, ..Losses::default() };
    for (h, (name, c)) in &src {
        let tag = (*h, name.clone());
        let Some((_, o)) = out.get(h).filter(|(n, _)| n == name) else {
            l.missing.push(tag);
            continue;
        };
        // Every block of extended data the file had, byte for byte (the application by handle).
        if !c.extended_data.iter().all(|b| o.extended_data.contains(b)) {
            l.xdata.push(tag.clone());
        }
        if !c.reactors.iter().all(|r| o.reactors.contains(r)) {
            let gone: Vec<String> =
                c.reactors.iter().filter(|r| !o.reactors.contains(r)).map(|r| src.get(r).map_or(format!("{r:X}?"), |x| x.0.clone())).collect();
            l.reactors.push((*h, format!("{name}->{}", gone.join("+"))));
        }
        if let (Some(a), Some(b)) = (c.xdict, o.xdict)
            && a != 0
            && b != a
        {
            l.xdict.push(tag);
        }
    }
    l
}

/// Count by type, most first.
fn by_type(v: &[(u64, String)]) -> String {
    let mut m: BTreeMap<&str, usize> = BTreeMap::new();
    for (_, t) in v {
        *m.entry(t).or_default() += 1;
    }
    let mut v: Vec<_> = m.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    v.iter().map(|(t, n)| format!("{t} {n}")).collect::<Vec<_>>().join(", ")
}

/// AutoCAD's DWGs opened and saved by CADCraft keep every object's extended data, persistent
/// reactors and extension dictionary (and the object itself, by handle and type), 2004 to 2018.
/// The 2000 file is saved as 2004, which has no VX table control. The losses are printed with
/// `--no-capture`.
#[test]
fn autocad_dwgs_keep_every_objects_common_data() {
    let Some(dir) = std::env::var_os("MI201_CORPUS").map(std::path::PathBuf::from).filter(|d| d.is_dir()) else { return };
    for name in ["mi201-r2018.dwg", "mi201-r2013.dwg", "mi201-r2010.dwg", "mi201-r2004.dwg", "mi201-r2000.dwg"] {
        let Ok(bytes) = std::fs::read(dir.join(name)) else { continue };
        let d = cadcraft_io::read(&bytes, name).unwrap();
        let saved = cadcraft_io::write(&d, name).unwrap();
        let mut l = losses(&bytes, &saved);
        eprintln!("{name}: {} objects ({} unread)", l.objects, l.unread);
        for (what, v) in
            [("missing or retyped", &l.missing), ("extended data", &l.xdata), ("reactors", &l.reactors), ("extension dictionary", &l.xdict)]
        {
            eprintln!("  {what}: {} [{}]", v.len(), by_type(v));
        }
        l.missing.retain(|(_, t)| t != "VX_CONTROL");
        assert_eq!(l.unread, 0, "{name}: objects that don't read");
        for (what, v) in
            [("missing or retyped", &l.missing), ("extended data", &l.xdata), ("reactors", &l.reactors), ("extension dictionary", &l.xdict)]
        {
            assert!(v.is_empty(), "{name}: {what} lost: {}", by_type(v));
        }
    }
}

/// Each record of a DXF the bridge made from a DWG, by handle: its type and (code, value) groups.
fn records_by_handle(dxf: &[u8]) -> BTreeMap<String, (String, Vec<(i32, String)>)> {
    let tags = cadcraft_dxf::parse(dxf).unwrap();
    let mut out = BTreeMap::new();
    let mut cur: Option<(String, Vec<(i32, String)>)> = None;
    let mut flush = |cur: &mut Option<(String, Vec<(i32, String)>)>| {
        if let Some((kind, groups)) = cur.take()
            && let Some((_, h)) = groups.iter().find(|(c, _)| *c == 5 || *c == 105)
        {
            out.insert(h.clone(), (kind, groups));
        }
    };
    for t in tags {
        if t.code == 0 {
            flush(&mut cur);
            cur = Some((t.str(), Vec::new()));
        } else if let Some((_, g)) = cur.as_mut() {
            g.push((t.code, t.str().trim().to_string()));
        }
    }
    flush(&mut cur);
    out
}

/// Field by field, as the bridge reads both files (printed with `--no-capture`): what still
/// differs between AutoCAD's DWG and CADCraft's save of it, record by record. A report, not a
/// check: CADCraft adds its own data (its xdata, named object dictionary entries, application) and
/// some differences are its known gaps (docs/file-format-parity.md).
#[test]
fn dwg_corpus_fields_report() {
    let Some(dir) = std::env::var_os("MI201_CORPUS").map(std::path::PathBuf::from).filter(|d| d.is_dir()) else { return };
    for name in ["mi201-r2018.dwg", "mi201-r2013.dwg", "mi201-r2010.dwg", "mi201-r2004.dwg", "mi201-r2000.dwg"] {
        let Ok(bytes) = std::fs::read(dir.join(name)) else { continue };
        let d = cadcraft_io::read(&bytes, name).unwrap();
        let saved = cadcraft_io::write(&d, name).unwrap();
        let (a, b) = (records_by_handle(&cadcraft_dwg::dwg_to_dxf(&bytes).unwrap()), records_by_handle(&cadcraft_dwg::dwg_to_dxf(&saved).unwrap()));
        let mut differ: BTreeMap<String, usize> = BTreeMap::new();
        let mut samples = Vec::new();
        for (h, (kind, ga)) in &a {
            let Some((_, gb)) = b.get(h) else {
                *differ.entry(format!("{kind} (gone)")).or_default() += 1;
                continue;
            };
            if ga != gb {
                *differ.entry(kind.clone()).or_default() += 1;
                if samples.len() < 12 {
                    let only_a: Vec<_> = ga.iter().filter(|g| !gb.contains(g)).take(4).collect();
                    let only_b: Vec<_> = gb.iter().filter(|g| !ga.contains(g)).take(4).collect();
                    samples.push(format!("{kind} {h}: file {only_a:?} / saved {only_b:?}"));
                }
            }
        }
        eprintln!("{name}: {} records, {} differ: {differ:?}", a.len(), differ.values().sum::<usize>());
        for s in samples {
            eprintln!("    {s}");
        }
    }
}
