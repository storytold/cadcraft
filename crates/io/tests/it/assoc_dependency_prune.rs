//! A kept `ACDBASSOCDEPENDENCY` whose `dependentOnObject` (group 330 in `AcDbAssocDependency`)
//! names a handle that is no longer written must not be emitted with `330 = 0` as an active
//! dependency: it is dropped together with the objects whose owner it is, and its owning
//! `ACDBASSOCACTION` drops its list entry (group 330 or 360) and decrements its dependency count.

/// The parts of the fixture that the tests change.
struct Parts<'a> {
    dep1_target: &'a str,
    dep2_target: &'a str,
    /// Groups added to the action's `AcDbAssocAction` header after its status.
    action_extra: &'a str,
    /// The action's dependency list after its count (group 330 or 360 per entry).
    action_list: &'a str,
    /// The body handle (group 360) of dependency 504.
    dep1_body: &'a str,
    /// The owner (group 330) of XRECORD 506, which dependency 505 hard-owns.
    body_owner: &'a str,
    /// Objects added at the end of the OBJECTS section.
    extra_objects: &'a str,
}

const PARTS: Parts<'static> = Parts {
    dep1_target: "100",
    dep2_target: "999",
    action_extra: "",
    action_list: "360\n504\n360\n505\n",
    dep1_body: "0",
    body_owner: "505",
    extra_objects: "",
};

fn dxf_with_assoc_action(dep1_target: &str, dep2_target: &str) -> String {
    dxf(&Parts { dep1_target, dep2_target, ..PARTS })
}

fn dxf(p: &Parts) -> String {
    let Parts { dep1_target, dep2_target, action_extra, action_list, dep1_body, body_owner, extra_objects } = p;
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
         100\nAcDbAssocAction\n90\n1\n90\n0\n{action_extra}330\n502\n360\n0\n90\n1\n90\n2\n90\n2\n{action_list}\
         0\nACDBASSOCDEPENDENCY\n5\n504\n330\n503\n\
         100\nAcDbAssocDependency\n90\n2\n90\n0\n290\n1\n290\n0\n290\n1\n290\n1\n90\n0\n330\n{dep1_target}\n290\n0\n330\n0\n330\n0\n360\n{dep1_body}\n90\n1\n\
         0\nACDBASSOCDEPENDENCY\n5\n505\n330\n503\n\
         100\nAcDbAssocDependency\n90\n2\n90\n0\n290\n1\n290\n1\n290\n1\n290\n1\n90\n0\n330\n{dep2_target}\n290\n0\n330\n0\n330\n0\n360\n506\n90\n2\n\
         0\nXRECORD\n5\n506\n330\n{body_owner}\n100\nAcDbXrecord\n280\n1\n1\nDEP_BODY_MARKER\n\
         {extra_objects}0\nENDSEC\n0\nEOF\n"
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

/// The groups of the written action from its `100 AcDbAssocAction` marker on.
fn action_subclass(saved: &str) -> Vec<(String, String)> {
    let actions = objects_of(saved, "ACDBASSOCACTION");
    assert_eq!(actions.len(), 1, "one action is written");
    actions[0].iter().skip_while(|(c, v)| !(c == "100" && v == "AcDbAssocAction")).cloned().collect()
}

/// The dependency count at `at` in an action subclass and the handle groups that follow it.
fn dependency_list(sub: &[(String, String)], at: usize) -> (usize, Vec<(String, String)>) {
    assert_eq!(sub[at].0, "90", "the dependency count is a group 90");
    let count: usize = sub[at].1.parse().expect("count");
    let entries = sub[at + 1..].iter().take_while(|(c, _)| c == "330" || c == "360").cloned().collect();
    (count, entries)
}

fn handle_of(object: &[(String, String)]) -> String {
    object.iter().find(|(c, _)| c == "5").map(|(_, v)| v.clone()).expect("handle")
}

#[test]
fn non_owned_dependency_entry_keeps_its_code_when_its_sibling_is_pruned() {
    // acadrust writes a non-owned dependency as a group 330 entry. Dependency 505 is dangling.
    let dxf = dxf(&Parts { action_list: "330\n504\n360\n505\n", ..PARTS });
    let doc = cadcraft_io::read(dxf.as_bytes(), "assoc_mixed.dxf").expect("read DXF");
    let saved = cadcraft_io::write_dxf(&doc);

    let deps = objects_of(&saved, "ACDBASSOCDEPENDENCY");
    assert_eq!(deps.len(), 1, "the dangling dependency is dropped, got {deps:?}");
    let (count, entries) = dependency_list(&action_subclass(&saved), 7);
    assert!(count < 2, "the count drops below the two read entries");
    assert_eq!(count, entries.len(), "the count matches the written list: {entries:?}");
    assert_eq!(entries, vec![("330".to_string(), handle_of(&deps[0]))], "the surviving entry keeps group 330");
}

#[test]
fn null_owner_body_goes_with_its_pruned_hard_owner() {
    // XRECORD 506 has owner 0. Only the dangling dependency 505 holds it (group 360).
    let dxf = dxf(&Parts { body_owner: "0", ..PARTS });
    let doc = cadcraft_io::read(dxf.as_bytes(), "assoc_null_owner.dxf").expect("read DXF");
    let saved = cadcraft_io::write_dxf(&doc);
    assert_eq!(objects_of(&saved, "ACDBASSOCDEPENDENCY").len(), 1, "the dangling dependency is dropped");
    assert!(!saved.contains("DEP_BODY_MARKER"), "a body held only by the dropped dependency is dropped");
}

#[test]
fn null_owner_body_stays_while_another_live_object_holds_it() {
    // Live dependency 504 also holds XRECORD 506 by group 360.
    let dxf = dxf(&Parts { body_owner: "0", dep1_body: "506", ..PARTS });
    let doc = cadcraft_io::read(dxf.as_bytes(), "assoc_shared_body.dxf").expect("read DXF");
    let saved = cadcraft_io::write_dxf(&doc);
    assert_eq!(objects_of(&saved, "ACDBASSOCDEPENDENCY").len(), 1, "the dangling dependency is dropped");
    assert!(saved.contains("DEP_BODY_MARKER"), "a body with another live holder stays");
}

#[test]
fn dependency_of_an_action_with_an_unknown_header_is_not_pruned() {
    // An extra group in the action header hides where its list starts, so the list stays as read.
    let dxf = dxf(&Parts { action_extra: "290\n1\n", ..PARTS });
    let doc = cadcraft_io::read(dxf.as_bytes(), "assoc_unknown_action.dxf").expect("read DXF");
    let saved = cadcraft_io::write_dxf(&doc);

    let deps = objects_of(&saved, "ACDBASSOCDEPENDENCY");
    assert_eq!(deps.len(), 2, "both dependencies stay, got {deps:?}");
    let (count, entries) = dependency_list(&action_subclass(&saved), 8);
    assert_eq!(count, entries.len(), "the count matches the written list: {entries:?}");
    let written: Vec<String> = deps.iter().map(|d| handle_of(d)).collect();
    assert!(entries.iter().all(|(_, h)| written.contains(h)), "every entry names a written dependency: {entries:?}");
}

#[test]
fn dependency_listed_by_an_action_with_an_unknown_header_is_not_pruned() {
    // Known action 503 owns dangling dependency 505. Action 507 has an unknown header and lists 505 by group 330.
    let unknown = "0\nACDBASSOCACTION\n5\n507\n330\n502\n\
                   100\nAcDbAssocAction\n90\n1\n90\n0\n290\n1\n330\n502\n360\n0\n90\n2\n90\n0\n90\n1\n330\n505\n";
    let dxf = dxf(&Parts { extra_objects: unknown, ..PARTS });
    let doc = cadcraft_io::read(dxf.as_bytes(), "assoc_unknown_lister.dxf").expect("read DXF");
    let saved = cadcraft_io::write_dxf(&doc);

    let deps = objects_of(&saved, "ACDBASSOCDEPENDENCY");
    assert_eq!(deps.len(), 2, "both dependencies stay, got {deps:?}");
    let written: Vec<String> = deps.iter().map(|d| handle_of(d)).collect();
    let actions = objects_of(&saved, "ACDBASSOCACTION");
    let lister = actions.iter().find(|a| a.iter().any(|(c, _)| c == "290")).expect("the unknown action is written");
    let sub: Vec<(String, String)> = lister.iter().skip_while(|(c, v)| !(c == "100" && v == "AcDbAssocAction")).cloned().collect();
    let (count, entries) = dependency_list(&sub, 8);
    assert_eq!(count, entries.len(), "the count matches the written list: {entries:?}");
    assert!(entries.iter().all(|(_, h)| written.contains(h)), "every entry names a written dependency: {entries:?}");
}
