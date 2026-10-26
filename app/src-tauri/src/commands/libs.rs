use std::path::PathBuf;

use pcbplannercad_core::library::{Model3d, SymbolUnit};
use pcbplannercad_core::item::Field;
use pcbplannercad_core::libgen::{symbols, wizard};
use pcbplannercad_core::{CachedPart, Component, DocKind, Document, Footprint, Library, Symbol, units::Coord};
use serde::{Deserialize, Serialize};
use tauri::State;

use super::{Res, lock};
use crate::AppState;
use crate::workspace::{DocInfo, LibInfo, Origin, Workspace};

#[tauri::command]
pub fn lib_list(state: State<AppState>) -> Res<Vec<LibInfo>> {
    let ws = lock(&state);
    ws.libs.keys().map(|id| ws.lib_info(id)).collect()
}

#[tauri::command]
pub fn lib_get(state: State<AppState>, id: String) -> Res<Library> {
    Ok(lock(&state).lib(&id)?.lib.clone())
}

#[tauri::command]
pub fn lib_new(state: State<AppState>, name: String) -> Res<LibInfo> {
    let mut ws = lock(&state);
    let id = ws.add_lib(Library { name, ..Default::default() }, None, true);
    ws.lib_info(&id)
}

#[tauri::command]
pub fn lib_open(state: State<AppState>, path: String) -> Res<LibInfo> {
    let mut ws = lock(&state);
    let id = ws.open_lib_file(&PathBuf::from(path))?;
    ws.lib_info(&id)
}

#[tauri::command]
pub fn lib_save(state: State<AppState>, id: String, path: Option<String>) -> Res<LibInfo> {
    let mut ws = lock(&state);
    ws.save_lib(&id, path.map(PathBuf::from))?;
    ws.lib_info(&id)
}

/// Returns ids of documents that were closed with it.
#[tauri::command]
pub fn lib_close(state: State<AppState>, id: String) -> Res<Vec<String>> {
    let mut ws = lock(&state);
    if ws.lib(&id)?.readonly {
        return Err("the built-in library can't be closed".into());
    }
    let closed = ws.close_entry_docs(&id, |_| true);
    ws.libs.remove(&id);
    Ok(closed)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentEntry {
    library: String,
    library_name: String,
    name: String,
    description: String,
    value: String,
    symbol: String,
    footprints: Vec<String>,
    units: usize,
    pins: usize,
}

/// Flat index of every component in every open library, for search.
#[tauri::command]
pub fn lib_component_index(state: State<AppState>) -> Res<Vec<ComponentEntry>> {
    let ws = lock(&state);
    let mut out = Vec::new();
    for (id, l) in &ws.libs {
        for c in &l.lib.components {
            let sym = l.lib.symbol(&c.symbol);
            out.push(ComponentEntry {
                library: id.clone(),
                library_name: l.lib.name.clone(),
                name: c.name.clone(),
                description: if c.description.is_empty() { sym.map(|s| s.description.clone()).unwrap_or_default() } else { c.description.clone() },
                value: c.parameter("Value").unwrap_or_default().to_string(),
                symbol: c.symbol.clone(),
                footprints: c.footprints.clone(),
                units: sym.map_or(0, |s| s.units.len()),
                pins: sym.map_or(0, |s| s.all_pins().count()),
            });
        }
    }
    Ok(out)
}

#[tauri::command]
pub fn lib_cached_part(state: State<AppState>, library: String, component: String) -> Res<CachedPart> {
    lock(&state).lib(&library)?.lib.cached_part(&component).ok_or_else(|| format!("{component} or its symbol is missing"))
}

/// Resolves a footprint reference (`Name` or `Lib:Name`) across open libraries.
#[tauri::command]
pub fn lib_find_footprint(state: State<AppState>, library: String, reference: String) -> Res<Option<Footprint>> {
    Ok(lock(&state).find_footprint(&library, &reference).map(|(_, f)| f))
}

// ---------- entry management ----------

fn unique_name(existing: impl Iterator<Item = String>, base: &str) -> String {
    let taken: std::collections::HashSet<String> = existing.collect();
    if !taken.contains(base) {
        return base.to_string();
    }
    (2..).map(|n| format!("{base}_{n}")).find(|n| !taken.contains(n)).unwrap()
}

#[derive(Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum EntryKind {
    Symbol,
    Footprint,
    Component,
}

#[tauri::command]
pub fn lib_new_symbol(state: State<AppState>, id: String, name: String, prefix: String) -> Res<String> {
    let mut ws = lock(&state);
    let l = ws.lib_writable(&id)?;
    let name = unique_name(l.lib.symbols.iter().map(|s| s.name.clone()), &name);
    let mut sym = Symbol::new(name.clone(), prefix);
    sym.fields = default_symbol_fields();
    l.lib.symbols.push(sym);
    l.dirty = true;
    Ok(name)
}

fn default_symbol_fields() -> Vec<Field> {
    use pcbplannercad_core::item::{FIELD_DESIGNATOR, FIELD_VALUE, HAlign, VAlign};
    use pcbplannercad_core::units::{Point, mm};
    let f = |name: &str, y: f64| Field {
        name: name.into(),
        value: String::new(),
        offset: Point::new(0, mm(y)),
        rotation: 0.0,
        height: mm(1.27),
        h_align: HAlign::Left,
        v_align: VAlign::Middle,
        visible: true,
    };
    vec![f(FIELD_DESIGNATOR, 6.35), f(FIELD_VALUE, -6.35)]
}

#[tauri::command]
pub fn lib_new_footprint(state: State<AppState>, id: String, name: String) -> Res<String> {
    let mut ws = lock(&state);
    let l = ws.lib_writable(&id)?;
    let name = unique_name(l.lib.footprints.iter().map(|f| f.name.clone()), &name);
    l.lib.footprints.push(Footprint::new(name.clone()));
    l.dirty = true;
    Ok(name)
}

/// Creates or replaces a component. `previous` renames an existing one.
#[tauri::command]
pub fn lib_put_component(state: State<AppState>, id: String, component: Component, previous: Option<String>) -> Res<()> {
    let mut ws = lock(&state);
    let l = ws.lib_writable(&id)?;
    if component.name.trim().is_empty() {
        return Err("component name is empty".into());
    }
    let old = previous.as_deref().unwrap_or(&component.name);
    if old != component.name && l.lib.component(&component.name).is_some() {
        return Err(format!("a component named {} already exists", component.name));
    }
    match l.lib.components.iter().position(|c| c.name == old) {
        Some(i) => l.lib.components[i] = component,
        None => l.lib.components.push(component),
    }
    l.dirty = true;
    Ok(())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SymbolMeta {
    name: String,
    description: String,
    prefix: String,
    show_pin_numbers: bool,
    show_pin_names: bool,
    pin_name_offset: Coord,
    is_power: bool,
    fields: Vec<Field>,
}

#[tauri::command]
pub fn lib_update_symbol(state: State<AppState>, id: String, name: String, meta: SymbolMeta) -> Res<()> {
    let mut ws = lock(&state);
    let l = ws.lib_writable(&id)?;
    if meta.name != name && l.lib.symbol(&meta.name).is_some() {
        return Err(format!("a symbol named {} already exists", meta.name));
    }
    let sym = l.lib.symbols.iter_mut().find(|s| s.name == name).ok_or("symbol not found")?;
    sym.name = meta.name.clone();
    sym.description = meta.description;
    sym.prefix = meta.prefix;
    sym.show_pin_numbers = meta.show_pin_numbers;
    sym.show_pin_names = meta.show_pin_names;
    sym.pin_name_offset = meta.pin_name_offset;
    sym.is_power = meta.is_power;
    sym.fields = meta.fields;
    if meta.name != name {
        for c in &mut l.lib.components {
            if c.symbol == name {
                c.symbol = meta.name.clone();
            }
        }
        rename_entry_docs(&mut ws, &id, &name, &meta.name, true);
    }
    ws.lib_mut(&id)?.dirty = true;
    Ok(())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FootprintMeta {
    name: String,
    description: String,
    model: Option<Model3d>,
}

#[tauri::command]
pub fn lib_update_footprint(state: State<AppState>, id: String, name: String, meta: FootprintMeta) -> Res<()> {
    let mut ws = lock(&state);
    let l = ws.lib_writable(&id)?;
    if meta.name != name && l.lib.footprint(&meta.name).is_some() {
        return Err(format!("a footprint named {} already exists", meta.name));
    }
    let fp = l.lib.footprints.iter_mut().find(|f| f.name == name).ok_or("footprint not found")?;
    fp.name = meta.name.clone();
    fp.description = meta.description;
    fp.model = meta.model;
    if meta.name != name {
        for c in &mut l.lib.components {
            for f in &mut c.footprints {
                if *f == name {
                    *f = meta.name.clone();
                }
            }
        }
        rename_entry_docs(&mut ws, &id, &name, &meta.name, false);
    }
    ws.lib_mut(&id)?.dirty = true;
    Ok(())
}

fn rename_entry_docs(ws: &mut Workspace, lib_id: &str, old: &str, new: &str, symbol: bool) {
    for d in ws.docs.values_mut() {
        match &mut d.origin {
            Origin::Symbol { lib, name, .. } if symbol && lib == lib_id && name == old => *name = new.to_string(),
            Origin::Footprint { lib, name } if !symbol && lib == lib_id && name == old => *name = new.to_string(),
            _ => {}
        }
    }
}

/// Deletes an entry; returns ids of documents that were closed.
#[tauri::command]
pub fn lib_delete_entry(state: State<AppState>, id: String, kind: EntryKind, name: String) -> Res<Vec<String>> {
    let mut ws = lock(&state);
    let l = ws.lib_writable(&id)?;
    match kind {
        EntryKind::Symbol => l.lib.symbols.retain(|s| s.name != name),
        EntryKind::Footprint => l.lib.footprints.retain(|f| f.name != name),
        EntryKind::Component => l.lib.components.retain(|c| c.name != name),
    }
    l.dirty = true;
    Ok(ws.close_entry_docs(&id, |o| match o {
        Origin::Symbol { name: n, .. } => kind == EntryKind::Symbol && *n == name,
        Origin::Footprint { name: n, .. } => kind == EntryKind::Footprint && *n == name,
        Origin::File(_) => false,
    }))
}

#[tauri::command]
pub fn lib_duplicate_entry(state: State<AppState>, id: String, kind: EntryKind, name: String) -> Res<String> {
    let mut ws = lock(&state);
    let l = ws.lib_writable(&id)?;
    let copy_name = |names: Vec<String>| unique_name(names.into_iter(), &format!("{name}_copy"));
    let new_name = match kind {
        EntryKind::Symbol => {
            let mut s = l.lib.symbol(&name).ok_or("symbol not found")?.clone();
            s.name = copy_name(l.lib.symbols.iter().map(|s| s.name.clone()).collect());
            let n = s.name.clone();
            l.lib.symbols.push(s);
            n
        }
        EntryKind::Footprint => {
            let mut f = l.lib.footprint(&name).ok_or("footprint not found")?.clone();
            f.name = copy_name(l.lib.footprints.iter().map(|f| f.name.clone()).collect());
            let n = f.name.clone();
            l.lib.footprints.push(f);
            n
        }
        EntryKind::Component => {
            let mut c = l.lib.component(&name).ok_or("component not found")?.clone();
            c.name = copy_name(l.lib.components.iter().map(|c| c.name.clone()).collect());
            let n = c.name.clone();
            l.lib.components.push(c);
            n
        }
    };
    l.dirty = true;
    Ok(new_name)
}

/// Copies entries (with what they depend on) from one library into another.
#[tauri::command]
pub fn lib_copy_components(state: State<AppState>, from: String, to: String, components: Vec<String>) -> Res<usize> {
    let mut ws = lock(&state);
    let src = ws.lib(&from)?.lib.clone();
    let mut add = Library::default();
    for name in &components {
        let c = src.component(name).ok_or_else(|| format!("{name} not found"))?;
        if let Some(s) = src.symbol(&c.symbol) {
            add.symbols.push(s.clone());
        }
        for f in &c.footprints {
            if let Some(fp) = src.footprint(f) {
                add.footprints.push(fp.clone());
            }
        }
        add.components.push(c.clone());
    }
    let n = add.components.len();
    let l = ws.lib_writable(&to)?;
    l.lib.merge(add);
    l.dirty = true;
    Ok(n)
}

// ---------- entry editing documents ----------

/// Opens (or returns) the documents for every unit of a symbol.
#[tauri::command]
pub fn lib_open_symbol(state: State<AppState>, id: String, name: String) -> Res<Vec<DocInfo>> {
    let mut ws = lock(&state);
    let units = ws.lib(&id)?.lib.symbol(&name).ok_or("symbol not found")?.units.clone();
    let mut out = Vec::new();
    for (i, u) in units.into_iter().enumerate() {
        let origin = Origin::Symbol { lib: id.clone(), name: name.clone(), unit: i };
        let doc_id = match ws.find_doc(&origin) {
            Some(d) => d,
            None => ws.add_doc(DocKind::Symbol, origin, Document::from_item_list(DocKind::Symbol, u.items)),
        };
        out.push(ws.doc_info(&doc_id)?);
    }
    Ok(out)
}

#[tauri::command]
pub fn lib_add_symbol_unit(state: State<AppState>, id: String, name: String) -> Res<Vec<DocInfo>> {
    {
        let mut ws = lock(&state);
        let l = ws.lib_writable(&id)?;
        let sym = l.lib.symbols.iter_mut().find(|s| s.name == name).ok_or("symbol not found")?;
        sym.units.push(SymbolUnit::default());
        l.dirty = true;
    }
    lib_open_symbol(state, id, name)
}

#[tauri::command]
pub fn lib_remove_symbol_unit(state: State<AppState>, id: String, name: String, unit: usize) -> Res<Vec<DocInfo>> {
    {
        let mut ws = lock(&state);
        let l = ws.lib_writable(&id)?;
        let sym = l.lib.symbols.iter_mut().find(|s| s.name == name).ok_or("symbol not found")?;
        if sym.units.len() <= 1 {
            return Err("a symbol needs at least one unit".into());
        }
        sym.units.remove(unit);
        l.dirty = true;
        // Unit documents are positional: reopen them all.
        let n = name.clone();
        ws.close_entry_docs(&id, |o| matches!(o, Origin::Symbol { name: s, .. } if *s == n));
    }
    lib_open_symbol(state, id, name)
}

#[tauri::command]
pub fn lib_open_footprint(state: State<AppState>, id: String, name: String) -> Res<DocInfo> {
    let mut ws = lock(&state);
    let items = ws.lib(&id)?.lib.footprint(&name).ok_or("footprint not found")?.items.clone();
    let origin = Origin::Footprint { lib: id.clone(), name };
    let doc_id = match ws.find_doc(&origin) {
        Some(d) => d,
        None => ws.add_doc(DocKind::Footprint, origin, Document::from_item_list(DocKind::Footprint, items)),
    };
    ws.doc_info(&doc_id)
}

// ---------- wizards ----------

#[tauri::command]
pub fn wizard_families() -> Res<serde_json::Value> {
    serde_json::to_value(wizard::families()).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn wizard_default_name(spec: wizard::FootprintSpec) -> Res<String> {
    Ok(wizard::default_name(&spec))
}

#[tauri::command]
pub fn wizard_preview(spec: wizard::FootprintSpec) -> Res<Footprint> {
    wizard::generate(&spec).map_err(|e| e.to_string())
}

/// Generates a footprint into a library; returns the stored name.
#[tauri::command]
pub fn wizard_generate(state: State<AppState>, id: String, spec: wizard::FootprintSpec, name: Option<String>) -> Res<String> {
    let mut fp = wizard::generate(&spec).map_err(|e| e.to_string())?;
    let mut ws = lock(&state);
    let l = ws.lib_writable(&id)?;
    let base = name.filter(|n| !n.trim().is_empty()).unwrap_or_else(|| wizard::default_name(&spec));
    fp.name = unique_name(l.lib.footprints.iter().map(|f| f.name.clone()), &base);
    let n = fp.name.clone();
    l.lib.footprints.push(fp);
    l.dirty = true;
    Ok(n)
}

#[tauri::command]
pub fn symbol_wizard(state: State<AppState>, id: String, spec: symbols::SymbolSpec) -> Res<String> {
    let mut sym = symbols::box_symbol(&spec).map_err(|e| e.to_string())?;
    let mut ws = lock(&state);
    let l = ws.lib_writable(&id)?;
    sym.name = unique_name(l.lib.symbols.iter().map(|s| s.name.clone()), &sym.name);
    let n = sym.name.clone();
    l.lib.symbols.push(sym);
    l.dirty = true;
    Ok(n)
}

pub fn ensure_builtin(ws: &mut Workspace) {
    use crate::workspace::{BUILTIN_LIB, OpenLib};
    ws.libs.insert(
        BUILTIN_LIB.to_string(),
        OpenLib { path: None, lib: pcbplannercad_core::libgen::starter::starter_library(), dirty: false, readonly: true },
    );
}
