//! Lines with right-to-left text or Thai: the line is split into bidi runs in display order
//! ([`crate::bidi::visual_runs`]), each run into pieces by the font that draws them (the text's
//! font, a fallback font for characters it lacks, or the stroke font), and each font piece is
//! shaped with `harfrust` (OpenType GSUB/GPOS: Arabic contextual forms, lam-alef and other
//! ligatures, mark positioning, mirroring) before its glyphs are placed. Shaping happens per
//! piece in the font that draws it, so Arabic in a stroke-font style is joined by the fallback
//! font. Fonts without OpenType Arabic features get Unicode presentation forms instead.
//!
//! Thai is left to right but needs the shaper too: a consonant carries up to two stacked marks
//! above (a vowel, then a tone mark) and one below, placed by the font's `mark`/`mkmk`
//! positioning; letters with a descender (ฐ, ญ) lose it under a below vowel (`ccmp`); and sara
//! am (ำ) splits into nikhahit and sara aa with the nikhahit moved before any tone mark.

use std::ops::Range;
use std::panic::{AssertUnwindSafe, catch_unwind};

use cadcraft_geom::Vec2;

use crate::ttf::{self, Face};
use crate::{Shaped, bidi, stroke};

/// Longest line (characters) laid out this way; longer lines keep one glyph per character.
const MAX_CHARS: usize = 10_000;

/// Whether a line is laid out here rather than one glyph per character: it has right-to-left
/// text, or Thai (see the module notes).
pub(crate) fn needed(s: &str) -> bool {
    s.chars().any(|c| bidi::is_rtl(c) || is_thai(c))
}

/// Whether `c` is in the Thai block.
pub(crate) fn is_thai(c: char) -> bool {
    matches!(c, '\u{0E00}'..='\u{0E7F}')
}

/// What draws a piece of a run.
#[derive(Clone)]
enum Src {
    /// The built-in stroke font.
    Stroke,
    /// No font has the character: a missing-glyph box.
    Missing,
    /// An invisible formatting character outside any shaped piece.
    Hidden,
    /// A TrueType/OpenType face (the text's font or a fallback).
    Face(Face),
}

impl Src {
    fn same(&self, o: &Src) -> bool {
        match (self, o) {
            (Src::Stroke, Src::Stroke) | (Src::Missing, Src::Missing) | (Src::Hidden, Src::Hidden) => true,
            (Src::Face(a), Src::Face(b)) => a.same(b),
            _ => false,
        }
    }
    fn face(&self) -> Option<&Face> {
        match self {
            Src::Face(f) => Some(f),
            _ => None,
        }
    }
}

/// Shape one line containing right-to-left text in `primary` (None: the stroke font), with the
/// same conventions as the left-to-right paths: baseline at y = 0, x from 0, text height = cap
/// height, `%%` codes decoded. `base_rtl` is the line's direction (`None`: that of its first
/// strong character). `None` when the line is too long (the caller then lays it out one glyph
/// per character) or shaping failed.
pub(crate) fn shape(primary: Option<&Face>, s: &str, height: f64, width_factor: f64, oblique: f64, base_rtl: Option<bool>) -> Option<Shaped> {
    let chars = crate::decode_controls(s);
    if chars.is_empty() || chars.len() > MAX_CHARS {
        return None;
    }
    // A last-resort guard around the bidi and shaping libraries: a panic in them must not take
    // the drawing down with it (the line then falls back to one glyph per character).
    catch_unwind(AssertUnwindSafe(|| shape_chars(primary, &chars, height, width_factor, oblique, base_rtl))).ok()
}

/// Lay out decoded directional runs, keeping advances and decoration spans for empty glyphs.
fn shape_chars(primary: Option<&Face>, chars: &[(char, bool, bool)], height: f64, width_factor: f64, oblique: f64, base_rtl: Option<bool>) -> Shaped {
    let normalized_height = if height.is_finite() && height > 0.0 { height } else { 1.0 };
    let scale_x = if width_factor.is_finite() && width_factor.abs() > 1e-6 { width_factor } else { 1.0 };
    let shear = oblique.tan().clamp(-10.0, 10.0);
    let stroke_scale = normalized_height / stroke::CAP;
    // A character the text's face lacks is one of its substitutes when it has one, as in the
    // left-to-right path. The bidi runs come from the text as decoded, before substitution: a
    // substitute may be of another bidi class (⌀ and ∅ are neutral, Ø is left-to-right), and
    // the line keeps the direction its own text gives it; the substitute is drawn at the same
    // logical index.
    let decoded: Vec<char> = chars.iter().map(|character| character.0).collect();
    let characters: Vec<char> = decoded
        .iter()
        .map(|&character| match primary {
            Some(face) if !character.is_whitespace() && !face.has_glyph(character) => {
                crate::substitutes(character).iter().copied().find(|&substitute| face.has_glyph(substitute)).unwrap_or(character)
            }
            _ => character,
        })
        .collect();
    let flags = |index: usize| chars.get(index).map_or((false, false), |character| (character.1, character.2));
    let mut shaped = Shaped::default();
    let mut points_left = ttf::MAX_SHAPED_POINTS;
    let mut spans: Vec<(f64, f64, bool, bool)> = Vec::new();
    let mut pen_x = 0.0;
    // Whether the rightmost glyph so far is a stroke-font glyph.
    let mut stroke_last = false;
    for (run, rtl) in bidi::visual_runs(&decoded, base_rtl) {
        let mut pieces = itemize(&characters, run, primary);
        if rtl {
            pieces.reverse();
        }
        for (src, range) in pieces {
            match &src {
                Src::Hidden => {}
                Src::Stroke | Src::Missing => {
                    let indices: Vec<usize> = if rtl { range.rev().collect() } else { range.collect() };
                    for index in indices {
                        let character = characters.get(index).copied().unwrap_or(' ');
                        let character = if rtl { bidi::mirror(character) } else { character };
                        match (&src, stroke::glyph(character)) {
                            (Src::Stroke, Some((_, spec))) => {
                                crate::draw_stroke_glyph(&mut shaped, spec, pen_x, stroke_scale, scale_x, shear);
                            }
                            (Src::Missing, _) => crate::missing_glyph_box(&mut shaped, pen_x, normalized_height, stroke_scale * scale_x),
                            _ => {}
                        }
                        let advance = crate::char_advance(character) * normalized_height * scale_x;
                        let (underline, overline) = flags(index);
                        spans.push((pen_x, pen_x + advance, underline, overline));
                        pen_x += advance;
                        stroke_last = true;
                    }
                }
                Src::Face(face) => {
                    let glyphs = shape_run(face, &characters, range.clone(), rtl);
                    // Right-to-left runs stay outlines only: their glyphs come in display order
                    // and include ligatures (lam-alef) that stand for several characters.
                    let shows = if rtl { Vec::new() } else { glyph_chars(face, &characters, &glyphs, range.end) };
                    for (n, glyph) in glyphs.iter().enumerate() {
                        let outline = ttf::glyph_by_id(face, glyph.gid);
                        let glyph_scale = normalized_height / outline.cap * scale_x;
                        let offset_y = glyph.dy * normalized_height / outline.cap;
                        if let Some(mut contours) =
                            ttf::place_glyph_budgeted(&outline, pen_x + glyph.dx * glyph_scale, normalized_height, scale_x, shear, &mut points_left)
                        {
                            for point in contours.iter_mut().flatten() {
                                *point += Vec2::new(offset_y * shear, offset_y);
                            }
                            let origin = Vec2::new(pen_x + glyph.dx * glyph_scale + offset_y * shear, offset_y);
                            let texts = shows.get(n).into_iter().flatten().filter(|character| !character.is_whitespace()).filter_map(|character| {
                                ttf::text_glyph_at(&outline, *character, origin, normalized_height / outline.cap, scale_x, shear)
                            });
                            shaped.push_glyph(contours, texts);
                        }
                        let advance = glyph.advance * glyph_scale;
                        let (underline, overline) = flags(glyph.cluster);
                        spans.push((pen_x, pen_x + advance, underline, overline));
                        pen_x += advance;
                        stroke_last = false;
                    }
                }
            }
        }
    }
    shaped.strokes.extend(crate::decorations(&spans, normalized_height));
    // A stroke-font glyph at the right end of a stroke-font line: the width leaves out its
    // trailing gap, like `shape_stroke`. A TrueType line keeps it, like `ttf::shape`.
    shaped.width = if stroke_last && primary.is_none() { (pen_x - stroke::GAP * stroke_scale * scale_x).max(0.0) } else { pen_x };
    shaped
}

/// Split a logical-order run into pieces by what draws each character. Joiners and combining
/// marks stay in the piece before them (the shaper needs them in context); other characters
/// prefer the previous fallback face when it has them, so one word stays in one font.
fn itemize(cs: &[char], run: Range<usize>, primary: Option<&Face>) -> Vec<(Src, Range<usize>)> {
    let mut pieces: Vec<(Src, Range<usize>)> = Vec::new();
    for i in run {
        let c = cs.get(i).copied().unwrap_or(' ');
        let prev = pieces.last().and_then(|(s, _)| s.face()).cloned();
        let src = match prev {
            Some(f) if bidi::is_format_control(c) || bidi::is_mark(c) => Src::Face(f),
            None if bidi::is_format_control(c) => Src::Hidden,
            prev => resolve(c, primary, prev.as_ref()),
        };
        match pieces.last_mut() {
            Some((s, r)) if s.same(&src) => r.end = i + 1,
            _ => pieces.push((src, i..i + 1)),
        }
    }
    pieces
}

/// What draws `c`: the text's face when it has the character, else the previous fallback face,
/// else the first fallback font that has it (FONTFALLBACK, FONTALT, installed fonts), else the
/// stroke font when it has it; what no font has is the text's face's missing-glyph shape (a box
/// in the stroke font).
fn resolve(c: char, primary: Option<&Face>, prev: Option<&Face>) -> Src {
    if let Some(p) = primary
        && (c.is_whitespace() || p.has_glyph(c))
    {
        return Src::Face(p.clone());
    }
    if primary.is_none() && (c.is_whitespace() || stroke::glyph(c).is_some()) {
        return Src::Stroke;
    }
    if let Some(f) = prev.filter(|f| f.has_glyph(c)) {
        return Src::Face(f.clone());
    }
    match (ttf::fallback_for(c), primary) {
        (Some(f), _) => Src::Face(f),
        // No font has it: the stroke font's glyph, as in the left-to-right path.
        (None, _) if stroke::glyph(c).is_some() => Src::Stroke,
        // The text's font draws its own missing-glyph shape.
        (None, Some(p)) => Src::Face(p.clone()),
        (None, None) => Src::Missing,
    }
}

/// A shaped glyph: id, source character index, advance and offset in font units at 1000 per em.
struct Glyph {
    gid: u32,
    cluster: usize,
    advance: f64,
    dx: f64,
    dy: f64,
}

/// Shape characters `r` of `cs` (one direction, one face) into glyphs in display order.
fn shape_run(face: &Face, cs: &[char], r: Range<usize>, rtl: bool) -> Vec<Glyph> {
    let run = cs.get(r.clone()).unwrap_or_default();
    // Keep the shared font bytes: shaping a run must not clone the entire font file.
    let source = harfrust::font::Blob::Shared(face.bytes.clone());
    let Some(font) = harfrust::Font::new(source, face.index) else { return Vec::new() };
    let shaper = harfrust::ShaperFont::new(&font);
    let mut buf = harfrust::Buffer::new();
    let arabic = run.iter().any(|c| matches!(*c, '\u{0600}'..='\u{08FF}'));
    if arabic && !face.has_joining_features() {
        for (c, i) in bidi::presentation_forms(run, |p| face.has_glyph(p)) {
            buf.push(u32::from(c), u32::try_from(r.start + i).unwrap_or(u32::MAX));
        }
    } else {
        for (i, c) in run.iter().enumerate() {
            buf.push(u32::from(*c), u32::try_from(r.start + i).unwrap_or(u32::MAX));
        }
    }
    buf.set_direction(if rtl { harfrust::Direction::RightToLeft } else { harfrust::Direction::LeftToRight });
    buf.guess_segment_properties();
    if harfrust::shape(&shaper, &mut buf, harfrust::ShapeOptions::new()).is_err() {
        return Vec::new();
    }
    let k = 1000.0 / f64::from(shaper.units_per_em().max(1));
    buf.glyph_infos()
        .iter()
        .zip(buf.glyph_positions())
        .map(|(gi, gp)| Glyph {
            gid: gi.glyph_id,
            cluster: usize::try_from(gi.cluster).unwrap_or(usize::MAX),
            advance: f64::from(gp.x_advance) * k,
            dx: f64::from(gp.x_offset) * k,
            dy: f64::from(gp.y_offset) * k,
        })
        .collect()
}

/// Longest cluster whose glyphs are matched to its characters by glyph id; the glyphs of a
/// longer one (a hostile run of thousands of stacked marks) take its characters in order.
const MAX_MATCHED_CLUSTER: usize = 32;

/// The characters each glyph of a left-to-right run shows, for output that keeps text as text
/// ([`crate::TextGlyph`]). The shaper reorders, splits and substitutes glyphs inside a cluster
/// (a letter with its marks), so within each cluster a glyph that is a character's own glyph
/// takes that character first, then the other glyphs take the cluster's remaining characters in
/// order, and characters left over (a ligature such as ฐ with sara u below) are shown by the
/// glyph before them. Each character is shown once, and the glyphs show the characters in typed
/// order wherever that leaves no fewer glyphs showing one, so text read back in drawing order is
/// the typed text: a split Thai sara am is drawn as nikhahit, then any tone mark, then sara aa,
/// so in น้ำ the sara aa shows ำ and the nikhahit, moved before the tone mark, shows none and is
/// drawn as an outline only.
fn glyph_chars(face: &Face, cs: &[char], glyphs: &[Glyph], end: usize) -> Vec<Vec<char>> {
    let mut out: Vec<Vec<char>> = vec![Vec::new(); glyphs.len()];
    let mut i = 0;
    while let Some(cluster) = glyphs.get(i).map(|g| g.cluster) {
        let j = glyphs.iter().skip(i).position(|g| g.cluster != cluster).map_or(glyphs.len(), |n| i + n);
        // Clusters ascend in a left-to-right run: this one's characters end where the next starts.
        let to = glyphs.get(j).map_or(end, |g| g.cluster).clamp(cluster, end.max(cluster));
        let chars = cs.get(cluster..to).unwrap_or_default();
        let ids = if chars.len() <= MAX_MATCHED_CLUSTER { face.glyph_ids(chars) } else { Vec::new() };
        let gids: Vec<u32> = glyphs.get(i..j).unwrap_or_default().iter().map(|g| g.gid).collect();
        let any = cluster_chars(chars, &ids, &gids, false).unwrap_or_default();
        // Output that keeps one character per glyph (render, PDF) writes each glyph's first: typed
        // order must not leave fewer of them than the plain assignment.
        let firsts = |s: &[Vec<char>]| s.iter().filter(|c| !c.is_empty()).count();
        let shows = cluster_chars(chars, &ids, &gids, true).filter(|t| firsts(t) >= firsts(&any)).unwrap_or(any);
        for (o, s) in out.iter_mut().skip(i).zip(shows) {
            *o = s;
        }
        i = j;
    }
    out
}

/// The characters each glyph `gids` of one cluster shows, of the cluster's characters `chars`
/// with their own glyph ids `ids` (empty: none matched): a glyph that is a character's own glyph
/// takes that character first, then the other glyphs take the remaining characters in order,
/// and the cluster's last glyph takes those left over. `typed`: own glyphs are matched in typed
/// order only and a character with no glyph of its own between them goes with the glyph before
/// it, so the glyphs show the characters in typed order; `None` when a character comes before
/// every glyph that shows one (a vowel sign drawn before its consonant).
fn cluster_chars(chars: &[char], ids: &[Option<u32>], gids: &[u32], typed: bool) -> Option<Vec<Vec<char>>> {
    let mut taken = vec![false; chars.len()];
    let mut at: Vec<Vec<usize>> = vec![Vec::new(); gids.len()];
    let mut from = 0;
    for (a, gid) in at.iter_mut().zip(gids) {
        let own = ids.iter().zip(&taken).enumerate().skip(from).find(|(_, (id, t))| !**t && **id == Some(*gid)).map(|(p, _)| p);
        if let Some(p) = own {
            a.push(p);
            taken.get_mut(p).into_iter().for_each(|t| *t = true);
            from = if typed { p + 1 } else { 0 };
        }
    }
    // The characters before `next` are shown; `last` is the glyph that shows the latest one.
    let (mut next, mut last): (usize, Option<usize>) = (0, None);
    for k in 0..at.len() {
        match at.get(k).and_then(|a| a.first()).copied() {
            Some(p) if typed => {
                if next < p {
                    at.get_mut(last?)?.extend(next..p);
                }
                (next, last) = (p + 1, Some(k));
            }
            Some(_) => {}
            None => {
                while !typed && taken.get(next) == Some(&true) {
                    next += 1;
                }
                if taken.get(next) == Some(&false) {
                    at.get_mut(k)?.push(next);
                    (next, last) = (next + 1, Some(k));
                }
            }
        }
    }
    let rest: Vec<usize> = (next..chars.len()).filter(|p| taken.get(*p) == Some(&false)).collect();
    if !rest.is_empty() {
        at.last_mut()?.extend(rest);
    }
    Some(at.into_iter().map(|a| a.into_iter().filter_map(|p| chars.get(p).copied()).collect()).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A single empty glyph with a positive advance, without relying on installed fonts.
    fn empty_glyph_face() -> Face {
        mapped_face(&[])
    }

    /// [`empty_glyph_face`] with a character map: each character to its glyph id (all ids draw
    /// the one empty glyph).
    fn mapped_face(map: &[(char, u32)]) -> Face {
        let mut head = vec![0_u8; 54];
        head[..4].copy_from_slice(&0x0001_0000_u32.to_be_bytes());
        head[18..20].copy_from_slice(&1000_u16.to_be_bytes());
        let mut hhea = vec![0_u8; 36];
        hhea[..4].copy_from_slice(&0x0001_0000_u32.to_be_bytes());
        hhea[4..6].copy_from_slice(&800_i16.to_be_bytes());
        hhea[6..8].copy_from_slice(&(-200_i16).to_be_bytes());
        hhea[34..36].copy_from_slice(&1_u16.to_be_bytes());
        let mut cmap = Vec::from([0, 0, 0, 1, 0, 3, 0, 10, 0, 0, 0, 12, 0, 12, 0, 0]); // Format 12 at 12.
        let len = u32::try_from(16 + 12 * map.len()).unwrap();
        cmap.extend(len.to_be_bytes().into_iter().chain([0; 4]).chain(u32::try_from(map.len()).unwrap().to_be_bytes()));
        let mut map = map.to_vec();
        map.sort_unstable();
        for (c, gid) in map {
            cmap.extend([u32::from(c), u32::from(c), gid].into_iter().flat_map(u32::to_be_bytes));
        }
        let tables = [
            (*b"cmap", cmap),
            (*b"head", head),
            (*b"hhea", hhea),
            (*b"hmtx", vec![1, 244, 0, 0]),      // Advance 500, left side bearing 0.
            (*b"maxp", vec![0, 0, 80, 0, 0, 1]), // Version 0.5, one glyph.
        ];
        let mut bytes = vec![0_u8; 12 + tables.len() * 16];
        bytes[..4].copy_from_slice(&0x0001_0000_u32.to_be_bytes());
        bytes[4..6].copy_from_slice(&u16::try_from(tables.len()).unwrap().to_be_bytes());
        bytes[6..8].copy_from_slice(&64_u16.to_be_bytes());
        bytes[8..10].copy_from_slice(&2_u16.to_be_bytes());
        for (i, (tag, data)) in tables.into_iter().enumerate() {
            while !bytes.len().is_multiple_of(4) {
                bytes.push(0);
            }
            let record = 12 + i * 16;
            let offset = u32::try_from(bytes.len()).unwrap();
            let len = u32::try_from(data.len()).unwrap();
            bytes[record..record + 4].copy_from_slice(&tag);
            bytes[record + 8..record + 12].copy_from_slice(&offset.to_be_bytes());
            bytes[record + 12..record + 16].copy_from_slice(&len.to_be_bytes());
            bytes.extend(data);
        }
        Face::new(std::sync::Arc::new(bytes), 0).expect("synthetic font parses")
    }

    /// Empty complex glyphs retain spacing and underlines without adding geometry or text slots.
    #[test]
    fn empty_complex_glyph_keeps_advance_without_geometry() {
        let face = empty_glyph_face();
        let shaped = shape(Some(&face), " ", 1.0, 1.0, 0.0, None).expect("shapes empty glyph");
        assert!(shaped.width > 0.0, "the font's advance is retained");
        assert!(shaped.is_empty());
        assert!(shaped.glyphs.is_empty());
        assert!(shaped.text.is_empty());

        let underlined = shape(Some(&face), "%%u ", 1.0, 1.0, 0.0, None).expect("shapes decorated empty glyph");
        assert!((underlined.width - shaped.width).abs() < f64::EPSILON);
        assert!(underlined.glyphs.is_empty());
        assert!(underlined.text.is_empty());
        assert!(!underlined.strokes.is_empty(), "decoration spans survive an empty outline");
    }

    #[test]
    fn stroke_font_rtl_lines_keep_latin_and_digits_in_the_stroke_font() {
        // With or without installed fonts: the Persian words are fallback glyphs or
        // missing-glyph boxes, the digits stay stroke glyphs, in the middle of the line (between
        // the two words), and the measured width matches the shaped one.
        let s = "قیمت 25 تومان";
        let sh = shape(None, s, 1.0, 1.0, 0.0, None).unwrap_or_default();
        assert!(sh.width > 0.0);
        assert!((crate::line_width(s, 1.0, 1.0) - sh.width).abs() < 1e-9);
        let digits: Vec<f64> = sh.strokes.iter().filter(|st| !(st.len() == 5 && st.first() == st.last())).flatten().map(|p| p.x).collect();
        assert!(!digits.is_empty());
        let (lo, hi) = (digits.iter().copied().fold(f64::MAX, f64::min), digits.iter().copied().fold(f64::MIN, f64::max));
        assert!(lo > 0.2 * sh.width && hi < 0.8 * sh.width, "digits between the words: {lo}..{hi} of {}", sh.width);
    }

    #[test]
    fn rtl_order_of_stroke_glyphs() {
        // Hebrew letters are R; in an RTL line the Latin word keeps its order and sits where the
        // bidi algorithm puts it: "abc" first visually when it ends the logical line.
        let rtl = crate::shape_stroke("\u{05D0} abc", 1.0, 1.0, 0.0);
        let ltr = crate::shape_stroke("abc", 1.0, 1.0, 0.0);
        let x0 = |s: &Shaped| s.strokes.iter().flatten().map(|p| p.x).fold(f64::MAX, f64::min);
        // "abc" is laid out at the left edge in both.
        assert!((x0(&rtl) - x0(&ltr)).abs() < 1e-9);
        assert!(rtl.width > ltr.width);
    }

    #[test]
    fn invisible_controls_take_no_space() {
        let a = crate::line_width("AB", 1.0, 1.0);
        assert!((crate::line_width("A\u{200C}B", 1.0, 1.0) - a).abs() < 1e-9);
        assert!((crate::line_width("A\u{200F}B", 1.0, 1.0) - a).abs() < 1e-9);
        let sh = crate::shape_stroke("A\u{200F}B", 1.0, 1.0, 0.0);
        assert!(!sh.strokes.iter().any(|s| s.len() == 5 && s.first() == s.last()), "no boxes for marks");
    }

    #[test]
    fn a_face_lacking_what_no_font_has_gets_the_stroke_glyph() {
        // A face that maps no character stands in for the text's font lacking one. What no
        // installed font has either is the stroke font's glyph when it has one, as in the
        // left-to-right path; what the stroke font lacks too, and whitespace, stay the face's.
        let junk = Face { bytes: std::sync::Arc::new(b"not a font".to_vec()), index: 0 };
        let c = '\u{2300}';
        match ttf::fallback_for(c) {
            None => assert!(matches!(resolve(c, Some(&junk), None), Src::Stroke), "the stroke font's glyph"),
            Some(f) => assert!(matches!(resolve(c, Some(&junk), None), Src::Face(g) if g.same(&f)), "the fallback font's"),
        }
        assert!(matches!(resolve('\u{E000}', Some(&junk), None), Src::Face(_)), "the face's missing-glyph shape");
        assert!(matches!(resolve(' ', Some(&junk), None), Src::Face(_)));
        assert!(matches!(resolve(c, None, None), Src::Stroke));
        if ttf::fallback_for(c).is_none() {
            // Drawn as strokes, not a box, with the stroke font's advance; the trailing gap
            // stays in the width, as `ttf::shape` keeps it.
            let sh = shape(Some(&junk), "\u{05D0}\u{2300}", 1.0, 1.0, 0.0, None).unwrap_or_default();
            assert!(sh.strokes.iter().any(|s| s.len() == 2), "the slash of the diameter sign");
            assert!(!sh.strokes.iter().any(|s| s.len() == 5 && s.first() == s.last()), "no missing-glyph box");
            assert!(sh.width >= crate::char_advance(c) - 1e-9, "{}", sh.width);
        }
    }

    /// An installed font that shapes Arabic (Persian), or `None` (the test then skips).
    fn arabic_face() -> Option<Face> {
        ttf::fallback_for('\u{0633}').filter(|f| f.has_glyph('\u{06CC}') && f.has_glyph('\u{067E}'))
    }

    fn nominal(f: &Face, c: char) -> Option<u32> {
        use skrifa::MetadataProvider;
        f.font().and_then(|font| font.charmap().map(c)).map(skrifa::GlyphId::to_u32)
    }

    #[test]
    fn persian_is_shaped_by_an_installed_font_when_available() {
        let Some(face) = arabic_face() else { return };
        let cs: Vec<char> = "سلام".chars().collect();
        let gs = shape_run(&face, &cs, 0..cs.len(), true);
        // Display order is right to left: clusters come out descending.
        let clusters: Vec<usize> = gs.iter().map(|g| g.cluster).collect();
        assert!(clusters.windows(2).all(|w| w[0] >= w[1]), "{clusters:?}");
        // Seen is word-initial: not its isolated (nominal) glyph.
        let seen = gs.iter().find(|g| g.cluster == 0).map(|g| g.gid);
        assert!(seen.is_some() && seen != nominal(&face, '\u{0633}'), "initial form");
        // ZWNJ: in "می‌روم" yeh is final, in "میروم" medial.
        let with: Vec<char> = "می\u{200C}روم".chars().collect();
        let without: Vec<char> = "میروم".chars().collect();
        let yeh = |cs: &[char]| shape_run(&face, cs, 0..cs.len(), true).into_iter().find(|g| g.cluster == 1).map(|g| g.gid);
        assert!(yeh(&with).is_some() && yeh(&with) != yeh(&without), "ZWNJ breaks the join");
        // In a stroke-font style the line draws glyph outlines from the fallback font, no boxes.
        let sh = crate::shape_stroke("قیمت 25 تومان", 1.0, 1.0, 0.0);
        assert!(sh.glyphs.len() >= 6, "{}", sh.glyphs.len());
        assert!(!sh.strokes.iter().any(|s| s.len() == 5 && s.first() == s.last()), "no missing-glyph boxes");
        // The ASCII digits are stroke glyphs in the middle of the line.
        let stroke_x: Vec<f64> = sh.strokes.iter().flatten().map(|p| p.x).collect();
        let (lo, hi) = (stroke_x.iter().copied().fold(f64::MAX, f64::min), stroke_x.iter().copied().fold(f64::MIN, f64::max));
        assert!(lo > 0.2 * sh.width && hi < 0.8 * sh.width, "digits between the words: {lo}..{hi} of {}", sh.width);
        assert!((crate::line_width("قیمت 25 تومان", 1.0, 1.0) - sh.width).abs() < 1e-9);
    }

    #[test]
    fn devanagari_run_is_reordered_with_line_clusters_and_advances_per_1000_em() {
        use skrifa::MetadataProvider;
        use skrifa::instance::{LocationRef, Size};
        let Some(face) = ttf::fallback_for('\u{0915}') else { return };
        let Some(ka) = nominal(&face, '\u{0915}') else { return };
        // "कि" after two other characters of the line: the vowel sign i is drawn before the
        // consonant (Indic reordering, which needs the script the shaper guesses from the text),
        // both glyphs carry the cluster of the consonant's index in the line, not in the run.
        let cs: Vec<char> = "ab\u{0915}\u{093F}".chars().collect();
        let gs = shape_run(&face, &cs, 2..4, false);
        let gids: Vec<u32> = gs.iter().map(|g| g.gid).collect();
        assert_eq!(gids.len(), 2, "{gids:?}");
        assert!(gids.first() != Some(&ka) && gids.last() == Some(&ka), "vowel sign before ka: {gids:?}");
        assert!(gs.iter().all(|g| g.cluster == 2), "{:?}", gs.iter().map(|g| g.cluster).collect::<Vec<_>>());
        // Advances come out at 1000 units per em, whatever the font's own em. A font that is
        // already at 1000 per em could not tell a missing scale apart, so it checks nothing here.
        let Some(font) = face.font() else { return };
        if font.metrics(Size::unscaled(), LocationRef::default()).units_per_em == 1000 {
            return;
        }
        let hmtx = font.glyph_metrics(Size::new(1000.0), LocationRef::default()).advance_width(skrifa::GlyphId::new(ka));
        let adv = gs.last().map(|g| g.advance);
        assert!(matches!((adv, hmtx), (Some(a), Some(h)) if (a - f64::from(h)).abs() < 1e-3), "{adv:?} vs {hmtx:?}");
    }

    #[test]
    fn thai_lines_are_shaped_and_latin_lines_are_not() {
        assert!(needed("ระดับดินเดิม") && needed("ขนาด 50x50 mm.") && needed("\u{0E4C}"));
        assert!(needed("abc سلام"));
        assert!(!needed("50x50 mm.") && !needed("Ünïcödé %%c25") && !needed(""));
    }

    /// The shaper splits sara am (ำ) into nikhahit and sara aa and draws the nikhahit before a
    /// tone mark: in drawing order the glyphs still show the typed text, each character on a
    /// glyph of its own, of its parts or before it (issue #2899). A glyph drawn before the
    /// character it stands for (a vowel sign before its consonant) keeps its own character: no
    /// character is moved onto another glyph to keep the order.
    #[test]
    fn reordered_glyphs_show_their_cluster_in_typed_order() {
        let (no, mai_tho, sara_am, nikhahit, sara_aa) = ('\u{0E19}', '\u{0E49}', '\u{0E33}', '\u{0E4D}', '\u{0E32}');
        let (deva_ka, deva_i, sara_u) = ('\u{0915}', '\u{093F}', '\u{0E38}');
        let face = mapped_face(&[(no, 1), (mai_tho, 2), (sara_am, 3), (nikhahit, 4), (sara_aa, 5), (deva_ka, 6), (deva_i, 7), (sara_u, 8)]);
        let with_gids = |gids: &[u32]| gids.iter().map(|gid| Glyph { gid: *gid, ..glyph(0) }).collect::<Vec<_>>();
        let cs = [no, mai_tho, sara_am];
        // What the shaper draws for น้ำ: no, nikhahit, mai tho, sara aa.
        let shows = glyph_chars(&face, &cs, &with_gids(&[1, 4, 2, 5]), cs.len());
        assert_eq!(shows.iter().flatten().collect::<String>(), "น้ำ");
        assert_eq!(shows.get(2), Some(&vec![mai_tho]), "the tone mark's glyph shows the tone mark: {shows:?}");
        // A sara u drawn into the letter's glyph goes with it, before the tone mark.
        let cs = [no, sara_u, mai_tho];
        let shows = glyph_chars(&face, &cs, &with_gids(&[1, 2]), cs.len());
        assert_eq!(shows, [vec![no, sara_u], vec![mai_tho]]);
        // Typed with the tone mark before sara u but drawn after it: in typed order the tone mark
        // would only follow no on its glyph, and output that writes a glyph's first character
        // would lose it, so each glyph keeps its own character.
        let cs = [no, mai_tho, sara_u];
        let shows = glyph_chars(&face, &cs, &with_gids(&[1, 8, 2]), cs.len());
        assert_eq!(shows, [vec![no], vec![sara_u], vec![mai_tho]]);
        // Devanagari ki: the vowel sign is drawn first.
        let cs = [deva_ka, deva_i];
        let shows = glyph_chars(&face, &cs, &with_gids(&[7, 6]), cs.len());
        assert_eq!(shows, [vec![deva_i], vec![deva_ka]]);
    }

    fn glyph(cluster: usize) -> Glyph {
        Glyph { gid: 0, cluster, advance: 500.0, dx: 0.0, dy: 0.0 }
    }

    /// Without installed fonts: a face that maps no character, so glyphs take their cluster's
    /// characters in order.
    #[test]
    fn glyphs_show_their_cluster_characters_once() {
        let face = empty_glyph_face();
        let cs: Vec<char> = "xabcd".chars().collect();
        // A run from index 1: cluster 1 has "ab" (two glyphs), cluster 3 has "cd" and a split
        // character's extra glyph, which shows nothing.
        let gs = Vec::from([1, 1, 3, 3, 3].map(glyph));
        assert_eq!(glyph_chars(&face, &cs, &gs, 5), vec![vec!['a'], vec!['b'], vec!['c'], vec!['d'], vec![]]);
        // A ligature (ฐ with sara u below as one glyph) shows every character of its cluster.
        let gs = Vec::from([1, 3].map(glyph));
        assert_eq!(glyph_chars(&face, &cs, &gs, 5), vec![vec!['a', 'b'], vec!['c', 'd']]);
        // Clusters out of order, past the run or past the line show nothing and don't panic.
        let gs = Vec::from([3, 1, usize::MAX, 9].map(glyph));
        assert_eq!(glyph_chars(&face, &cs, &gs, 5), vec![vec![], vec!['a', 'b', 'c', 'd'], vec![], vec![]]);
        assert!(glyph_chars(&face, &cs, &[], 5).is_empty());
        // A cluster longer than the matching limit still shows each character once, in order.
        let long: Vec<char> = std::iter::once('ก').chain(std::iter::repeat_n('\u{0E48}', MAX_CHARS)).collect();
        let gs: Vec<Glyph> = (0..long.len()).map(|_| glyph(0)).collect();
        let shows = glyph_chars(&face, &long, &gs, long.len());
        assert_eq!(shows.first(), Some(&vec!['ก']));
        assert!(shows.iter().all(|s| s.len() == 1));
    }

    /// An installed font with Thai, or `None` (the tests then skip).
    fn thai_face() -> Option<Face> {
        ttf::fallback_for('\u{0E01}').filter(|f| ['\u{0E14}', '\u{0E34}', '\u{0E4C}', '\u{0E33}', '\u{0E10}'].iter().all(|c| f.has_glyph(*c)))
    }

    /// A Thai face's letters are measured in ems: ก has neither ascender nor descender, ไ rises
    /// above it, and a space or an unmapped character has no height.
    #[test]
    fn thai_glyph_heights_are_measured_in_ems() {
        let Some(face) = thai_face() else { return };
        let ko = face.glyph_height('ก').unwrap_or_default();
        assert!((0.2..1.0).contains(&ko), "ก is {ko} em");
        assert!(face.glyph_height('ไ').is_some_and(|h| h > ko), "ไ is taller than ก");
        assert_eq!(face.glyph_height(' '), None);
        assert_eq!(face.glyph_height('\u{10fffd}'), None);
    }

    /// Lowest point of the glyph that shows `c`.
    fn bottom_of(sh: &Shaped, c: char) -> Option<f64> {
        let t = sh.text.iter().find(|t| t.ch == c)?;
        sh.glyphs.get(t.outline)?.iter().flatten().map(|p| p.y).reduce(f64::min)
    }

    #[test]
    fn thai_marks_stack_in_an_installed_font() {
        let Some(face) = thai_face() else { return };
        let shape = |s: &str| ttf::shape(&face, s, 1.0, 1.0, 0.0).expect("the face parses");
        // Thanthakhat over sara i (ดิ์) sits above the vowel, higher than over the bare consonant
        // (ด์): the font's mark-to-mark positioning. One glyph per character drew both marks at
        // the same height, on top of each other.
        let stacked = shape("ดิ์");
        let alone = shape("ด์");
        let (Some(over_vowel), Some(over_letter)) = (bottom_of(&stacked, '\u{0E4C}'), bottom_of(&alone, '\u{0E4C}')) else {
            panic!("thanthakhat drawn as text: {:?}", stacked.text.iter().map(|t| t.ch).collect::<String>())
        };
        assert!(over_vowel > over_letter + 0.05, "stacked {over_vowel} vs alone {over_letter}");
        // Every character is written as text exactly once, however the shaper reorders, splits
        // (sara am) or substitutes (ฐ without its tail under sara u) the glyphs.
        for s in ["ดิ์", "น้ำ", "ฐุ", "ปั้น", "จีรศักดิ์"] {
            let mut shown: Vec<char> = shape(s).text.iter().map(|t| t.ch).collect();
            let mut typed: Vec<char> = s.chars().collect();
            shown.sort_unstable();
            typed.sort_unstable();
            assert_eq!(shown, typed, "{s}");
        }
        // Measured and laid-out widths agree, in the face and in a stroke-font style.
        let s = "ทาสีขาว 50x50 mm.";
        assert!((ttf::width(&face, s, 1.0, 1.0).unwrap_or_default() - shape(s).width).abs() < 1e-9);
        assert!((crate::line_width(s, 1.0, 1.0) - crate::shape_stroke(s, 1.0, 1.0, 0.0).width).abs() < 1e-9);
        // In a stroke-font style Thai comes from the fallback font as text, without
        // missing-glyph boxes, and the Latin stays in the stroke font.
        let sh = crate::shape_stroke("ระดับดินเดิม +0.00", 1.0, 1.0, 0.0);
        assert!(!sh.strokes.iter().any(|s| s.len() == 5 && s.first() == s.last()), "no missing-glyph boxes");
        assert_eq!(sh.text.len(), "ระดับดินเดิม".chars().count());
        assert!(!sh.strokes.is_empty(), "\"+0.00\" in the stroke font");
    }

    #[test]
    fn hostile_thai_lines_never_panic() {
        for s in [
            "\u{0E33}\u{0E33}\u{0E33}",
            "\u{0E4C}\u{0E49}\u{0E34}",
            "ก\u{0E49}\u{0E49}\u{0E49}\u{0E33}\u{0E33}",
            "%%u\u{0E34}%%o",
            "\u{0E01}\u{200D}\u{0E33}\u{10FFFF}",
        ] {
            let _ = crate::shape_stroke(s, f64::NAN, -1.0, f64::INFINITY);
            let _ = crate::line_width(s, 0.0, 0.0);
            if let Some(face) = thai_face() {
                let _ = ttf::shape(&face, s, f64::NAN, 0.0, f64::NAN);
                let _ = ttf::width(&face, s, -1.0, f64::INFINITY);
            }
        }
        // One consonant under thousands of tone marks: a single huge cluster.
        let marks = format!("ก{}", "\u{0E48}".repeat(MAX_CHARS - 1));
        assert!(crate::shape_stroke(&marks, 1.0, 1.0, 0.0).width > 0.0);
        let long = "ก".repeat(MAX_CHARS + 1);
        assert!(shape(None, &long, 1.0, 1.0, 0.0, None).is_none());
        assert!(crate::shape_stroke(&long, 1.0, 1.0, 0.0).width > 0.0);
    }

    #[test]
    fn hostile_rtl_lines_never_panic() {
        for s in ["\u{202E}%%u\u{05D0}%%", "\u{2067}\u{0644}\u{0627}\u{2069}%%o", "\u{064E}\u{064E}\u{05D0}", "\u{05D0}\u{E000}\u{10FFFF}"] {
            let _ = crate::shape_stroke(s, f64::NAN, -1.0, f64::INFINITY);
            let _ = crate::line_width(s, 0.0, 0.0);
        }
        let long = "\u{05D0}b ".repeat(MAX_CHARS);
        assert!(shape(None, &long, 1.0, 1.0, 0.0, None).is_none());
        assert!(crate::shape_stroke(&long, 1.0, 1.0, 0.0).width > 0.0);
    }

    /// A TrueType font that maps `chars` to glyphs 1, 2, ... in code point order (each with the
    /// same advance) and nothing else: a character map and metrics, no outlines. Enough for
    /// `has_glyph` and the shaper, with a coverage the test chooses, whatever fonts are installed.
    fn font_mapping(chars: &[char]) -> Face {
        // Big-endian fields: (width in bytes, value).
        fn table(fields: &[(usize, i64)]) -> Vec<u8> {
            fields.iter().flat_map(|&(w, x)| x.to_be_bytes()[8 - w..].to_vec()).collect()
        }
        let mut chars = chars.to_vec();
        chars.sort_unstable();
        chars.dedup();
        let n = i64::try_from(chars.len() + 1).unwrap();
        // head: version 1.0, revision, checksum adjustment, magic, flags, units per em; created,
        // modified and bounds (zero); mac style, lowest ppem, direction hint, loca format, glyph
        // data format.
        let mut head = table(&[(2, 1), (2, 0), (4, 0x1_0000), (4, 0), (4, 0x5F0F_3CF5), (2, 0), (2, 1000)]);
        head.extend(table(&[(8, 0), (8, 0), (8, 0), (2, 0), (2, 8), (2, 2), (2, 0), (2, 0)]));
        // hhea: version 1.0, ascender, descender, line gap, max advance, min side bearings,
        // max extent, caret slope and offset, reserved, metric format, number of h metrics.
        let hhea =
            table(&[(2, 1), (2, 0), (2, 800), (2, -200), (2, 0), (2, 500), (2, 0), (2, 0), (2, 500), (2, 1), (2, 0), (2, 0), (8, 0), (2, 0), (2, n)]);
        let maxp = table(&[(4, 0x5000), (2, n)]);
        let hmtx = table(&[(2, 500), (2, 0)].repeat(chars.len() + 1));
        // cmap: one (3, 10) format 12 subtable, one group per character, in code point order.
        let groups: Vec<(usize, i64)> = chars
            .iter()
            .enumerate()
            .flat_map(|(i, c)| [(4, i64::from(u32::from(*c))), (4, i64::from(u32::from(*c))), (4, i64::try_from(i + 1).unwrap())])
            .collect();
        let count = i64::try_from(chars.len()).unwrap();
        let mut cmap = table(&[(2, 0), (2, 1), (2, 3), (2, 10), (4, 12), (2, 12), (2, 0), (4, 16 + 12 * count), (4, 0), (4, count)]);
        cmap.extend(table(&groups));
        let tables = [(b"cmap", cmap), (b"head", head), (b"hhea", hhea), (b"hmtx", hmtx), (b"maxp", maxp)];
        let mut font = table(&[(4, 0x1_0000), (2, 5), (2, 64), (2, 2), (2, 16)]);
        let mut offset = 12 + 16 * tables.len();
        for (tag, t) in &tables {
            font.extend_from_slice(*tag);
            font.extend(table(&[(4, 0), (4, i64::try_from(offset).unwrap()), (4, i64::try_from(t.len()).unwrap())]));
            offset += t.len().next_multiple_of(4);
        }
        for (_, t) in &tables {
            font.extend_from_slice(t);
            font.resize(font.len().next_multiple_of(4), 0);
        }
        Face { bytes: std::sync::Arc::new(font), index: 0 }
    }

    #[test]
    fn a_substitute_does_not_change_the_direction_of_the_line() {
        // ⌀ and ∅ are bidi-neutral; Ø, drawn for them in a font that lacks both, is
        // left-to-right. "⌀50 אב" reads right to left (its first strong character is Hebrew),
        // so "50⌀" sits at the right end of the line. A font without ⌀ lays the line out the
        // same way: the runs come from the text as decoded, not from the characters
        // substituted for it, which would make Ø the first strong character and flip the line.
        let s = "%%u\u{2300}50%%u \u{05D0}\u{05D1}";
        assert_eq!(bidi::base_rtl(&crate::decode_controls(s).iter().map(|c| c.0).collect::<String>()), Some(true));
        let shared: Vec<char> = "Ø50 \u{05D0}\u{05D1}".chars().collect();
        let has = font_mapping(&[&['\u{2300}'][..], &shared].concat());
        let lacks = font_mapping(&shared);
        assert!(has.has_glyph('\u{2300}') && !lacks.has_glyph('\u{2300}') && !lacks.has_glyph('\u{2205}') && lacks.has_glyph('Ø'));
        let with = shape(Some(&has), s, 1.0, 1.0, 0.0, None).unwrap();
        let without = shape(Some(&lacks), s, 1.0, 1.0, 0.0, None).unwrap();
        // The underline follows "⌀50" wherever the line puts it; nothing else is a stroke.
        let underline = |sh: &Shaped| {
            assert_eq!(sh.strokes.len(), 1, "{:?}", sh.strokes);
            (sh.strokes[0][0].x, sh.strokes[0][1].x)
        };
        let (x0, x1) = underline(&with);
        assert!(x0 > 0.3 * with.width && (x1 - with.width).abs() < 1e-9, "at the right end: {x0}..{x1} of {}", with.width);
        let (y0, y1) = underline(&without);
        assert!((y0 - x0).abs() < 1e-9 && (y1 - x1).abs() < 1e-9, "{y0}..{y1}, not {x0}..{x1}");
        assert!((without.width - with.width).abs() < 1e-9);
    }
}
