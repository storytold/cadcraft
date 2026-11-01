//! A kept 2D constraint group (`ACDBASSOC2DCONSTRAINTGROUP`, the geometric constraints of a
//! parametric block) keeps its nodes through the DWG bridge and through DXF of another version.
//! AutoCAD 2013 and later write a node's status after its connections, earlier versions right
//! after its id; the group was written as read, so the bridge's R2000 DXF gave acadrust every
//! node's status and connections from the wrong groups, and AutoCAD refused the DWG of a drawing
//! with parametric blocks ("Error Decrypting Data").

use std::io::Cursor;

use acadrust::objects::{AssociativeData, ObjectType};
use cadcraft_io::{SaveOptions, Version, write_with};

/// A drawing of `acadver` whose named object dictionary holds a constraint group with two
/// constraints: node 1 (status 1) connected to node 2, node 2 (status 2) connected to node 1.
/// `modern` writes the nodes in the 2013+ order.
fn dxf(acadver: &str, modern: bool) -> String {
    let node = |id: u32, status: u32, other: u32| {
        let (head, tail) = if modern { (String::new(), format!("70\n{status}\n")) } else { (format!("70\n{status}\n"), String::new()) };
        format!("90\n{id}\n{head}90\n1\n90\n{other}\n{tail}90\n0\n290\n0\n290\n1\n")
    };
    format!(
        "0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\n{acadver}\n0\nENDSEC\n\
0\nSECTION\n2\nCLASSES\n0\nCLASS\n1\nACDBASSOC2DCONSTRAINTGROUP\n2\nAcDbAssoc2dConstraintGroup\n3\nObjectDBX Classes\n90\n1025\n280\n0\n281\n0\n0\nENDSEC\n\
0\nSECTION\n2\nOBJECTS\n\
0\nDICTIONARY\n5\nC\n330\n0\n100\nAcDbDictionary\n281\n1\n3\nTESTCG\n350\n20\n\
0\nACDBASSOC2DCONSTRAINTGROUP\n5\n20\n330\nC\n100\nAcDbAssocAction\n90\n2\n90\n0\n330\n0\n360\n0\n90\n2\n90\n45\n90\n0\n90\n0\n90\n0\n90\n0\n90\n0\n\
100\nAcDbAssoc2dConstraintGroup\n90\n2\n70\n0\n10\n0.0\n20\n0.0\n30\n0.0\n10\n1.0\n20\n0.0\n30\n0.0\n10\n0.0\n20\n1.0\n30\n0.0\n360\n0\n90\n0\n\
90\n3\n90\n0\n90\n2\n90\n1\n90\n2\n290\n0\n90\n2\n1\nAcPerpendicularConstraint\n90\n1\n1\nAcPointCoincidenceConstraint\n90\n2\n{}{}\
0\nENDSEC\n0\nEOF\n",
        node(1, 1, 2),
        node(2, 2, 1)
    )
}

/// The (id, status, connections) of every non-root node of the document's constraint group.
fn nodes(doc: &acadrust::CadDocument) -> Vec<(i32, u8, Vec<i32>)> {
    let group = doc.objects.values().find_map(|o| match o {
        ObjectType::Associative(a) => match &a.data {
            AssociativeData::ConstraintGroup(g) => Some(g.clone()),
            _ => None,
        },
        _ => None,
    });
    let group = group.expect("the constraint group is written");
    group.nodes.iter().skip(1).map(|n| (n.node_id, n.status, n.connections.clone())).collect()
}

fn expected() -> Vec<(i32, u8, Vec<i32>)> {
    vec![(1, 1, vec![2]), (2, 2, vec![1])]
}

fn save(src: &str, name: &str, version: Version) -> Vec<u8> {
    let d = cadcraft_io::read(src.as_bytes(), "a.dxf").expect("read");
    write_with(&d, name, &SaveOptions { version: Some(version), binary: false }).expect("write")
}

#[test]
fn constraint_group_nodes_survive_the_dwg_bridge() {
    for (acadver, modern) in [("AC1032", true), ("AC1024", false)] {
        for version in [Version::R2018, Version::R2010, Version::R2004] {
            let dwg = save(&dxf(acadver, modern), "a.dwg", version);
            let doc = acadrust::DwgReader::from_stream(Cursor::new(dwg)).read().expect("acadrust reads the DWG");
            assert_eq!(nodes(&doc), expected(), "{acadver} drawing saved as {version:?} DWG");
        }
    }
}

#[test]
fn constraint_group_nodes_follow_the_dxf_version() {
    for (acadver, modern) in [("AC1032", true), ("AC1024", false)] {
        for version in [Version::R2018, Version::R2013, Version::R2010, Version::R2000] {
            let out = save(&dxf(acadver, modern), "a.dxf", version);
            let doc = acadrust::DxfReader::from_reader(Cursor::new(out)).expect("dxf").read().expect("acadrust reads the DXF");
            assert_eq!(nodes(&doc), expected(), "{acadver} drawing saved as {version:?} DXF");
        }
    }
}
