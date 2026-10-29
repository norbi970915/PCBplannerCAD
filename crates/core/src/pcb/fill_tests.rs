use super::*;
use crate::DocKind;
use crate::item::{Drill, Pad, Rect};
use crate::layer;
use crate::library::Footprint;
use crate::pcb::board::Rule;
use crate::pcb::items::{ArcTrack, PlacedFootprint, Side, Track, Via};
use crate::units::mm;

fn p(x: f64, y: f64) -> Point {
    Point::new(mm(x), mm(y))
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<Point> {
    vec![p(x0, y0), p(x1, y0), p(x1, y1), p(x0, y1)]
}

/// 50 × 30 mm board with default rules (clearance 0.2, outline clearance 0.3,
/// thermal gap/spoke 0.3).
fn board_items() -> Vec<Item> {
    vec![
        Item::Board(BoardSettings::default()),
        Item::Rect(Rect { a: p(0.0, 0.0), b: p(50.0, 30.0), width: mm(0.1), layer: layer::BOARD_OUTLINE, fill: Default::default() }),
    ]
}

fn zone(outline: Vec<Point>, net: &str) -> Zone {
    Zone { locked: false,
            teardrop: false,
        outline,
        layer: layer::TOP_COPPER,
        net: net.into(),
        name: String::new(),
        priority: 0,
        clearance: 0,
        min_width: 0,
        connect: ZoneConnect::Thermal,
        thermal_gap: mm(0.3),
        spoke_width: mm(0.3),
        keepout: false,
        remove_islands: false,
        fill: vec![],
    }
}

fn track(a: Point, b: Point, w: f64, net: &str) -> Item {
    Item::Track(Track { locked: false, a, b, width: mm(w), layer: layer::TOP_COPPER, net: net.into() })
}

fn via(pos: Point, net: &str) -> Item {
    Item::Via(Via { locked: false, pos, diameter: mm(0.6), drill: mm(0.3), from_layer: layer::TOP_COPPER, to_layer: layer::BOTTOM_COPPER, net: net.into() })
}

fn doc_with(items: Vec<Item>) -> Document {
    Document::from_item_list(DocKind::Pcb, items)
}

/// Adds a footprint "L/X" made of `pads`, placed at `pos`, all pads on `net`.
fn with_footprint(items: &mut Vec<Item>, pads: Vec<Pad>, pos: Point, net: &str) -> Footprint {
    let mut f = Footprint::new("X");
    let pad_nets = pads.iter().map(|p| (p.number.clone(), net.to_string())).collect();
    f.items = pads.into_iter().map(Item::Pad).collect();
    items.push(Item::Footprint(PlacedFootprint {
        board_only: false,
        designator: "U1".into(),
        value: String::new(),
        footprint: "L/X".into(),
        pos,
        rotation: 0.0,
        side: Side::Top,
        locked: false,
        pad_nets,
        component: String::new(),
        parameters: Default::default(),
    }));
    f
}

fn smd_pad(number: &str, w: f64, h: f64, shape: PadShape) -> Pad {
    Pad {
        number: number.into(),
        pos: Point::default(),
        rotation: 0.0,
        shape,
        width: mm(w),
        height: mm(h),
        corner_ratio: 0.25,
        kind: PadKind::Smd,
        layer: layer::TOP_COPPER,
        drill: None,
     copper_shapes: Default::default(),}
}

fn first_zone(doc: &Document) -> ItemId {
    doc.items().find_map(|(id, i)| matches!(i, Item::Zone(_)).then_some(id)).unwrap()
}

fn ring_contains(r: &[Point], q: Point) -> bool {
    let (x, y) = (q.x as f64, q.y as f64);
    let mut inside = false;
    for i in 0..r.len() {
        let (a, b) = (r[i], r[(i + 1) % r.len()]);
        let (ax, ay, bx, by) = (a.x as f64, a.y as f64, b.x as f64, b.y as f64);
        if (ay > y) != (by > y) && x < ax + (y - ay) * (bx - ax) / (by - ay) {
            inside = !inside;
        }
    }
    inside
}

fn in_fill(fill: &[ZonePolygon], q: Point) -> bool {
    fill.iter().any(|z| ring_contains(&z.outer, q) && !z.holes.iter().any(|h| ring_contains(h, q)))
}

fn area(fill: &[ZonePolygon]) -> f64 {
    let a = |r: &[Point]| super::super::geometry::signed_area(r);
    fill.iter().map(|z| a(&z.outer) + z.holes.iter().map(|h| a(h)).sum::<f64>()).sum()
}

fn mm2(v: f64) -> f64 {
    v * 1e12
}

fn bbox(fill: &[ZonePolygon]) -> (Point, Point) {
    let pts = fill.iter().flat_map(|z| z.outer.iter().copied());
    let b = crate::geom::BBox::from_points(pts).unwrap();
    (b.min, b.max)
}

fn rules_mut(items: &mut [Item]) -> &mut Vec<Rule> {
    for i in items.iter_mut() {
        if let Item::Board(b) = i {
            return &mut b.rules;
        }
    }
    unreachable!()
}

fn set_connect(items: &mut [Item], style: ZoneConnect) {
    for r in rules_mut(items) {
        if let RuleKind::ZoneConnect { style: s, .. } = &mut r.kind {
            *s = style;
        }
    }
}

fn dist_point_seg(q: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let l2 = dx * dx + dy * dy;
    let t = if l2 > 0.0 { (((q.0 - a.0) * dx + (q.1 - a.1) * dy) / l2).clamp(0.0, 1.0) } else { 0.0 };
    (a.0 + t * dx - q.0).hypot(a.1 + t * dy - q.1)
}

/// Minimum distance from segment `a`–`b` to any fill edge (vertices sampled densely).
fn boundary_distance(fill: &[ZonePolygon], a: Point, b: Point) -> f64 {
    let (a, b) = ((a.x as f64, a.y as f64), (b.x as f64, b.y as f64));
    let mut best = f64::MAX;
    for z in fill {
        for r in std::iter::once(&z.outer).chain(z.holes.iter()) {
            for i in 0..r.len() {
                let (u, v) = (r[i], r[(i + 1) % r.len()]);
                for k in 0..=20 {
                    let t = k as f64 / 20.0;
                    let q = (u.x as f64 + t * (v.x - u.x) as f64, u.y as f64 + t * (v.y - u.y) as f64);
                    best = best.min(dist_point_seg(q, a, b));
                }
            }
        }
    }
    best
}

#[test]
fn rectangular_zone_without_obstacles_equals_its_outline() {
    let mut items = board_items();
    items.push(Item::Zone(zone(rect(5.0, 5.0, 45.0, 25.0), "GND")));
    let doc = doc_with(items);
    let fill = fill_zone(&doc, first_zone(&doc));
    assert_eq!(fill.len(), 1);
    assert!(fill[0].holes.is_empty());
    assert_eq!(fill[0].outer.len(), 4);
    assert!(super::super::geometry::signed_area(&fill[0].outer) > 0.0, "outer must be CCW");
    let mut pts = fill[0].outer.clone();
    pts.sort_by_key(|q| (q.x, q.y));
    let mut want = rect(5.0, 5.0, 45.0, 25.0);
    want.sort_by_key(|q| (q.x, q.y));
    assert_eq!(pts, want);
}

#[test]
fn zone_is_clipped_to_board_minus_outline_clearance() {
    let mut items = board_items();
    items.push(Item::Zone(zone(rect(-10.0, -10.0, 60.0, 40.0), "GND")));
    let doc = doc_with(items);
    let fill = fill_zone(&doc, first_zone(&doc));
    assert_eq!(fill.len(), 1);
    let (lo, hi) = bbox(&fill);
    let tol = 2_000;
    assert!((lo.x - mm(0.3)).abs() < tol && (lo.y - mm(0.3)).abs() < tol, "{lo:?}");
    assert!((hi.x - mm(49.7)).abs() < tol && (hi.y - mm(29.7)).abs() < tol, "{hi:?}");
    assert!((area(&fill) - mm2(49.4 * 29.4)).abs() < mm2(0.01));
}

#[test]
fn no_board_outline_uses_zone_outline() {
    let items = vec![Item::Board(BoardSettings::default()), Item::Zone(zone(rect(-10.0, -10.0, 60.0, 40.0), "GND"))];
    let doc = doc_with(items);
    let fill = fill_zone(&doc, first_zone(&doc));
    assert!((area(&fill) - mm2(70.0 * 50.0)).abs() < mm2(0.001));
}

#[test]
fn board_cutout_is_cleared_with_outline_clearance() {
    let mut items = board_items();
    items.push(Item::Rect(Rect { a: p(20.0, 10.0), b: p(30.0, 20.0), width: mm(0.1), layer: layer::BOARD_OUTLINE, fill: Default::default() }));
    items.push(Item::Zone(zone(rect(-10.0, -10.0, 60.0, 40.0), "GND")));
    let doc = doc_with(items);
    let fill = fill_zone(&doc, first_zone(&doc));
    assert_eq!(fill.len(), 1);
    assert_eq!(fill[0].holes.len(), 1);
    assert!(!in_fill(&fill, p(25.0, 15.0)));
    assert!(!in_fill(&fill, p(19.8, 15.0)));
    assert!(in_fill(&fill, p(19.6, 15.0)));
}

#[test]
fn other_net_track_gets_exact_clearance() {
    let mut items = board_items();
    let (a, b) = (p(10.0, 12.0), p(40.0, 18.0));
    items.push(track(a, b, 0.25, "SIG"));
    // A track crossing the whole zone too.
    items.push(track(p(-5.0, 25.0), p(55.0, 25.0), 0.5, "SIG"));
    items.push(Item::Zone(zone(rect(2.0, 2.0, 48.0, 28.0), "GND")));
    let doc = doc_with(items);
    let fill = fill_zone(&doc, first_zone(&doc));
    // The crossing track splits the zone.
    assert_eq!(fill.len(), 2);
    let d = boundary_distance(&fill, a, b);
    let want = mm(0.2 + 0.125) as f64;
    assert!((d - want).abs() < 3_000.0, "distance {d} vs {want}");
    let d2 = boundary_distance(&fill, p(5.0, 25.0), p(45.0, 25.0));
    assert!((d2 - mm(0.45) as f64).abs() < 3_000.0, "distance {d2}");
    // Never closer than the clearance anywhere along the track.
    assert!(!in_fill(&fill, p(25.0, 15.0)));
    assert!(!in_fill(&fill, p(25.0, 15.0 + 0.3)));
    assert!(in_fill(&fill, p(25.0, 15.0 + 0.4)));
}

#[test]
fn zone_extra_clearance_wins_when_larger() {
    let mut items = board_items();
    let (a, b) = (p(10.0, 15.0), p(40.0, 15.0));
    items.push(track(a, b, 0.2, "SIG"));
    let mut z = zone(rect(2.0, 2.0, 48.0, 28.0), "GND");
    z.clearance = mm(0.5);
    items.push(Item::Zone(z));
    let doc = doc_with(items);
    let fill = fill_zone(&doc, first_zone(&doc));
    let d = boundary_distance(&fill, a, b);
    assert!((d - mm(0.6) as f64).abs() < 3_000.0, "distance {d}");
}

#[test]
fn arc_track_is_cleared() {
    let mut items = board_items();
    items.push(Item::ArcTrack(ArcTrack { locked: false, center: p(25.0, 15.0), radius: mm(5.0), start_angle: 0.0, sweep: 180.0, width: mm(0.25), layer: layer::TOP_COPPER, net: "SIG".into() }));
    items.push(Item::Zone(zone(rect(2.0, 2.0, 48.0, 28.0), "GND")));
    let doc = doc_with(items);
    let fill = fill_zone(&doc, first_zone(&doc));
    for deg in [0.0f64, 33.0, 90.0, 147.0, 180.0] {
        let (s, c) = deg.to_radians().sin_cos();
        for r in [5.0 - 0.3, 5.0, 5.0 + 0.3] {
            assert!(!in_fill(&fill, p(25.0 + r * c, 15.0 + r * s)), "deg {deg} r {r}");
        }
        assert!(in_fill(&fill, p(25.0 + 5.4 * c, 15.0 + 5.4 * s)));
    }
}

fn thermal_doc(style: ZoneConnect, shape: PadShape) -> (Document, Vec<ZonePolygon>) {
    let mut items = board_items();
    set_connect(&mut items, style);
    let f = with_footprint(&mut items, vec![smd_pad("1", 2.0, 2.0, shape)], p(25.0, 15.0), "GND");
    items.push(via(p(5.0, 5.0), "GND"));
    let mut z = zone(rect(-10.0, -10.0, 60.0, 40.0), "GND");
    z.remove_islands = true;
    items.push(Item::Zone(z));
    let mut doc = doc_with(items);
    doc.put_cached_footprint("L/X", f);
    let fill = fill_zone(&doc, first_zone(&doc));
    (doc, fill)
}

#[test]
fn thermal_relief_has_ring_gap_and_four_spokes() {
    for shape in [PadShape::Rect, PadShape::Circle, PadShape::RoundRect] {
        let (_, fill) = thermal_doc(ZoneConnect::Thermal, shape);
        assert_eq!(fill.len(), 1, "{shape:?}: pad connects to the main fill");
        assert_eq!(fill[0].holes.len(), 4, "{shape:?}: ring split by four spokes");
        // Pad copper area is flooded, the gap is open except on the spokes.
        assert!(in_fill(&fill, p(25.0, 15.0)));
        let g = 1.0 + 0.15;
        for (dx, dy) in [(g, 0.0), (-g, 0.0), (0.0, g), (0.0, -g)] {
            assert!(in_fill(&fill, p(25.0 + dx, 15.0 + dy)), "{shape:?}: spoke at ({dx}, {dy})");
        }
        // Off the spoke axis, inside the gap.
        for (dx, dy) in [(g, 0.5), (-g, -0.5), (0.5, g), (-0.5, -g)] {
            assert!(!in_fill(&fill, p(25.0 + dx, 15.0 + dy)), "{shape:?}: gap at ({dx}, {dy})");
        }
        // Spoke width: 0.3 mm.
        assert!(in_fill(&fill, p(25.0 + g, 15.0 + 0.13)));
        assert!(!in_fill(&fill, p(25.0 + g, 15.0 + 0.17)));
    }
}

#[test]
fn solid_connection_has_no_gap() {
    let (_, fill) = thermal_doc(ZoneConnect::Solid, PadShape::Rect);
    assert_eq!(fill.len(), 1);
    assert!(fill[0].holes.is_empty());
    assert!(in_fill(&fill, p(25.0 + 1.15, 15.0 + 0.5)));
}

#[test]
fn no_connection_isolates_the_pad() {
    let (_, fill) = thermal_doc(ZoneConnect::None, PadShape::Rect);
    assert_eq!(fill.len(), 1);
    assert_eq!(fill[0].holes.len(), 1);
    assert!(!in_fill(&fill, p(25.0, 15.0)));
    assert!(!in_fill(&fill, p(25.0 + 1.15, 15.0)));
    assert!(!in_fill(&fill, p(25.0 + 1.25, 15.0)));
    assert!(in_fill(&fill, p(25.0 + 1.35, 15.0)));
}

#[test]
fn zone_settings_apply_without_connect_rule() {
    let mut items = board_items();
    rules_mut(&mut items).retain(|r| !matches!(r.kind, RuleKind::ZoneConnect { .. }));
    let f = with_footprint(&mut items, vec![smd_pad("1", 2.0, 2.0, PadShape::Rect)], p(25.0, 15.0), "GND");
    let mut z = zone(rect(-10.0, -10.0, 60.0, 40.0), "GND");
    z.connect = ZoneConnect::Solid;
    items.push(Item::Zone(z));
    let mut doc = doc_with(items);
    doc.put_cached_footprint("L/X", f);
    let fill = fill_zone(&doc, first_zone(&doc));
    assert!(fill[0].holes.is_empty());
}

#[test]
fn other_net_pad_is_cleared() {
    let mut items = board_items();
    let f = with_footprint(&mut items, vec![smd_pad("1", 2.0, 1.0, PadShape::Rect)], p(25.0, 15.0), "SIG");
    items.push(Item::Zone(zone(rect(-10.0, -10.0, 60.0, 40.0), "GND")));
    let mut doc = doc_with(items);
    doc.put_cached_footprint("L/X", f);
    let fill = fill_zone(&doc, first_zone(&doc));
    assert_eq!(fill[0].holes.len(), 1);
    assert!(!in_fill(&fill, p(25.0 + 1.0 + 0.19, 15.0)));
    assert!(in_fill(&fill, p(25.0 + 1.0 + 0.22, 15.0)));
    assert!(!in_fill(&fill, p(25.0, 15.0 + 0.5 + 0.19)));
}

#[test]
fn higher_priority_zone_carves_lower_one() {
    let mut items = board_items();
    let mut hi = zone(rect(20.0, 10.0, 30.0, 20.0), "VCC");
    hi.priority = 1;
    items.push(Item::Zone(hi));
    items.push(Item::Zone(zone(rect(-10.0, -10.0, 60.0, 40.0), "GND")));
    let doc = doc_with(items);
    let fills = fill_all(&doc);
    assert_eq!(fills.len(), 2);
    let (vcc, gnd) = (&fills[0].1, &fills[1].1);
    assert!((area(vcc) - mm2(100.0)).abs() < mm2(0.001), "higher priority is untouched");
    assert!(!in_fill(gnd, p(25.0, 15.0)));
    assert!(!in_fill(gnd, p(20.0 - 0.19, 15.0)));
    assert!(in_fill(gnd, p(20.0 - 0.21, 15.0)));
    assert_eq!(gnd[0].holes.len(), 1);
    // Clockwise holes.
    assert!(super::super::geometry::signed_area(&gnd[0].holes[0]) < 0.0);

    // fill_zone uses the stored fill of the other zone.
    let mut items = doc.item_list();
    if let Item::Zone(z) = &mut items[2] {
        z.fill = vcc.clone();
    }
    let doc2 = doc_with(items);
    let gnd2 = fill_zone(&doc2, ItemId(3));
    assert!((area(&gnd2) - area(gnd)).abs() < mm2(0.001));
}

#[test]
fn lower_priority_zone_does_not_affect_higher_one() {
    let mut items = board_items();
    items.push(Item::Zone(zone(rect(20.0, 10.0, 30.0, 20.0), "VCC")));
    let mut big = zone(rect(-10.0, -10.0, 60.0, 40.0), "GND");
    big.priority = 5;
    items.push(Item::Zone(big));
    let doc = doc_with(items);
    let fills = fill_all(&doc);
    let (vcc, gnd) = (&fills[0].1, &fills[1].1);
    assert!(gnd[0].holes.is_empty());
    assert!((area(gnd) - mm2(49.4 * 29.4)).abs() < mm2(0.01));
    assert!(vcc.is_empty(), "fully covered by the higher-priority pour");
}

#[test]
fn island_removal_drops_unconnected_regions() {
    for remove in [true, false] {
        let mut items = board_items();
        items.push(track(p(25.0, -5.0), p(25.0, 35.0), 0.3, "SIG"));
        items.push(via(p(10.0, 15.0), "GND"));
        let mut z = zone(rect(-10.0, -10.0, 60.0, 40.0), "GND");
        z.remove_islands = remove;
        items.push(Item::Zone(z));
        let doc = doc_with(items);
        let fill = fill_zone(&doc, first_zone(&doc));
        assert_eq!(fill.len(), if remove { 1 } else { 2 });
        assert!(in_fill(&fill, p(10.0, 10.0)));
        assert_eq!(in_fill(&fill, p(40.0, 10.0)), !remove);
    }
}

#[test]
fn same_net_track_anchors_island_and_is_not_cleared() {
    let mut items = board_items();
    items.push(track(p(25.0, -5.0), p(25.0, 35.0), 0.3, "SIG"));
    items.push(track(p(35.0, 10.0), p(45.0, 10.0), 0.3, "GND"));
    items.push(via(p(10.0, 15.0), "GND"));
    let mut z = zone(rect(-10.0, -10.0, 60.0, 40.0), "GND");
    z.remove_islands = true;
    items.push(Item::Zone(z));
    let doc = doc_with(items);
    let fill = fill_zone(&doc, first_zone(&doc));
    assert_eq!(fill.len(), 2);
    assert!(in_fill(&fill, p(40.0, 10.0)));
}

#[test]
fn empty_net_zone_keeps_all_islands() {
    let mut items = board_items();
    items.push(track(p(25.0, -5.0), p(25.0, 35.0), 0.3, "SIG"));
    let mut z = zone(rect(-10.0, -10.0, 60.0, 40.0), "");
    z.remove_islands = true;
    items.push(Item::Zone(z));
    let doc = doc_with(items);
    assert_eq!(fill_zone(&doc, first_zone(&doc)).len(), 2);
}

#[test]
fn min_width_removes_thin_neck() {
    // Two 10 × 10 squares joined by a 0.2 mm wide, 5 mm long neck.
    let outline = vec![p(5.0, 5.0), p(15.0, 5.0), p(15.0, 9.9), p(20.0, 9.9), p(20.0, 5.0), p(30.0, 5.0), p(30.0, 15.0), p(20.0, 15.0), p(20.0, 10.1), p(15.0, 10.1), p(15.0, 15.0), p(5.0, 15.0)];
    for (min_width, pieces) in [(0.0, 1), (0.15, 1), (0.3, 2)] {
        let mut items = board_items();
        let mut z = zone(outline.clone(), "GND");
        z.min_width = mm(min_width);
        items.push(Item::Zone(z));
        let doc = doc_with(items);
        let fill = fill_zone(&doc, first_zone(&doc));
        assert_eq!(fill.len(), pieces, "min width {min_width}");
        // The squares survive almost untouched (only convex corners round off).
        let a = area(&fill);
        assert!(a > mm2(199.9) && a <= mm2(201.0 + 1e-6), "area {a}");
    }
}

#[test]
fn keepout_subtracts_on_its_layer_only() {
    let mut items = board_items();
    let mut k = zone(rect(20.0, 10.0, 30.0, 20.0), "");
    k.keepout = true;
    items.push(Item::Zone(k.clone()));
    k.layer = layer::BOTTOM_COPPER;
    k.outline = rect(5.0, 5.0, 10.0, 10.0);
    items.push(Item::Zone(k));
    items.push(Item::Zone(zone(rect(-10.0, -10.0, 60.0, 40.0), "GND")));
    let doc = doc_with(items);
    let fills = fill_all(&doc);
    assert_eq!(fills.len(), 1, "keepouts are not filled");
    let fill = &fills[0].1;
    assert!(!in_fill(fill, p(25.0, 15.0)));
    assert!(in_fill(fill, p(19.9, 15.0)));
    assert!(in_fill(fill, p(7.0, 7.0)));
}

#[test]
fn npth_hole_is_cleared_on_every_layer() {
    let mut items = board_items();
    let hole = Pad {
        number: "".into(),
        pos: Point::default(),
        rotation: 0.0,
        shape: PadShape::Circle,
        width: mm(3.0),
        height: mm(3.0),
        corner_ratio: 0.0,
        kind: PadKind::Npth,
        layer: layer::MULTI_LAYER,
        drill: Some(Drill { width: mm(3.0), height: mm(3.0) }),
     copper_shapes: Default::default(),};
    let f = with_footprint(&mut items, vec![hole], p(25.0, 15.0), "");
    items.push(Item::Zone(zone(rect(-10.0, -10.0, 60.0, 40.0), "GND")));
    let mut bottom = zone(rect(-10.0, -10.0, 60.0, 40.0), "GND");
    bottom.layer = layer::BOTTOM_COPPER;
    items.push(Item::Zone(bottom));
    let mut doc = doc_with(items);
    doc.put_cached_footprint("L/X", f);
    let fills = fill_all(&doc);
    assert_eq!(fills.len(), 2);
    for (_, fill) in &fills {
        assert!(!in_fill(fill, p(25.0, 15.0)));
        assert!(!in_fill(fill, p(25.0 + 1.69, 15.0)));
        assert!(in_fill(fill, p(25.0 + 1.72, 15.0)));
        assert!(!in_fill(fill, p(25.0, 15.0 - 1.69)));
    }
}

#[test]
fn other_net_via_is_cleared_and_same_net_via_is_flooded() {
    let mut items = board_items();
    items.push(via(p(10.0, 15.0), "SIG"));
    items.push(via(p(40.0, 15.0), "GND"));
    items.push(Item::Zone(zone(rect(-10.0, -10.0, 60.0, 40.0), "GND")));
    let doc = doc_with(items);
    let fill = fill_zone(&doc, first_zone(&doc));
    assert_eq!(fill[0].holes.len(), 1);
    assert!(!in_fill(&fill, p(10.0 + 0.49, 15.0)));
    assert!(in_fill(&fill, p(10.0 + 0.52, 15.0)));
    assert!(in_fill(&fill, p(40.0, 15.0)));
}

#[test]
fn output_is_simplified() {
    let mut items = board_items();
    // Collinear points on the zone outline are dropped.
    items.push(Item::Zone(zone(vec![p(5.0, 5.0), p(10.0, 5.0), p(15.0, 5.0), p(15.0, 15.0), p(5.0, 15.0)], "GND")));
    let doc = doc_with(items);
    let fill = fill_zone(&doc, first_zone(&doc));
    assert_eq!(fill[0].outer.len(), 4);
}

#[test]
fn spokes_as_wide_as_min_width_survive() {
    let mut items = board_items();
    let f = with_footprint(&mut items, vec![smd_pad("1", 2.0, 2.0, PadShape::Rect)], p(25.0, 15.0), "GND");
    let mut z = zone(rect(-10.0, -10.0, 60.0, 40.0), "GND");
    z.remove_islands = true;
    z.min_width = mm(0.3);
    items.push(Item::Zone(z));
    let mut doc = doc_with(items);
    doc.put_cached_footprint("L/X", f);
    let fill = fill_zone(&doc, first_zone(&doc));
    assert_eq!(fill.len(), 1);
    assert_eq!(fill[0].holes.len(), 4);
    assert!(in_fill(&fill, p(25.0 + 1.15, 15.0)));
}

#[test]
fn blocked_thermal_leaves_no_stub_island() {
    for remove in [true, false] {
        let mut items = board_items();
        let f = with_footprint(&mut items, vec![smd_pad("1", 2.0, 2.0, PadShape::Rect)], p(25.0, 15.0), "GND");
        // Other-net square loop whose clearance meets the thermal ring.
        let d = 1.5;
        let c = [p(25.0 - d, 15.0 - d), p(25.0 + d, 15.0 - d), p(25.0 + d, 15.0 + d), p(25.0 - d, 15.0 + d)];
        for i in 0..4 {
            items.push(track(c[i], c[(i + 1) % 4], 0.25, "SIG"));
        }
        items.push(via(p(5.0, 5.0), "GND"));
        let mut z = zone(rect(-10.0, -10.0, 60.0, 40.0), "GND");
        z.remove_islands = remove;
        items.push(Item::Zone(z));
        let mut doc = doc_with(items);
        doc.put_cached_footprint("L/X", f);
        let fill = fill_zone(&doc, first_zone(&doc));
        assert_eq!(fill.len(), if remove { 1 } else { 2 });
        assert_eq!(in_fill(&fill, p(25.0, 15.0)), !remove);
    }
}

#[test]
fn rotated_pad_spokes_follow_pad_frame() {
    let mut items = board_items();
    let mut pad = smd_pad("1", 3.0, 1.0, PadShape::Rect);
    pad.rotation = 45.0;
    let f = with_footprint(&mut items, vec![pad], p(25.0, 15.0), "GND");
    let mut z = zone(rect(-10.0, -10.0, 60.0, 40.0), "GND");
    z.remove_islands = true;
    items.push(Item::Zone(z));
    let mut doc = doc_with(items);
    doc.put_cached_footprint("L/X", f);
    let fill = fill_zone(&doc, first_zone(&doc));
    assert_eq!(fill.len(), 1);
    assert_eq!(fill[0].holes.len(), 4);
    // Along the pad's long axis, just past its end (1.5 + 0.15 mm).
    let (s, c) = 45f64.to_radians().sin_cos();
    assert!(in_fill(&fill, p(25.0 + 1.65 * c, 15.0 + 1.65 * s)));
    // In the gap beside the long edge, off the spoke axis.
    assert!(!in_fill(&fill, p(25.0 + 0.5 * c - 0.65 * -s, 15.0 + 0.5 * s - 0.65 * c)));
}

/// 100 × 100 mm board, 500 pads (a fifth of them GND), 2000 tracks and 200
/// vias; pours GND on both outer layers. Run with
/// `cargo test --release -p pcbplannercad-core pcb::fill -- --ignored --nocapture`.
#[test]
#[ignore]
fn perf_large_board() {
    let mut items = vec![
        Item::Board(BoardSettings::default()),
        Item::Rect(Rect { a: p(0.0, 0.0), b: p(100.0, 100.0), width: mm(0.1), layer: layer::BOARD_OUTLINE, fill: Default::default() }),
    ];
    let mut seed = 12345u64;
    let mut rnd = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed % 1_000_000) as f64 / 1_000_000.0
    };
    // 50 footprints × 10 pads, 2.54 mm pitch rows.
    let mut f = Footprint::new("X");
    f.items = (0..10)
        .map(|i| Item::Pad(Pad { pos: Point::new(mm(i as f64 * 1.27), 0), ..smd_pad(&format!("{}", i + 1), 0.6, 1.5, PadShape::RoundRect) }))
        .collect();
    for k in 0..50 {
        let pos = p(5.0 + (k % 7) as f64 * 13.0, 5.0 + (k / 7) as f64 * 12.5);
        let pad_nets = (0..10).map(|i| (format!("{}", i + 1), if i % 5 == 0 { "GND".to_string() } else { format!("N{k}_{i}") })).collect();
        items.push(Item::Footprint(PlacedFootprint {
            board_only: false,
            designator: format!("U{k}"),
            value: String::new(),
            footprint: "L/X".into(),
            pos,
            rotation: if k % 2 == 0 { 0.0 } else { 90.0 },
            side: if k % 3 == 0 { Side::Bottom } else { Side::Top },
            locked: false,
            pad_nets,
            component: String::new(),
            parameters: Default::default(),
        }));
    }
    for i in 0..2000 {
        let a = p(2.0 + rnd() * 96.0, 2.0 + rnd() * 96.0);
        let (dx, dy) = if i % 2 == 0 { (rnd() * 10.0, 0.0) } else { (0.0, rnd() * 10.0) };
        let b = Point::new(a.x + mm(dx), a.y + mm(dy));
        let layer = if i % 3 == 0 { layer::BOTTOM_COPPER } else { layer::TOP_COPPER };
        let net = if i % 10 == 0 { "GND".to_string() } else { format!("T{}", i % 97) };
        items.push(Item::Track(Track { locked: false, a, b, width: mm(0.25), layer, net }));
    }
    for i in 0..200 {
        items.push(via(p(3.0 + rnd() * 94.0, 3.0 + rnd() * 94.0), if i % 4 == 0 { "GND" } else { "SIG" }));
    }
    for l in [layer::TOP_COPPER, layer::BOTTOM_COPPER] {
        let mut z = zone(rect(-1.0, -1.0, 101.0, 101.0), "GND");
        z.layer = l;
        z.min_width = mm(0.2);
        z.remove_islands = true;
        items.push(Item::Zone(z));
    }
    let mut doc = doc_with(items);
    doc.put_cached_footprint("L/X", f);
    let t = std::time::Instant::now();
    let fills = fill_all(&doc);
    let dt = t.elapsed();
    let verts: usize = fills.iter().flat_map(|(_, f)| f.iter()).map(|z| z.outer.len() + z.holes.iter().map(|h| h.len()).sum::<usize>()).sum();
    let polys: usize = fills.iter().map(|(_, f)| f.len()).sum();
    println!("filled 2 GND planes in {dt:?}: {polys} polygons, {verts} vertices");
    assert_eq!(fills.len(), 2);
    assert!(polys > 0);
}

/// A 10 × 10 mm square with a 2 × 2 mm hole joined to its outline by a slit,
/// as fractured fills are stored, resolves into one outline with one hole.
#[test]
fn unfracture_turns_slit_holes_into_holes() {
    let ring = vec![p(0.0, 0.0), p(10.0, 0.0), p(10.0, 10.0), p(0.0, 10.0), p(0.0, 4.0), p(4.0, 4.0), p(4.0, 6.0), p(6.0, 6.0), p(6.0, 4.0), p(4.0, 4.0), p(0.0, 4.0)];
    let fractured = vec![ZonePolygon { outer: ring, holes: vec![] }];
    assert!(is_fractured(&fractured));
    let fixed = unfracture(&fractured);
    assert_eq!(fixed.len(), 1);
    assert_eq!(fixed[0].holes.len(), 1);
    assert!(!is_fractured(&fixed));
    let area = |r: &[Point]| crate::pcb::geometry::signed_area(r).abs() / 1e12;
    assert!((area(&fixed[0].outer) - area(&fixed[0].holes[0]) - 96.0).abs() < 1e-6);
    let plain = vec![ZonePolygon { outer: rect(0.0, 0.0, 1.0, 1.0), holes: vec![] }];
    assert!(!is_fractured(&plain));
}
