//! Manufacturing and document outputs, plus small file helpers for outputs
//! produced in the UI (PDF) and 3D models loaded by the viewer.

use std::path::{Path, PathBuf};

use pcbplannercad_output::{assembly, fab, gerber_read, ipc2581, step};
use tauri::State;

use super::{Res, lock};
use crate::AppState;
use crate::workspace::Workspace;

fn board<'a>(ws: &'a Workspace, id: &str) -> Res<&'a pcbplannercad_core::Document> {
    let d = ws.doc(id)?;
    if d.kind != pcbplannercad_core::DocKind::Pcb {
        return Err("not a PCB document".into());
    }
    Ok(d.session.document())
}

/// The board with a project variant applied: its DNP parts get `DNP=1`, which
/// the BOM, pick-and-place and IPC-2581 writers leave out.
fn with_variant(ws: &Workspace, doc: &pcbplannercad_core::Document, variant: Option<&str>) -> Res<Option<pcbplannercad_core::Document>> {
    let Some(name) = variant.filter(|v| !v.is_empty()) else { return Ok(None) };
    let v = ws.project()?.project.variants.iter().find(|v| v.name == name).ok_or_else(|| format!("variant {name} not found"))?;
    let mut items = std::collections::BTreeMap::new();
    for (id, item) in doc.items() {
        let mut item = item.clone();
        if let pcbplannercad_core::Item::Footprint(f) = &mut item {
            if v.dnp.contains(&f.designator) {
                f.parameters.insert("DNP".into(), "1".into());
            }
        }
        items.insert(id, item);
    }
    Ok(Some(pcbplannercad_core::Document::from_parts(doc.kind(), items, doc.cache().clone()).with_footprints(doc.footprint_cache().clone())))
}

fn project_name(ws: &Workspace, id: &str) -> String {
    ws.project
        .as_ref()
        .map(|p| p.project.name.clone())
        .or_else(|| ws.doc_info(id).ok().map(|i| i.title.trim_end_matches(".arpcb").to_string()))
        .unwrap_or_else(|| "board".into())
}

/// Writes Gerbers, drills and optional assembly files into `dir`.
#[tauri::command]
pub fn out_fab(state: State<AppState>, id: String, dir: String, mut options: fab::FabOptions, variant: Option<String>) -> Res<Vec<String>> {
    let ws = lock(&state);
    if options.gerber.project_name.is_empty() {
        options.gerber.project_name = project_name(&ws, &id);
    }
    let base = board(&ws, &id)?;
    let varied = with_variant(&ws, base, variant.as_deref())?;
    let files = fab::write_package(varied.as_ref().unwrap_or(base), Path::new(&dir), &options).map_err(|e| e.to_string())?;
    Ok(files.into_iter().map(|p| p.display().to_string()).collect())
}

#[tauri::command]
pub fn out_pnp(state: State<AppState>, id: String, path: String, format: assembly::PnpFormat, variant: Option<String>) -> Res<()> {
    let ws = lock(&state);
    let base = board(&ws, &id)?;
    let varied = with_variant(&ws, base, variant.as_deref())?;
    let csv = assembly::pick_and_place_csv(varied.as_ref().unwrap_or(base), format, None);
    std::fs::write(&path, csv).map_err(|e| format!("cannot write {path}: {e}"))
}

#[tauri::command]
pub fn out_board_bom(state: State<AppState>, id: String, path: String, format: assembly::BomFormat, variant: Option<String>) -> Res<()> {
    let ws = lock(&state);
    let base = board(&ws, &id)?;
    let varied = with_variant(&ws, base, variant.as_deref())?;
    let csv = assembly::bom_csv(varied.as_ref().unwrap_or(base), format);
    std::fs::write(&path, csv).map_err(|e| format!("cannot write {path}: {e}"))
}

#[tauri::command]
pub fn out_ipc2581(state: State<AppState>, id: String, path: String) -> Res<()> {
    let ws = lock(&state);
    let xml = ipc2581::write_ipc2581(board(&ws, &id)?, &project_name(&ws, &id)).map_err(|e| e.to_string())?;
    std::fs::write(&path, xml).map_err(|e| format!("cannot write {path}: {e}"))
}

#[tauri::command]
pub fn out_board_model(state: State<AppState>, id: String) -> Res<step::BoardModel> {
    let ws = lock(&state);
    Ok(step::board_model(board(&ws, &id)?))
}

/// Writes meshes (board + component models triangulated in the UI) as STEP or OBJ.
#[tauri::command]
pub fn out_step(path: String, name: String, meshes: Vec<step::Mesh>) -> Res<()> {
    let text = if path.to_ascii_lowercase().ends_with(".obj") { step::mesh_to_obj(&meshes) } else { step::write_step(&name, &meshes).map_err(|e| e.to_string())? };
    std::fs::write(&path, text).map_err(|e| format!("cannot write {path}: {e}"))
}

/// Parses Gerber/Excellon files from a folder or .zip for the viewer.
#[tauri::command]
pub fn out_read_gerbers(path: String) -> Res<Vec<gerber_read::GerberFile>> {
    gerber_read::read_dir_or_zip(Path::new(&path)).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn write_file_bytes(path: String, bytes: Vec<u8>) -> Res<()> {
    std::fs::write(&path, bytes).map_err(|e| format!("cannot write {path}: {e}"))
}

#[tauri::command]
pub fn read_file_bytes(path: String) -> Res<tauri::ipc::Response> {
    let bytes = std::fs::read(&path).map_err(|e| format!("cannot read {path}: {e}"))?;
    Ok(tauri::ipc::Response::new(bytes))
}

/// Expands `${STRUCTURED*_3DMODEL_DIR}` style variables and relative paths for
/// footprint 3D models; returns the first existing candidate.
#[tauri::command]
pub fn resolve_model_path(path: String, base: Option<String>) -> Option<String> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    let library_models = super::imports::library_share().map(|s| s.join("3dmodels"));
    let expanded = if let (Some(start), Some(end)) = (path.find("${"), path.find('}')) {
        let var = &path[start + 2..end];
        let rest = path[end + 1..].trim_start_matches(['/', '\\']);
        let value = std::env::var(var).ok().map(PathBuf::from).or_else(|| if var.contains("3DMODEL") { library_models.clone() } else { None });
        value.map(|v| v.join(rest))
    } else {
        Some(PathBuf::from(&path))
    };
    if let Some(p) = expanded {
        if p.is_relative() {
            if let Some(b) = &base {
                candidates.push(Path::new(b).join(&p));
            }
            if let Some(k) = &library_models {
                candidates.push(k.join(&p));
            }
        }
        candidates.push(p);
    }
    // structured format libraries often reference .wrl while .step ships alongside (and vice versa).
    let mut alt = Vec::new();
    for c in &candidates {
        for ext in ["step", "stp", "wrl"] {
            alt.push(c.with_extension(ext));
        }
    }
    candidates.extend(alt);
    candidates.into_iter().find(|c| c.is_file()).map(|c| c.display().to_string())
}
