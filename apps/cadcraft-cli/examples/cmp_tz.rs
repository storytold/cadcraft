//! 临时诊断：验证真正格式墙几何从 text 流大数解码成 segments。
use std::fs;
use std::path::PathBuf;

fn main() {
    let mut args = std::env::args();
    args.next();
    let real = PathBuf::from(args.next().expect("真正格式路径"));
    let bytes = fs::read(&real).unwrap();
    let d = cadcraft_io::read(&bytes, &real).unwrap();

    let mut n = 0usize;
    if let Some(store) = d.space(&cadcraft_doc::Space::Model) {
        for e in store.iter() {
            let cadcraft_doc::EntityKind::Tianzheng(t) = &e.kind else { continue };
            if t.class_name != "TCH_WALL" || n >= 3 {
                continue;
            }
            n += 1;
            println!("WALL 段数 {}:", t.segments.len());
            for s in t.segments.iter().take(3) {
                println!("  ({:.2},{:.2}) -> ({:.2},{:.2})", s.0, s.1, s.3, s.4);
            }
        }
    }
}
