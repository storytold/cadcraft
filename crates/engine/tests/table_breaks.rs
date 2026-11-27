//! Table breaking (#534): parts with repeated labels, part grips, TABLEDIT in any part; data
//! links filling a table from a CSV file and DATALINKUPDATE.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{BreakDirection, EntityKind, Handle, Table};
use cadcraft_engine::geom::Vec2;
use cadcraft_engine::render;
use serde_json::json;

fn table(s: &Session, h: &str) -> Table {
    match &s.doc().unwrap().entity(Handle::parse_hex(h).unwrap()).unwrap().kind {
        EntityKind::Table(t) => t.clone(),
        _ => panic!("not a table"),
    }
}

/// Title, header and 8 data rows, each 1 high, 2 columns of 3.
fn session() -> (Session, String) {
    let mut s = Session::new();
    let cells: Vec<Vec<String>> = (1..=8).map(|i| vec![format!("r{i}"), i.to_string()]).collect();
    let r = s
        .execute("table", &json!({ "at": [0, 0], "title": "T", "header": ["Name", "N"], "cells": cells, "rowHeight": 1.0, "colWidth": 3.0 }))
        .unwrap();
    let h = r["handle"].as_str().unwrap().to_string();
    // Same height for the title row.
    let hh = Handle::parse_hex(&h).unwrap();
    s.doc_mut()
        .unwrap()
        .modify_entity(hh, |e| {
            if let EntityKind::Table(t) = &mut e.kind {
                t.row_heights[0] = 1.0;
            }
        })
        .unwrap();
    (s, h)
}

#[test]
fn breaking_draws_parts_with_repeated_labels() {
    let (mut s, h) = session();
    let r = s.execute("table.break", &json!({ "handle": h, "height": 6.0, "spacing": 1.0 })).unwrap();
    // 2 label rows + 4 data rows per part.
    assert_eq!(r["parts"], 2);
    let t = table(&s, &h);
    let g = render::table_geometry(s.doc().unwrap(), &t);
    // The header text appears in both parts.
    assert_eq!(g.texts.iter().filter(|x| x.text == "Name").count(), 2);
    let r5 = g.texts.iter().find(|x| x.text == "r5").unwrap();
    assert_eq!(r5.row, 6);
    assert!(r5.at.x > 7.0 && r5.at.y > -3.0, "r5 starts the second part at x 7: {:?}", r5.at);
    // Extents cover both parts.
    let b = cadcraft_engine::doc::entity_bounds(s.doc().unwrap(), s.doc().unwrap().entity(Handle::parse_hex(&h).unwrap()).unwrap(), 0);
    assert!((b.max.x - 13.0).abs() < 1e-9 && (b.min.y + 6.0).abs() < 1e-9, "{b:?}");
    // TABLEDIT finds cells in the second part.
    s.execute("tabledit", &json!({ "at": [11.5, -2.5], "text": "five" })).unwrap();
    assert_eq!(table(&s, &h).cells[6][1].text, "five");
    // Down, with manual heights; Off.
    s.execute("table.break", &json!({ "handle": h, "direction": "down", "heights": [4.0] })).unwrap();
    let t = table(&s, &h);
    assert_eq!(t.breaks.as_ref().unwrap().direction, BreakDirection::Down);
    assert_eq!(t.parts().first().unwrap().rows, vec![0, 1, 2, 3]);
    let r = s.execute("table.break", &json!({ "handle": h, "enabled": false })).unwrap();
    assert_eq!(r["parts"], 1);
    assert!(s.execute("table.break", &json!({ "handle": h, "direction": "up" })).is_err());
    assert!(s.execute("table.break", &json!({ "handle": h, "height": -1 })).is_err());
}

#[test]
fn part_grips_move_parts() {
    let (mut s, h) = session();
    s.execute("table.break", &json!({ "handle": h, "height": 6.0, "spacing": 1.0 })).unwrap();
    let k = EntityKind::Table(table(&s, &h));
    let grips = k.grips();
    assert_eq!(grips, vec![Vec2::new(0.0, 0.0), Vec2::new(7.0, 0.0)]);
    // Drag the second part's grip; the first part stays.
    let moved = cadcraft_engine::grips::stretch_grip(&k, 1, Vec2::new(9.0, -2.0)).unwrap();
    let EntityKind::Table(t) = &moved else { panic!() };
    let parts = t.parts();
    assert_eq!((parts[0].origin, parts[1].origin), (Vec2::new(0.0, 0.0), Vec2::new(9.0, -2.0)));
    // The insertion grip moves everything.
    let all = cadcraft_engine::grips::stretch_grip(&moved, 0, Vec2::new(1.0, 1.0)).unwrap();
    let EntityKind::Table(t) = &all else { panic!() };
    assert_eq!(t.parts()[1].origin, Vec2::new(10.0, -1.0));
    // Typed: select the table by a point, then the height.
    s.execute("table.break", &json!({ "handle": h, "enabled": false })).unwrap();
    s.cmdline("table.break").unwrap();
    s.cmdline("1,-0.5").unwrap();
    s.cmdline("4").unwrap();
    assert_eq!(table(&s, &h).parts().len(), 4);
}

#[test]
fn data_links_fill_and_update() {
    let dir = std::env::temp_dir().join(format!("cadcraft-datalink-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("parts.csv");
    std::fs::write(&path, "Part,Qty,Note\r\nbolt,4,\"M6, zinc\"\nnut,8,\n").unwrap();
    let mut s = Session::new();
    let r = s.execute("table", &json!({ "at": [0, 0], "title": "Parts", "cols": 2, "rows": 1 })).unwrap();
    let h = r["handle"].as_str().unwrap().to_string();
    let r = s.execute("datalink", &json!({ "handle": h, "path": path.to_str().unwrap() })).unwrap();
    assert_eq!((r["rows"].as_u64(), r["cols"].as_u64()), (Some(4), Some(3)));
    let t = table(&s, &h);
    assert_eq!(t.cells[0][0].merged, Some((1, 3)));
    assert_eq!(t.cells[1][0].text, "Part");
    assert_eq!(t.cells[2][2].text, "M6, zinc");
    // An update re-reads the file and resizes the table to it.
    std::fs::write(&path, "Part,Qty\nwasher,12\n").unwrap();
    let r = s.execute("datalinkupdate", &json!({})).unwrap();
    assert_eq!(r["updated"], 1);
    let t = table(&s, &h);
    assert_eq!((t.cells.len(), t.col_widths.len()), (3, 2));
    assert_eq!(t.cells[2][1].text, "12");
    // Typed DATALINKUPDATE with a selection.
    s.cmdline("datalinkupdate").unwrap();
    s.cmdline("1,-0.2").unwrap();
    s.cmdline("").unwrap();
    // Missing files and missing paths are errors, not panics.
    assert!(s.execute("datalink", &json!({ "handle": h })).is_err());
    assert!(s.execute("datalink", &json!({ "handle": h, "path": dir.join("missing.csv").to_str().unwrap() })).is_err());
    std::fs::remove_file(&path).unwrap();
    let r = s.execute("datalinkupdate", &json!({})).unwrap();
    assert_eq!(r["updated"], 0);
    s.execute("datalink", &json!({ "handle": h, "remove": true })).unwrap();
    assert!(table(&s, &h).data_link.is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn data_links_saved_in_a_drawing_are_read_beside_it_or_on_this_computer_only() {
    cadcraft_engine::cmd::file::set_io(cadcraft_engine::cmd::file::IoHooks {
        read: |b, name| cadcraft_io::read(b, name).map_err(|e| e.to_string()),
        write: |d, name| cadcraft_io::write(d, name).map_err(|e| e.to_string()),
        plot: None,
    });
    let base = std::env::temp_dir().join(format!("cadcraft-datalink-saved-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let (dir, elsewhere) = (base.join("plans"), base.join("elsewhere"));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::create_dir_all(&elsewhere).unwrap();
    std::fs::write(dir.join("parts.csv"), "Part,Qty\nbolt,4\n").unwrap();
    std::fs::write(elsewhere.join("far.csv"), "Part,Qty\nnut,8\n").unwrap();

    let mut links = vec![r"\\192.0.2.1\share\far.csv".to_string(), "file://192.0.2.1/s/far.csv".to_string(), "parts.csv".to_string()];
    if cfg!(unix) {
        // Read as a local path by Linux, and as a network path by the resolver.
        links.push(format!("/{}", elsewhere.join("far.csv").display()));
    }
    let mut s = Session::new();
    for (i, link) in links.iter().enumerate() {
        let r = s.execute("table", &json!({ "at": [0, 10.0 * i as f64], "cols": 2, "rows": 1 })).unwrap();
        let h = Handle::parse_hex(r["handle"].as_str().unwrap()).unwrap();
        s.doc_mut()
            .unwrap()
            .modify_entity(h, |e| {
                if let EntityKind::Table(t) = &mut e.kind {
                    t.data_link = Some(cadcraft_engine::doc::DataLink { path: link.clone(), start_row: 0 });
                }
            })
            .unwrap();
    }
    let host = dir.join("host.dxf");
    s.execute("saveas", &json!({ "path": host.to_str().unwrap() })).unwrap();
    let mut s = Session::new();
    s.execute("open", &json!({ "path": host.to_str().unwrap() })).unwrap();

    let r = s.execute("datalinkupdate", &json!({})).unwrap();
    assert_eq!(r["updated"], 1, "only the file beside the drawing: {r}");
    let errors: Vec<&str> = r["errors"].as_array().unwrap().iter().filter_map(|e| e.as_str()).collect();
    assert_eq!(errors.len(), links.len() - 1, "{errors:?}");
    assert!(errors.iter().all(|e| e.contains("another computer")), "{errors:?}");
    let _ = std::fs::remove_dir_all(&base);
}

/// Read and write drawings through `cadcraft-io`, as the app does.
fn use_file_io() {
    cadcraft_engine::cmd::file::set_io(cadcraft_engine::cmd::file::IoHooks {
        read: |b, name| cadcraft_io::read(b, name).map_err(|e| e.to_string()),
        write: |d, name| cadcraft_io::write(d, name).map_err(|e| e.to_string()),
        plot: None,
    });
}

/// A new table, `i` rows of tables down, whose data link names `path` as a drawing file stores it.
fn linked_table(s: &mut Session, i: usize, path: &str) {
    let r = s.execute("table", &json!({ "at": [0, 10.0 * i as f64], "cols": 2, "rows": 1 })).unwrap();
    let h = Handle::parse_hex(r["handle"].as_str().unwrap()).unwrap();
    s.doc_mut()
        .unwrap()
        .modify_entity(h, |e| {
            if let EntityKind::Table(t) = &mut e.kind {
                t.data_link = Some(cadcraft_engine::doc::DataLink { path: path.to_string(), start_row: 0 });
            }
        })
        .unwrap();
}

/// The text of every cell of every table in model space.
fn table_texts(s: &Session) -> String {
    let mut out = String::new();
    for e in s.doc().unwrap().model.iter() {
        if let EntityKind::Table(t) = &e.kind {
            for cell in t.cells.iter().flatten() {
                out.push_str(&cell.text);
                out.push('\n');
            }
        }
    }
    out
}

/// A drawing's own data link can't copy a file from outside its folder into it: not by `..`, not
/// by an absolute path, not through a folder whose name only starts the same, not through a link
/// inside the folder that leads out. Links within the folder, also by `..` back into it, still
/// update.
#[test]
fn a_saved_data_link_reads_no_file_outside_the_drawings_folder() {
    use_file_io();
    let base = std::env::temp_dir().join(format!("cadcraft-datalink-outside-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let (dir, private, sibling) = (base.join("plans"), base.join("private"), base.join("plans-old"));
    for d in [dir.join("sub"), private.clone(), sibling.clone()] {
        std::fs::create_dir_all(d).unwrap();
    }
    let secret = "not-for-the-drawing";
    std::fs::write(private.join("secret.csv"), format!("{secret},1\n")).unwrap();
    std::fs::write(sibling.join("secret.csv"), format!("{secret},2\n")).unwrap();
    std::fs::write(dir.join("parts.csv"), "bolt,4\n").unwrap();
    std::fs::write(dir.join("sub").join("more.csv"), "nut,8\n").unwrap();

    let path = |p: std::path::PathBuf| p.to_str().unwrap().to_string();
    let inside = ["parts.csv".to_string(), "sub/more.csv".to_string(), "../plans/parts.csv".to_string(), path(dir.join("parts.csv"))];
    let mut outside = vec!["../private/secret.csv".to_string(), path(private.join("secret.csv")), path(sibling.join("secret.csv"))];
    #[cfg(unix)]
    let linked = std::os::unix::fs::symlink(private.join("secret.csv"), dir.join("link.csv")).is_ok();
    // Windows lets only some accounts make symbolic links.
    #[cfg(windows)]
    let linked = std::os::windows::fs::symlink_file(private.join("secret.csv"), dir.join("link.csv")).is_ok();
    if linked {
        outside.push("link.csv".to_string());
    } else {
        eprintln!("symbolic links can't be made here: the link case is not checked");
    }

    let mut s = Session::new();
    for (i, link) in inside.iter().chain(&outside).enumerate() {
        linked_table(&mut s, i, link);
    }
    let host = dir.join("host.dxf");
    s.execute("saveas", &json!({ "path": path(host.clone()) })).unwrap();
    let mut s = Session::new();
    s.execute("open", &json!({ "path": path(host) })).unwrap();

    let r = s.execute("datalinkupdate", &json!({})).unwrap();
    assert_eq!(r["updated"], inside.len(), "only the files in the drawing's folder: {r}");
    let errors: Vec<&str> = r["errors"].as_array().unwrap().iter().filter_map(|e| e.as_str()).collect();
    assert_eq!(errors.len(), outside.len(), "{errors:?}");
    assert!(errors.iter().all(|e| e.contains("outside the drawing's folder")), "{errors:?}");
    let texts = table_texts(&s);
    assert!(texts.contains("bolt") && texts.contains("nut"), "the files inside are read: {texts}");
    assert!(!texts.contains(secret), "{texts}");
    // So the next save carries nothing of them either.
    let after = dir.join("after.dxf");
    s.execute("saveas", &json!({ "path": path(after.clone()) })).unwrap();
    assert!(!std::fs::read_to_string(&after).unwrap().contains(secret));
    let _ = std::fs::remove_dir_all(&base);
}

/// A file outside the drawing's folder that the user links with DATALINK keeps updating while
/// the drawing stays open; once it is opened again, the drawing alone can't read it until the
/// user links it again.
#[test]
fn a_file_the_user_links_outside_the_folder_updates_until_the_drawing_is_opened_again() {
    use_file_io();
    let base = std::env::temp_dir().join(format!("cadcraft-datalink-chosen-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let (dir, data) = (base.join("plans"), base.join("data"));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::create_dir_all(&data).unwrap();
    let bom = data.join("bom.csv");
    let bom_path = bom.to_str().unwrap();
    let host = dir.join("host.dxf");
    let host_path = host.to_str().unwrap();

    let mut s = Session::new();
    let r = s.execute("table", &json!({ "at": [0, 0], "cols": 2, "rows": 1 })).unwrap();
    let h = r["handle"].as_str().unwrap().to_string();
    s.execute("saveas", &json!({ "path": host_path })).unwrap();
    std::fs::write(&bom, "bolt,rev-a\n").unwrap();
    s.execute("datalink", &json!({ "handle": h, "path": bom_path })).unwrap();
    std::fs::write(&bom, "bolt,rev-b\n").unwrap();
    let r = s.execute("datalinkupdate", &json!({})).unwrap();
    assert_eq!(r["updated"], 1, "{r}");
    assert!(table_texts(&s).contains("rev-b"));
    s.execute("saveas", &json!({ "path": host_path })).unwrap();

    let mut s = Session::new();
    s.execute("open", &json!({ "path": host_path })).unwrap();
    std::fs::write(&bom, "bolt,rev-c\n").unwrap();
    let r = s.execute("datalinkupdate", &json!({})).unwrap();
    assert_eq!(r["updated"], 0, "{r}");
    assert!(r["errors"][0].as_str().unwrap().contains("outside the drawing's folder"), "{r}");
    assert!(!table_texts(&s).contains("rev-c"));
    // Linked again by the user, it reads and keeps updating.
    s.execute("datalink", &json!({ "handle": h, "path": bom_path })).unwrap();
    assert!(table_texts(&s).contains("rev-c"));
    std::fs::write(&bom, "bolt,rev-d\n").unwrap();
    let r = s.execute("datalinkupdate", &json!({})).unwrap();
    assert_eq!(r["updated"], 1, "{r}");
    assert!(table_texts(&s).contains("rev-d"));
    let _ = std::fs::remove_dir_all(&base);
}
