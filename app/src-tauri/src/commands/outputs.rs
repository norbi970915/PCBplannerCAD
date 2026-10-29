//! Manufacturing and document outputs, plus small file helpers for outputs
//! produced in the UI (PDF) and 3D models loaded by the viewer.

use std::path::{Path, PathBuf};

use pcbplannercad_output::{assembly, fab, gerber_read, ipc2581, step};
use tauri::State;

use super::{Res, lock};
use crate::AppState;
use crate::workspace::Workspace;

fn board<'a>(ws: &'a Workspace, id: &str) -> Res<&'a pcbplannercad_core::Document> {
    let d = ws.doc(id)?;
    if d.kind != pcbplannercad_core::DocKind::Pcb {
        return Err("not a PCB document".into());
    }
    Ok(d.session.document())
}

/// The board with a project variant applied: its DNP parts get `DNP=1`, which
/// CSV/XML BOMs and placement lists omit; physical manufacturing geometry stays.
fn with_variant(ws: &Workspace, id: &str, doc: &pcbplannercad_core::Document, variant: Option<&str>) -> Res<Option<pcbplannercad_core::Document>> {
    let Some(name) = variant.filter(|v| !v.is_empty()) else { return Ok(None) };
    if ws.project_info().and_then(|p|p.pcb).and_then(|p|p.doc_id).as_deref()!=Some(id) {return Err("Assembly variants apply only to the current project board".into());}
    let v = ws.project()?.project.variants.iter().find(|v| v.name == name).ok_or_else(|| format!("variant {name} not found"))?;
    let mut items = std::collections::BTreeMap::new();
    for (id, item) in doc.items() {
        let mut item = item.clone();
        if let pcbplannercad_core::Item::Footprint(f) = &mut item {
            if v.dnp.contains(&f.designator) {
                f.parameters.insert("DNP".into(), "1".into());
            }
        }
        items.insert(id, item);
    }
    Ok(Some(pcbplannercad_core::Document::from_parts(doc.kind(), items, doc.cache().clone()).with_footprints(doc.footprint_cache().clone())))
}

fn project_name(ws: &Workspace, id: &str) -> String {
    ws.project_info().filter(|p|p.pcb.as_ref().and_then(|b|b.doc_id.as_deref())==Some(id)).map(|p|p.name)
        .or_else(||ws.doc_info(id).ok().map(|i|i.title.trim_end_matches(".arpcb").to_string())).unwrap_or_else(||"board".into())
}

/// Writes Gerbers, drills and optional assembly files into `dir`.
#[tauri::command(async)]
pub fn out_fab(state: State<AppState>, id: String, dir: String, mut options: fab::FabOptions, variant: Option<String>) -> Res<Vec<String>> {
    let ws = lock(&state);
    if options.gerber.project_name.is_empty() {
        options.gerber.project_name = project_name(&ws, &id);
    }
    let base = board(&ws, &id)?;
    let varied = with_variant(&ws, &id, base, variant.as_deref())?;
    ws.ensure_export_target(Path::new(&dir))?;
    let files = fab::write_package(varied.as_ref().unwrap_or(base), Path::new(&dir), &options).map_err(|e| e.to_string())?;
    Ok(files.into_iter().map(|p| p.display().to_string()).collect())
}

#[tauri::command(async)]
pub fn out_pnp(state: State<AppState>, id: String, path: String, format: assembly::PnpFormat, variant: Option<String>) -> Res<()> {
    let ws = lock(&state);
    let base = board(&ws, &id)?;
    let varied = with_variant(&ws, &id, base, variant.as_deref())?;
    let csv = assembly::pick_and_place_csv(varied.as_ref().unwrap_or(base), format, None);
    ws.write_export_file(Path::new(&path),csv.as_bytes())
}

#[tauri::command(async)]
pub fn out_board_bom(state: State<AppState>, id: String, path: String, format: assembly::BomFormat, variant: Option<String>) -> Res<()> {
    let ws = lock(&state);
    let base = board(&ws, &id)?;
    let varied = with_variant(&ws, &id, base, variant.as_deref())?;
    let csv = assembly::bom_csv(varied.as_ref().unwrap_or(base), format);
    ws.write_export_file(Path::new(&path),csv.as_bytes())
}

#[tauri::command(async)]
pub fn out_ipc2581(state: State<AppState>, id: String, path: String, variant: Option<String>) -> Res<()> {
    let ws = lock(&state);
    let base=board(&ws,&id)?;
    let varied=with_variant(&ws,&id,base,variant.as_deref())?;
    let xml = ipc2581::write_ipc2581(varied.as_ref().unwrap_or(base), &project_name(&ws, &id)).map_err(|e| e.to_string())?;
    ws.write_export_file(Path::new(&path),xml.as_bytes())
}

#[tauri::command]
pub fn out_board_model(state: State<AppState>, id: String) -> Res<step::BoardModel> {
    let ws = lock(&state);
    Ok(step::board_model(board(&ws, &id)?))
}

/// Writes meshes (board + component models triangulated in the UI) as STEP or OBJ.
#[tauri::command(async)]
pub fn out_step(state: State<AppState>, path: String, name: String, meshes: Vec<step::Mesh>) -> Res<()> {
    let text = if path.to_ascii_lowercase().ends_with(".obj") { step::mesh_to_obj(&meshes) } else { step::write_step(&name, &meshes).map_err(|e| e.to_string())? };
    lock(&state).write_export_file(Path::new(&path),text.as_bytes())
}

/// Parses Gerber/Excellon files from a folder or .zip for the viewer.
#[tauri::command]
pub fn out_read_gerbers(path: String) -> Res<Vec<gerber_read::GerberFile>> {
    gerber_read::read_dir_or_zip(Path::new(&path)).map_err(|e| e.to_string())
}

#[tauri::command(async)]
pub fn write_file_bytes(state: State<AppState>, path: String, bytes: Vec<u8>) -> Res<()> {
    lock(&state).write_export_file(Path::new(&path),&bytes)
}

#[tauri::command]
pub fn read_file_bytes(path: String) -> Res<tauri::ipc::Response> {
    let bytes = std::fs::read(&path).map_err(|e| format!("cannot read {path}: {e}"))?;
    Ok(tauri::ipc::Response::new(bytes))
}

/// Expands `${STRUCTURED*_3DMODEL_DIR}` style variables and relative paths for
/// footprint 3D models; returns the first existing candidate.
#[tauri::command]
pub fn resolve_model_path(state: State<AppState>, path: String, base: Option<String>) -> Option<String> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    let library_models = super::imports::library_share().map(|s| s.join("3dmodels"));
    let expanded = if let (Some(start), Some(end)) = (path.find("${"), path.find('}')) {
        let var = &path[start + 2..end];
        let rest = path[end + 1..].trim_start_matches(['/', '\\']);
        let value = std::env::var(var).ok().map(PathBuf::from).or_else(|| if var.contains("3DMODEL") { library_models.clone() } else { None });
        value.map(|v| v.join(rest))
    } else {
        Some(PathBuf::from(&path))
    };
    if let Some(p) = expanded {
        if p.is_relative() {
            if let Some(b) = &base {
                candidates.push(Path::new(b).join(&p));
            }
            // Imported libraries carry their copied assets beside the .arlib.
            for library in lock(&state).libs.values() {
                if let Some(dir)=library.path.as_ref().and_then(|p|p.parent()){candidates.push(dir.join(&p));}
            }
            if let Some(k) = &library_models {
                candidates.push(k.join(&p));
            }
        }
        candidates.push(p);
    }
    // structured format libraries often reference .wrl while .step ships alongside (and vice versa).
    let mut alt = Vec::new();
    for c in &candidates {
        for ext in ["step", "stp", "wrl"] {
            alt.push(c.with_extension(ext));
        }
    }
    candidates.extend(alt);
    candidates.into_iter().find(|c| c.is_file()).map(|c| c.display().to_string())
}


#[cfg(test)]
mod tests {
    use super::*;
    use pcbplannercad_core::{DocKind,Document,Item,Project,io};
    use crate::workspace::{OpenProject,Origin};
    fn part(name:&str)->Item{serde_json::from_value(serde_json::json!({"kind":"footprint","designator":name,"value":"10k","footprint":"Parts/R","pos":{"x":0,"y":0},"rotation":0,"side":"top","padNets":{},"component":"","parameters":{}})).unwrap()}
    fn fixture()->(Workspace,String,String){
        let mut ws=Workspace::default();let path=PathBuf::from("test-project/Board.arpcb");
        let id=ws.add_doc(DocKind::Pcb,Origin::File(Some(path)),Document::from_item_list(DocKind::Pcb,[Item::Board(Default::default()),part("R1"),part("R2"),serde_json::from_value(serde_json::json!({"kind":"track","a":{"x":0,"y":0},"b":{"x":10000000,"y":0},"width":200000,"layer":0,"net":"SIGNAL"})).unwrap()]));
        let other=ws.add_doc(DocKind::Pcb,Origin::File(Some("Standalone.arpcb".into())),Document::new(DocKind::Pcb));
        ws.project=Some(OpenProject{revision:None,path:"test-project/Design.arproj".into(),project:Project{name:"Assembly".into(),pcb:Some("Board.arpcb".into()),variants:vec![pcbplannercad_core::project::Variant{name:"Reduced".into(),dnp:vec!["R1".into()]}],..Default::default()}});
        (ws,id,other)
    }
    #[test]
    fn assembly_variant_is_bound_to_its_project_board(){
        let (ws,id,other)=fixture();assert_eq!(project_name(&ws,&id),"Assembly");assert_eq!(project_name(&ws,&other),"Standalone");
        assert!(with_variant(&ws,&other,board(&ws,&other).unwrap(),Some("Reduced")).is_err());
        assert!(with_variant(&ws,&other,board(&ws,&other).unwrap(),None).unwrap().is_none());
        assert!(with_variant(&ws,&id,board(&ws,&id).unwrap(),Some("Missing")).is_err());
    }
    #[test]
    fn selected_variant_agrees_in_csv_and_xml_boms_without_changing_board_copper(){
        let (ws,id,_)=fixture();let original=board(&ws,&id).unwrap();let before=io::document_to_string(original).unwrap();
        let varied=with_variant(&ws,&id,original,Some("Reduced")).unwrap().unwrap();
        let csv=assembly::bom_csv(&varied,assembly::BomFormat::Generic);assert!(!csv.contains("R1"));assert!(csv.contains("R2"));
        let placement=assembly::pick_and_place_csv(&varied,assembly::PnpFormat::Generic,None);assert!(!placement.contains("R1"));assert!(placement.contains("R2"));
        let xml=ipc2581::write_ipc2581(&varied,"Assembly").unwrap();let bom=xml.split("<Bom ").nth(1).unwrap().split("</Bom>").next().unwrap();
        assert!(!bom.contains(r#"name="R1""#));assert!(bom.contains(r#"name="R2""#));assert!(xml.contains(r#"refDes="R1""#),"physical component geometry must remain in manufacturing data");
        assert_eq!(io::document_to_string(board(&ws,&id).unwrap()).unwrap(),before);assert!(!ws.doc(&id).unwrap().session.is_dirty());
    }
}
