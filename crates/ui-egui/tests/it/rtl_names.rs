//! A Hebrew file name reads as it does in the file manager: the letters right to left, the
//! extension on the right (`שלום.dwg`), whatever the text shaper does with right-to-left runs.

#[test]
fn hebrew_file_name_is_laid_out_in_reading_order() {
    let ctx = egui::Context::default();
    let mut out = ctx.run_ui(egui::RawInput::default(), |_| {});
    out.textures_delta.clear();
    let job = cadcraft_ui_egui::i18n::job("שלום.dwg", egui::FontId::proportional(14.0), egui::Color32::WHITE);
    let galley = ctx.fonts_mut(|f| f.layout_job(job));
    let mut glyphs: Vec<(f32, char)> = galley.rows.iter().flat_map(|r| r.glyphs.iter().map(|g| (g.pos.x, g.chr))).collect();
    glyphs.sort_by(|a, b| a.0.total_cmp(&b.0));
    let shown: String = glyphs.iter().map(|(_, c)| c).collect();
    // Left to right on screen: the last letter first, the extension last and not reversed.
    assert_eq!(shown, "םולש.dwg");
    ctx.tex_manager().write().take_delta().clear();
}
