//! Excellon 2 drill files (metric, decimal coordinates with 3 decimals = 1 µm,
//! absolute). X2-style file and tool attributes are written as `; #@! ` comments
//! (as recommended by the Gerber spec for drill files). Slots use `G85`.
//!
//! Plated file: via holes and plated through-hole pad holes. Non-plated file:
//! NPTH pad holes. Tools are numbered T1… in ascending diameter order; holes
//! of the same diameter but a different function (via vs component) get
//! separate tools so the `TA.AperFunction` comment stays accurate.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use pcbplannercad_core::document::{Document, LayerId};
use pcbplannercad_core::geom::rotate;
use pcbplannercad_core::item::{Item, PadKind};
use pcbplannercad_core::pcb::BoardSettings;
use pcbplannercad_core::pcb::geometry::{all_pads, board_settings};
use pcbplannercad_core::units::{Coord, Point};

use crate::OutputError;
use crate::gerber::{VERSION, creation_date, div_round, fmt_fixed};

/// Nanometers per Excellon unit (1 µm, 3 decimals in mm).
const UNIT: i64 = 1000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum HoleFn {
    Via,
    Component,
}

enum Hit {
    Round(Point),
    Slot(Point, Point),
}

/// Writes the through-hole plated or non-plated file. Partial-span vias belong
/// in separate files returned by `drill_files`.
pub fn write_drill(doc: &Document, plated: bool) -> Result<String, OutputError> {
    let default = BoardSettings::default();
    let board = board_settings(doc).map_or(&default, |(_, b)| b);
    let layers = board.stackup.copper_layers();
    if plated && doc.items().any(|(_, item)| matches!(item, Item::Via(v) if via_span(v, &layers).is_none_or(|span|span != (1,layers.len())))) {
        return Err(OutputError::Invalid("Partial-span vias require separate drill files; export a fabrication package".into()));
    }
    write_span(doc, plated, (1,layers.len().max(1)))
}

fn via_span(via: &pcbplannercad_core::pcb::Via, copper: &[LayerId]) -> Option<(usize,usize)> {
    let a = copper.iter().position(|l|*l == via.from_layer)? + 1;
    let b = copper.iter().position(|l|*l == via.to_layer)? + 1;
    (a != b).then_some((a.min(b),a.max(b)))
}

/// File suffix, contents and description, with one plated file per drill span.
pub fn drill_files(doc: &Document) -> Result<Vec<(String,String,String)>, OutputError> {
    let default = BoardSettings::default();
    let board = board_settings(doc).map_or(&default, |(_, b)| b);
    let copper = board.stackup.copper_layers();
    let full = (1,copper.len().max(1));
    let mut spans = std::collections::BTreeSet::from([full]);
    for (_,item) in doc.items() {
        if let Item::Via(via) = item {
            spans.insert(via_span(via,&copper).ok_or_else(||OutputError::Invalid("A via has an invalid drill span for this board stack".into()))?);
        }
    }
    let mut files = Vec::new();
    for span in spans {
        let suffix = if span == full { "PTH.drl".into() } else { format!("PTH-L{}-L{}.drl",span.0,span.1) };
        files.push((suffix,write_span(doc,true,span)?,format!("Plated holes, copper layers {}–{} (Excellon 2, metric)",span.0,span.1)));
    }
    files.push(("NPTH.drl".into(),write_span(doc,false,full)?,"Non-plated through holes (Excellon 2, metric)".into()));
    Ok(files)
}

fn write_span(doc: &Document, plated: bool, span: (usize,usize)) -> Result<String, OutputError> {
    let default = BoardSettings::default();
    let board = board_settings(doc).map_or(&default, |(_, b)| b);
    let copper = board.stackup.copper_layers();
    let full = (1,copper.len().max(1));
    // (diameter µm, function) → hits
    let mut tools: BTreeMap<(i64, HoleFn), Vec<Hit>> = BTreeMap::new();
    if plated {
        for (_, item) in doc.items() {
            if let Item::Via(v) = item
                && v.drill > 0 && via_span(v,&copper) == Some(span)
            {
                tools.entry((div_round(v.drill, UNIT), HoleFn::Via)).or_default().push(Hit::Round(v.pos));
            }
        }
    }
    for p in all_pads(doc, board).into_iter().filter(|_|span == full) {
        let want = if plated { PadKind::Tht } else { PadKind::Npth };
        let Some(d) = p.drill else { continue };
        if p.kind != want || d.width <= 0 || d.height <= 0 {
            continue;
        }
        let dia = d.width.min(d.height);
        let hit = if d.width == d.height {
            Hit::Round(p.pos)
        } else {
            let half = (d.width.max(d.height) - dia) / 2;
            let along = if d.width > d.height { p.rotation } else { p.rotation + 90.0 };
            let a = rotate(p.pos.translated(-half, 0), p.pos, along);
            let b = rotate(p.pos.translated(half, 0), p.pos, along);
            Hit::Slot(a, b)
        };
        tools.entry((div_round(dia, UNIT), HoleFn::Component)).or_default().push(hit);
    }

    let layers = full.1;
    let date = creation_date();
    let mut s = String::new();
    s.push_str("M48\n");
    let _ = writeln!(s, "; DRILL file {{PCBPlanner Studio {VERSION}}} date {date}");
    s.push_str("; FORMAT={-:-/ absolute / metric / decimal}\n");
    let _ = writeln!(s, "; #@! TF.CreationDate,{date}");
    let _ = writeln!(s, "; #@! TF.GenerationSoftware,PCBPlanner Studio,PCBPlanner Studio,{VERSION}");
    if plated {
        let kind = if span == full { "PTH" } else if span.0 == 1 || span.1 == layers { "Blind" } else { "Buried" };
        let _ = writeln!(s, "; #@! TF.FileFunction,Plated,{},{},{kind}",span.0,span.1);
    } else {
        let _ = writeln!(s, "; #@! TF.FileFunction,NonPlated,1,{layers},NPTH");
    }
    s.push_str("; #@! TF.FilePolarity,Positive\n");
    s.push_str("FMAT,2\nMETRIC\n");
    for (i, (&(dia, f), _)) in tools.iter().enumerate() {
        let func = match (plated, f) {
            (true, HoleFn::Via) => "ViaDrill",
            (true, HoleFn::Component) => "ComponentDrill",
            (false, _) => "ComponentDrill",
        };
        let _ = writeln!(s, "; #@! TA.AperFunction,{func}");
        let _ = writeln!(s, "T{}C{}", i + 1, fmt_fixed(dia, 3));
    }
    s.push_str("%\nG90\nG05\n");
    for (i, hits) in tools.values().enumerate() {
        let _ = writeln!(s, "T{}", i + 1);
        for h in hits {
            match h {
                Hit::Round(p) => {
                    let _ = writeln!(s, "{}", xy(*p));
                }
                Hit::Slot(a, b) => {
                    let _ = writeln!(s, "{}G85{}", xy(*a), xy(*b));
                }
            }
        }
    }
    s.push_str("M30\n");
    Ok(s)
}

fn coord(c: Coord) -> String {
    fmt_fixed(div_round(c, UNIT), 3)
}

fn xy(p: Point) -> String {
    format!("X{}Y{}", coord(p.x), coord(p.y))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gerber::test_board::{board_doc, footprint_with, place};
    use pcbplannercad_core::item::{Drill, Pad, PadShape};
    use pcbplannercad_core::layer;
    use pcbplannercad_core::pcb::Via;
    use pcbplannercad_core::units::mm;

    fn hole_pad(number: &str, x: f64, kind: PadKind, drill: (f64, f64), rotation: f64) -> Item {
        Item::Pad(Pad {
            number: number.into(),
            pos: Point::new(mm(x), 0),
            rotation,
            shape: PadShape::Oval,
            width: mm(drill.0 + 0.6),
            height: mm(drill.1 + 0.6),
            corner_ratio: 0.0,
            kind,
            layer: layer::MULTI_LAYER,
            drill: Some(Drill { width: mm(drill.0), height: mm(drill.1) }),
         copper_shapes: Default::default(),})
    }

    fn doc() -> Document {
        let fp = footprint_with(vec![
            hole_pad("1", 0.0, PadKind::Tht, (1.0, 1.0), 0.0),
            hole_pad("2", 2.54, PadKind::Tht, (0.8, 0.8), 0.0),
            hole_pad("3", 5.08, PadKind::Tht, (2.0, 1.0), 0.0),
            hole_pad("MH", 10.0, PadKind::Npth, (3.2, 3.2), 0.0),
        ]);
        let mut items = vec![place("J1", "L/J", Point::new(mm(10.0), mm(20.0)), 90.0)];
        items.push(Item::Via(Via { locked: false, pos: Point::new(1_234_500, -1_234_500), diameter: mm(0.6), drill: mm(0.3), from_layer: layer::TOP_COPPER, to_layer: layer::BOTTOM_COPPER, net: "GND".into() }));
        items.push(Item::Via(Via { locked: false, pos: Point::new(1_234_499, 7), diameter: mm(0.6), drill: mm(0.3), from_layer: layer::TOP_COPPER, to_layer: layer::BOTTOM_COPPER, net: String::new() }));
        board_doc(items, &[("L/J", fp)])
    }

    fn tool_table(s: &str) -> Vec<(String, f64)> {
        s.lines()
            .take_while(|l| *l != "%")
            .filter(|l| l.starts_with('T') && l.contains('C'))
            .map(|l| {
                let (t, d) = l.split_once('C').unwrap();
                (t.to_string(), d.parse().unwrap())
            })
            .collect()
    }

    fn layered_doc() -> Document {
        use pcbplannercad_core::{Command,Session};
        let mut session=Session::with_document(doc());
        let (id,board)=board_settings(session.document()).unwrap();
        let mut board=board.clone();board.stackup=pcbplannercad_core::pcb::board::Stackup::with_copper(4);
        session.execute(Command::Replace{id,item:Item::Board(board)}).unwrap();
        let copper=board_settings(session.document()).unwrap().1.stackup.copper_layers();
        for (i,(from,to)) in [(0,1),(1,2),(3,2),(1,0)].into_iter().enumerate() {
            session.execute(Command::Add{items:vec![Item::Via(Via{locked:false,pos:Point::new(mm(40.+i as f64),mm(40.)),diameter:mm(0.6),drill:mm(0.3),from_layer:copper[from],to_layer:copper[to],net:"P".into()})]}).unwrap();
        }
        session.document().clone()
    }

    #[test]
    fn partial_span_drills_are_separate_and_never_duplicate_component_holes() {
        let doc=layered_doc();assert!(write_drill(&doc,true).unwrap_err().to_string().contains("separate drill files"));
        let files=drill_files(&doc).unwrap();assert_eq!(files.len(),5);
        let file=|suffix:&str| &files.iter().find(|(name,_,_)|name==suffix).unwrap().1;
        let hits=|s:&str|s.lines().filter(|l|l.starts_with('X')).count();
        assert_eq!(hits(file("PTH.drl")),5); // two through vias, two round pads, one slot
        assert_eq!(hits(file("PTH-L1-L2.drl")),2); // reverse span groups with forward span
        assert_eq!(hits(file("PTH-L2-L3.drl")),1);
        assert_eq!(hits(file("PTH-L3-L4.drl")),1);
        assert_eq!(hits(file("NPTH.drl")),1);
        for (suffix,function) in [("PTH.drl","Plated,1,4,PTH"),("PTH-L1-L2.drl","Plated,1,2,Blind"),("PTH-L2-L3.drl","Plated,2,3,Buried"),("PTH-L3-L4.drl","Plated,3,4,Blind")] {
            assert!(file(suffix).contains(&format!("TF.FileFunction,{function}")));
            assert!(crate::gerber_read::parse_excellon(suffix,file(suffix)).unwrap().warnings.is_empty());
        }
        let dir=std::env::temp_dir().join(format!("pcad-span-export-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let written=crate::fab::write_package(&doc,&dir,&crate::fab::FabOptions{zip:true,..Default::default()}).unwrap();
        assert_eq!(written.iter().filter(|p|p.extension().is_some_and(|e|e=="drl")).count(),5);
        let readme=std::fs::read_to_string(dir.join("README.txt")).unwrap();assert!(readme.contains("PTH-L2-L3.drl"));
        let mut archive=zip::ZipArchive::new(std::fs::File::open(dir.join("board_fab.zip")).unwrap()).unwrap();
        for (suffix,contents,_) in &files {use std::io::Read;let mut actual=String::new();archive.by_name(&format!("board-{suffix}")).unwrap().read_to_string(&mut actual).unwrap();assert_eq!(actual.lines().filter(|l|!l.contains("Date")&&!l.contains("date")).collect::<Vec<_>>(),contents.lines().filter(|l|!l.contains("Date")&&!l.contains("date")).collect::<Vec<_>>());}
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn invalid_drill_span_rejects_package_before_creating_destination() {
        let mut session=pcbplannercad_core::Session::with_document(layered_doc());
        session.execute(pcbplannercad_core::Command::Add{items:vec![Item::Via(Via{locked:false,pos:Point::default(),diameter:mm(0.6),drill:mm(0.3),from_layer:layer::TOP_COPPER,to_layer:layer::inner_copper(9),net:"P".into()})]}).unwrap();
        assert!(drill_files(session.document()).is_err());
        let dir=std::env::temp_dir().join(format!("pcad-invalid-span-{}",std::process::id()));
        assert!(!dir.exists());assert!(crate::fab::write_package(session.document(),&dir,&Default::default()).is_err());assert!(!dir.exists());
    }

    #[test]
    fn tool_table_is_sorted_and_hits_are_rounded() {
        let s = write_drill(&doc(), true).unwrap();
        assert!(s.starts_with("M48\n"));
        assert!(s.contains("; #@! TF.FileFunction,Plated,1,2,PTH"));
        assert!(s.contains("FMAT,2\nMETRIC\n"));
        assert!(s.trim_end().ends_with("M30"));
        let tools = tool_table(&s);
        assert_eq!(tools, vec![("T1".into(), 0.3), ("T2".into(), 0.8), ("T3".into(), 1.0)]);
        // T1 (vias): 1.2345 mm rounds half away from zero → 1.235, −1.2345 → −1.235.
        assert!(s.contains("T1\nX1.235Y-1.235\nX1.234Y0.000\n"));
        assert!(s.contains("; #@! TA.AperFunction,ViaDrill\nT1C0.300"));
        assert!(s.contains("; #@! TA.AperFunction,ComponentDrill\nT2C0.800"));
        // Pad 1 at local (0,0) → (10, 20); pad 2 at local (2.54, 0) rotated 90 → (10, 22.54).
        assert!(s.contains("T3\nX10.000Y20.000\n"));
        assert!(s.contains("T2\nX10.000Y22.540\n"));
        // No NPTH hole in the plated file.
        assert!(!s.contains("Y30.000"));
    }

    #[test]
    fn slots_use_g85_along_the_rotated_long_axis() {
        let s = write_drill(&doc(), true).unwrap();
        // 2.0×1.0 slot at local (5.08, 0) → world (10, 25.08), long axis rotated to Y.
        // Tool = 1.0 mm (shared with the 1.0 round hole), ends ±0.5 mm along Y.
        assert!(s.contains("X10.000Y24.580G85X10.000Y25.580"), "{s}");
    }

    #[test]
    fn non_plated_holes_go_to_their_own_file() {
        let s = write_drill(&doc(), false).unwrap();
        assert!(s.contains("; #@! TF.FileFunction,NonPlated,1,2,NPTH"));
        assert!(s.contains("; #@! TA.AperFunction,ComponentDrill"));
        assert_eq!(tool_table(&s), vec![("T1".into(), 3.2)]);
        assert!(s.contains("T1\nX10.000Y30.000\n"));
        assert_eq!(s.lines().filter(|l| l.starts_with('X')).count(), 1);
    }
}
