//! Original synthetic geometry, style values and xdata, licensed with the repository.
//! The fixed fields follow Autodesk's 2010 DXF Reference, VISUALSTYLE, pp. 208-210:
//! https://images.autodesk.com/adsk/files/acad_dxf1.pdf

use cadcraft_doc::EntityKind;
use cadcraft_io::{SaveOptions, Version};

fn source(version: &str) -> String {
    let mut text = format!(
        "0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\n{version}\n9\n$HANDSEED\n5\n100\n0\nENDSEC\n\
         0\nSECTION\n2\nTABLES\n0\nTABLE\n2\nAPPID\n5\n10\n100\nAcDbSymbolTable\n70\n1\n\
         0\nAPPID\n5\n11\n330\n10\n100\nAcDbSymbolTableRecord\n100\nAcDbRegAppTableRecord\n2\nACAD\n70\n0\n0\nENDTAB\n0\nENDSEC\n\
         0\nSECTION\n2\nENTITIES\n0\nLINE\n5\n30\n100\nAcDbEntity\n8\n0\n100\nAcDbLine\n10\n0\n20\n0\n30\n0\n11\n10\n21\n5\n31\n0\n0\nENDSEC\n\
         0\nSECTION\n2\nOBJECTS\n0\nDICTIONARY\n5\nC\n330\n0\n100\nAcDbDictionary\n3\nACAD_VISUALSTYLE\n350\n20\n\
         0\nDICTIONARY\n5\n20\n330\nC\n100\nAcDbDictionary\n281\n1\n3\nSynthetic0\n350\n21\n3\nSynthetic1\n350\n22\n"
    );
    for index in 0..2 {
        text.push_str(&format!(
            "0\nVISUALSTYLE\n5\n{:X}\n330\n20\n100\nAcDbVisualStyle\n2\nSynthetic{index}\n\
             70\n0\n71\n1\n72\n1\n73\n1\n90\n0\n40\n1.0\n41\n0.5\n63\n7\n\
             74\n2\n91\n0\n64\n7\n65\n7\n75\n1\n175\n1\n42\n30.0\n92\n0\n66\n7\n43\n1.0\n\
             76\n1\n77\n0\n78\n0\n67\n7\n79\n1\n170\n0\n171\n0\n290\n0\n174\n0\n93\n0\n44\n0.0\n173\n0\n291\n0\n\
             1001\nACAD\n1000\nAcRTDxfName\n1000\nVISUALSTYLE\n",
            0x21 + index
        ));
    }
    text.push_str("0\nENDSEC\n0\nEOF\n");
    text
}

#[test]
fn legacy_visualstyles_are_reported_and_left_out_of_2007_dwg_only() {
    cadcraft_io::dwg_worker::allow_in_process();
    for from in ["AC1021", "AC1024"] {
        let drawing = cadcraft_io::read_dxf(source(from).as_bytes()).unwrap();
        let styles = || drawing.foreign.objects.iter().filter(|object| object.kind == "VISUALSTYLE").count();
        assert_eq!(styles(), 2);
        for version in [Version::R2007, Version::R2013] {
            let options = SaveOptions { version: Some(version), binary: false };
            let (bytes, report) = cadcraft_io::write_noted(&drawing, "styles.dwg", &options).unwrap();
            let back = cadcraft_io::read(&bytes, "styles.dwg").unwrap();
            assert!(back.model.iter().any(|entity| matches!(entity.kind, EntityKind::Line(_))));
            let retained = back
                .foreign
                .objects
                .iter()
                .any(|object| object.kind == "VISUALSTYLE" && object.tags.iter().any(|tag| tag.code == 2 && tag.value.starts_with("Synthetic")));
            if version == Version::R2007 {
                assert_eq!(report.get("VISUALSTYLE"), Some(&styles()));
                assert!(!retained);
            } else {
                assert!(!report.contains_key("VISUALSTYLE"));
                assert!(retained);
            }
            let (dxf, report) = cadcraft_io::write_noted(&drawing, "styles.dxf", &options).unwrap();
            assert!(report.is_empty());
            let back = cadcraft_io::read_dxf(&dxf).unwrap();
            assert_eq!(back.foreign.objects.iter().filter(|object| object.kind == "VISUALSTYLE").count(), styles());
        }
        assert_eq!(styles(), 2);
    }
}

#[test]
fn orphaned_legacy_visualstyles_are_not_reported_as_compatibility_omissions() {
    cadcraft_io::dwg_worker::allow_in_process();
    let mut drawing = cadcraft_io::read_dxf(source("AC1021").as_bytes()).unwrap();
    let foreign = std::sync::Arc::make_mut(&mut drawing.foreign);
    let orphan = foreign.objects.iter_mut().find(|object| object.kind == "VISUALSTYLE").unwrap();
    orphan.tags.iter_mut().find(|tag| tag.code == 330).unwrap().value = "FF".into();
    let options = SaveOptions { version: Some(Version::R2007), binary: false };
    let (bytes, report) = cadcraft_io::write_noted(&drawing, "styles.dwg", &options).unwrap();
    let back = cadcraft_io::read(&bytes, "styles.dwg").unwrap();
    assert!(back.model.iter().any(|entity| matches!(entity.kind, EntityKind::Line(_))));
    assert_eq!(report.get("VISUALSTYLE"), Some(&1));
}

#[test]
fn newer_visualstyle_properties_are_kept_in_2007_dwg() {
    cadcraft_io::dwg_worker::allow_in_process();
    let document = acadrust::DxfReader::from_reader(std::io::Cursor::new(source("AC1027").into_bytes())).unwrap().read().unwrap();
    let modern = acadrust::DxfWriter::new(&document).write_to_vec().unwrap();
    let drawing = cadcraft_io::read_dxf(&modern).unwrap();
    assert!(drawing.foreign.objects.iter().any(|object| object.kind == "VISUALSTYLE" && object.tags.iter().any(|tag| tag.code == 176)));
    let options = SaveOptions { version: Some(Version::R2007), binary: false };
    let (bytes, report) = cadcraft_io::write_noted(&drawing, "styles.dwg", &options).unwrap();
    assert!(!report.contains_key("VISUALSTYLE"));
    let back = cadcraft_io::read(&bytes, "styles.dwg").unwrap();
    assert!(
        back.foreign
            .objects
            .iter()
            .any(|object| { object.kind == "VISUALSTYLE" && object.tags.iter().any(|tag| tag.code == 2 && tag.value.starts_with("Synthetic")) })
    );
}
