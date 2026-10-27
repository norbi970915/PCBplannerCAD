use super::*;
use crate::DocKind;
use crate::item::{Line, Text};
use crate::layer::{BOTTOM_COPPER, TOP_COPPER};
use crate::pcb::board::{Rule, RuleKind};
use crate::pcb::connectivity::fixtures::*;
use crate::pcb::items::{Via, Zone, ZonePolygon};
use crate::units::mm;

fn codes(v: &[DrcViolation]) -> Vec<&'static str> {
    v.iter().map(|v| v.code).collect()
}

fn count(r: &DrcReport, code: &str) -> usize {
    r.violations.iter().filter(|v| v.code == code).count()
}

/// R1 (A/B) at x=5 and R2 (A/B) at x=15, both nets routed around each other.
fn clean_items() -> Vec<Item> {
    vec![
        outline(0.0, -5.0, 20.0, 5.0),
        fp("R1", "L/R", 5.0, 0.0, &[("1", "A"), ("2", "B")]),
        fp("R2", "L/R", 15.0, 0.0, &[("1", "A"), ("2", "B")]),
        track((4.2, 0.0), (4.2, 2.0), 0.25, TOP_COPPER, "A"),
        track((4.2, 2.0), (14.2, 2.0), 0.25, TOP_COPPER, "A"),
        track((14.2, 2.0), (14.2, 0.0), 0.25, TOP_COPPER, "A"),
        track((5.8, 0.0), (5.8, -2.0), 0.25, TOP_COPPER, "B"),
        track((5.8, -2.0), (15.8, -2.0), 0.25, TOP_COPPER, "B"),
        track((15.8, -2.0), (15.8, 0.0), 0.25, TOP_COPPER, "B"),
    ]
}

fn with(extra: Vec<Item>) -> Document {
    let mut items = clean_items();
    items.extend(extra);
    doc(items)
}

fn with_rule(d: &mut Document, rule: Rule) {
    let (id, b) = board_settings(d).map(|(id, b)| (id, b.clone())).unwrap();
    let mut b = b;
    b.rules.insert(0, rule);
    let mut items: Vec<Item> = d.items().map(|(i, it)| if i == id { Item::Board(b.clone()) } else { it.clone() }).collect();
    let fps = d.footprint_cache().clone();
    *d = Document::from_item_list(DocKind::Pcb, items.drain(..)).with_footprints(fps);
}

#[test]
fn clean_board_has_no_violations() {
    let r = run(&with(vec![]));
    assert!(r.violations.is_empty(), "{:#?}", r.violations);
    assert_eq!(r.stats, RoutingStats { nets: 2, connections: 2, unrouted: 0 });
    assert!(r.rule_errors.is_empty());
}

#[test]
fn clearance_violation_and_pass() {
    // R1 pad 1 (net A) spans x 3.8..4.6 and y ±0.45; a net-C track at y = 0.45 + 0.1 + 0.05 is 0.1 mm away.
    let r = run(&with(vec![track((0.5, -0.625), (3.0, -0.625), 0.15, TOP_COPPER, "C"), track((3.0, -0.625), (4.6, -0.625), 0.15, TOP_COPPER, "C")]));
    assert_eq!(codes(&r.violations), vec!["clearance"], "{:#?}", r.violations);
    let v = r.violations.iter().find(|v| v.code == "clearance").unwrap();
    assert_eq!(v.rule, "Clearance");
    assert_eq!(v.layer, Some(TOP_COPPER));
    assert!(v.message.contains("0.100 mm < 0.200 mm"), "{}", v.message);
    assert_eq!(v.items.len(), 2);
    // Moved to 0.3 mm away: fine. On the bottom layer: fine.
    let r = run(&with(vec![track((0.5, -0.8), (4.6, -0.8), 0.15, TOP_COPPER, "C")]));
    assert_eq!(count(&r, "clearance"), 0, "{:#?}", r.violations);
    let r = run(&with(vec![track((0.5, -0.625), (4.6, -0.625), 0.15, BOTTOM_COPPER, "C")]));
    assert_eq!(count(&r, "clearance"), 0);
}

#[test]
fn scoped_clearance_rules_apply() {
    let mut d = with(vec![track((0.5, -0.8), (4.6, -0.8), 0.15, TOP_COPPER, "HV")]);
    assert_eq!(count(&run(&d), "clearance"), 0);
    with_rule(&mut d, Rule { name: "HighVoltage".into(), enabled: true, kind: RuleKind::Clearance { first: "InNet('HV')".into(), second: "All".into(), min: mm(1.0) } });
    let r = run(&d);
    let v: Vec<_> = r.violations.iter().filter(|v| v.code == "clearance").collect();
    assert!(!v.is_empty() && v.iter().all(|v| v.rule == "HighVoltage"), "{v:#?}");
}

#[test]
fn shorts_direct_and_through_unassigned_copper() {
    let r = run(&with(vec![track((3.9, -1.5), (3.9, -0.4), 0.2, TOP_COPPER, "B")]));
    assert_eq!(count(&r, "short"), 1, "{:#?}", r.violations);
    assert_eq!(count(&r, "clearance"), 0);
    // A no-net track joining pads of A and B.
    let r = run(&with(vec![track((4.2, 0.0), (5.8, 0.0), 0.2, TOP_COPPER, "")]));
    let shorts: Vec<_> = r.violations.iter().filter(|v| v.code == "short").collect();
    assert_eq!(shorts.len(), 1, "{:#?}", r.violations);
    assert!(shorts[0].message.contains("unassigned"));
    // No-net copper attached to one net only is fine.
    let r = run(&with(vec![track((4.2, 0.0), (3.0, 0.0), 0.2, TOP_COPPER, "")]));
    assert_eq!(count(&r, "short") + count(&r, "clearance"), 0, "{:#?}", r.violations);
}

#[test]
fn track_width_limits() {
    let r = run(&with(vec![track((1.0, 4.0), (3.0, 4.0), 0.1, TOP_COPPER, "")]));
    assert_eq!(codes(&r.violations), vec!["track-width"]);
    assert!(r.violations[0].message.contains("below the minimum 0.150 mm"));
    let r = run(&with(vec![track((1.0, 4.0), (3.0, 4.0), 0.2, TOP_COPPER, "")]));
    assert!(r.violations.is_empty());
    let mut d = with(vec![track((1.0, 4.0), (3.0, 4.0), 0.2, TOP_COPPER, "")]);
    with_rule(&mut d, Rule { name: "Narrow".into(), enabled: true, kind: RuleKind::Width { scope: "OnLayer('Top Layer')".into(), min: mm(0.05), preferred: mm(0.1), max: mm(0.15) } });
    let r = run(&d);
    assert!(r.violations.iter().any(|v| v.code == "track-width" && v.rule == "Narrow" && v.message.contains("above")));
}

#[test]
fn via_style_and_hole_size() {
    let small = Item::Via(Via { locked: false, pos: p(10.0, 4.0), diameter: mm(0.5), drill: mm(0.3), from_layer: TOP_COPPER, to_layer: BOTTOM_COPPER, net: String::new() });
    let r = run(&with(vec![small]));
    assert_eq!(codes(&r.violations), vec!["via-style"]);
    assert!(r.violations[0].message.contains("diameter") && r.violations[0].message.contains("annular ring"));
    let r = run(&with(vec![via(10.0, 4.0, "")]));
    assert!(r.violations.is_empty(), "{:#?}", r.violations);
    let tiny = Item::Via(Via { locked: false, pos: p(10.0, 4.0), diameter: mm(0.6), drill: mm(0.15), from_layer: TOP_COPPER, to_layer: BOTTOM_COPPER, net: String::new() });
    let r = run(&with(vec![tiny]));
    assert!(codes(&r.violations).contains(&"hole-size"), "{:#?}", r.violations);
    // Pad holes: 0.8 mm THT drill against a 0.5 mm maximum.
    let mut d = with(vec![fp("J1", "L/TH", 10.0, 3.5, &[])]);
    assert_eq!(count(&run(&d), "hole-size"), 0);
    with_rule(&mut d, Rule { name: "SmallHoles".into(), enabled: true, kind: RuleKind::HoleSize { scope: "IsPad".into(), min: mm(0.1), max: mm(0.5) } });
    let r = run(&d);
    let v: Vec<_> = r.violations.iter().filter(|v| v.code == "hole-size").collect();
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].rule, "SmallHoles");
}

#[test]
fn board_outline_clearance_outside_and_missing() {
    // Pad edge 0.1 mm from the top edge (y = 5).
    let r = run(&with(vec![fp("R3", "L/R", 10.0, 4.45, &[])]));
    let v: Vec<_> = r.violations.iter().filter(|v| v.code == "board-outline").collect();
    assert_eq!(v.len(), 2, "{v:#?}"); // both pads
    assert!(v[0].message.contains("0.100 mm from the board outline"));
    let r = run(&with(vec![track((30.0, 0.0), (32.0, 0.0), 0.25, TOP_COPPER, "")]));
    assert!(r.violations.iter().any(|v| v.code == "board-outline" && v.message.contains("outside")), "{:#?}", r.violations);
    let r = run(&with(vec![track((19.0, 0.0), (21.0, 0.0), 0.25, TOP_COPPER, "")]));
    assert!(r.violations.iter().any(|v| v.code == "board-outline" && v.message.contains("crosses")));
    let r = run(&with(vec![fp("R3", "L/R", 10.0, 3.0, &[])]));
    assert_eq!(count(&r, "board-outline"), 0);
    let mut items = clean_items();
    items.remove(0);
    let r = run(&doc(items));
    assert_eq!(codes(&r.violations), vec!["no-outline"]);
    assert_eq!(r.violations[0].severity, Severity::Warning);
}

#[test]
fn courtyard_overlap_and_component_clearance() {
    let base = |x: f64| vec![outline(0.0, -10.0, 40.0, 10.0), fp("U1", "L/R", 10.0, 6.0, &[]), fp("U2", "L/R", 10.0 + x, 6.0, &[])];
    let r = run(&doc(base(2.8)));
    assert_eq!(codes(&r.violations), vec!["courtyard-overlap"]);
    assert!(r.violations[0].message.contains("overlap"));
    // Abutting courtyards are fine with a zero clearance.
    assert!(run(&doc(base(3.0))).violations.is_empty());
    // Other side: fine.
    let mut items = base(2.8);
    if let Item::Footprint(f) = &mut items[2] {
        f.side = Side::Bottom;
    }
    assert!(run(&doc(items)).violations.is_empty());
    let mut d = doc(base(3.1));
    with_rule(&mut d, Rule { name: "Spacing".into(), enabled: true, kind: RuleKind::ComponentClearance { min: mm(0.25) } });
    let r = run(&d);
    assert_eq!(codes(&r.violations), vec!["courtyard-overlap"]);
    assert_eq!(r.violations[0].rule, "Spacing");
    // Footprints without courtyard fall back to the pad box.
    let r = run(&doc(vec![outline(0.0, -10.0, 40.0, 10.0), fp("J1", "L/TH", 10.0, 0.0, &[]), fp("J2", "L/TH", 11.0, 0.0, &[])]));
    assert!(codes(&r.violations).contains(&"courtyard-overlap"), "{:#?}", r.violations);
}

#[test]
fn silk_over_pads() {
    let silk_line = |y: f64| Item::Line(Line { a: p(3.0, y), b: p(7.0, y), width: mm(0.15), layer: layer::TOP_SILK });
    let r = run(&with(vec![silk_line(0.0)]));
    let v: Vec<_> = r.violations.iter().filter(|v| v.code == "silk-over-pad").collect();
    assert_eq!(v.len(), 2, "{v:#?}");
    assert!(v.iter().all(|v| v.severity == Severity::Warning));
    assert!(run(&with(vec![silk_line(1.0)])).violations.is_empty());
    // Bottom silk does not care about top SMD pads.
    let r = run(&with(vec![Item::Line(Line { a: p(3.0, 0.0), b: p(7.0, 0.0), width: mm(0.15), layer: layer::BOTTOM_SILK })]));
    assert!(r.violations.is_empty());
    let text = Item::Text(Text {
        pos: p(4.2, 0.0),
        text: "R1".into(),
        height: mm(1.0),
        rotation: 0.0,
        h_align: Default::default(),
        v_align: Default::default(),
        layer: layer::TOP_SILK,
        mirrored: false,
    });
    assert!(count(&run(&with(vec![text])), "silk-over-pad") >= 1);
}

#[test]
fn unrouted_missing_footprint_and_rule_errors() {
    let mut items = clean_items();
    items.truncate(3); // drop all tracks
    items.push(fp("X1", "L/Nope", 10.0, 0.0, &[]));
    let mut d = doc(items);
    with_rule(&mut d, Rule { name: "Bad".into(), enabled: true, kind: RuleKind::Width { scope: "InNet(".into(), min: 1, preferred: 2, max: 3 } });
    let r = run(&d);
    assert_eq!(count(&r, "unrouted"), 2);
    assert!(r.violations.iter().any(|v| v.code == "unrouted" && v.message == "Net A has 1 unrouted connection"));
    assert_eq!(count(&r, "missing-footprint"), 1);
    let e: Vec<_> = r.violations.iter().filter(|v| v.code == "rule-error").collect();
    assert_eq!(e.len(), 1);
    assert_eq!(e[0].rule, "Bad");
    assert_eq!(r.rule_errors.len(), 1);
    assert_eq!(r.stats, RoutingStats { nets: 2, connections: 2, unrouted: 2 });
}

#[test]
fn keepout_areas() {
    let keepout = |l: LayerId| {
        let outer = vec![p(8.0, 3.0), p(12.0, 3.0), p(12.0, 4.5), p(8.0, 4.5)];
        Item::Zone(Zone { locked: false,
            teardrop: false,
            outline: outer,
            layer: l,
            net: String::new(),
            name: "KO".into(),
            priority: 0,
            clearance: 0,
            min_width: 0,
            connect: Default::default(),
            thermal_gap: 0,
            spoke_width: 0,
            keepout: true,
            remove_islands: false,
            fill: vec![],
        })
    };
    let t = track((9.0, 4.0), (11.0, 4.0), 0.25, TOP_COPPER, "");
    let r = run(&with(vec![keepout(TOP_COPPER), t.clone()]));
    assert_eq!(codes(&r.violations), vec!["keepout"]);
    assert!(run(&with(vec![keepout(BOTTOM_COPPER), t])).violations.is_empty());
}

#[test]
fn zone_fill_clearance() {
    // A GND fill 0.1 mm from R1 pad 1 (A) on top.
    let mut z = zone(1.0, -4.0, 3.7, 4.0, TOP_COPPER, "GND");
    if let Item::Zone(zz) = &mut z {
        zz.fill = vec![ZonePolygon { outer: vec![p(1.0, -1.0), p(3.7, -1.0), p(3.7, 1.0), p(1.0, 1.0)], holes: vec![] }];
    }
    let r = run(&with(vec![z]));
    assert_eq!(codes(&r.violations), vec!["clearance"], "{:#?}", r.violations);
    assert!(r.violations[0].message.contains("Zone (GND)"));
}

#[test]
fn online_drc_reports_only_near_items() {
    let d = with(vec![
        track((0.5, -0.625), (4.6, -0.625), 0.15, TOP_COPPER, "C"), // clearance to R1.1
        track((10.5, -0.625), (14.6, -0.625), 0.15, TOP_COPPER, "D"), // clearance to R2.1
    ]);
    let full = run(&d);
    assert_eq!(count(&full, "clearance"), 2);
    let near = run_near(&d, &[id(9)]);
    assert_eq!(codes(&near), vec!["clearance"], "{near:#?}");
    assert!(near[0].items.contains(&id(9)));
    // The footprint side of the same violation.
    let near = run_near(&d, &[id(2)]);
    assert_eq!(codes(&near), vec!["clearance"]);
    assert!(near[0].items.contains(&id(10)));
    assert!(run_near(&d, &[id(3)]).is_empty());
    assert!(run_near(&d, &[ItemId(9999)]).is_empty());
}

/// ~10k copper objects: 1600 footprints (3200 pads), 4800 tracks, 1600 vias
/// and a 2000-vertex zone fill.
fn big_board() -> Document {
    let mut items = vec![outline(-5.0, -5.0, 205.0, 205.0)];
    let n = 40;
    for r in 0..n {
        for c in 0..n {
            let (x, y) = (c as f64 * 5.0, r as f64 * 5.0);
            let k = r * n + c;
            let (n1, n2) = (format!("N{k}"), format!("N{}", k + 1));
            items.push(fp(&format!("R{k}"), "L/R", x, y, &[("1", &n1), ("2", &n2)]));
            if c + 1 < n {
                items.push(track((x + 0.8, y), (x + 4.2, y), 0.25, TOP_COPPER, &n2));
            }
            items.push(track((x - 0.8, y), (x - 0.8, y - 1.5), 0.25, TOP_COPPER, &n1));
            items.push(via(x - 0.8, y - 1.5, &n1));
            items.push(track((x - 0.8, y - 1.5), (x + 1.5, y - 1.5), 0.25, BOTTOM_COPPER, &n1));
        }
    }
    let ring: Vec<Point> = (0..2000)
        .map(|i| {
            let a = std::f64::consts::TAU * i as f64 / 2000.0;
            p(100.0 + 172.0 * a.cos().clamp(-0.6, 0.6), 100.0 + 172.0 * a.sin().clamp(-0.6, 0.6))
        })
        .collect();
    let hole = vec![p(-1.6, -2.2), p(-1.6, 198.0), p(198.0, 198.0), p(198.0, -2.2)];
    let mut z = zone(-4.0, -4.0, 204.0, 204.0, BOTTOM_COPPER, "GND");
    if let Item::Zone(zz) = &mut z {
        zz.fill = vec![ZonePolygon { outer: ring, holes: vec![hole] }];
    }
    items.push(z);
    doc(items)
}

#[test]
#[ignore = "performance: cargo test --release -p pcbplannercad-core drc_perf -- --ignored --nocapture"]
fn drc_perf_10k_objects() {
    let d = big_board();
    let objs = conn::copper_objects(&d, &BoardSettings::default()).len();
    let t = std::time::Instant::now();
    let r = run(&d);
    let full = t.elapsed();
    let t = std::time::Instant::now();
    let near = run_near(&d, &[id(500), id(501), id(502), id(503)]);
    let online = t.elapsed();
    let t = std::time::Instant::now();
    let rats = conn::ratsnest(&d).len();
    let rn = t.elapsed();
    println!("objects {objs}: full DRC {full:?} ({} violations, {:?}), run_near {online:?} ({}), ratsnest {rn:?} ({rats})", r.violations.len(), r.stats, near.len());
    assert!(objs > 9000);
    assert_eq!(r.stats.unrouted, n_rows() - 1);
    assert!(r.violations.iter().all(|v| v.code == "unrouted"), "{:#?}", &r.violations[..r.violations.len().min(5)]);
}

fn n_rows() -> usize {
    40
}
