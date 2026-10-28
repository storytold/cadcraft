//! Reading a DWG file down to its objects, without interpreting them.

use super::bits::BitReader;
use super::classes;
use super::error::{Error, ErrorKind, Result};
use super::types::type_name;
use super::version::DwgVersion as V;
use super::{ClassDef, Diagnostic, DwgVersion, SectionInfo, Severity, objmap, r2000, r2004, section};

/// An entry of the object map: where in [`DwgContainer::objects`] an object starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ObjectRef {
    pub handle: u64,
    /// Byte offset of the object's size prefix.
    pub offset: usize,
}

/// An object as stored: its type and bytes, not interpreted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawObject {
    /// The handle the object records for itself.
    pub handle: u64,
    /// The object type code (a fixed type below 500, otherwise a class number).
    pub type_code: u16,
    /// The object's data (its size prefix and trailing checksum excluded).
    pub data: Vec<u8>,
    /// Size in bits of the handle stream at the end of `data` (R2010 and later; 0 before).
    pub handle_stream_bits: u64,
}

/// Where an entity lives, from its own record (the entity mode of §20.4.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EntitySpace {
    Model,
    Paper,
    /// In a block definition (or owned by another entity: a polyline's vertices, an insert's
    /// attributes).
    Block,
}

impl RawObject {
    /// Where this object, read as an entity, says it lives: the common entity data up to the
    /// entity mode (type, own handle, extended data, graphics, mode). Only meaningful for entities
    /// ([`type_name`]); for other objects it reads whatever bits are there.
    pub fn entity_space(&self, version: DwgVersion) -> Result<EntitySpace> {
        entity_space(&self.data, version)
    }

    /// The data every object starts with (ODA DWG specification, common entity data and common
    /// non-entity object data): its extended data and, from the handle stream, its owner, its
    /// persistent reactors and its extension dictionary. `entity` says which of the two layouts
    /// it has ([`type_name`]).
    pub fn common(&self, version: DwgVersion, entity: bool) -> Result<CommonData> {
        let mut dat = BitReader::new(&self.data);
        let (own, extended_data, handles_at) = object_head_at(&mut dat, version, self)?;
        let space = if entity { Some(entity_mode(&mut dat, version)?) } else { None };
        let reactor_count = dat.bl()?;
        // From 2004 a flag says when there is no extension dictionary (and its handle is left out).
        let xdict_missing = version >= V::R2004 && dat.b()?;
        let Some(at) = handles_at else {
            return Ok(CommonData { extended_data, owner: None, reactors: Vec::new(), reactor_count, xdict: None, space });
        };
        let mut hs = BitReader::new(&self.data);
        hs.set_position(at)?;
        // Entities with an owner handle: in a block or owned by another entity (§20.4.1, entmode 0).
        let owner = if !entity || space == Some(EntitySpace::Block) { Some(hs.handle()?.absolute(own)) } else { None };
        let mut reactors = Vec::new();
        for _ in 0..reactor_count.min(MAX_REACTORS) {
            reactors.push(hs.handle()?.absolute(own));
        }
        let xdict = if xdict_missing { Some(0) } else { Some(hs.handle()?.absolute(own)) };
        Ok(CommonData { extended_data, owner, reactors, reactor_count, xdict, space })
    }
}

/// An object's common data ([`RawObject::common`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommonData {
    /// Extended data (xdata): each application's handle as stored and its bytes.
    pub extended_data: Vec<(u64, Vec<u8>)>,
    /// The owner, for objects and for entities in a block (`None` when not read).
    pub owner: Option<u64>,
    /// The persistent reactors' handles (empty when the handle stream isn't read).
    pub reactors: Vec<u64>,
    /// How many persistent reactors the object says it has.
    pub reactor_count: u32,
    /// The extension dictionary's handle, 0 for none (`None` when not read).
    pub xdict: Option<u64>,
    /// For an entity, where it lives.
    pub space: Option<EntitySpace>,
}

/// Most reactor handles read from one object.
const MAX_REACTORS: u32 = 4096;

/// [`object_head`], with the object's own handle and the bit where its handle stream starts
/// (2000 and later; `None` before, and when it can't be placed).
fn object_head_at(dat: &mut BitReader<'_>, version: DwgVersion, o: &RawObject) -> Result<(u64, Vec<(u64, Vec<u8>)>, Option<usize>)> {
    if version >= V::R2010 {
        dat.ot()?;
    } else {
        dat.bs()?;
    }
    let size_bits = if (V::R2000..=V::R2007).contains(&version) { Some(dat.rl()?) } else { None };
    let own = dat.handle()?.value;
    let extended_data = extended_data(dat)?;
    let total = o.data.len().saturating_mul(8);
    let at = match size_bits {
        Some(bits) => usize::try_from(bits).ok(),
        None if version >= V::R2010 => usize::try_from(o.handle_stream_bits).ok().and_then(|h| total.checked_sub(h)),
        None => None,
    };
    Ok((own, extended_data, at.filter(|a| *a <= total)))
}

/// [`RawObject::entity_space`] of an object's data.
fn entity_space(data: &[u8], version: DwgVersion) -> Result<EntitySpace> {
    let mut dat = BitReader::new(data);
    object_head(&mut dat, version)?;
    entity_mode(&mut dat, version)
}

/// Most extended data blocks read from one object (a damaged file can repeat them).
const MAX_EXTENDED_DATA: usize = 4096;

/// Read an object's type, size (2000 to 2007), own handle and extended data.
fn object_head(dat: &mut BitReader<'_>, version: DwgVersion) -> Result<Vec<(u64, Vec<u8>)>> {
    if version >= V::R2010 {
        dat.ot()?;
    } else {
        dat.bs()?;
    }
    if (V::R2000..=V::R2007).contains(&version) {
        dat.rl()?;
    }
    dat.handle()?;
    extended_data(dat)
}

/// Extended data: (size, application handle, bytes) until a size of 0. Every pass reads at least
/// 18 bits, so the loop ends with the data.
pub(crate) fn extended_data(dat: &mut BitReader<'_>) -> Result<Vec<(u64, Vec<u8>)>> {
    let mut out = Vec::new();
    loop {
        let size = usize::from(dat.bs()?);
        if size == 0 {
            break;
        }
        if out.len() >= MAX_EXTENDED_DATA {
            return Err(Error::format("too many extended data blocks"));
        }
        let app = dat.handle()?;
        out.push((app.value, dat.raw_bytes(size)?));
    }
    Ok(out)
}

/// After the extended data of an entity: its graphics (skipped) and its entity mode.
pub(crate) fn entity_mode(dat: &mut BitReader<'_>, version: DwgVersion) -> Result<EntitySpace> {
    if dat.b()? {
        let size = if version >= V::R2010 { dat.bll()? } else { u64::from(dat.rl()?) };
        let bits = usize::try_from(size).ok().and_then(|s| s.checked_mul(8)).ok_or_else(|| Error::format("graphics size overflows"))?;
        dat.skip_bits(bits)?;
    }
    if version <= V::R14 {
        dat.rl()?;
    }
    Ok(match dat.bb()? {
        1 => EntitySpace::Paper,
        2 => EntitySpace::Model,
        _ => EntitySpace::Block,
    })
}

/// The begin (BLOCK) and end (ENDBLK) markers of the two space blocks, by handle.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SpaceMarkers {
    pub model_begin: Option<u64>,
    pub model_end: Option<u64>,
    pub paper_begin: Option<u64>,
    pub paper_end: Option<u64>,
}

/// A DWG file taken apart into its sections, object map and class list.
#[derive(Debug, Clone)]
pub struct DwgContainer {
    pub version: DwgVersion,
    pub maintenance: u8,
    pub codepage: u16,
    /// The header section, from its start sentinel; `None` if the file has none.
    pub header: Option<Vec<u8>>,
    /// The classes the file defines.
    pub classes: Vec<ClassDef>,
    /// The bytes the object map's offsets index: the whole file before R2004, the objects section
    /// from R2004.
    pub objects: Vec<u8>,
    /// The object map, in file order. Entries pointing outside `objects` are left out.
    pub map: Vec<ObjectRef>,
    /// The sections the file stores.
    pub sections: Vec<SectionInfo>,
    /// The other named sections of an R2004-family file (summary info, preview, ...), unpacked.
    pub others: Vec<(String, Vec<u8>)>,
    /// What did not check out, read around.
    pub notes: Vec<Diagnostic>,
}

/// The version of a DWG file from its first six bytes.
pub fn sniff_version(bytes: &[u8]) -> Option<DwgVersion> {
    DwgVersion::from_magic(bytes)
}

/// Take a DWG file apart (R13 to 2018 and later, except 2007-2009).
///
/// Whatever does not check out (a bad checksum, a missing sentinel, an object map entry that points
/// nowhere) is a note on the result, not a failure; only a file whose container can't be read at
/// all is an error.
pub fn read_container(bytes: &[u8]) -> Result<DwgContainer> {
    let version = DwgVersion::from_magic(bytes).ok_or_else(|| {
        let tag: String = bytes.iter().take(6).map(|&b| if b.is_ascii_graphic() { char::from(b) } else { '?' }).collect();
        Error::new(ErrorKind::NotDwg, format!("unrecognised version tag {tag:?}"))
    })?;
    let container = match version {
        V::R13 | V::R14 | V::R2000 => r2000::read(bytes, version)?,
        V::R2004 | V::R2010 | V::R2013 | V::R2018 => r2004::read(bytes, version)?,
        V::R2007 => {
            return Err(Error::new(
                ErrorKind::UnsupportedVersion,
                "AC1021 (AutoCAD 2007-2009) uses its own container, which is not read; save the drawing as 2004 or 2010-and-later format",
            ));
        }
    };
    let mut notes = container.notes;
    let (classes, class_notes) = match &container.classes {
        Some(data) => match classes::parse(data, version, container.maintenance, container.codepage) {
            Ok(found) => found,
            Err(e) => {
                notes.push(Diagnostic { severity: Severity::Warning, message: format!("classes: {e}") });
                (Vec::new(), Vec::new())
            }
        },
        None => {
            notes.push(Diagnostic { severity: Severity::Warning, message: "no classes section".into() });
            (Vec::new(), Vec::new())
        }
    };
    notes.extend(class_notes.into_iter().map(|message| Diagnostic { severity: Severity::Info, message }));
    let entries = choose_object_map(&container.handles, version, &container.objects, &mut notes)?;
    let before = entries.len();
    let map: Vec<ObjectRef> = entries
        .iter()
        .filter(|e| e.handle != 0)
        .filter_map(|e| {
            let offset = usize::try_from(e.location).ok()?;
            (offset < container.objects.len()).then_some(ObjectRef { handle: e.handle, offset })
        })
        .collect();
    let outside = entries.iter().filter(|e| e.handle != 0).count().saturating_sub(map.len());
    if outside > 0 {
        notes.push(Diagnostic {
            severity: Severity::Warning,
            message: format!("{outside} of {before} object map entries point outside the object data"),
        });
    }
    Ok(DwgContainer {
        version,
        maintenance: container.maintenance,
        codepage: container.codepage,
        header: container.header,
        classes,
        objects: container.objects,
        map,
        sections: container.sections,
        others: container.others,
        notes,
    })
}

impl DwgContainer {
    /// The object whose size prefix starts at `offset` in [`DwgContainer::objects`].
    pub fn object_at(&self, offset: usize) -> Result<RawObject> {
        object_at(self.version, &self.objects, offset)
    }

    /// The object stored at `offset` as the file holds it: size prefix, data and checksum (what
    /// [`super::edit::replace_objects`] takes).
    pub fn stored(&self, offset: usize) -> Option<&[u8]> {
        let len = super::edit::stored_len(self.version, &self.objects, offset).ok()?;
        self.objects.get(offset..offset.checked_add(len)?)
    }

    /// The header section's data, without its sentinels, size and checksum; `None` when the file
    /// has no header section or its start sentinel is missing.
    pub fn header_body(&self) -> Option<&[u8]> {
        let data = self.header.as_deref()?;
        section::unwrap(data, &section::HEADER_START, &section::HEADER_END, self.version, self.maintenance).ok().map(|w| w.body)
    }

    /// Every object in the map, in file order; one that can't be read is an `Err` in its place.
    pub fn objects_iter(&self) -> impl Iterator<Item = (ObjectRef, Result<RawObject>)> + '_ {
        self.map.iter().map(|r| (*r, self.object_at(r.offset)))
    }

    /// How many objects of each type the file holds, by type name ([`type_name`]), with whether the
    /// type is an entity. Objects that can't be read are counted under `?`.
    pub fn inventory(&self) -> std::collections::BTreeMap<String, (usize, bool)> {
        let mut out = std::collections::BTreeMap::new();
        for (_, o) in self.objects_iter() {
            let (name, entity) = match o {
                Ok(o) => type_name(o.type_code, &self.classes),
                Err(_) => ("?".to_string(), false),
            };
            out.entry(name).or_insert((0, entity)).0 += 1;
        }
        out
    }

    /// How many entities of each type sit in model space and in paper space, by type name. Entities
    /// in blocks aren't counted, nor ones whose record can't be read that far.
    pub fn space_counts(&self) -> std::collections::BTreeMap<(EntitySpace, String), usize> {
        let mut out = std::collections::BTreeMap::new();
        for r in &self.map {
            let Ok(p) = peek(self.version, &self.objects, r.offset) else { continue };
            let (name, entity) = type_name(p.type_code, &self.classes);
            if entity && let Ok(space @ (EntitySpace::Model | EntitySpace::Paper)) = entity_space(p.data, self.version) {
                *out.entry((space, name)).or_insert(0) += 1;
            }
        }
        out
    }

    /// The handles of the model and paper space blocks' begin and end markers (the BLOCK and ENDBLK
    /// whose records say they sit in that space).
    pub fn space_markers(&self) -> SpaceMarkers {
        let mut m = SpaceMarkers::default();
        for r in &self.map {
            let Ok(p) = peek(self.version, &self.objects, r.offset) else { continue };
            let slot = match (p.type_code, entity_space(p.data, self.version)) {
                (0x04, Ok(EntitySpace::Model)) => &mut m.model_begin,
                (0x05, Ok(EntitySpace::Model)) => &mut m.model_end,
                (0x04, Ok(EntitySpace::Paper)) => &mut m.paper_begin,
                (0x05, Ok(EntitySpace::Paper)) => &mut m.paper_end,
                _ => continue,
            };
            slot.get_or_insert(r.handle);
        }
        m
    }

    /// Whether the object map has `handle`.
    pub fn has_handle(&self, handle: u64) -> bool {
        self.map.iter().any(|r| r.handle == handle)
    }

    /// A few lines saying what the file holds, for a person.
    pub fn summary(&self) -> String {
        let mut s = format!("{}\n{} classes, {} objects\n", self.version, self.classes.len(), self.map.len());
        for x in &self.sections {
            let flags = match (x.compressed, x.encrypted) {
                (true, true) => " (compressed, encrypted)",
                (true, false) => " (compressed)",
                (false, true) => " (encrypted)",
                (false, false) => "",
            };
            s += &format!("  {}: {} bytes in {} page(s){flags}\n", x.name, x.size, x.pages);
        }
        for n in self.notes.iter().filter(|n| n.severity != Severity::Info) {
            s += &format!("  note: {}\n", n.message);
        }
        s
    }
}

/// Read the object at `offset` of an object stream: size prefix, then the type and own handle.
pub(crate) fn object_at(version: DwgVersion, data: &[u8], offset: usize) -> Result<RawObject> {
    let p = peek(version, data, offset)?;
    Ok(RawObject { handle: p.handle, type_code: p.type_code, data: p.data.to_vec(), handle_stream_bits: p.handle_stream_bits })
}

/// [`RawObject`] borrowing its bytes.
struct Peeked<'a> {
    handle: u64,
    type_code: u16,
    data: &'a [u8],
    handle_stream_bits: u64,
}

fn peek(version: DwgVersion, data: &[u8], offset: usize) -> Result<Peeked<'_>> {
    let mut r = BitReader::new(data);
    r.set_position(offset.checked_mul(8).ok_or_else(|| Error::format("object offset overflows"))?)?;
    let size = usize::try_from(r.ms()?).map_err(|_| Error::format("object size overflows"))?;
    let handle_stream_bits = if version >= V::R2010 { r.mc_unsigned()? } else { 0 };
    let start = r.position() / 8;
    let end = start.checked_add(size).ok_or_else(|| Error::format("object size overflows"))?;
    let bytes = match data.get(start..end) {
        Some(b) if size > 0 => b,
        _ => return Err(Error::truncated(format!("object at {offset} of {size} bytes runs past data"))),
    };
    let mut dat = BitReader::new(bytes);
    let type_code = if version >= V::R2010 { dat.ot()? } else { dat.bs()? };
    if (V::R2000..=V::R2007).contains(&version) {
        dat.rl()?;
    }
    let handle = dat.handle()?.value;
    Ok(Peeked { handle, type_code, data: bytes, handle_stream_bits })
}

/// The object map has two possible readings (the running handle and location either restart in
/// every block or don't); keep the one whose entries match the handles the objects record.
fn choose_object_map(handles: &[u8], version: DwgVersion, objects: &[u8], notes: &mut Vec<Diagnostic>) -> Result<Vec<objmap::MapEntry>> {
    let score = |entries: &[objmap::MapEntry]| -> (usize, usize) {
        let step = (entries.len() / 64).max(1);
        let mut hit = 0;
        let mut tried = 0;
        for e in entries.iter().step_by(step) {
            tried += 1;
            if let Ok(offset) = usize::try_from(e.location)
                && object_at(version, objects, offset).is_ok_and(|o| o.handle == e.handle)
            {
                hit += 1;
            }
        }
        (hit, tried)
    };
    match (objmap::parse(handles, true), objmap::parse(handles, false)) {
        (Ok(a), Ok(b)) => {
            let ((sa, ta), (sb, tb)) = (score(&a), score(&b));
            let (chosen, label, s, t) = if sb > sa { (b, "running", sb, tb) } else { (a, "per-block reset", sa, ta) };
            let severity = if t > 0 && s.saturating_mul(10) >= t.saturating_mul(9) { Severity::Info } else { Severity::Warning };
            notes.push(Diagnostic {
                severity,
                message: format!("object map: {} entries, {label} reading, {s}/{t} sampled handles match", chosen.len()),
            });
            Ok(chosen)
        }
        (Ok(a), Err(_)) | (Err(_), Ok(a)) => Ok(a),
        (Err(e), Err(_)) => Err(e.context("object map")),
    }
}
