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

use pcbplannercad_core::document::Document;
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

/// Writes the plated (`plated = true`: vias + PTH pads) or non-plated drill file.
pub fn write_drill(doc: &Document, plated: bool) -> Result<String, OutputError> {
    let default = BoardSettings::default();
    let board = board_settings(doc).map_or(&default, |(_, b)| b);
    // (diameter µm, function) → hits
    let mut tools: BTreeMap<(i64, HoleFn), Vec<Hit>> = BTreeMap::new();
    if plated {
        for (_, item) in doc.items() {
            if let Item::Via(v) = item
                && v.drill > 0
            {
                tools.entry((div_round(v.drill, UNIT), HoleFn::Via)).or_default().push(Hit::Round(v.pos));
            }
        }
    }
    for p in all_pads(doc, board) {
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

    let layers = board.stackup.copper_layers().len().max(1);
    let date = creation_date();
    let mut s = String::new();
    s.push_str("M48\n");
    let _ = writeln!(s, "; DRILL file {{pCAD {VERSION}}} date {date}");
    s.push_str("; FORMAT={-:-/ absolute / metric / decimal}\n");
    let _ = writeln!(s, "; #@! TF.CreationDate,{date}");
    let _ = writeln!(s, "; #@! TF.GenerationSoftware,pCAD,pCAD,{VERSION}");
    if plated {
        let _ = writeln!(s, "; #@! TF.FileFunction,Plated,1,{layers},PTH");
    } else {
        let _ = writeln!(s, "; #@! TF.FileFunction,NonPlated,1,{layers},NPTH");
    }
    s.push_str("; #@! TF.FilePolarity,Positive\n");
    s.push_str("FMAT,2\nMETRIC\n");
    for (i, (&(dia, f), _)) in tools.iter().enumerate() {
        let func = match (plated, f) {
            (true, HoleFn::Via) => "Plated,PTH,ViaDrill",
            (true, HoleFn::Component) => "Plated,PTH,ComponentDrill",
            (false, _) => "NonPlated,NPTH,ComponentDrill",
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
        })
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
        assert!(s.contains("; #@! TA.AperFunction,Plated,PTH,ViaDrill\nT1C0.300"));
        assert!(s.contains("; #@! TA.AperFunction,Plated,PTH,ComponentDrill\nT2C0.800"));
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
        assert!(s.contains("; #@! TA.AperFunction,NonPlated,NPTH,ComponentDrill"));
        assert_eq!(tool_table(&s), vec![("T1".into(), 3.2)]);
        assert!(s.contains("T1\nX10.000Y30.000\n"));
        assert_eq!(s.lines().filter(|l| l.starts_with('X')).count(), 1);
    }
}
