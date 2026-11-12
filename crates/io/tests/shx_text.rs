//! Synthetic DXF STYLE -> TEXT/MTEXT -> display list, without external font fixtures.

use cadcraft_doc::Space;

fn font() -> Vec<u8> {
    let mut bytes = b"AutoCAD-86 unifont 1.0\r\n\x1a".to_vec();
    bytes.extend_from_slice(&[4, 0, 0, 0, 8, 0]);
    bytes.extend_from_slice(&[b's', 0, 6, 2, 2, 0, 0, 0]);
    for (id, program) in
        [(32u16, vec![2, 8, 3, 0, 0]), (65, vec![8, 0, 6, 8, 4, 250, 2, 8, 3, 0, 0]), (193, vec![7, 0, 65, 5, 2, 8, 252, 7, 1, 8, 2, 1, 6, 0])]
    {
        bytes.extend_from_slice(&id.to_le_bytes());
        bytes.extend_from_slice(&(program.len() as u16 + 1).to_le_bytes());
        bytes.push(0);
        bytes.extend(program);
    }
    bytes
}

#[test]
fn shx_style_and_unicode_escapes_render_as_native_strokes_and_roundtrip() {
    cadcraft_fonts::shx::register("cadcraft-io-synthetic.shx", &font()).unwrap();
    let dxf = "0\nSECTION\n2\nTABLES\n0\nTABLE\n2\nSTYLE\n0\nSTYLE\n2\nR80\n3\ncadcraft-io-synthetic.shx\n0\nENDTAB\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nTEXT\n7\nR80\n10\n0\n20\n0\n40\n6\n1\nA\\U+00C1\n0\nMTEXT\n7\nR80\n10\n0\n20\n-20\n40\n6\n41\n18\n1\nA\\U+00C1 A\\U+00C1\n0\nENDSEC\n0\nEOF\n";
    let drawing = cadcraft_io::read(dxf.as_bytes(), "synthetic.dxf").unwrap();
    let display = cadcraft_render::build(&drawing, &Space::Model, &cadcraft_render::Options::default());
    assert!(display.tris.is_empty(), "SHX letters and accents are strokes, not filled TrueType outlines");
    assert!(display.verts.iter().any(|p| (p.x - 11.0).abs() < 1e-9), "native advance of 7, not the built-in advance");
    assert!(display.verts.iter().any(|p| p.y > 6.0), "accent above the cap height");
    assert!(display.verts.iter().any(|p| p.y < -27.0), "MTEXT wraps its second word");
    let saved = cadcraft_io::write_dxf(&drawing);
    let reopened = cadcraft_io::read(saved.as_bytes(), "synthetic.dxf").unwrap();
    let again = cadcraft_render::build(&reopened, &Space::Model, &cadcraft_render::Options::default());
    assert_eq!(display.verts, again.verts);
    assert!(again.tris.is_empty());
}
