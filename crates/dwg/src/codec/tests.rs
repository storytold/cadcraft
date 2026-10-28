//! The container layer against synthetic files (always) and the MI-201 corpus (when `MI201_CORPUS`
//! names its folder; the corpus is not part of the repository).

use std::panic::{AssertUnwindSafe, catch_unwind};

use super::bits::BitWriter;
use super::pack::{FileInfo, MAX_PAGE, Section, object_map, pack};
use super::section::{HEADER_END, HEADER_START};
use super::{DwgVersion, ErrorKind, read_container};

const INFO: FileInfo = FileInfo { version: *b"AC1032", maintenance: 0, app_version: 33, app_maintenance: 0, codepage: 30, acad_maintenance: 0 };

/// An AC1032 object as an objects section stores it: size, handle-stream size, data, checksum.
/// The data is the type, the object's own handle and some padding.
fn object(type_code: u16, handle: u64, padding: usize) -> Vec<u8> {
    let mut data = BitWriter::new();
    data.ot(type_code);
    data.handle(0, handle);
    for i in 0..padding {
        data.rc(i as u8);
    }
    let data = data.into_bytes();
    let mut w = BitWriter::new();
    w.ms(data.len() as u32);
    w.mc_unsigned(0);
    let mut out = w.into_bytes();
    out.extend_from_slice(&data);
    out.extend_from_slice(&[0, 0]);
    out
}

/// A header section: start sentinel, size (and, for 2018, a high size), body, checksum, end sentinel.
fn header(body: &[u8]) -> Vec<u8> {
    let mut out = HEADER_START.to_vec();
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(body);
    out.extend_from_slice(&[0, 0]);
    out.extend_from_slice(&HEADER_END);
    out
}

/// A small 2018 file: a header section and objects with handles 1 to 40 (types 0x13 and 0x1F2+).
fn synthetic() -> (Vec<u8>, Vec<(u64, u16)>, Vec<u8>) {
    synthetic_with(None)
}

/// [`synthetic`] with a chosen page compressor (`None`: [`pack`]'s own).
fn synthetic_with(compress: Option<super::pack::Compress>) -> (Vec<u8>, Vec<(u64, u16)>, Vec<u8>) {
    let mut objects = 0x0DCAu32.to_le_bytes().to_vec();
    let mut map = Vec::new();
    let mut expected = Vec::new();
    for h in 1..=40u64 {
        let type_code = if h % 3 == 0 { 0x1F2 + (h as u16 % 5) } else { 0x13 };
        map.push((h, objects.len() as u64));
        objects.extend_from_slice(&object(type_code, h, (h as usize * 7) % 23));
        expected.push((h, type_code));
    }
    let header_body: Vec<u8> = (0..300u32).map(|i| (i * 31 % 251) as u8).collect();
    let sections = vec![
        Section { name: "AcDb:Header".into(), data: header(&header_body), compressed: true, page_size: MAX_PAGE },
        Section { name: "AcDb:AcDbObjects".into(), data: objects, compressed: true, page_size: MAX_PAGE },
        Section { name: "AcDb:Handles".into(), data: object_map(&map).unwrap(), compressed: true, page_size: MAX_PAGE },
    ];
    let order = ["AcDb:AcDbObjects", "AcDb:Handles", "AcDb:Header"];
    let file = match compress {
        None => pack(&INFO, &sections, &order),
        Some(c) => super::pack::pack_with(&INFO, &sections, &order, c),
    }
    .unwrap();
    (file, expected, header_body)
}

#[test]
fn a_packed_file_reads_back_whole() {
    let (file, expected, header_body) = synthetic();
    let c = read_container(&file).unwrap();
    assert_eq!(c.version, DwgVersion::R2018);
    assert_eq!(c.codepage, 30);
    let names: Vec<&str> = c.sections.iter().map(|s| s.name.as_str()).collect();
    for name in ["AcDb:Header", "AcDb:AcDbObjects", "AcDb:Handles"] {
        assert!(names.contains(&name), "{names:?}");
    }
    assert_eq!(c.header_body(), Some(&header_body[..]));
    let read: Vec<(u64, u16)> = c.objects_iter().map(|(r, o)| (r.handle, o.unwrap().type_code)).collect();
    assert_eq!(read, expected);
    assert!(c.objects_iter().all(|(r, o)| o.is_ok_and(|o| o.handle == r.handle)));
    // Checksums and sentinels all check out: nothing above an information note but the missing
    // classes section (this file defines no classes).
    let worse: Vec<_> = c.notes.iter().filter(|n| n.severity != super::Severity::Info && n.message != "no classes section").collect();
    assert!(worse.is_empty(), "{worse:?}");
    assert!(c.summary().contains("40 objects"), "{}", c.summary());
}

/// The pages are compressed as the specification allows in its simplest form (one literal run
/// each) as well: the reader takes both.
#[test]
fn literal_only_pages_read_back_too() {
    let (file, expected, header_body) = synthetic_with(Some(super::pack::literal_only));
    let c = read_container(&file).unwrap();
    assert_eq!(c.header_body(), Some(&header_body[..]));
    assert_eq!(c.objects_iter().map(|(r, o)| (r.handle, o.unwrap().type_code)).collect::<Vec<_>>(), expected);
}

/// Each data page header (§4.6, XORed with its address) records the page's decompressed size at
/// offset 12, which readers take as the most the page may decompress to.
#[test]
fn data_page_headers_record_the_decompressed_page_size() {
    let (file, _, _) = synthetic();
    // The first data page (the header section's) follows the 0x100-byte file header.
    let word = |i: usize| {
        let at = 0x100 + i * 4;
        u32::from_le_bytes(file[at..at + 4].try_into().unwrap()) ^ 0x4164_536B ^ 0x100
    };
    assert_eq!(word(0), 0x4163_043B);
    assert_eq!(word(3), MAX_PAGE as u32);
    assert!(word(2) < word(3), "the stored (compressed) size is smaller");
}

#[test]
fn object_maps_need_ascending_handles_and_split_into_blocks() {
    assert!(object_map(&[(5, 10), (4, 20)]).is_err());
    assert!(object_map(&[(5, 10), (5, 20)]).is_err());
    // Enough entries for several blocks, read back by the map reader.
    let entries: Vec<(u64, u64)> = (1..3000u64).map(|h| (h * 3, h * 1000)).collect();
    let bytes = object_map(&entries).unwrap();
    let read = super::objmap::parse(&bytes, true).unwrap();
    assert_eq!(read.len(), entries.len());
    assert!(read.iter().zip(&entries).all(|(r, e)| r.handle == e.0 && r.location == e.1 as i64));
}

#[test]
fn files_that_are_not_read_say_why() {
    assert_eq!(read_container(b"hello").unwrap_err().kind, ErrorKind::NotDwg);
    let mut file = synthetic().0;
    file[..6].copy_from_slice(b"AC1021");
    assert_eq!(read_container(&file).unwrap_err().kind, ErrorKind::UnsupportedVersion);
    assert_eq!(read_container(b"AC1032").unwrap_err().kind, ErrorKind::Truncated);
}

/// Read `file` as far as anything goes: the container, every object and the header.
fn read_everything(file: &[u8]) {
    if let Ok(c) = read_container(file) {
        for (_, o) in c.objects_iter() {
            let _ = o;
        }
        let _ = c.header_body();
        let _ = c.summary();
    }
}

/// Every damaged variant of `file` (each byte set to a few values, every length cut short) reads
/// to a result, never a panic. Run in a debug build, so an arithmetic overflow is a panic too.
fn hostile(file: &[u8], stride: usize) {
    let mut failures = Vec::new();
    let mut check = |what: String, bytes: &[u8]| {
        if catch_unwind(AssertUnwindSafe(|| read_everything(bytes))).is_err() {
            failures.push(what);
        }
    };
    let mut copy = file.to_vec();
    for at in (0..file.len()).step_by(stride) {
        let was = copy[at];
        for v in [0x00, 0xFF, 0x80, was ^ 0x01, was.wrapping_add(0x40)] {
            copy[at] = v;
            check(format!("byte {at:#x} = {v:#04x}"), &copy);
        }
        copy[at] = was;
    }
    for len in (0..file.len()).step_by(stride.max(1)) {
        check(format!("cut to {len} bytes"), &file[..len]);
    }
    assert!(failures.is_empty(), "{} damaged variants panicked, first: {:?}", failures.len(), failures.iter().take(5).collect::<Vec<_>>());
}

#[test]
fn hostile_input_never_panics() {
    hostile(&synthetic().0, 1);
}

/// The MI-201 corpus folder, when `MI201_CORPUS` names it.
fn corpus() -> Option<std::path::PathBuf> {
    let dir = std::path::PathBuf::from(std::env::var_os("MI201_CORPUS")?);
    dir.is_dir().then_some(dir)
}

/// Every DWG of the corpus that the layer reads comes apart into sections, classes and objects,
/// and every object in the map records the handle the map gives it.
#[test]
fn the_corpus_comes_apart() {
    let Some(dir) = corpus() else { return };
    let mut read = 0;
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("dwg") {
            continue;
        }
        let bytes = std::fs::read(&path).unwrap();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        match read_container(&bytes) {
            Err(e) if e.kind == ErrorKind::UnsupportedVersion => assert!(name.contains("2007"), "{name}: {e}"),
            Err(e) => panic!("{name}: {e}"),
            Ok(c) => {
                read += 1;
                assert!(c.map.len() > 100, "{name}: {} objects", c.map.len());
                assert!(!c.classes.is_empty(), "{name}: no classes");
                let bad: Vec<_> = c.objects_iter().filter(|(r, o)| !o.as_ref().is_ok_and(|o| o.handle == r.handle)).map(|(r, _)| r).collect();
                assert!(bad.is_empty(), "{name}: {} of {} objects don't match their map entry", bad.len(), c.map.len());
                assert!(c.header_body().is_some_and(|b| !b.is_empty()), "{name}: header");
            }
        }
    }
    assert!(read >= 6, "read {read} corpus files");
}

/// The corpus's 2018 file, damaged byte by byte (every 61st byte, so it stays quick), never panics.
#[test]
fn the_corpus_damaged_never_panics() {
    let Some(dir) = corpus() else { return };
    let Ok(bytes) = std::fs::read(dir.join("mi201-r2018.dwg")) else { return };
    hostile(&bytes, 61);
}
