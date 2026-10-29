//! AutoLISP / VB.NET / C# IDE and Application Loader (`VBAIDE`, `VLIDE`, `NETLOAD`).
//!
//! Provides an interactive multi-line code editor (`.lsp`, `.vb`, `.cs`), `.dll` and `.lsp`
//! application loader (`NETLOAD` / `APPLOAD`), `.NET` `[CommandMethod]` discovery and 1-click
//! execution via `cadcraft-nethost.exe`, live VB.NET / C# compiler (`vbc.exe` / `csc.exe`),
//! quick LISP expression evaluator, and output console.

use std::sync::atomic::{AtomicU16, Ordering};

use egui::{RichText, vec2};
use serde_json::Value;

use crate::CadApp;
use crate::theme::Tokens;

/// Loopback TCP port of the active `control_server` in `cadcraft`, used by `cadcraft-nethost.exe`
/// to communicate with the live drawing via `dotnet.bridge`.
pub static ACTIVE_CONTROL_PORT: AtomicU16 = AtomicU16::new(0);

const DEFAULT_LISP: &str = r#";; CADCraft AutoLISP / VBAIDE Studio
;; Click [Load & Run LISP] to load the functions below,
;; or open an external .dll / .lsp file via [Load .DLL (NETLOAD)].

(defun add-numbers (a b)
  (+ a b)
)

;; Command C:HELLO -> type HELLO on the command line or click [HELLO] below
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

const DEFAULT_VBNET: &str = r#"' CADCraft VB.NET Plugin Template (.vb -> .dll)
' Click [Compile & Load .NET DLL] to compile and load directly into CADCraft!
Imports System
Imports System.Windows.Forms
Imports Autodesk.AutoCAD.Runtime
Imports AcadApp = Autodesk.AutoCAD.ApplicationServices.Application

Public Module MyCadPlugin
    <CommandMethod("VBHELLO")>
    Public Sub VbHello()
        Dim doc As Object = AcadApp.DocumentManager.MdiActiveDocument.AcadDocument
        ' Draw a closed LWPOLYLINE rectangle via the ActiveX / COM model:
        Dim pts As Double() = New Double() {0.0, 0.0, 50.0, 0.0, 50.0, 30.0, 0.0, 30.0}
        Dim pl As Object = doc.ModelSpace.AddLightWeightPolyline(pts)
        pl.Closed = True
        MessageBox.Show("VB.NET plugin executed! Polyline created in drawing.", "CADCraft .NET")
    End Sub
End Module
"#;

const DEFAULT_CSHARP: &str = r#"// CADCraft C# Plugin Template (.cs -> .dll)
// Click [Compile & Load .NET DLL] to compile and load directly into CADCraft!
using System;
using System.Windows.Forms;
using Autodesk.AutoCAD.Runtime;
using AcadApp = Autodesk.AutoCAD.ApplicationServices.Application;

public static class MyCSharpPlugin
{
    [CommandMethod("CSHELLO")]
    public static void CsHello()
    {
        dynamic doc = AcadApp.DocumentManager.MdiActiveDocument.AcadDocument;
        double[] startPt = new double[] { 0.0, 0.0, 0.0 };
        double[] endPt = new double[] { 80.0, 40.0, 0.0 };
        doc.ModelSpace.AddLine(startPt, endPt);
        MessageBox.Show("C# plugin executed! Line (0,0)-(80,40) drawn.", "CADCraft .NET");
    }
}
"#;

/// Source language active in the IDE editor buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdeLang {
    /// AutoLISP (`.lsp`) interpreted directly by the engine.
    Lisp,
    /// Visual Basic .NET (`.vb`) compiled to a `.dll` via `vbc.exe`.
    VbNet,
    /// C# (`.cs`) compiled to a `.dll` via `csc.exe`.
    CSharp,
}

/// Metadata for a loaded `.NET` plugin assembly (`.dll`) and its discovered `[CommandMethod]` names.
#[derive(Clone, Debug)]
pub struct LoadedDll {
    /// Full file system path to the `.dll` assembly.
    pub path: String,
    /// Display file name of the `.dll`.
    pub name: String,
    /// Discovered `[CommandMethod("...")]` command names in uppercase.
    pub commands: Vec<String>,
}

/// Interactive state for the AutoLISP / VB.NET / C# IDE dialog (`VBAIDE`, `VLIDE`, `NETLOAD`).
#[derive(Clone, Debug)]
pub struct LispIdeState {
    /// Active source language in the editor.
    pub lang: IdeLang,
    /// Display name of the current `.lsp`, `.vb`, or `.cs` file.
    pub file_name: String,
    /// Full disk path when opened from or saved to a file.
    pub file_path: Option<String>,
    /// Current source text in the editor buffer.
    pub code: String,
    /// Last editor text that was loaded into the LISP interpreter.
    pub loaded_code: Option<String>,
    /// Single-line expression in the quick evaluation input box.
    pub quick_expr: String,
    /// Console output lines produced by evaluations, compilations, and loads.
    pub output: Vec<String>,
    /// Discovered `(defun c:NAME ...)` command names in uppercase.
    pub commands: Vec<String>,
    /// Loaded `.NET` DLL assemblies and their discovered commands.
    pub loaded_dlls: Vec<LoadedDll>,
    /// Whether the compact floating plugin command menu is shown when the Studio is closed.
    pub show_plugin_menu: bool,
    /// Most recent status summary shown at the bottom of the dialog.
    pub status: Option<String>,
    /// Index of the active drawing document for which `startup_plugins` were last ensured.
    pub startup_synced_doc: Option<usize>,
    /// Snapshot of `app.ui.startup_plugins` when `startup_synced_doc` was last synced.
    pub startup_synced_list: Vec<String>,
}

impl Default for LispIdeState {
    fn default() -> Self {
        Self {
            lang: IdeLang::Lisp,
            file_name: "Nieuw.lsp".into(),
            file_path: None,
            code: String::new(),
            loaded_code: None,
            quick_expr: String::new(),
            output: vec![
                crate::tl!("Ready. Open a .dll (.NET VB.NET/C#) or .lsp (AutoLISP) file via NETLOAD.").to_string(),
            ],
            commands: Vec::new(),
            loaded_dlls: Vec::new(),
            show_plugin_menu: true,
            status: None,
            startup_synced_doc: None,
            startup_synced_list: Vec::new(),
        }
    }
}

/// Whether `path` is a `.dll`, `.vb`, or `.cs` plugin file handled by the NETLOAD/VBAIDE Studio.
pub fn is_plugin_file(path: &str) -> bool {
    let lower = path.trim().to_ascii_lowercase();
    lower.ends_with(".dll") || lower.ends_with(".vb") || lower.ends_with(".cs")
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

/// Locate `cadcraft-nethost.exe` via `CADCRAFT_NETHOST`, next to the running executable, or in the working directory.
fn find_nethost_exe() -> Option<std::path::PathBuf> {
    if let Ok(custom) = std::env::var("CADCRAFT_NETHOST") {
        let p = std::path::PathBuf::from(custom.trim());
        if p.exists() {
            return Some(p);
        }
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        let cand = dir.join("cadcraft-nethost.exe");
        if cand.exists() {
            return Some(cand);
        }
    }
    let cwd_cand = std::path::PathBuf::from("cadcraft-nethost.exe");
    if cwd_cand.exists() {
        return Some(cwd_cand);
    }
    None
}

/// Try to run `cmd` if it matches a `[CommandMethod]` in any loaded `.NET` DLL.
pub fn try_run_dotnet_command(app: &mut CadApp, cmd: &str) -> bool {
    let clean = cmd.trim().trim_start_matches(['_', '.']).to_ascii_uppercase();
    if clean.is_empty() {
        return false;
    }
    let dll_path = app.lisp_ide.as_ref().and_then(|ide| {
        ide.loaded_dlls
            .iter()
            .find(|d| d.commands.iter().any(|c| c.eq_ignore_ascii_case(&clean)))
            .map(|d| d.path.clone())
    });
    let Some(dll_path) = dll_path else {
        return false;
    };
    spawn_dotnet_command(app, &dll_path, &clean);
    true
}

/// Create a [`std::process::Command`] for `cadcraft-nethost.exe` with `CREATE_NO_WINDOW` set on Windows
/// so no console window flashes on screen.
fn nethost_command(host: &std::path::Path) -> std::process::Command {
    #[allow(unused_mut)]
    let mut cmd = std::process::Command::new(host);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// Spawn `cadcraft-nethost.exe` to execute `[CommandMethod]` `cmd` inside `dll_path`.
pub fn spawn_dotnet_command(app: &mut CadApp, dll_path: &str, cmd: &str) {
    let Some(host) = find_nethost_exe() else {
        let msg = crate::tl!("Error: cadcraft-nethost.exe not found next to the application.").to_string();
        if let Some(ide) = &mut app.lisp_ide {
            ide.output.push(msg.clone());
            ide.status = Some(msg.clone());
        }
        app.session.echo(msg);
        return;
    };
    app.ensure_control();
    let port = ACTIVE_CONTROL_PORT.load(Ordering::SeqCst);
    let mut command = nethost_command(&host);
    command.arg("run").arg(dll_path).arg(cmd).arg(port.to_string());
    if let Some(dir) = std::path::Path::new(dll_path).parent() {
        command.current_dir(dir);
    }
    match command.spawn() {
        Ok(_) => {
            let msg = format!("▶ {}: {cmd} ({dll_path})", crate::tl!(".NET command started"));
            if let Some(ide) = &mut app.lisp_ide {
                ide.output.push(msg.clone());
                ide.status = Some(msg.clone());
            }
            app.session.echo(msg);
        }
        Err(e) => {
            let msg = format!("{} {cmd}: {e}", crate::tl!("Failed to start .NET command"));
            if let Some(ide) = &mut app.lisp_ide {
                ide.output.push(msg.clone());
                ide.status = Some(msg.clone());
            }
            app.session.echo(msg);
        }
    }
}

/// Inspect a `.NET` assembly (`.dll`) via `cadcraft-nethost.exe` and register its `[CommandMethod]` commands.
pub fn load_dll_into_ide(app: &mut CadApp, path: &str) {
    let name = std::path::Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string());
    let Some(host) = find_nethost_exe() else {
        let msg = format!("NETLOAD: {} ({name})", crate::tl!("cadcraft-nethost.exe not found"));
        let ide = app.lisp_ide.get_or_insert_with(LispIdeState::default);
        ide.output.push(msg.clone());
        ide.status = Some(msg.clone());
        app.session.echo(msg);
        return;
    };
    let out = nethost_command(&host)
        .arg("inspect")
        .arg(path)
        .output();
    let ide = app.lisp_ide.get_or_insert_with(LispIdeState::default);
    match out {
        Ok(res) => {
            let stdout = String::from_utf8_lossy(&res.stdout).trim().to_string();
            let stderr = String::from_utf8_lossy(&res.stderr).trim().to_string();
            let parsed: Option<Value> = serde_json::from_str(&stdout).ok();
            if let Some(ref v) = parsed
                && v.get("ok").and_then(Value::as_bool) == Some(true)
            {
                let cmds: Vec<String> = v
                    .get("commands")
                    .and_then(Value::as_array)
                    .map(|a| {
                        a.iter()
                            .filter_map(|x| x.as_str().map(|s| s.to_ascii_uppercase()))
                            .collect()
                    })
                    .unwrap_or_default();
                ide.loaded_dlls.retain(|d| !d.path.eq_ignore_ascii_case(path));
                ide.loaded_dlls.push(LoadedDll {
                    path: path.to_string(),
                    name: name.clone(),
                    commands: cmds.clone(),
                });
                ide.show_plugin_menu = true;
                let msg = if cmds.is_empty() {
                    format!(
                        "{}: {name} ({})",
                        crate::tl!("NETLOAD .NET DLL loaded"),
                        crate::tl!("no [CommandMethod] found")
                    )
                } else {
                    format!(
                        "{}: {name} — {}: {}",
                        crate::tl!("NETLOAD .NET DLL loaded"),
                        crate::tl!("Commands"),
                        cmds.join(", ")
                    )
                };
                ide.output.push(msg.clone());
                ide.status = Some(msg.clone());
                for cmd in &cmds {
                    app.cmd.push_history(cmd);
                }
                app.session.echo(msg);
                if !cmds.is_empty() && app.ui.dialog.as_deref() == Some("vbaide") {
                    app.ui.dialog = None;
                }
            } else {
                let err_detail = parsed
                    .as_ref()
                    .and_then(|v| v.get("error"))
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .unwrap_or_else(|| {
                        if !stderr.is_empty() {
                            stderr
                        } else {
                            stdout
                        }
                    });
                let msg = format!("{} {name}: {err_detail}", crate::tl!("NETLOAD error in"));
                ide.output.push(msg.clone());
                ide.status = Some(msg.clone());
                app.session.echo(msg);
            }
        }
        Err(e) => {
            let msg = format!("{}: {e}", crate::tl!("NETLOAD failed to run cadcraft-nethost.exe"));
            ide.output.push(msg.clone());
            ide.status = Some(msg.clone());
            app.session.echo(msg);
        }
    }
}

/// Compile the current VB.NET or C# source buffer into a `.dll` using `cadcraft-nethost.exe` and load it.
fn compile_and_load_dotnet_source(app: &mut CadApp) {
    let Some(ide) = app.lisp_ide.as_ref() else { return };
    let ext = match ide.lang {
        IdeLang::VbNet => "vb",
        IdeLang::CSharp => "cs",
        IdeLang::Lisp => return,
    };
    let code = ide.code.clone();
    let stem: String = std::path::Path::new(&ide.file_name)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "CustomPlugin".into())
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .collect();
    let stem = if stem.is_empty() { "CustomPlugin" } else { &stem };
    let temp_dir = std::env::temp_dir().join(format!("cadcraft_dotnet_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&temp_dir);
    let src_path = temp_dir.join(format!("{stem}.{ext}"));
    let dll_path = temp_dir.join(format!("{stem}.dll"));
    if let Err(e) = std::fs::write(&src_path, &code) {
        if let Some(ide) = &mut app.lisp_ide {
            let msg = format!("{}: {e}", crate::tl!("Failed to write source file"));
            ide.output.push(msg.clone());
            ide.status = Some(msg);
        }
        return;
    }
    let Some(host) = find_nethost_exe() else {
        if let Some(ide) = &mut app.lisp_ide {
            let msg = crate::tl!("Error: cadcraft-nethost.exe not found next to the application.").to_string();
            ide.output.push(msg.clone());
            ide.status = Some(msg);
        }
        return;
    };
    let out = nethost_command(&host)
        .arg("compile")
        .arg(&src_path)
        .arg(&dll_path)
        .output();
    match out {
        Ok(res) => {
            let stdout = String::from_utf8_lossy(&res.stdout).trim().to_string();
            let parsed: Option<Value> = serde_json::from_str(&stdout).ok();
            if let Some(v) = &parsed
                && v.get("ok").and_then(Value::as_bool) == Some(true)
            {
                let dll_str = dll_path.to_string_lossy().into_owned();
                if let Some(ide) = &mut app.lisp_ide {
                    ide.output.push(format!("{}: {dll_str}", crate::tl!("Compilation succeeded")));
                }
                load_dll_into_ide(app, &dll_str);
            } else {
                let err_msg = parsed
                    .as_ref()
                    .and_then(|v| v.get("error"))
                    .and_then(Value::as_str)
                    .unwrap_or(&stdout)
                    .to_string();
                if let Some(ide) = &mut app.lisp_ide {
                    ide.output.push(format!("{}:\n{err_msg}", crate::tl!("Compilation error")));
                    ide.status = Some(crate::tl!("Compilation failed (see Console)").to_string());
                }
            }
        }
        Err(e) => {
            if let Some(ide) = &mut app.lisp_ide {
                let msg = format!("{}: {e}", crate::tl!("Failed to invoke compiler"));
                ide.output.push(msg.clone());
                ide.status = Some(msg);
            }
        }
    }
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
            open_file_into_ide(app);
            Some(Ok(Value::Null))
        }
        _ => None,
    }
}

/// Open the host file picker and load the chosen `.dll`, `.lsp`, `.vb`, or `.cs` file into the IDE.
pub fn open_file_into_ide(app: &mut CadApp) {
    let picked = app.services.pick_open.as_ref().and_then(|f| f());
    let Some(path) = picked else { return };
    load_path_into_ide(app, &path);
}

/// Whether `path` is currently in `app.ui.startup_plugins` (case-insensitive on Windows).
pub fn is_in_startup_suite(app: &CadApp, path: &str) -> bool {
    let clean = path.trim();
    !clean.is_empty()
        && app
            .ui
            .startup_plugins
            .iter()
            .any(|p| p.eq_ignore_ascii_case(clean))
}

/// Toggle `path` in `app.ui.startup_plugins` (Startup Suite).
pub fn toggle_startup_plugin(app: &mut CadApp, path: &str) {
    let clean = path.trim();
    if clean.is_empty() {
        return;
    }
    if is_in_startup_suite(app, clean) {
        app.ui
            .startup_plugins
            .retain(|p| !p.eq_ignore_ascii_case(clean));
        let msg = format!("{}: {clean}", crate::tl!("Removed from Startup Suite (Auto-Load)"));
        if let Some(ide) = app.lisp_ide.as_mut() {
            ide.output.push(msg.clone());
            ide.status = Some(msg.clone());
        }
        app.session.echo(msg);
    } else {
        app.ui.startup_plugins.push(clean.to_string());
        let msg = format!("{}: {clean}", crate::tl!("Added to Startup Suite (Auto-Load at startup & per drawing)"));
        if let Some(ide) = app.lisp_ide.as_mut() {
            ide.output.push(msg.clone());
            ide.status = Some(msg.clone());
        }
        app.session.echo(msg);
    }
}

/// Automatically load any `.lsp` or `.dll` files pinned in `app.ui.startup_plugins` at startup
/// and whenever a new or opened drawing becomes active.
pub fn ensure_startup_plugins_loaded(app: &mut CadApp) {
    if app.ui.startup_plugins.is_empty() || app.session.docs.is_empty() {
        return;
    }
    if app.session.lisp_busy() || app.session.prompting() {
        return;
    }
    let cur_doc = app.session.active;
    let already_synced = app.lisp_ide.as_ref().is_some_and(|ide| {
        ide.startup_synced_doc == Some(cur_doc) && ide.startup_synced_list == app.ui.startup_plugins
    });
    if already_synced {
        return;
    }
    let paths = app.ui.startup_plugins.clone();
    {
        let ide = app.lisp_ide.get_or_insert_with(LispIdeState::default);
        ide.startup_synced_doc = Some(cur_doc);
        ide.startup_synced_list = paths.clone();
    }
    for path in paths {
        if !std::path::Path::new(&path).exists() {
            continue;
        }
        if path.trim().to_ascii_lowercase().ends_with(".dll") {
            let already_loaded = app.lisp_ide.as_ref().is_some_and(|ide| {
                ide.loaded_dlls
                    .iter()
                    .any(|d| d.path.eq_ignore_ascii_case(&path))
            });
            if !already_loaded {
                load_dll_into_ide(app, &path);
            }
        } else {
            load_path_into_ide(app, &path);
        }
    }
}

/// Load a `.dll` assembly or read a `.lsp` / `.vb` / `.cs` file from `path` into the IDE and session.
pub fn load_path_into_ide(app: &mut CadApp, path: &str) {
    let lower = path.trim().to_ascii_lowercase();
    if lower.ends_with(".dll") {
        load_dll_into_ide(app, path);
        return;
    }
    match cadcraft_engine::lisp::builtins::read_text_file(path) {
        Some(text) => {
            let name = std::path::Path::new(path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| path.to_string());
            let ide = app.lisp_ide.get_or_insert_with(LispIdeState::default);
            ide.file_name = name.clone();
            ide.file_path = Some(path.to_string());
            ide.code = text.clone();
            if lower.ends_with(".vb") {
                ide.lang = IdeLang::VbNet;
                let msg = format!("{}: {name}", crate::tl!("VB.NET source opened"));
                ide.output.push(msg.clone());
                ide.status = Some(msg);
                return;
            }
            if lower.ends_with(".cs") {
                ide.lang = IdeLang::CSharp;
                let msg = format!("{}: {name}", crate::tl!("C# source opened"));
                ide.output.push(msg.clone());
                ide.status = Some(msg);
                return;
            }
            ide.lang = IdeLang::Lisp;
            let cmds = extract_c_commands(&text);
            app.session.set_lisp_file_dir(
                std::path::Path::new(path)
                    .parent()
                    .map(|p| p.to_string_lossy().into_owned()),
            );
            let log_before = app.session.log.len();
            match app.session.lisp_load(&name, &text, false) {
                Ok(Some(Ok(v))) => {
                    ide.commands = cmds.clone();
                    ide.loaded_code = Some(text);
                    ide.show_plugin_menu = true;
                    let new_logs: Vec<String> = app.session.log.get(log_before..).map(|s| s.to_vec()).unwrap_or_default();
                    for l in new_logs {
                        ide.output.push(l);
                    }
                    let val_str = cadcraft_engine::lisp::value::to_text(&v, true);
                    let msg = if cmds.is_empty() {
                        format!("{} {name} → {val_str}", crate::tl!("Loaded"))
                    } else {
                        format!(
                            "{} {name} — {}: {}",
                            crate::tl!("Loaded"),
                            crate::tl!("Commands"),
                            cmds.join(", ")
                        )
                    };
                    ide.output.push(msg.clone());
                    ide.status = Some(msg.clone());
                    for cmd in &cmds {
                        app.cmd.push_history(cmd);
                    }
                    app.session.echo(msg);
                    if !cmds.is_empty() && app.ui.dialog.as_deref() == Some("vbaide") {
                        app.ui.dialog = None;
                    }
                }
                Ok(Some(Err(e))) => {
                    ide.commands.clear();
                    ide.loaded_code = None;
                    let new_logs: Vec<String> = app.session.log.get(log_before..).map(|s| s.to_vec()).unwrap_or_default();
                    for l in new_logs {
                        ide.output.push(l);
                    }
                    let err_msg = format!("{} {name}: {e}", crate::tl!("Error in"));
                    ide.output.push(err_msg.clone());
                    ide.status = Some(err_msg);
                }
                Ok(None) => {
                    ide.commands = cmds.clone();
                    ide.loaded_code = Some(text);
                    ide.show_plugin_menu = true;
                    for cmd in &cmds {
                        app.cmd.push_history(cmd);
                    }
                    let msg = format!("{} {name}...", crate::tl!("Routine started from"));
                    ide.output.push(msg.clone());
                    ide.status = Some(msg);
                    if app.ui.dialog.as_deref() == Some("vbaide") {
                        app.ui.dialog = None;
                    }
                }
                Err(e) => {
                    ide.commands.clear();
                    ide.loaded_code = None;
                    let err_msg = format!("{} {name}: {e}", crate::tl!("Failed to load"));
                    ide.output.push(err_msg.clone());
                    ide.status = Some(err_msg);
                }
            }
        }
        None => {
            let ide = app.lisp_ide.get_or_insert_with(LispIdeState::default);
            let msg = format!("NETLOAD: {} {path}", crate::tl!("cannot read file"));
            ide.output.push(msg.clone());
            ide.status = Some(msg.clone());
            app.session.echo(msg);
        }
    }
}

/// Evaluate or compile the current editor buffer, returning `true` when LISP evaluation succeeded.
fn execute_current_code(app: &mut CadApp) -> bool {
    let Some(lang) = app.lisp_ide.as_ref().map(|i| i.lang) else {
        return false;
    };
    if lang != IdeLang::Lisp {
        compile_and_load_dotnet_source(app);
        return false;
    }
    let Some(ide) = app.lisp_ide.as_ref() else { return false };
    let name = ide.file_name.clone();
    let code = ide.code.clone();
    let cmds = extract_c_commands(&code);
    let log_before = app.session.log.len();
    let res = app.session.lisp_load(&name, &code, false);
    let new_logs: Vec<String> = app.session.log.get(log_before..).map(|s| s.to_vec()).unwrap_or_default();
    let Some(ide) = app.lisp_ide.as_mut() else { return false };
    for l in new_logs {
        ide.output.push(l);
    }
    match res {
        Ok(Some(Ok(v))) => {
            ide.commands = cmds.clone();
            ide.loaded_code = Some(code);
            let val_str = cadcraft_engine::lisp::value::to_text(&v, true);
            let msg = if cmds.is_empty() {
                format!("⇒ {val_str}")
            } else {
                format!(
                    "⇒ {val_str}  ({}: {})",
                    crate::tl!("Registered commands"),
                    cmds.join(", ")
                )
            };
            ide.output.push(msg.clone());
            ide.status = Some(msg);
            for cmd in &cmds {
                app.cmd.push_history(cmd);
            }
            true
        }
        Ok(Some(Err(e))) => {
            ide.commands.clear();
            ide.loaded_code = None;
            let err_msg = format!("{}: {e}", crate::tl!("Error"));
            ide.output.push(err_msg.clone());
            ide.status = Some(err_msg);
            false
        }
        Ok(None) => {
            ide.commands = cmds.clone();
            ide.loaded_code = Some(code);
            ide.status = Some(format!("{} ({name})...", crate::tl!("Routine started")));
            for cmd in &cmds {
                app.cmd.push_history(cmd);
            }
            true
        }
        Err(e) => {
            ide.commands.clear();
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

/// Render the AutoLISP / VB.NET / C# IDE & NETLOAD Studio window.
pub fn dialog(app: &mut CadApp, ctx: &egui::Context, open: &mut bool) {
    if app.lisp_ide.is_none() {
        app.lisp_ide = Some(LispIdeState::default());
    }
    let tok = Tokens::get();
    let mut do_open_file = false;
    let mut do_save_file = false;
    let mut do_run_code = false;
    let mut do_eval_quick = false;
    let mut do_hide_studio = false;
    let mut toggle_startup_path: Option<String> = None;
    let mut run_lisp_cmd: Option<String> = None;
    let mut run_dotnet_cmd: Option<(String, String)> = None;
    let cur_file_pinned = app
        .lisp_ide
        .as_ref()
        .and_then(|i| i.file_path.as_deref())
        .is_some_and(|p| is_in_startup_suite(app, p));
    let startup_list_snapshot = app.ui.startup_plugins.clone();

    egui::Window::new(crate::tl!("AutoLISP / VB.NET / C# IDE & NETLOAD Studio"))
        .id(egui::Id::unique("vbaide_window"))
        .open(open)
        .default_size(vec2(820.0, 620.0))
        .min_size(vec2(560.0, 420.0))
        .resizable(true)
        .show(ctx, |ui| {
            let Some(ide) = app.lisp_ide.as_mut() else { return };

            // Top toolbar
            ui.horizontal_wrapped(|ui| {
                if ui
                    .button(
                        RichText::new(format!("📂 {}", crate::tl!("Open .lsp / .vb / .cs")))
                            .strong()
                            .color(egui::Color32::from_rgb(0x38, 0xb6, 0xff)),
                    )
                    .on_hover_text(crate::tl!("Select and load a .lsp, .dll, .vb, or .cs file from disk"))
                    .clicked()
                {
                    do_open_file = true;
                }
                if ui
                    .button(format!("💾 {}", crate::tl!("Save")))
                    .on_hover_text(crate::tl!("Save the current source code in the editor to disk"))
                    .clicked()
                {
                    do_save_file = true;
                }
                let run_label = match ide.lang {
                    IdeLang::Lisp => format!("▶ {}", crate::tl!("Load & Run LISP")),
                    IdeLang::VbNet | IdeLang::CSharp => format!("▶ {}", crate::tl!("Compile & Load .NET DLL")),
                };
                if ui
                    .button(
                        RichText::new(run_label)
                            .strong()
                            .color(egui::Color32::from_rgb(0x4e, 0xc9, 0x6f)),
                    )
                    .on_hover_text(crate::tl!("Evaluate the LISP code or compile the VB.NET / C# source directly into a .NET DLL"))
                    .clicked()
                {
                    do_run_code = true;
                }
                if (!ide.loaded_dlls.is_empty() || !ide.commands.is_empty())
                    && ui
                        .button(RichText::new(format!("⬇ {}", crate::tl!("Compact Menu"))).strong())
                        .on_hover_text(crate::tl!("Hide this Studio window and show the compact floating Plugin Commands menu"))
                        .clicked()
                {
                    ide.show_plugin_menu = true;
                    do_hide_studio = true;
                }
                if let Some(fp) = &ide.file_path {
                    let pin_label = if cur_file_pinned {
                        RichText::new(format!("📌 ✓ {}", crate::tl!("Auto-Load")))
                            .strong()
                            .color(egui::Color32::from_rgb(0x4e, 0xc9, 0x6f))
                    } else {
                        RichText::new(format!("📌 {}", crate::tl!("Auto-Load")))
                    };
                    if ui
                        .button(pin_label)
                        .on_hover_text(crate::tl!("Toggle Startup Suite: automatically load this file at startup and in every opened drawing"))
                        .clicked()
                    {
                        toggle_startup_path = Some(fp.clone());
                    }
                }

                ui.separator();
                if ui.selectable_label(ide.lang == IdeLang::Lisp, "LISP").clicked() {
                    ide.lang = IdeLang::Lisp;
                    if !ide.file_name.ends_with(".lsp") {
                        ide.file_name = "Nieuw.lsp".into();
                    }
                }
                if ui.selectable_label(ide.lang == IdeLang::VbNet, "VB.NET").clicked() {
                    ide.lang = IdeLang::VbNet;
                    if !ide.file_name.ends_with(".vb") {
                        ide.file_name = "Plugin.vb".into();
                    }
                }
                if ui.selectable_label(ide.lang == IdeLang::CSharp, "C#").clicked() {
                    ide.lang = IdeLang::CSharp;
                    if !ide.file_name.ends_with(".cs") {
                        ide.file_name = "Plugin.cs".into();
                    }
                }

                ui.separator();
                craft_ui::ComboBox::from_id_salt("lisp_template_combo")
                    .selected_text(crate::tl!("Template..."))
                    .show_ui(ui, |ui| {
                        if ui.selectable_label(false, crate::tl!("0. Empty file (New)")).clicked() {
                            ide.file_name = match ide.lang {
                                IdeLang::Lisp => "Nieuw.lsp".into(),
                                IdeLang::VbNet => "Nieuw.vb".into(),
                                IdeLang::CSharp => "Nieuw.cs".into(),
                            };
                            ide.file_path = None;
                            ide.code.clear();
                            ide.loaded_code = None;
                            ide.commands.clear();
                        }
                        if ui.selectable_label(false, crate::tl!("1. AutoLISP: Basic Math & Commands")).clicked() {
                            ide.lang = IdeLang::Lisp;
                            ide.file_name = "routine.lsp".into();
                            ide.code = DEFAULT_LISP.to_string();
                            ide.loaded_code = None;
                            ide.commands = extract_c_commands(&ide.code);
                        }
                        if ui.selectable_label(false, crate::tl!("2. AutoLISP: Survey Marker (c:meetpunt)")).clicked() {
                            ide.lang = IdeLang::Lisp;
                            ide.file_name = "meetpunt.lsp".into();
                            ide.code = r#";; Draws a circle with crosshair at a picked point
(defun c:meetpunt (/ pt)
  (setq pt (getpoint "\nSpecify survey point: "))
  (if pt
    (progn
      (command "_.CIRCLE" pt 2.5)
      (command "_.LINE" (list (- (car pt) 4.0) (cadr pt)) (list (+ (car pt) 4.0) (cadr pt)) "")
      (command "_.LINE" (list (car pt) (- (cadr pt) 4.0)) (list (car pt) (+ (cadr pt) 4.0)) "")
      (princ "\nSurvey point placed at: ")
      (princ pt)
    )
  )
  (princ)
)
"#
                            .to_string();
                            ide.loaded_code = None;
                            ide.commands = extract_c_commands(&ide.code);
                        }
                        if ui.selectable_label(false, crate::tl!("3. VB.NET Plugin (.vb -> .dll)")).clicked() {
                            ide.lang = IdeLang::VbNet;
                            ide.file_name = "MyCadPlugin.vb".into();
                            ide.code = DEFAULT_VBNET.to_string();
                        }
                        if ui.selectable_label(false, crate::tl!("4. C# Plugin (.cs -> .dll)")).clicked() {
                            ide.lang = IdeLang::CSharp;
                            ide.file_name = "MyCSharpPlugin.cs".into();
                            ide.code = DEFAULT_CSHARP.to_string();
                        }
                    });
            });

            // Loaded .NET DLLs bar
            if !ide.loaded_dlls.is_empty() {
                ui.separator();
                for dll in &ide.loaded_dlls {
                    let dll_pinned = startup_list_snapshot
                        .iter()
                        .any(|p| p.eq_ignore_ascii_case(&dll.path));
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            RichText::new(format!("🧩 .NET DLL {}:", dll.name))
                                .strong()
                                .color(egui::Color32::from_rgb(0x38, 0xb6, 0xff)),
                        )
                        .on_hover_text(&dll.path);
                        let pin_btn = if dll_pinned { "📌 ✓" } else { "📌" };
                        if ui
                            .small_button(pin_btn)
                            .on_hover_text(crate::tl!("Toggle Startup Suite (Auto-Load at startup)"))
                            .clicked()
                        {
                            toggle_startup_path = Some(dll.path.clone());
                        }
                        for cmd in &dll.commands {
                            if ui
                                .button(
                                    RichText::new(format!("▶ {cmd}"))
                                        .monospace()
                                        .strong()
                                        .color(egui::Color32::from_rgb(0x4e, 0xc9, 0x6f)),
                                )
                                .on_hover_text(format!(
                                    "{} {cmd} ({})",
                                    crate::tl!("Run .NET command"),
                                    dll.name
                                ))
                                .clicked()
                            {
                                run_dotnet_cmd = Some((dll.path.clone(), cmd.clone()));
                            }
                        }
                    });
                }
            }

            // Detected LISP `c:` commands bar
            if ide.lang == IdeLang::Lisp && !ide.commands.is_empty() {
                ui.separator();
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(format!("{}:", crate::tl!("AutoLISP Commands (click to run)"))).strong());
                    for cmd in &ide.commands {
                        if ui
                            .button(RichText::new(format!("▶ {cmd}")).monospace().strong())
                            .on_hover_text(format!("{} {cmd}", crate::tl!("Run command")))
                            .clicked()
                        {
                            run_lisp_cmd = Some(cmd.clone());
                        }
                    }
                });
            }

            ui.separator();

            // Code editor
            let editor_title = match ide.lang {
                IdeLang::Lisp => crate::tl!("AutoLISP Editor (.lsp)"),
                IdeLang::VbNet => crate::tl!("VB.NET Plugin Editor (.vb)"),
                IdeLang::CSharp => crate::tl!("C# Plugin Editor (.cs)"),
            };
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("{editor_title}:")).strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new(&ide.file_name).monospace().color(tok.text_dim));
                });
            });
            egui::ScrollArea::vertical()
                .id_salt("lisp_ide_editor_scroll")
                .max_height(250.0)
                .show(ui, |ui| {
                    let te = egui::TextEdit::multiline(&mut ide.code)
                        .font(egui::TextStyle::Monospace)
                        .code_editor()
                        .desired_width(f32::INFINITY)
                        .desired_rows(13);
                    if ui.add(te).changed() && ide.lang == IdeLang::Lisp {
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
        let ok = execute_current_code(app);
        let has_cmds = app
            .lisp_ide
            .as_ref()
            .is_some_and(|i| !i.commands.is_empty() || !i.loaded_dlls.is_empty());
        if ok && has_cmds {
            if let Some(ide) = app.lisp_ide.as_mut() {
                ide.show_plugin_menu = true;
            }
            *open = false;
        }
        if let Some(id) = ctx.memory(|m| m.focused()) {
            ctx.memory_mut(|m| m.surrender_focus(id));
        }
    }
    if do_eval_quick {
        eval_quick_expr(app);
    }
    if let Some(cmd) = run_lisp_cmd {
        if let Some(ide) = app.lisp_ide.as_mut() {
            ide.show_plugin_menu = true;
        }
        *open = false;
        if let Some(id) = ctx.memory(|m| m.focused()) {
            ctx.memory_mut(|m| m.surrender_focus(id));
        }
        run_lisp_command_from_ui(app, &cmd);
    }
    if let Some((dll_path, cmd)) = run_dotnet_cmd {
        if let Some(ide) = app.lisp_ide.as_mut() {
            ide.show_plugin_menu = true;
        }
        *open = false;
        if let Some(id) = ctx.memory(|m| m.focused()) {
            ctx.memory_mut(|m| m.surrender_focus(id));
        }
        spawn_dotnet_command(app, &dll_path, &cmd);
    }
    if do_hide_studio {
        *open = false;
        if let Some(id) = ctx.memory(|m| m.focused()) {
            ctx.memory_mut(|m| m.surrender_focus(id));
        }
    }
    if let Some(path) = toggle_startup_path {
        toggle_startup_plugin(app, &path);
    }
}

/// Compact floating menu window (`🧩 Plugin Commands`) shown on the drawing canvas when the
/// large NETLOAD Studio is hidden in the background and at least one `.NET` DLL or AutoLISP routine is loaded.
pub fn plugin_menu_overlay(app: &mut CadApp, ctx: &egui::Context) {
    if app.ui.dialog.as_deref() == Some("vbaide") {
        return;
    }
    let has_plugins = app.lisp_ide.as_ref().is_some_and(|ide| {
        ide.show_plugin_menu
            && (!ide.loaded_dlls.is_empty()
                || (ide.loaded_code.is_some() && !ide.commands.is_empty()))
    });
    if !has_plugins {
        return;
    }

    let mut open_menu = true;
    let mut do_netload = false;
    let mut do_open_studio = false;
    let mut toggle_startup_path: Option<String> = None;
    let mut run_dotnet_cmd: Option<(String, String)> = None;
    let mut run_lisp_cmd: Option<String> = None;
    let cur_file_pinned = app
        .lisp_ide
        .as_ref()
        .and_then(|i| i.file_path.as_deref())
        .is_some_and(|p| is_in_startup_suite(app, p));

    let window_title = app
        .lisp_ide
        .as_ref()
        .map(|ide| {
            if ide.loaded_dlls.len() == 1 && ide.commands.is_empty() {
                format!("🧩 {}", ide.loaded_dlls[0].name)
            } else if ide.loaded_dlls.is_empty() && !ide.commands.is_empty() {
                format!("📜 {}", ide.file_name)
            } else {
                format!("🧩 {}", crate::tl!("Plugin Commands"))
            }
        })
        .unwrap_or_else(|| format!("🧩 {}", crate::tl!("Plugin Commands")));

    egui::Window::new(window_title)
        .id(egui::Id::unique("cc_plugin_mini_menu"))
        .open(&mut open_menu)
        .default_pos(egui::pos2(260.0, 62.0))
        .resizable(false)
        .collapsible(true)
        .show(ctx, |ui| {
            let Some(ide) = app.lisp_ide.as_ref() else { return };
            for (idx, dll) in ide.loaded_dlls.iter().enumerate() {
                if idx > 0 {
                    ui.separator();
                }
                if ide.loaded_dlls.len() > 1 || !ide.commands.is_empty() {
                    ui.label(
                        RichText::new(&dll.name)
                            .strong()
                            .size(11.5)
                            .color(egui::Color32::from_rgb(0x38, 0xb6, 0xff)),
                    )
                    .on_hover_text(&dll.path);
                }
                ui.horizontal_wrapped(|ui| {
                    for cmd in &dll.commands {
                        if ui
                            .button(
                                RichText::new(format!("▶ {cmd}"))
                                    .monospace()
                                    .strong()
                                    .color(egui::Color32::from_rgb(0x4e, 0xc9, 0x6f)),
                            )
                            .on_hover_text(format!(
                                "{} {cmd} ({})",
                                crate::tl!("Run .NET command"),
                                dll.name
                            ))
                            .clicked()
                        {
                            run_dotnet_cmd = Some((dll.path.clone(), cmd.clone()));
                        }
                    }
                });
            }
            if ide.loaded_code.is_some() && !ide.commands.is_empty() {
                if !ide.loaded_dlls.is_empty() {
                    ui.separator();
                    ui.label(
                        RichText::new(format!("📜 {}", ide.file_name))
                            .strong()
                            .size(11.5)
                            .color(egui::Color32::from_rgb(0x38, 0xb6, 0xff)),
                    );
                }
                ui.horizontal_wrapped(|ui| {
                    for cmd in &ide.commands {
                        if ui
                            .button(
                                RichText::new(format!("▶ {cmd}"))
                                    .monospace()
                                    .strong()
                                    .color(egui::Color32::from_rgb(0x4e, 0xc9, 0x6f)),
                            )
                            .on_hover_text(format!("{} {cmd}", crate::tl!("Run command")))
                            .clicked()
                        {
                            run_lisp_cmd = Some(cmd.clone());
                        }
                    }
                });
            }
            ui.separator();
            ui.horizontal(|ui| {
                if ui
                    .small_button(format!("📂 {}", crate::tl!("Open .lsp / .vb / .cs")))
                    .on_hover_text(crate::tl!("Select and load a .lsp, .dll, .vb, or .cs file from disk"))
                    .clicked()
                {
                    do_netload = true;
                }
                if ui
                    .small_button(format!("🛠 {}", crate::tl!("Open Studio")))
                    .on_hover_text(crate::tl!("Open the full AutoLISP / VB.NET / C# & NETLOAD Studio"))
                    .clicked()
                {
                    do_open_studio = true;
                }
                if let Some(fp) = &ide.file_path {
                    let pin_txt = if cur_file_pinned {
                        format!("📌 ✓ {}", crate::tl!("Auto-Load"))
                    } else {
                        format!("📌 {}", crate::tl!("Auto-Load"))
                    };
                    if ui
                        .small_button(pin_txt)
                        .on_hover_text(crate::tl!("Toggle Startup Suite: automatically load this file at startup and in every opened drawing"))
                        .clicked()
                    {
                        toggle_startup_path = Some(fp.clone());
                    }
                }
            });
        });

    if let Some(path) = toggle_startup_path {
        toggle_startup_plugin(app, &path);
    }

    if !open_menu
        && let Some(ide) = app.lisp_ide.as_mut()
    {
        ide.show_plugin_menu = false;
    }
    if do_netload {
        open_file_into_ide(app);
    }
    if do_open_studio {
        app.ui.dialog = Some("vbaide".into());
    }
    if let Some((dll_path, cmd)) = run_dotnet_cmd {
        if let Some(id) = ctx.memory(|m| m.focused()) {
            ctx.memory_mut(|m| m.surrender_focus(id));
        }
        spawn_dotnet_command(app, &dll_path, &cmd);
    }
    if let Some(cmd) = run_lisp_cmd {
        if let Some(id) = ctx.memory(|m| m.focused()) {
            ctx.memory_mut(|m| m.surrender_focus(id));
        }
        run_lisp_command_from_ui(app, &cmd);
    }
}

/// Ensure the active drawing has the current LISP buffer loaded, cancel any waiting prompt/routine,
/// and invoke `cmd`.
fn run_lisp_command_from_ui(app: &mut CadApp, cmd: &str) {
    if app.session.prompting() || app.session.running.is_some() || app.session.lisp_running() {
        app.session.cancel();
    }
    if app.session.lisp_running() {
        app.session.lisp_abort("Function cancelled");
    }
    let needs_load = app.lisp_ide.as_ref().is_some_and(|i| {
        i.loaded_code.as_deref() != Some(i.code.as_str())
            || app.session.lisp_var(&format!("C:{cmd}")).is_nil()
    });
    if needs_load && !execute_current_code(app) {
        return;
    }
    let log_before = app.session.log.len();
    app.cmdline(cmd);
    let new_logs: Vec<String> = app.session.log.get(log_before..).map(|s| s.to_vec()).unwrap_or_default();
    if let Some(ide) = app.lisp_ide.as_mut() {
        for l in &new_logs {
            ide.output.push(l.clone());
        }
        if let Some(last) = new_logs.last() {
            ide.status = Some(last.clone());
        }
    }
}

/// Top menu-bar dropdown (`🧩 Plugins`) for quick access to loaded `.NET` DLL and AutoLISP commands.
pub fn plugin_menubar_item(app: &mut CadApp, ui: &mut egui::Ui) {
    let mut do_netload = false;
    let mut do_open_studio = false;
    let mut toggle_floating = false;
    let mut run_dotnet_cmd: Option<(String, String)> = None;
    let mut run_lisp_cmd: Option<String> = None;

    let plugin_count = app.lisp_ide.as_ref().map_or(0, |ide| {
        ide.loaded_dlls.len() + usize::from(ide.loaded_code.is_some() && !ide.commands.is_empty())
    });
    let label = if plugin_count > 0 {
        RichText::new(format!("🧩 {} ({plugin_count})", crate::tl!("Plugins")))
            .strong()
            .color(egui::Color32::from_rgb(0x38, 0xb6, 0xff))
    } else {
        RichText::new(format!("🧩 {}", crate::tl!("Plugins")))
    };

    ui.menu_button(label, |ui| {
        if ui
            .button(
                RichText::new(format!("📂 {}...", crate::tl!("Open .lsp / .vb / .cs")))
                    .strong()
                    .color(egui::Color32::from_rgb(0x38, 0xb6, 0xff)),
            )
            .clicked()
        {
            do_netload = true;
            ui.close();
        }
        if ui
            .button(format!("🛠 {}...", crate::tl!("Open NETLOAD Studio (VBAIDE)")))
            .clicked()
        {
            do_open_studio = true;
            ui.close();
        }
        if let Some(ide) = app.lisp_ide.as_ref()
            && (!ide.loaded_dlls.is_empty()
                || (ide.loaded_code.is_some() && !ide.commands.is_empty()))
        {
            let float_label = if ide.show_plugin_menu {
                format!("✓ {}", crate::tl!("Show Floating Command Menu"))
            } else {
                format!("  {}", crate::tl!("Show Floating Command Menu"))
            };
            if ui.button(float_label).clicked() {
                toggle_floating = true;
                ui.close();
            }
            for dll in &ide.loaded_dlls {
                ui.separator();
                ui.label(
                    RichText::new(format!("🧩 {}", dll.name))
                        .strong()
                        .color(egui::Color32::from_rgb(0x38, 0xb6, 0xff)),
                );
                for cmd in &dll.commands {
                    if ui
                        .button(
                            RichText::new(format!("▶ {cmd}"))
                                .monospace()
                                .strong()
                                .color(egui::Color32::from_rgb(0x4e, 0xc9, 0x6f)),
                        )
                        .clicked()
                    {
                        run_dotnet_cmd = Some((dll.path.clone(), cmd.clone()));
                        ui.close();
                    }
                }
            }
            if ide.loaded_code.is_some() && !ide.commands.is_empty() {
                ui.separator();
                ui.label(
                    RichText::new(format!("📜 {}", ide.file_name))
                        .strong()
                        .color(egui::Color32::from_rgb(0x38, 0xb6, 0xff)),
                );
                for cmd in &ide.commands {
                    if ui
                        .button(
                            RichText::new(format!("▶ {cmd}"))
                                .monospace()
                                .strong()
                                .color(egui::Color32::from_rgb(0x4e, 0xc9, 0x6f)),
                        )
                        .clicked()
                    {
                        run_lisp_cmd = Some(cmd.clone());
                        ui.close();
                    }
                }
            }
        }
    });

    if toggle_floating
        && let Some(ide) = app.lisp_ide.as_mut()
    {
        ide.show_plugin_menu = !ide.show_plugin_menu;
    }
    if do_netload {
        if app.lisp_ide.is_none() {
            app.lisp_ide = Some(LispIdeState::default());
        }
        open_file_into_ide(app);
    }
    if do_open_studio {
        if app.lisp_ide.is_none() {
            app.lisp_ide = Some(LispIdeState::default());
        }
        app.ui.dialog = Some("vbaide".into());
    }
    if let Some((dll_path, cmd)) = run_dotnet_cmd {
        if let Some(id) = ui.ctx().memory(|m| m.focused()) {
            ui.ctx().memory_mut(|m| m.surrender_focus(id));
        }
        spawn_dotnet_command(app, &dll_path, &cmd);
    }
    if let Some(cmd) = run_lisp_cmd {
        if let Some(id) = ui.ctx().memory(|m| m.focused()) {
            ui.ctx().memory_mut(|m| m.surrender_focus(id));
        }
        run_lisp_command_from_ui(app, &cmd);
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

    #[test]
    fn recognizes_plugin_file_extensions() {
        assert!(is_plugin_file("MyCadApp.dll"));
        assert!(is_plugin_file("test.vb"));
        assert!(is_plugin_file("test.cs"));
        assert!(!is_plugin_file("drawing.dwg"));
        assert!(!is_plugin_file("routine.lsp"));
    }
}
