//! `set_tianzheng_thickness` 改直墙（2 段双线）墙厚：中线不动，双线沿法线外扩/内缩。
#![cfg(not(any(target_arch = "wasm32", all(target_os = "freebsd", target_arch = "aarch64"))))]

use acadrust::entities::{ProxyGraphicRecord, ProxyGraphics};
use cadcraft_doc::{Common, EntityKind, Space, Tianzheng};

/// 构造一个 52 字节的类型 6 图元 payload（u32 顶点数 2 + 6×f64）。
fn wall_seg(x1: f64, y1: f64, x2: f64, y2: f64) -> Vec<u8> {
    let mut d = Vec::with_capacity(52);
    d.extend_from_slice(&2u32.to_le_bytes());
    for v in [x1, y1, 0.0_f64, x2, y2, 0.0] {
        d.extend_from_slice(&v.to_le_bytes());
    }
    d
}

fn graphic_data(segs: &[(f64, f64, f64, f64, f64, f64)]) -> Vec<u8> {
    let records = segs
        .iter()
        .map(|(x1, y1, _, x2, y2, _)| ProxyGraphicRecord::Unknown { record_type: 6, data: wall_seg(*x1, *y1, *x2, *y2) })
        .collect();
    ProxyGraphics { records }.encode().unwrap()
}

/// 两条平行线的垂直距离（墙厚）。
fn thickness(a: (f64, f64, f64, f64, f64, f64), b: (f64, f64, f64, f64, f64, f64)) -> f64 {
    let (ax, ay) = (a.3 - a.0, a.4 - a.1);
    let len = (ax * ax + ay * ay).sqrt();
    let (nx, ny) = (-ay / len, ax / len);
    let (max, may) = ((a.0 + a.3) / 2.0, (a.1 + a.4) / 2.0);
    let (mbx, mby) = ((b.0 + b.3) / 2.0, (b.1 + b.4) / 2.0);
    ((mbx - max) * nx + (mby - may) * ny).abs()
}

fn add_wall(d: &mut cadcraft_doc::Drawing, segs: Vec<(f64, f64, f64, f64, f64, f64)>) -> cadcraft_doc::Handle {
    let gd = graphic_data(&segs);
    d.add(
        &Space::Model,
        Common::default(),
        EntityKind::Tianzheng(Tianzheng {
            class_name: "TCH_WALL".into(),
            fields: vec![],
            raw_data: None,
            handle_bits: 0,
            tags: vec![],
            dxf_type: "ACAD_PROXY_ENTITY".into(),
            segments: segs,
            graphic_data: Some(gd),
            texts: vec![],
            circles: vec![],
            polys: vec![],
            arrow_tips: vec![],
        }),
    )
    .unwrap()
}

#[test]
fn set_thickness_widens_straight_wall_keeping_centreline() {
    let mut d = cadcraft_doc::Drawing::new_metric();
    // 直墙：两条平行竖线，间距 240mm（墙厚）。
    let segs = vec![
        (137026.8, 4505.9, 0.0, 137026.8, 10135.9, 0.0),
        (136786.8, 10135.9, 0.0, 136786.8, 4505.9, 0.0),
    ];
    let h = add_wall(&mut d, segs);

    assert!(cadcraft_io::set_tianzheng_thickness(&mut d, h, 300.0));

    let t = match &d.model.get(h).unwrap().kind {
        EntityKind::Tianzheng(t) => t,
        _ => panic!("实体不是 Tianzheng"),
    };
    // 墙厚 240 → 300，中线 (136906.8, 7320.9) 不动。
    assert!((thickness(t.segments[0], t.segments[1]) - 300.0).abs() < 1e-6);
    let mid = |a: (f64, f64, f64, f64, f64, f64), b: (f64, f64, f64, f64, f64, f64)| {
        ((a.0 + a.3 + b.0 + b.3) / 4.0, (a.1 + a.4 + b.1 + b.4) / 4.0)
    };
    let (mx, my) = mid(t.segments[0], t.segments[1]);
    assert!((mx - 136906.8).abs() < 1e-6 && (my - 7320.9).abs() < 1e-6, "中线应不动，实际 ({mx},{my})");

    // graphic_data 已同步：extract_wall_segments 读回 = 新 segments。
    let back = cadcraft_dwg::tianzheng::extract_wall_segments(t.graphic_data.as_deref().unwrap());
    assert_eq!(back.len(), 2);
    assert!((thickness(back[0], back[1]) - 300.0).abs() < 1e-6);
}

#[test]
fn set_thickness_rejects_non_straight_walls() {
    let mut d = cadcraft_doc::Drawing::new_metric();
    // 3 段（拐角/开洞墙）不支持，返回 false。
    let segs = vec![
        (100.0, 200.0, 0.0, 100.0, 400.0, 0.0),
        (100.0, 400.0, 0.0, 340.0, 400.0, 0.0),
        (340.0, 400.0, 0.0, 340.0, 200.0, 0.0),
    ];
    let h = add_wall(&mut d, segs);
    assert!(!cadcraft_io::set_tianzheng_thickness(&mut d, h, 300.0));

    // 非正墙厚拒绝。
    let segs2 = vec![(100.0, 0.0, 0.0, 100.0, 100.0, 0.0), (200.0, 0.0, 0.0, 200.0, 100.0, 0.0)];
    let h2 = add_wall(&mut d, segs2);
    assert!(!cadcraft_io::set_tianzheng_thickness(&mut d, h2, 0.0));
    assert!(!cadcraft_io::set_tianzheng_thickness(&mut d, h2, -5.0));
}
