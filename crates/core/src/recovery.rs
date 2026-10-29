//! Local recovery copies. Working files are never replaced by this module.
use crate::{DocKind, io, storage};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};
type Res<T> = Result<T, String>;
const MAX_BYTES: u64 = 128 * 1024 * 1024;
const MAX_FILES: usize = 512;
static SEQUENCE: AtomicU64 = AtomicU64::new(0);
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryFile {
    pub path: String,
    pub source: Option<String>,
    pub content: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Bundle {
    pub title: String,
    pub project: Option<String>,
    pub files: Vec<RecoveryFile>,
    pub notes: Vec<String>,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Record {
    version: u8,
    created_ms: u128,
    checksum: String,
    payload: String,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Info {
    pub id: String,
    pub created_ms: u128,
    pub title: String,
    pub documents: usize,
    pub libraries: usize,
    pub bytes: u64,
    pub notes: Vec<String>,
}
#[derive(Serialize)]
pub struct Listing {
    pub location: String,
    pub entries: Vec<Info>,
    pub issues: Vec<String>,
}
#[derive(Serialize)]
pub struct Restored {
    pub folder: String,
    pub project: Option<String>,
    pub files: Vec<String>,
    pub notes: Vec<String>,
}
fn error(e: impl std::fmt::Display) -> String {
    e.to_string()
}
fn now() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}
pub fn fingerprint(bytes: &[u8]) -> String {
    let mut h = 0xcbf29ce484222325u64;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}
fn safe_path(path: &str) -> Res<()> {
    if path.len() > 240
        || path.contains('\\')
        || path
            .split('/')
            .any(|p| crate::project::files::folder_name(p).is_err())
    {
        return Err("Invalid recovery file path".into());
    }
    storage::relative_path(Path::new(path)).map_err(error)
}
fn validate(bundle: &Bundle) -> Res<()> {
    if bundle.files.is_empty() || bundle.files.len() > MAX_FILES {
        return Err("Recovery copy has an invalid file count".into());
    }
    let mut names = std::collections::BTreeSet::new();
    let mut size = 0u64;
    for file in &bundle.files {
        safe_path(&file.path)?;
        if !names.insert(file.path.to_lowercase()) {
            return Err("Duplicate recovery file path".into());
        }
        size += file.content.len() as u64;
        if size > MAX_BYTES {
            return Err("Recovery copy exceeds 128 MiB".into());
        }
        match Path::new(&file.path).extension().and_then(|s| s.to_str()) {
            Some("arsch") => {
                io::document_from_str(DocKind::Schematic, &file.content).map_err(error)?;
            }
            Some("arpcb") => {
                io::document_from_str(DocKind::Pcb, &file.content).map_err(error)?;
            }
            Some("arlib") => {
                io::library_from_str(&file.content).map_err(error)?;
            }
            Some("arproj") => {
                if bundle.project.as_deref() != Some(file.path.as_str()) || file.path.contains('/')
                {
                    return Err("Recovery project metadata must be at the copy root".into());
                }
                let p = io::project_from_str(&file.content).map_err(error)?;
                for rel in p
                    .sheets
                    .iter()
                    .chain(p.pcb.iter())
                    .chain(p.libraries.iter())
                {
                    safe_path(rel)?;
                    if !bundle.files.iter().any(|f| f.path == *rel) {
                        return Err("Recovery project references a missing file".into());
                    }
                }
            }
            _ => return Err("Unsupported recovery file type".into()),
        }
    }
    if let Some(project) = &bundle.project {
        if !project.ends_with(".arproj") || !bundle.files.iter().any(|f| f.path == *project) {
            return Err("Recovery project is missing".into());
        }
    }
    Ok(())
}
fn info(id: String, record: &Record, bundle: &Bundle) -> Info {
    Info {
        id,
        created_ms: record.created_ms,
        title: bundle.title.clone(),
        documents: bundle
            .files
            .iter()
            .filter(|f| f.path.ends_with(".arsch") || f.path.ends_with(".arpcb"))
            .count(),
        libraries: bundle
            .files
            .iter()
            .filter(|f| f.path.ends_with(".arlib"))
            .count(),
        bytes: record.payload.len() as u64,
        notes: bundle.notes.clone(),
    }
}
fn regular(path: &Path) -> Res<()> {
    let m = fs::symlink_metadata(path).map_err(error)?;
    if m.file_type().is_symlink() || !m.is_file() || m.len() > MAX_BYTES * 2 {
        return Err("Recovery record must be a regular file within the size limit".into());
    }
    Ok(())
}
fn read(root: &Path, id: &str) -> Res<(Record, Bundle)> {
    safe_path(id)?;
    let parts: Vec<_> = id.split('/').collect();
    if parts.len() != 2
        || !parts[0].starts_with("s-")
        || !parts[1].starts_with("r-")
        || !parts[1].ends_with(".pcad-recovery")
    {
        return Err("Invalid recovery record ID".into());
    }
    let dir = root.join(parts[0]);
    if fs::symlink_metadata(&dir)
        .map_err(error)?
        .file_type()
        .is_symlink()
    {
        return Err("Linked recovery folders are not supported".into());
    }
    let path = root.join(id);
    regular(&path)?;
    let record: Record = serde_json::from_slice(&fs::read(path).map_err(error)?).map_err(error)?;
    if record.version != 1 || record.checksum != fingerprint(record.payload.as_bytes()) {
        return Err("Recovery copy is unsupported or damaged".into());
    }
    let bundle: Bundle = serde_json::from_str(&record.payload).map_err(error)?;
    validate(&bundle)?;
    Ok((record, bundle))
}
pub fn save(root: &Path, session: &str, bundle: &Bundle) -> Res<Info> {
    safe_path(session)?;
    if !session.starts_with("s-") || session.contains('/') {
        return Err("Invalid recovery session".into());
    }
    validate(bundle)?;
    fs::create_dir_all(root).map_err(error)?;
    let dir = root.join(session);
    if dir.exists()
        && fs::symlink_metadata(&dir)
            .map_err(error)?
            .file_type()
            .is_symlink()
    {
        return Err("Linked recovery folders are not supported".into());
    }
    fs::create_dir_all(&dir).map_err(error)?;
    let payload = serde_json::to_string(bundle).map_err(error)?;
    if payload.len() as u64 > MAX_BYTES {
        return Err("Recovery copy exceeds 128 MiB".into());
    }
    let record = Record {
        version: 1,
        created_ms: now(),
        checksum: fingerprint(payload.as_bytes()),
        payload,
    };
    let name = format!(
        "r-{}-{:06}.pcad-recovery",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    );
    storage::atomic_write(
        &dir.join(&name),
        serde_json::to_vec(&record).map_err(error)?,
    )
    .map_err(error)?;
    let mut older: Vec<_> = fs::read_dir(&dir)
        .map_err(error)?
        .filter_map(Result::ok)
        .filter(|e| {
            e.file_type().is_ok_and(|t| t.is_file())
                && e.file_name().to_string_lossy().ends_with(".pcad-recovery")
        })
        .map(|e| e.path())
        .collect();
    older.retain(|p| p.file_name().is_none_or(|n| n != name.as_str()));
    older.sort();
    let remove = older.len().saturating_sub(2);
    for path in older.into_iter().take(remove) {
        let _ = fs::remove_file(path);
    }
    Ok(info(format!("{session}/{name}"), &record, bundle))
}
pub fn list(root: &Path) -> Res<Listing> {
    let mut out = Listing {
        location: root.to_string_lossy().into_owned(),
        entries: vec![],
        issues: vec![],
    };
    if !root.exists() {
        return Ok(out);
    }
    let mut scanned = 0;
    let mut sessions: Vec<_> = fs::read_dir(root)
        .map_err(error)?
        .collect::<Result<_, _>>()
        .map_err(error)?;
    sessions.sort_by_key(|d| {
        std::cmp::Reverse(
            d.file_name()
                .to_string_lossy()
                .rsplit('-')
                .next()
                .and_then(|p| p.parse::<u128>().ok())
                .unwrap_or(0),
        )
    });
    for dir in sessions {
        if !dir.file_type().map_err(error)?.is_dir()
            || !dir.file_name().to_string_lossy().starts_with("s-")
        {
            continue;
        }
        let entries = match fs::read_dir(dir.path()) {
            Ok(entries) => entries,
            Err(e) => {
                out.issues
                    .push(format!("Cannot read recovery session: {e}"));
                continue;
            }
        };
        for entry in entries {
            let entry = entry.map_err(error)?;
            if !entry
                .file_name()
                .to_string_lossy()
                .ends_with(".pcad-recovery")
            {
                continue;
            }
            scanned += 1;
            if scanned > 256 {
                out.issues
                    .push("Only the first 256 recovery records were scanned.".into());
                out.entries.sort_by(|a, b| b.created_ms.cmp(&a.created_ms));
                return Ok(out);
            }
            let id = format!(
                "{}/{}",
                dir.file_name().to_string_lossy(),
                entry.file_name().to_string_lossy()
            );
            match read(root, &id) {
                Ok((r, b)) => out.entries.push(info(id, &r, &b)),
                Err(e) => out.issues.push(format!("{id}: {e}")),
            }
        }
    }
    out.entries.sort_by(|a, b| b.created_ms.cmp(&a.created_ms));
    Ok(out)
}
pub fn restore(root: &Path, id: &str, dest: &Path) -> Res<Restored> {
    let (_, bundle) = read(root, id)?;
    let mut files: Vec<(PathBuf, Vec<u8>)> = bundle
        .files
        .iter()
        .map(|f| (PathBuf::from(&f.path), f.content.as_bytes().to_vec()))
        .collect();
    let mut notes=vec!["Recovered native files. Originals were not overwritten.".to_string(),"Undo history and external 3D model assets are not included. Keep the original project/library asset folders and relink relative model paths after restoring.".to_string()];
    notes.extend(bundle.notes.clone());
    let mut readme = notes.join("\n");
    readme.push_str("\n\nOriginal locations:\n");
    for f in &bundle.files {
        readme.push_str(&format!(
            "{}: {}\n",
            f.path,
            f.source.as_deref().unwrap_or("Untitled")
        ));
    }
    files.push((PathBuf::from("RECOVERY-NOTES.txt"), readme.into_bytes()));
    storage::create_bundle(dest, &files).map_err(error)?;
    Ok(Restored {
        folder: dest.to_string_lossy().into_owned(),
        project: bundle
            .project
            .as_ref()
            .map(|p| dest.join(p).to_string_lossy().into_owned()),
        files: bundle
            .files
            .iter()
            .map(|f| dest.join(&f.path).to_string_lossy().into_owned())
            .collect(),
        notes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Document, Library, Project};
    struct Temp(PathBuf);
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn temp() -> Temp {
        let path = std::env::temp_dir().join(format!(
            "pcad-recovery-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&path).unwrap();
        Temp(path)
    }
    fn bundle() -> Bundle {
        let p = Project {
            name: "Recovered".into(),
            sheets: vec!["documents/Main.arsch".into()],
            pcb: Some("documents/Board.arpcb".into()),
            libraries: vec!["libraries/Parts.arlib".into()],
            ..Default::default()
        };
        Bundle {
            title: "Unsaved design".into(),
            project: Some("Recovered.arproj".into()),
            notes: vec!["Synthetic recovery fixture".into()],
            files: vec![
                RecoveryFile {
                    path: "documents/Main.arsch".into(),
                    source: Some("original/Main.arsch".into()),
                    content: io::document_to_string(&Document::new(DocKind::Schematic)).unwrap(),
                },
                RecoveryFile {
                    path: "documents/Board.arpcb".into(),
                    source: Some("original/Board.arpcb".into()),
                    content: io::document_to_string(&Document::new(DocKind::Pcb)).unwrap(),
                },
                RecoveryFile {
                    path: "libraries/Parts.arlib".into(),
                    source: None,
                    content: io::library_to_string(&Library {
                        name: "Unsaved parts".into(),
                        ..Default::default()
                    }),
                },
                RecoveryFile {
                    path: "Recovered.arproj".into(),
                    source: Some("original/Design.arproj".into()),
                    content: io::project_to_string(&p),
                },
            ],
        }
    }
    #[test]
    fn persistent_copies_restore_linked_native_files_and_never_replace_originals() {
        let t = temp();
        let root = t.0.join("store");
        let original = t.0.join("Original.arpcb");
        fs::write(&original, b"newer saved original").unwrap();
        let info = save(&root, "s-test", &bundle()).unwrap();
        let records = list(&root).unwrap();
        assert_eq!(records.entries.len(), 1);
        assert_eq!(records.entries[0].documents, 2);
        assert_eq!(records.entries[0].libraries, 1);
        let dest = t.0.join("Recovered");
        let result = restore(&root, &info.id, &dest).unwrap();
        assert!(result.project.unwrap().ends_with("Recovered.arproj"));
        for file in &bundle().files {
            assert_eq!(
                fs::read_to_string(dest.join(&file.path)).unwrap(),
                file.content
            );
        }
        assert!(
            fs::read_to_string(dest.join("RECOVERY-NOTES.txt"))
                .unwrap()
                .contains("external 3D model")
        );
        assert_eq!(fs::read(original).unwrap(), b"newer saved original");
        assert!(restore(&root, &info.id, &dest).is_err());
        assert_eq!(fs::read_dir(&dest).unwrap().count(), 4);
    }
    #[test]
    fn damaged_records_are_reported_without_hiding_other_copies_or_publishing_partial_data() {
        let t = temp();
        let good = save(&t.0, "s-first", &bundle()).unwrap();
        let bad = save(&t.0, "s-second", &bundle()).unwrap();
        let path = t.0.join(&bad.id);
        let mut r: Record = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        r.payload.push(' ');
        fs::write(path, serde_json::to_vec(&r).unwrap()).unwrap();
        let records = list(&t.0).unwrap();
        assert_eq!(records.entries.len(), 1);
        assert_eq!(records.entries[0].id, good.id);
        assert_eq!(records.issues.len(), 1);
        let dest = t.0.join("Failed");
        assert!(restore(&t.0, &bad.id, &dest).is_err());
        assert!(!dest.exists());
    }
    #[test]
    fn rolling_retention_keeps_three_copies_and_failed_capture_keeps_the_previous_set() {
        let t = temp();
        for n in 0..5 {
            let mut b = bundle();
            b.title = format!("Version {n}");
            save(&t.0, "s-one", &b).unwrap();
        }
        let records = list(&t.0).unwrap();
        assert_eq!(records.entries.len(), 3);
        assert!(
            !records
                .entries
                .iter()
                .any(|r| r.title == "Version 0" || r.title == "Version 1")
        );
        let mut invalid = bundle();
        invalid.files[0].content = "broken".into();
        assert!(save(&t.0, "s-one", &invalid).is_err());
        assert_eq!(list(&t.0).unwrap().entries.len(), 3);
        save(&t.0, "s-two", &bundle()).unwrap();
        assert_eq!(list(&t.0).unwrap().entries.len(), 4);
    }
    #[test]
    fn traversal_duplicate_names_missing_dependencies_and_malformed_native_files_are_rejected() {
        let t = temp();
        for path in [
            "../Outside.arpcb",
            "C:/Outside.arpcb",
            "documents/../../Bad.arpcb",
            "documents/CON.arpcb",
        ] {
            let mut b = bundle();
            b.files[1].path = path.into();
            assert!(save(&t.0, "s-test", &b).is_err());
        }
        let mut b = bundle();
        b.files.push(b.files[0].clone());
        assert!(save(&t.0, "s-test", &b).is_err());
        let mut b = bundle();
        b.files.remove(0);
        assert!(save(&t.0, "s-test", &b).is_err());
        assert!(restore(&t.0, "../../outside", &t.0.join("Bad")).is_err());
        let too_many = Bundle {
            files: vec![bundle().files[0].clone(); 513],
            ..bundle()
        };
        assert!(save(&t.0, "s-test", &too_many).is_err());
        assert_eq!(fs::read_dir(&t.0).unwrap().count(), 0);
    }
}

#[cfg(test)]
mod clock_tests {
    use super::*;
    #[test]
    fn the_new_copy_survives_retention_even_if_the_clock_moves_backwards() {
        let root = std::env::temp_dir().join(format!(
            "pcad-recovery-clock-{}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let mut bundle = Bundle {
            title: "Old".into(),
            project: None,
            notes: vec![],
            files: vec![RecoveryFile {
                path: "Board.arpcb".into(),
                source: None,
                content: io::document_to_string(&crate::Document::new(DocKind::Pcb)).unwrap(),
            }],
        };
        for n in 0..3 {
            let info = save(&root, "s-test", &bundle).unwrap();
            fs::rename(
                root.join(info.id),
                root.join("s-test")
                    .join(format!("r-999999999999999999999999-{n}.pcad-recovery")),
            )
            .unwrap();
        }
        bundle.title = "Newest work".into();
        let newest = save(&root, "s-test", &bundle).unwrap();
        assert!(root.join(&newest.id).is_file());
        assert_eq!(list(&root).unwrap().entries.len(), 3);
        assert!(
            list(&root)
                .unwrap()
                .entries
                .iter()
                .any(|e| e.title == "Newest work")
        );
        fs::remove_dir_all(root).unwrap();
    }
}
