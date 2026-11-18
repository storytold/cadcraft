//! Export timing on a realistic large drawing: `examples/apartment.dxf` repeated on a grid
//! (a floor plan of many apartments).
//!
//! `cargo run --release -p cadcraft-io --example export_bench -- [FORMAT] [GRID] [RUNS] [OUT_DIR]`
//!
//! FORMAT is `png`, `pdf`, `svg`, `dxf` or `all` (default `png`), GRID the apartments per side
//! (default 10), RUNS the timed runs per format (default 7). Prints the median and minimum wall
//! time of each format's export (from the drawing in memory to the file's bytes). With OUT_DIR,
//! the last run's output is written there as `bench.FORMAT` to compare builds.

use std::time::{Duration, Instant};

use cadcraft_doc::{Drawing, Space};
use cadcraft_geom::{Mat3, Vec2};

fn drawing(grid: usize) -> Drawing {
    let src = include_bytes!("../../../examples/apartment.dxf");
    let Ok(mut d) = cadcraft_io::read(src, "apartment.dxf") else {
        eprintln!("apartment.dxf did not read");
        std::process::exit(1);
    };
    let ents: Vec<_> = d.space(&Space::Model).map(|s| s.iter().map(|e| (**e).clone()).collect()).unwrap_or_default();
    let ext = d.extents(&Space::Model);
    let step = Vec2::new(ext.width() * 1.1, ext.height() * 1.1);
    for i in 0..grid {
        for j in 0..grid {
            if (i, j) == (0, 0) {
                continue;
            }
            let m = Mat3::translate(Vec2::new(step.x * j as f64, step.y * i as f64));
            for e in &ents {
                let mut e = e.clone();
                e.handle = d.new_handle();
                e.kind.transform(&m);
                if let Some(s) = d.space_mut(&Space::Model) {
                    s.push(e);
                }
            }
        }
    }
    d
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let format = args.first().map(String::as_str).unwrap_or("png").to_string();
    let grid: usize = args.get(1).and_then(|a| a.parse().ok()).unwrap_or(10);
    let runs: usize = args.get(2).and_then(|a| a.parse().ok()).unwrap_or(7).max(1);
    let out_dir = args.get(3);
    let d = drawing(grid);
    eprintln!("{} entities", d.entity_count());
    let formats: Vec<&str> = if format == "all" { vec!["dxf", "png", "pdf", "svg"] } else { vec![format.as_str()] };
    for f in formats {
        let name = format!("bench.{f}");
        let mut times: Vec<Duration> = Vec::new();
        let mut last = Vec::new();
        for _ in 0..runs {
            let t = Instant::now();
            match cadcraft_io::write(&d, &name) {
                Ok(bytes) => {
                    times.push(t.elapsed());
                    last = bytes;
                }
                Err(e) => {
                    eprintln!("{f}: {e}");
                    std::process::exit(1);
                }
            }
        }
        times.sort();
        let ms = |t: Duration| t.as_secs_f64() * 1000.0;
        println!("{f}: median {:.1} ms, min {:.1} ms, {} runs, {} bytes", ms(times[times.len() / 2]), ms(times[0]), runs, last.len());
        if let Some(dir) = out_dir
            && let Err(e) = std::fs::write(std::path::Path::new(dir).join(&name), &last)
        {
            eprintln!("{name}: {e}");
        }
    }
}
