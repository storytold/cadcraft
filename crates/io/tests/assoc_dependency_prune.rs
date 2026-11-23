//! A kept `ACDBASSOCDEPENDENCY` whose `dependentOnObject` (group 330 in `AcDbAssocDependency`)
//! names a handle that is no longer written must not be emitted with `330 = 0` as an active
//! dependency: it is dropped together with objects owned only by it, and its owning
//! `ACDBASSOCACTION` drops its `360` entry and decrements its dependency count.

fn dxf_with_assoc_action(dep1_target: &str, dep2_target: &str) -> String {
    format!(
        "0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1024\n0\nENDSEC\n\
         0\nSECTION\n2\nCLASSES\n\
         0\nCLASS\n1\nACDBASSOCNETWORK\n2\nAcDbAssocNetwork\n3\nObjectDBX Classes\n90\n1025\n280\n0\n281\n0\n\
         0\nCLASS\n1\nACDBASSOCACTION\n2\nAcDbAssocAction\n3\nObjectDBX Classes\n90\n1025\n280\n0\n281\n0\n\
         0\nCLASS\n1\nACDBASSOCDEPENDENCY\n2\nAcDbAssocDependency\n3\nObjectDBX Classes\n90\n1025\n280\n0\n281\n0\n\
         0\nENDSEC\n\
         0\nSECTION\n2\nENTITIES\n\
         0\nLINE\n5\n100\n102\n{{ACAD_REACTORS\n330\n504\n102\n}}\n330\n1F\n100\nAcDbEntity\n8\n0\n100\nAcDbLine\n\
         10\n0.0\n20\n0.0\n30\n0.0\n11\n10.0\n21\n0.0\n31\n0.0\n\
         0\nENDSEC\n\
         0\nSECTION\n2\nOBJECTS\n\
         0\nDICTIONARY\n5\nC\n330\n0\n100\nAcDbDictionary\n281\n1\n3\nACAD_ASSOCNETWORK\n350\n501\n\
         0\nDICTIONARY\n5\n501\n102\n{{ACAD_REACTORS\n330\nC\n102\n}}\n330\nC\n100\nAcDbDictionary\n281\n1\n3\nACAD_ASSOCNETWORK\n350\n502\n\
         0\nACDBASSOCNETWORK\n5\n502\n102\n{{ACAD_REACTORS\n330\n501\n102\n}}\n330\n501\n\
         100\nAcDbAssocAction\n90\n1\n90\n0\n330\n0\n360\n0\n90\n0\n90\n0\n90\n0\n\
         100\nAcDbAssocNetwork\n90\n0\n90\n1\n90\n1\n360\n503\n\
         0\nACDBASSOCACTION\n5\n503\n330\n502\n\
         100\nAcDbAssocAction\n90\n1\n90\n0\n330\n502\n360\n0\n90\n1\n90\n2\n90\n2\n360\n504\n360\n505\n\
         0\nACDBASSOCDEPENDENCY\n5\n504\n330\n503\n\
         100\nAcDbAssocDependency\n90\n2\n90\n0\n290\n1\n290\n0\n290\n1\n290\n1\n90\n0\n330\n{dep1_target}\n290\n0\n330\n0\n330\n0\n360\n0\n90\n1\n\
         0\nACDBASSOCDEPENDENCY\n5\n505\n330\n503\n\
         100\nAcDbAssocDependency\n90\n2\n90\n0\n290\n1\n290\n1\n290\n1\n290\n1\n90\n0\n330\n{dep2_target}\n290\n0\n330\n0\n330\n0\n360\n506\n90\n2\n\
         0\nXRECORD\n5\n506\n330\n505\n100\nAcDbXrecord\n280\n1\n1\nDEP_BODY_MARKER\n\
         0\nENDSEC\n0\nEOF\n"
    )
}

fn objects_of(dxf: &str, kind: &str) -> Vec<Vec<(String, String)>> {
    let lines: Vec<&str> = dxf.lines().collect();
    let mut out = Vec::new();
    let mut i = 0;
    let mut in_objects = false;
    while i + 1 < lines.len() {
        let c = lines[i].trim();
        let v = lines[i + 1].trim();
        if c == "2" && v == "OBJECTS" {
            in_objects = true;
            i += 2;
            continue;
        }
        if in_objects && c == "0" {
            if v == "ENDSEC" {
                break;
            }
            let obj_kind = v;
            let mut pairs = Vec::new();
            i += 2;
            while i + 1 < lines.len() && lines[i].trim() != "0" {
                pairs.push((lines[i].trim().to_string(), lines[i + 1].trim().to_string()));
                i += 2;
            }
            if obj_kind == kind {
                out.push(pairs);
            }
            continue;
        }
        i += 2;
    }
    out
}

#[test]
fn orphaned_assoc_dependency_is_pruned_along_with_action_entry_and_body() {
    // dep1 points at live LINE 100; dep2 points at missing handle 999 and owns XRECORD 506.
    let dxf = dxf_with_assoc_action("100", "999");
    let doc = cadcraft_io::read(dxf.as_bytes(), "assoc.dxf").expect("read DXF");
    let saved = cadcraft_io::write_dxf(&doc);

    let deps = objects_of(&saved, "ACDBASSOCDEPENDENCY");
    assert_eq!(deps.len(), 1, "only the live dependency should be written, got {deps:?}");
    let dep_handle = deps[0].iter().find(|(c, _)| c == "5").map(|(_, v)| v.as_str()).unwrap();
    let dep_target =
        deps[0].iter().skip_while(|(c, v)| !(c == "100" && v == "AcDbAssocDependency")).find(|(c, _)| c == "330").map(|(_, v)| v.as_str());
    assert_eq!(dep_target, Some("100"), "live dependency keeps its target handle");

    // The child XRECORD owned by the dropped dependency must be dropped too.
    assert!(!saved.contains("DEP_BODY_MARKER"), "child owned by dropped dependency must not be written");

    // The owning ACDBASSOCACTION must list only the surviving dependency with count = 1.
    let actions = objects_of(&saved, "ACDBASSOCACTION");
    assert_eq!(actions.len(), 1);
    let action_sub: Vec<&(String, String)> = actions[0].iter().skip_while(|(c, v)| !(c == "100" && v == "AcDbAssocAction")).collect();
    assert_eq!(action_sub[7], &("90".to_string(), "1".to_string()), "dependency count should be decremented to 1");
    assert_eq!(action_sub[8], &("360".to_string(), dep_handle.to_string()), "only the live dependency remains in 360 list");
    assert_eq!(action_sub.len(), 9, "no stale 360 entry remains");
}

#[test]
fn originally_unbound_assoc_dependency_with_null_target_is_kept() {
    // dep2 already had dependentOnObject = 0 in the input file, so it is not an unmapped handle.
    let dxf = dxf_with_assoc_action("100", "0");
    let doc = cadcraft_io::read(dxf.as_bytes(), "assoc_null.dxf").expect("read DXF");
    let saved = cadcraft_io::write_dxf(&doc);

    let deps = objects_of(&saved, "ACDBASSOCDEPENDENCY");
    assert_eq!(deps.len(), 2, "originally unbound dependency should remain kept");
    assert!(saved.contains("DEP_BODY_MARKER"));
}
