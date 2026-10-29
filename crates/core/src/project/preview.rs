//! Saved-file preview data. Reads never open workspace documents or write designs.
use crate::{
    DocKind, Item, io,
    library::{CachedPart, Footprint, Symbol},
};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};
type Res<T> = Result<T, String>;
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub views: Vec<View>,
    pub model: Option<String>,
    pub dependencies: Vec<String>,
    pub notes: Vec<String>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct View {
    pub label: String,
    pub kind: DocKind,
    pub items: Vec<Item>,
    pub cache: BTreeMap<String, CachedPart>,
    pub footprints: BTreeMap<String, Footprint>,
    pub symbol: Option<Symbol>,
}
fn checked(root: &Path, path: &Path) -> Res<PathBuf> {
    let p = fs::canonicalize(path).map_err(|e| e.to_string())?;
    if !p.starts_with(root) || !p.is_file() {
        return Err("Preview must stay inside the working folder".into());
    }
    if fs::metadata(&p).map_err(|e| e.to_string())?.len() > 32 * 1024 * 1024 {
        return Err(
            "This file is too large for a thumbnail; open it to inspect its contents".into(),
        );
    }
    Ok(p)
}
fn read(root: &Path, path: &Path, out: &mut Preview) -> Res<String> {
    let path = checked(root, path)?;
    let text = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    out.dependencies.push(path.to_string_lossy().into_owned());
    Ok(text)
}
fn document(root: &Path, path: &Path, kind: DocKind, label: &str, out: &mut Preview) -> Res<()> {
    let text = read(root, path, out)?;
    let doc = io::document_from_str(kind, &text).map_err(|e| e.to_string())?;
    let count = doc.len()
        + doc
            .footprint_cache()
            .values()
            .map(|f| f.items.len())
            .sum::<usize>()
        + doc
            .cache()
            .values()
            .map(|p| p.symbol.units.iter().map(|u| u.items.len()).sum::<usize>())
            .sum::<usize>();
    if count > 30000 {
        return Err(
            "This drawing exceeds the thumbnail geometry budget; open it for the full view".into(),
        );
    }
    let items = doc
        .items()
        .map(|(_, item)| {
            let mut item = item.clone();
            if let Item::Zone(z) = &mut item {
                z.fill.clear();
            }
            item
        })
        .collect();
    out.views.push(View {
        label: label.into(),
        kind,
        items,
        cache: doc.cache().clone(),
        footprints: doc.footprint_cache().clone(),
        symbol: None,
    });
    Ok(())
}
pub fn saved_preview(root: &Path, path: &Path) -> Res<Preview> {
    let root = fs::canonicalize(root).map_err(|e| e.to_string())?;
    let path = checked(&root, path)?;
    let mut out = Preview {
        views: vec![],
        model: None,
        dependencies: vec![path.to_string_lossy().into_owned()],
        notes: vec![],
    };
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "arsch" => document(&root, &path, DocKind::Schematic, "Schematic", &mut out)?,
        "arpcb" => {
            document(&root, &path, DocKind::Pcb, "Board", &mut out)?;
            out.notes
                .push("Saved board preview; copper fills are shown as outlines".into());
        }
        "arproj" => {
            let text = read(&root, &path, &mut out)?;
            let project = io::project_from_str(&text).map_err(|e| e.to_string())?;
            let dir = path.parent().unwrap();
            if let Some(s) = project.sheets.first() {
                if let Err(e) = document(
                    &root,
                    &dir.join(s),
                    DocKind::Schematic,
                    "Schematic",
                    &mut out,
                ) {
                    out.notes.push(e);
                }
            }
            if let Some(board) = project.pcb {
                if let Err(e) = document(&root, &dir.join(board), DocKind::Pcb, "Board", &mut out) {
                    out.notes.push(e);
                }
            }
            out.notes.push("Project preview: first saved schematic sheet and saved board; copper fills shown as outlines".into());
        }
        "arlib" => {
            let text = read(&root, &path, &mut out)?;
            let lib = io::library_from_str(&text).map_err(|e| e.to_string())?;
            if let Some(symbol) = lib.symbols.first() {
                if let Some(unit) = symbol.units.first() {
                    if unit.items.len() > 30000 {
                        return Err("Symbol exceeds the thumbnail budget".into());
                    }
                    out.views.push(View {
                        label: "Symbol".into(),
                        kind: DocKind::Symbol,
                        items: unit.items.clone(),
                        cache: BTreeMap::new(),
                        footprints: BTreeMap::new(),
                        symbol: Some(symbol.clone()),
                    });
                }
            }
            if let Some(fp) = lib.footprints.first() {
                if fp.items.len() > 30000 {
                    return Err("Footprint exceeds the thumbnail budget".into());
                }
                out.views.push(View {
                    label: "Footprint".into(),
                    kind: DocKind::Footprint,
                    items: fp.items.clone(),
                    cache: BTreeMap::new(),
                    footprints: BTreeMap::new(),
                    symbol: None,
                });
            }
            out.notes
                .push("Library preview shows the first symbol/unit and first footprint".into());
        }
        "step" | "stp" | "wrl" => {
            if fs::metadata(&path).map_err(|e| e.to_string())?.len() > 8 * 1024 * 1024 {
                return Err(
                    "Model exceeds the thumbnail size budget; open it in 3D to inspect it".into(),
                );
            }
            out.model = Some(path.to_string_lossy().into_owned());
        }
        _ => return Err("Content preview is not available for this file type".into()),
    }
    out.dependencies.sort();
    out.dependencies.dedup();
    Ok(out)
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
            "pcad-preview-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&p).unwrap();
        Temp(p)
    }
    #[test]
    fn project_and_library_previews_are_saved_only_and_read_only() {
        let t = temp();
        let p = super::super::files::new_project(&t.0, Path::new(""), "View").unwrap();
        let root = p.parent().unwrap();
        let board = root.join("Board.arpcb");
        let bytes = fs::read(&board).unwrap();
        let data = saved_preview(&t.0, &p).unwrap();
        assert_eq!(data.views.len(), 2);
        assert!(data.dependencies.iter().any(|p| p.ends_with("Board.arpcb")));
        assert_eq!(fs::read(&board).unwrap(), bytes);
        assert!(!root.join(".pcad-history").exists());
        let lib = saved_preview(&t.0, &root.join("libraries/Project.arlib")).unwrap();
        assert!(lib.views.is_empty());
    }
    #[test]
    fn previews_reject_escape_and_corruption_and_keep_valid_project_views() {
        let t = temp();
        let p = super::super::files::new_project(&t.0, Path::new(""), "View").unwrap();
        let root = p.parent().unwrap();
        fs::write(root.join("Board.arpcb"), b"damaged").unwrap();
        assert!(saved_preview(root, &root.join("Board.arpcb")).is_err());
        let data = saved_preview(root, &p).unwrap();
        assert_eq!(data.views.len(), 1);
        assert!(!data.notes.is_empty());
        assert!(saved_preview(&root.join("libraries"), &p).is_err());
        assert_eq!(fs::read(root.join("Board.arpcb")).unwrap(), b"damaged");
    }
}
