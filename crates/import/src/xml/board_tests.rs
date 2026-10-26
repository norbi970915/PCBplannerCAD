use std::collections::HashMap;
use std::path::{Path, PathBuf};

use pcbplannercad_core::geom::{mirror_x, on_segment, rotate};
use pcbplannercad_core::io::{document_from_str, document_to_string};
use pcbplannercad_core::item::{Arc, Item};
use pcbplannercad_core::layer::{self, inner_copper};
use pcbplannercad_core::pcb::geometry::{WorldPad, all_pads, board_settings};
use pcbplannercad_core::pcb::{RuleKind, Side, ZoneConnect};
use pcbplannercad_core::units::{Point, mm};
use pcbplannercad_core::{DocKind, Document};

use super::board::{parse_len, setup_layers, via_extent};
use super::util::{parse_rot, parse_xml};
use super::*;


#[test]
fn design_rule_values_parse() {
    assert_eq!(parse_len("0.15mm"), Some(0.15));
    assert!((parse_len("10mil").unwrap() - 0.254).abs() < 1e-12);
    assert!((parse_len("0.1inch").unwrap() - 2.54).abs() < 1e-12);
    assert_eq!(parse_len("500mic"), Some(0.5));
    assert_eq!(parse_len("2"), Some(2.0));
    assert_eq!(parse_len("abc"), None);
    assert_eq!(setup_layers("(1*16)").into_iter().collect::<Vec<_>>(), vec![1, 16]);
    assert_eq!(setup_layers("[1:1+2*15:16]").into_iter().collect::<Vec<_>>(), vec![1, 2, 15, 16]);
    assert_eq!(via_extent(Some("1-16")), (1, 16));
    assert_eq!(via_extent(Some("16-2")), (2, 16));
    assert_eq!(via_extent(None), (1, 16));
}

const TEST_BRD: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<!DOCTYPE eagle SYSTEM "eagle.dtd">
<eagle version="9.6.2"><drawing><board>
<plain>
<wire x1="0" y1="0" x2="40" y2="0" width="0" layer="20"/>
<wire x1="40" y1="0" x2="40" y2="30" width="0" layer="20"/>
<wire x1="40" y1="30" x2="0" y2="30" width="0" layer="20"/>
<wire x1="0" y1="30" x2="0" y2="0" width="0" layer="20" curve="0"/>
<text x="1" y="28" size="1.27" layer="21">TOP</text>
<hole x="2" y="2" drill="3.2"/>
<rectangle x1="30" y1="20" x2="35" y2="25" layer="41"/>
</plain>
<libraries><library name="lib">
<packages><package name="R0603">
<smd name="1" x="-1" y="0" dx="1" dy="1.2" layer="1"/>
<smd name="2" x="1" y="0" dx="1" dy="1.2" layer="1"/>
<wire x1="-0.5" y1="0.7" x2="0.5" y2="0.7" width="0.1" layer="21"/>
<text x="0" y="1" size="1" layer="25">&gt;NAME</text>
</package>
<package name="TH">
<pad name="A" x="-2.54" y="0" drill="0.8"/>
<pad name="B" x="2.54" y="0" drill="0.8" diameter="1.2" shape="long"/>
</package></packages>
</library></libraries>
<designrules name="test">
<param name="layerSetup" value="(1*2*15*16)"/>
<param name="mdWireWire" value="8mil"/>
<param name="mdWirePad" value="0.15mm"/>
<param name="mdPadPad" value="0.2mm"/>
<param name="msWidth" value="0.127mm"/>
<param name="msDrill" value="0.35mm"/>
<param name="rvPadTop" value="0.25"/>
<param name="rlMinPadTop" value="10mil"/>
<param name="rlMaxPadTop" value="20mil"/>
<param name="rvViaOuter" value="0.25"/>
<param name="rlMinViaOuter" value="8mil"/>
<param name="rlMaxViaOuter" value="20mil"/>
<param name="mlMinStopFrame" value="4mil"/>
<param name="mlMaxStopFrame" value="4mil"/>
<param name="mdCopperDimension" value="0.5mm"/>
<param name="slThermalIsolate" value="10mil"/>
<param name="psElongationLong" value="100"/>
</designrules>
<classes>
<class number="0" name="default" width="0.2" drill="0.4"><clearance class="0" value="0.2"/></class>
<class number="1" name="power" width="0.5" drill="0.6"><clearance class="1" value="0.3"/></class>
</classes>
<elements>
<element name="R1" library="lib" package="R0603" value="10k" x="10" y="10" rot="R90" locked="yes"/>
<element name="R2" library="lib" package="R0603" value="1k" x="20" y="10" rot="MR90" smashed="yes">
<attribute name="NAME" x="20" y="12" size="1" layer="26" rot="MR90"/>
<attribute name="MPN" value="RC0603" layer="28"/>
</element>
<element name="J1" library="lib" package="TH" value="" x="30" y="10"/>
</elements>
<signals>
<signal name="N1">
<contactref element="R1" pad="1"/>
<contactref element="R2" pad="1"/>
<wire x1="10" y1="9" x2="15" y2="9" width="0.25" layer="1"/>
<wire x1="15" y1="9" x2="20" y2="9" width="0.25" layer="16"/>
<via x="15" y="9" extent="1-16" drill="0.4"/>
</signal>
<signal name="VCC" class="1">
<contactref element="R1" pad="2"/>
<contactref element="J1" pad="A"/>
<wire x1="10" y1="11" x2="27.46" y2="11" width="0.5" layer="15" curve="-20"/>
<wire x1="27.46" y1="11" x2="27.46" y2="10" width="0.5" layer="15"/>
<wire x1="0" y1="0" x2="1" y2="1" width="0.5" layer="19"/>
<polygon width="0.3" layer="1" isolate="0.4" rank="2" thermals="no" orphans="yes">
<vertex x="1" y="15"/><vertex x="15" y="15" curve="90"/><vertex x="15" y="25"/><vertex x="1" y="25"/>
</polygon>
</signal>
</signals>
</board></drawing></eagle>
"#;

fn pads_by_key(doc: &Document) -> HashMap<(String, String), WorldPad> {
    let (_, board) = board_settings(doc).unwrap();
    all_pads(doc, board).into_iter().map(|p| ((p.designator.clone(), p.number.clone()), p)).collect()
}

fn inside(p: Point, poly: &[Point]) -> bool {
    let mut c = false;
    let n = poly.len();
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        if on_segment(p, a, b) {
            return true;
        }
        if (a.y > p.y) != (b.y > p.y) {
            let x = a.x as f64 + (p.y - a.y) as f64 * (b.x - a.x) as f64 / (b.y - a.y) as f64;
            if (p.x as f64) < x {
                c = !c;
            }
        }
    }
    c
}

#[test]
fn board_elements_signals_and_rules() {
    let imp = parse_board(TEST_BRD, "demo").unwrap();
    assert_eq!(imp.file_name, "demo.arpcb");
    assert_eq!(imp.libraries.len(), 1);
    let doc = &imp.document;
    assert_eq!(doc.kind(), DocKind::Pcb);
    let (_, b) = board_settings(doc).unwrap();

    // Stack from layerSetup: 4 copper layers, XML 2/15 → inner 1/2.
    assert_eq!(b.stackup.copper_layers(), vec![layer::TOP_COPPER, inner_copper(1), inner_copper(2), layer::BOTTOM_COPPER]);
    for r in &b.rules {
        match &r.kind {
            RuleKind::Clearance { min, .. } => assert_eq!(*min, mm(0.15)),
            RuleKind::Width { min, preferred, .. } => assert_eq!((*min, *preferred), (mm(0.127), mm(0.2))),
            RuleKind::HoleSize { min, .. } => assert_eq!(*min, mm(0.35)),
            RuleKind::SolderMaskExpansion { expansion, .. } => assert_eq!(*expansion, mm(0.1016)),
            RuleKind::BoardOutlineClearance { min } => assert_eq!(*min, mm(0.5)),
            RuleKind::ViaStyle { drill, diameter, .. } => assert_eq!((*drill, *diameter), (mm(0.4), mm(0.8064))),
            _ => {}
        }
    }
    assert_eq!(b.net_classes[0].name, "Default");
    assert_eq!(b.net_classes[0].nets, vec!["N1".to_string()]);
    let power = &b.net_classes[1];
    assert_eq!((power.name.as_str(), power.track_width, power.clearance, power.via_drill), ("power", mm(0.5), mm(0.3), mm(0.6)));
    assert_eq!(power.nets, vec!["VCC".to_string()]);

    // Footprints: R1 top R90, R2 bottom MR90 (XML rotates, then mirrors).
    let fps: Vec<_> = doc.items().filter_map(|(_, i)| if let Item::Footprint(f) = i { Some(f) } else { None }).collect();
    assert_eq!(fps.len(), 3);
    let r1 = fps.iter().find(|f| f.designator == "R1").unwrap();
    assert_eq!((r1.side, r1.rotation, r1.locked, r1.value.as_str(), r1.footprint.as_str()), (Side::Top, 90.0, true, "10k", "lib/R0603"));
    let r2 = fps.iter().find(|f| f.designator == "R2").unwrap();
    assert_eq!((r2.side, r2.rotation), (Side::Bottom, 270.0));
    assert_eq!(r2.parameters.get("MPN").map(String::as_str), Some("RC0603"));
    assert_eq!(r2.net_of("1"), "N1");
    let pads = pads_by_key(doc);
    assert_eq!(pads[&("R1".into(), "1".into())].pos, Point::new(mm(10.0), mm(9.0)));
    assert_eq!(pads[&("R1".into(), "2".into())].pos, Point::new(mm(10.0), mm(11.0)));
    // Local (-1, 0) → rotate 90 → (0, -1) → mirror → (0, -1).
    let r2p1 = &pads[&("R2".into(), "1".into())];
    assert_eq!(r2p1.pos, Point::new(mm(20.0), mm(9.0)));
    assert_eq!(r2p1.copper, vec![layer::BOTTOM_COPPER]);
    assert_eq!(r2p1.net, "N1");
    // THT pads: diameter from restring rules (0.8 + 2 × 0.254), long pad 2:1.
    let a = &pads[&("J1".into(), "A".into())];
    assert_eq!((a.width, a.pos), (mm(1.308), Point::new(mm(27.46), mm(10.0))));
    let bpad = &pads[&("J1".into(), "B".into())];
    assert_eq!((bpad.width, bpad.height), (mm(2.616), mm(1.308)));
    assert_eq!(a.copper.len(), 4);

    // Copper.
    let tracks: Vec<_> = doc.items().filter_map(|(_, i)| if let Item::Track(t) = i { Some(t) } else { None }).collect();
    assert_eq!(tracks.len(), 3);
    assert!(tracks.iter().any(|t| t.layer == layer::BOTTOM_COPPER && t.net == "N1"));
    assert!(tracks.iter().any(|t| t.layer == inner_copper(2) && t.net == "VCC"));
    let arc = doc.items().find_map(|(_, i)| if let Item::ArcTrack(a) = i { Some(a) } else { None }).unwrap();
    assert_eq!((arc.layer, arc.sweep, arc.net.as_str()), (inner_copper(2), -20.0, "VCC"));
    let as_arc = Arc { center: arc.center, radius: arc.radius, start_angle: arc.start_angle, sweep: arc.sweep, width: 0, layer: arc.layer };
    assert!((as_arc.start().x - mm(10.0)).abs() <= 2 && (as_arc.end().x - mm(27.46)).abs() <= 2);
    let via = doc.items().find_map(|(_, i)| if let Item::Via(v) = i { Some(v) } else { None }).unwrap();
    assert_eq!((via.drill, via.diameter, via.net.as_str()), (mm(0.4), mm(0.8064), "N1"));
    assert_eq!((via.from_layer, via.to_layer), (layer::TOP_COPPER, layer::BOTTOM_COPPER));
    let zones: Vec<_> = doc.items().filter_map(|(_, i)| if let Item::Zone(z) = i { Some(z) } else { None }).collect();
    let pour = zones.iter().find(|z| z.net == "VCC").unwrap();
    assert_eq!((pour.layer, pour.priority, pour.clearance, pour.connect, pour.remove_islands), (layer::TOP_COPPER, 5, mm(0.4), ZoneConnect::Solid, false));
    assert!(pour.outline.len() > 4, "curved edge tessellated");
    assert!(pour.fill.is_empty());
    let keepout = zones.iter().find(|z| z.keepout).unwrap();
    assert_eq!((keepout.layer, keepout.outline.len()), (layer::TOP_COPPER, 4));

    // Outline and plain graphics.
    let outline = doc.items().filter(|(_, i)| matches!(i, Item::Line(l) if l.layer == layer::BOARD_OUTLINE)).count();
    assert_eq!(outline, 4);
    assert_eq!(pcbplannercad_core::pcb::geometry::outline_loops(doc, 1000).len(), 1);
    assert!(doc.items().any(|(_, i)| matches!(i, Item::Text(t) if t.layer == layer::TOP_SILK && t.text == "TOP")));
    let msgs: Vec<String> = imp.report.messages.iter().map(|m| m.text.clone()).collect();
    assert!(msgs.iter().any(|m| m.contains("1 free hole")), "{msgs:?}");
    assert!(msgs.iter().any(|m| m.contains("1 airwire")), "{msgs:?}");

    let s = document_to_string(doc).unwrap();
    assert_eq!(&document_from_str(DocKind::Pcb, &s).unwrap(), doc);
}

#[test]
fn bad_board_input_is_an_error() {
    assert!(parse_board("", "x").is_err());
    assert!(parse_board("<eagle><drawing><schematic/></drawing></eagle>", "x").is_err());
    let junk = r#"<eagle><drawing><board><elements><element name="X" library="none" package="P" x="a" y="1e999" rot="Mxx"/></elements>
        <signals><signal name="S"><via x="0" y="0" drill="-1"/><wire x1="0" y1="0" x2="1" y2="1" width="-3" layer="1"/>
        <polygon layer="1"><vertex x="0" y="0"/></polygon></signal></signals></board></drawing></eagle>"#;
    let imp = parse_board(junk, "j").unwrap();
    for (_, i) in imp.document.items() {
        assert!(i.validate().is_ok());
    }
}

// ---------- Real boards ----------

fn find_boards(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            find_boards(&p, out);
        } else if p.extension().is_some_and(|e| e.eq_ignore_ascii_case("brd")) {
            out.push(p);
        }
    }
}


/// Pad name, local center, inscribed radius.
type PadProbe = (String, Point, i64);

/// XML's own pad placement (rotate, then mirror), independent of the
/// importer: `(element, pad)` → (center, radius of a circle surely inside the pad).
fn xml_pads(xml: &roxmltree::Document<'_>) -> HashMap<(String, String), (Point, i64)> {
    let mut packages: HashMap<(String, String), Vec<PadProbe>> = HashMap::new();
    for lib in xml.descendants().filter(|n| n.has_tag_name("library")) {
        let lname = lib.attribute("name").unwrap_or("").to_string();
        for p in lib.descendants().filter(|n| n.has_tag_name("package")) {
            let pads = p
                .children()
                .filter_map(|c| {
                    let r = if c.has_tag_name("smd") {
                        super::util::coord(c, "dx").abs().min(super::util::coord(c, "dy").abs()) / 2
                    } else if c.has_tag_name("pad") {
                        super::util::coord(c, "drill") / 2
                    } else {
                        return None;
                    };
                    Some((c.attribute("name").unwrap_or("").to_string(), super::util::point(c, "x", "y"), r))
                })
                .collect();
            packages.insert((lname.clone(), p.attribute("name").unwrap_or("").to_string()), pads);
        }
    }
    let mut out = HashMap::new();
    for e in xml.descendants().filter(|n| n.has_tag_name("element")) {
        let key = (e.attribute("library").unwrap_or("").to_string(), e.attribute("package").unwrap_or("").to_string());
        let origin = super::util::point(e, "x", "y");
        let rot = parse_rot(e.attribute("rot").unwrap_or(""));
        for (name, local, r) in packages.get(&key).into_iter().flatten() {
            let mut w = rotate(*local, Point::default(), rot.angle);
            if rot.mirror {
                w = mirror_x(w, Point::default());
            }
            out.insert((e.attribute("name").unwrap_or("").to_string(), name.clone()), (w.translated(origin.x, origin.y), *r));
        }
    }
    out
}

fn dist(a: Point, b: Point) -> f64 {
    ((a.x - b.x) as f64).hypot((a.y - b.y) as f64)
}

#[test]
fn example_boards_keep_copper_on_pads() {
    let Some(root) = std::env::var_os("PCAD_TEST_XML_EXAMPLES").map(PathBuf::from) else { return };
    if !root.is_dir() {
        println!("note: XML fixtures are unavailable; skipping");
        return;
    }
    let mut files = Vec::new();
    find_boards(&root, &mut files);
    assert!(!files.is_empty(), "no board fixtures found");
    for f in files {
        let imp = import_board(&f).unwrap();
        let doc = &imp.document;
        let text = std::fs::read_to_string(&f).unwrap();
        let xml = parse_xml(&text).unwrap();
        let pads = pads_by_key(doc);
        let source_pads = xml_pads(&xml);

        // 1. Pad centers match XML's transform exactly.
        let pos_ok = pads.iter().filter(|(k, p)| source_pads.get(*k).map(|e| e.0) == Some(p.pos)).count();

        // 2. Every XML contactref is a pad net, and every signal wire end that
        //    XML puts on one of its pads lies inside that imported pad.
        let (mut refs, mut refs_ok, mut ends, mut ends_ok) = (0, 0, 0, 0);
        let polys: HashMap<&(String, String), Vec<Point>> = pads.iter().map(|(k, p)| (k, p.polygon(0))).collect();
        for s in xml.descendants().filter(|n| n.has_tag_name("signal")) {
            let net = s.attribute("name").unwrap_or("");
            let contacts: Vec<(String, String)> = s
                .children()
                .filter(|c| c.has_tag_name("contactref"))
                .map(|c| (c.attribute("element").unwrap_or("").to_string(), c.attribute("pad").unwrap_or("").to_string()))
                .collect();
            for k in &contacts {
                refs += 1;
                if pads.get(k).is_some_and(|p| p.net == net) {
                    refs_ok += 1;
                }
            }
            for w in s.children().filter(|c| c.has_tag_name("wire")) {
                let l: u32 = w.attribute("layer").and_then(|l| l.parse().ok()).unwrap_or(0);
                if !(1..=16).contains(&l) {
                    continue;
                }
                for e in [super::util::point(w, "x1", "y1"), super::util::point(w, "x2", "y2")] {
                    for k in &contacts {
                        let Some(&(c, r)) = source_pads.get(k) else { continue };
                        if dist(e, c) > r as f64 {
                            continue;
                        }
                        ends += 1;
                        let Some(p) = pads.get(k) else { continue };
                        let layer_ok = match l {
                            1 => p.on_layer(layer::TOP_COPPER),
                            16 => p.on_layer(layer::BOTTOM_COPPER),
                            _ => p.copper.len() > 2,
                        };
                        if layer_ok && p.net == net && inside(e, &polys[k]) {
                            ends_ok += 1;
                        } else {
                            println!("  wire end {e:?} ({net}, layer {l}) not inside imported pad {k:?}");
                        }
                    }
                }
            }
        }

        // 3. Imported tracks touching pads of another net (informational: XML
        //    designs may bridge solder jumpers with copper on purpose).
        let polys_all: Vec<(&WorldPad, &Vec<Point>)> = pads.iter().map(|(k, p)| (p, &polys[k])).collect();
        let mut mismatches = Vec::new();
        let mut track_ends = 0;
        for (_, i) in doc.items() {
            let Item::Track(t) = i else { continue };
            for e in [t.a, t.b] {
                track_ends += 1;
                for (p, poly) in &polys_all {
                    if p.on_layer(t.layer) && p.net != t.net && inside(e, poly) {
                        mismatches.push(format!("{} end {e:?} in pad {}.{} ({})", t.net, p.designator, p.number, p.net));
                    }
                }
            }
        }

        // 4. Via count and round trip.
        let vias = doc.items().filter(|(_, i)| matches!(i, Item::Via(_))).count();
        let xml_vias = xml.descendants().filter(|n| n.has_tag_name("via")).count();
        let s = document_to_string(doc).unwrap();
        let back = document_from_str(DocKind::Pcb, &s).unwrap();

        let pct = 100.0 * ends_ok as f64 / ends.max(1) as f64;
        println!(
            "{}: {} footprints, {} pads ({pos_ok} at XML positions), contactrefs as pad nets {refs_ok}/{refs}, \
             wire ends on XML pads inside imported same-net pads {ends_ok}/{ends} ({pct:.2}%), \
             track ends touching other-net pads {}/{track_ends}, {vias} vias (XML {xml_vias}), round trip {}",
            f.display(),
            doc.items().filter(|(_, i)| matches!(i, Item::Footprint(_))).count(),
            pads.len(),
            mismatches.len(),
            if back == *doc { "ok" } else { "CHANGED" }
        );
        for m in &mismatches {
            println!("  {m}");
        }
        for m in &imp.report.messages {
            println!("  {m}");
        }
        assert_eq!(pos_ok, pads.len(), "pad positions differ from XML's transform");
        assert_eq!(refs_ok, refs, "contactrefs missing from pad nets");
        assert!(ends > 0 && pct >= 99.0, "only {pct:.2}% of wire ends land inside their imported pads");
        assert!(mismatches.len() * 500 <= track_ends.max(1), "too many tracks touch pads of other nets: {mismatches:?}");
        assert_eq!(vias, xml_vias);
        assert_eq!(&back, doc, "document changed in the save/load round trip");
    }
}
