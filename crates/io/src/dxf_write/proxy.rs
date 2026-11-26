//! Kept proxy records (ACAD_PROXY_OBJECT, ACAD_PROXY_ENTITY) in the group layout of the DXF version
//! written (#3229). Proxies are kept with the groups they were read with, and the DWG bridge
//! brings them in the DWG's own version's layout, so a 2010 drawing saved as an R2000 DXF wrote
//! 2010 groups that R2000 readers reject ("Out of range").
//!
//! The layouts, after the `AcDbProxyObject` / `AcDbProxyEntity` marker:
//! - R2000 and R2004 (DXF Reference): 92 graphics size, 93 data size in bits, 95 the drawing
//!   format (low word the DWG version, high word the maintenance release), 70 the data format.
//! - 2007: as R2000, with 96 for the size of the separate string stream. A proxy whose string
//!   stream isn't empty has no earlier form: the raw writer leaves it out of an earlier file and
//!   the save says so.
//! - 2010 and 2013: 160 graphics size, 161 data size, 162 string stream size.
//! - 2018 and later: as 2010, with the drawing format split into 71 (DWG version) and 97
//!   (maintenance release).

use std::collections::{BTreeMap, HashSet};

use cadcraft_doc::foreign::is_proxy;
use cadcraft_doc::{Drawing, EntityKind, Handle};
use craft_dxf::{Tag, Version};

/// What a DXF of `version` leaves out of `d` because a file earlier than 2007 has no form for a
/// proxy's separate string stream, counted by DXF type: those proxies, and the objects that go
/// with them (their extension dictionary and its records) because nothing written owns them any
/// more. Found by writing the drawing as 2007 and as `version` and comparing what each keeps, so a proxy that no version writes (its class isn't written, or no written block holds
/// it) is not counted, and nothing that stays in the file is. Empty from 2007 on (#3229).
pub fn proxies_lost(d: &Drawing, version: Version) -> BTreeMap<String, usize> {
    // Writing twice only for a drawing that holds such a proxy at all.
    if version >= Version::R2007 || cadcraft_doc::foreign::proxies_dropped_before_2007(d).is_empty() {
        return BTreeMap::new();
    }
    // Unknown entities keep their handles in the file; kept foreign objects get new ones, so
    // the writer names those by their handles in the drawing.
    let (tags_2007, objects_2007) = super::tags_kept(d, Version::R2007);
    let (tags_here, objects_here) = super::tags_kept(d, version);
    let (entities_2007, entities_here) = (written_handles(&tags_2007), written_handles(&tags_here));
    let lost = |h: Handle| entities_2007.contains(&h) && !entities_here.contains(&h);
    let lost_object = |h: Handle| objects_2007.contains(&h) && !objects_here.contains(&h);
    let mut out = BTreeMap::new();
    let mut count = |kind: &str| {
        let n: &mut usize = out.entry(kind.trim().to_ascii_uppercase()).or_default();
        *n = n.saturating_add(1);
    };
    let stores = std::iter::once(&d.model).chain(d.layouts.iter().map(|l| &l.entities)).chain(d.blocks.values().map(|b| &b.entities));
    for e in stores.flat_map(|s| s.iter()) {
        if let EntityKind::Unknown(u) = &e.kind
            && lost(e.handle)
        {
            count(&u.dxf_type);
        }
    }
    for o in d.foreign.objects.iter().filter(|o| lost_object(o.handle)) {
        count(&o.kind);
    }
    out
}

/// The handles of the records in written `tags` (group 5, and 105 for a DIMSTYLE).
fn written_handles(tags: &[Tag]) -> HashSet<Handle> {
    tags.iter().filter(|t| t.code == 5 || t.code == 105).filter_map(|t| Handle::parse_hex(t.str().trim())).collect()
}

/// What a proxy group holds, whatever version's code it was read with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Field {
    /// Size of the graphics data in bytes (92, 160).
    Graphics,
    /// Size of the object or entity data in bits (93, 161).
    Data,
    /// Size of the separate string stream (96, 162), from the 2007 format.
    Strings,
}

fn field(code: i32) -> Option<Field> {
    match code {
        92 | 160 => Some(Field::Graphics),
        93 | 161 => Some(Field::Data),
        96 | 162 => Some(Field::Strings),
        _ => None,
    }
}

/// The code `version` writes `f` with; `None` when it has none (a string stream before 2007).
fn code_for(f: Field, version: Version) -> Option<i32> {
    let late = version >= Version::R2010;
    match f {
        Field::Graphics => Some(if late { 160 } else { 92 }),
        Field::Data => Some(if late { 161 } else { 93 }),
        Field::Strings if late => Some(162),
        Field::Strings => (version >= Version::R2007).then_some(96),
    }
}

/// Rewrite every proxy record in `tags` into `version`'s layout. Other records are untouched.
pub(super) fn proxy_layout(tags: &mut Vec<Tag>, version: Version) {
    if !tags.iter().any(|t| t.code == 0 && is_proxy(&t.str())) {
        return;
    }
    let src = std::mem::take(tags);
    let mut out = Vec::with_capacity(src.len());
    let mut rest = src.as_slice();
    while let Some((first, tail)) = rest.split_first() {
        // One record: its type, and its groups up to the next record.
        let end = tail.iter().position(|t| t.code == 0).unwrap_or(tail.len());
        let (body, next) = tail.split_at(end);
        out.push(first.clone());
        if first.code == 0 && is_proxy(&first.str()) {
            convert(body, version, &mut out);
        } else {
            out.extend_from_slice(body);
        }
        rest = next;
    }
    *tags = out;
}

/// One proxy record's groups (after its type) in `version`'s layout. Only the groups after the
/// proxy subclass marker change: before it, an entity's common groups have a 92 and a 160 of
/// their own.
fn convert(body: &[Tag], version: Version, out: &mut Vec<Tag>) {
    let marker = body.iter().position(|t| t.code == 100 && t.str().trim().to_ascii_lowercase().starts_with("acdbproxy"));
    let Some(marker) = marker else {
        out.extend_from_slice(body);
        return;
    };
    let (common, own) = body.split_at(marker.saturating_add(1));
    out.extend_from_slice(common);
    let format = drawing_format(own);
    let mut format_written = false;
    for t in own {
        if matches!(t.code, 95 | 71 | 97) {
            // The drawing format goes where the first of its groups was, in this version's form.
            if !format_written && let Some(f) = format {
                if version >= Version::R2018 {
                    out.push(Tag::i(71, f & 0xFFFF));
                    out.push(Tag::i(97, (f >> 16) & 0xFFFF));
                } else {
                    out.push(Tag::i(95, f));
                }
            }
            format_written = true;
            continue;
        }
        match field(t.code) {
            Some(f) => {
                if let Some(code) = code_for(f, version) {
                    out.push(Tag { code, value: t.value.clone() });
                }
            }
            None => out.push(t.clone()),
        }
    }
}

/// The proxy's drawing format (DWG version in the low word, maintenance release in the high
/// word), from a 95 or from 71 and 97.
fn drawing_format(own: &[Tag]) -> Option<i64> {
    let get = |code| own.iter().find(|t| t.code == code).map(|t| t.value.as_i64());
    get(95).map(|f| f & 0xFFFF_FFFF).or_else(|| get(71).map(|lo| ((get(97).unwrap_or(0) & 0xFFFF) << 16) | (lo & 0xFFFF)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(groups: &[(i32, &str)]) -> Vec<Tag> {
        groups.iter().map(|(c, v)| Tag::s(*c, *v)).collect()
    }

    fn codes(tags: &[Tag]) -> Vec<(i32, String)> {
        tags.iter().map(|t| (t.code, t.str())).collect()
    }

    /// A proxy object as a 2010 DWG brings it (the record in the report).
    fn from_2010() -> Vec<Tag> {
        record(&[
            (0, "ACAD_PROXY_OBJECT"),
            (5, "2CBA9C"),
            (100, "AcDbProxyObject"),
            (90, "499"),
            (91, "521"),
            (95, "393245"),
            (70, "0"),
            (162, "0"),
            (161, "303"),
            (310, "00FF"),
            (340, "1F"),
            (94, "0"),
            (0, "ENDSEC"),
        ])
    }

    fn own(tags: &[Tag]) -> Vec<(i32, String)> {
        codes(tags).into_iter().skip(3).take_while(|(c, _)| *c != 0).collect()
    }

    fn s(c: i32, v: &str) -> (i32, String) {
        (c, v.to_string())
    }

    #[test]
    fn a_2010_proxy_takes_each_versions_layout() {
        // 393245 = 0x6001D: DWG version 29, maintenance release 6.
        let mut t = from_2010();
        proxy_layout(&mut t, Version::R2000);
        assert_eq!(own(&t), [s(90, "499"), s(91, "521"), s(95, "393245"), s(70, "0"), s(93, "303"), s(310, "00FF"), s(340, "1F"), s(94, "0")]);
        let mut t = from_2010();
        proxy_layout(&mut t, Version::R2007);
        assert_eq!(
            own(&t),
            [s(90, "499"), s(91, "521"), s(95, "393245"), s(70, "0"), s(96, "0"), s(93, "303"), s(310, "00FF"), s(340, "1F"), s(94, "0")]
        );
        let mut t = from_2010();
        proxy_layout(&mut t, Version::R2010);
        assert_eq!(codes(&t), codes(&from_2010()), "already 2010");
        let mut t = from_2010();
        proxy_layout(&mut t, Version::R2018);
        assert_eq!(
            own(&t),
            [s(90, "499"), s(91, "521"), s(71, "29"), s(97, "6"), s(70, "0"), s(162, "0"), s(161, "303"), s(310, "00FF"), s(340, "1F"), s(94, "0")]
        );
        // And back from the 2018 form.
        proxy_layout(&mut t, Version::R2004);
        assert_eq!(own(&t), [s(90, "499"), s(91, "521"), s(95, "393245"), s(70, "0"), s(93, "303"), s(310, "00FF"), s(340, "1F"), s(94, "0")]);
        // The ENDSEC after it is untouched.
        assert_eq!(t.last().map(Tag::str).as_deref(), Some("ENDSEC"));
    }

    #[test]
    fn an_entitys_common_groups_stay() {
        // Before the subclass marker, 92 and 160 are the entity's own proxy graphics.
        let mut t = record(&[
            (0, "ACAD_PROXY_ENTITY"),
            (100, "AcDbEntity"),
            (92, "4"),
            (310, "AA"),
            (100, "AcDbProxyEntity"),
            (90, "498"),
            (91, "500"),
            (160, "8"),
            (310, "BB"),
            (161, "16"),
            (310, "CC"),
            (94, "0"),
            (95, "393245"),
            (70, "0"),
        ]);
        proxy_layout(&mut t, Version::R2000);
        let c: Vec<i32> = t.iter().map(|x| x.code).collect();
        assert_eq!(c, [0, 100, 92, 310, 100, 90, 91, 92, 310, 93, 310, 94, 95, 70]);
    }

    #[test]
    fn other_records_and_hostile_values_are_safe() {
        let mut t = record(&[(0, "LINE"), (95, "1"), (100, "AcDbProxyObject"), (93, "1")]);
        let before = codes(&t);
        proxy_layout(&mut t, Version::R2018);
        assert_eq!(codes(&t), before, "not a proxy");
        // A huge or negative drawing format doesn't overflow.
        let mut t = record(&[(0, "ACAD_PROXY_OBJECT"), (100, "AcDbProxyObject"), (95, "-1"), (71, "99999999999")]);
        proxy_layout(&mut t, Version::R2018);
        assert_eq!(codes(&t), [s(0, "ACAD_PROXY_OBJECT"), s(100, "AcDbProxyObject"), s(71, "65535"), s(97, "65535")]);
        // No marker: left as it is.
        let mut t = record(&[(0, "ACAD_PROXY_OBJECT"), (93, "1")]);
        proxy_layout(&mut t, Version::R2018);
        assert_eq!(codes(&t), [s(0, "ACAD_PROXY_OBJECT"), s(93, "1")]);
    }
}
