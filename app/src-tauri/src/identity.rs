//! Non-destructive local-data transition to the personal app identity.
use std::{fs::{self, OpenOptions}, io, path::Path};
use tauri::Manager;

const PREVIOUS_IDS: [&str; 2] = ["com.pcbplanner.pcad", "eu.allengra.pcbplannercad"];
pub struct MigrationStatus(pub Vec<String>);

fn linked(meta: &fs::Metadata) -> bool {
    #[cfg(windows)]
    { use std::os::windows::fs::MetadataExt; meta.file_attributes() & 0x400 != 0 }
    #[cfg(not(windows))]
    { meta.file_type().is_symlink() }
}

fn copy_missing(source: &Path, target: &Path, skip_locks: bool) -> io::Result<()> {
    if skip_locks && source.file_name().is_some_and(|n| n == "LOCK") { return Ok(()); }
    if let Ok(meta) = fs::symlink_metadata(target) { if linked(&meta) { return Err(io::Error::other("linked destination paths are not migrated automatically")); } }
    for ancestor in target.ancestors() { if let Ok(meta) = fs::symlink_metadata(ancestor) { if linked(&meta) { return Err(io::Error::other("linked destination paths are not migrated automatically")); } } }
    let meta = fs::symlink_metadata(source)?;
    if linked(&meta) { return Err(io::Error::other("linked data paths are not migrated automatically")); }
    if meta.is_dir() {
        fs::create_dir_all(target)?;
        for entry in fs::read_dir(source)? { let entry = entry?; copy_missing(&entry.path(), &target.join(entry.file_name()), skip_locks)?; }
    } else if meta.is_file() {
        let mut input = fs::File::open(source)?;
        match OpenOptions::new().write(true).create_new(true).open(target) {
            Ok(mut output) => { io::copy(&mut input, &mut output)?; output.sync_all()?; }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => (),
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

// Never merge two browser databases. Publish each new copy only after the full tree copied.
fn copy_new_tree(source: &Path, target: &Path) -> io::Result<()> {
    if !source.exists() || target.exists() { return Ok(()); }
    let parent = target.parent().ok_or_else(|| io::Error::other("missing migration parent"))?;
    for ancestor in parent.ancestors() { if let Ok(meta) = fs::symlink_metadata(ancestor) { if linked(&meta) { return Err(io::Error::other("linked destination paths are not migrated automatically")); } } }
    fs::create_dir_all(parent)?;
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos();
    let temporary = parent.join(format!(".pcad-migration-{}-{stamp}", std::process::id()));
    copy_missing(source, &temporary, true)?;
    if target.exists() { return Err(io::Error::other("newer app data appeared during migration; original data were kept")); }
    fs::rename(temporary, target)
}

fn migrate_root(previous: &Path, current: &Path) -> Vec<String> {
    if !previous.is_dir() || previous == current { return Vec::new(); }
    let mut messages = Vec::new();
    let recovery = previous.join("recovery");
    if recovery.exists() {
        if let Err(error) = copy_missing(&recovery, &current.join("recovery"), false) {
            messages.push(format!("Previous recovery copies remain available at {}. Automatic copy was incomplete: {error}", recovery.display()));
        }
    }
    let browser = previous.join("EBWebView");
    if browser.is_dir() {
        let lock = browser.join("Default/Local Storage/leveldb/LOCK");
        let mut options = OpenOptions::new(); options.read(true);
        #[cfg(windows)]
        { use std::os::windows::fs::OpenOptionsExt; options.share_mode(0); }
        let guard = if lock.exists() { match options.open(&lock) {
            Ok(file) => Some(file),
            Err(_) => { messages.push("Close the previous app and restart PCBPlanner Studio to copy its saved preferences. Previous data were not changed.".into()); return messages; }
        }} else { None };
        // Only saved browser state is migrated; caches, diagnostics and downloaded runtime data stay behind.
        for relative in ["Default/Local Storage", "Default/Session Storage", "Default/IndexedDB"] {
            if let Err(error) = copy_new_tree(&browser.join(relative), &current.join("EBWebView").join(relative)) {
                messages.push(format!("Previous {relative} was preserved. Automatic migration needs attention: {error}"));
            }
        }
        drop(guard);
    }
    messages
}

pub fn migrate(app: &tauri::AppHandle) -> Vec<String> {
    let mut messages = Vec::new();
    let paths = app.path();
    if let (Ok(base), Ok(current)) = (paths.local_data_dir(), paths.app_local_data_dir()) {
        for previous in PREVIOUS_IDS { messages.extend(migrate_root(&base.join(previous), &current)); }
    }
    if let (Ok(base), Ok(current)) = (paths.data_dir(), paths.app_data_dir()) {
        for previous in PREVIOUS_IDS { messages.extend(migrate_root(&base.join(previous), &current)); }
    }
    messages
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    struct Temp(PathBuf);
    impl Temp { fn new() -> Self { let stamp=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos(); let p=std::env::temp_dir().join(format!("pcad-id-migration-{}-{stamp}",std::process::id())); fs::create_dir_all(&p).unwrap();Self(p) } }
    impl Drop for Temp { fn drop(&mut self) { if self.0.starts_with(std::env::temp_dir()) && self.0.file_name().unwrap().to_string_lossy().starts_with("pcad-id-migration-") { let _=fs::remove_dir_all(&self.0); } } }
    #[test] fn recovery_is_copied_without_overwriting_or_removing_originals() {
        let t=Temp::new();let old=t.0.join("old");let new=t.0.join("new");fs::create_dir_all(old.join("recovery")).unwrap();fs::create_dir_all(new.join("recovery")).unwrap();
        fs::write(old.join("recovery/saved"),"old").unwrap();fs::write(old.join("recovery/new-copy"),"preserve").unwrap();fs::write(new.join("recovery/saved"),"newer").unwrap();
        assert!(migrate_root(&old,&new).is_empty());assert_eq!(fs::read_to_string(new.join("recovery/saved")).unwrap(),"newer");assert_eq!(fs::read_to_string(new.join("recovery/new-copy")).unwrap(),"preserve");assert_eq!(fs::read_to_string(old.join("recovery/saved")).unwrap(),"old");
    }
    #[test] fn browser_databases_are_never_merged_with_an_existing_profile() {
        let t=Temp::new();let old=t.0.join("old");let new=t.0.join("new");let rel="EBWebView/Default/Local Storage";fs::create_dir_all(old.join(rel)).unwrap();fs::write(old.join(rel).join("CURRENT"),"first").unwrap();
        assert!(migrate_root(&old,&new).is_empty());assert_eq!(fs::read_to_string(new.join(rel).join("CURRENT")).unwrap(),"first");fs::write(old.join(rel).join("CURRENT"),"changed").unwrap();fs::write(old.join(rel).join("extra"),"not merged").unwrap();
        assert!(migrate_root(&old,&new).is_empty());assert_eq!(fs::read_to_string(new.join(rel).join("CURRENT")).unwrap(),"first");assert!(!new.join(rel).join("extra").exists());
    }
    #[test] fn browser_lock_is_not_copied_but_recovery_lock_named_files_are() {
        let t=Temp::new();let old=t.0.join("old");let new=t.0.join("new");let rel="EBWebView/Default/Local Storage/leveldb";fs::create_dir_all(old.join(rel)).unwrap();
        fs::write(old.join(rel).join("LOCK"),"").unwrap();fs::write(old.join(rel).join("CURRENT"),"database").unwrap();fs::create_dir_all(old.join("recovery")).unwrap();fs::write(old.join("recovery/LOCK"),"user copy").unwrap();
        assert!(migrate_root(&old,&new).is_empty());assert!(!new.join(rel).join("LOCK").exists());assert_eq!(fs::read_to_string(new.join(rel).join("CURRENT")).unwrap(),"database");assert_eq!(fs::read_to_string(new.join("recovery/LOCK")).unwrap(),"user copy");
    }
    #[cfg(windows)]
    #[test] fn active_browser_database_is_preserved_without_copying() {
        use std::os::windows::fs::OpenOptionsExt;
        let t=Temp::new();let old=t.0.join("old");let new=t.0.join("new");let rel="EBWebView/Default/Local Storage/leveldb";fs::create_dir_all(old.join(rel)).unwrap();fs::write(old.join(rel).join("LOCK"),"").unwrap();
        let _guard=OpenOptions::new().read(true).write(true).share_mode(0).open(old.join(rel).join("LOCK")).unwrap();
        assert!(migrate_root(&old,&new).iter().any(|m|m.contains("Close the previous app")));assert!(!new.join("EBWebView").exists());assert!(old.join(rel).join("LOCK").exists());
    }

    #[test]
    #[ignore = "requires an isolated Chromium fixture; run explicitly during installation audit"]
    fn migrates_isolated_chromium_profile_fixture() {
        let root=Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/local-notes/install-audit-2026-10-30/profile-migration");
        assert!(root.join("fixture-ready.json").is_file(),"prepare the isolated browser fixture first");
        let old=root.join("old");let new=root.join("new");assert!(old.join("EBWebView/Default/Local Storage").is_dir());
        let messages=migrate_root(&old,&new);assert!(messages.is_empty(),"{messages:?}");
        assert!(new.join("EBWebView/Default/Local Storage").is_dir());
        assert_eq!(fs::read(new.join("recovery/fixture-saved.txt")).unwrap(),b"saved recovery fixture");
        if old.join("recovery/new-copy.txt").exists(){assert_eq!(fs::read(new.join("recovery/new-copy.txt")).unwrap(),b"later recovery copy");}
    }

}
