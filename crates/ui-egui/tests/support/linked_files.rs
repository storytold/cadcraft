//! Original synthetic native-DXF/resource fixture, shared by native and actual Window tests.
use cadcraft_engine::doc::{Block, Drawing, Entity, EntityKind, Image, Layout, XrefStatus};
use cadcraft_engine::{
    Session,
    cmd::file,
    linked_files::{Bytes, Edge, File, Input},
};
use cadcraft_ui_egui::{CadApp, Services};
use std::sync::Arc;

pub fn install_formats() {
    file::set_io(file::IoHooks {
        read: |b, n| cadcraft_io::read(b, n).map_err(|e| e.to_string()),
        write: |d, n| cadcraft_io::write(d, n).map_err(|e| e.to_string()),
        plot: None,
    });
    file::set_write_as(|d, n, o| {
        cadcraft_io::write_with(d, n, &cadcraft_io::SaveOptions { version: o.version, binary: o.binary }).map_err(|e| e.to_string())
    });
}
fn image(d: &mut Drawing, path: &str, place: u8) {
    let entity = Entity::new(d.new_handle(), EntityKind::Image(Image { path: path.into(), ..Image::default() }));
    match place {
        0 => {
            d.model.push(entity);
        }
        1 => {
            let mut layout = Layout::new("Sheet", 1);
            layout.entities.push(entity);
            d.layouts.push(layout);
        }
        _ => {
            let mut block = Block::new("Picture");
            block.entities.push(entity);
            d.blocks.insert(block.name.clone(), Arc::new(block));
        }
    }
}
fn xref(d: &mut Drawing, name: &str, path: &str, overlay: bool, unloaded: bool) {
    let mut block = Block::new(name);
    block.xref_path = Some(path.into());
    block.xref_overlay = overlay;
    if unloaded {
        block.xref_status = XrefStatus::Unloaded;
    }
    d.blocks.insert(name.into(), Arc::new(block));
}
fn bytes(d: &Drawing) -> Result<Arc<Bytes>, String> {
    Bytes::new(cadcraft_io::write(d, "Fixture.dxf").map_err(|e| e.to_string())?)
}
fn png(color: u8) -> Result<Arc<Bytes>, String> {
    let img = image::RgbaImage::from_pixel(1, 1, image::Rgba([color, 64, 128, 255]));
    let mut output = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(img).write_to(&mut output, image::ImageFormat::Png).map_err(|e| e.to_string())?;
    Bytes::new(output.into_inner())
}
/// Includes model/layout/block images, nested/overlay/unloaded xrefs and a cycle to main.
pub fn input(color: u8) -> Result<Input, String> {
    install_formats();
    let mut main = Drawing::new_imperial();
    for place in 0..3 {
        image(&mut main, "../images/main.png", place);
    }
    xref(&mut main, "Child", "../parts/child.dxf", false, false);
    xref(&mut main, "Overlay", "../parts/overlay.dxf", true, true);
    if let Some(style) = main.text_styles.first_mut() {
        style.font = "CADCraft Stroke".into();
        style.big_font = "../fonts/original.shx".into();
    }
    let mut child = Drawing::new_imperial();
    xref(&mut child, "Nested", "nested.dxf", false, false);
    image(&mut child, "../images/detail.png", 0);
    let mut nested = Drawing::new_imperial();
    xref(&mut nested, "Cycle", "../plans/main.dxf", false, false);
    let root = bytes(&main)?;
    let files = vec![
        File { key: "parts/child.dxf".into(), kind: "dxf".into(), bytes: bytes(&child)? },
        File { key: "parts/nested.dxf".into(), kind: "dxf".into(), bytes: bytes(&nested)? },
        File { key: "parts/overlay.dxf".into(), kind: "dxf".into(), bytes: bytes(&Drawing::new_imperial())? },
        File { key: "images/main.png".into(), kind: "raster".into(), bytes: png(color)? },
        File { key: "images/detail.png".into(), kind: "raster".into(), bytes: png(255 - color)? },
        File {
            key: "fonts/original.shx".into(),
            kind: "font".into(),
            bytes: Bytes::new(b"original licensed fixture font bytes, not a renderer".to_vec())?,
        },
    ];
    let mut edges = Vec::new();
    for (from, raw, role, to) in [
        ("plans/main.dxf", "../images/main.png", "image", "images/main.png"),
        ("plans/main.dxf", "../parts/child.dxf", "xref", "parts/child.dxf"),
        ("plans/main.dxf", "../parts/overlay.dxf", "xref", "parts/overlay.dxf"),
        ("plans/main.dxf", "../fonts/original.shx", "bigFont", "fonts/original.shx"),
        ("parts/child.dxf", "nested.dxf", "xref", "parts/nested.dxf"),
        ("parts/child.dxf", "../images/detail.png", "image", "images/detail.png"),
        ("parts/nested.dxf", "../plans/main.dxf", "xref", "plans/main.dxf"),
    ] {
        edges.push(Edge { from_key: from.into(), raw_reference: raw.into(), role: role.into(), target_key: to.into() });
    }
    Ok(Input {
        main_key: "plans/main.dxf".into(),
        name: "Main.dxf".into(),
        main_hash: root.hash().into(),
        main: Some(root),
        files,
        edges,
        version: "version-1".into(),
    })
}
pub fn app() -> CadApp {
    install_formats();
    CadApp::new(Session::empty(), Services { no_file_system: true, ..Services::default() })
}
