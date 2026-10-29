//! Non-truncating file replacement and complete new directory bundles.
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
/// In-process content fingerprint for accidental external-edit detection, not authentication.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileRevision {
    bytes: u64,
    hash: u64,
}
impl FileRevision {
    pub fn from_bytes(bytes: &[u8]) -> Self {
        use std::hash::Hasher;
        let mut hash = std::hash::DefaultHasher::new();
        hash.write(bytes);
        Self {
            bytes: bytes.len() as u64,
            hash: hash.finish(),
        }
    }
    pub fn read(path: &Path) -> io::Result<Option<Self>> {
        use std::{hash::Hasher, io::Read};
        match fs::symlink_metadata(path) {
            Ok(m) if !m.is_file() || m.file_type().is_symlink() => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "save destination must be a regular file",
                ))
            }
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e),
        }
        let mut file = File::open(path)?;
        let mut hash = std::hash::DefaultHasher::new();
        let mut bytes = 0;
        let mut buffer = [0u8; 65536];
        loop {
            let n = file.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            hash.write(&buffer[..n]);
            bytes += n as u64;
        }
        Ok(Some(Self {
            bytes,
            hash: hash.finish(),
        }))
    }
}
/// None means a new destination which must still be absent.
pub fn verify_revision(path: &Path, expected: Option<FileRevision>) -> io::Result<()> {
    let actual = FileRevision::read(path)?;
    if actual == expected {
        return Ok(());
    }
    let message = if expected.is_none() {
        "File already exists; choose a new filename. Existing files are preserved."
    } else {
        "File changed or was removed outside pCAD. Your edits are still open; use File > Save Copy with a new filename to keep both versions."
    };
    Err(io::Error::new(io::ErrorKind::AlreadyExists, message))
}
/// Verify before staging and again immediately before publication. This is not a
/// filesystem compare-and-swap or a lock against a simultaneous external writer.
pub fn atomic_write_if_unchanged(
    path: &Path,
    bytes: impl AsRef<[u8]>,
    expected: Option<FileRevision>,
) -> io::Result<()> {
    verify_revision(path, expected)?;
    atomic_write_with_guard(
        path,
        |file| file.write_all(bytes.as_ref()),
        || verify_revision(path, expected),
    )
}
static NEXT: AtomicU64 = AtomicU64::new(0);
fn unique(parent: &Path) -> PathBuf {
    parent.join(format!(
        ".pcad-tmp-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}
struct Staging(PathBuf);
impl Drop for Staging {
    fn drop(&mut self) {
        if self.0.is_dir() {
            let _ = fs::remove_dir_all(&self.0);
        } else {
            let _ = fs::remove_file(&self.0);
        }
    }
}
/// Only relative, non-empty paths without parent traversal are permitted.
pub fn relative_path(path: &Path) -> io::Result<()> {
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "bundle paths must stay inside their folder",
        ));
    }
    Ok(())
}
/// Flush a sibling file before replacing the destination with one rename.
/// Directory-entry durability after power loss depends on the filesystem.
pub fn atomic_write(path: &Path, bytes: impl AsRef<[u8]>) -> io::Result<()> {
    atomic_write_with(path, |file| file.write_all(bytes.as_ref()))
}
fn atomic_write_with(
    path: &Path,
    write: impl FnOnce(&mut File) -> io::Result<()>,
) -> io::Result<()> {
    atomic_write_with_guard(path, write, || Ok(()))
}
fn atomic_write_with_guard(
    path: &Path,
    write: impl FnOnce(&mut File) -> io::Result<()>,
    before_replace: impl FnOnce() -> io::Result<()>,
) -> io::Result<()> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(m) => Some(m),
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => return Err(e),
    };
    if let Some(m) = &metadata {
        if !m.is_file() || m.file_type().is_symlink() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "save destination must be a regular file",
            ));
        }
        if m.permissions().readonly() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "save destination is read-only",
            ));
        }
    }
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let temp_path = unique(parent);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp_path)?;
    let temp = Staging(temp_path);
    let result = (|| {
        write(&mut file)?;
        if let Some(m) = metadata {
            file.set_permissions(m.permissions())?;
        }
        file.sync_all()
    })();
    drop(file);
    result?;
    before_replace()?;
    fs::rename(&temp.0, path)
}
/// Publish a new folder after every file is complete. Existing folders are rejected.
pub fn create_bundle(dest: &Path, files: &[(PathBuf, Vec<u8>)]) -> io::Result<()> {
    if fs::symlink_metadata(dest).is_ok() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "choose a new folder; the destination already exists",
        ));
    }
    let mut names = std::collections::BTreeSet::new();
    for (path, _) in files {
        relative_path(path)?;
        if !names.insert(path.to_string_lossy().to_lowercase()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "duplicate output file name",
            ));
        }
    }
    let parent = dest
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let temp_path = unique(parent);
    fs::create_dir(&temp_path)?;
    let temp = Staging(temp_path);
    for (path, bytes) in files {
        let out = temp.0.join(path);
        if let Some(dir) = out.parent() {
            fs::create_dir_all(dir)?;
        }
        let mut file = OpenOptions::new().write(true).create_new(true).open(out)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    if fs::symlink_metadata(dest).is_ok() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "destination appeared while preparing files",
        ));
    }
    fs::rename(&temp.0, dest)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn sandbox() -> Staging {
        let p = unique(&std::env::temp_dir());
        fs::create_dir(&p).unwrap();
        Staging(p)
    }
    #[test]
    fn interrupted_save_preserves_original_and_removes_staging() {
        let dir = sandbox();
        let file = dir.0.join("design.arpcb");
        fs::write(&file, b"saved").unwrap();
        assert!(atomic_write_with(&file, |f| {
            f.write_all(b"partial")?;
            Err(io::Error::other("interrupted"))
        })
        .is_err());
        assert_eq!(fs::read(&file).unwrap(), b"saved");
        assert_eq!(fs::read_dir(&dir.0).unwrap().count(), 1);
        atomic_write(&file, b"complete").unwrap();
        assert_eq!(fs::read(&file).unwrap(), b"complete");
    }
    #[test]
    fn failed_new_save_leaves_no_partial_file() {
        let dir = sandbox();
        let file = dir.0.join("new.arlib");
        assert!(atomic_write_with(&file, |_| Err(io::Error::other("full"))).is_err());
        assert!(!file.exists());
        assert_eq!(fs::read_dir(&dir.0).unwrap().count(), 0);
    }
    #[test]
    fn read_only_and_directory_destinations_remain_untouched() {
        let dir = sandbox();
        let file = dir.0.join("locked");
        fs::write(&file, b"saved").unwrap();
        let mut p = fs::metadata(&file).unwrap().permissions();
        p.set_readonly(true);
        fs::set_permissions(&file, p).unwrap();
        assert!(atomic_write(&file, b"new").is_err());
        assert_eq!(fs::read(&file).unwrap(), b"saved");
        assert!(atomic_write(&dir.0, b"bad").is_err());
        let mut p = fs::metadata(&file).unwrap().permissions();
        #[allow(clippy::permissions_set_readonly_false)]
        {
            p.set_readonly(false);
        }
        fs::set_permissions(file, p).unwrap();
    }
    #[test]
    fn bundles_reject_conflicts_duplicates_and_traversal() {
        let dir = sandbox();
        let dest = dir.0.join("Project");
        let files = vec![
            (PathBuf::from("libraries/a.arlib"), b"library".to_vec()),
            (PathBuf::from("board.arpcb"), b"board".to_vec()),
        ];
        create_bundle(&dest, &files).unwrap();
        assert_eq!(fs::read(dest.join("board.arpcb")).unwrap(), b"board");
        assert!(create_bundle(&dest, &files).is_err());
        assert!(
            create_bundle(&dir.0.join("bad"), &[(PathBuf::from("../outside"), vec![])]).is_err()
        );
        assert!(create_bundle(
            &dir.0.join("bad"),
            &[(PathBuf::from("A"), vec![]), (PathBuf::from("a"), vec![])]
        )
        .is_err());
        assert!(!dir.0.join("bad").exists());
        assert_eq!(fs::read_dir(&dir.0).unwrap().count(), 1);
    }
}

#[cfg(test)]
mod revision_tests {
    use super::*;
    fn sandbox() -> Staging {
        let p = unique(&std::env::temp_dir());
        fs::create_dir(&p).unwrap();
        Staging(p)
    }
    #[test]
    fn streaming_fingerprint_matches_loaded_bytes_and_detects_equal_length_edits() {
        let dir = sandbox();
        let path = dir.0.join("board.arpcb");
        let mut bytes = vec![b'x'; 150000];
        fs::write(&path, &bytes).unwrap();
        let original = FileRevision::from_bytes(&bytes);
        assert_eq!(FileRevision::read(&path).unwrap(), Some(original));
        bytes[70000] = b'y';
        fs::write(&path, &bytes).unwrap();
        assert_ne!(FileRevision::read(&path).unwrap(), Some(original));
    }
    #[test]
    fn guarded_writes_allow_identical_content_but_reject_changed_deleted_or_appeared_files() {
        let dir = sandbox();
        let path = dir.0.join("board.arpcb");
        atomic_write_if_unchanged(&path, b"original", None).unwrap();
        let original = FileRevision::from_bytes(b"original");
        fs::write(&path, b"original").unwrap(); // Metadata/identity can change with identical content.
        atomic_write_if_unchanged(&path, b"saved edit", Some(original)).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"saved edit");
        fs::write(&path, b"external").unwrap();
        assert!(atomic_write_if_unchanged(&path, b"lost", Some(original)).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"external");
        assert!(atomic_write_if_unchanged(&path, b"overwrite", None).is_err());
        fs::remove_file(&path).unwrap();
        assert!(atomic_write_if_unchanged(&path, b"recreated", Some(original)).is_err());
        assert!(!path.exists());
        assert_eq!(fs::read_dir(&dir.0).unwrap().count(), 0);
    }
    #[test]
    fn a_destination_changed_during_staging_survives_and_the_temp_file_is_removed() {
        let dir = sandbox();
        let path = dir.0.join("board.arpcb");
        fs::write(&path, b"original").unwrap();
        let expected = FileRevision::from_bytes(b"original");
        let result = atomic_write_with_guard(
            &path,
            |file| {
                file.write_all(b"staged edit")?;
                fs::write(&path, b"external update")
            },
            || verify_revision(&path, Some(expected)),
        );
        assert!(result.is_err());
        assert_eq!(fs::read(&path).unwrap(), b"external update");
        assert_eq!(fs::read_dir(&dir.0).unwrap().count(), 1);
        let appeared = dir.0.join("new.arlib");
        assert!(atomic_write_with_guard(
            &appeared,
            |file| {
                file.write_all(b"staged")?;
                fs::write(&appeared, b"other writer")
            },
            || verify_revision(&appeared, None)
        )
        .is_err());
        assert_eq!(fs::read(&appeared).unwrap(), b"other writer");
        assert_eq!(fs::read_dir(&dir.0).unwrap().count(), 2);
    }
}
