use super::*;
use crate::commands::{docs::open_doc_file, project::load_project};
use pcbplannercad_core::{item::Wire, Command, Item, Point};
struct Temp(PathBuf);
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn temp() -> Temp {
    let path = std::env::temp_dir().join(format!(
        "pcad-conflict-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&path).unwrap();
    Temp(path)
}
fn new_file(ws: &mut Workspace, path: &Path) -> String {
    let id = ws.add_doc(
        DocKind::Schematic,
        Origin::File(None),
        Document::new(DocKind::Schematic),
    );
    ws.save_doc_file(&id, path).unwrap();
    id
}
fn edit(ws: &mut Workspace, id: &str) {
    ws.doc_mut(id)
        .unwrap()
        .session
        .execute(Command::Add {
            items: vec![Item::Wire(Wire {
                a: Point::new(0, 0),
                b: Point::new(1000000, 0),
            })],
        })
        .unwrap();
}
fn contents(ws: &Workspace, id: &str) -> String {
    io::document_to_string(ws.doc(id).unwrap().session.document()).unwrap()
}
#[test]
fn changed_document_is_preserved_with_dirty_and_undo_state_and_save_as_keeps_both() {
    let t = temp();
    let path = t.0.join("Main.arsch");
    let mut ws = Workspace::default();
    let id = new_file(&mut ws, &path);
    edit(&mut ws, &id);
    let live = contents(&ws, &id);
    let origin = ws.doc(&id).unwrap().origin.clone();
    let revision = ws.doc(&id).unwrap().revision;
    std::fs::write(&path, b"new external contents").unwrap();
    assert!(ws
        .save_doc_file(&id, &path)
        .unwrap_err()
        .contains("outside PCBPlanner Studio"));
    assert_eq!(std::fs::read(&path).unwrap(), b"new external contents");
    let d = ws.doc(&id).unwrap();
    assert!(d.session.is_dirty());
    assert!(d.session.can_undo());
    assert_eq!(d.origin, origin);
    assert_eq!(d.revision, revision);
    assert_eq!(contents(&ws, &id), live);
    let copy = t.0.join("My copy.arsch");
    ws.save_doc_file(&id, &copy).unwrap();
    assert_eq!(std::fs::read_to_string(&copy).unwrap(), live);
    assert_eq!(std::fs::read(&path).unwrap(), b"new external contents");
    assert!(!ws.doc(&id).unwrap().session.is_dirty());
    assert!(ws.doc(&id).unwrap().session.can_undo());
    edit(&mut ws, &id);
    ws.save_doc_file(&id, &copy).unwrap();
    assert_eq!(std::fs::read_to_string(copy).unwrap(), contents(&ws, &id));
}
#[test]
fn opening_uses_the_original_disk_bytes_and_close_reload_can_refresh_the_baseline() {
    let t = temp();
    let path = t.0.join("Main.arsch");
    let text = io::document_to_string(&Document::new(DocKind::Schematic)).unwrap();
    // Formatting differences from canonical serialization must not cause false conflicts.
    let text = format!("{text}\n\n");
    std::fs::write(&path, &text).unwrap();
    let mut ws = Workspace::default();
    let id = open_doc_file(&mut ws, &path).unwrap();
    edit(&mut ws, &id);
    ws.save_doc_file(&id, &path).unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), contents(&ws, &id));
    std::fs::remove_file(&path).unwrap();
    edit(&mut ws, &id);
    assert!(ws.save_doc_file(&id, &path).is_err());
    assert!(!path.exists());
    // Closing and reopening deliberately establishes the external version as the baseline.
    ws.docs.remove(&id);
    std::fs::write(&path, text).unwrap();
    let reopened = open_doc_file(&mut ws, &path).unwrap();
    edit(&mut ws, &reopened);
    ws.save_doc_file(&reopened, &path).unwrap();
}
#[test]
fn save_copy_preserves_live_history_original_path_and_project_links() {
    let t = temp();
    let mut ws = Workspace::default();
    let path = t.0.join("Main.arsch");
    let id = new_file(&mut ws, &path);
    edit(&mut ws, &id);
    let original = std::fs::read(&path).unwrap();
    let origin = ws.doc(&id).unwrap().origin.clone();
    let revision = ws.doc(&id).unwrap().revision;
    let live = contents(&ws, &id);
    let project_path = t.0.join("Design.arproj");
    std::fs::write(
        &project_path,
        io::project_to_string(&Project {
            name: "Design".into(),
            sheets: vec!["Main.arsch".into()],
            ..Default::default()
        }),
    )
    .unwrap();
    load_project(&mut ws, &project_path).unwrap();
    let copy = t.0.join("Saved copy.arsch");
    ws.save_doc_copy(&id, &copy).unwrap();
    assert_eq!(std::fs::read_to_string(&copy).unwrap(), live);
    assert_eq!(std::fs::read(&path).unwrap(), original);
    let d = ws.doc(&id).unwrap();
    assert_eq!(d.origin, origin);
    assert_eq!(d.revision, revision);
    assert!(d.session.is_dirty());
    assert!(d.session.can_undo());
    assert_eq!(
        ws.project_info().unwrap().sheets[0].doc_id.as_deref(),
        Some(id.as_str())
    );
    assert!(ws.save_doc_copy(&id, &copy).is_err());
    assert!(ws.save_doc_copy(&id, &path).is_err());
    assert!(ws.save_doc_copy(&id, &project_path).is_err());
    ws.doc_mut(&id).unwrap().session.undo().unwrap();
    assert_ne!(contents(&ws, &id), live);
    assert_eq!(std::fs::read_to_string(copy).unwrap(), live);
}
#[test]
fn library_entry_copy_saves_the_complete_library_without_rebinding_it() {
    let t = temp();
    let mut ws = Workspace::default();
    let lib = Library {
        name: "Editable library".into(),
        ..Default::default()
    };
    let library = ws.add_lib(lib, None, true);
    let entry = ws.add_doc(
        DocKind::Symbol,
        Origin::Symbol {
            lib: library.clone(),
            name: "Symbol".into(),
            unit: 0,
        },
        Document::new(DocKind::Symbol),
    );
    let path = t.0.join("Library copy.arlib");
    ws.save_doc_copy(&entry, &path).unwrap();
    assert_eq!(
        io::library_from_str(&std::fs::read_to_string(path).unwrap())
            .unwrap()
            .name,
        "Editable library"
    );
    let l = ws.lib(&library).unwrap();
    assert!(l.dirty);
    assert!(l.path.is_none());
    assert!(matches!(&ws.doc(&entry).unwrap().origin,Origin::Symbol{lib,..}if lib==&library));
}
#[test]
fn a_new_save_as_cannot_overwrite_an_unopened_existing_file() {
    let t = temp();
    let mut ws = Workspace::default();
    let id = new_file(&mut ws, &t.0.join("Main.arsch"));
    edit(&mut ws, &id);
    let path = t.0.join("Existing.arsch");
    std::fs::write(&path, b"preserve").unwrap();
    let before = ws.doc(&id).unwrap().origin.clone();
    assert!(ws
        .save_doc_file(&id, &path)
        .unwrap_err()
        .contains("new filename"));
    assert_eq!(std::fs::read(path).unwrap(), b"preserve");
    assert_eq!(ws.doc(&id).unwrap().origin, before);
    assert!(ws.doc(&id).unwrap().session.is_dirty());
}
#[test]
fn library_conflicts_reject_save_and_preserve_the_live_library_until_saved_to_a_new_copy() {
    let t = temp();
    let path = t.0.join("Parts.arlib");
    std::fs::write(
        &path,
        io::library_to_string(&Library {
            name: "Original".into(),
            ..Default::default()
        }),
    )
    .unwrap();
    let mut ws = Workspace::default();
    let id = ws.open_lib_file(&path).unwrap();
    let l = ws.lib_mut(&id).unwrap();
    l.lib.name = "My edits".into();
    l.dirty = true;
    let revision = l.revision;
    std::fs::write(&path, b"external library").unwrap();
    assert!(ws.save_lib(&id, None).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"external library");
    let l = ws.lib(&id).unwrap();
    assert!(l.dirty);
    assert_eq!(l.lib.name, "My edits");
    assert_eq!(l.revision, revision);
    let existing = t.0.join("Other.arlib");
    std::fs::write(&existing, b"other").unwrap();
    assert!(ws.save_lib(&id, Some(existing.clone())).is_err());
    assert_eq!(std::fs::read(existing).unwrap(), b"other");
    let copy = t.0.join("Parts copy.arlib");
    ws.save_lib(&id, Some(copy.clone())).unwrap();
    assert!(!ws.lib(&id).unwrap().dirty);
    assert_eq!(
        io::library_from_str(&std::fs::read_to_string(copy).unwrap())
            .unwrap()
            .name,
        "My edits"
    );
    assert_eq!(std::fs::read(path).unwrap(), b"external library");
}
#[test]
fn save_all_preflights_documents_libraries_and_project_before_saving_any_member() {
    for conflict in ["document", "library", "project"] {
        let t = temp();
        let mut ws = Workspace::default();
        let a = t.0.join("First.arsch");
        let b = t.0.join("Second.arsch");
        let first = new_file(&mut ws, &a);
        let second = new_file(&mut ws, &b);
        let lib_path = t.0.join("Parts.arlib");
        let library = ws.add_lib(Library::default(), Some(lib_path.clone()), true);
        ws.save_lib(&library, None).unwrap();
        let project_path = t.0.join("Design.arproj");
        std::fs::write(
            &project_path,
            io::project_to_string(&Project {
                name: "Design".into(),
                ..Default::default()
            }),
        )
        .unwrap();
        load_project(&mut ws, &project_path).unwrap();
        edit(&mut ws, &first);
        edit(&mut ws, &second);
        ws.lib_mut(&library).unwrap().dirty = true;
        let changed = match conflict {
            "document" => &b,
            "library" => &lib_path,
            _ => &project_path,
        };
        std::fs::write(changed, b"external").unwrap();
        let before: Vec<_> = [&a, &b, &lib_path, &project_path]
            .iter()
            .map(|p| std::fs::read(p).unwrap())
            .collect();
        assert!(ws.save_all_files().unwrap_err().contains("outside PCBPlanner Studio"));
        for (path, expected) in [&a, &b, &lib_path, &project_path].iter().zip(before) {
            assert_eq!(std::fs::read(path).unwrap(), expected);
        }
        assert!(ws.doc(&first).unwrap().session.is_dirty());
        assert!(ws.doc(&second).unwrap().session.is_dirty());
        assert!(ws.lib(&library).unwrap().dirty);
    }
}
#[test]
fn project_metadata_conflicts_keep_live_links_and_refresh_after_successful_writes() {
    let t = temp();
    let path = t.0.join("Design.arproj");
    std::fs::write(
        &path,
        io::project_to_string(&Project {
            name: "Original".into(),
            ..Default::default()
        }),
    )
    .unwrap();
    let mut ws = Workspace::default();
    load_project(&mut ws, &path).unwrap();
    ws.edit_project(|p| p.name = "First edit".into()).unwrap();
    ws.edit_project(|p| p.name = "Second edit".into()).unwrap();
    assert_eq!(
        io::project_from_str(&std::fs::read_to_string(&path).unwrap())
            .unwrap()
            .name,
        "Second edit"
    );
    std::fs::write(&path, b"external project").unwrap();
    assert!(ws
        .edit_project(|p| p.sheets.push("Lost.arsch".into()))
        .is_err());
    assert!(ws.project().unwrap().project.sheets.is_empty());
    assert_eq!(ws.project().unwrap().project.name, "Second edit");
    assert_eq!(std::fs::read(path).unwrap(), b"external project");
}
