use std::path::{Path, PathBuf};

use pcbplannercad_core::schematic::{self, AnnotateMode, Compiled, SheetRef};
use pcbplannercad_core::{Command, DocKind, Document, Project, io};
use serde::Serialize;
use tauri::State;

use super::docs::open_doc_file;
use super::{Res, lock};
use crate::AppState;
use crate::workspace::{DocInfo, OpenProject, Origin, ProjectInfo, Workspace};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectState {
    project: Option<ProjectInfo>,
    /// Problems hit while loading (missing files…).
    messages: Vec<String>,
}

pub fn state_of(ws: &Workspace, messages: Vec<String>) -> ProjectState {
    ProjectState { project: ws.project_info(), messages }
}

#[tauri::command]
pub fn project_info(state: State<AppState>) -> Res<ProjectState> {
    Ok(state_of(&lock(&state), vec![]))
}

/// Loads every sheet and library of a project file into the workspace.
pub fn load_project(ws: &mut Workspace, path: &Path) -> Res<Vec<String>> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let project = io::project_from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    let open = OpenProject { path: path.to_path_buf(), project };
    let mut messages = Vec::new();
    for rel in open.project.sheets.clone() {
        if let Err(e) = open_doc_file(ws, &open.resolve(&rel)) {
            messages.push(format!("Sheet {rel}: {e}"));
        }
    }
    if let Some(rel) = open.project.pcb.clone() {
        if let Err(e) = open_doc_file(ws, &open.resolve(&rel)) {
            messages.push(format!("Board {rel}: {e}"));
        }
    }
    for rel in open.project.libraries.clone() {
        if let Err(e) = ws.open_lib_file(&open.resolve(&rel)) {
            messages.push(format!("Library {rel}: {e}"));
        }
    }
    ws.project = Some(open);
    Ok(messages)
}

fn ensure_closed_cleanly(ws: &Workspace) -> Res<()> {
    if let Ok(sheets) = ws.project_sheet_docs() {
        for (rel, id) in sheets {
            if ws.doc(&id)?.session.is_dirty() {
                return Err(format!("{rel} has unsaved changes — save or close it first"));
            }
        }
    }
    if let Some(pcb) = ws.project_info().and_then(|p| p.pcb) {
        if let Some(id) = &pcb.doc_id {
            if ws.doc(id)?.session.is_dirty() {
                return Err(format!("{} has unsaved changes — save or close it first", pcb.file));
            }
        }
    }
    Ok(())
}

fn close_project_docs(ws: &mut Workspace) {
    if let Ok(sheets) = ws.project_sheet_docs() {
        for (_, id) in sheets {
            ws.docs.remove(&id);
        }
    }
    if let Some(id) = ws.project_info().and_then(|p| p.pcb).and_then(|p| p.doc_id) {
        ws.docs.remove(&id);
    }
    ws.project = None;
}

#[tauri::command]
pub fn project_open(state: State<AppState>, path: String) -> Res<ProjectState> {
    let mut ws = lock(&state);
    ensure_closed_cleanly(&ws)?;
    close_project_docs(&mut ws);
    let messages = load_project(&mut ws, &PathBuf::from(path))?;
    Ok(state_of(&ws, messages))
}

/// Creates `<dir>/<name>.arproj` with one empty sheet.
#[tauri::command]
pub fn project_new(state: State<AppState>, path: String) -> Res<ProjectState> {
    let path = PathBuf::from(path);
    if path.exists() {
        return Err(format!("{} already exists", path.display()));
    }
    let mut ws = lock(&state);
    ensure_closed_cleanly(&ws)?;
    let dir = path.parent().ok_or("invalid project path")?.to_path_buf();
    let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Project".into());
    let sheet = "Sheet1.arsch".to_string();
    let sheet_path = dir.join(&sheet);
    if !sheet_path.exists() {
        let text = io::document_to_string(&Document::new(DocKind::Schematic)).map_err(|e| e.to_string())?;
        std::fs::write(&sheet_path, text).map_err(|e| e.to_string())?;
    }
    let project = Project { name, sheets: vec![sheet], pcb: None, libraries: vec![], variants: vec![] };
    std::fs::write(&path, io::project_to_string(&project)).map_err(|e| e.to_string())?;
    close_project_docs(&mut ws);
    let messages = load_project(&mut ws, &path)?;
    Ok(state_of(&ws, messages))
}

#[tauri::command]
pub fn project_close(state: State<AppState>) -> Res<ProjectState> {
    let mut ws = lock(&state);
    ensure_closed_cleanly(&ws)?;
    close_project_docs(&mut ws);
    Ok(state_of(&ws, vec![]))
}

fn relative_to(dir: &Path, path: &Path) -> String {
    path.strip_prefix(dir).map(|p| p.to_string_lossy().replace('\\', "/")).unwrap_or_else(|_| path.to_string_lossy().into_owned())
}

/// Adds a new empty sheet file (or an existing one) to the project.
#[tauri::command]
pub fn project_add_sheet(state: State<AppState>, path: String) -> Res<ProjectState> {
    let mut ws = lock(&state);
    let path = PathBuf::from(path);
    let dir = ws.project()?.dir();
    if !path.exists() {
        let text = io::document_to_string(&Document::new(DocKind::Schematic)).map_err(|e| e.to_string())?;
        std::fs::write(&path, text).map_err(|e| e.to_string())?;
    }
    open_doc_file(&mut ws, &path)?;
    let rel = relative_to(&dir, &path);
    let p = ws.project.as_mut().ok_or("no project")?;
    if !p.project.sheets.contains(&rel) {
        p.project.sheets.push(rel);
    }
    ws.save_project_file()?;
    Ok(state_of(&ws, vec![]))
}

#[tauri::command]
pub fn project_remove_sheet(state: State<AppState>, file: String) -> Res<ProjectState> {
    let mut ws = lock(&state);
    let p = ws.project.as_mut().ok_or("no project")?;
    p.project.sheets.retain(|s| *s != file);
    ws.save_project_file()?;
    Ok(state_of(&ws, vec![]))
}

/// Adds a saved library to the project.
#[tauri::command]
pub fn project_add_library(state: State<AppState>, id: String) -> Res<ProjectState> {
    let mut ws = lock(&state);
    let path = ws.lib(&id)?.path.clone().ok_or("save the library to a file first")?;
    let dir = ws.project()?.dir();
    let rel = relative_to(&dir, &path);
    let p = ws.project.as_mut().ok_or("no project")?;
    if !p.project.libraries.contains(&rel) {
        p.project.libraries.push(rel);
    }
    ws.save_project_file()?;
    Ok(state_of(&ws, vec![]))
}

#[tauri::command]
pub fn project_remove_library(state: State<AppState>, id: String) -> Res<ProjectState> {
    let mut ws = lock(&state);
    let path = ws.lib(&id)?.path.clone();
    let dir = ws.project()?.dir();
    if let Some(path) = path {
        let rel = relative_to(&dir, &path);
        let p = ws.project.as_mut().ok_or("no project")?;
        p.project.libraries.retain(|l| *l != rel);
        ws.save_project_file()?;
    }
    Ok(state_of(&ws, vec![]))
}

/// Saves every dirty project sheet and project library.
#[tauri::command]
pub fn project_save_all(state: State<AppState>) -> Res<Vec<DocInfo>> {
    let mut ws = lock(&state);
    let mut saved = Vec::new();
    let ids: Vec<String> = ws.docs.keys().cloned().collect();
    for id in ids {
        let d = ws.doc(&id)?;
        if let (Origin::File(Some(path)), true) = (d.origin.clone(), d.session.is_dirty()) {
            ws.save_doc_file(&id, &path)?;
            saved.push(ws.doc_info(&id)?);
        }
    }
    let libs: Vec<String> = ws.libs.iter().filter(|(_, l)| l.dirty && l.path.is_some() && !l.readonly).map(|(id, _)| id.clone()).collect();
    for id in libs {
        ws.save_lib(&id, None)?;
    }
    if ws.project.is_some() {
        ws.save_project_file()?;
    }
    Ok(saved)
}

fn with_sheets<T>(ws: &Workspace, f: impl FnOnce(&[SheetRef]) -> T) -> Res<T> {
    let sheets = ws.project_sheet_docs()?;
    let refs: Vec<SheetRef> = sheets
        .iter()
        .map(|(rel, id)| Ok(SheetRef { path: rel.clone(), doc: ws.doc(id)?.session.document() }))
        .collect::<Res<_>>()?;
    Ok(f(&refs))
}

/// Netlist + ERC over all project sheets (including unsaved edits).
#[tauri::command]
pub fn project_compile(state: State<AppState>) -> Res<Compiled> {
    let ws = lock(&state);
    with_sheets(&ws, schematic::compile)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnnotateResult {
    changed: usize,
    docs: Vec<String>,
}

#[tauri::command]
pub fn project_annotate(state: State<AppState>, all: bool) -> Res<AnnotateResult> {
    let mut ws = lock(&state);
    let mode = if all { AnnotateMode::All } else { AnnotateMode::Incremental };
    let changes = with_sheets(&ws, |s| schematic::annotate(s, mode))?;
    let sheet_docs = ws.project_sheet_docs()?;
    let mut result = AnnotateResult { changed: 0, docs: vec![] };
    for (sheet, items) in changes {
        if items.is_empty() {
            continue;
        }
        let Some((_, id)) = sheet_docs.iter().find(|(rel, _)| *rel == sheet) else { continue };
        result.changed += items.len();
        ws.doc_mut(id)?.session.execute(Command::ReplaceMany { items }).map_err(|e| e.to_string())?;
        result.docs.push(id.clone());
    }
    Ok(result)
}

#[tauri::command]
pub fn project_export_bom(state: State<AppState>, path: String) -> Res<usize> {
    let ws = lock(&state);
    let compiled = with_sheets(&ws, schematic::compile)?;
    std::fs::write(&path, schematic::bom_csv(&compiled)).map_err(|e| format!("cannot write {path}: {e}"))?;
    Ok(compiled.parts.len())
}

#[tauri::command]
pub fn sheet_connected_items(state: State<AppState>, id: String, item: pcbplannercad_core::ItemId) -> Res<Vec<pcbplannercad_core::ItemId>> {
    let ws = lock(&state);
    Ok(schematic::connected_items(ws.doc(&id)?.session.document(), item))
}

/// Opens the document for a project sheet path (used by ERC navigation).
#[tauri::command]
pub fn project_sheet_doc(state: State<AppState>, file: String) -> Res<Option<String>> {
    let ws = lock(&state);
    Ok(ws.project_sheet_docs()?.into_iter().find(|(rel, _)| *rel == file).map(|(_, id)| id))
}


/// Creates the project board (`<name>.arpcb` next to the project) and opens it.
#[tauri::command]
pub fn project_new_pcb(state: State<AppState>) -> Res<ProjectState> {
    let mut ws = lock(&state);
    let p = ws.project()?;
    if p.project.pcb.is_some() {
        return Err("the project already has a board".into());
    }
    let rel = format!("{}.arpcb", p.project.name.replace(|c: char| !c.is_alphanumeric() && c != '-' && c != '_', "_"));
    let path = p.resolve(&rel);
    if !path.exists() {
        let doc = Document::from_item_list(DocKind::Pcb, [pcbplannercad_core::Item::Board(Default::default())]);
        std::fs::write(&path, io::document_to_string(&doc).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    }
    open_doc_file(&mut ws, &path)?;
    ws.project.as_mut().ok_or("no project")?.project.pcb = Some(rel);
    ws.save_project_file()?;
    Ok(state_of(&ws, vec![]))
}

#[tauri::command]
pub fn project_set_variants(state: State<AppState>, variants: Vec<pcbplannercad_core::project::Variant>) -> Res<ProjectState> {
    let mut ws = lock(&state);
    ws.project.as_mut().ok_or("no project")?.project.variants = variants;
    ws.save_project_file()?;
    Ok(state_of(&ws, vec![]))
}
