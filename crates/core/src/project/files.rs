//! Local folder navigation, complete electronics projects and saved-file revisions.
use crate::{DocKind, Document, Item, Library, Project, io, storage};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
type Res<T> = Result<T, String>;
const HISTORY: &str = ".pcad-history";
fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}
fn now() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}
pub fn folder_name(name: &str) -> Res<()> {
    if name.trim() != name
        || name.is_empty()
        || name.ends_with('.')
        || name
            .chars()
            .any(|c| c.is_control() || "<>:\"/\\|?*".contains(c))
    {
        return Err(
            "Use a non-empty folder name without reserved characters or trailing dots/spaces"
                .into(),
        );
    }
    let base = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    if matches!(base.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (base.len() == 4
            && (base.starts_with("COM") || base.starts_with("LPT"))
            && matches!(base.as_bytes()[3], b'1'..=b'9'))
        || name == ".."
    {
        return Err("That name is reserved; choose another folder name".into());
    }
    Ok(())
}
fn checked_dir(root: &Path, relative: &Path) -> Res<PathBuf> {
    let root = fs::canonicalize(root).map_err(err)?;
    if !relative.as_os_str().is_empty() {
        storage::relative_path(relative).map_err(err)?;
    }
    let dir = fs::canonicalize(root.join(relative)).map_err(err)?;
    if !dir.starts_with(&root) || !dir.is_dir() {
        return Err("Folder must stay inside the working folder".into());
    }
    Ok(dir)
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub name: String,
    pub path: String,
    pub kind: String,
    pub modified_ms: u128,
    pub bytes: u64,
}
pub fn browse(root: &Path, relative: &Path) -> Res<Vec<Entry>> {
    let dir = checked_dir(root, relative)?;
    let mut out = vec![];
    for entry in fs::read_dir(dir).map_err(err)? {
        let e = entry.map_err(err)?;
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let m = fs::symlink_metadata(e.path()).map_err(err)?;
        if m.file_type().is_symlink() {
            continue;
        }
        let kind = if m.is_dir() {
            "folder"
        } else {
            match e
                .path()
                .extension()
                .and_then(|x| x.to_str())
                .unwrap_or("")
                .to_ascii_lowercase()
                .as_str()
            {
                "arproj" => "project",
                "arsch" => "schematic",
                "arpcb" => "board",
                "arlib" => "library",
                "step" | "stp" | "wrl" | "stl" | "obj" => "model",
                _ => "file",
            }
        };
        out.push(Entry {
            name,
            path: e.path().to_string_lossy().into_owned(),
            kind: kind.into(),
            modified_ms: m
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_millis()),
            bytes: m.len(),
        });
    }
    out.sort_by(|a, b| {
        (a.kind != "folder", a.name.to_lowercase())
            .cmp(&(b.kind != "folder", b.name.to_lowercase()))
    });
    Ok(out)
}
pub fn new_folder(root: &Path, relative: &Path, name: &str) -> Res<PathBuf> {
    folder_name(name)?;
    let dir = checked_dir(root, relative)?.join(name);
    fs::create_dir(&dir).map_err(err)?;
    Ok(dir)
}
pub fn new_project(root: &Path, relative: &Path, name: &str) -> Res<PathBuf> {
    folder_name(name)?;
    let dest = checked_dir(root, relative)?.join(name);
    let project = Project {
        name: name.into(),
        sheets: vec!["Schematic.arsch".into()],
        pcb: Some("Board.arpcb".into()),
        libraries: vec!["libraries/Project.arlib".into()],
        variants: vec![],
    };
    let board = Document::from_item_list(DocKind::Pcb, [Item::Board(Default::default())]);
    let files = vec![
        (
            PathBuf::from(format!("{name}.arproj")),
            io::project_to_string(&project).into_bytes(),
        ),
        (
            PathBuf::from("Schematic.arsch"),
            io::document_to_string(&Document::new(DocKind::Schematic))
                .map_err(err)?
                .into_bytes(),
        ),
        (
            PathBuf::from("Board.arpcb"),
            io::document_to_string(&board).map_err(err)?.into_bytes(),
        ),
        (
            PathBuf::from("libraries/Project.arlib"),
            io::library_to_string(&Library {
                name: "Project".into(),
                ..Default::default()
            })
            .into_bytes(),
        ),
        (PathBuf::from("models/.keep"), vec![]),
    ];
    storage::create_bundle(&dest, &files).map_err(err)?;
    Ok(dest.join(format!("{name}.arproj")))
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RevisionInfo {
    pub id: String,
    pub created_ms: u128,
    pub label: String,
    pub files: usize,
    pub bytes: u64,
}
#[derive(Serialize, Deserialize)]
struct SavedFile {
    path: PathBuf,
    bytes: u64,
    checksum: String,
}
#[derive(Serialize, Deserialize)]
struct Manifest {
    version: u32,
    info: RevisionInfo,
    project: PathBuf,
    files: Vec<SavedFile>,
}
fn checksum(bytes: &[u8]) -> String {
    let h = bytes.iter().fold(0xcbf29ce484222325u64, |h, b| {
        (h ^ (*b as u64)).wrapping_mul(0x100000001b3)
    });
    format!("{h:016x}")
}
fn collect(root: &Path, rel: &Path, out: &mut Vec<(PathBuf, Vec<u8>)>, total: &mut u64) -> Res<()> {
    for e in fs::read_dir(root.join(rel)).map_err(err)? {
        let e = e.map_err(err)?;
        let name = e.file_name();
        let n = name.to_string_lossy();
        if n == HISTORY || n == ".git" || n.starts_with(".pcad-tmp-") {
            continue;
        }
        let path = rel.join(name);
        let m = fs::symlink_metadata(e.path()).map_err(err)?;
        if m.file_type().is_symlink() {
            return Err(format!(
                "Cannot checkpoint linked asset {}; copy it into the project first",
                path.display()
            ));
        }
        if m.is_dir() {
            collect(root, &path, out, total)?;
        } else if m.is_file() {
            *total = total
                .checked_add(m.len())
                .ok_or("Checkpoint size overflow")?;
            if *total > 2 * 1024 * 1024 * 1024 || out.len() >= 16384 {
                return Err("Checkpoint exceeds the current size/file limit".into());
            }
            let bytes = fs::read(e.path()).map_err(err)?;
            out.push((path, bytes));
        }
    }
    Ok(())
}
fn project_root(project: &Path) -> Res<PathBuf> {
    fs::canonicalize(project.parent().ok_or("Invalid project path")?).map_err(err)
}
fn history_dir(root: &Path) -> Res<PathBuf> {
    let path = root.join(HISTORY);
    if fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err("Revision storage must be a real directory inside the project".into());
    }
    Ok(path)
}
pub fn checkpoint(project: &Path, label: &str) -> Res<RevisionInfo> {
    let root = project_root(project)?;
    let text = fs::read_to_string(project).map_err(err)?;
    let p = io::project_from_str(&text).map_err(err)?;
    for rel in p
        .sheets
        .iter()
        .chain(p.pcb.iter())
        .chain(p.libraries.iter())
    {
        storage::relative_path(Path::new(rel)).map_err(|_| {
            format!("External project file {rel}; copy it into the project before checkpointing")
        })?;
        if !root.join(rel).is_file() {
            return Err(format!("Missing project file {rel}"));
        }
    }
    for rel in &p.libraries {
        let lib_path = root.join(rel);
        let lib =
            io::library_from_str(&fs::read_to_string(&lib_path).map_err(err)?).map_err(err)?;
        for fp in lib.footprints {
            if let Some(model) = fp.model {
                storage::relative_path(Path::new(&model.path)).map_err(|_| {
                    format!(
                        "External model for {}; copy it into the project before checkpointing",
                        fp.name
                    )
                })?;
                let asset = fs::canonicalize(lib_path.parent().unwrap().join(&model.path))
                    .map_err(|_| {
                        format!(
                            "Missing model for {}; attach a local copy before checkpointing",
                            fp.name
                        )
                    })?;
                if !asset.starts_with(&root) {
                    return Err(format!("Model for {} is outside the project", fp.name));
                }
            }
        }
    }
    for rel in &p.sheets {
        io::document_from_str(
            DocKind::Schematic,
            &fs::read_to_string(root.join(rel)).map_err(err)?,
        )
        .map_err(err)?;
    }
    if let Some(rel) = &p.pcb {
        let doc = io::document_from_str(
            DocKind::Pcb,
            &fs::read_to_string(root.join(rel)).map_err(err)?,
        )
        .map_err(err)?;
        for fp in doc.footprint_cache().values() {
            if let Some(model) = &fp.model {
                storage::relative_path(Path::new(&model.path)).map_err(|_|format!("External cached model for {}; use a project-local model before checkpointing",fp.name))?;
                let mut candidates = vec![root.join(&model.path)];
                for lib in &p.libraries {
                    if let Some(parent) = root.join(lib).parent() {
                        candidates.push(parent.join(&model.path));
                    }
                }
                if !candidates
                    .iter()
                    .any(|c| fs::canonicalize(c).is_ok_and(|p| p.starts_with(&root) && p.is_file()))
                {
                    return Err(format!(
                        "Missing cached model for {}; attach it inside the project before checkpointing",
                        fp.name
                    ));
                }
            }
        }
    }
    let mut files = vec![];
    let mut total = 0;
    collect(&root, Path::new(""), &mut files, &mut total)?;
    files.sort_by(|a, b| a.0.cmp(&b.0));
    let id = format!(
        "{}-{}",
        now(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .subsec_nanos()
    );
    let info = RevisionInfo {
        id: id.clone(),
        created_ms: now(),
        label: label.trim().chars().take(240).collect(),
        files: files.len(),
        bytes: total,
    };
    let manifest = Manifest {
        version: 1,
        info: info.clone(),
        project: PathBuf::from(project.file_name().ok_or("Invalid project filename")?),
        files: files
            .iter()
            .map(|(p, b)| SavedFile {
                path: p.clone(),
                bytes: b.len() as u64,
                checksum: checksum(b),
            })
            .collect(),
    };
    let mut writes: Vec<_> = files
        .into_iter()
        .map(|(p, b)| (Path::new("files").join(p), b))
        .collect();
    writes.push((
        PathBuf::from("manifest.json"),
        serde_json::to_vec_pretty(&manifest).map_err(err)?,
    ));
    let history = history_dir(&root)?;
    fs::create_dir_all(&history).map_err(err)?;
    storage::create_bundle(&history.join(id), &writes).map_err(err)?;
    Ok(info)
}
pub fn revisions(project: &Path) -> Res<Vec<RevisionInfo>> {
    let history = history_dir(&project_root(project)?)?;
    if !history.exists() {
        return Ok(vec![]);
    }
    let mut out = vec![];
    for e in fs::read_dir(history).map_err(err)? {
        let e = e.map_err(err)?;
        if e.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        if e.file_type().map_err(err)?.is_symlink() {
            return Err("Linked revision folder is not supported".into());
        }
        let m: Manifest =
            serde_json::from_slice(&fs::read(e.path().join("manifest.json")).map_err(err)?)
                .map_err(err)?;
        if m.version != 1 {
            return Err("Unsupported revision manifest version".into());
        }
        out.push(m.info);
    }
    out.sort_by(|a, b| b.created_ms.cmp(&a.created_ms));
    Ok(out)
}
/// Restore only into a new folder. The working project and checkpoint stay untouched.
pub fn restore_copy(project: &Path, id: &str, dest: &Path) -> Res<PathBuf> {
    folder_name(id)?;
    let history = history_dir(&project_root(project)?)?;
    let source = checked_dir(&history, Path::new(id))?;
    let m: Manifest = serde_json::from_slice(&fs::read(source.join("manifest.json")).map_err(err)?)
        .map_err(err)?;
    if m.version != 1 || m.info.id != id || m.info.files != m.files.len() {
        return Err("Invalid revision manifest".into());
    }
    storage::relative_path(&m.project).map_err(err)?;
    let mut writes = vec![];
    for f in m.files {
        storage::relative_path(&f.path).map_err(err)?;
        let file = fs::canonicalize(source.join("files").join(&f.path)).map_err(err)?;
        if !file.starts_with(&source) {
            return Err("Revision file escapes checkpoint".into());
        }
        let b = fs::read(file).map_err(err)?;
        if b.len() as u64 != f.bytes || checksum(&b) != f.checksum {
            return Err(format!(
                "Checkpoint file {} is damaged; restore cancelled",
                f.path.display()
            ));
        }
        writes.push((f.path, b));
    }
    let p = writes
        .iter()
        .find(|(p, _)| p == &m.project)
        .ok_or("Checkpoint project file is missing")?;
    io::project_from_str(std::str::from_utf8(&p.1).map_err(err)?).map_err(err)?;
    storage::create_bundle(dest, &writes).map_err(err)?;
    Ok(dest.join(m.project))
}
#[cfg(test)]
mod tests {
    use super::*;
    struct Temp(PathBuf);
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn temp() -> Temp {
        let p = std::env::temp_dir().join(format!(
            "pcad-hub-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&p).unwrap();
        Temp(p)
    }
    #[test]
    fn complete_design_nested_navigation_and_checkpoint_restore() {
        let t = temp();
        new_folder(&t.0, Path::new(""), "Customer").unwrap();
        let p = new_project(&t.0, Path::new("Customer"), "Sensor").unwrap();
        let root = p.parent().unwrap();
        assert!(root.join("Board.arpcb").is_file());
        assert!(root.join("Schematic.arsch").is_file());
        assert!(root.join("libraries/Project.arlib").is_file());
        assert_eq!(
            browse(&t.0, Path::new("Customer")).unwrap()[0].name,
            "Sensor"
        );
        fs::write(root.join("models/body.step"), b"original model").unwrap();
        let r = checkpoint(&p, "Before routing").unwrap();
        assert_eq!(revisions(&p).unwrap().len(), 1);
        fs::write(root.join("models/body.step"), b"new model").unwrap();
        let restored = restore_copy(&p, &r.id, &t.0.join("Recovered")).unwrap();
        assert_eq!(
            fs::read(restored.parent().unwrap().join("models/body.step")).unwrap(),
            b"original model"
        );
        assert_eq!(
            fs::read(root.join("models/body.step")).unwrap(),
            b"new model"
        );
        assert!(restore_copy(&p, &r.id, root).is_err());
        assert_eq!(revisions(&p).unwrap()[0].files, r.files);
    }
    #[test]
    fn damaged_revision_and_unsafe_names_do_not_create_outputs() {
        let t = temp();
        for n in ["..", "A/B", "CON", "Lpt1.txt", "x.", " x"] {
            assert!(new_folder(&t.0, Path::new(""), n).is_err());
        }
        assert!(browse(&t.0, Path::new("..")).is_err());
        let p = new_project(&t.0, Path::new(""), "Test").unwrap();
        assert!(new_project(&t.0, Path::new(""), "Test").is_err());
        let r = checkpoint(&p, "good").unwrap();
        fs::write(
            p.parent()
                .unwrap()
                .join(HISTORY)
                .join(&r.id)
                .join("files/Board.arpcb"),
            b"broken",
        )
        .unwrap();
        assert!(restore_copy(&p, &r.id, &t.0.join("bad")).is_err());
        assert!(!t.0.join("bad").exists());
        assert!(restore_copy(&p, "../escape", &t.0.join("bad")).is_err());
    }

    #[test]
    fn missing_assets_and_malformed_saved_documents_cannot_be_checkpointed() {
        let t = temp();
        let p = new_project(&t.0, Path::new(""), "Assets").unwrap();
        let root = p.parent().unwrap();
        let lib_path = root.join("libraries/Project.arlib");
        let mut lib = io::library_from_str(&fs::read_to_string(&lib_path).unwrap()).unwrap();
        let mut fp = crate::Footprint::new("Case");
        fp.model = Some(crate::library::Model3d {
            path: "models/case.step".into(),
            offset: [0.; 3],
            rotation: [0.; 3],
            scale: [1.; 3],
        });
        lib.footprints.push(fp);
        fs::write(&lib_path, io::library_to_string(&lib)).unwrap();
        assert!(
            checkpoint(&p, "missing")
                .unwrap_err()
                .contains("Missing model")
        );
        assert!(!root.join(HISTORY).exists());
        fs::create_dir_all(root.join("libraries/models")).unwrap();
        fs::write(
            root.join("libraries/models/case.step"),
            b"original model bytes",
        )
        .unwrap();
        let revision = checkpoint(&p, "complete").unwrap();
        fs::remove_file(root.join("libraries/models/case.step")).unwrap();
        let restored = restore_copy(&p, &revision.id, &t.0.join("Recovered")).unwrap();
        assert_eq!(
            fs::read(
                restored
                    .parent()
                    .unwrap()
                    .join("libraries/models/case.step")
            )
            .unwrap(),
            b"original model bytes"
        );
        fs::write(root.join("libraries/models/case.step"), b"returned").unwrap();
        fs::write(root.join("Board.arpcb"), b"malformed saved board").unwrap();
        assert!(checkpoint(&p, "bad board").is_err());
        assert_eq!(revisions(&p).unwrap().len(), 1);
    }
    #[test]
    fn checkpoint_comparison_is_read_only_directional_and_detects_corruption() {
        let t = temp();
        let project = new_project(&t.0, Path::new(""), "Diff").unwrap();
        let root = project.parent().unwrap();
        let first = checkpoint(&project, "Original").unwrap();
        let mut doc = io::document_from_str(
            DocKind::Pcb,
            &fs::read_to_string(root.join("Board.arpcb")).unwrap(),
        )
        .unwrap();
        let mut session = crate::Session::with_document(doc.clone());
        session
            .execute(crate::Command::Add {
                items: vec![Item::Track(crate::pcb::Track {
                    locked: false,
                    net: "SIGNAL".into(),
                    layer: crate::layer::TOP_COPPER,
                    width: 200000,
                    a: crate::Point::new(0, 0),
                    b: crate::Point::new(3000000, 0),
                })],
            })
            .unwrap();
        doc = session.document().clone();
        let saved = io::document_to_string(&doc).unwrap();
        fs::write(root.join("Board.arpcb"), &saved).unwrap();
        fs::write(root.join("models/body.step"), b"local model").unwrap();
        let second = checkpoint(&project, "Routed").unwrap();
        let diff = compare_revisions(&project, &first.id, &second.id).unwrap();
        assert_eq!((diff.added, diff.removed, diff.modified), (1, 0, 1));
        assert!(
            diff.files
                .iter()
                .find(|f| f.path == "Board.arpcb")
                .unwrap()
                .details
                .iter()
                .any(|d| d.contains("Added track"))
        );
        assert_eq!(fs::read_to_string(root.join("Board.arpcb")).unwrap(), saved);
        let reverse = compare_revisions(&project, &second.id, &first.id).unwrap();
        assert_eq!(
            (reverse.added, reverse.removed, reverse.modified),
            (0, 1, 1)
        );
        assert!(
            compare_revisions(&project, &first.id, &first.id)
                .unwrap()
                .files
                .is_empty()
        );
        fs::write(
            root.join(HISTORY)
                .join(&second.id)
                .join("files/Board.arpcb"),
            b"damaged",
        )
        .unwrap();
        assert!(compare_revisions(&project, &first.id, &second.id).is_err());
        assert_eq!(revisions(&project).unwrap().len(), 2);
    }
}

#[path = "changes.rs"]
mod changes;
pub use changes::{FileChange, RevisionChanges, compare_revisions};
