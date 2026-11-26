//! Drawing text in the interface. egui's bundled faces and the fonts
//! [`crate::theme::install_fonts`] loads cover the interface's own languages, but the interface
//! also shows drawing text, which can be in any script the drawing uses: MTEXT contents in the
//! in-place editor, layer, layout and block names, a prompt that quotes a text. A character none
//! of those fonts has would be a box, while the canvas draws it from a fallback font.
//!
//! An egui font provider closes that gap: egui asks it once per family and character no
//! installed font has, and it answers with the face drawing text takes for that character
//! ([`cadcraft_fonts::ttf::fallback_for`]), so the interface shows what the canvas shows. The
//! provider hands egui the font table's own bytes: nothing is bundled, read again or copied.
//!
//! A fallback face is sized to the interface text, not to its own em: the Thai faces drawings use
//! (Cordia New, Angsana New) draw their letters small for their size, so their consonants are
//! scaled up to the interface's x-height ([`scale`]). Its glyphs sit on the interface text's
//! baseline, and a line with them grows to fit their stacked marks.

use std::sync::Arc;

use cadcraft_fonts::ttf::Face;

/// Let egui ask the drawing-text fallback for characters no interface font has. Once per
/// context: providers outlive the `set_fonts` of [`crate::theme::install_fonts`], so a second
/// call (a language change) adds nothing.
pub(crate) fn install(ctx: &egui::Context) {
    let id = egui::Id::unique("cadcraft::drawing_fonts");
    let installed = ctx.data_mut(|d| std::mem::replace(d.get_temp_mut_or_default::<bool>(id), true));
    if !installed {
        ctx.add_font_provider(Arc::new(|request: &egui::FallbackRequest<'_>| font_for(request.cluster, cadcraft_fonts::ttf::fallback_for)));
    }
}

/// How tall a lowercase x is in the interface faces, in ems (Ubuntu Light, egui's bundled face,
/// 0.52; Segoe UI 0.50). A Thai consonant without ascender or descender (ก) is about as tall in
/// the Thai faces made for screens (Leelawadee UI 0.51, Tahoma and Noto Sans Thai 0.56).
const X_HEIGHT: f64 = 0.52;
/// The most a fallback face is scaled up, so a face with an odd or tiny reference letter never
/// draws huge text. Cordia New needs 1.28, Angsana New 1.30.
const MAX_SCALE: f64 = 1.5;

/// The font for the first character of `cluster`: the face `fallback` finds for it, if any.
fn font_for(cluster: &str, fallback: impl Fn(char) -> Option<Face>) -> Option<egui::FontInsert> {
    let c = cluster.chars().next()?;
    fallback(c).map(|face| font_insert(&face, c))
}

/// The letter of `c`'s script that the interface's x-height measures: a Thai consonant without
/// ascender or descender, else the face's own Latin x.
fn reference_letter(c: char) -> char {
    if ('\u{0e00}'..='\u{0e7f}').contains(&c) { 'ก' } else { 'x' }
}

/// How much a face is scaled up whose reference letter is `height` ems tall: to [`X_HEIGHT`],
/// at most [`MAX_SCALE`]. A face that is already big enough, or whose letter has no height, keeps
/// its size.
fn scale(height: Option<f64>) -> f32 {
    height.filter(|h| h.is_finite() && *h > 0.0).map_or(1.0, |h| (X_HEIGHT / h).clamp(1.0, MAX_SCALE) as f32)
}

/// `face`, found for character `c`, as an egui font: the font table's bytes, shared rather than
/// copied, under a name that is the same for every character of the face, so egui loads the face
/// once (sized by the first character's script); scaled to the interface text and on its baseline.
fn font_insert(face: &Face, c: char) -> egui::FontInsert {
    let name = format!("drawing-text-{:p}-{}-{}", Arc::as_ptr(&face.bytes), face.bytes.len(), face.index);
    let tweak = egui::FontTweak {
        scale: scale(face.glyph_height(reference_letter(c))),
        fallback_align: egui::epaint::text::FallbackAlign::Baseline,
        ..Default::default()
    };
    egui::FontInsert::new(&name, egui::FontData::from_blob(face.bytes.clone(), face.index).tweak(tweak), Vec::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A face from one of egui's bundled fonts (there are no font files in this repository).
    fn bundled_face() -> Face {
        let defs = egui::FontDefinitions::default();
        let bytes = defs.font_data.get("Hack").map(|d| d.bytes().to_vec()).unwrap_or_default();
        Face::new(Arc::new(bytes), 0).expect("egui bundles Hack")
    }

    /// Every character of a face names the same font, whose bytes are the face's own (not a
    /// copy); a character without a fallback face, or no character, gets no font.
    #[test]
    fn a_fallback_face_is_one_shared_font() {
        let face = bundled_face();
        let fallback = |c: char| (c != '\u{e000}').then(|| face.clone());
        let a = font_for("ก", fallback).expect("a font");
        let b = font_for("ขั", fallback).expect("a font");
        assert_eq!(a.name, b.name);
        assert!(std::ptr::eq(a.data.bytes().as_ptr(), face.bytes.as_ptr()), "the font table's bytes, not a copy");
        assert_eq!(a.data.index, face.index);
        assert!(font_for("\u{e000}", fallback).is_none());
        assert!(font_for("", fallback).is_none());
        let other = Face::new(Arc::new(face.bytes.as_ref().clone()), 0).expect("a second copy parses");
        assert_ne!(font_insert(&other, 'ก').name, a.name, "another file is another font");
        assert_eq!(a.data.tweak.fallback_align, egui::epaint::text::FallbackAlign::Baseline, "on the interface text's baseline");
    }

    /// A face whose reference letter is short is scaled up to the interface's x-height, at most
    /// by half; one that is tall enough, or can't be measured, keeps its size.
    #[test]
    fn short_faces_are_scaled_to_the_interface_x_height() {
        assert_eq!(reference_letter('ค'), 'ก');
        assert_eq!(reference_letter('\u{0e4c}'), 'ก', "a Thai mark measures by the consonant too");
        assert_eq!(reference_letter('א'), 'x');
        // Cordia New's ก is 0.405 em.
        assert!((f64::from(scale(Some(0.405))) * 0.405 - X_HEIGHT).abs() < 1e-6);
        assert_eq!(scale(Some(0.56)), 1.0, "never scaled down");
        assert_eq!(scale(Some(0.01)), MAX_SCALE as f32);
        for unmeasured in [None, Some(0.0), Some(-1.0), Some(f64::NAN), Some(f64::INFINITY)] {
            assert_eq!(scale(unmeasured), 1.0, "{unmeasured:?}");
        }
    }

    /// Every Thai face found here (Cordia New, Angsana New on Windows; Noto Sans Thai from
    /// craft-fonts in CI) draws ก at least as tall as the interface's x-height, and is never shrunk.
    #[test]
    fn thai_faces_draw_their_consonants_at_the_interface_x_height() {
        let names = ["Cordia New", "Angsana New", "Browallia New", "Noto Sans Thai", "IBM Plex Sans Thai", "Leelawadee UI", "Tahoma"];
        let faces = names.iter().filter_map(|n| cadcraft_fonts::ttf::find(n)).chain(cadcraft_fonts::ttf::fallback_for('ก'));
        for face in faces.filter(|f| f.has_glyph('ก')) {
            let height = face.glyph_height('ก').unwrap_or_default();
            let drawn = height * f64::from(font_insert(&face, 'ก').data.tweak.scale);
            let family = face.family_name();
            assert!(drawn >= X_HEIGHT - 1e-6, "{family:?}: ก drawn {drawn} em");
            assert!(drawn <= height.max(X_HEIGHT) + 1e-6, "{family:?}: ก drawn {drawn} em, not shrunk or overshot");
        }
    }
}
