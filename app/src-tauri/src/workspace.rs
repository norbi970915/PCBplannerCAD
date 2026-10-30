//! Everything the app has open: documents (each with its own undo history),
//! libraries and the current project. Library entries (symbol units and
//! footprints) are edited through ordinary documents that write their items
//! back into the library after every change.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use pcbplannercad_core::{DocKind, Document, Library, Project, Session, io};
use serde::Serialize;
use pcbplannercad_core::storage::{self, FileRevision};

pub const BUILTIN_LIB: &str = "builtin:basic";

#[derive(Clone, Debug, PartialEq)]
pub enum Origin {
    File(Option<PathBuf>),
    Symbol { lib: String, name: String, unit: usize },
    Footprint { lib: String, name: String },
}

pub struct OpenDoc {
    pub kind: DocKind,
    pub origin: Origin,
    pub session: Session,
    pub revision: Option<FileRevision>,
}

pub struct OpenLib {
    pub path: Option<PathBuf>,
    pub lib: Library,
    pub dirty: bool,
    pub readonly: bool,
    pub revision: Option<FileRevision>,
}

pub struct OpenProject {
    pub revision: Option<FileRevision>,
    pub path: PathBuf,
    pub project: Project,
}

impl OpenProject {
    pub fn dir(&self) -> PathBuf {
        self.path.parent().map(Path::to_path_buf).unwrap_or_default()
    }

    pub fn resolve(&self, rel: &str) -> PathBuf {
        self.dir().join(rel)
    }
}

#[derive(Default)]
pub struct Workspace {
    pub docs: BTreeMap<String, OpenDoc>,
    pub libs: BTreeMap<String, OpenLib>,
    pub project: Option<OpenProject>,
    /// Pending schematic → board change orders, per board document.
    pub eco_plans: BTreeMap<String, pcbplannercad_core::pcb::sync::EcoPlan>,
    pub pending_import: Option<(String, pcbplannercad_import::wizard::Prepared)>,
    pub import_generation: u64,
    next_id: u64,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DocInfo {
    pub id: String,
    pub kind: DocKind,
    pub title: String,
    pub path: Option<String>,
    pub dirty: bool,
    pub can_undo: bool,
    pub can_redo: bool,
    pub library: Option<String>,
    pub entry: Option<String>,
    pub unit: Option<usize>,
    pub in_project: bool,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct LibInfo {
    pub id: String,
    pub name: String,
    pub path: Option<String>,
    pub dirty: bool,
    pub readonly: bool,
    pub symbols: usize,
    pub footprints: usize,
    pub components: usize,
    pub in_project: bool,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ProjectInfo {
    pub path: String,
    pub name: String,
    pub sheets: Vec<ProjectSheet>,
    pub libraries: Vec<String>,
    /// The project board, when it has one.
    pub pcb: Option<ProjectSheet>,
    pub variants: Vec<pcbplannercad_core::project::Variant>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSheet {
    pub file: String,
    pub doc_id: Option<String>,
}

pub fn file_name(p: &Path) -> String {
    p.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

fn same_path(a: &Path, b: &Path) -> bool {
    let canon = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    canon(a) == canon(b)
}

impl Workspace {
    pub fn alloc_id(&mut self, prefix: &str) -> String {
        self.next_id += 1;
        format!("{prefix}{}", self.next_id)
    }

    // ---------- documents ----------

    pub fn doc(&self, id: &str) -> Result<&OpenDoc, String> {
        self.docs.get(id).ok_or_else(|| format!("document {id} is not open"))
    }

    pub fn doc_mut(&mut self, id: &str) -> Result<&mut OpenDoc, String> {
        self.docs.get_mut(id).ok_or_else(|| format!("document {id} is not open"))
    }

    pub fn add_doc(&mut self, kind: DocKind, origin: Origin, doc: Document) -> String {
        let id = self.alloc_id("d");
        self.docs.insert(id.clone(), OpenDoc { kind, origin, session: Session::with_document(doc), revision:None });
        id
    }

    pub fn find_doc(&self, origin: &Origin) -> Option<String> {
        self.docs
            .iter()
            .find(|(_, d)| match (&d.origin, origin) {
                (Origin::File(Some(a)), Origin::File(Some(b))) => same_path(a, b),
                (a, b) => a == b && !matches!(a, Origin::File(None)),
            })
            .map(|(id, _)| id.clone())
    }

    fn in_project(&self, path: &Path) -> bool {
        self.project
            .as_ref()
            .is_some_and(|p| p.project.sheets.iter().chain(p.project.pcb.iter()).any(|s| same_path(&p.resolve(s), path)))
    }

    pub fn doc_info(&self, id: &str) -> Result<DocInfo, String> {
        let d = self.doc(id)?;
        let s = &d.session;
        let mut info = DocInfo {
            id: id.to_string(),
            kind: d.kind,
            title: String::new(),
            path: None,
            dirty: s.is_dirty(),
            can_undo: s.can_undo(),
            can_redo: s.can_redo(),
            library: None,
            entry: None,
            unit: None,
            in_project: false,
        };
        match &d.origin {
            Origin::File(path) => {
                info.title = match path {
                    Some(p) => file_name(p),
                    None => format!("Untitled.{}", if d.kind == DocKind::Schematic { "arsch" } else { "arpcb" }),
                };
                info.path = path.as_ref().map(|p| p.to_string_lossy().into_owned());
                info.in_project = path.as_deref().is_some_and(|p| self.in_project(p));
            }
            Origin::Symbol { lib, name, unit } => {
                info.title = name.clone();
                info.library = Some(lib.clone());
                info.entry = Some(name.clone());
                info.unit = Some(*unit);
                info.dirty = self.libs.get(lib).is_some_and(|l| l.dirty);
            }
            Origin::Footprint { lib, name } => {
                info.title = name.clone();
                info.library = Some(lib.clone());
                info.entry = Some(name.clone());
                info.dirty = self.libs.get(lib).is_some_and(|l| l.dirty);
            }
        }
        Ok(info)
    }

    /// Copies a library-entry document's items back into its library.
    /// Returns the library id when one was touched.
    pub fn sync_entry(&mut self, doc_id: &str) -> Result<Option<String>, String> {
        let d = self.doc(doc_id)?;
        let items = d.session.document().item_list();
        match d.origin.clone() {
            Origin::File(_) => Ok(None),
            Origin::Symbol { lib, name, unit } => {
                let l = self.lib_mut(&lib)?;
                let sym = l.lib.symbols.iter_mut().find(|s| s.name == name).ok_or_else(|| format!("symbol {name} no longer exists"))?;
                let u = sym.units.get_mut(unit).ok_or("symbol unit no longer exists")?;
                u.items = items;
                l.dirty = true;
                Ok(Some(lib))
            }
            Origin::Footprint { lib, name } => {
                let l = self.lib_mut(&lib)?;
                let fp = l.lib.footprints.iter_mut().find(|f| f.name == name).ok_or_else(|| format!("footprint {name} no longer exists"))?;
                fp.items = items;
                l.dirty = true;
                Ok(Some(lib))
            }
        }
    }

    /// Closes documents editing entries of `lib` that match `pred`.
    pub fn close_entry_docs(&mut self, lib: &str, pred: impl Fn(&Origin) -> bool) -> Vec<String> {
        let ids: Vec<String> = self
            .docs
            .iter()
            .filter(|(_, d)| match &d.origin {
                Origin::Symbol { lib: l, .. } | Origin::Footprint { lib: l, .. } => l == lib && pred(&d.origin),
                Origin::File(_) => false,
            })
            .map(|(id, _)| id.clone())
            .collect();
        for id in &ids {
            self.docs.remove(id);
        }
        ids
    }

    fn check_save_target(&self, path: &Path, document: Option<&str>, library: Option<&str>) -> Result<(), String> {
        if self.docs.iter().any(|(id,d)|Some(id.as_str())!=document && matches!(&d.origin, Origin::File(Some(p)) if same_path(p,path))) {
            return Err("That file is already open in another document; save to a different path".into());
        }
        if self.libs.iter().any(|(id,l)|Some(id.as_str())!=library && l.path.as_deref().is_some_and(|p|same_path(p,path))) {
            return Err("That file is already open in another library; save to a different path".into());
        }
        if self.project.as_ref().is_some_and(|p|same_path(&p.path,path)) {
            return Err("That file is the open project; save to a different path".into());
        }
        Ok(())
    }

    pub fn ensure_export_target(&self,path:&Path)->Result<(),String>{self.check_save_target(path,None,None)}

    /// Exported files must never replace an open design, library or project.
    pub fn write_export_file(&self, path: &Path, bytes: &[u8]) -> Result<(), String> {
        self.ensure_export_target(path)?;
        pcbplannercad_core::storage::atomic_write(path,bytes).map_err(|e|format!("cannot write {}: {e}",path.display()))
    }

    /// Copies live content without changing origins, project links, dirty state or Undo.
    pub fn save_doc_copy(&self, id: &str, path: &Path) -> Result<(), String> {
        self.ensure_export_target(path)?;
        let d = self.doc(id)?;
        let bytes = match &d.origin {
            Origin::File(_) => {
                io::document_to_string(d.session.document()).map_err(|e| e.to_string())?
            }
            Origin::Symbol { lib, .. } | Origin::Footprint { lib, .. } => {
                io::library_to_string(&self.lib(lib)?.lib)
            }
        };
        storage::atomic_write_if_unchanged(path, bytes, None)
            .map_err(|e| format!("Cannot save copy {}: {e}", path.display()))
    }
    fn doc_revision_for(&self, id: &str, path: &Path) -> Result<Option<FileRevision>, String> {
        self.check_save_target(path, Some(id), None)?;
        let d = self.doc(id)?;
        Ok(match &d.origin {
            Origin::File(Some(current)) if same_path(current, path) => d.revision,
            _ => None,
        })
    }
    pub fn save_doc_file(&mut self, id: &str, path: &Path) -> Result<(), String> {
        let expected = self.doc_revision_for(id, path)?;
        let d = self.doc_mut(id)?;
        let text = io::document_to_string(d.session.document()).map_err(|e| e.to_string())?;
        storage::atomic_write_if_unchanged(path, &text, expected)
            .map_err(|e| format!("cannot write {}: {e}", path.display()))?;
        d.revision = Some(FileRevision::from_bytes(text.as_bytes()));
        d.session.mark_saved();
        d.origin = Origin::File(Some(path.to_path_buf()));
        Ok(())
    }

    // ---------- libraries ----------

    pub fn lib(&self, id: &str) -> Result<&OpenLib, String> {
        self.libs.get(id).ok_or_else(|| format!("library {id} is not open"))
    }

    pub fn lib_mut(&mut self, id: &str) -> Result<&mut OpenLib, String> {
        self.libs.get_mut(id).ok_or_else(|| format!("library {id} is not open"))
    }

    pub fn lib_writable(&mut self, id: &str) -> Result<&mut OpenLib, String> {
        let l = self.lib_mut(id)?;
        if l.readonly {
            return Err(format!("library '{}' is read-only — copy it with Save As to edit", l.lib.name));
        }
        Ok(l)
    }

    pub fn add_lib(&mut self, lib: Library, path: Option<PathBuf>, dirty: bool) -> String {
        let id = self.alloc_id("lib");
        self.libs.insert(id.clone(), OpenLib { path, lib, dirty, readonly: false, revision:None });
        id
    }

    pub fn find_lib_by_path(&self, path: &Path) -> Option<String> {
        self.libs.iter().find(|(_, l)| l.path.as_deref().is_some_and(|p| same_path(p, path))).map(|(id, _)| id.clone())
    }

    pub fn open_lib_file(&mut self, path: &Path) -> Result<String, String> {
        if let Some(id) = self.find_lib_by_path(path) {
            return Ok(id);
        }
        let text = std::fs::read_to_string(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        let lib = io::library_from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        let id=self.add_lib(lib, Some(path.to_path_buf()), false);
        self.lib_mut(&id)?.revision=Some(FileRevision::from_bytes(text.as_bytes()));
        Ok(id)
    }

    pub fn save_lib(&mut self, id: &str, path: Option<PathBuf>) -> Result<(), String> {
        let current = self.lib(id)?;
        let target = path.or_else(|| current.path.clone()).ok_or("library has no file yet")?;
        self.check_save_target(&target,None,Some(id))?;
        let expected=current.path.as_deref().filter(|p|same_path(p,&target)).and(current.revision);
        let l = self.lib_mut(id)?;
        let text=io::library_to_string(&l.lib);
        storage::atomic_write_if_unchanged(&target,&text,expected).map_err(|e| format!("cannot write {}: {e}", target.display()))?;
        l.revision=Some(FileRevision::from_bytes(text.as_bytes()));
        l.path = Some(target);
        l.dirty = false;
        l.readonly = false;
        Ok(())
    }

    pub fn lib_info(&self, id: &str) -> Result<LibInfo, String> {
        let l = self.lib(id)?;
        let in_project = match (&self.project, &l.path) {
            (Some(p), Some(path)) => p.project.libraries.iter().any(|rel| same_path(&p.resolve(rel), path)),
            _ => false,
        };
        Ok(LibInfo {
            id: id.to_string(),
            name: l.lib.name.clone(),
            path: l.path.as_ref().map(|p| p.to_string_lossy().into_owned()),
            dirty: l.dirty,
            readonly: l.readonly,
            symbols: l.lib.symbols.len(),
            footprints: l.lib.footprints.len(),
            components: l.lib.components.len(),
            in_project,
        })
    }

    /// Finds a footprint by `Lib:Name` or plain name (searching `preferred` first).
    pub fn find_footprint(&self, preferred: &str, reference: &str) -> Option<(String, pcbplannercad_core::Footprint)> {
        let (lib_name, name) = match reference.split_once(':') {
            Some((l, n)) => (Some(l), n),
            None => (None, reference),
        };
        let order = std::iter::once(preferred.to_string()).chain(self.libs.keys().cloned());
        for id in order {
            let Some(l) = self.libs.get(&id) else { continue };
            if lib_name.is_some_and(|n| n != l.lib.name && !(id == BUILTIN_LIB && n == "pCAD Basic")) {
                continue;
            }
            if let Some(fp) = l.lib.footprint(name) {
                return Some((id, fp.clone()));
            }
        }
        None
    }

    // ---------- project ----------

    pub fn project(&self) -> Result<&OpenProject, String> {
        self.project.as_ref().ok_or_else(|| "no project is open".to_string())
    }

    /// History requests are bound to the project the user reviewed, not whichever
    /// project happens to be active when an asynchronous request reaches native code.
    pub fn project_at(&self, expected: &Path) -> Result<&OpenProject, String> {
        let project=self.project()?;
        if !same_path(&project.path,expected) { return Err("Project changed; reopen Project revisions and try again".into()); }
        Ok(project)
    }

    /// Publish metadata only after the complete new file has been saved.
    pub fn edit_project(&mut self, edit: impl FnOnce(&mut Project)) -> Result<(), String> {
        let p = self.project()?;
        let mut next = p.project.clone();
        edit(&mut next);
        let text = io::project_to_string(&next);
        storage::atomic_write_if_unchanged(&p.path, &text, p.revision)
            .map_err(|e| format!("Cannot save project: {e}"))?;
        let p = self.project.as_mut().unwrap();
        p.revision = Some(FileRevision::from_bytes(text.as_bytes()));
        p.project = next;
        Ok(())
    }
    pub fn ensure_project_revision(&self) -> Result<(), String> {
        let p = self.project()?;
        storage::verify_revision(&p.path, p.revision)
            .map_err(|e| format!("Cannot save project {}: {e}", p.path.display()))
    }
    pub fn save_project_file(&mut self) -> Result<(), String> {
        self.edit_project(|_| {})
    }
    /// Catch known conflicts before writing any member. Individual publication is
    /// still guarded; I/O errors or later concurrent edits can leave a partial Save All.
    pub fn save_all_files(&mut self) -> Result<Vec<DocInfo>, String> {
        let docs: Vec<_> = self
            .docs
            .iter()
            .filter_map(|(id, d)| match &d.origin {
                Origin::File(Some(p)) if d.session.is_dirty() => Some((id.clone(), p.clone())),
                _ => None,
            })
            .collect();
        let libs: Vec<_> = self
            .libs
            .iter()
            .filter(|(_, l)| l.dirty && !l.readonly && l.path.is_some())
            .map(|(id, l)| (id.clone(), l.path.clone().unwrap()))
            .collect();
        for (id, path) in &docs {
            let expected = self.doc_revision_for(id, path)?;
            storage::verify_revision(path, expected)
                .map_err(|e| format!("Cannot save {}: {e}", path.display()))?;
        }
        for (id, path) in &libs {
            let l = self.lib(id)?;
            self.check_save_target(path, None, Some(id))?;
            storage::verify_revision(path, l.revision)
                .map_err(|e| format!("Cannot save {}: {e}", path.display()))?;
        }
        if self.project.is_some() {
            self.ensure_project_revision()?;
        }
        let mut saved = Vec::new();
        for (id, path) in docs {
            self.save_doc_file(&id, &path)?;
            saved.push(self.doc_info(&id)?);
        }
        for (id, _) in libs {
            self.save_lib(&id, None)?;
        }
        if self.project.is_some() {
            self.save_project_file()?;
        }
        Ok(saved)
    }

    /// Document id of each project sheet (loaded on project open).
    pub fn project_sheet_docs(&self) -> Result<Vec<(String, String)>, String> {
        let p = self.project()?;
        Ok(p.project
            .sheets
            .iter()
            .filter_map(|rel| self.find_doc(&Origin::File(Some(p.resolve(rel)))).map(|id| (rel.clone(), id)))
            .collect())
    }

    pub fn project_info(&self) -> Option<ProjectInfo> {
        let p = self.project.as_ref()?;
        Some(ProjectInfo {
            path: p.path.to_string_lossy().into_owned(),
            name: p.project.name.clone(),
            sheets: p
                .project
                .sheets
                .iter()
                .map(|rel| ProjectSheet { file: rel.clone(), doc_id: self.find_doc(&Origin::File(Some(p.resolve(rel)))) })
                .collect(),
            libraries: p.project.libraries.iter().filter_map(|rel| self.find_lib_by_path(&p.resolve(rel))).collect(),
            pcb: p.project.pcb.as_ref().map(|rel| ProjectSheet { file: rel.clone(), doc_id: self.find_doc(&Origin::File(Some(p.resolve(rel)))) }),
            variants: p.project.variants.clone(),
        })
    }
}

#[cfg(test)]mod file_tests{
    #[test]
    fn history_project_guard_rejects_switches_and_missing_projects(){
        let t=temp();let path=t.0.join("Design.arproj");std::fs::write(&path,b"saved project").unwrap();
        let mut ws=Workspace::default();assert!(ws.project_at(&path).is_err());
        ws.project=Some(OpenProject{revision:None,path:path.clone(),project:Project::default()});
        assert_eq!(ws.project_at(&t.0.join(".").join("Design.arproj")).unwrap().path,path);
        assert!(ws.project_at(&t.0.join("Other.arproj")).err().unwrap().contains("Project changed"));
        assert_eq!(std::fs::read(&path).unwrap(),b"saved project");
    }
    #[test]
    fn exported_text_cannot_replace_live_sources_and_failed_export_keeps_them(){
        let t=temp();let project=t.0.join("Design.arproj");let document=t.0.join("Board.arpcb");let library=t.0.join("Parts.arlib");
        for path in [&project,&document,&library]{std::fs::write(path,b"original").unwrap();}
        let mut ws=Workspace::default();ws.project=Some(OpenProject{revision:None,path:project.clone(),project:Project::default()});
        ws.add_doc(DocKind::Pcb,Origin::File(Some(document.clone())),Document::new(DocKind::Pcb));ws.add_lib(Library::default(),Some(library.clone()),true);
        for path in [&project,&document,&library]{assert!(ws.write_export_file(path,b"changelog").is_err());assert_eq!(std::fs::read(path).unwrap(),b"original");}
        assert!(ws.write_export_file(&t.0,b"changelog").is_err());
        let export=t.0.join("Changes.txt");std::fs::write(&export,b"old log").unwrap();ws.write_export_file(&export,b"new log").unwrap();assert_eq!(std::fs::read(export).unwrap(),b"new log");
    }

    #[test]
    fn library_save_as_rejects_another_open_library_without_losing_either_state(){
        let t=temp();let path=t.0.join("Other.arlib");
        let mut ws=Workspace::default();
        let other=ws.add_lib(Library{name:"Original".into(),..Default::default()},Some(path.clone()),true);
        ws.save_lib(&other,None).unwrap();let bytes=std::fs::read(&path).unwrap();ws.lib_mut(&other).unwrap().dirty=true;
        let id=ws.add_lib(Library{name:"Replacement".into(),..Default::default()},None,true);
        assert!(ws.save_lib(&id,Some(t.0.join(".").join("Other.arlib"))).unwrap_err().contains("another library"));
        assert_eq!(std::fs::read(&path).unwrap(),bytes);assert!(ws.lib(&other).unwrap().dirty);assert!(ws.lib(&id).unwrap().dirty);assert!(ws.lib(&id).unwrap().path.is_none());
    }
    #[test]
    fn library_and_document_saves_cannot_overwrite_other_live_file_kinds(){
        let t=temp();let project=t.0.join("Design.arproj");let document=t.0.join("Board.arpcb");let library=t.0.join("Parts.arlib");
        for path in [&project,&document,&library]{std::fs::write(path,b"preserve").unwrap();}
        let mut ws=Workspace::default();ws.project=Some(OpenProject{revision:None,path:project.clone(),project:Project::default()});
        ws.add_doc(DocKind::Pcb,Origin::File(Some(document.clone())),Document::new(DocKind::Pcb));ws.add_lib(Library::default(),Some(library.clone()),false);
        let id=ws.add_lib(Library::default(),None,true);let doc=ws.add_doc(DocKind::Pcb,Origin::File(None),Document::new(DocKind::Pcb));
        for path in [&project,&document]{assert!(ws.save_lib(&id,Some(path.clone())).is_err());assert_eq!(std::fs::read(path).unwrap(),b"preserve");}
        for path in [&project,&library]{assert!(ws.save_doc_file(&doc,path).is_err());assert_eq!(std::fs::read(path).unwrap(),b"preserve");}
        assert!(ws.lib(&id).unwrap().path.is_none());assert_eq!(ws.doc(&doc).unwrap().origin,Origin::File(None));
    }

    use super::*;
    struct Temp(PathBuf);impl Drop for Temp{fn drop(&mut self){let _=std::fs::remove_dir_all(&self.0);}}
    fn temp()->Temp{let p=std::env::temp_dir().join(format!("pcad-save-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));std::fs::create_dir(&p).unwrap();Temp(p)}
    #[test]
    fn library_entry_sync_and_workspace_save_preserve_per_layer_lands() {
        use pcbplannercad_core::{Item, Command, ItemId, Footprint};
        let t = temp();
        let pad: Item = serde_json::from_str(r#"{"kind":"pad","number":"1","pos":{"x":0,"y":0},"rotation":0,"shape":"rect","width":2000000,"height":2000000,"cornerRatio":0,"padKind":"tht","layer":15,"drill":{"width":800000,"height":800000},"copperShapes":{"33":{"shape":"circle","width":1200000,"height":1200000,"cornerRatio":0}}}"#).unwrap();
        let mut footprint = Footprint::new("P");
        footprint.items.push(pad.clone());
        let mut ws = Workspace::default();
        let lib = ws.add_lib(Library { name: "L".into(), footprints: vec![footprint], ..Default::default() }, None, true);
        let doc = ws.add_doc(DocKind::Footprint, Origin::Footprint { lib: lib.clone(), name: "P".into() }, Document::from_item_list(DocKind::Footprint, [pad.clone()]));
        let mut renamed = pad;
        if let Item::Pad(p) = &mut renamed { p.number = "7".into(); }
        ws.doc_mut(&doc).unwrap().session.execute(Command::Replace { id: ItemId(0), item: renamed.clone() }).unwrap();
        ws.sync_entry(&doc).unwrap();
        let path = t.0.join("L.arlib");
        ws.save_lib(&lib, Some(path.clone())).unwrap();
        let text = std::fs::read_to_string(path).unwrap();
        assert_eq!(serde_json::from_str::<serde_json::Value>(&text).unwrap()["version"], 3);
        let reopened = io::library_from_str(&text).unwrap();
        assert_eq!(reopened.footprints[0].items, vec![renamed]);
        assert!(!ws.lib(&lib).unwrap().dirty);
        ws.doc_mut(&doc).unwrap().session.undo().unwrap();
        ws.sync_entry(&doc).unwrap();
        let Item::Pad(before) = &ws.lib(&lib).unwrap().lib.footprints[0].items[0] else { panic!("pad") };
        assert_eq!(before.number, "1");
        assert_eq!(before.copper_shapes.len(), 1);
    }
    #[test]fn failed_document_save_preserves_origin_dirty_state_and_history(){let t=temp();let mut ws=Workspace::default();let id=ws.add_doc(DocKind::Pcb,Origin::File(None),Document::new(DocKind::Pcb));ws.doc_mut(&id).unwrap().session.mark_dirty();assert!(ws.save_doc_file(&id,&t.0).is_err());let d=ws.doc(&id).unwrap();assert_eq!(d.origin,Origin::File(None));assert!(d.session.is_dirty());assert!(!d.session.can_undo());let path=t.0.join("Board.arpcb");ws.save_doc_file(&id,&path).unwrap();assert!(!ws.doc(&id).unwrap().session.is_dirty());assert!(io::document_from_str(DocKind::Pcb,&std::fs::read_to_string(path).unwrap()).is_ok());}
    #[test]fn failed_library_and_project_changes_keep_live_metadata(){let t=temp();let mut ws=Workspace::default();let id=ws.add_lib(Library{name:"Parts".into(),..Default::default()},None,true);assert!(ws.save_lib(&id,Some(t.0.clone())).is_err());assert!(ws.lib(&id).unwrap().dirty);assert!(ws.lib(&id).unwrap().path.is_none());ws.project=Some(OpenProject{revision:None,path:t.0.clone(),project:Project{name:"Original".into(),..Default::default()}});assert!(ws.edit_project(|p|{p.name="Changed".into();p.libraries.push("lost.arlib".into());}).is_err());assert_eq!(ws.project().unwrap().project.name,"Original");assert!(ws.project().unwrap().project.libraries.is_empty());}
    #[test]fn save_as_cannot_replace_another_live_document(){let t=temp();let path=t.0.join("Other.arpcb");std::fs::write(&path,b"original").unwrap();let mut ws=Workspace::default();ws.add_doc(DocKind::Pcb,Origin::File(Some(path.clone())),Document::new(DocKind::Pcb));let id=ws.add_doc(DocKind::Pcb,Origin::File(None),Document::new(DocKind::Pcb));assert!(ws.save_doc_file(&id,&path).unwrap_err().contains("another document"));assert_eq!(std::fs::read(&path).unwrap(),b"original");}
    #[test]
    fn previous_starter_library_references_resolve_after_brand_change() {
        let mut ws=Workspace::default();
        let library=pcbplannercad_core::libgen::starter::starter_library();let footprint=library.footprints[0].name.clone();
        ws.libs.insert(BUILTIN_LIB.into(),OpenLib {revision:None,path:None,lib:library,dirty:false,readonly:true});
        assert_eq!(ws.find_footprint(BUILTIN_LIB,&format!("pCAD Basic:{footprint}")).unwrap().0,BUILTIN_LIB);
        assert_eq!(ws.find_footprint(BUILTIN_LIB,&format!("PCBPlanner Basic:{footprint}")).unwrap().0,BUILTIN_LIB);
        assert!(ws.find_footprint(BUILTIN_LIB,&format!("Unrelated:{footprint}")).is_none());
    }

}

#[cfg(test)]
mod save_conflict_tests;
