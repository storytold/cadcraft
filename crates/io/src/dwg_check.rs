//! What CADCraft's own DWG reader (`cadcraft_dwg::codec`) finds in a file, used to correct what the
//! DWG bridge (acadrust, through DXF) hands over.

use cadcraft_doc::{Drawing, Handle, KeptHandle};

/// The space blocks' end markers keep the file's handles. The bridge gives the paper space block's
/// ENDBLK a new handle; the file records which BLOCK and ENDBLK sit in each space, so a layout whose
/// begin marker is the file's and whose end marker is a handle the file doesn't have gets the
/// file's end marker back (when nothing else in the drawing has that handle).
pub(crate) fn space_block_ends(d: &mut Drawing, bytes: &[u8]) {
    let Ok(c) = cadcraft_dwg::codec::read_container(bytes) else { return };
    let m = c.space_markers();
    let free = |d: &Drawing, h: u64| d.entity(Handle(h)).is_none() && !d.layouts.iter().any(|l| l.end_handle.get() == Handle(h));
    let fix = |begin: KeptHandle, end: KeptHandle, file_begin: Option<u64>, file_end: Option<u64>| -> Option<KeptHandle> {
        let (b, e) = (file_begin?, file_end?);
        (begin.get() == Handle(b) && end.get() != Handle(e) && !c.has_handle(end.get().0)).then_some(KeptHandle(Handle(e)))
    };
    for i in 0..d.layouts.len() {
        let Some(l) = d.layouts.get(i) else { continue };
        if let Some(new) = fix(l.begin_handle, l.end_handle, m.paper_begin, m.paper_end)
            && free(d, new.get().0)
            && let Some(l) = d.layouts.get_mut(i)
        {
            l.end_handle = new;
        }
    }
    if let Some(l) = d.model_layout.as_deref()
        && let Some(new) = fix(l.begin_handle, l.end_handle, m.model_begin, m.model_end)
        && free(d, new.get().0)
        && let Some(l) = d.model_layout.as_deref_mut()
    {
        l.end_handle = new;
    }
}

/// Most DXF lines looked at (a DXF the bridge or the writer made is always well under it).
const MAX_DXF_LINES: usize = 64_000_000;

/// A DXF as (code, value) line pairs over its bytes: the code's line and the value's line, each
/// with its line ending, so the file can be put back together exactly. Group codes and handles are
/// ASCII in any DXF; the rest (text before 2007 is in the drawing's code page) is only copied.
fn dxf_pairs(bytes: &[u8]) -> Vec<(&[u8], &[u8])> {
    let lines: Vec<&[u8]> = bytes.split_inclusive(|b| *b == b'\n').take(MAX_DXF_LINES).collect();
    lines.chunks_exact(2).filter_map(|p| Some((*p.first()?, *p.get(1)?))).collect()
}

/// A DXF line without its line ending and surrounding blanks.
fn trimmed(line: &[u8]) -> &[u8] {
    line.trim_ascii()
}

/// A line as a handle, when it is one.
fn line_handle(line: &[u8]) -> Option<u64> {
    std::str::from_utf8(trimmed(line)).ok().and_then(Handle::parse_hex).map(|h| h.0)
}

/// Each record's handle (group 5, or 105 for DIMSTYLE) with the persistent reactors its
/// `{ACAD_REACTORS` group names, for every record that has a handle.
fn dxf_reactors(bytes: &[u8]) -> std::collections::HashMap<u64, Vec<u64>> {
    let mut out = std::collections::HashMap::new();
    let mut handle: Option<u64> = None;
    let mut reactors: Vec<u64> = Vec::new();
    let mut in_group = false;
    for (c, v) in dxf_pairs(bytes) {
        match trimmed(c) {
            b"0" => {
                if let Some(h) = handle.take() {
                    out.insert(h, std::mem::take(&mut reactors));
                }
                reactors.clear();
                in_group = false;
            }
            b"5" | b"105" if handle.is_none() => handle = line_handle(v),
            b"102" => in_group = trimmed(v) == b"{ACAD_REACTORS",
            b"330" if in_group => reactors.extend(line_handle(v)),
            _ => {}
        }
    }
    if let Some(h) = handle {
        out.insert(h, reactors);
    }
    out
}

/// The bridge's DXF of `bytes` with the persistent reactors it left out put back: every object
/// of the file whose record in the DXF lacks some of the object's reactors gets them (those that
/// name a record the DXF has), so the DXF reader keeps them as it keeps any other reactors. The
/// bridge drops them from table records (a dimension style's reactors to its dimensions).
pub(crate) fn add_dropped_reactors(dxf: Vec<u8>, bytes: &[u8]) -> Vec<u8> {
    let Ok(c) = cadcraft_dwg::codec::read_container(bytes) else { return dxf };
    let have = dxf_reactors(&dxf);
    let mut missing: std::collections::HashMap<u64, Vec<u64>> = std::collections::HashMap::new();
    for (r, o) in c.objects_iter() {
        let Ok(o) = o else { continue };
        let (_, entity) = cadcraft_dwg::codec::type_name(o.type_code, &c.classes);
        let Ok(common) = o.common(c.version, entity) else { continue };
        let Some(listed) = have.get(&r.handle) else { continue };
        let add: Vec<u64> = common.reactors.into_iter().filter(|h| !listed.contains(h) && have.contains_key(h)).collect();
        if !add.is_empty() {
            missing.insert(r.handle, add);
        }
    }
    if missing.is_empty() {
        return dxf;
    }
    put_reactors(&dxf, &missing)
}

/// `dxf` with the reactors of `missing` (by record handle) added to each record's
/// `{ACAD_REACTORS` group, which is made right after the handle when the record has none (where the
/// DXF Reference puts it).
fn put_reactors(dxf: &[u8], missing: &std::collections::HashMap<u64, Vec<u64>>) -> Vec<u8> {
    let eol: &[u8] = if dxf.windows(2).any(|w| w == b"\r\n") { b"\r\n" } else { b"\n" };
    let pair = |out: &mut Vec<u8>, c: &str, v: &str| {
        out.extend_from_slice(format!("{c:>3}").as_bytes());
        out.extend_from_slice(eol);
        out.extend_from_slice(v.as_bytes());
        out.extend_from_slice(eol);
    };
    let pairs = dxf_pairs(dxf);
    let mut out = Vec::with_capacity(dxf.len().saturating_add(missing.len().saturating_mul(32)));
    let mut add: Option<&Vec<u64>> = None;
    let mut in_group = false;
    let mut seen_handle = false;
    for (i, (c, v)) in pairs.iter().enumerate() {
        let (code, value) = (trimmed(c), trimmed(v));
        match code {
            b"0" => {
                add = None;
                in_group = false;
                seen_handle = false;
            }
            b"102" if value == b"}" && in_group => {
                // The close of the record's own reactors group: the missing ones go before it.
                for h in add.take().into_iter().flatten() {
                    pair(&mut out, "330", &format!("{h:X}"));
                }
                in_group = false;
            }
            b"102" if value == b"{ACAD_REACTORS" => in_group = add.is_some(),
            _ => {}
        }
        out.extend_from_slice(c);
        out.extend_from_slice(v);
        if matches!(code, b"5" | b"105") && !seen_handle {
            seen_handle = true;
            add = line_handle(v).and_then(|h| missing.get(&h));
            // The group, if the record has one, follows the handle directly.
            let has_group = pairs.get(i + 1).is_some_and(|(nc, nv)| trimmed(nc) == b"102" && trimmed(nv) == b"{ACAD_REACTORS");
            if add.is_some() && !has_group {
                pair(&mut out, "102", "{ACAD_REACTORS");
                for h in add.take().into_iter().flatten() {
                    pair(&mut out, "330", &format!("{h:X}"));
                }
                pair(&mut out, "102", "}");
            }
        }
    }
    // A trailing line without its pair (a damaged file) is kept as it was.
    let used: usize = pairs.iter().map(|(c, v)| c.len() + v.len()).sum();
    out.extend_from_slice(dxf.get(used..).unwrap_or_default());
    out
}

/// A DWG the bridge wrote from `dxf`, with the persistent reactors it left out put back (2004 and
/// later): every object whose record in `dxf` names reactors gets exactly those (the ones that
/// are objects of the file). The file is checked before it is used: read again, every object is
/// as the bridge wrote it but the ones given reactors, which have them; otherwise, and for files
/// before 2004, the bridge's file is kept. The bridge drops the reactor that ties a dictionary's
/// entries (scales, dictionary variables, styles) to their dictionary.
pub(crate) fn restore_reactors(dxf: &str, dwg: Vec<u8>) -> Vec<u8> {
    use cadcraft_dwg::codec::{edit, read_container, type_name};
    let Ok(c) = read_container(&dwg) else { return dwg };
    let want = dxf_reactors(dxf.as_bytes());
    let mut replace = std::collections::BTreeMap::new();
    for (r, o) in c.objects_iter() {
        let Ok(o) = o else { continue };
        let Some(w) = want.get(&r.handle) else { continue };
        let w: Vec<u64> = w.iter().copied().filter(|h| c.has_handle(*h)).collect();
        let (_, entity) = type_name(o.type_code, &c.classes);
        let Ok(have) = edit::reactors(&o, c.version, entity) else { continue };
        if have != w
            && w.iter().any(|h| !have.contains(h))
            && let Ok(stored) = edit::with_reactors(&o, c.version, entity, &w)
        {
            replace.insert(r.handle, (stored, w));
        }
    }
    if replace.is_empty() {
        return dwg;
    }
    let stored: std::collections::BTreeMap<u64, Vec<u8>> = replace.iter().map(|(h, (s, _))| (*h, s.clone())).collect();
    let Ok(new) = edit::replace_objects(&dwg, &stored) else { return dwg };
    // Read the new file back: the same objects, but for the reactors put back.
    let Ok(d) = read_container(&new) else { return dwg };
    let before: std::collections::HashMap<u64, usize> = c.map.iter().map(|r| (r.handle, r.offset)).collect();
    let ok = d.map.len() == c.map.len()
        && d.objects_iter().all(|(r, o)| {
            let Ok(o) = o else { return false };
            match replace.get(&r.handle) {
                Some((_, w)) => edit::reactors(&o, d.version, type_name(o.type_code, &d.classes).1).is_ok_and(|v| &v == w),
                None => before.get(&r.handle).and_then(|off| c.object_at(*off).ok()).is_some_and(|b| b == o),
            }
        });
    if ok { new } else { dwg }
}

/// A fingerprint of each record of a DXF CADCraft wrote, by handle: every group of the record.
pub(crate) fn fingerprints(dxf: &[u8]) -> std::collections::HashMap<u64, u64> {
    use std::hash::{Hash, Hasher};
    let mut out = std::collections::HashMap::new();
    let mut cur: Option<(Option<u64>, std::collections::hash_map::DefaultHasher)> = None;
    let mut finish = |cur: Option<(Option<u64>, std::collections::hash_map::DefaultHasher)>| {
        if let Some((Some(h), hasher)) = cur {
            out.insert(h, hasher.finish());
        }
    };
    for (c, v) in dxf_pairs(dxf) {
        let (code, value) = (trimmed(c), trimmed(v));
        if code == b"0" {
            finish(cur.take());
            cur = Some((None, std::collections::hash_map::DefaultHasher::new()));
        }
        if let Some((h, hasher)) = cur.as_mut() {
            if h.is_none() && matches!(code, b"5" | b"105") {
                *h = line_handle(v);
            }
            code.hash(hasher);
            value.hash(hasher);
        }
    }
    finish(cur);
    out
}

/// Each record's references in a DXF CADCraft wrote, by handle: the handles of its pointer groups
/// (330 to 369 and 390 to 399, before its extended data).
pub(crate) fn record_references(dxf: &[u8]) -> std::collections::HashMap<u64, std::collections::BTreeSet<u64>> {
    let mut out = std::collections::HashMap::new();
    let mut cur: Option<(Option<u64>, std::collections::BTreeSet<u64>, bool)> = None;
    let mut finish = |cur: Option<(Option<u64>, std::collections::BTreeSet<u64>, bool)>| {
        if let Some((Some(h), refs, _)) = cur {
            out.insert(h, refs);
        }
    };
    for (c, v) in dxf_pairs(dxf) {
        let code = trimmed(c);
        if code == b"0" {
            finish(cur.take());
            cur = Some((None, std::collections::BTreeSet::new(), false));
            continue;
        }
        let Some((h, refs, xdata)) = cur.as_mut() else { continue };
        let Some(n) = std::str::from_utf8(code).ok().and_then(|c| c.parse::<u32>().ok()) else { continue };
        *xdata |= n >= 1000;
        if h.is_none() && matches!(n, 5 | 105) {
            *h = line_handle(v);
        } else if !*xdata
            && matches!(n, 330..=369 | 390..=399)
            && let Some(r) = line_handle(v)
        {
            refs.insert(r);
        }
    }
    finish(cur);
    out
}

/// Whether the source file's object `was` references what the bridge's `out` does: the same
/// handles; or, for a dictionary (whose entries CADCraft's record lists, so its fingerprint
/// covers them), all of `out`'s and entries the bridge left out that the new file still has; or
/// the same references in the same places except where the bridge wrote another than CADCraft's
/// record (`record`) names: there the file's is the one the record names, and the bridge's one
/// the record doesn't (a dimension style's text style reference naming the UCS table, which the
/// bridge turns into the standard text style).
fn same_references(
    name: &str,
    was: &cadcraft_dwg::codec::RawObject,
    out: &cadcraft_dwg::codec::RawObject,
    o: &cadcraft_dwg::codec::DwgContainer,
    version: cadcraft_dwg::codec::DwgVersion,
    record: Option<&std::collections::BTreeSet<u64>>,
) -> bool {
    use cadcraft_dwg::codec::edit::handle_refs;
    let (Ok(a), Ok(b)) = (handle_refs(was, version), handle_refs(out, o.version)) else { return false };
    if let Some(record) = record
        && a.len() == b.len()
        && a.iter().zip(&b).all(|(x, y)| x == y || (record.contains(x) && !record.contains(y) && o.has_handle(*x)))
    {
        return true;
    }
    let (a, b): (std::collections::BTreeSet<u64>, std::collections::BTreeSet<u64>) = (a.into_iter().collect(), b.into_iter().collect());
    if a == b {
        return true;
    }
    matches!(name, "DICTIONARY" | "ACDBDICTIONARYWDFLT") && b.is_subset(&a) && a.iter().all(|h| *h == 0 || o.has_handle(*h))
}

/// A DWG the bridge wrote from `dxf` for `d`, with every object CADCraft hasn't changed since `d`
/// was read from a DWG of this version written back as that file had it, byte for byte: the
/// bridge drops or changes what it doesn't model (a multileader style's draw order and block
/// flags, the entries of a 2004 file's material dictionary…). An object counts as unchanged
/// when CADCraft writes its record as it did right after reading ([`DwgSource::fingerprints`]),
/// it has the same type in both files (a class object gets the new file's class number), and it
/// references what the object the bridge wrote does ([`same_references`]): DWG-only lists (a
/// block's entities, a reference's attributes) then say the same as the bridge's. The file is read
/// back before it is used: the copied objects are the source's, every other object the bridge's;
/// otherwise the bridge's file is kept.
///
/// [`DwgSource::fingerprints`]: cadcraft_doc::DwgSource::fingerprints
pub(crate) fn restore_unchanged(d: &Drawing, dxf: &str, dwg: Vec<u8>) -> Vec<u8> {
    use cadcraft_dwg::codec::{edit, read_container, type_name};
    let Some(src) = d.dwg_source.as_deref() else { return dwg };
    if src.bytes.get(..6) != dwg.get(..6) {
        return dwg;
    }
    let (Ok(s), Ok(o)) = (read_container(&src.bytes), read_container(&dwg)) else { return dwg };
    let now = fingerprints(dxf.as_bytes());
    let records = record_references(dxf.as_bytes());
    let src_at: std::collections::HashMap<u64, usize> = s.map.iter().map(|r| (r.handle, r.offset)).collect();
    let mut replace = std::collections::BTreeMap::new();
    for (r, out) in o.objects_iter() {
        let Ok(out) = out else { continue };
        let Some(&at) = src_at.get(&r.handle) else { continue };
        if !now.contains_key(&r.handle) || now.get(&r.handle) != src.fingerprints.get(&r.handle) {
            continue;
        }
        let Ok(was) = s.object_at(at) else { continue };
        let (name, _) = type_name(was.type_code, &s.classes);
        if was == out || name != type_name(out.type_code, &o.classes).0 || !same_references(&name, &was, &out, &o, s.version, records.get(&r.handle))
        {
            continue;
        }
        // A class object keeps its class by name: the number is the new file's.
        let stored =
            if was.type_code == out.type_code { s.stored(at).map(<[u8]>::to_vec) } else { edit::with_type(&was, s.version, out.type_code).ok() };
        if let Some(stored) = stored
            && let Ok(expect) = edit::parse_stored(o.version, &stored)
        {
            replace.insert(r.handle, (stored, expect));
        }
    }
    if replace.is_empty() {
        return dwg;
    }
    let stored: std::collections::BTreeMap<u64, Vec<u8>> = replace.iter().map(|(h, (b, _))| (*h, b.clone())).collect();
    let Ok(new) = edit::replace_objects(&dwg, &stored) else { return dwg };
    let Ok(n) = read_container(&new) else { return dwg };
    let out_at: std::collections::HashMap<u64, usize> = o.map.iter().map(|r| (r.handle, r.offset)).collect();
    let ok = n.map.len() == o.map.len()
        && n.objects_iter().all(|(r, obj)| {
            let Ok(obj) = obj else { return false };
            match replace.get(&r.handle) {
                Some((_, was)) => &obj == was,
                None => out_at.get(&r.handle).and_then(|at| o.object_at(*at).ok()).is_some_and(|b| b == obj),
            }
        });
    if ok { new } else { dwg }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
#[allow(clippy::unwrap_used)] // the helper below is test code too
mod tests {
    use super::*;
    use cadcraft_doc::geom::Vec3;
    use cadcraft_doc::{EntityKind, Line, Space};

    /// A DWG CADCraft wrote, with the paper space block's markers at 1B and 1D, and the drawing
    /// read from it.
    fn saved() -> (Vec<u8>, Drawing) {
        let mut d = Drawing::new_imperial();
        if let Some(l) = d.layouts.first_mut() {
            (l.begin_handle, l.end_handle) = (KeptHandle(Handle(0x1B)), KeptHandle(Handle(0x1D)));
        }
        d.add(&Space::Model, Default::default(), EntityKind::Line(Line { a: Vec3::new(0.0, 0.0, 0.0), b: Vec3::new(1.0, 0.0, 0.0) })).unwrap();
        let bytes = crate::write(&d, "x.dwg").unwrap();
        let read = crate::read(&bytes, "x.dwg").unwrap();
        (bytes, read)
    }

    /// The bridge's symptom (the paper space ENDBLK with a handle the file doesn't have) is put
    /// right from the file.
    #[test]
    fn a_renumbered_paper_space_end_marker_gets_the_files_handle() {
        let (bytes, mut d) = saved();
        let l = d.layouts.iter_mut().find(|l| l.begin_handle.get() == Handle(0x1B)).unwrap();
        l.end_handle = KeptHandle(Handle(0x3D13A));
        space_block_ends(&mut d, &bytes);
        let l = d.layouts.iter().find(|l| l.begin_handle.get() == Handle(0x1B)).unwrap();
        assert_eq!(l.end_handle.get(), Handle(0x1D));
    }

    /// Nothing changes when the drawing already has the file's markers, when its end marker is a
    /// handle the file has (another object's: not the bridge's symptom), when the layout isn't the
    /// file's paper space block, or when the bytes aren't a DWG CADCraft's reader takes apart.
    #[test]
    fn markers_that_are_right_or_not_the_symptom_are_left_alone() {
        let (bytes, d) = saved();
        let mut same = d.clone();
        space_block_ends(&mut same, &bytes);
        assert_eq!(
            same.layouts.iter().map(|l| l.end_handle.get()).collect::<Vec<_>>(),
            d.layouts.iter().map(|l| l.end_handle.get()).collect::<Vec<_>>()
        );
        let model_line = d.model.iter().next().unwrap().handle;
        for (begin, end) in [(Handle(0x1B), model_line), (Handle(0x99), Handle(0x3D13A))] {
            let mut x = d.clone();
            let l = x.layouts.iter_mut().find(|l| l.begin_handle.get() == Handle(0x1B)).unwrap();
            (l.begin_handle, l.end_handle) = (KeptHandle(begin), KeptHandle(end));
            space_block_ends(&mut x, &bytes);
            let l = x.layouts.iter().find(|l| l.begin_handle.get() == begin).unwrap();
            assert_eq!(l.end_handle.get(), end, "begin {begin:?}");
        }
        let mut x = d.clone();
        let l = x.layouts.iter_mut().find(|l| l.begin_handle.get() == Handle(0x1B)).unwrap();
        l.end_handle = KeptHandle(Handle(0x3D13A));
        space_block_ends(&mut x, b"not a DWG at all");
        assert!(x.layouts.iter().any(|l| l.end_handle.get() == Handle(0x3D13A)));
    }
}
