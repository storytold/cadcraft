use cadcraft_doc::{Drawing, EntityKind, HatchLoop, Region};
use cadcraft_io::{read_dxf, write_dxf};

fn boundary(points: &[(f64, f64)], flags: i64) -> String {
    let mut text = format!("92\n{flags}\n");
    if flags & 2 != 0 {
        text.push_str(&format!("72\n0\n73\n1\n93\n{}\n", points.len()));
        for (point_x, point_y) in points {
            text.push_str(&format!("10\n{point_x}\n20\n{point_y}\n"));
        }
    } else {
        text.push_str(&format!("93\n{}\n", points.len()));
        for (start, end) in points.iter().zip(points.iter().cycle().skip(1)) {
            text.push_str(&format!("72\n1\n10\n{}\n20\n{}\n11\n{}\n21\n{}\n", start.0, start.1, end.0, end.1));
        }
    }
    text.push_str("97\n0\n");
    text
}

fn logo(flags: [i64; 4]) -> String {
    let hexagon = [(-10.0, 0.0), (-5.0, -8.0), (5.0, -8.0), (10.0, 0.0), (5.0, 8.0), (-5.0, 8.0)];
    let letter = [(-3.0, -4.0), (3.0, -4.0), (3.0, -2.0), (-1.0, -2.0), (-1.0, 4.0), (-3.0, 4.0)];
    let right: Vec<_> = hexagon.iter().map(|(point_x, point_y)| (point_x + 28.0, *point_y)).collect();
    let hole: Vec<_> = hexagon.iter().map(|(point_x, point_y)| (point_x * 0.6 + 28.0, point_y * 0.6)).collect();
    let mut text = String::from("0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1015\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n");
    for (index, loops) in
        [[(&hexagon[..], flags[0]), (&letter[..], flags[1])], [(&right[..], flags[2]), (&hole[..], flags[3])]].into_iter().enumerate()
    {
        text.push_str(&format!(
            "0\nHATCH\n5\n{:X}\n100\nAcDbEntity\n8\n0\n100\nAcDbHatch\n10\n0\n20\n0\n30\n0\n210\n0\n220\n0\n230\n1\n2\nSOLID\n70\n1\n71\n0\n91\n2\n",
            0x100 + index
        ));
        for (points, flags) in loops {
            text.push_str(&boundary(points, flags));
        }
        text.push_str("75\n1\n76\n1\n98\n0\n");
    }
    text.push_str("0\nENDSEC\n0\nEOF\n");
    text
}

fn flags_written(drawing: &Drawing) -> Vec<i64> {
    let text = write_dxf(drawing);
    let lines: Vec<_> = text.lines().collect();
    let mut hatch = false;
    lines
        .as_chunks::<2>()
        .0
        .iter()
        .filter_map(|pair| {
            let code: i32 = pair[0].trim().parse().unwrap();
            if code == 0 {
                hatch = pair[1].trim() == "HATCH";
            }
            if hatch && code == 92 { Some(pair[1].trim().parse().unwrap()) } else { None }
        })
        .collect()
}

#[test]
fn boundary_flags_survive_dxf_open_and_save() {
    for flags in [[7, 22, 7, 6], [7, 22, 6, 20], [0, 9, 16, 31]] {
        let drawing = read_dxf(logo(flags).as_bytes()).unwrap();
        let expected = flags.map(|flag| flag | 2);
        assert_eq!(flags_written(&drawing), expected);
        let back = read_dxf(write_dxf(&drawing).as_bytes()).unwrap();
        assert_eq!(flags_written(&back), expected);
        for entity in back.model.iter() {
            if let EntityKind::Hatch(hatch) = &entity.kind {
                let json = serde_json::to_vec(&hatch.loops).unwrap();
                let restored: Vec<HatchLoop> = serde_json::from_slice(&json).unwrap();
                assert_eq!(restored, hatch.loops);
            }
        }
    }
}

#[test]
fn reclassified_loops_write_current_external_bit() {
    for imported in [7, 23] {
        let mut drawing = read_dxf(logo([7, imported, 7, 6]).as_bytes()).unwrap();
        let handle = drawing.model.iter().next().unwrap().handle;
        drawing.model.modify(handle, |entity| {
            let EntityKind::Hatch(hatch) = &mut entity.kind else { panic!("expected hatch") };
            assert!(hatch.loops[1].outer);
            let mut region = Region { loops: hatch.loops.clone() };
            assert!(region.renest());
            hatch.loops = region.loops;
            assert!(!hatch.loops[1].outer);
            assert_eq!(hatch.loops[1].boundary_flags, Some(imported));
        });
        let written = flags_written(&drawing)[1];
        assert_eq!(written & 1, 0);
        assert_eq!(written & (4 | 16), imported & (4 | 16));
        assert_ne!(written & 2, 0);
        let back = read_dxf(write_dxf(&drawing).as_bytes()).unwrap();
        assert_eq!(flags_written(&back), flags_written(&drawing));
    }
}

#[test]
fn undocumented_boundary_bits_are_not_written_back() {
    let flags = [i64::MAX, -1, 32 | 7, i64::MIN | 22];
    let drawing = read_dxf(logo(flags).as_bytes()).unwrap();
    let expected = flags.map(|flag| (flag & 31) | 2);
    assert_eq!(flags_written(&drawing), expected);
    let back = read_dxf(write_dxf(&drawing).as_bytes()).unwrap();
    assert_eq!(flags_written(&back), expected);
}

#[test]
#[cfg(not(any(target_arch = "wasm32", all(target_os = "freebsd", target_arch = "aarch64"))))]
fn boundary_flags_survive_dwg_open_and_both_save_formats() {
    cadcraft_io::dwg_worker::allow_in_process();
    for flags in [[7, 22, 7, 6], [7, 22, 6, 20]] {
        let drawing = read_dxf(logo(flags).as_bytes()).unwrap();
        let bytes = cadcraft_io::write(&drawing, "logo.dwg").unwrap();
        let back = cadcraft_io::read(&bytes, "logo.dwg").unwrap();
        let expected = flags.map(|flag| flag | 2);
        assert_eq!(flags_written(&back), expected);
        let again = cadcraft_io::write(&back, "logo.dwg").unwrap();
        assert_eq!(flags_written(&cadcraft_io::read(&again, "logo.dwg").unwrap()), expected);
    }
}

#[test]
fn old_serialized_loops_keep_locally_created_flags() {
    let mut drawing = read_dxf(logo([7, 22, 6, 20]).as_bytes()).unwrap();
    let mut expected = Vec::new();
    let handles: Vec<_> = drawing.model.iter().map(|entity| entity.handle).collect();
    for handle in handles {
        drawing.model.modify(handle, |entity| {
            if let EntityKind::Hatch(hatch) = &mut entity.kind {
                for boundary in &mut hatch.loops {
                    let mut json = serde_json::to_value(&*boundary).unwrap();
                    json.as_object_mut().unwrap().remove("boundary_flags");
                    *boundary = serde_json::from_value::<HatchLoop>(json).unwrap();
                    expected.push(2 | i64::from(boundary.outer));
                }
            }
        });
    }
    assert_eq!(flags_written(&drawing), expected);
}
