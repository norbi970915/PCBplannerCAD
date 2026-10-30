//! Parse first, review counts and diagnostics, then publish the exact prepared copy.
use crate::{BoardImport, ImportReport, SchematicImport, structured, xml};
use pcbplannercad_core::{Library, Project, io, library::Model3d, storage};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};
type Res<T> = Result<T, String>;
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelLink {
    pub footprint: String,
    pub path: String,
}
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    pub path: String,
    #[serde(default)]
    pub board_path: Option<String>,
    #[serde(default)]
    pub models: Vec<ModelLink>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub name: String,
    pub kind: String,
    pub sheets: usize,
    pub board_items: usize,
    pub board_file: Option<String>,
    pub linked_design: bool,
    pub symbols: usize,
    pub footprints: Vec<String>,
    pub components: usize,
    pub models: usize,
    pub outputs: Vec<String>,
    pub report: Vec<Note>,
}
#[derive(Serialize)]
pub struct Note {
    pub warning: bool,
    pub text: String,
}
pub struct Prepared {
    pub summary: Summary,
    pub files: Vec<(PathBuf, Vec<u8>)>,
    pub project: Option<PathBuf>,
    pub library: Option<PathBuf>,
}
fn stem(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Imported".into())
}
fn safe(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || "-_".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect();
    if s.is_empty() { "Imported".into() } else { s }
}
/// Disk paths are case-insensitive on some supported filesystems. Keep logical
/// library names/references unchanged and disambiguate only serialized filenames.
fn library_files(libraries: &[Library], existing: &[(PathBuf, Vec<u8>)]) -> Vec<String> {
    let stems: Vec<_> = libraries
        .iter()
        .map(|lib| {
            let name = safe(&lib.name);
            if pcbplannercad_core::project::files::folder_name(&name).is_err() {
                format!("_{name}")
            } else {
                name
            }
        })
        .collect();
    let preferred: BTreeSet<_> = stems
        .iter()
        .map(|s| format!("libraries/{s}.arlib").to_lowercase())
        .collect();
    let mut used: BTreeSet<_> = existing
        .iter()
        .map(|(p, _)| p.to_string_lossy().replace('\\', "/").to_lowercase())
        .collect();
    stems
        .into_iter()
        .map(|stem| {
            let candidate = format!("libraries/{stem}.arlib");
            if used.insert(candidate.to_lowercase()) {
                return candidate;
            }
            (2..)
                .map(|suffix| format!("libraries/{stem}-{suffix}.arlib"))
                .find(|path| {
                    !preferred.contains(&path.to_lowercase()) && used.insert(path.to_lowercase())
                })
                .unwrap()
        })
        .collect()
}
fn read(path: &Path) -> Res<String> {
    fs::read_to_string(path).map_err(|e| format!("Cannot read {}: {e}", path.display()))
}
fn model_bytes(path: &Path) -> Res<Vec<u8>> {
    let b = fs::read(path).map_err(|e| e.to_string())?;
    let ext = path
        .extension()
        .and_then(|x| x.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let head = String::from_utf8_lossy(&b[..b.len().min(1024)]);
    let valid = match ext.as_str() {
        "step" | "stp" => head
            .trim_start_matches('\u{feff}')
            .trim_start()
            .starts_with("ISO-10303-21;"),
        "wrl" => head.trim_start().starts_with("#VRML V2.0"),
        _ => false,
    };
    if !valid {
        return Err(
            "Choose a STEP (.step/.stp) or VRML 2 (.wrl) model with a matching file header".into(),
        );
    }
    Ok(b)
}
impl Prepared {
    pub fn publish(&self, dest: &Path) -> Res<()> {
        storage::create_bundle(dest, &self.files)
            .map_err(|e| format!("Cannot create import folder: {e}"))
    }
}
pub fn prepare(req: &Request) -> Res<Prepared> {
    let path = Path::new(&req.path);
    let name = stem(path);
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if req.board_path.is_some() && !matches!(ext.as_str(), "sch" | "kicad_sch") {
        return Err("Select a schematic source before choosing a companion board".into());
    }
    let companion = |extension: &str| -> Res<Option<PathBuf>> {
        if let Some(board) = &req.board_path {
            let p = PathBuf::from(board);
            if !p
                .extension()
                .is_some_and(|x| x.to_string_lossy().eq_ignore_ascii_case(extension))
            {
                return Err(
                    "Choose a companion board in the same source format as the schematic".into(),
                );
            }
            if !p.is_file() {
                return Err("The selected companion board does not exist".into());
            }
            Ok(Some(p))
        } else {
            let p = path.with_extension(extension);
            Ok(p.is_file().then_some(p))
        }
    };
    let mut report = ImportReport::default();
    let mut design: Option<(SchematicImport, Option<BoardImport>)> = None;
    let mut library: Option<Library> = None;
    let mut project_name = name.clone();
    match ext.as_str(){
        "kicad_pro"=>{let p=structured::import_project(path).map_err(|e|e.to_string())?;project_name=p.project.name;design=Some((p.schematic,p.board));},
        "kicad_sch"=>{let board=companion("kicad_pcb")?;design=Some((structured::import_schematic(path).map_err(|e|e.to_string())?,board.map(|p|structured::import_board(&p).map_err(|e|e.to_string())).transpose()?));},
        "sch"=>{let board=companion("brd")?;design=Some((xml::import_schematic(path).map_err(|e|e.to_string())?,board.map(|p|xml::import_board(&p).map_err(|e|e.to_string())).transpose()?));},
        "kicad_pcb"=>design=Some((SchematicImport::default(),Some(structured::import_board(path).map_err(|e|e.to_string())?))),
        "brd"=>design=Some((SchematicImport::default(),Some(xml::import_board(path).map_err(|e|e.to_string())?))),
        "kicad_sym"=>{let (l,r)=structured::parse_symbol_library(&read(path)?,&name).map_err(|e|e.to_string())?;library=Some(l);report=r;},
        "pretty" if path.is_dir()=>{let (l,r)=structured::import_footprint_dir(path,&name).map_err(|e|e.to_string())?;library=Some(l);report=r;},
        "lbr"=>{let (l,r)=xml::parse_library(&read(path)?,&name).map_err(|e|e.to_string())?;library=Some(l);report=r;},
        "arlib"=>library=Some(io::library_from_str(&read(path)?).map_err(|e|e.to_string())?),
        "pcbdoc"|"schdoc"|"pcblib"|"schlib"|"intlib"|"prjpcb"=>return Err("This native document format is not supported yet. It needs a dedicated importer; renaming its extension will not convert it. Use a supported interchange export when available.".into()),
        "f3d"|"f3z"|"flbr"|"fsch"|"fbrd"=>return Err("This archive format is not supported directly. Export an XML electronics library (.lbr), schematic/board (.sch/.brd), or STEP model (.step/.stp) first.".into()),
        _=>return Err("Unsupported input. Choose a structured project/schematic/board, XML electronics design/library, symbol library, footprint folder or native .arlib. Attach STEP/VRML models after analyzing a library.".into())
    }
    let mut source_notices=crate::notices::collect(path);
    for model in &req.models { for notice in crate::notices::collect(Path::new(&model.path)) { if !source_notices.contains(&notice) { source_notices.push(notice); } } }
    let mut files = vec![];
    if !source_notices.is_empty() { files.push((PathBuf::from("SOURCE-NOTICES.txt"), source_notices.join("\n\n").into_bytes())); }
    report.info("Source licensing", "Imports do not grant redistribution rights. Review source licenses and embedded notices; adjacent notice files are retained when found.");
    let mut summary = Summary {
        name: project_name.clone(),
        kind: if library.is_some() {
            "library"
        } else {
            "design"
        }
        .into(),
        sheets: 0,
        board_items: 0,
        board_file: None,
        linked_design: false,
        symbols: 0,
        footprints: vec![],
        components: 0,
        models: 0,
        outputs: vec![],
        report: vec![],
    };
    let mut project_file = None;
    let mut library_file = None;
    if let Some(mut lib) = library {
        for notice in &source_notices { if !lib.notices.contains(notice) { lib.notices.push(notice.clone()); } }
        let mut linked = BTreeSet::new();
        for link in &req.models {
            if !linked.insert(link.footprint.clone()) {
                return Err("Each footprint can have only one 3D model".into());
            }
            let fp = lib
                .footprints
                .iter_mut()
                .find(|f| f.name == link.footprint)
                .ok_or_else(|| format!("Unknown footprint {}", link.footprint))?;
            let source = Path::new(&link.path);
            let bytes = model_bytes(source)?;
            let ext = source
                .extension()
                .unwrap()
                .to_string_lossy()
                .to_ascii_lowercase();
            let rel = PathBuf::from(format!("models/{}/{}.{}", safe(&name), safe(&fp.name), ext));
            fp.model = Some(Model3d {
                path: rel.to_string_lossy().replace('\\', "/"),
                offset: [0.; 3],
                rotation: [0.; 3],
                scale: [1.; 3],
            });
            files.push((rel, bytes));
        }
        // Package existing local model references too, so a library remains portable.
        for fp in &mut lib.footprints {
            if linked.contains(&fp.name) {
                continue;
            }
            if let Some(model) = &mut fp.model {
                let source = if Path::new(&model.path).is_absolute() {
                    PathBuf::from(&model.path)
                } else {
                    path.parent().unwrap_or(Path::new(".")).join(&model.path)
                };
                if source.is_file() {
                    let bytes = model_bytes(&source)?;
                    let ext = source
                        .extension()
                        .unwrap()
                        .to_string_lossy()
                        .to_ascii_lowercase();
                    let rel =
                        PathBuf::from(format!("models/{}/{}.{}", safe(&name), safe(&fp.name), ext));
                    model.path = rel.to_string_lossy().replace('\\', "/");
                    files.push((rel, bytes));
                } else {
                    report.warn(&fp.name,"3D model is not available locally; attach a model before importing for a portable library");
                }
            }
        }
        for component in &lib.components {
            let Some(symbol) = lib.symbol(&component.symbol) else {
                report.warn(
                    &component.name,
                    "Component symbol is missing; review this entry before placement",
                );
                continue;
            };
            for footprint in &component.footprints {
                let Some(fp) = lib.footprint(footprint) else {
                    report.warn(&component.name, format!("Footprint {footprint} is missing"));
                    continue;
                };
                let pads: BTreeSet<_> = fp
                    .items
                    .iter()
                    .filter_map(|item| match item {
                        pcbplannercad_core::Item::Pad(p) => Some(p.number.as_str()),
                        _ => None,
                    })
                    .collect();
                let mut invalid = 0;
                for (unit, pin) in symbol.all_pins() {
                    if component
                        .pads_for(unit, &pin.number)
                        .iter()
                        .any(|p| !pads.contains(p.as_str()))
                    {
                        invalid += 1;
                    }
                }
                if invalid > 0 {
                    report.warn(&component.name,format!("{invalid} symbol pins reference absent pads in {footprint}; verify pin-to-pad mapping"));
                }
            }
        }
        summary.symbols = lib.symbols.len();
        summary.footprints = lib.footprints.iter().map(|f| f.name.clone()).collect();
        summary.components = lib.components.len();
        summary.models = files.iter().filter(|(path,_)| path.starts_with("models")).count();
        if lib.symbols.is_empty() && lib.footprints.is_empty() && lib.components.is_empty() {
            return Err("The file contains no supported library entries".into());
        }
        if ext == "lbr" {
            report.info(&name,"Cloud package/model links are not downloaded. Attach local STEP/VRML files to map 3D models to footprints.");
        }
        let rel = PathBuf::from(format!("{}.arlib", safe(&name)));
        files.push((rel.clone(), io::library_to_string(&lib).into_bytes()));
        library_file = Some(rel);
    }
    if let Some((sch, board)) = design {
        summary.board_file = board.as_ref().map(|b| b.file_name.clone());
        summary.linked_design = !sch.sheets.is_empty() && board.is_some();
        if let Some(board) = &board {
            if !sch.sheets.is_empty() {
                let compiled = pcbplannercad_core::schematic::compile(
                    &sch.sheets
                        .iter()
                        .map(|s| {
                            pcbplannercad_core::schematic::SheetRef::new(&s.file_name, &s.document)
                        })
                        .collect::<Vec<_>>(),
                );
                let plan =
                    pcbplannercad_core::pcb::sync::plan(&compiled, &board.document, &|part| {
                        let key = part.footprint.replace(':', "/");
                        board
                            .document
                            .cached_footprint(&part.footprint)
                            .map(|f| (part.footprint.clone(), f.clone()))
                            .or_else(|| {
                                board
                                    .document
                                    .cached_footprint(&key)
                                    .map(|f| (key, f.clone()))
                            })
                    });
                use pcbplannercad_core::pcb::sync::EcoChange;
                let connectivity = plan
                    .changes
                    .iter()
                    .filter(|c| {
                        matches!(
                            c,
                            EcoChange::AddFootprint { .. }
                                | EcoChange::RemoveFootprint { .. }
                                | EcoChange::ChangeFootprint { .. }
                                | EcoChange::UpdateNets { .. }
                                | EcoChange::RenameNet { .. }
                        )
                    })
                    .count();
                let warnings = plan
                    .changes
                    .iter()
                    .filter(|c| matches!(c, EcoChange::Warning { .. }))
                    .count();
                report.info(&project_name,"Schematic and board are grouped in one electronics project. Saves update their native working files; schematic changes use reviewed Update PCB.");
                if connectivity > 0 || warnings > 0 {
                    report.warn(&project_name,format!("Schematic/board review: {connectivity} placement/connectivity changes and {warnings} warnings. Verify this is the matching pair before Update PCB; nothing is applied automatically."));
                }
            }
        }

        if !req.models.is_empty() {
            return Err(
                "Attach models to a library import, then place its footprints in the design".into(),
            );
        }
        let mut p = Project {
            name: project_name.clone(),
            sheets: vec![],
            pcb: None,
            libraries: vec![],
            variants: vec![],
        };
        summary.sheets = sch.sheets.len();
        for sheet in sch.sheets {
            storage::relative_path(Path::new(&sheet.file_name)).map_err(|e| e.to_string())?;
            files.push((
                PathBuf::from(&sheet.file_name),
                io::document_to_string(&sheet.document)
                    .map_err(|e| e.to_string())?
                    .into_bytes(),
            ));
            p.sheets.push(sheet.file_name);
        }
        report.messages.extend(sch.report.messages);
        let mut libs = sch.libraries;
        if let Some(b) = board {
            summary.board_items = b.document.len();
            p.pcb = Some(b.file_name.clone());
            files.push((
                PathBuf::from(b.file_name),
                io::document_to_string(&b.document)
                    .map_err(|e| e.to_string())?
                    .into_bytes(),
            ));
            report.messages.extend(b.report.messages);
            for lib in b.libraries {
                if let Some(old) = libs.iter_mut().find(|l| l.name == lib.name) {
                    old.merge(lib);
                } else {
                    libs.push(lib);
                }
            }
        }
        let library_paths = library_files(&libs, &files);
        for (mut lib, rel) in libs.into_iter().zip(library_paths) {
            for notice in &source_notices { if !lib.notices.contains(notice) { lib.notices.push(notice.clone()); } }
            summary.symbols += lib.symbols.len();
            summary.components += lib.components.len();
            summary
                .footprints
                .extend(lib.footprints.iter().map(|f| f.name.clone()));
            if rel != format!("libraries/{}.arlib", safe(&lib.name)) {
                report.info(&lib.name,format!("Library filename disambiguated to {rel}; library identity and component references retained"));
            }
            p.libraries.push(rel.clone());
            files.push((PathBuf::from(rel), io::library_to_string(&lib).into_bytes()));
        }
        let rel = PathBuf::from(format!("{}.arproj", safe(&project_name)));
        files.push((rel.clone(), io::project_to_string(&p).into_bytes()));
        project_file = Some(rel);
    }
    let mut unique = BTreeSet::new();
    for (p, _) in &files {
        storage::relative_path(p).map_err(|e| e.to_string())?;
        if !unique.insert(p.to_string_lossy().to_lowercase()) {
            return Err(format!(
                "Two imported entries create the same output name: {}",
                p.display()
            ));
        }
    }
    summary.outputs = files
        .iter()
        .map(|(p, _)| p.to_string_lossy().replace('\\', "/"))
        .collect();
    summary.report = report
        .messages
        .iter()
        .map(|m| Note {
            warning: m.severity == crate::Severity::Warning,
            text: m.to_string(),
        })
        .collect();
    Ok(Prepared {
        summary,
        files,
        project: project_file,
        library: library_file,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unsupported_archives_and_native_documents_are_rejected_explicitly() {
        for path in [
            "design.PcbDoc",
            "design.SchDoc",
            "library.IntLib",
            "archive.flbr",
            "archive.f3d",
        ] {
            let e = prepare(&Request {
                board_path: None,
                path: path.into(),
                models: vec![],
            })
            .err()
            .unwrap();
            assert!(e.contains("not supported"), "{e}");
        }
    }
    #[test]
    fn library_and_model_are_reviewed_and_copied_without_mutating_source() {
        let dir = std::env::temp_dir().join(format!(
            "pcad-import-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&dir).unwrap();
        let source = dir.join("assets.arlib");
        let mut l = Library {
            name: "Assets".into(),
            ..Default::default()
        };
        l.footprints.push(pcbplannercad_core::Footprint {
            name: "CASE".into(),
            description: String::new(),
            items: vec![],
            model: None,
        });
        fs::write(&source, io::library_to_string(&l)).unwrap();
        let original = fs::read(&source).unwrap();
        fs::write(dir.join("LICENSE.txt"),"Original author; test redistribution license").unwrap();
        let model = dir.join("case.step");
        fs::write(&model, b"ISO-10303-21;\nEND-ISO-10303-21;").unwrap();
        let req = Request {
            board_path: None,
            path: source.to_string_lossy().into_owned(),
            models: vec![ModelLink {
                footprint: "CASE".into(),
                path: model.to_string_lossy().into_owned(),
            }],
        };
        let p = prepare(&req).unwrap();
        assert_eq!(p.summary.models, 1);
        let dest = dir.join("Imported");
        p.publish(&dest).unwrap();
        assert!(dest.join("models/assets/CASE.step").is_file());
        let lib =
            io::library_from_str(&fs::read_to_string(dest.join("assets.arlib")).unwrap()).unwrap();
        assert_eq!(
            lib.footprints[0].model.as_ref().unwrap().path,
            "models/assets/CASE.step"
        );
        assert_eq!(fs::read(&source).unwrap(), original);
        assert!(lib.notices.iter().any(|n|n.contains("Original author; test redistribution license")));
        assert!(fs::read_to_string(dest.join("SOURCE-NOTICES.txt")).unwrap().contains("Original author; test redistribution license"));
        assert!(p.publish(&dest).is_err());
        let mut bad = req.clone();
        bad.models[0].footprint = "missing".into();
        assert!(prepare(&bad).is_err());
        fs::write(&model, b"not a model").unwrap();
        assert!(prepare(&req).is_err());
        fs::remove_dir_all(dir).unwrap();
    }
}
