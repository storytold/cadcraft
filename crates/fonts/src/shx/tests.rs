//! SHX tests. Every font here is compiled from shape source written for these tests: no font
//! file is read from disk or committed.

use std::f64::consts::PI;

use cadcraft_geom::Vec2;

use super::*;
use crate::{TextFont, TextParams};

/// A small dual-orientation text font: 6 units above, 2 below.
pub(crate) const TEXT_FONT: &str = "\
;; CADCraft test font (written for the tests)
*0,4,cadcraft test
6,2,2,0
*32,3,SPACE
2,060,0
; A: up 6, right 4, down 6, then a gap of 2
*65,7,UCA
1,064,040,06C,2,020,0
; B: a displacement out and back, then 5 on
*66,10,UCB
2,010,1,8,(3,4),8,(-3,-4),2,050,0
; C: vertical-only moves (code 14)
*67,12,UCC
2,14,8,(0,-8),1,064,2,14,8,(0,-6),040,0
; the degree sign: a full octant circle
*256,12,DEGREE
2,8,(1,4),1,10,(1,0),2,8,(2,-4),0
*257,6,PLUSMINUS
1,040,2,020,0
*258,6,DIAMETER
1,044,2,020,0
";

/// A Unicode font with the same letters, a subshape and the diameter sign at U+2205.
const UNI_FONT: &str = "\
*UNIFONT,6,cadcraft uni
6,2,0,0,0,0
*00041,7,uca
1,064,040,06C,2,020,0
*000C5,9,ring a
7,00041,2,8,(-4,7),1,040,2,8,(2,-7),0
*02205,6,diameter
1,044,2,020,0
";

/// A big font with Shift-JIS lead bytes: one square character (U+3042, 0x82A0).
const BIG_FONT: &str = "\
*BIGFONT 2,1,081,09F
*0,4,cadcraft big
8,0,0,0
*082A0,7,square
1,080,084,088,08C,2,080,0
";

/// An extended big font: two primitives of a 16 x 16 cell drawn into one character.
const EXT_BIG_FONT: &str = "\
*BIGFONT 2,1,081,09F
*0,5,cadcraft ext
16,0,0,16,0
*08F01,10,box
4,2,1,080,084,088,08C,3,2,0
*082A2,20,two boxes
7,0,08F01,0,0,8,16,7,0,08F01,8,0,8,16,2,4,2,080,3,2,0
";

fn font(src: &str) -> ShxFont {
    ShxFont::parse(&compile(src).unwrap()).unwrap()
}

/// A shape file (no font record) with one shape from `spec`.
fn one(spec: &str) -> ShxFont {
    font(&format!("*1,1,T\n{spec}\n"))
}

fn adv(spec: &str) -> Vec2 {
    one(spec).glyph(1, false).unwrap().advance
}

fn close(a: Vec2, b: Vec2) -> bool {
    (a - b).len() < 1e-6
}

fn dir(deg: f64) -> Vec2 {
    let r = deg.to_radians();
    Vec2::new(r.cos(), r.sin())
}

#[test]
fn every_vector_direction() {
    let want = [
        (1.0, 0.0),
        (1.0, 0.5),
        (1.0, 1.0),
        (0.5, 1.0),
        (0.0, 1.0),
        (-0.5, 1.0),
        (-1.0, 1.0),
        (-1.0, 0.5),
        (-1.0, 0.0),
        (-1.0, -0.5),
        (-1.0, -1.0),
        (-0.5, -1.0),
        (0.0, -1.0),
        (0.5, -1.0),
        (1.0, -1.0),
        (1.0, -0.5),
    ];
    for (d, (x, y)) in want.iter().enumerate() {
        let a = adv(&format!("02{d:X},0"));
        assert!(close(a, Vec2::new(2.0 * x, 2.0 * y)), "direction {d}: {a:?}");
    }
    // Length 15, the longest vector.
    assert!(close(adv("0F0,0"), Vec2::new(15.0, 0.0)));
}

#[test]
fn pen_up_down_scale_push_pop() {
    let f = one("2,010,1,020,2,010,1,010,0");
    let g = f.glyph(1, false).unwrap();
    assert_eq!(g.strokes, vec![vec![Vec2::new(1.0, 0.0), Vec2::new(3.0, 0.0)], vec![Vec2::new(4.0, 0.0), Vec2::new(5.0, 0.0)]]);
    // 4 multiplies, 3 divides: 3 + 1.
    assert!(close(adv("4,3,010,3,3,010,0"), Vec2::new(4.0, 0.0)));
    // Push, draw, pop: the next run starts where the push was.
    let g = one("5,040,6,014,0").glyph(1, false).unwrap();
    assert_eq!(g.strokes.len(), 2);
    assert!(close(g.strokes[1][0], Vec2::ZERO) && close(g.advance, Vec2::new(0.0, 1.0)));
    // An unmatched pop is ignored.
    assert!(close(adv("6,010,0"), Vec2::new(1.0, 0.0)));
}

#[test]
fn displacements() {
    assert!(close(adv("8,(-3,5),0"), Vec2::new(-3.0, 5.0)));
    assert!(close(adv("9,(1,1),(2,-1),(-127,0),(0,0),0"), Vec2::new(-124.0, 0.0)));
}

#[test]
fn octant_and_fractional_arcs() {
    // A full circle comes back to where it started.
    let g = one("10,(2,0),0").glyph(1, false).unwrap();
    assert!(close(g.advance, Vec2::ZERO));
    let maxx = g.strokes.iter().flatten().map(|p| p.x).fold(f64::MIN, f64::max);
    assert!((maxx - 0.0).abs() < 1e-9 && g.strokes.iter().flatten().any(|p| (p.x + 4.0).abs() < 1e-6), "circle left of the pen");
    // Clockwise from octant 4 (180°), three octants: ends at 45°.
    assert!(close(adv("10,(1,-043),0"), dir(45.0) - dir(180.0)));
    // Counter-clockwise quarter from 0°.
    assert!(close(adv("10,(3,002),0"), (dir(90.0) - dir(0.0)) * 3.0));
    // The documented fractional example: 55° to 95°, radius 3 (start octant 1, two octants).
    let a0 = 45.0 + 56.0 / 256.0 * 45.0;
    let a1 = 90.0 + 28.0 / 256.0 * 45.0;
    assert!(close(adv("11,(56,28,0,3,012),0"), (dir(a1) - dir(a0)) * 3.0));
    // High radius byte: 256 + 4.
    assert!(close(adv("11,(0,0,1,4,001),0"), (dir(45.0) - dir(0.0)) * 260.0));
}

#[test]
fn bulge_arcs() {
    // A counter-clockwise semicircle up the chord bulges to the right... of the chord's left
    // normal: from (0,0) to (0,4) about (0,2), through (2,2).
    let g = one("12,(0,4,127),0").glyph(1, false).unwrap();
    assert!(close(g.advance, Vec2::new(0.0, 4.0)));
    assert!(g.strokes.iter().flatten().any(|p| close(*p, Vec2::new(2.0, 2.0))), "{:?}", g.strokes);
    // Bulge 0 is a straight line.
    assert_eq!(one("12,(3,0,0),0").glyph(1, false).unwrap().strokes, vec![vec![Vec2::ZERO, Vec2::new(3.0, 0.0)]]);
    // Polyarc: two arcs, then the (0,0) end without a bulge.
    let g = one("13,(4,0,127),(0,4,-127),(0,0),010,0").glyph(1, false).unwrap();
    assert!(close(g.advance, Vec2::new(5.0, 4.0)));
    assert!(g.strokes.iter().flatten().any(|p| (p.y + 2.0).abs() < 1e-6), "first arc dips below");
}

#[test]
fn vertical_only_commands_and_text_font_record() {
    let f = font(TEXT_FONT);
    assert_eq!((f.kind(), f.name(), f.above(), f.below(), f.is_dual(), f.is_font()), (ShxKind::Shapes, "cadcraft test", 6.0, 2.0, true, true));
    let h = f.glyph(67, false).unwrap();
    assert_eq!(h.strokes, vec![vec![Vec2::ZERO, Vec2::new(0.0, 6.0)]]);
    let v = f.glyph(67, true).unwrap();
    assert_eq!(v.strokes, vec![vec![Vec2::new(0.0, -8.0), Vec2::new(0.0, -2.0)]]);
    assert!(close(v.advance, Vec2::new(4.0, -8.0)));
    // A font that isn't dual runs code 14's command in horizontal text too.
    let g = one("2,14,8,(0,-8),0").glyph(1, false).unwrap();
    assert!(close(g.advance, Vec2::new(0.0, -8.0)));
    assert_eq!(f.shape_number("ucb"), Some(66));
    assert_eq!(f.shape_name(65).as_deref(), Some("UCA"));
    assert_eq!(f.shape_numbers().first(), Some(&32));
}

#[test]
fn subshapes_shapes_unicode_and_extended_big_font() {
    let s = font("*1,4,BAR\n1,030,0\n*2,6,TWO\n7,1,7,1,0\n");
    assert!(close(s.glyph(2, false).unwrap().advance, Vec2::new(6.0, 0.0)));
    let u = font(UNI_FONT);
    assert_eq!((u.kind(), u.name(), u.above(), u.encoding()), (ShxKind::Unifont, "cadcraft uni", 6.0, 0));
    let ring = u.glyph(0xC5, false).unwrap();
    assert_eq!(ring.strokes.len(), 2, "the A and the ring's bar");
    assert!(close(ring.advance, Vec2::new(8.0, 0.0)));
    let e = font(EXT_BIG_FONT);
    assert_eq!((e.kind(), e.above()), (ShxKind::BigFont, 16.0));
    let g = e.glyph(0x82A2, false).unwrap();
    assert_eq!(g.strokes.len(), 2);
    let xs: Vec<(f64, f64)> = g.strokes.iter().map(|s| s.iter().fold((f64::MAX, f64::MIN), |(a, b), p| (a.min(p.x), b.max(p.x)))).collect();
    assert_eq!(xs, vec![(0.0, 8.0), (8.0, 16.0)], "each primitive squeezed into half the cell");
    let ys = g.strokes.iter().flatten().fold((f64::MAX, f64::MIN), |(a, b), p| (a.min(p.y), b.max(p.y)));
    assert_eq!(ys, (0.0, 16.0));
    assert!(close(g.advance, Vec2::new(16.0, 0.0)));
}

#[test]
fn big_font_header_and_escape_ranges() {
    let b = font(BIG_FONT);
    assert_eq!(b.kind(), ShxKind::BigFont);
    assert_eq!(b.escape_ranges(), &[(0x81, 0x9F)]);
    assert!(b.is_lead_byte(0x82) && !b.is_lead_byte(0x41) && !b.is_lead_byte(0xA0));
    assert_eq!(b.above(), 8.0);
    assert!(b.has_shape(0x82A0));
}

#[test]
fn cyclic_and_deep_subshapes_are_bounded() {
    let f = font("*1,3,SELF\n7,1,0\n*2,3,A\n7,3,0\n*3,3,B\n7,2,0\n*4,3,MISSING\n7,9,0\n");
    for n in 1..=4 {
        assert!(f.glyph(n, false).is_none(), "shape {n}");
    }
    // A chain deeper than the bound fails; a shallow one draws.
    let mut src = String::new();
    for n in 1..=40u16 {
        if n == 40 {
            src.push_str(&format!("*{n},2,L\n010,0\n"));
        } else {
            src.push_str(&format!("*{n},3,L\n7,{},0\n", n + 1));
        }
    }
    let f = font(&src);
    assert!(f.glyph(1, false).is_none() && f.glyph(30, false).is_some());
    // Doubling with subshapes: 2^n steps are cut off by the step bound.
    let mut src = String::from("*1,2,L\n010,0\n");
    for n in 2..=10u16 {
        src.push_str(&format!("*{n},9,D\n7,{0},7,{0},7,{0},7,{0},0\n", n - 1));
    }
    let f = font(&src);
    assert!(f.glyph(10, false).is_none() && f.glyph(3, false).is_some());
    // Pushes without pops, and runaway scales.
    let pushes = format!("{}0", "5,".repeat(100));
    assert!(one(&pushes).glyph(1, false).is_none());
    let scales = format!("{}010,0", "4,255,".repeat(10));
    assert!(one(&scales).glyph(1, false).is_none());
    // A zero radius or a zero factor draws nothing and doesn't fail.
    assert!(one("10,(0,001),4,0,010,0").glyph(1, false).is_some());
}

/// Parse every prefix and many mutations of `bytes`; draw every shape of whatever parses.
fn hammer(bytes: &[u8]) {
    let draw = |b: &[u8]| {
        if let Ok(f) = ShxFont::parse(b) {
            for n in f.shape_numbers() {
                let _ = f.glyph(n, false);
                let _ = f.glyph(n, true);
            }
            let _ = f.shape_number("X");
        }
    };
    for n in 0..=bytes.len() {
        draw(&bytes[..n]);
    }
    let mut seed: u64 = 0x5EED_1234;
    for _ in 0..2000 {
        let mut b = bytes.to_vec();
        for _ in 0..3 {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let i = (seed >> 33) as usize % b.len().max(1);
            if let Some(x) = b.get_mut(i) {
                *x = (seed >> 24) as u8;
            }
        }
        draw(&b);
    }
}

#[test]
fn hostile_files_never_panic() {
    for src in [TEXT_FONT, UNI_FONT, BIG_FONT, EXT_BIG_FONT] {
        hammer(&compile(src).unwrap());
    }
    // Counts far beyond the data.
    let mut shapes = parse::SHAPES_10.to_vec();
    shapes.extend([1, 0, 0xFF, 0xFF, 0xFF, 0xFF]);
    assert_eq!(ShxFont::parse(&shapes).err(), Some(ShxError::Truncated));
    let mut big = parse::BIGFONT.to_vec();
    big.extend([0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]);
    assert_eq!(ShxFont::parse(&big).err(), Some(ShxError::TooLarge));
    let mut big = parse::BIGFONT.to_vec();
    big.extend([0xFF, 0xFF, 0xFF, 0xFF, 0, 0]);
    assert_eq!(ShxFont::parse(&big).err(), Some(ShxError::Truncated));
    // A big font index pointing anywhere.
    let mut big = parse::BIGFONT.to_vec();
    big.extend([1, 0, 1, 0, 0, 0, 0x41, 0, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x7F]);
    assert_eq!(ShxFont::parse(&big).err(), Some(ShxError::Empty));
    let mut uni = parse::UNIFONT.to_vec();
    uni.extend([0xFF, 0xFF, 0xFF, 0xFF, 2, 0, b'x', 0]);
    assert_eq!(ShxFont::parse(&uni).err(), Some(ShxError::Empty));
    assert_eq!(ShxFont::parse(b"not a font").err(), Some(ShxError::NotShx));
    assert_eq!(ShxFont::parse(&[]).err(), Some(ShxError::NotShx));
    // Hostile shape source.
    for src in ["*1,2,X\n7\n", "*1,2,X\n9,(1,1)\n", "*99999,1,X\n0\n", "*1,1,X\n300,0\n", "010,0\n", "*1,1,X\n1,0zz,0\n", "*1,1,X\n0\n*1,1,Y\n0\n"] {
        assert!(compile(src).is_err(), "{src:?}");
    }
    assert!(compile("").is_err());
}

#[test]
fn a_far_extended_subshape_cell_is_cut_off() {
    // Hostile: nested cells 255 units out put the pen 255^4 units away; a location pushed there
    // and popped at the top became the advance.
    let mut src = String::from("*BIGFONT 6,1,081,09F\n*0,5,far\n1,0,0,1,0\n");
    for k in 1..=3 {
        src.push_str(&format!("*08F0{k},10,L\n7,0,08F0{},255,0,255,255,0\n", k + 1));
    }
    src.push_str("*08F04,2,PUSH\n5,0\n*082A0,10,T\n7,0,08F01,255,0,255,255,6,0\n*082A1,10,NEAR\n7,0,08F04,255,0,255,255,6,0\n");
    let f = font(&src);
    assert!(f.glyph(0x82A0, false).is_none());
    assert!(close(f.glyph(0x82A1, false).unwrap().advance, Vec2::new(255.0, 0.0)));
}

#[test]
fn subshape_fan_out_past_the_point_and_step_bounds_is_cut_off() {
    // Hostile: a few hundred bytes of fan-out drew 48,000 points per character (1.7 GB for a
    // 1,000-character text) or ran 200,000 steps per character with the pen up.
    let src = format!(
        "*1,1,TOO_MANY_POINTS\n{a}0\n*2,1,HUNDRED\n{b}0\n*3,3,V\n011,019,0\n*4,1,ENOUGH\n{c}0\n*5,1,TOO_MANY_STEPS\n{d}0\n*6,1,UP\n{e}0\n*7,3,U\n2,010,0\n",
        a = "7,2,".repeat(41),
        b = "7,3,".repeat(100),
        c = "7,2,".repeat(40),
        d = "7,6,".repeat(100),
        e = "7,7,".repeat(100),
    );
    let f = font(&src);
    assert!(f.glyph(1, false).is_none() && f.glyph(5, false).is_none());
    assert_eq!(f.glyph(4, false).unwrap().strokes.iter().map(Vec::len).sum::<usize>(), 8001);
    assert!(f.glyph(6, false).is_some());
}

#[test]
fn the_glyph_cache_is_bounded_by_points() {
    // Hostile: every glyph at the point bound, so 16,384 cached glyphs would hold 2 GB.
    let mut src = format!("*2,1,HUNDRED\n{}0\n*3,3,V\n011,019,0\n", "7,3,".repeat(100));
    for n in 10..=200 {
        src.push_str(&format!("*{n},1,HEAVY\n{}0\n", "7,2,".repeat(40)));
    }
    let f = font(&src);
    for n in 10..=200 {
        assert!(f.glyph(n, false).is_some());
    }
    let c = f.cache.read().unwrap();
    let cached: usize = c.glyphs.values().flatten().map(|g| g.strokes.iter().map(Vec::len).sum::<usize>()).sum();
    assert!(cached <= MAX_CACHED_POINTS && cached == c.points, "{cached}");
}

#[test]
fn a_name_is_looked_for_in_its_first_kilobyte_only() {
    // Hostile: 65,535 big font records over one 64 KB run without a NUL were each searched to
    // the end, 4 G bytes (1.7 s) for a 576 KB file.
    let (short, long) = ("N".repeat(1024), "N".repeat(1025));
    let f = font(&format!("*BIGFONT 2,1,081,09F\n*082A0,1,{short}\n010,0\n*082A1,1,{long}\n010,0\n"));
    assert!(f.has_shape(0x82A0) && !f.has_shape(0x82A1));
    let mut big = parse::BIGFONT.to_vec();
    big.extend([3, 0, 3, 0, 0, 0]);
    for n in 1..=3u8 {
        big.extend([n, 0, 0xFF, 0xFF, 55, 0, 0, 0]);
    }
    big.extend([b'N'; 0xFFFF]);
    assert_eq!(ShxFont::parse(&big).err(), Some(ShxError::Empty));
}

#[test]
fn all_65535_shape_numbers_compile_and_a_repeat_is_found() {
    // Hostile: each of 65,535 definitions was compared with all before it, 2.8 s for an 821 KB
    // source.
    let src: String = (1..=65535u32).map(|n| format!("*{n},1,X\n0\n")).collect();
    assert_eq!(ShxFont::parse(&compile(&src).unwrap()).unwrap().shape_count(), 65535);
    assert_eq!(compile("*1,1,X\n0\n*2,1,Y\n0\n*1,1,Z\n0\n").err(), Some(ShxError::Source(5, "shape 1 is defined twice".into())));
}

#[test]
fn a_doubled_minus_sign_is_not_a_number() {
    // Hostile: the digits after the sign were themselves negative, down to i64::MIN; as a big
    // font's range count the magnitude pushed ranges until 8 GB failed to allocate.
    for src in [
        "*1,1,X\n--9223372036854775808,0\n",
        "*BIGFONT 4,--9223372036854775807,081,09F\n*1,1,X\n0\n",
        "*1,1,X\n-0-8000000000000000,0\n",
        "*--5,1,X\n0\n",
        "*1,1,X\n+-5,0\n",
    ] {
        assert!(compile(src).is_err(), "{src:?}");
    }
}

/// Register a compiled test font under `name` (each test its own names: the table is shared).
fn reg(name: &str, src: &str) {
    register(name, &compile(src).unwrap()).unwrap();
}

#[test]
fn text_metrics_follow_the_font() {
    reg("cadcraft-test-metrics.shx", TEXT_FONT);
    let f = TextFont::resolve_style("CADCRAFT-TEST-METRICS", "", false);
    assert!(f.is_shx() && !f.is_outline());
    let h = 3.0;
    // Advances are the pen's end positions: A 6, B 6, at h / above = 0.5.
    assert!((crate::text_width(&f, "AB", h, 1.0) - 6.0).abs() < 1e-9);
    assert!((crate::text_width(&f, "AB", h, 0.5) - 3.0).abs() < 1e-9);
    let sh = crate::shape_line(&f, "AB", h, 1.0, 0.0);
    assert!((sh.width - 6.0).abs() < 1e-9);
    let ys = sh.strokes.iter().flatten().fold((f64::MAX, f64::MIN), |(a, b), p| (a.min(p.y), b.max(p.y)));
    assert_eq!(ys, (0.0, 3.0), "cap height = text height");
    // %%d %%p %%c come from shapes 256-258; \U+0041 is A.
    assert!((crate::text_width(&f, "%%d", h, 1.0) - 1.5).abs() < 1e-9);
    assert!((crate::text_width(&f, "%%p%%c", h, 1.0) - 4.0).abs() < 1e-9);
    assert!((crate::text_width(&f, "\\U+0041", h, 1.0) - 3.0).abs() < 1e-9);
    assert_eq!(crate::shape_line(&f, "\\U+0041", h, 1.0, 0.0), crate::shape_line(&f, "A", h, 1.0, 0.0));
    // A character the font lacks comes from the stroke font, with its advance.
    let z = crate::text_width(&f, "Z", h, 1.0);
    assert!((z - crate::stroke_advance('Z', h, 1.0)).abs() < 1e-9);
    assert!(!crate::shape_line(&f, "Z", h, 1.0, 0.0).strokes.is_empty());
    // Widths and shapes agree, with obliquing, underline and mixed fallbacks.
    for s in ["AB AZ", "%%uAB%%u", "A\u{3042}B"] {
        let sh = crate::shape_line(&f, s, h, 0.8, 0.2);
        assert!((sh.width - crate::text_width(&f, s, h, 0.8)).abs() < 1e-9, "{s}");
    }
    // Placement: centred text sits on the alignment point by the font's own width.
    let p = TextParams { h: crate::Align::Center, align_pt: Some(Vec2::new(10.0, 0.0)), ..TextParams::new(Vec2::ZERO, h) };
    let (_, bb) = crate::place(&f, "AB", &p);
    assert!((bb.center().x - 10.0).abs() < 1e-9 && (bb.width() - 6.0).abs() < 1e-9);
    // MTEXT with the style's SHX font, and its \U+ escape.
    let mp = crate::MTextParams { font: f.clone(), ..crate::MTextParams::new(Vec2::ZERO, h) };
    let m = crate::layout_mtext_with("\\U+0041B", &mp);
    assert_eq!(m.lines, vec!["AB".to_string()]);
    assert_eq!(crate::space_width(&f, h, 1.0), 3.0);
}

#[test]
fn unifont_text_and_specials() {
    reg("cadcraft-test-uni.shx", UNI_FONT);
    let f = TextFont::resolve("cadcraft-test-uni.shx");
    assert!((crate::text_width(&f, "A\u{C5}", 6.0, 1.0) - 14.0).abs() < 1e-9);
    // %%c is the diameter sign, at U+2205 in this font.
    assert!((crate::text_width(&f, "%%c", 6.0, 1.0) - 2.0).abs() < 1e-9);
}

#[test]
fn big_font_draws_double_byte_characters() {
    reg("cadcraft-test-main.shx", TEXT_FONT);
    reg("cadcraft-test-big.shx", BIG_FONT);
    let f = TextFont::resolve_style("cadcraft-test-main.shx", "cadcraft-test-big.shx", false);
    let TextFont::Shx(t) = &f else { panic!("SHX") };
    assert_eq!(t.big_font_encoding(), "Shift_JIS");
    let h = 2.5;
    // あ (Shift-JIS 82A0) fills a square cell of the text height.
    let w = crate::text_width(&f, "\u{3042}", h, 1.0);
    assert!((w - h).abs() < 1e-9, "{w}");
    let sh = crate::shape_line(&f, "A\u{3042}", h, 1.0, 0.0);
    let big: Vec<Vec2> = sh.strokes.iter().flatten().copied().filter(|p| p.x > 2.5 - 1e-9).collect();
    let ys = big.iter().fold((f64::MAX, f64::MIN), |(a, b), p| (a.min(p.y), b.max(p.y)));
    assert_eq!(ys, (0.0, h));
    // Only the big font found: Latin letters from the stroke font, the big font's own.
    let f = TextFont::resolve_style("cadcraft-missing-main-font.shx", "cadcraft-test-big.shx", false);
    assert!(f.is_shx());
    assert!((crate::text_width(&f, "\u{3042}", h, 1.0) - h).abs() < 1e-9);
    assert!((crate::text_width(&f, "A", h, 1.0) - crate::stroke_advance('A', h, 1.0)).abs() < 1e-9);
    // Neither found: the stroke font as before.
    assert!(!TextFont::resolve_style("cadcraft-missing-a.shx", "cadcraft-missing-b.shx", false).is_shx());
}

#[test]
fn big_font_code_pages() {
    for (name, ranges, want) in [
        ("gbcbig.shx", vec![(0xA1, 0xFE)], "GBK"),
        ("chineset.shx", vec![(0xA1, 0xFE)], "Big5"),
        ("whgtxt.shx", vec![(0xA1, 0xFE)], "EUC-KR"),
        ("extfont2.shx", vec![(0x81, 0x9F)], "Shift_JIS"),
        ("custom.shx", vec![(0x81, 0x9F), (0xE0, 0xFC)], "Shift_JIS"),
        ("custom.shx", vec![(0xA1, 0xFE)], "GBK"),
    ] {
        let src = format!(
            "*BIGFONT 1,{},{}\n*0,4,x\n8,0,0,0\n*0FFFF,2,x\n010,0\n",
            ranges.len(),
            ranges.iter().map(|(a, b)| format!("0{a:X},0{b:X}")).collect::<Vec<_>>().join(",")
        );
        let b = std::sync::Arc::new(font(&src));
        assert_eq!(ShxText::new(None, Some(b), name, false).big_font_encoding(), want, "{name}");
    }
}

#[test]
fn vertical_text_runs_down() {
    reg("cadcraft-test-vertical.shx", TEXT_FONT);
    let v = TextFont::resolve_style("cadcraft-test-vertical.shx", "", true);
    let sh = crate::shape_line(&v, "CC", 1.0, 1.0, 0.0);
    let miny = sh.strokes.iter().flatten().map(|p| p.y).fold(f64::MAX, f64::min);
    // Two characters, each 8 units (8/6 of the height) down.
    assert!((miny + 16.0 / 6.0).abs() < 1e-9, "{miny}");
    assert!(sh.strokes.iter().flatten().all(|p| p.y <= 1e-9), "below the insertion point");
    // A font without modes 2 stays horizontal.
    reg("cadcraft-test-horizontal.shx", "*0,4,h\n6,2,0,0\n*67,3,C\n1,064,0\n");
    let h = TextFont::resolve_style("cadcraft-test-horizontal.shx", "", true);
    assert!(crate::shape_line(&h, "CC", 1.0, 1.0, 0.0).strokes.iter().flatten().all(|p| p.y >= -1e-9));
}

#[test]
fn names_and_lookup() {
    assert_eq!(table::key("ROMANS.SHX").as_deref(), Some("romans.shx"));
    assert_eq!(table::key("txt").as_deref(), Some("txt.shx"));
    assert_eq!(table::key(r"C:\Fonts\Simplex.shx").as_deref(), Some("simplex.shx"));
    assert_eq!(table::key("../x/gbcbig.shx").as_deref(), Some("gbcbig.shx"));
    for n in ["", "arial.ttf", "CADCraft Stroke", "Times New Roman", ".shx", "a\u{0}.shx"] {
        assert_eq!(table::key(n), None, "{n:?}");
    }
    assert!(register("not-a-font.ttf", &compile(TEXT_FONT).unwrap()).is_err());
    assert!(register("broken.shx", b"AutoCAD-86 shapes 1.0\r\n\x1a").is_err());
    // A path stored in a drawing only names the file.
    reg("cadcraft-test-path.shx", TEXT_FONT);
    assert!(find(r"\\server\share\CADCRAFT-TEST-PATH.SHX").is_some());
    assert!(find("cadcraft-never-registered.shx").is_none());
}

#[test]
fn arcs_follow_the_circle() {
    // Every point of an octant arc is on its circle.
    let g = one("10,(5,034),0").glyph(1, false).unwrap();
    let c = -dir(135.0) * 5.0;
    for p in g.strokes.iter().flatten() {
        assert!(((*p - c).len() - 5.0).abs() < 1e-9);
    }
    assert!(close(g.advance, (dir(135.0 + 180.0) - dir(135.0)) * 5.0));
    let _ = PI;
}

/// A one-byte font with Hebrew letters at `codes`, each shape advancing by a different amount
/// (its code's low byte in units), and a font record 6 units high.
fn hebrew_font(codes: impl Iterator<Item = u16>) -> ShxFont {
    let mut src = String::from("*0,4,hebrew\n6,2,0,0\n");
    for c in codes {
        src.push_str(&format!("*0{c:X},3,h\n8,({},0),0\n", c & 0x1F));
    }
    font(&src)
}

/// Israeli SHX fonts keep Hebrew letters at their Windows-1255 (0xE0..0xFA) or DOS 862
/// (0x80..0x9A) places: Hebrew text draws from them, while a Cyrillic font with letters at the
/// same places is not taken for Hebrew (#2316, rebuilt on #1944).
#[test]
fn hebrew_letters_draw_from_their_one_byte_places() {
    let draws = |f: ShxFont, c: char| {
        let t = ShxText::new(Some(std::sync::Arc::new(f)), None, "", false);
        super::text::width(&t, &c.to_string(), 6.0, 1.0)
    };
    // Windows-1255: alef at 0xE0 advances 0xE0 & 0x1F = 0 units; bet at 0xE1 one unit.
    assert!((draws(hebrew_font(0xE0..=0xFA), 'ב') - 1.0).abs() < 1e-9);
    // DOS 862: bet at 0x81 advances one unit.
    assert!((draws(hebrew_font(0x80..=0x9A), 'ב') - 1.0).abs() < 1e-9);
    assert_eq!(hebrew_font(0x80..=0x9A).hebrew_places(), (false, true));
    // The old degree sign at 127 isn't a letter in DOS 862: `%%d` still draws from it
    // (127 & 0x1F = 31 units).
    assert!((draws(hebrew_font((0x80..=0x9A).chain([127])), '°') - 31.0).abs() < 1e-9);
    // A Cyrillic-style font (0xC0..0xFF all drawn) is not read as Hebrew: the letter comes
    // from the built-in stroke font instead of a Cyrillic shape.
    let cyr = hebrew_font(0xC0..=0xFF);
    assert_eq!(cyr.hebrew_places(), (false, false));
    let stroke = crate::stroke_advance('ב', 6.0, 1.0);
    assert!((draws(cyr, 'ב') - stroke).abs() < 1e-9);
}
