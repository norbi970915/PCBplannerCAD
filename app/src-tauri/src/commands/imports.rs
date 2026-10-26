//! Importing structured format and XML designs and libraries.

use std::path::{Path, PathBuf};

use pcbplannercad_core::{Project, io};
use pcbplannercad_import::{ImportReport, SchematicImport, Severity, xml, structured};
use serde::{Deserialize, Serialize};
use tauri::State;

use super::project::{ProjectState, load_project, state_of};
use super::{Res, lock};
use crate::AppState;
use crate::workspace::LibInfo;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportLine {
    warning: bool,
    text: String,
}

fn report_lines(r: &ImportReport) -> Vec<ReportLine> {
    r.messages.iter().map(|m| ReportLine { warning: m.severity == Severity::Warning, text: m.to_string() }).collect()
}

#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub enum LibrarySource {
    StructuredSymbols,
    StructuredFootprints,
    XmlLibrary,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryImport {
    library: LibInfo,
    report: Vec<ReportLine>,
}

fn stem(path: &Path) -> String {
    path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Imported".into())
}

/// Imports a library into a new, unsaved workspace library.
#[tauri::command]
pub fn import_library(state: State<AppState>, source: LibrarySource, path: String) -> Res<LibraryImport> {
    let path = PathBuf::from(path);
    let name = stem(&path);
    let read = |p: &Path| std::fs::read_to_string(p).map_err(|e| format!("cannot read {}: {e}", p.display()));
    let (lib, report) = match source {
        LibrarySource::StructuredSymbols => structured::parse_symbol_library(&read(&path)?, &name),
        LibrarySource::StructuredFootprints => structured::import_footprint_dir(&path, &name),
        LibrarySource::XmlLibrary => xml::parse_library(&read(&path)?, &name),
    }
    .map_err(|e| format!("{}: {e}", path.display()))?;
    let mut ws = lock(&state);
    let id = ws.add_lib(lib, None, true);
    Ok(LibraryImport { library: ws.lib_info(&id)?, report: report_lines(&report) })
}

#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub enum DesignSource {
    StructuredProject,
    StructuredSchematic,
    XmlSchematic,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesignImport {
    project: ProjectState,
    report: Vec<ReportLine>,
    files: Vec<String>,
}

fn sanitize(name: &str) -> String {
    name.chars().map(|c| if c.is_alphanumeric() || "-_ .".contains(c) { c } else { '_' }).collect()
}

/// Imports a schematic design into a new PCBPlannerCAD project in `dest_dir`
/// (which must not already contain the files to be written), then opens it.
#[tauri::command]
pub fn import_design(state: State<AppState>, source: DesignSource, path: String, dest_dir: String) -> Res<DesignImport> {
    let src = PathBuf::from(&path);
    let dest = PathBuf::from(&dest_dir);
    let name = stem(&src);
    let (project_name, sch, board): (String, SchematicImport, Option<pcbplannercad_import::BoardImport>) = match source {
        DesignSource::StructuredProject => {
            let p = structured::import_project(&src).map_err(|e| e.to_string())?;
            (p.project.name, p.schematic, p.board)
        }
        DesignSource::StructuredSchematic => {
            let pcb = src.with_extension("kicad_pcb");
            let board = if pcb.is_file() { Some(structured::import_board(&pcb).map_err(|e| format!("{}: {e}", pcb.display()))?) } else { None };
            (name.clone(), structured::import_schematic(&src).map_err(|e| e.to_string())?, board)
        }
        DesignSource::XmlSchematic => {
            // XML design exports the board next to the schematic with the same name.
            let brd = src.with_extension("brd");
            let board = if brd.is_file() { Some(xml::import_board(&brd).map_err(|e| format!("{}: {e}", brd.display()))?) } else { None };
            (name.clone(), xml::import_schematic(&src).map_err(|e| e.to_string())?, board)
        }
    };

    // Plan every file first so nothing is half-written on conflicts.
    let project_file = dest.join(format!("{}.arproj", sanitize(&project_name)));
    let mut writes: Vec<(PathBuf, String)> = Vec::new();
    let mut project = Project { name: project_name, sheets: vec![], pcb: None, libraries: vec![], variants: vec![] };
    for sheet in &sch.sheets {
        let text = io::document_to_string(&sheet.document).map_err(|e| e.to_string())?;
        writes.push((dest.join(&sheet.file_name), text));
        project.sheets.push(sheet.file_name.clone());
    }
    // Merge same-named libraries from the schematic and board (symbols + footprints).
    let mut libs: Vec<pcbplannercad_core::Library> = sch.libraries.clone();
    if let Some(b) = &board {
        for l in &b.libraries {
            match libs.iter_mut().find(|x| x.name == l.name) {
                Some(x) => x.merge(l.clone()),
                None => libs.push(l.clone()),
            }
        }
    }
    for lib in &libs {
        let rel = format!("libraries/{}.arlib", sanitize(&lib.name));
        writes.push((dest.join(&rel), io::library_to_string(lib)));
        project.libraries.push(rel);
    }
    if let Some(b) = &board {
        writes.push((dest.join(&b.file_name), io::document_to_string(&b.document).map_err(|e| e.to_string())?));
        project.pcb = Some(b.file_name.clone());
    }
    writes.push((project_file.clone(), io::project_to_string(&project)));
    let conflicts: Vec<String> = writes.iter().filter(|(p, _)| p.exists()).map(|(p, _)| p.display().to_string()).collect();
    if !conflicts.is_empty() {
        return Err(format!("these files already exist — choose an empty folder:\n{}", conflicts.join("\n")));
    }
    std::fs::create_dir_all(dest.join("libraries")).map_err(|e| e.to_string())?;
    for (p, text) in &writes {
        std::fs::write(p, text).map_err(|e| format!("cannot write {}: {e}", p.display()))?;
    }

    let mut report = report_lines(&sch.report);
    if let Some(b) = &board {
        report.extend(report_lines(&b.report));
    }
    let mut ws = lock(&state);
    if let Ok(sheets) = ws.project_sheet_docs() {
        if sheets.iter().any(|(_, id)| ws.doc(id).is_ok_and(|d| d.session.is_dirty())) {
            return Err("the current project has unsaved changes — save it first (files were written but not opened)".into());
        }
        for (_, id) in sheets {
            ws.docs.remove(&id);
        }
    }
    ws.project = None;
    for m in load_project(&mut ws, &project_file)? {
        report.push(ReportLine { warning: true, text: m });
    }
    Ok(DesignImport {
        project: state_of(&ws, vec![]),
        report,
        files: writes.iter().map(|(p, _)| p.display().to_string()).collect(),
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryPaths {
    symbols: Option<String>,
    footprints: Option<String>,
}

/// Locates the newest structured format installation's stock libraries.
#[tauri::command]
pub fn library_paths() -> LibraryPaths {
    let share = library_share();
    let sub = |s: &str| share.as_ref().map(|p| p.join(s)).filter(|p| p.is_dir()).map(|p| p.display().to_string());
    LibraryPaths { symbols: sub("symbols"), footprints: sub("footprints") }
}

/// Finds a configured or installed library data folder by layout.
pub fn library_share() -> Option<PathBuf> {
    pcbplannercad_import::paths::library_share()
}

/// Lists library files in a directory (structured format `*.kicad_sym` / `*.pretty`).
#[tauri::command]
pub fn list_library_files(dir: String) -> Res<Vec<String>> {
    let mut out: Vec<String> = std::fs::read_dir(&dir)
        .map_err(|e| format!("cannot read {dir}: {e}"))?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "kicad_sym" || x == "pretty" || x == "lbr"))
        .map(|p| p.display().to_string())
        .collect();
    out.sort();
    Ok(out)
}
