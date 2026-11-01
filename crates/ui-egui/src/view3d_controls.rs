//! Native controls for the current session's 3D projection and surface appearance.

use cadcraft_render::{Projection3d, VisualStyle3d};
use egui::{Align2, Color32, Id, pos2, vec2};
use serde_json::json;

use crate::CadApp;

pub(crate) fn show(app: &mut CadApp, ctx: &egui::Context) {
    // A layout tab draws in 2D even while the model tab keeps its camera.
    if !crate::canvas::showing_3d(app) {
        return;
    }
    if let Some(rect) = app.canvas.rect {
        egui::Area::new(Id::unique("3d-render-settings-toggle"))
            .fixed_pos(pos2(rect.left() + 8.0, rect.top() + 32.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                if ui.small_button("3D Settings").clicked() {
                    app.canvas.view3d_settings_open = !app.canvas.view3d_settings_open;
                }
            });
    }
    if !app.canvas.view3d_settings_open {
        return;
    }

    let mut open = app.canvas.view3d_settings_open;
    let mut options = app.session.render3d;
    let mut changed = false;
    egui::Window::new("3D View Settings")
        .id(Id::unique("3d-render-settings"))
        .anchor(Align2::RIGHT_TOP, vec2(-12.0, 44.0))
        .default_width(270.0)
        .open(&mut open)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Projection");
                craft_ui::ComboBox::from_id_salt("view3d-projection")
                    .selected_text(match options.projection {
                        Projection3d::Orthographic => "Orthographic",
                        Projection3d::Perspective => "Perspective",
                    })
                    .show_ui(ui, |ui| {
                        changed |= ui.selectable_value(&mut options.projection, Projection3d::Orthographic, "Orthographic").changed();
                        changed |= ui.selectable_value(&mut options.projection, Projection3d::Perspective, "Perspective").changed();
                    });
            });
            if options.projection == Projection3d::Perspective {
                changed |= ui.add(egui::Slider::new(&mut options.field_of_view, 10.0..=120.0).text("Field of view")).changed();
            }
            ui.horizontal(|ui| {
                ui.label("Style");
                craft_ui::ComboBox::from_id_salt("view3d-style")
                    .selected_text(match options.style {
                        VisualStyle3d::Shaded => "Shaded",
                        VisualStyle3d::Flat => "Flat",
                        VisualStyle3d::Realistic => "Realistic",
                        VisualStyle3d::Wireframe => "Wireframe",
                    })
                    .show_ui(ui, |ui| {
                        changed |= ui.selectable_value(&mut options.style, VisualStyle3d::Shaded, "Shaded").changed();
                        changed |= ui.selectable_value(&mut options.style, VisualStyle3d::Flat, "Flat").changed();
                        changed |= ui.selectable_value(&mut options.style, VisualStyle3d::Realistic, "Realistic").changed();
                        changed |= ui.selectable_value(&mut options.style, VisualStyle3d::Wireframe, "Wireframe").changed();
                    });
            });
            ui.separator();
            ui.label("Directional light");
            changed |= ui.add(egui::Slider::new(&mut options.light_direction.x, -1.0..=1.0).text("X")).changed();
            changed |= ui.add(egui::Slider::new(&mut options.light_direction.y, -1.0..=1.0).text("Y")).changed();
            changed |= ui.add(egui::Slider::new(&mut options.light_direction.z, -1.0..=1.0).text("Z")).changed();
            changed |= ui.add(egui::Slider::new(&mut options.light_intensity, 0.0..=4.0).text("Intensity")).changed();
            changed |= ui.add(egui::Slider::new(&mut options.ambient, 0.0..=1.0).text("Ambient")).changed();
            if options.style == VisualStyle3d::Realistic {
                changed |= ui.add(egui::Slider::new(&mut options.specular, 0.0..=1.0).text("Specular")).changed();
                changed |= ui.add(egui::Slider::new(&mut options.shininess, 1.0..=256.0).text("Shininess")).changed();
            }
            ui.separator();
            ui.horizontal(|ui| {
                ui.label("Material");
                if let Some(rgb) = options.material_color {
                    let mut color = Color32::from_rgb(rgb.0, rgb.1, rgb.2);
                    if egui::color_picker::color_edit_button_srgba(ui, &mut color, egui::color_picker::Alpha::Opaque).changed() {
                        options.material_color = Some(cadcraft_color::Rgb(color.r(), color.g(), color.b()));
                        changed = true;
                    }
                    if ui.small_button("By object").clicked() {
                        options.material_color = None;
                        changed = true;
                    }
                } else {
                    ui.label("Object colors");
                    let mut color = Color32::from_rgb(170, 190, 215);
                    if egui::color_picker::color_edit_button_srgba(ui, &mut color, egui::color_picker::Alpha::Opaque).changed() {
                        options.material_color = Some(cadcraft_color::Rgb(color.r(), color.g(), color.b()));
                        changed = true;
                    }
                }
            });
        });
    app.canvas.view3d_settings_open = open;
    if changed {
        let material = options.material_color.map(|c| [c.0, c.1, c.2]);
        let result = app.run("view3d", json!({
            "projection": match options.projection { Projection3d::Orthographic => "orthographic", Projection3d::Perspective => "perspective" },
            "fov": options.field_of_view,
            "style": match options.style { VisualStyle3d::Shaded => "shaded", VisualStyle3d::Flat => "flat", VisualStyle3d::Realistic => "realistic", VisualStyle3d::Wireframe => "wireframe" },
            "lightDirection": [options.light_direction.x, options.light_direction.y, options.light_direction.z],
            "lightIntensity": options.light_intensity,
            "ambient": options.ambient,
            "specular": options.specular,
            "shininess": options.shininess,
            "materialColor": material,
        }));
        if let Err(e) = result {
            app.session.echo(e);
        }
    }
}
