use std::path::{Path, PathBuf};
use std::time::Instant;

use pcbplannercad_core::geom::normalize_deg;
use pcbplannercad_core::io::{document_from_str, document_to_string};
use pcbplannercad_core::item::{DESIGNATOR_TOKEN, Item, PadKind};
use pcbplannercad_core::layer;
use pcbplannercad_core::pcb::geometry::{all_pads, board_settings, world_graphics};
use pcbplannercad_core::pcb::{Side, ZoneConnect};
use pcbplannercad_core::units::{Coord, Point, mm};
use pcbplannercad_core::{DocKind, Document};

use super::sexpr::{List, parse};
use super::*;
use crate::BoardImport;

fn library_share() -> Option<PathBuf> {
    let p=std::env::var_os("PCAD_TEST_STRUCTURED_SHARE").map(PathBuf::from)?;
    p.is_dir().then_some(p)
}

/// structured format's own placement math (Y-down, angles counter-clockwise on screen),
/// converted to our Y-up world: the reference the importer must reproduce.
fn structured_world(fp_at: (f64, f64, f64), local: (f64, f64)) -> Point {
    let (s, c) = fp_at.2.to_radians().sin_cos();
    let (x, y) = local;
    let wx = fp_at.0 + x * c + y * s;
    let wy = fp_at.1 - x * s + y * c;
    Point::new(mm(wx), -mm(wy))
}

fn near(a: Point, b: Point, tol: Coord) -> bool {
    (a.x - b.x).abs() <= tol && (a.y - b.y).abs() <= tol
}

/// Every pad's world position/rotation/layer from the import must equal
/// structured format's, computed directly from the board file. Returns (checked, failures).
fn check_pads_against_structured(text: &str, imp: &BoardImport) -> (usize, Vec<String>) {
    let root = parse(text).unwrap();
    let doc = &imp.document;
    let (_, settings) = board_settings(doc).unwrap();
    let pads = all_pads(doc, settings);
    let mut checked = 0;
    let mut failures = Vec::new();
    let fps: Vec<&List> = root.lists().filter(|l| l.head() == "footprint" || l.head() == "module").collect();
    let placed: Vec<_> = doc.items().filter_map(|(id, i)| if let Item::Footprint(f) = i { Some((id, f)) } else { None }).collect();
    assert_eq!(fps.len(), placed.len());
    for (node, (id, fp)) in fps.iter().zip(&placed) {
        let at = node.child("at").unwrap();
        let fat = (at.num(1).unwrap(), at.num(2).unwrap(), at.num(3).unwrap_or(0.0));
        let bottom = node.child_str("layer") == Some("B.Cu");
        assert_eq!(fp.side == Side::Bottom, bottom);
        let mine: Vec<_> = pads.iter().filter(|p| p.owner == *id).collect();
        // The pads the importer keeps as pads: holes with a drill, SMD pads with copper.
        let becomes_pad = |p: &&List| {
            let has_cu = p.child("layers").is_some_and(|l| l.items.iter().any(|a| a.as_atom().is_some_and(|s| s.ends_with(".Cu"))));
            match p.atom(2) {
                Some("thru_hole" | "np_thru_hole") => p
                    .child("drill")
                    .and_then(|d| d.items.iter().skip(1).find_map(|a| a.as_atom().and_then(|s| s.parse::<f64>().ok())))
                    .is_some_and(|v| v > 0.0),
                _ => has_cu && p.child("size").and_then(|s| s.num(1)).is_some_and(|w| w > 0.0),
            }
        };
        let theirs: Vec<&List> = node.children("pad").filter(becomes_pad).collect();
        assert_eq!(mine.len(), theirs.len(), "{}", fp.designator);
        for (p, w) in theirs.iter().zip(&mine) {
            checked += 1;
            let pat = p.child("at").unwrap();
            let expect = structured_world(fat, (pat.num(1).unwrap(), pat.num(2).unwrap()));
            let rot = normalize_deg(pat.num(3).unwrap_or(0.0));
            // Pad shapes are point-symmetric: compare modulo a half turn (quarter turn for squares).
            let diff = normalize_deg(w.rotation - rot);
            let period = if w.width == w.height { 90.0 } else { 180.0 };
            let rot_ok = {
                let m = diff % period;
                m.abs() < 1e-3 || (period - m).abs() < 1e-3
            };
            let layer_ok = match w.kind {
                PadKind::Smd => {
                    let names: Vec<_> = p.child("layers").unwrap().items.iter().filter_map(|a| a.as_atom()).collect();
                    let expect = if names.contains(&"B.Cu") && !names.contains(&"F.Cu") { layer::BOTTOM_COPPER } else { layer::TOP_COPPER };
                    w.copper == vec![expect]
                }
                _ => true,
            };
            if !near(w.pos, expect, 2) || !rot_ok || !layer_ok {
                failures.push(format!(
                    "{} pad {}: got {:?} rot {} {:?}, structured format {:?} rot {rot}",
                    fp.designator, w.number, w.pos, w.rotation, w.copper, expect
                ));
            }
        }
    }
    (checked, failures)
}

fn point_in_polygon(p: Point, poly: &[Point]) -> bool {
    let (x, y) = (p.x as f64, p.y as f64);
    let mut inside = false;
    let n = poly.len();
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        let (ax, ay, bx, by) = (a.x as f64, a.y as f64, b.x as f64, b.y as f64);
        if (ay > y) != (by > y) && x < (bx - ax) * (y - ay) / (by - ay) + ax {
            inside = !inside;
        }
    }
    inside
}

fn dist_to_segment(p: Point, a: Point, b: Point) -> f64 {
    let (px, py, ax, ay, bx, by) = (p.x as f64, p.y as f64, a.x as f64, a.y as f64, b.x as f64, b.y as f64);
    let (dx, dy) = (bx - ax, by - ay);
    let l2 = dx * dx + dy * dy;
    let t = if l2 == 0.0 { 0.0 } else { (((px - ax) * dx + (py - ay) * dy) / l2).clamp(0.0, 1.0) };
    ((px - ax - t * dx).powi(2) + (py - ay - t * dy).powi(2)).sqrt()
}

#[derive(Default, Debug)]
struct Copper {
    endpoints: usize,
    on_pad: usize,
    on_via: usize,
    on_track: usize,
    in_zone: usize,
    dangling: usize,
    net_checked: usize,
    net_mismatch: usize,
    mismatches: Vec<String>,
    source_nc_violations: usize,
    dangling_examples: Vec<String>,
}

/// Spatial hash of item indices by 1 mm cells.
#[derive(Default)]
struct Grid {
    cells: std::collections::HashMap<(i64, i64), Vec<usize>>,
}

impl Grid {
    const CELL: i64 = 1_000_000;

    fn insert(&mut self, idx: usize, min: Point, max: Point) {
        for cx in min.x.div_euclid(Self::CELL)..=max.x.div_euclid(Self::CELL) {
            for cy in min.y.div_euclid(Self::CELL)..=max.y.div_euclid(Self::CELL) {
                self.cells.entry((cx, cy)).or_default().push(idx);
            }
        }
    }

    fn at(&self, p: Point) -> &[usize] {
        self.cells.get(&(p.x.div_euclid(Self::CELL), p.y.div_euclid(Self::CELL))).map_or(&[], |v| v.as_slice())
    }
}

fn bbox(pts: &[Point], grow: Coord) -> (Point, Point) {
    let (mut lo, mut hi) = (pts[0], pts[0]);
    for p in pts {
        lo = Point::new(lo.x.min(p.x), lo.y.min(p.y));
        hi = Point::new(hi.x.max(p.x), hi.y.max(p.y));
    }
    (lo.translated(-grow, -grow), hi.translated(grow, grow))
}

/// Classifies every track endpoint: on a pad (and same net), on a via, on
/// another track, inside a same-net zone fill, or dangling.
fn copper_connectivity(doc: &Document) -> Copper {
    let (_, settings) = board_settings(doc).unwrap();
    let pads = all_pads(doc, settings);
    let pad_polys: Vec<_> = pads.iter().map(|p| p.polygon(0)).collect();
    let mut pad_grid = Grid::default();
    for (i, poly) in pad_polys.iter().enumerate() {
        // Grown so track ends touching a pad with their width are found too.
        let (lo, hi) = bbox(poly, mm(1.0));
        pad_grid.insert(i, lo, hi);
    }
    let mut segs = Vec::new(); // (a, b, half width, layer, owner track)
    let mut ends = Vec::new(); // (point, layer, net, owner track, half width)
    let mut vias = Vec::new();
    let mut zones = Vec::new();
    let mut owner = 0;
    for (_, item) in doc.items() {
        match item {
            Item::Track(t) => {
                owner += 1;
                ends.push((t.a, t.layer, t.net.clone(), owner, t.width / 2));
                ends.push((t.b, t.layer, t.net.clone(), owner, t.width / 2));
                segs.push((t.a, t.b, t.width as f64 / 2.0, t.layer, owner));
            }
            Item::ArcTrack(a) => {
                owner += 1;
                let arc = pcbplannercad_core::item::Arc {
                    center: a.center,
                    radius: a.radius,
                    start_angle: a.start_angle,
                    sweep: a.sweep,
                    width: a.width,
                    layer: a.layer,
                };
                ends.push((arc.start(), a.layer, a.net.clone(), owner, a.width / 2));
                ends.push((arc.end(), a.layer, a.net.clone(), owner, a.width / 2));
                for w in pcbplannercad_core::pcb::geometry::arc_points(&arc).windows(2) {
                    segs.push((w[0], w[1], a.width as f64 / 2.0, a.layer, owner));
                }
            }
            Item::Via(v) => vias.push((v.pos, v.diameter / 2, settings.stackup.span(v.from_layer, v.to_layer))),
            Item::Zone(z) if !z.keepout => zones.push(z),
            _ => {}
        }
    }
    let mut seg_grid = Grid::default();
    for (i, (a, b, hw, _, _)) in segs.iter().enumerate() {
        let (lo, hi) = bbox(&[*a, *b], *hw as Coord + 1);
        seg_grid.insert(i, lo, hi);
    }
    let mut via_grid = Grid::default();
    for (i, (p, r, _)) in vias.iter().enumerate() {
        via_grid.insert(i, p.translated(-r - 1, -r - 1), p.translated(r + 1, r + 1));
    }
    let mut c = Copper::default();
    for (e, l, net, own, hw) in &ends {
        c.endpoints += 1;
        // On the pad outline, or touching it with the track's own width.
        let hits: Vec<usize> = pad_grid
            .at(*e)
            .iter()
            .copied()
            .filter(|&i| pads[i].on_layer(*l) && (point_in_polygon(*e, &pad_polys[i]) || point_in_polygon(*e, &pads[i].polygon(*hw))))
            .collect();
        if !hits.is_empty() {
            c.on_pad += 1;
            // A track end touching several overlapping pads must match at least one.
            let named: Vec<_> = hits.iter().filter(|&&i| !pads[i].net.is_empty()).collect();
            if !named.is_empty() && !net.is_empty() {
                c.net_checked += 1;
                // structured format names no-connect pads "unconnected-(…)"; a track on one is a
                // DRC error in the source design, not an import error.
                let nc = named.iter().all(|&&i| pads[i].net.starts_with("unconnected-"));
                if nc {
                    c.source_nc_violations += 1;
                } else if !named.iter().any(|&&i| pads[i].net == *net) {
                    c.net_mismatch += 1;
                    c.mismatches.push(format!(
                        "{} pad {} net {} vs track net {net}",
                        pads[*named[0]].designator, pads[*named[0]].number, pads[*named[0]].net
                    ));
                }
            }
            continue;
        }
        let ex = (e.x as f64, e.y as f64);
        if via_grid.at(*e).iter().any(|&i| {
            let (p, r, span) = &vias[i];
            span.contains(l) && ((p.x as f64 - ex.0).powi(2) + (p.y as f64 - ex.1).powi(2)).sqrt() <= *r as f64 + 1.0
        }) {
            c.on_via += 1;
            continue;
        }
        if seg_grid.at(*e).iter().any(|&i| {
            let (a, b, hw, sl, o) = &segs[i];
            o != own && sl == l && dist_to_segment(*e, *a, *b) <= *hw + 1.0
        }) {
            c.on_track += 1;
            continue;
        }
        if zones.iter().any(|z| z.layer == *l && z.net == *net && z.fill.iter().any(|f| point_in_polygon(*e, &f.outer))) {
            c.in_zone += 1;
            continue;
        }
        c.dangling += 1;
        if c.dangling_examples.len() < 5 {
            c.dangling_examples.push(format!("({:.3}, {:.3}) layer {} net {net}", e.x as f64 / 1e6, -e.y as f64 / 1e6, l.0));
        }
    }
    c
}

fn assert_round_trip(doc: &Document) {
    let s = document_to_string(doc).unwrap();
    let back = document_from_str(DocKind::Pcb, &s).unwrap();
    assert_eq!(back.item_list(), doc.item_list());
    assert_eq!(back.footprint_cache(), doc.footprint_cache());
    assert!(back == *doc, "round trip changed the document");
}

// ---------- inline board ----------

const BOARD: &str = r#"(kicad_pcb (version 20221018) (generator pcbnew)
  (general (thickness 1.6))
  (layers (0 "F.Cu" signal) (1 "In1.Cu" signal) (2 "In2.Cu" signal) (31 "B.Cu" signal) (36 "B.SilkS" user) (37 "F.SilkS" user) (44 "Edge.Cuts" user))
  (setup
    (stackup
      (layer "F.SilkS" (type "Top Silk Screen") (color "White"))
      (layer "F.Mask" (type "Top Solder Mask") (color "Red") (thickness 0.01))
      (layer "F.Cu" (type "copper") (thickness 0.035))
      (layer "dielectric 1" (type "prepreg") (thickness 0.2) (material "FR4") (epsilon_r 4.3))
      (layer "In1.Cu" (type "copper") (thickness 0.035))
      (layer "dielectric 2" (type "core") (thickness 1.0) (material "FR4") (epsilon_r 4.6))
      (layer "In2.Cu" (type "copper") (thickness 0.035))
      (layer "dielectric 3" (type "prepreg") (thickness 0.2) (material "FR4") (epsilon_r 4.3))
      (layer "B.Cu" (type "copper") (thickness 0.035))
      (layer "B.Mask" (type "Bottom Solder Mask") (thickness 0.01)))
    (aux_axis_origin 10 20))
  (net 0 "") (net 1 "GND") (net 2 "SIG")
  (net_class "Default" "" (clearance 0.15) (trace_width 0.2) (via_dia 0.6) (via_drill 0.3))
  (net_class "Power" "" (clearance 0.3) (trace_width 0.5) (via_dia 0.8) (via_drill 0.4) (add_net "GND"))
  (footprint "Lib:R" (layer "F.Cu") (at 100 50 90)
    (property "Reference" "R1" (at 0 -1.5 90) (layer "F.SilkS") (effects (font (size 1 1))))
    (property "Value" "10k" (at 0 1.5 90) (layer "F.Fab") (effects (font (size 1 1))))
    (property "MPN" "RC0603" (at 0 0 90) (layer "F.Fab") (hide yes) (effects (font (size 1 1))))
    (fp_line (start -1 -0.5) (end 1 -0.5) (stroke (width 0.12)) (layer "F.SilkS"))
    (pad "1" smd rect (at -1 0 90) (size 0.8 1.2) (layers "F.Cu" "F.Mask") (net 1 "GND"))
    (pad "2" smd rect (at 1 0 120) (size 0.8 1.2) (layers "F.Cu" "F.Mask") (net 2 "SIG")))
  (footprint "Lib:R" (layer "F.Cu") (at 110 50 90)
    (property "Reference" "R2" (at 0.5 -2 90) (layer "F.SilkS") (effects (font (size 1 1))))
    (property "Value" "10k" (at 0 1.5 90) (layer "F.Fab") (effects (font (size 1 1))))
    (fp_line (start -1 -0.5) (end 1 -0.5) (stroke (width 0.12)) (layer "F.SilkS"))
    (pad "1" smd rect (at -1 0 90) (size 0.8 1.2) (layers "F.Cu" "F.Mask") (net 1 "GND"))
    (pad "2" smd rect (at 1 0 120) (size 0.8 1.2) (layers "F.Cu" "F.Mask") (net 2 "SIG")))
  (footprint "Lib:R" (layer "B.Cu") (at 120 50 30) (locked yes)
    (property "Reference" "R3" (at 0 1.5 30) (layer "B.SilkS") (effects (font (size 1 1)) (justify mirror)))
    (property "Value" "10k" (at 0 -1.5 30) (layer "B.Fab") (effects (font (size 1 1)) (justify mirror)))
    (fp_line (start -1 0.5) (end 1 0.5) (stroke (width 0.12)) (layer "B.SilkS"))
    (pad "1" smd rect (at -1 0 30) (size 0.8 1.2) (layers "B.Cu" "B.Mask") (net 1 "GND"))
    (pad "2" smd rect (at 1 0 0) (size 0.8 1.2) (layers "B.Cu" "B.Mask") (net 2 "SIG")))
  (footprint "Lib:R" (layer "F.Cu") (at 130 50)
    (property "Reference" "R4" (at 0 -1.5) (layer "F.SilkS") (effects (font (size 1 1))))
    (fp_line (start -1 -0.5) (end 1 -0.5) (stroke (width 0.2)) (layer "F.SilkS"))
    (pad "1" smd rect (at -1 0) (size 0.8 1.2) (layers "F.Cu" "F.Mask") (net 1 "GND"))
    (pad "2" smd rect (at 1 0 30) (size 0.8 1.2) (layers "F.Cu" "F.Mask") (net 2 "SIG")))
  (gr_line (start 0 0) (end 200 0) (stroke (width 0.1)) (layer "Edge.Cuts"))
  (gr_arc (start 0 0) (mid -5 5) (end 0 10) (stroke (width 0.1)) (layer "Edge.Cuts"))
  (gr_text "HELLO" (at 50 10 0) (layer "F.SilkS") (effects (font (size 1.5 1.5))))
  (dimension (type aligned) (layer "Dwgs.User"))
  (segment (start 100 49) (end 110 49) (width 0.25) (layer "F.Cu") (net 1))
  (arc (start 100 60) (mid 105 55) (end 110 60) (width 0.3) (layer "B.Cu") (net 2))
  (via (at 105 49) (size 0.6) (drill 0.3) (layers "F.Cu" "B.Cu") (net 1))
  (via blind (at 106 49) (size 0.5) (drill 0.2) (layers "F.Cu" "In1.Cu") (net 1))
  (zone (net 1) (net_name "GND") (layers "F.Cu" "B.Cu") (name "gnd") (priority 2)
    (connect_pads yes (clearance 0.4)) (min_thickness 0.2)
    (fill yes (thermal_gap 0.5) (thermal_bridge_width 0.6) (island_removal_mode 1))
    (polygon (pts (xy 0 0) (xy 50 0) (xy 50 30) (xy 0 30)))
    (filled_polygon (layer "F.Cu") (pts (xy 1 1) (xy 49 1) (xy 49 29)))
    (filled_polygon (layer "B.Cu") (pts (xy 2 2) (xy 48 2) (xy 48 28))))
  (zone (net 0) (net_name "") (layer "F.Cu") (name "ko")
    (keepout (tracks not_allowed) (vias not_allowed) (pads allowed) (copperpour not_allowed) (footprints allowed))
    (polygon (pts (xy 60 0) (xy 70 0) (xy 70 10))))
)"#;

#[test]
fn structured_board_inline_mapping() {
    let imp = parse_board(BOARD, "demo").unwrap();
    assert_eq!(imp.file_name, "demo.arpcb");
    let doc = &imp.document;
    assert_eq!(doc.kind(), DocKind::Pcb);
    let boards: Vec<_> = doc.items().filter(|(_, i)| matches!(i, Item::Board(_))).collect();
    assert_eq!(boards.len(), 1);
    let (_, s) = board_settings(doc).unwrap();
    assert_eq!(s.stackup.copper_layers(), vec![layer::TOP_COPPER, layer::inner_copper(1), layer::inner_copper(2), layer::BOTTOM_COPPER]);
    let d2 = s.stackup.layers.iter().find(|l| l.name == "Dielectric 2").unwrap();
    assert_eq!((d2.thickness, d2.epsilon_r, d2.material.as_str()), (mm(1.0), 4.6, "FR4"));
    assert_eq!(s.mask_color, "#a01818");
    assert_eq!(s.origin, Point::new(mm(10.0), mm(-20.0)));
    assert_eq!(s.net_classes.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["Default", "Power"]);
    assert_eq!(s.net_classes[1].nets, vec!["GND".to_string()]);
    assert_eq!(s.net_classes[1].track_width, mm(0.5));

    // Footprints: identical geometry shares a cache entry even with moved
    // reference text; different geometry gets a variant key.
    let fps: Vec<_> = doc.items().filter_map(|(_, i)| if let Item::Footprint(f) = i { Some(f) } else { None }).collect();
    assert_eq!(fps.iter().map(|f| f.footprint.as_str()).collect::<Vec<_>>(), ["Lib:R", "Lib:R", "Lib:R", "Lib:R#2"]);
    assert_eq!(doc.footprint_cache().len(), 2);
    let r1 = fps[0];
    assert_eq!(
        (r1.designator.as_str(), r1.value.as_str(), r1.pos, r1.rotation, r1.side),
        ("R1", "10k", Point::new(mm(100.0), mm(-50.0)), 90.0, Side::Top)
    );
    assert_eq!(r1.parameters.get("MPN").map(String::as_str), Some("RC0603"));
    assert_eq!(r1.pad_nets.get("1").map(String::as_str), Some("GND"));
    let r3 = fps[2];
    assert_eq!((r3.side, r3.locked, r3.rotation), (Side::Bottom, true, 210.0));
    // The cached footprint is top-side library geometry.
    let cached = doc.cached_footprint("Lib:R").unwrap();
    assert!(cached.items.iter().all(|i| match i {
        Item::Pad(p) => p.layer == layer::TOP_COPPER,
        Item::Line(l) => l.layer == layer::TOP_SILK,
        Item::Text(t) => !t.mirrored && t.layer != layer::BOTTOM_SILK,
        _ => true,
    }));
    let pad2 = cached.items.iter().find_map(|i| if let Item::Pad(p) = i { (p.number == "2").then_some(p) } else { None }).unwrap();
    assert_eq!((pad2.pos, pad2.rotation), (Point::new(mm(1.0), 0), 30.0));

    // World pads reproduce structured format exactly, including the bottom-side part.
    let (checked, failures) = check_pads_against_structured(BOARD, &imp);
    assert_eq!(checked, 8);
    assert!(failures.is_empty(), "{failures:#?}");
    let pads = all_pads(doc, s);
    let r3p1 = pads.iter().find(|p| p.designator == "R3" && p.number == "1").unwrap();
    assert_eq!(r3p1.copper, vec![layer::BOTTOM_COPPER]);
    assert!((normalize_deg(r3p1.rotation - 30.0) % 180.0).abs() < 1e-6, "{}", r3p1.rotation);

    // Bottom-side graphics land where structured format draws them, on bottom layers, mirrored text.
    let g = world_graphics(r3, doc.cached_footprint(&r3.footprint).unwrap());
    let line = g.iter().find_map(|i| if let Item::Line(l) = i { Some(l) } else { None }).unwrap();
    assert_eq!(line.layer, layer::BOTTOM_SILK);
    let ends = [structured_world((120.0, 50.0, 30.0), (-1.0, 0.5)), structured_world((120.0, 50.0, 30.0), (1.0, 0.5))];
    assert!(ends.iter().any(|e| near(*e, line.a, 2)) && ends.iter().any(|e| near(*e, line.b, 2)), "{line:?} vs {ends:?}");
    let des = g.iter().find_map(|i| if let Item::Text(t) = i { (t.text == "R3").then_some(t) } else { None }).unwrap();
    assert!(des.mirrored && des.layer == layer::BOTTOM_SILK);
    assert!(near(des.pos, structured_world((120.0, 50.0, 30.0), (0.0, 1.5)), 2));
    let top_des = world_graphics(r1, cached)
        .into_iter()
        .find_map(|i| if let Item::Text(t) = i { (t.text == "R1").then_some(t) } else { None })
        .unwrap();
    assert!((top_des.rotation - 90.0).abs() < 1e-6 && near(top_des.pos, structured_world((100.0, 50.0, 90.0), (0.0, -1.5)), 2));
    assert!(cached.items.iter().any(|i| matches!(i, Item::Text(t) if t.text == DESIGNATOR_TOKEN)));

    // Copper.
    let tracks: Vec<_> = doc.items().filter_map(|(_, i)| if let Item::Track(t) = i { Some(t) } else { None }).collect();
    assert_eq!((tracks[0].a, tracks[0].net.as_str(), tracks[0].width), (Point::new(mm(100.0), mm(-49.0)), "GND", mm(0.25)));
    let arc = doc.items().find_map(|(_, i)| if let Item::ArcTrack(a) = i { Some(a) } else { None }).unwrap();
    assert_eq!(
        (arc.layer, arc.net.as_str(), arc.center, arc.radius),
        (layer::BOTTOM_COPPER, "SIG", Point::new(mm(105.0), mm(-60.0)), mm(5.0))
    );
    let vias: Vec<_> = doc.items().filter_map(|(_, i)| if let Item::Via(v) = i { Some(v) } else { None }).collect();
    assert_eq!((vias[0].from_layer, vias[0].to_layer, vias[0].net.as_str()), (layer::TOP_COPPER, layer::BOTTOM_COPPER, "GND"));
    assert_eq!(vias[1].to_layer, layer::inner_copper(1));
    let zones: Vec<_> = doc.items().filter_map(|(_, i)| if let Item::Zone(z) = i { Some(z) } else { None }).collect();
    assert_eq!(zones.len(), 3);
    let (zf, zb) = (zones[0], zones[1]);
    assert_eq!(
        (zf.layer, zb.layer, zf.net.as_str(), zf.priority, zf.connect),
        (layer::TOP_COPPER, layer::BOTTOM_COPPER, "GND", 2, ZoneConnect::Solid)
    );
    assert_eq!(
        (zf.clearance, zf.min_width, zf.thermal_gap, zf.spoke_width, zf.remove_islands),
        (mm(0.4), mm(0.2), mm(0.5), mm(0.6), false)
    );
    assert_eq!(zf.fill[0].outer[0], Point::new(mm(1.0), mm(-1.0)));
    assert_eq!(zb.fill[0].outer[0], Point::new(mm(2.0), mm(-2.0)));
    assert!(zones[2].keepout && zones[2].net.is_empty());

    // Board graphics.
    assert!(doc.items().any(|(_, i)| matches!(i, Item::Line(l) if l.layer == layer::BOARD_OUTLINE)));
    assert!(doc.items().any(|(_, i)| matches!(i, Item::Arc(a) if a.layer == layer::BOARD_OUTLINE && (a.sweep - 180.0).abs() < 1e-3)));
    assert!(doc.items().any(|(_, i)| matches!(i, Item::Text(t) if t.text == "HELLO" && t.layer == layer::TOP_SILK)));
    assert!(imp.report.messages.iter().any(|m| m.text.contains("dimensions")));

    // Libraries and round trip.
    assert_eq!(imp.libraries.len(), 1);
    assert_eq!(imp.libraries[0].name, "Lib");
    assert_eq!(imp.libraries[0].footprints.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), ["R", "R#2"]);
    for (_, i) in doc.items() {
        i.validate().unwrap();
    }
    assert_round_trip(doc);
}

#[test]
fn structured_board_rejects_other_files_and_never_panics() {
    assert!(matches!(parse_board("(kicad_sch)", "x"), Err(crate::ImportError::Invalid(_))));
    for cut in (0..BOARD.len()).step_by(7) {
        if BOARD.is_char_boundary(cut) {
            let _ = parse_board(&BOARD[..cut], "x");
        }
    }
}

// ---------- demo boards ----------

fn demo_boards(share: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(share.join("demos")) else { return out };
    let mut dirs: Vec<PathBuf> = rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.is_dir()).collect();
    dirs.sort();
    for d in dirs {
        let mut files: Vec<PathBuf> = std::fs::read_dir(&d)
            .map(|rd| rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "kicad_pcb")).collect())
            .unwrap_or_default();
        files.sort();
        out.extend(files);
    }
    out
}

#[test]
fn structured_demo_boards_import_connected() {
    let Some(share) = library_share() else { return };
    let boards = demo_boards(&share);
    assert!(boards.len() >= 10);
    let mut failures = Vec::new();
    let (mut total, mut pads_checked, mut pad_failures) = (Copper::default(), 0, 0);
    let t = Instant::now();
    for path in &boards {
        let text = std::fs::read_to_string(path).unwrap();
        let imp = import_board(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let doc = &imp.document;
        for (_, i) in doc.items() {
            i.validate().unwrap_or_else(|e| panic!("{}: invalid {}: {e}", path.display(), i.kind_name()));
        }
        assert_round_trip(doc);
        let (checked, pf) = check_pads_against_structured(&text, &imp);
        pads_checked += checked;
        pad_failures += pf.len();
        if !pf.is_empty() {
            failures.push(format!("{}: {} pad mismatches, e.g. {:?}", path.display(), pf.len(), &pf[..pf.len().min(3)]));
        }
        let c = copper_connectivity(doc);
        let landed = c.endpoints - c.dangling;
        let ratio = if c.endpoints == 0 { 1.0 } else { (c.on_pad + c.on_via + c.on_track) as f64 / c.endpoints as f64 };
        let fps = doc.items().filter(|(_, i)| matches!(i, Item::Footprint(_))).count();
        println!(
            "{:<42} fp {:>5} cache {:>4}  track ends {:>6}: pad {:>6} via {:>5} track {:>6} zone {:>4} dangling {:>3} ({:>6.2}% on pad/via/track)  pad nets {}/{} ok  pads vs structured format {}  report {}",
            path.file_name().unwrap().to_string_lossy(),
            fps,
            doc.footprint_cache().len(),
            c.endpoints,
            c.on_pad,
            c.on_via,
            c.on_track,
            c.in_zone,
            c.dangling,
            100.0 * ratio,
            c.net_checked - c.net_mismatch,
            c.net_checked,
            checked,
            imp.report.messages.len()
        );
        let _ = landed;
        // The "-unrouted" demo is partially routed on purpose (open track ends in the source).
        let unrouted = path.file_name().is_some_and(|n| n.to_string_lossy().contains("unrouted"));
        if ratio < 0.99 && !unrouted {
            failures.push(format!(
                "{}: only {:.2}% of track ends on pad/via/track; dangling e.g. {:?}",
                path.display(),
                100.0 * ratio,
                c.dangling_examples
            ));
        }
        if c.net_mismatch > 0 {
            failures.push(format!(
                "{}: {} pad/track net mismatches, e.g. {:?}",
                path.display(),
                c.net_mismatch,
                &c.mismatches[..c.mismatches.len().min(3)]
            ));
        }
        total.endpoints += c.endpoints;
        total.on_pad += c.on_pad;
        total.on_via += c.on_via;
        total.on_track += c.on_track;
        total.in_zone += c.in_zone;
        total.dangling += c.dangling;
        total.net_checked += c.net_checked;
        total.net_mismatch += c.net_mismatch;
        total.source_nc_violations += c.source_nc_violations;
    }
    println!(
        "TOTAL: {} boards, {} track ends ({} on pads, {} on vias, {} on tracks, {} in zones, {} dangling), pad nets {}/{} ok ({} source tracks on no-connect pads), {} pads checked against structured format ({} mismatches), {:?}",
        boards.len(),
        total.endpoints,
        total.on_pad,
        total.on_via,
        total.on_track,
        total.in_zone,
        total.dangling,
        total.net_checked - total.net_mismatch,
        total.net_checked,
        total.source_nc_violations,
        pads_checked,
        pad_failures,
        t.elapsed()
    );
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn structured_projects_import_their_boards() {
    let Some(share) = library_share() else { return };
    let imp = import_project(&share.join("demos/pic_programmer/pic_programmer.kicad_pro")).unwrap();
    assert_eq!(imp.project.pcb.as_deref(), Some("pic_programmer.arpcb"));
    let board = imp.board.expect("board");
    let (_, s) = board_settings(&board.document).unwrap();
    // pic_programmer.kicad_pro: min_track_width 0.25, min_through_hole_diameter 0.5.
    let width_min =
        s.rules.iter().find_map(|r| if let pcbplannercad_core::pcb::RuleKind::Width { min, .. } = r.kind { Some(min) } else { None });
    assert_eq!(width_min, Some(mm(0.25)));
    let hole_min =
        s.rules.iter().find_map(|r| if let pcbplannercad_core::pcb::RuleKind::HoleSize { min, .. } = r.kind { Some(min) } else { None });
    assert_eq!(hole_min, Some(mm(0.5)));
    assert_eq!(s.origin, Point::new(mm(62.23), mm(-153.67)));

    // Net class patterns (video: "pwr" class by pattern).
    let video = import_board(&share.join("demos/video/video.kicad_pcb")).unwrap();
    let (_, vs) = board_settings(&video.document).unwrap();
    let pwr = vs.net_classes.iter().find(|c| c.name == "pwr").expect("pwr class");
    // Patterns "+12V", "+3.3V", "+5F": only the last two exist on this board.
    assert_eq!(pwr.nets, vec!["+3.3V".to_string(), "+5F".to_string()]);
    assert_eq!(vs.net_classes[0].name, "Default");

    let user = std::env::var_os("PCAD_TEST_STRUCTURED_PROJECT").map(PathBuf::from).unwrap_or_default();
    if user.is_file() {
        let u = import_project(&user).unwrap();
        assert!(u.project.pcb.as_deref().is_some_and(|p| p.ends_with(".arpcb")));
        let b = u.board.unwrap();
        let (_, us) = board_settings(&b.document).unwrap();
        assert_eq!(us.stackup.copper_layers().len(), 2);
        assert_round_trip(&b.document);
    }
}

#[test]
fn structured_dimensions_and_teardrop_zones_import() {
    use pcbplannercad_core::pcb::dimension::DimUnits;
    // From the bundled "video" demo: an aligned dimension measuring down (structured format Y), its line 3.175 mm to the right.
    let extra = r#"
	(dimension (type aligned) (layer "Dwgs.User") (pts (xy 224.028 134.366) (xy 224.028 154.94)) (height -3.175)
		(format (prefix "") (suffix "") (units 2) (units_format 1) (precision 4))
		(style (thickness 0.2) (arrow_length 1.27))
		(gr_text "20.5740 mm" (at 225.479 144.653 90) (layer "Dwgs.User") (effects (font (size 1.524 1.524) (thickness 0.2)))))
	(dimension (type orthogonal) (layer "Cmts.User") (pts (xy 0 0) (xy 10 4)) (height 2) (orientation 1)
		(format (units 1) (precision 1)) (style (thickness 0.1)))
	(zone (net 0) (net_name "") (layer "F.Cu") (hatch edge 0.5) (attr (teardrop (type padvia)))
		(connect_pads yes (clearance 0)) (min_thickness 0.0254) (fill yes (thermal_gap 0.5) (thermal_bridge_width 0.5))
		(polygon (pts (xy 0 0) (xy 1 0) (xy 1 1))))
"#;
    let text = format!("{}{extra})", BOARD.trim_end().strip_suffix(')').unwrap());
    let imp = parse_board(&text, "demo").unwrap();
    let dims: Vec<_> = imp.document.items().filter_map(|(_, i)| if let Item::Dimension(d) = i { Some(d.clone()) } else { None }).collect();
    assert_eq!(dims.len(), 2);
    let d = &dims[0];
    assert_eq!((d.layer, d.units, d.precision, d.text_height, d.width), (layer::MECHANICAL_1, DimUnits::Mm, 4, mm(1.524), mm(0.2)));
    assert_eq!(d.label(), "20.5740 mm");
    // The dimension line sits on the side of structured format's text (x ≈ 225.5): right of the measured points.
    let pcbplannercad_core::item::Item::Line(line) = &d.primitives()[2] else { panic!() };
    assert_eq!(line.a.x, mm(224.028 + 3.175));
    assert!(dims[1].orthogonal && dims[1].angle == 90.0 && dims[1].units == DimUnits::Mil);
    assert_eq!(dims[1].label(), "157.5 mil");
    let teardrops = imp.document.items().filter(|(_, i)| matches!(i, Item::Zone(z) if z.teardrop)).count();
    assert_eq!(teardrops, 1);
}


#[test]
fn independent_and_graphic_only_footprints_are_kept_but_electrical_parts_stay_linked() {
    use pcbplannercad_core::{pcb::sync,schematic::Compiled};
    let text=r#"(kicad_pcb (version 20241229) (layers (0 "F.Cu" signal) (31 "B.Cu" signal))
        (footprint "L:Graphic" (layer "F.Cu") (at 0 0) (attr board_only) (property "Reference" "G1"))
        (footprint "L:Graphic" (layer "F.Cu") (at 5 0) (attr exclude_from_bom exclude_from_pos_files) (property "Reference" "G2"))
        (footprint "L:Test" (layer "F.Cu") (at 10 0) (attr exclude_from_bom exclude_from_pos_files) (property "Reference" "TP1")
            (pad "1" smd rect (at 0 0) (size 1 1) (layers "F.Cu")))
        (footprint "L:Part" (layer "F.Cu") (at 15 0) (property "Reference" "R1")))"#;
    let imported=parse_board(text,"original").unwrap();
    let fps:Vec<_>=imported.document.items().filter_map(|(_,i)|if let Item::Footprint(f)=i{Some((f.designator.as_str(),f.board_only))}else{None}).collect();
    assert_eq!(fps,vec![("G1",true),("G2",true),("TP1",false),("R1",false)]);
    let reopened=document_from_str(DocKind::Pcb,&document_to_string(&imported.document).unwrap()).unwrap();
    let plan=sync::plan(&Compiled::default(),&reopened,&|_|None);
    let removed:Vec<_>=plan.changes.iter().filter_map(|c|if let sync::EcoChange::RemoveFootprint{designator,..}=c{Some(designator.as_str())}else{None}).collect();
    assert_eq!(removed,vec!["R1","TP1"]);
}
