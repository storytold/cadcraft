mod animation_effects;
mod block_description_palette;
mod chrome_geometry;
mod clipboard_between_drawings;
mod context_menu;
mod context_menus;
mod control_cmdline;
mod control_cmdline_key;
mod control_methods;
mod dialog_dispatch;
mod dimension_menu;
mod dimstyle_dialog;
mod docking_workflows;
mod drafting_polar_increment;
mod drafting_settings_scroll;
mod drafting_spacing;
mod file_tab_titles;
mod gpu_viewport_stencil;
mod hidden_command_line;
mod instant_modals;
mod lisp_files;
mod measure_distance_snapping;
mod menubar_hover;
mod mspace_viewport_view;
mod named_viewports_dialog;
mod native_menu_text_keys;
mod native_windows;
mod options_dialog;
mod options_right_click;
mod parameters_rename;
mod pickdrag;
mod preview_zoom;
mod properties_described;
mod properties_units;
mod rapid_clicks;
mod selection_cycling;
mod shared_controls;
mod shortcut_menus;
mod shortcuts_checked;
mod status_bar_toggles;
mod style_dialogs;
mod system_cursor;
mod text_double_click;
mod toolsets_gear;
mod toolsets_parametric;
mod viewport_double_click_selection;
mod workspace_focus;
mod zoomed_far_in;

/// Plot Preview plots through a process-wide hook that `preview_zoom` records. The tests that open
/// the preview take this lock, so one never plots while the other counts.
pub(crate) static PLOT_PREVIEW: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// The process-wide io hooks, set once by whichever test runs first, so every test that sets them
/// sets these: a DXF writer stub for the clipboard text, and a plot that `preview_zoom` records.
pub(crate) fn shared_io() -> cadcraft_engine::cmd::file::IoHooks {
    cadcraft_engine::cmd::file::IoHooks {
        read: |_, _| Ok(cadcraft_engine::doc::Drawing::new_metric()),
        write: |_, _| Ok(b"0\nSECTION\n0\nENDSEC\n0\nEOF\n".to_vec()),
        plot: Some(preview_zoom::recording_plot),
    }
}
