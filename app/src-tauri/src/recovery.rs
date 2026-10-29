use crate::{
    AppState,
    commands::lock,
    workspace::{Origin, Workspace},
};
use pcbplannercad_core::{
    io,
    recovery::{self, Bundle, RecoveryFile},
};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::State;
type Res<T> = Result<T, String>;
pub struct RecoveryState {
    pub root: PathBuf,
    session: String,
    last: Mutex<Option<String>>,
}
impl RecoveryState {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            session: format!(
                "s-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos()
            ),
            last: Mutex::new(None),
        }
    }
}
fn stem(path: Option<&Path>, fallback: &str) -> String {
    let text = path
        .and_then(|p| p.file_stem())
        .and_then(|p| p.to_str())
        .unwrap_or(fallback);
    let s: String = text
        .chars()
        .take(40)
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if s.is_empty() { fallback.into() } else { s }
}
fn add_file(files: &mut Vec<RecoveryFile>, file: RecoveryFile) -> Res<()> {
    if files.len() >= 512
        || file.content.len() as u64 + files.iter().map(|f| f.content.len() as u64).sum::<u64>()
            > 128 * 1024 * 1024
    {
        return Err(
            "Recovery copy exceeds 512 files or 128 MiB; earlier copies were preserved".into(),
        );
    }
    files.push(file);
    Ok(())
}
fn collect(ws: &Workspace, force: bool) -> Res<Option<Bundle>> {
    let dirty = ws
        .docs
        .values()
        .any(|d| matches!(d.origin, Origin::File(_)) && d.session.is_dirty())
        || ws.libs.values().any(|l| !l.readonly && l.dirty);
    if !force && !dirty {
        return Ok(None);
    }
    let mut files = Vec::new();
    let mut docs = BTreeMap::new();
    let mut libs = BTreeMap::new();
    for (id, doc) in &ws.docs {
        let Origin::File(source) = &doc.origin else {
            continue;
        };
        let ext = match doc.kind {
            pcbplannercad_core::DocKind::Pcb => "arpcb",
            pcbplannercad_core::DocKind::Schematic => "arsch",
            _ => continue,
        };
        let path = format!(
            "documents/{id}-{}.{ext}",
            stem(source.as_deref(), "Untitled")
        );
        add_file(
            &mut files,
            RecoveryFile {
                path: path.clone(),
                source: source.as_ref().map(|p| p.to_string_lossy().into_owned()),
                content: io::document_to_string(doc.session.document())
                    .map_err(|e| e.to_string())?,
            },
        )?;
        docs.insert(id.clone(), path);
    }
    for (id, lib) in &ws.libs {
        if lib.readonly {
            continue;
        }
        let path = format!(
            "libraries/{id}-{}.arlib",
            stem(lib.path.as_deref(), "Library")
        );
        add_file(
            &mut files,
            RecoveryFile {
                path: path.clone(),
                source: lib.path.as_ref().map(|p| p.to_string_lossy().into_owned()),
                content: io::library_to_string(&lib.lib),
            },
        )?;
        libs.insert(id.clone(), path);
    }
    if files.is_empty() {
        return Ok(None);
    }
    let mut notes=vec!["External 3D model assets are not copied. Keep the original design/library asset folders and relink relative model paths after restoring.".into()];
    let mut project_file = None;
    if let (Some(info), Some(open)) = (ws.project_info(), ws.project.as_ref()) {
        let sheets: Option<Vec<_>> = info
            .sheets
            .iter()
            .map(|s| s.doc_id.as_ref().and_then(|id| docs.get(id)).cloned())
            .collect();
        let board = match &info.pcb {
            None => Some(None),
            Some(b) => b
                .doc_id
                .as_ref()
                .and_then(|id| docs.get(id))
                .cloned()
                .map(Some),
        };
        let libraries: Option<Vec<_>> = info
            .libraries
            .iter()
            .map(|id| libs.get(id).cloned())
            .collect();
        if let (Some(sheets), Some(board), Some(libraries)) = (sheets, board, libraries) {
            if libraries.len() == open.project.libraries.len() {
                let mut p = open.project.clone();
                p.sheets = sheets;
                p.pcb = board;
                p.libraries = libraries;
                project_file = Some("Recovered.arproj".to_string());
                add_file(
                    &mut files,
                    RecoveryFile {
                        path: "Recovered.arproj".into(),
                        source: Some(open.path.to_string_lossy().into_owned()),
                        content: io::project_to_string(&p),
                    },
                )?;
            }
        }
        if project_file.is_none() {
            notes.push("Some project dependencies were unavailable. Native documents/libraries were copied separately; no incomplete project link was created.".into());
        }
    }
    Ok(Some(Bundle {
        title: ws
            .project
            .as_ref()
            .map(|p| p.project.name.clone())
            .unwrap_or_else(|| "Open documents and libraries".into()),
        project: project_file,
        files,
        notes,
    }))
}
#[tauri::command(async)]
pub fn recovery_capture(
    state: State<AppState>,
    recovery_state: State<RecoveryState>,
    force: bool,
) -> Res<Option<recovery::Info>> {
    let mut last = recovery_state
        .last
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let Some(bundle) = ({ collect(&lock(&state), force)? }) else {
        return Ok(None);
    };
    let signature = recovery::fingerprint(&serde_json::to_vec(&bundle).map_err(|e| e.to_string())?);
    if !force && last.as_ref() == Some(&signature) {
        return Ok(None);
    }
    let result = recovery::save(&recovery_state.root, &recovery_state.session, &bundle)?;
    *last = Some(signature);
    Ok(Some(result))
}
#[tauri::command(async)]
pub fn recovery_list(recovery_state: State<RecoveryState>) -> Res<recovery::Listing> {
    recovery::list(&recovery_state.root)
}
#[tauri::command(async)]
pub fn recovery_restore(
    state: State<AppState>,
    recovery_state: State<RecoveryState>,
    id: String,
    dest: String,
) -> Res<recovery::Restored> {
    let ws = lock(&state);
    ws.ensure_export_target(Path::new(&dest))?;
    recovery::restore(&recovery_state.root, &id, Path::new(&dest))
}
#[cfg(test)]
mod tests {
    use super::*;
    use pcbplannercad_core::{DocKind, Document, Library, Project};
    #[test]
    fn capture_includes_unsaved_native_contents_and_links_without_mutating_history() {
        let mut ws = Workspace::default();
        let sch = ws.add_doc(
            DocKind::Schematic,
            Origin::File(Some("source/Main.arsch".into())),
            Document::new(DocKind::Schematic),
        );
        let pcb = ws.add_doc(
            DocKind::Pcb,
            Origin::File(Some("source/Board.arpcb".into())),
            Document::new(DocKind::Pcb),
        );
        let lib = ws.add_lib(
            Library {
                name: "Parts".into(),
                ..Default::default()
            },
            Some("source/Parts.arlib".into()),
            true,
        );
        ws.project = Some(crate::workspace::OpenProject {
            revision:None,
            path: "source/Project.arproj".into(),
            project: Project {
                name: "Recovery test".into(),
                sheets: vec!["Main.arsch".into()],
                pcb: Some("Board.arpcb".into()),
                libraries: vec!["Parts.arlib".into()],
                ..Default::default()
            },
        });
        ws.doc_mut(&sch)
            .unwrap()
            .session
            .execute(pcbplannercad_core::Command::Add {
                items: vec![pcbplannercad_core::Item::Wire(
                    pcbplannercad_core::item::Wire {
                        a: pcbplannercad_core::Point::new(0, 0),
                        b: pcbplannercad_core::Point::new(5000000, 0),
                    },
                )],
            })
            .unwrap();
        let before = ws.doc(&sch).unwrap().session.document().clone();
        let b = collect(&ws, false).unwrap().unwrap();
        assert_eq!(b.files.len(), 4);
        let p = io::project_from_str(
            &b.files
                .iter()
                .find(|f| f.path == "Recovered.arproj")
                .unwrap()
                .content,
        )
        .unwrap();
        assert!(p.sheets[0].starts_with("documents/"));
        assert!(p.pcb.unwrap().contains(&pcb));
        assert!(p.libraries[0].contains(&lib));
        assert_eq!(ws.doc(&sch).unwrap().session.document(), &before);
        assert!(ws.doc(&sch).unwrap().session.is_dirty());
        assert!(ws.doc(&sch).unwrap().session.can_undo());
        let saved = b.files.iter().find(|f| f.path.ends_with(".arsch")).unwrap();
        assert_eq!(
            io::document_from_str(DocKind::Schematic, &saved.content)
                .unwrap()
                .len(),
            1
        );
    }
    #[test]
    fn clean_workspace_skips_autocopy_and_missing_dependencies_do_not_create_broken_projects() {
        let mut ws = Workspace::default();
        ws.add_doc(
            DocKind::Pcb,
            Origin::File(None),
            Document::new(DocKind::Pcb),
        );
        assert!(collect(&ws, false).unwrap().is_none());
        ws.project = Some(crate::workspace::OpenProject {
            revision:None,
            path: "Missing.arproj".into(),
            project: Project {
                sheets: vec!["Missing.arsch".into()],
                ..Default::default()
            },
        });
        let b = collect(&ws, true).unwrap().unwrap();
        assert!(b.project.is_none());
        assert!(b.notes.iter().any(|n| n.contains("unavailable")));
    }
}
