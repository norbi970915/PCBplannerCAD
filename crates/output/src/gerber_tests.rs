use std::collections::HashMap;

use super::test_board::{board_doc, footprint_with, place, smd_pad};
use super::*;
use pcbplannercad_core::item::{Drill, HAlign, Pad, Text, VAlign};
use pcbplannercad_core::pcb::{ArcTrack, Track, Via, Zone, ZonePolygon};
use pcbplannercad_core::units::mm;

/// Just enough of a Gerber reader to check our own output.
#[derive(Default, Debug)]
struct Parsed {
    dec: u32,
    /// D code → (template, aperture function)
    apertures: HashMap<u32, (String, Option<String>)>,
    file_attrs: Vec<String>,
    flashes: Vec<(u32, f64, f64)>,
    /// Draw endpoints (D01) in mm.
    draws: Vec<(f64, f64)>,
    regions: Vec<Vec<(f64, f64)>>,
    region_functions: Vec<Option<String>>,
    macros: Vec<String>,
    lpc: bool,
    arcs: usize,
    last_statement: String,
}

fn parse(src: &str) -> Parsed {
    let mut p = Parsed { dec: 6, ..Default::default() };
    let mut ta: Option<String> = None;
    let (mut cur, mut x, mut y) = (0u32, 0i64, 0i64);
    let mut in_region = false;
    let mut interp = 1;
    let mut chars = src.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
            continue;
        }
        if c == '%' {
            chars.next();
            let block: String = chars.by_ref().take_while(|&c| c != '%').collect();
            let first = block.split('*').next().unwrap_or("").trim().to_string();
            if let Some(rest) = first.strip_prefix("FSLAX") {
                p.dec = rest[1..2].parse().unwrap();
            } else if let Some(rest) = first.strip_prefix("TA.AperFunction,") {
                ta = Some(rest.to_string());
            } else if first.starts_with("TD") {
                ta = None;
            } else if let Some(rest) = first.strip_prefix("ADD") {
                let n: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
                p.apertures.insert(n.parse().unwrap(), (rest[n.len()..].to_string(), ta.clone()));
            } else if first.starts_with("TF.") {
                p.file_attrs.push(first.clone());
            } else if first == "LPC" {
                p.lpc = true;
            } else if let Some(name) = first.strip_prefix("AM") {
                p.macros.push(name.to_string());
            }
            p.last_statement = first;
            continue;
        }
        let stmt: String = chars.by_ref().take_while(|&c| c != '*').collect();
        let stmt = stmt.trim().to_string();
        p.last_statement = stmt.clone();
        if stmt.starts_with("G04") || stmt.is_empty() {
            continue;
        }
        match stmt.as_str() {
            "G36" => {
                in_region = true;
                p.regions.push(Vec::new());
                p.region_functions.push(ta.clone());
                continue;
            }
            "G37" => {
                in_region = false;
                continue;
            }
            "G01" => {
                interp = 1;
                continue;
            }
            "G02" | "G03" => {
                interp = 2;
                continue;
            }
            "G75" | "M02" => continue,
            _ => {}
        }
        if let Some(d) = stmt.strip_prefix('D')
            && d.parse::<u32>().is_ok_and(|n| n >= 10)
        {
            cur = d.parse().unwrap();
            continue;
        }
        // Coordinate data.
        let num = |key: char| -> Option<i64> {
            let i = stmt.find(key)?;
            let s: String = stmt[i + 1..].chars().take_while(|c| *c == '-' || c.is_ascii_digit()).collect();
            s.parse().ok()
        };
        if let Some(v) = num('X') {
            x = v;
        }
        if let Some(v) = num('Y') {
            y = v;
        }
        let scale = 10f64.powi(p.dec as i32);
        let pt = (x as f64 / scale, y as f64 / scale);
        if stmt.ends_with("D03") {
            p.flashes.push((cur, pt.0, pt.1));
        } else if stmt.ends_with("D01") {
            if in_region {
                p.regions.last_mut().unwrap().push(pt);
            } else {
                p.draws.push(pt);
            }
            if interp == 2 {
                assert!(stmt.contains('I') && stmt.contains('J'), "arc without I/J: {stmt}");
                p.arcs += 1;
            }
        } else if stmt.ends_with("D02") {
            if in_region {
                p.regions.last_mut().unwrap().push(pt);
            }
        } else {
            panic!("unexpected statement {stmt}");
        }
    }
    p
}

fn opts() -> GerberOptions {
    GerberOptions { project_name: "Test".into(), ..Default::default() }
}

fn tht_pad(number: &str, x: f64) -> Item {
    Item::Pad(Pad {
        number: number.into(),
        pos: Point::new(mm(x), 0),
        rotation: 0.0,
        shape: PadShape::Circle,
        width: mm(1.6),
        height: mm(1.6),
        corner_ratio: 0.0,
        kind: PadKind::Tht,
        layer: layer::MULTI_LAYER,
        drill: Some(Drill { width: mm(0.8), height: mm(0.8) }),
    })
}

/// Two resistors sharing one footprint, a THT header, a track, an arc track and a via.
fn sample() -> Document {
    let r = footprint_with(vec![
        smd_pad("1", -0.8, 1.0, 0.5, PadShape::Rect, 0.0),
        smd_pad("2", 0.8, 1.0, 0.5, PadShape::Rect, 0.0),
        Item::Text(Text { pos: Point::new(0, mm(1.0)), text: DESIGNATOR_TOKEN.into(), height: mm(1.0), rotation: 0.0, h_align: HAlign::Center, v_align: VAlign::Middle, layer: layer::TOP_SILK, mirrored: false }),
        Item::Text(Text { pos: Point::new(0, mm(-1.0)), text: VALUE_TOKEN.into(), height: mm(1.0), rotation: 0.0, h_align: HAlign::Center, v_align: VAlign::Middle, layer: layer::TOP_SILK, mirrored: false }),
    ]);
    let j = footprint_with(vec![tht_pad("1", 0.0), tht_pad("2", 2.54)]);
    let items = vec![
        place("R1", "L/R", Point::new(mm(10.0), mm(10.0)), 0.0),
        place("R2", "L/R", Point::new(mm(20.0), mm(10.0)), 90.0),
        place("J1", "L/J", Point::new(mm(30.0), mm(15.0)), 0.0),
        Item::Track(Track { a: Point::new(mm(10.8), mm(10.0)), b: Point::new(mm(20.0), mm(9.2)), width: mm(0.25), layer: layer::TOP_COPPER, net: "N1".into() }),
        Item::ArcTrack(ArcTrack { center: Point::new(mm(25.0), mm(20.0)), radius: mm(2.0), start_angle: 0.0, sweep: 90.0, width: mm(0.25), layer: layer::TOP_COPPER, net: "N1".into() }),
        Item::Via(Via { pos: Point::new(mm(5.0), mm(5.0)), diameter: mm(0.6), drill: mm(0.3), from_layer: layer::TOP_COPPER, to_layer: layer::BOTTOM_COPPER, net: "GND".into() }),
    ];
    board_doc(items, &[("L/R", r), ("L/J", j)])
}

fn near(a: (f64, f64), b: (f64, f64)) -> bool {
    (a.0 - b.0).abs() < 1e-9 && (a.1 - b.1).abs() < 1e-9
}

#[test]
fn copper_apertures_are_reused_and_pads_flash_at_centers() {
    let s = write_layer(&sample(), LayerFunction::Copper(layer::TOP_COPPER), &opts()).unwrap();
    let p = parse(&s);
    assert_eq!(p.last_statement, "M02");
    assert!(!p.lpc);
    // Apertures: SMD rect (R1), SMD rect swapped (R2 at 90°), THT circle, via, one track width.
    let templates: Vec<&str> = p.apertures.values().map(|(t, _)| t.as_str()).collect();
    assert_eq!(p.apertures.len(), 5, "{templates:?}");
    assert!(templates.contains(&"R,1.000000X0.500000"));
    assert!(templates.contains(&"R,0.500000X1.000000"));
    let func_of = |t: &str| p.apertures.values().find(|(x, _)| x == t).and_then(|(_, f)| f.clone());
    assert_eq!(func_of("R,1.000000X0.500000").as_deref(), Some("SMDPad,CuDef"));
    assert_eq!(func_of("C,1.600000").as_deref(), Some("ComponentPad"));
    assert_eq!(func_of("C,0.600000").as_deref(), Some("ViaPad"));
    assert_eq!(func_of("C,0.250000").as_deref(), Some("Conductor"));
    // 4 SMD + 2 THT + 1 via flashes, at pad centers.
    assert_eq!(p.flashes.len(), 7);
    let at = |x: f64, y: f64| p.flashes.iter().any(|f| near((f.1, f.2), (x, y)));
    for (x, y) in [(9.2, 10.0), (10.8, 10.0), (20.0, 9.2), (20.0, 10.8), (30.0, 15.0), (32.54, 15.0), (5.0, 5.0)] {
        assert!(at(x, y), "no flash at ({x}, {y})");
    }
    // R1 and R2 pads of the same orientation use the same D code.
    let d = |x: f64, y: f64| p.flashes.iter().find(|f| near((f.1, f.2), (x, y))).unwrap().0;
    assert_eq!(d(9.2, 10.0), d(10.8, 10.0));
    assert_eq!(d(20.0, 9.2), d(20.0, 10.8));
    // Arc track drawn as one G03 arc.
    assert_eq!(p.arcs, 1);
    assert!(s.contains("G75*"));
    assert!(s.contains("G03*\nX25000000Y22000000I-2000000J0D01*"), "{s}");
    // Object attributes.
    assert!(s.contains("%TO.P,R1,1*%"));
    assert!(s.contains("%TO.N,GND*%"));
}

#[test]
fn file_attributes_per_layer() {
    let doc = sample();
    let expected = [
        (".GTL", "Copper,L1,Top", "Positive"),
        (".GBL", "Copper,L2,Bot", "Positive"),
        (".GTS", "Soldermask,Top", "Negative"),
        (".GBS", "Soldermask,Bot", "Negative"),
        (".GTP", "Paste,Top", "Positive"),
        (".GBP", "Paste,Bot", "Positive"),
        (".GTO", "Legend,Top", "Positive"),
        (".GBO", "Legend,Bot", "Positive"),
        (".GKO", "Profile,NP", "Positive"),
    ];
    let layers = standard_layers(&doc);
    assert_eq!(layers.len(), expected.len());
    for ((f, ext), (e_ext, e_fn, e_pol)) in layers.into_iter().zip(expected) {
        assert_eq!(ext, e_ext);
        let s = write_layer(&doc, f, &opts()).unwrap();
        let p = parse(&s);
        assert!(p.file_attrs.contains(&format!("TF.FileFunction,{e_fn}")), "{ext}: {:?}", p.file_attrs);
        assert!(p.file_attrs.contains(&format!("TF.FilePolarity,{e_pol}")));
        assert!(p.file_attrs.iter().any(|a| a.starts_with("TF.GenerationSoftware,PCBPlannerCAD,PCBPlannerCAD,")));
        assert!(p.file_attrs.iter().any(|a| a.starts_with("TF.CreationDate,20")));
        assert!(p.file_attrs.iter().any(|a| a.starts_with("TF.ProjectId,Test,")));
        assert!(s.contains("%FSLAX46Y46*%\n") && s.contains("%MOMM*%\n%LPD*%\n"));
        assert_eq!(p.last_statement, "M02", "{ext}");
        assert!(s.ends_with("M02*\n"));
    }
    // Inner layers of a 4-layer stack.
    let mut doc4 = doc.clone();
    let (id, _) = board_settings(&doc4).unwrap();
    let b = BoardSettings { stackup: pcbplannercad_core::pcb::Stackup::with_copper(4), ..Default::default() };
    doc4 = Document::from_parts(doc4.kind(), doc4.items().map(|(i, it)| (i, if i == id { Item::Board(b.clone()) } else { it.clone() })).collect(), Default::default()).with_footprints(doc.footprint_cache().clone());
    let names: Vec<String> = standard_layers(&doc4).into_iter().map(|(_, e)| e).collect();
    assert_eq!(&names[..4], &[".GTL", ".G1", ".G2", ".GBL"]);
    let s = write_layer(&doc4, LayerFunction::Copper(layer::inner_copper(1)), &opts()).unwrap();
    assert!(s.contains("%TF.FileFunction,Copper,L2,Inr*%"));
    // Inner layer: THT pads and through vias only.
    assert_eq!(parse(&s).flashes.len(), 3);
    assert!(write_layer(&doc, LayerFunction::Copper(layer::inner_copper(1)), &opts()).is_err());
}

#[test]
fn zone_with_hole_is_one_cut_in_region() {
    let sq = |x0: f64, y0: f64, s: f64| vec![Point::new(mm(x0), mm(y0)), Point::new(mm(x0 + s), mm(y0)), Point::new(mm(x0 + s), mm(y0 + s)), Point::new(mm(x0), mm(y0 + s))];
    let outer = sq(0.0, 0.0, 20.0);
    let mut hole = sq(5.0, 5.0, 4.0);
    hole.reverse();
    let fill = vec![ZonePolygon { outer: outer.clone(), holes: vec![hole, sq(12.0, 6.0, 3.0)] }, ZonePolygon { outer: sq(30.0, 0.0, 5.0), holes: vec![] }];
    let zone = Zone {
        teardrop: false,
        outline: outer,
        layer: layer::BOTTOM_COPPER,
        net: "GND".into(),
        name: String::new(),
        priority: 0,
        clearance: 0,
        min_width: 0,
        connect: Default::default(),
        thermal_gap: 0,
        spoke_width: 0,
        keepout: false,
        remove_islands: true,
        fill,
    };
    let doc = board_doc(vec![Item::Zone(zone)], &[]);
    let s = write_layer(&doc, LayerFunction::Copper(layer::BOTTOM_COPPER), &opts()).unwrap();
    let p = parse(&s);
    assert!(!p.lpc && !s.contains("%LPC"));
    assert_eq!(p.regions.len(), 2);
    assert_eq!(p.region_functions[0].as_deref(), Some("Conductor"));
    let r = &p.regions[0];
    // outer 4 + 2 × (hole 4 + bridge point + repeated hole start + bridge point) + closing point
    assert_eq!(r.len(), 4 + 2 * 7 + 1, "{r:?}");
    assert!(near(r[0], *r.last().unwrap()));
    // Area of the single contour (shoelace) equals outer minus holes.
    let area: f64 = (0..r.len() - 1).map(|i| r[i].0 * r[i + 1].1 - r[i + 1].0 * r[i].1).sum::<f64>() / 2.0;
    assert!((area - (400.0 - 16.0 - 9.0)).abs() < 1e-6, "area {area}");
    // Every bridge is traversed once in each direction: hole points appear once each.
    for (x, y) in [(5.0, 9.0), (9.0, 5.0), (15.0, 9.0)] {
        assert_eq!(r.iter().filter(|q| near(**q, (x, y))).count(), 1);
    }
}

#[test]
fn coordinate_precision_round_trips() {
    let a = Point::new(1_234_567, -7_654_321);
    let b = Point::new(12_345_678_901 / 1000, 5);
    let doc = board_doc(vec![Item::Track(Track { a, b, width: mm(0.2), layer: layer::TOP_COPPER, net: String::new() })], &[]);
    for precision in [3u8, 4, 5, 6] {
        let o = GerberOptions { precision, ..opts() };
        let s = write_layer(&doc, LayerFunction::Copper(layer::TOP_COPPER), &o).unwrap();
        assert!(s.contains(&format!("%FSLAX4{precision}Y4{precision}*%")));
        let p = parse(&s);
        assert_eq!(p.dec, precision as u32);
        let half = 0.5 * 10f64.powi(-(precision as i32)) + 1e-12;
        let d = p.draws[0];
        assert!((d.0 - b.x as f64 / 1e6).abs() <= half && (d.1 - b.y as f64 / 1e6).abs() <= half, "{precision}: {d:?}");
        let ap = p.apertures.values().next().unwrap();
        assert_eq!(ap.0, format!("C,{}", fmt_fixed(div_round(mm(0.2), 10i64.pow(6 - precision as u32)), precision as u32)));
    }
    let s = write_layer(&doc, LayerFunction::Copper(layer::TOP_COPPER), &opts()).unwrap();
    assert!(s.contains("X1234567Y-7654321D02*"));
    assert!(s.contains("X12345678Y5D01*"));
    // 4 decimals: −7.654321 → −7.6543, 1.234567 → 1.2346 (half away from zero).
    let s = write_layer(&doc, LayerFunction::Copper(layer::TOP_COPPER), &GerberOptions { precision: 4, ..opts() }).unwrap();
    assert!(s.contains("X12346Y-76543D02*"));
}

#[test]
fn mask_and_paste_follow_expansions() {
    let doc = sample();
    let mask = parse(&write_layer(&doc, LayerFunction::SolderMask { bottom: false }, &opts()).unwrap());
    let templates: Vec<&str> = mask.apertures.values().map(|(t, _)| t.as_str()).collect();
    // Default mask expansion 0.05 mm per side; vias tented.
    assert!(templates.contains(&"R,1.100000X0.600000"), "{templates:?}");
    assert!(templates.contains(&"C,1.700000"));
    assert!(!templates.contains(&"C,0.700000"));
    assert_eq!(mask.flashes.len(), 6);
    let untented = parse(&write_layer(&doc, LayerFunction::SolderMask { bottom: false }, &GerberOptions { tent_vias: false, ..opts() }).unwrap());
    assert_eq!(untented.flashes.len(), 7);
    assert!(untented.apertures.values().any(|(t, f)| t == "C,0.700000" && f.as_deref() == Some("ViaPad")));
    // Bottom mask: only the THT pads.
    let bmask = parse(&write_layer(&doc, LayerFunction::SolderMask { bottom: true }, &opts()).unwrap());
    assert_eq!(bmask.flashes.len(), 2);
    // Paste: SMD pads only, default expansion 0.
    let paste = parse(&write_layer(&doc, LayerFunction::Paste { bottom: false }, &opts()).unwrap());
    assert_eq!(paste.flashes.len(), 4);
    assert!(paste.apertures.values().all(|(t, _)| t.starts_with("R,")));
    let bpaste = parse(&write_layer(&doc, LayerFunction::Paste { bottom: true }, &opts()).unwrap());
    assert!(bpaste.flashes.is_empty() && bpaste.apertures.is_empty());
}

#[test]
fn silk_designators_follow_options() {
    let doc = sample();
    let with = parse(&write_layer(&doc, LayerFunction::Silk { bottom: false }, &opts()).unwrap());
    let without = parse(&write_layer(&doc, LayerFunction::Silk { bottom: false }, &GerberOptions { include_ref_des: false, ..opts() }).unwrap());
    let values = parse(&write_layer(&doc, LayerFunction::Silk { bottom: false }, &GerberOptions { include_values: true, include_ref_des: false, ..opts() }).unwrap());
    assert!(!with.draws.is_empty());
    assert!(without.draws.is_empty());
    // Values are empty strings in the sample: nothing to plot.
    assert!(values.draws.is_empty());
    // Text stroke width follows the font: 0.13 × 1 mm.
    assert_eq!(with.apertures.values().next().unwrap().0, "C,0.130000");
    // Designator "R1" is centered around (10, 11).
    let xs: Vec<f64> = with.draws.iter().filter(|d| d.1 > 10.4 && d.1 < 11.6 && d.0 < 15.0).map(|d| d.0).collect();
    assert!(!xs.is_empty());
    let (lo, hi) = xs.iter().fold((f64::MAX, f64::MIN), |(a, b), &x| (a.min(x), b.max(x)));
    assert!(lo < 10.0 && hi > 10.0);
}

#[test]
fn rotated_and_rounded_pads_use_macros() {
    let f = footprint_with(vec![
        smd_pad("1", 0.0, 1.0, 0.5, PadShape::Rect, 30.0),
        smd_pad("2", 2.0, 1.0, 0.5, PadShape::RoundRect, 0.0),
        smd_pad("3", 4.0, 1.0, 0.5, PadShape::Oval, 45.0),
        smd_pad("4", 6.0, 1.0, 0.5, PadShape::Oval, 90.0),
    ]);
    let doc = board_doc(vec![place("U1", "L/U", Point::default(), 0.0)], &[("L/U", f)]);
    let s = write_layer(&doc, LayerFunction::Copper(layer::TOP_COPPER), &opts()).unwrap();
    let p = parse(&s);
    assert_eq!(p.macros, vec!["RotRect", "RotOval", "RoundRect"]);
    let t: Vec<&str> = p.apertures.values().map(|(t, _)| t.as_str()).collect();
    assert!(t.contains(&"RotRect,1.000000X0.500000X30"), "{t:?}");
    // corner radius 0.25 × 0.5 = 0.125; corner centers (±0.375, ±0.125)
    assert!(t.contains(&"RoundRect,0.125000X0.375000X0.125000X-0.375000X0.125000X-0.375000X-0.125000X0.375000X-0.125000"), "{t:?}");
    assert!(t.contains(&"O,0.500000X1.000000"));
    assert!(t.iter().any(|x| x.starts_with("RotOval,0.500000X-0.176777X-0.176777X0.176777X0.176777")), "{t:?}");
    assert_eq!(p.flashes.len(), 4);
}

#[test]
fn outline_is_profile_and_can_go_on_all_layers() {
    let doc = sample();
    let s = write_layer(&doc, LayerFunction::Outline, &opts()).unwrap();
    let p = parse(&s);
    assert_eq!(p.apertures.len(), 1);
    assert_eq!(p.apertures.values().next().unwrap().1.as_deref(), Some("Profile"));
    assert_eq!(p.draws.len(), 4);
    let cu = parse(&write_layer(&doc, LayerFunction::Copper(layer::TOP_COPPER), &GerberOptions { plot_outline_on_all: true, ..opts() }).unwrap());
    assert!(cu.apertures.values().any(|(_, f)| f.as_deref() == Some("Profile")));
}

#[test]
fn fracture_handles_nested_order_and_orientation() {
    // Holes given CCW are reversed; the result is one contour whose area is outer − holes.
    let outer = vec![Point::new(0, 0), Point::new(0, 100), Point::new(100, 100), Point::new(100, 0)]; // CW
    let h1 = vec![Point::new(60, 10), Point::new(80, 10), Point::new(80, 30), Point::new(60, 30)];
    let h2 = vec![Point::new(10, 50), Point::new(30, 50), Point::new(30, 70), Point::new(10, 70)];
    let c = fracture(&outer, &[h1, h2]);
    assert!((signed_area(&c) - (10_000.0 - 400.0 - 400.0)).abs() < 1e-6);
}

#[test]
fn dimensions_plot_as_strokes_on_their_layer() {
    use pcbplannercad_core::pcb::dimension::{DimUnits, Dimension};
    let mut items = sample().item_list();
    items.push(Item::Dimension(Dimension { a: Point::new(0, 0), b: Point::new(mm(20.0), 0), offset: mm(5.0), orthogonal: false, angle: 0.0, layer: layer::TOP_SILK, width: mm(0.15), text_height: mm(1.0), units: DimUnits::Mm, precision: 1 }));
    let doc = Document::from_item_list(pcbplannercad_core::DocKind::Pcb, items).with_footprints(sample().footprint_cache().clone());
    let before = parse(&write_layer(&sample(), LayerFunction::Silk { bottom: false }, &opts()).unwrap()).draws.len();
    let after = parse(&write_layer(&doc, LayerFunction::Silk { bottom: false }, &opts()).unwrap());
    assert!(after.draws.len() > before + 7, "extension lines, dimension line, arrows and the text");
    // The dimension line runs along y = 5 mm from x = 0 to 20.
    assert!(after.draws.iter().any(|d| near((d.0, d.1), (20.0, 5.0))));
}
