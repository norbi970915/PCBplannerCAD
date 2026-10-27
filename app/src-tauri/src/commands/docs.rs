use std::collections::BTreeMap;
use std::path::PathBuf;

use pcbplannercad_core::{CachedPart, Command, Delta, DocKind, Document, IdItem, io};
use serde::Serialize;
use tauri::State;

use super::{Res, lock};
use crate::AppState;
use crate::workspace::{DocInfo, Origin, Workspace};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    info: DocInfo,
    items: Vec<IdItem>,
    cache: BTreeMap<String, CachedPart>,
    footprints: BTreeMap<String, pcbplannercad_core::Footprint>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Update {
    delta: Delta,
    info: DocInfo,
    /// Library whose content changed (library-entry documents).
    touched_library: Option<String>,
}

pub fn snapshot(ws: &Workspace, id: &str) -> Res<Snapshot> {
    let doc = ws.doc(id)?.session.document();
    Ok(Snapshot {
        info: ws.doc_info(id)?,
        items: doc.items().map(|(id, item)| IdItem { id, item: item.clone() }).collect(),
        cache: doc.cache().clone(),
        footprints: doc.footprint_cache().clone(),
    })
}

fn update(ws: &mut Workspace, id: &str, delta: Delta) -> Res<Update> {
    let touched_library = if delta.upserted.is_empty() && delta.removed.is_empty() { None } else { ws.sync_entry(id)? };
    Ok(Update { delta, info: ws.doc_info(id)?, touched_library })
}

#[tauri::command]
pub fn doc_list(state: State<AppState>) -> Res<Vec<DocInfo>> {
    let ws = lock(&state);
    ws.docs.keys().map(|id| ws.doc_info(id)).collect()
}

#[tauri::command]
pub fn doc_snapshot(state: State<AppState>, id: String) -> Res<Snapshot> {
    snapshot(&lock(&state), &id)
}

#[tauri::command]
pub fn doc_new(state: State<AppState>, kind: DocKind) -> Res<Snapshot> {
    if io::format_for(kind).is_none() {
        return Err("symbols and footprints are created inside a library".into());
    }
    let mut ws = lock(&state);
    let doc = if kind == DocKind::Pcb { Document::from_item_list(kind, [pcbplannercad_core::Item::Board(Default::default())]) } else { Document::new(kind) };
    let id = ws.add_doc(kind, Origin::File(None), doc);
    snapshot(&ws, &id)
}

pub fn open_doc_file(ws: &mut Workspace, path: &std::path::Path) -> Res<String> {
    if let Some(id) = ws.find_doc(&Origin::File(Some(path.to_path_buf()))) {
        return Ok(id);
    }
    let ext = path.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default();
    let kind = io::kind_for_extension(&ext).ok_or_else(|| format!("unsupported file type: .{ext}"))?;
    let text = std::fs::read_to_string(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let doc = io::document_from_str(kind, &text).map_err(|e| format!("{}: {e}", path.display()))?;
    let id = ws.add_doc(kind, Origin::File(Some(path.to_path_buf())), doc);
    // Boards always carry their settings item (older files may lack it).
    if kind == DocKind::Pcb && pcbplannercad_core::pcb::geometry::board_settings(ws.doc(&id)?.session.document()).is_none() {
        let s = &mut ws.doc_mut(&id)?.session;
        s.execute(Command::Add { items: vec![pcbplannercad_core::Item::Board(Default::default())] }).map_err(|e| e.to_string())?;
        s.mark_saved();
    }
    Ok(id)
}

#[tauri::command]
pub fn doc_open(state: State<AppState>, path: String) -> Res<Snapshot> {
    let mut ws = lock(&state);
    let id = open_doc_file(&mut ws, &PathBuf::from(path))?;
    snapshot(&ws, &id)
}

fn ensure_editable(ws: &Workspace, id: &str) -> Res<()> {
    match &ws.doc(id)?.origin {
        Origin::Symbol { lib, .. } | Origin::Footprint { lib, .. } if ws.lib(lib)?.readonly => {
            Err(format!("library '{}' is read-only — use Save As to edit a copy", ws.lib(lib)?.lib.name))
        }
        _ => Ok(()),
    }
}

#[tauri::command(async)]
pub fn doc_execute(state: State<AppState>, id: String, cmd: Command) -> Res<Update> {
    let mut ws = lock(&state);
    ensure_editable(&ws, &id)?;
    let delta = ws.doc_mut(&id)?.session.execute(cmd).map_err(|e| e.to_string())?;
    update(&mut ws, &id, delta)
}

#[tauri::command]
pub fn doc_undo(state: State<AppState>, id: String) -> Res<Update> {
    let mut ws = lock(&state);
    let delta = ws.doc_mut(&id)?.session.undo().unwrap_or_default();
    update(&mut ws, &id, delta)
}

#[tauri::command]
pub fn doc_redo(state: State<AppState>, id: String) -> Res<Update> {
    let mut ws = lock(&state);
    let delta = ws.doc_mut(&id)?.session.redo().unwrap_or_default();
    update(&mut ws, &id, delta)
}

/// Saves a document. File documents need `path` when untitled; library-entry
/// documents save their library.
#[tauri::command]
pub fn doc_save(state: State<AppState>, id: String, path: Option<String>) -> Res<DocInfo> {
    let mut ws = lock(&state);
    match ws.doc(&id)?.origin.clone() {
        Origin::File(current) => {
            let target = path.map(PathBuf::from).or(current).ok_or("choose a file name first")?;
            ws.save_doc_file(&id, &target)?;
        }
        Origin::Symbol { lib, .. } | Origin::Footprint { lib, .. } => ws.save_lib(&lib, path.map(PathBuf::from))?,
    }
    ws.doc_info(&id)
}

#[tauri::command]
pub fn doc_close(state: State<AppState>, id: String) -> Res<()> {
    let mut ws = lock(&state);
    // Project sheets stay loaded so compile/annotate see them.
    let keep = match &ws.doc(&id)?.origin {
        Origin::File(Some(p)) => ws.project_info().is_some() && ws.doc_info(&id)?.in_project && p.exists(),
        _ => false,
    };
    if keep {
        // Discard unsaved edits by reloading from disk.
        if let Origin::File(Some(p)) = ws.doc(&id)?.origin.clone() {
            let kind = ws.doc(&id)?.kind;
            let text = std::fs::read_to_string(&p).map_err(|e| e.to_string())?;
            let doc = io::document_from_str(kind, &text).map_err(|e| e.to_string())?;
            ws.doc_mut(&id)?.session = pcbplannercad_core::Session::with_document(doc);
        }
    } else {
        ws.docs.remove(&id);
    }
    Ok(())
}

/// Stores library snapshots carried by pasted parts (from another sheet).
#[tauri::command]
pub fn doc_put_cache(state: State<AppState>, id: String, parts: Vec<CachedPart>) -> Res<BTreeMap<String, CachedPart>> {
    let mut ws = lock(&state);
    let doc = ws.doc_mut(&id)?;
    if doc.kind != DocKind::Schematic {
        return Err("parts can only be pasted into schematics".into());
    }
    for p in parts {
        doc.session.put_cached_part(p);
    }
    Ok(doc.session.document().cache().clone())
}

/// Copies a library component (with its symbol) into a schematic's part
/// cache and returns the cache key to use as `Part::lib_ref`.
#[tauri::command]
pub fn doc_cache_component(state: State<AppState>, id: String, library: String, component: String) -> Res<(String, CachedPart)> {
    let mut ws = lock(&state);
    let part = ws
        .lib(&library)?
        .lib
        .cached_part(&component)
        .ok_or_else(|| format!("component {component} (or its symbol) is missing from the library"))?;
    let doc = ws.doc_mut(&id)?;
    if doc.kind != DocKind::Schematic {
        return Err("parts can only be placed on schematics".into());
    }
    let key = doc.session.put_cached_part(part.clone());
    Ok((key, part))
}
