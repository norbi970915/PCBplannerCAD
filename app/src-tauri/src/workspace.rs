//! Everything the app has open: documents (each with its own undo history),
//! libraries and the current project. Library entries (symbol units and
//! footprints) are edited through ordinary documents that write their items
//! back into the library after every change.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use pcbplannercad_core::{DocKind, Document, Library, Project, Session, io};
use serde::Serialize;

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
}

pub struct OpenLib {
    pub path: Option<PathBuf>,
    pub lib: Library,
    pub dirty: bool,
    pub readonly: bool,
}

pub struct OpenProject {
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
        self.docs.insert(id.clone(), OpenDoc { kind, origin, session: Session::with_document(doc) });
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

    pub fn save_doc_file(&mut self, id: &str, path: &Path) -> Result<(), String> {
        let d = self.doc_mut(id)?;
        let text = io::document_to_string(d.session.document()).map_err(|e| e.to_string())?;
        std::fs::write(path, text).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
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
        self.libs.insert(id.clone(), OpenLib { path, lib, dirty, readonly: false });
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
        Ok(self.add_lib(lib, Some(path.to_path_buf()), false))
    }

    pub fn save_lib(&mut self, id: &str, path: Option<PathBuf>) -> Result<(), String> {
        let l = self.lib_mut(id)?;
        let target = path.or_else(|| l.path.clone()).ok_or("library has no file yet")?;
        std::fs::write(&target, io::library_to_string(&l.lib)).map_err(|e| format!("cannot write {}: {e}", target.display()))?;
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
            if lib_name.is_some_and(|n| n != l.lib.name) {
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

    pub fn save_project_file(&self) -> Result<(), String> {
        let p = self.project()?;
        std::fs::write(&p.path, io::project_to_string(&p.project)).map_err(|e| format!("cannot write {}: {e}", p.path.display()))
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
