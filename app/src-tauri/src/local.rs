//! Local project hub and reviewed imports; no cloud or Git operations.
use crate::{
    AppState,
    commands::{
        self,
        project::{self, ProjectState},
    },
    workspace::LibInfo,
};
use pcbplannercad_core::project::files;
use pcbplannercad_import::wizard;
use serde::Serialize;
use std::path::{Path, PathBuf};
use tauri::State;
type Res<T> = Result<T, String>;
#[tauri::command(async)]
pub fn hub_browse(root: String, relative: String) -> Res<Vec<files::Entry>> {
    files::browse(Path::new(&root), Path::new(&relative))
}
#[tauri::command]
pub fn hub_create_folder(root: String, relative: String, name: String) -> Res<String> {
    files::new_folder(Path::new(&root), Path::new(&relative), &name)
        .map(|p| p.to_string_lossy().into_owned())
}
#[tauri::command(async)]
pub fn hub_create_project(
    state: State<AppState>,
    root: String,
    relative: String,
    name: String,
) -> Res<ProjectState> {
    let mut ws = commands::lock(&state);
    project::ensure_closed_cleanly(&ws)?;
    let p = files::new_project(Path::new(&root), Path::new(&relative), &name)?;
    project::close_project_docs(&mut ws);
    let notes = project::load_project(&mut ws, &p)?;
    Ok(project::state_of(&ws, notes))
}
#[tauri::command(async)]
pub fn project_checkpoint(state: State<AppState>, project_path: String, label: String) -> Res<files::RevisionInfo> {
    let ws = commands::lock(&state);
    project::ensure_closed_cleanly(&ws)?;
    let p = ws.project_at(Path::new(&project_path))?;
    files::checkpoint(&p.path, &label)
}
#[tauri::command(async)]
pub fn project_revisions(state: State<AppState>, project_path: String) -> Res<Vec<files::RevisionInfo>> {
    let path = { commands::lock(&state).project_at(Path::new(&project_path))?.path.clone() };
    files::revisions(&path)
}
#[tauri::command(async)]
pub fn project_restore_copy(state: State<AppState>, project_path: String, revision: String, dest: String) -> Res<String> {
    let path = { commands::lock(&state).project_at(Path::new(&project_path))?.path.clone() };
    files::restore_copy(&path, &revision, Path::new(&dest))
        .map(|p| p.to_string_lossy().into_owned())
}
#[derive(Serialize)]
pub struct Review {
    token: String,
    summary: serde_json::Value,
}
#[tauri::command(async)]
pub fn import_preview(state: State<AppState>, request: wizard::Request) -> Res<Review> {
    let generation = {
        let mut ws = commands::lock(&state);
        ws.import_generation += 1;
        ws.pending_import = None;
        ws.import_generation
    };
    let plan = wizard::prepare(&request)?;
    let summary = serde_json::to_value(&plan.summary).map_err(|e| e.to_string())?;
    let token = format!(
        "{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    let mut ws = commands::lock(&state);
    if ws.import_generation != generation {
        return Err("Import analysis was superseded by a newer request".into());
    }
    ws.pending_import = Some((token.clone(), plan));
    Ok(Review { token, summary })
}
#[tauri::command]
pub fn import_discard(state: State<AppState>, token: String) {
    let mut ws = commands::lock(&state);
    if ws.pending_import.as_ref().is_some_and(|(t, _)| t == &token) {
        ws.pending_import = None;
    }
}
#[derive(Serialize)]
pub struct Applied {
    project: Option<ProjectState>,
    library: Option<LibInfo>,
    files: Vec<String>,
}
#[tauri::command(async)]
pub fn import_apply(state: State<AppState>, token: String, dest: String) -> Res<Applied> {
    let mut ws = commands::lock(&state);
    let (_, plan) = ws
        .pending_import
        .as_ref()
        .filter(|(t, _)| t == &token)
        .ok_or("Import review expired; analyze the input again")?;
    if plan.project.is_some() {
        project::ensure_closed_cleanly(&ws)?;
    }
    let dest = PathBuf::from(dest);
    plan.publish(&dest)?;
    let project_path = plan.project.as_ref().map(|p| dest.join(p));
    let library_path = plan.library.as_ref().map(|p| dest.join(p));
    let files = plan
        .summary
        .outputs
        .iter()
        .map(|p| dest.join(p).to_string_lossy().into_owned())
        .collect();
    ws.pending_import = None;
    let mut result = Applied {
        project: None,
        library: None,
        files,
    };
    if let Some(path) = project_path {
        project::close_project_docs(&mut ws);
        let notes = project::load_project(&mut ws, &path)?;
        result.project = Some(project::state_of(&ws, notes));
    }
    if let Some(path) = library_path {
        let id = ws.open_lib_file(&path)?;
        result.library = Some(ws.lib_info(&id)?);
    }
    Ok(result)
}

#[tauri::command(async)]
pub fn project_compare_revisions(
    state: State<AppState>,
    project_path: String,
    from: String,
    to: String,
) -> Res<files::RevisionChanges> {
    let path = { commands::lock(&state).project_at(Path::new(&project_path))?.path.clone() };
    files::compare_revisions(&path, &from, &to)
}

#[tauri::command(async)]
pub fn hub_preview(
    root: String,
    path: String,
) -> Res<pcbplannercad_core::project::preview::Preview> {
    pcbplannercad_core::project::preview::saved_preview(Path::new(&root), Path::new(&path))
}
