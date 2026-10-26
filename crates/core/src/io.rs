//! Native file formats: pretty-printed JSON with items sorted by id, so files
//! diff cleanly under version control.
//!
//! | Extension | Format tag       | Content                     |
//! |-----------|------------------|-----------------------------|
//! | `.arpcb`  | `pcbplannercad.pcb`   | board document              |
//! | `.arsch`  | `pcbplannercad.sch`   | schematic sheet + part cache|
//! | `.arlib`  | `pcbplannercad.lib`   | component library           |
//! | `.arproj` | `pcbplannercad.proj`  | project                     |

use std::collections::BTreeMap;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::document::{DocKind, Document, IdItem};
use crate::error::CoreError;
use crate::library::{CachedPart, Library};
use crate::project::Project;

pub const VERSION: u32 = 1;

pub const PCB_FORMAT: &str = "pcbplannercad.pcb";
pub const SCH_FORMAT: &str = "pcbplannercad.sch";
pub const LIB_FORMAT: &str = "pcbplannercad.lib";
pub const PROJECT_FORMAT: &str = "pcbplannercad.proj";

pub fn format_for(kind: DocKind) -> Option<&'static str> {
    match kind {
        DocKind::Pcb => Some(PCB_FORMAT),
        DocKind::Schematic => Some(SCH_FORMAT),
        // Symbols and footprints live inside libraries.
        DocKind::Symbol | DocKind::Footprint => None,
    }
}

#[derive(Serialize, Deserialize)]
struct Envelope<T> {
    format: String,
    version: u32,
    #[serde(flatten)]
    body: T,
}

fn write<T: Serialize>(format: &str, body: T) -> String {
    serde_json::to_string_pretty(&Envelope { format: format.into(), version: VERSION, body }).expect("serialization cannot fail")
}

fn read<T: DeserializeOwned>(format: &str, s: &str) -> Result<T, CoreError> {
    #[derive(Deserialize)]
    struct Header {
        format: String,
        version: u32,
    }
    let header: Header = serde_json::from_str(s)?;
    // Keep existing designs readable; all newly saved files use the current brand.
    let legacy = format.strip_prefix("pcbplannercad.").map(|kind| format!("allroute.{kind}"));
    if header.format != format && legacy.as_deref() != Some(header.format.as_str()) {
        return Err(CoreError::UnsupportedFile(format!("expected format '{format}', found '{}'", header.format)));
    }
    if header.version > VERSION {
        return Err(CoreError::UnsupportedFile(format!("file version {} is newer than supported ({VERSION})", header.version)));
    }
    Ok(serde_json::from_str::<Envelope<T>>(s)?.body)
}

#[derive(Serialize, Deserialize)]
struct DocBody {
    items: Vec<IdItem>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    parts: BTreeMap<String, CachedPart>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    footprints: BTreeMap<String, crate::library::Footprint>,
}

pub fn document_to_string(doc: &Document) -> Result<String, CoreError> {
    let format = format_for(doc.kind()).ok_or_else(|| CoreError::UnsupportedFile("library entries are saved with their library".into()))?;
    let items = doc.items().map(|(id, item)| IdItem { id, item: item.clone() }).collect();
    Ok(write(format, DocBody { items, parts: doc.cache().clone(), footprints: doc.footprint_cache().clone() }))
}

pub fn document_from_str(kind: DocKind, s: &str) -> Result<Document, CoreError> {
    let format = format_for(kind).ok_or_else(|| CoreError::UnsupportedFile("not a standalone document kind".into()))?;
    let body: DocBody = read(format, s)?;
    let mut items = BTreeMap::new();
    for IdItem { id, item } in body.items {
        item.validate()?;
        items.insert(id, item);
    }
    Ok(Document::from_parts(kind, items, body.parts).with_footprints(body.footprints))
}

/// Document kind implied by a file extension.
pub fn kind_for_extension(ext: &str) -> Option<DocKind> {
    match ext.to_ascii_lowercase().as_str() {
        "arpcb" => Some(DocKind::Pcb),
        "arsch" => Some(DocKind::Schematic),
        _ => None,
    }
}

pub fn library_to_string(lib: &Library) -> String {
    write(LIB_FORMAT, lib)
}

pub fn library_from_str(s: &str) -> Result<Library, CoreError> {
    read(LIB_FORMAT, s)
}

pub fn project_to_string(p: &Project) -> String {
    write(PROJECT_FORMAT, p)
}

pub fn project_from_str(s: &str) -> Result<Project, CoreError> {
    read(PROJECT_FORMAT, s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{ItemId, LayerId};
    use crate::item::{Item, Line, Part};
    use crate::library::{Component, Symbol};
    use crate::units::{Point, mm};
    use crate::{Command, Session};

    #[test]
    fn document_round_trip_preserves_ids_and_continues_id_allocation() {
        let mut s = Session::new(DocKind::Pcb);
        let item = Item::Line(Line { a: Point::new(0, 0), b: Point::new(mm(5.0), 0), width: mm(0.2), layer: LayerId(1) });
        s.execute(Command::Add { items: vec![item.clone(), item.clone()] }).unwrap();

        let text = document_to_string(s.document()).unwrap();
        let mut loaded = Session::with_document(document_from_str(DocKind::Pcb, &text).unwrap());
        assert_eq!(loaded.document(), s.document());

        let new_id = loaded.execute(Command::Add { items: vec![item] }).unwrap().upserted[0].id;
        assert_eq!(new_id, ItemId(2));
    }

    #[test]
    fn schematic_keeps_part_cache() {
        let mut s = Session::new(DocKind::Schematic);
        let key = s.put_cached_part(CachedPart {
            library: "Basic".into(),
            component: Component {
                name: "R".into(),
                description: String::new(),
                symbol: "R".into(),
                footprints: vec![],
                pin_map: vec![],
                parameters: vec![],
            },
            symbol: Symbol::new("R", "R"),
        });
        s.execute(Command::Add {
            items: vec![Item::Part(Part { pos: Point::default(), rotation: 0.0, mirror: false, lib_ref: key.clone(), unit: 1, fields: vec![] })],
        })
        .unwrap();
        let loaded = document_from_str(DocKind::Schematic, &document_to_string(s.document()).unwrap()).unwrap();
        assert!(loaded.cached_part(&key).is_some());
        assert_eq!(&loaded, s.document());
    }

    #[test]
    fn rejects_foreign_files() {
        assert!(document_from_str(DocKind::Pcb, r#"{"format":"other","version":1,"items":[]}"#).is_err());
        assert!(document_from_str(DocKind::Pcb, r#"{"format":"pcbplannercad.pcb","version":99,"items":[]}"#).is_err());
        assert!(document_from_str(DocKind::Schematic, r#"{"format":"pcbplannercad.pcb","version":1,"items":[]}"#).is_err());
    }

    #[test]
    fn library_and_project_round_trip() {
        let lib = Library { name: "Basic".into(), symbols: vec![Symbol::new("R", "R")], ..Default::default() };
        assert_eq!(library_from_str(&library_to_string(&lib)).unwrap(), lib);
        let p = Project { name: "Demo".into(), sheets: vec!["main.arsch".into()], pcb: None, libraries: vec![], variants: vec![] };
        assert_eq!(project_from_str(&project_to_string(&p)).unwrap(), p);
    }

    #[test]
    fn legacy_files_load_and_save_with_current_format_tags() {
        for kind in [DocKind::Pcb, DocKind::Schematic] {
            let doc = Document::new(kind);
            let current = document_to_string(&doc).unwrap();
            assert!(current.contains("pcbplannercad."));
            let legacy = current.replace("pcbplannercad.", "allroute.");
            let loaded = document_from_str(kind, &legacy).unwrap();
            assert_eq!(loaded, doc);
            assert_eq!(document_to_string(&loaded).unwrap(), current);
        }
        let lib = Library::default();
        let current = library_to_string(&lib);
        let legacy = current.replace("pcbplannercad.", "allroute.");
        assert_eq!(library_to_string(&library_from_str(&legacy).unwrap()), current);
        let project = Project::default();
        let current = project_to_string(&project);
        let legacy = current.replace("pcbplannercad.", "allroute.");
        assert_eq!(project_to_string(&project_from_str(&legacy).unwrap()), current);
        // Accepting the old brand must not let files cross document kinds.
        assert!(document_from_str(DocKind::Schematic, r#"{"format":"allroute.pcb","version":1,"items":[]}"#).is_err());
    }
}
