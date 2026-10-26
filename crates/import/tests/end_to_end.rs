//! Import → save/reload (native format) → compile, on real designs installed
//! on the developer machine. Skips with a note when the files are absent.

use std::path::PathBuf;

use pcbplannercad_core::schematic::{self, SheetRef};
use pcbplannercad_core::{DocKind, Item, io};
use pcbplannercad_import::{SchematicImport, xml, structured};

fn structured_demo(rel: &str) -> Option<PathBuf> {
    let p=PathBuf::from(std::env::var_os("PCAD_TEST_STRUCTURED_SHARE")?).join("demos").join(rel);
    p.exists().then_some(p)
}

fn xml_reference_schematic() -> Option<PathBuf> {
    let p=PathBuf::from(std::env::var_os("PCAD_TEST_XML_SCHEMATIC")?);
    p.is_file().then_some(p)
}

/// Saves every sheet to the native format and parses it back.
fn round_trip(import: &SchematicImport) -> Vec<(String, pcbplannercad_core::Document)> {
    import
        .sheets
        .iter()
        .map(|s| {
            let text = io::document_to_string(&s.document).expect("serialize");
            let back = io::document_from_str(DocKind::Schematic, &text).expect("reparse");
            assert_eq!(&back, &s.document, "{} changed in a save/load round trip", s.file_name);
            (s.file_name.clone(), back)
        })
        .collect()
}

fn compile(sheets: &[(String, pcbplannercad_core::Document)]) -> schematic::Compiled {
    let refs: Vec<SheetRef> = sheets.iter().map(|(p, d)| SheetRef { path: p.clone(), doc: d }).collect();
    schematic::compile(&refs)
}

#[test]
fn structured_hierarchy_imports_saves_and_compiles() {
    let Some(pro) = structured_demo(r"complex_hierarchy\complex_hierarchy.kicad_pro") else {
        eprintln!("structured format demos not installed; skipping");
        return;
    };
    let p = structured::import_project(&pro).expect("import");
    assert!(p.schematic.sheets.len() > 1, "hierarchy should yield several sheets");
    let sheets = round_trip(&p.schematic);
    let c = compile(&sheets);
    eprintln!(
        "complex_hierarchy: {} sheets, {} parts, {} nets, {} violations ({} errors)",
        sheets.len(),
        c.parts.len(),
        c.nets.len(),
        c.violations.len(),
        c.violations.iter().filter(|v| matches!(v.severity, schematic::Severity::Error)).count()
    );
    for v in c.violations.iter().take(15) {
        eprintln!("  {:?} {} {}", v.severity, v.code, v.message);
    }
    assert!(c.parts.len() > 10);
    assert!(c.nets.len() > 10);
    // PWR_FLAG markers must not short the supply rails together.
    assert!(!c.violations.iter().any(|v| v.code == "multiple-net-names" && v.message.contains("PWR_FLAG")), "PWR_FLAG merged nets");
    let gnd = c.nets.iter().find(|n| n.name == "GND").expect("GND net");
    assert!(!gnd.nodes.is_empty());
    assert!(c.nets.iter().any(|n| n.name.contains("12V")), "+12V should be its own net");
    // Nets must cross sheet boundaries through hierarchical ports.
    assert!(c.nets.iter().any(|n| n.nodes.iter().map(|x| &x.sheet).collect::<std::collections::HashSet<_>>().len() > 1));
}

#[test]
fn structured_flat_designs_compile_with_few_errors() {
    for demo in [r"ecc83\ecc83-pp.kicad_pro", r"pic_programmer\pic_programmer.kicad_pro"] {
        let Some(pro) = structured_demo(demo) else {
            eprintln!("{demo} not installed; skipping");
            continue;
        };
        let p = structured::import_project(&pro).expect("import");
        let sheets = round_trip(&p.schematic);
        let c = compile(&sheets);
        let errors: Vec<_> = c.violations.iter().filter(|v| matches!(v.severity, schematic::Severity::Error)).collect();
        eprintln!("{demo}: {} parts, {} nets, {} errors, {} warnings", c.parts.len(), c.nets.len(), errors.len(), c.violations.len() - errors.len());
        for v in errors.iter().take(10) {
            eprintln!("  {} {}", v.code, v.message);
        }
        // structured format's own ERC passes on its demos; ours may differ on edge rules
        // but must not see broken connectivity (lots of unconnected pins).
        let unconnected = c.violations.iter().filter(|v| v.code == "unconnected-pin").count();
        let pins: usize = c.nets.iter().map(|n| n.nodes.len()).sum();
        assert!(unconnected * 20 < pins.max(1), "{demo}: {unconnected} unconnected pins out of {pins}");
    }
}

#[test]
fn xml_schematic_imports_saves_and_compiles() {
    let Some(sch) = xml_reference_schematic() else {
        eprintln!("XML design examples not installed; skipping");
        return;
    };
    let imp = xml::import_schematic(&sch).expect("import");
    let sheets = round_trip(&imp);
    let c = compile(&sheets);
    let errors = c.violations.iter().filter(|v| matches!(v.severity, schematic::Severity::Error)).count();
    eprintln!("Arduino MEGA: {} parts, {} nets, {} errors, {} warnings", c.parts.len(), c.nets.len(), errors, c.violations.len() - errors);
    for v in c.violations.iter().take(15) {
        eprintln!("  {:?} {} {}", v.severity, v.code, v.message);
    }
    assert!(c.parts.len() > 40);
    assert!(c.nets.iter().any(|n| n.name == "GND" && n.nodes.len() > 10), "GND net should collect many pins");
    // The reference design leaves a number of MCU pins unused (XML has no
    // no-connect markers), so a modest share of unconnected pins is genuine.
    let unconnected = c.violations.iter().filter(|v| v.code == "unconnected-pin").count();
    let pins: usize = c.nets.iter().map(|n| n.nodes.len()).sum();
    assert!(unconnected * 5 < pins + unconnected, "{unconnected} unconnected pins out of {}", pins + unconnected);
}

#[test]
fn pads_serialize_with_distinct_tag_and_kind() {
    use pcbplannercad_core::item::{Pad, PadKind, PadShape};
    use pcbplannercad_core::layer::MULTI_LAYER;
    let pad = Item::Pad(Pad {
        number: "1".into(),
        pos: Default::default(),
        rotation: 0.0,
        shape: PadShape::Circle,
        width: 1_600_000,
        height: 1_600_000,
        corner_ratio: 0.0,
        kind: PadKind::Tht,
        layer: MULTI_LAYER,
        drill: Some(pcbplannercad_core::item::Drill { width: 800_000, height: 800_000 }),
    });
    let json = serde_json::to_value(&pad).unwrap();
    assert_eq!(json["kind"], "pad");
    assert_eq!(json["padKind"], "tht");
    assert_eq!(serde_json::from_value::<Item>(json).unwrap(), pad);
}

/// A freshly imported structured format project must already be in sync: the ECO planner
/// should find (almost) nothing to change between schematic and board.
#[test]
fn structured_imported_board_is_in_sync_with_schematic() {
    use pcbplannercad_core::pcb::sync::{self, EcoChange};
    for demo in [r"pic_programmer\pic_programmer.kicad_pro", r"complex_hierarchy\complex_hierarchy.kicad_pro"] {
        let Some(pro) = structured_demo(demo) else {
            eprintln!("structured format demos not installed; skipping");
            return;
        };
        let p = structured::import_project(&pro).expect("import");
        let Some(board) = p.board.as_ref() else { continue };
        let sheets = round_trip(&p.schematic);
        let c = compile(&sheets);
        let libs: Vec<&pcbplannercad_core::library::Library> = p.schematic.libraries.iter().chain(board.libraries.iter()).collect();
        let resolve = |part: &schematic::NetlistPart| {
            let key = part.footprint.clone();
            if let Some(f) = board.document.cached_footprint(&key) {
                return Some((key, f.clone()));
            }
            let (lib, name) = key.split_once(':')?;
            libs.iter().filter(|l| l.name == lib).find_map(|l| l.footprints.iter().find(|f| f.name == name)).map(|f| (key.clone(), f.clone()))
        };
        let plan = sync::plan(&c, &board.document, &resolve);
        let real: Vec<&EcoChange> = plan.changes.iter().filter(|c| !matches!(c, EcoChange::Warning { .. })).collect();
        eprintln!("{demo}: {} changes", real.len());
        for c in real.iter().take(25) {
            eprintln!("  {c:?}");
        }
        assert!(real.is_empty(), "{demo}: {} spurious ECO changes after import", real.len());
    }
}

/// Same for a XML design: the sibling .brd matches the .sch.
#[test]
fn xml_board_is_in_sync_with_schematic() {
    use pcbplannercad_core::pcb::sync::{self, EcoChange};
    let Some(sch) = xml_reference_schematic() else {
        eprintln!("XML design examples not installed; skipping");
        return;
    };
    let brd = sch.with_extension("brd");
    if !brd.exists() {
        return;
    }
    let imp = xml::import_schematic(&sch).expect("import");
    let board = xml::import_board(&brd).expect("board");
    let sheets = round_trip(&imp);
    let c = compile(&sheets);
    let libs: Vec<&pcbplannercad_core::library::Library> = imp.libraries.iter().chain(board.libraries.iter()).collect();
    let resolve = |part: &schematic::NetlistPart| {
        let key = part.footprint.clone();
        if let Some(f) = board.document.cached_footprint(&key) {
            return Some((key, f.clone()));
        }
        let (lib, name) = key.split_once(':').or_else(|| key.split_once('/'))?;
        libs.iter().filter(|l| l.name == lib).find_map(|l| l.footprints.iter().find(|f| f.name == name)).map(|f| (key.clone(), f.clone()))
    };
    let plan = sync::plan(&c, &board.document, &resolve);
    let real: Vec<&EcoChange> = plan.changes.iter().filter(|c| !matches!(c, EcoChange::Warning { .. })).collect();
    eprintln!("Arduino MEGA board: {} changes, {} warnings", real.len(), plan.changes.len() - real.len());
    for c in plan.changes.iter().take(30) {
        eprintln!("  {c:?}");
    }
    // XML leaves element values empty where the schematic shows the device
    // name, and RESET has a board-only GND pad; those updates are genuine.
    assert_eq!(real.len(), plan.changes.len(), "no footprint lookups should fail");
    assert!(
        real.iter().all(|c| matches!(c, EcoChange::UpdateValue { from, .. } if from.is_empty()) || matches!(c, EcoChange::UpdateNets { designator, .. } if designator == "RESET")),
        "unexpected ECO changes after import"
    );
}
