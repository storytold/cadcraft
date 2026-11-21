//! AutoLISP / VBA IDE and Application Loader (`VBAIDE`, `VLIDE`, `NETLOAD`).
//!
//! Provides an interactive multi-line LISP code editor, `.lsp` file loader (`NETLOAD` / `APPLOAD`),
//! `.lsp` file saver, one-click execution of `(defun c:... ())` commands defined in the script,
//! quick expression evaluator, and output console.

use egui::{RichText, vec2};
use serde_json::Value;

use crate::CadApp;
use crate::theme::Tokens;

const DEFAULT_LISP: &str = r#";; CADCraft AutoLISP / VBAIDE Studio
;; Click [▶ Run & Load] to load the functions below,
;; or open an external .lsp file via [📂 Open .lsp (NETLOAD)].

(defun add-numbers (a b)
  (+ a b)
)

;; Command C:HELLO -> type HELLO on the command line or click [▶ HELLO] below
(defun c:hello ()
  (princ "\nWelcome to CADCraft AutoLISP! (+ 1 2) = ")
  (princ (+ 1 2))
  (princ)
)

;; Command C:KADER -> draws a 100x50 rectangle at (0,0)
(defun c:kader ()
  (command "_.RECTANG" '(0 0) '(100 50))
  (princ "\nFrame 100x50 drawn.")
  (princ)
)
"#;

/// Interactive state for the AutoLISP / VBA IDE dialog (`VBAIDE`, `VLIDE`, `NETLOAD`).
#[derive(Clone, Debug)]
pub struct LispIdeState {
    /// Display name of the current `.lsp` routine.
    pub file_name: String,
    /// Full disk path when opened from or saved to a file.
    pub file_path: Option<String>,
    /// Current LISP source text in the editor buffer.
    pub code: String,
    /// Last editor text that was loaded into the LISP interpreter.
    pub loaded_code: Option<String>,
    /// Single-line expression in the quick evaluation input box.
    pub quick_expr: String,
    /// Console output lines produced by evaluations and loads.
    pub output: Vec<String>,
    /// Discovered `(defun c:NAME ...)` command names in uppercase.
    pub commands: Vec<String>,
    /// Most recent status summary shown at the bottom of the dialog.
    pub status: Option<String>,
}

impl Default for LispIdeState {
    fn default() -> Self {
        let code = DEFAULT_LISP.to_string();
        let commands = extract_c_commands(&code);
        Self {
            file_name: "routine.lsp".into(),
            file_path: None,
            code,
            loaded_code: None,
            quick_expr: "(+ 1 2)".into(),
            output: vec![crate::tl!("Ready. Click 'Run & Load' or open a .lsp file.").to_string()],
            commands,
            status: None,
        }
    }
}

/// Extract command names defined via `(defun c:name ...)` from LISP source text.
pub fn extract_c_commands(src: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in src.lines() {
        let code_part = line.split(';').next().unwrap_or("");
        let lower = code_part.to_ascii_lowercase();
        let mut rest = lower.as_str();
        while let Some(pos) = rest.find("(defun") {
            let after = rest[pos + "(defun".len()..].trim_start();
            rest = after;
            let Some(after_c) = after.strip_prefix("c:") else { continue };
            let name: String = after_c
                .chars()
                .take_while(|c| !c.is_whitespace() && *c != '(' && *c != ')')
                .collect();
            if !name.is_empty() && !out.iter().any(|x: &String| x.eq_ignore_ascii_case(&name)) {
                out.push(name.to_ascii_uppercase());
            }
        }
    }
    out
}

/// Route UI-level `vbaide`, `vlide`, `vlisp`, and `netload` commands when invoked without JSON parameters.
pub fn route(app: &mut CadApp, id: &str, params: &Value) -> Option<Result<Value, String>> {
    match id {
        "vbaide" | "vlide" | "vlisp" if params.is_null() => {
            if app.lisp_ide.is_none() {
                app.lisp_ide = Some(LispIdeState::default());
            }
            app.ui.dialog = Some("vbaide".into());
            Some(Ok(Value::Null))
        }
        "netload" if params.is_null() => {
            if app.lisp_ide.is_none() {
                app.lisp_ide = Some(LispIdeState::default());
            }
            app.ui.dialog = Some("vbaide".into());
            // Also open the native file picker immediately if available.
            open_file_into_ide(app);
            Some(Ok(Value::Null))
        }
        _ => None,
    }
}

/// Open the host file picker and load the chosen `.lsp` file into the IDE and session.
pub fn open_file_into_ide(app: &mut CadApp) {
    let picked = app.services.pick_open.as_ref().and_then(|f| f());
    let Some(path) = picked else { return };
    load_path_into_ide(app, &path);
}

/// Read an AutoLISP file from `path`, populate the IDE buffer, and evaluate it in `app.session`.
pub fn load_path_into_ide(app: &mut CadApp, path: &str) {
    match cadcraft_engine::lisp::builtins::read_text_file(path) {
        Some(text) => {
            let name = std::path::Path::new(path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| path.to_string());
            let cmds = extract_c_commands(&text);
            let ide = app.lisp_ide.get_or_insert_with(LispIdeState::default);
            ide.file_name = name.clone();
            ide.file_path = Some(path.to_string());
            ide.code = text.clone();
            ide.commands = cmds.clone();
            let log_before = app.session.log.len();
            match app.session.lisp_load(&name, &text, false) {
                Ok(res) => {
                    for line in app.session.log.get(log_before..).unwrap_or(&[]) {
                        ide.output.push(line.clone());
                    }
                    let msg = match res {
                        Some(Ok(v)) => {
                            ide.loaded_code = Some(text);
                            let printed = cadcraft_engine::lisp::value::to_text(&v, true);
                            if printed != "nil" {
                                ide.output.push(format!("⇒ {printed}"));
                            }
                            if cmds.is_empty() {
                                format!("{}: {name}", crate::tl!("Loaded"))
                            } else {
                                format!("{}: {name} ({}: {})", crate::tl!("Loaded"), crate::tl!("Commands"), cmds.join(", "))
                            }
                        }
                        Some(Err(e)) => {
                            ide.loaded_code = None;
                            format!("{}: {e}", crate::tl!("Error"))
                        }
                        None => {
                            ide.loaded_code = Some(text);
                            if cmds.is_empty() {
                                format!("{}: {name}", crate::tl!("Loaded"))
                            } else {
                                format!("{}: {name} ({}: {})", crate::tl!("Loaded"), crate::tl!("Commands"), cmds.join(", "))
                            }
                        }
                    };
                    ide.output.push(msg.clone());
                    ide.status = Some(msg.clone());
                    app.session.echo(msg);
                }
                Err(e) => {
                    ide.loaded_code = None;
                    let msg = format!("{} {name}: {e}", crate::tl!("Error loading"));
                    ide.output.push(msg.clone());
                    ide.status = Some(msg.clone());
                    app.session.echo(msg);
                }
            }
        }
        None => {
            let msg = format!("NETLOAD: {}: {path}", crate::tl!("cannot read file"));
            if let Some(ide) = &mut app.lisp_ide {
                ide.output.push(msg.clone());
                ide.status = Some(msg.clone());
            }
            app.session.echo(msg);
        }
    }
}

/// Evaluate the current LISP editor buffer in `app.session`, update discovered `c:` commands,
/// and return `true` when the buffer loaded without errors.
fn execute_current_code(app: &mut CadApp) -> bool {
    let Some(ide) = app.lisp_ide.as_ref() else { return false };
    let name = ide.file_name.clone();
    let code = ide.code.clone();
    let cmds = extract_c_commands(&code);
    let log_before = app.session.log.len();
    let res = app.session.lisp_load(&name, &code, false);
    let new_logs: Vec<String> = app.session.log.get(log_before..).map(|s| s.to_vec()).unwrap_or_default();
    let Some(ide) = app.lisp_ide.as_mut() else { return false };
    ide.commands = cmds.clone();
    for l in new_logs {
        ide.output.push(l);
    }
    match res {
        Ok(Some(Ok(v))) => {
            ide.loaded_code = Some(code);
            let txt = cadcraft_engine::lisp::value::to_text(&v, true);
            ide.output.push(format!("⇒ {txt}"));
            let msg = if cmds.is_empty() {
                format!("{} ({name}) ⇒ {txt}", crate::tl!("Executed"))
            } else {
                format!("{} ({name}) — {}: {}", crate::tl!("Loaded"), crate::tl!("Available commands"), cmds.join(", "))
            };
            ide.status = Some(msg);
            true
        }
        Ok(Some(Err(e))) => {
            ide.loaded_code = None;
            let err_msg = format!("{}: {e}", crate::tl!("Error"));
            ide.output.push(err_msg.clone());
            ide.status = Some(err_msg);
            false
        }
        Ok(None) => {
            ide.loaded_code = Some(code);
            ide.status = Some(format!("{} ({name})...", crate::tl!("Routine started")));
            true
        }
        Err(e) => {
            ide.loaded_code = None;
            let err_msg = format!("{}: {e}", crate::tl!("Error"));
            ide.output.push(err_msg.clone());
            ide.status = Some(err_msg);
            false
        }
    }
}

/// Evaluate the single-line quick expression (`(+ 1 2)` or bare `+ 1 2`) and append its result to the console.
fn eval_quick_expr(app: &mut CadApp) {
    let Some(ide) = app.lisp_ide.as_ref() else { return };
    let raw = ide.quick_expr.trim().to_string();
    if raw.is_empty() {
        return;
    }
    let expr = if cadcraft_engine::is_bare_lisp_prefix(&raw) {
        format!("({raw})")
    } else {
        raw.clone()
    };
    let log_before = app.session.log.len();
    let res = app.session.lisp_eval(&expr, true, false, Vec::new());
    let new_logs: Vec<String> = app.session.log.get(log_before..).map(|s| s.to_vec()).unwrap_or_default();
    let Some(ide) = app.lisp_ide.as_mut() else { return };
    ide.output.push(format!("> {raw}"));
    for l in new_logs {
        ide.output.push(format!("  {l}"));
    }
    match res {
        Ok(Some(Err(e))) => {
            ide.output.push(format!("  {}: {e}", crate::tl!("Error")));
        }
        Err(e) => {
            ide.output.push(format!("  {}: {e}", crate::tl!("Error")));
        }
        _ => {}
    }
}

/// Render the AutoLISP / VBA IDE & NETLOAD Studio window.
pub fn dialog(app: &mut CadApp, ctx: &egui::Context, open: &mut bool) {
    if app.lisp_ide.is_none() {
        app.lisp_ide = Some(LispIdeState::default());
    }
    let tok = Tokens::get();
    let mut do_open_file = false;
    let mut do_save_file = false;
    let mut do_run_code = false;
    let mut do_eval_quick = false;
    let mut run_cmd: Option<String> = None;

    egui::Window::new(crate::tl!("AutoLISP / VBA IDE & NETLOAD Studio"))
        .id(egui::Id::unique("vbaide_window"))
        .open(open)
        .default_size(vec2(760.0, 560.0))
        .min_size(vec2(520.0, 380.0))
        .resizable(true)
        .show(ctx, |ui| {
            let Some(ide) = app.lisp_ide.as_mut() else { return };

            // Top toolbar
            ui.horizontal_wrapped(|ui| {
                if ui
                    .button(RichText::new(format!("📂 {}", crate::tl!("Open .lsp (NETLOAD)"))).strong())
                    .on_hover_text(crate::tl!("Open and load an AutoLISP (.lsp) file from disk"))
                    .clicked()
                {
                    do_open_file = true;
                }
                if ui
                    .button(format!("💾 {}", crate::tl!("Save .lsp")))
                    .on_hover_text(crate::tl!("Save the current LISP routine as a .lsp file"))
                    .clicked()
                {
                    do_save_file = true;
                }
                if ui
                    .button(RichText::new(format!("▶ {}", crate::tl!("Run & Load"))).strong().color(egui::Color32::from_rgb(0x4e, 0xc9, 0x6f)))
                    .on_hover_text(crate::tl!("Evaluate the LISP code in the editor and register all (defun c:...) commands"))
                    .clicked()
                {
                    do_run_code = true;
                }

                ui.separator();
                ui.label(format!("{}:", crate::tl!("Template")));
                craft_ui::ComboBox::from_id_salt("lisp_template_combo")
                    .selected_text(crate::tl!("Choose template..."))
                    .show_ui(ui, |ui| {
                        if ui.selectable_label(false, crate::tl!("1. Basic Math & Commands")).clicked() {
                            ide.code = DEFAULT_LISP.to_string();
                            ide.loaded_code = None;
                            ide.commands = extract_c_commands(&ide.code);
                        }
                        if ui.selectable_label(false, crate::tl!("2. Draw Crosshair Point (c:meetpunt)")).clicked() {
                            ide.code = r#";; Draws a circle with crosshairs at a picked point
(defun c:meetpunt (/ pt)
  (setq pt (getpoint "\nSpecify survey point: "))
  (if pt
    (progn
      (command "_.CIRCLE" pt 1.5)
      (command "_.LINE" (list (- (car pt) 2.0) (cadr pt)) (list (+ (car pt) 2.0) (cadr pt)) "")
      (command "_.LINE" (list (car pt) (- (cadr pt) 2.0)) (list (car pt) (+ (cadr pt) 2.0)) "")
      (princ "\nSurvey point placed!")
    )
  )
  (princ)
)
"#
                            .to_string();
                            ide.loaded_code = None;
                            ide.commands = extract_c_commands(&ide.code);
                        }
                        if ui.selectable_label(false, crate::tl!("3. Inspect Last Entity DXF (c:objinfo)")).clicked() {
                            ide.code = r#";; Prints the DXF group codes of the last drawn entity
(defun c:objinfo (/ ent data)
  (setq ent (entlast))
  (if ent
    (progn
      (setq data (entget ent))
      (princ "\nLast entity DXF data: ")
      (princ data)
    )
    (princ "\nNo objects in drawing.")
  )
  (princ)
)
"#
                            .to_string();
                            ide.loaded_code = None;
                            ide.commands = extract_c_commands(&ide.code);
                        }
                    });

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new(&ide.file_name).monospace().color(tok.text_dim));
                });
            });

            // Detected `c:` commands bar
            if !ide.commands.is_empty() {
                ui.separator();
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(format!("{}:", crate::tl!("Loaded Commands (click to run)"))).strong());
                    for cmd in &ide.commands {
                        if ui
                            .button(RichText::new(format!("▶ {cmd}")).monospace().strong())
                            .on_hover_text(format!("{}: {cmd}", crate::tl!("Run command in drawing")))
                            .clicked()
                        {
                            run_cmd = Some(cmd.clone());
                        }
                    }
                });
            }

            ui.separator();

            // Code editor
            ui.label(RichText::new(format!("{}:", crate::tl!("AutoLISP Editor (.lsp)"))).strong());
            egui::ScrollArea::vertical()
                .id_salt("lisp_ide_editor_scroll")
                .max_height(280.0)
                .show(ui, |ui| {
                    let te = egui::TextEdit::multiline(&mut ide.code)
                        .font(egui::TextStyle::Monospace)
                        .code_editor()
                        .desired_width(f32::INFINITY)
                        .desired_rows(14);
                    if ui.add(te).changed() {
                        ide.commands = extract_c_commands(&ide.code);
                    }
                });

            ui.add_space(4.0);

            // Quick expression bar (`(+ 1 2)` or `+ 1 2`)
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("{}:", crate::tl!("Quick Expression"))).strong());
                let resp = ui.add(
                    egui::TextEdit::singleline(&mut ide.quick_expr)
                        .font(egui::TextStyle::Monospace)
                        .desired_width(340.0)
                        .hint_text(crate::tl!("e.g. (+ 1 2) or + 1 2")),
                );
                if (resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                    || ui.button(crate::tl!("Evaluate")).clicked()
                {
                    do_eval_quick = true;
                }
                if ui.button(crate::tl!("Clear Log")).clicked() {
                    ide.output.clear();
                }
            });

            ui.separator();

            // Output console
            ui.label(RichText::new(format!("{}:", crate::tl!("Console / Output"))).strong());
            egui::Frame::NONE
                .fill(tok.chrome_dark)
                .inner_margin(egui::Margin::same(6))
                .corner_radius(4.0)
                .show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("lisp_ide_output_scroll")
                        .stick_to_bottom(true)
                        .max_height(130.0)
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            for line in &ide.output {
                                ui.label(RichText::new(line).monospace().size(12.0));
                            }
                        });
                });

            if let Some(st) = &ide.status {
                ui.add_space(2.0);
                ui.label(RichText::new(st).color(tok.text_dim).size(11.5));
            }
        });

    handle_dialog_actions(app, do_open_file, do_save_file, do_run_code, do_eval_quick, run_cmd);
}

fn handle_dialog_actions(
    app: &mut CadApp,
    do_open_file: bool,
    do_save_file: bool,
    do_run_code: bool,
    do_eval_quick: bool,
    run_cmd: Option<String>,
) {
    if do_open_file {
        open_file_into_ide(app);
    }
    if do_save_file {
        let suggested = app
            .lisp_ide
            .as_ref()
            .map(|i| i.file_name.clone())
            .unwrap_or_else(|| "routine.lsp".into());
        if let Some(path) = app.services.pick_save.as_ref().and_then(|f| f(&suggested))
            && let Some(ide) = app.lisp_ide.as_mut()
        {
            match std::fs::write(&path, &ide.code) {
                Ok(()) => {
                    ide.file_path = Some(path.clone());
                    if let Some(name) = std::path::Path::new(&path).file_name() {
                        ide.file_name = name.to_string_lossy().to_string();
                    }
                    let msg = format!("{}: {path}", crate::tl!("Saved to"));
                    ide.output.push(msg.clone());
                    ide.status = Some(msg);
                }
                Err(e) => {
                    let msg = format!("{} {path}: {e}", crate::tl!("Failed to save to"));
                    ide.output.push(msg.clone());
                    ide.status = Some(msg);
                }
            }
        }
    }
    if do_run_code {
        let _ = execute_current_code(app);
    }
    if do_eval_quick {
        eval_quick_expr(app);
    }
    if let Some(cmd) = run_cmd {
        let needs_load = app
            .lisp_ide
            .as_ref()
            .is_some_and(|i| i.loaded_code.as_deref() != Some(i.code.as_str()));
        let ok = if needs_load { execute_current_code(app) } else { true };
        if ok && !app.session.prompting() {
            app.cmdline(&cmd);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_c_commands_from_lisp_source() {
        let cmds = extract_c_commands(DEFAULT_LISP);
        assert_eq!(cmds, vec!["HELLO".to_string(), "KADER".to_string()]);
    }

    #[test]
    fn extract_c_commands_ignores_non_defun_colon_tokens() {
        let src = r#"
            (defun misc:helper () 42)
            (setq doc:x 1)
            (princ "c:foo")
            (defun   c:valid_cmd () (princ))
        "#;
        assert_eq!(extract_c_commands(src), vec!["VALID_CMD".to_string()]);
    }
}
