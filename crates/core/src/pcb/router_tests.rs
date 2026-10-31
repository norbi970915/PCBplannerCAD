use std::collections::BTreeMap;

use super::geom::{octant, seg_poly_dist, seg_seg_dist, V};
use super::*;
use crate::document::DocKind;
use crate::item::{Line, Pad, PadKind, PadShape};
use crate::library::Footprint;
use crate::pcb::board::BoardSettings;
use crate::pcb::items::{PlacedFootprint, Side};
use crate::units::mm;

const TOP: LayerId = layer::TOP_COPPER;

fn pt(x: f64, y: f64) -> Point {
    Point::new(mm(x), mm(y))
}

fn track(a: Point, b: Point, net: &str) -> Item {
    Item::Track(Track { locked: false, a, b, width: mm(0.25), layer: TOP, net: net.into() })
}

fn pad(number: &str, x: f64, y: f64, w: f64, h: f64) -> Item {
    Item::Pad(Pad {
        number: number.into(),
        pos: pt(x, y),
        rotation: 0.0,
        shape: PadShape::Rect,
        width: mm(w),
        height: mm(h),
        corner_ratio: 0.0,
        kind: PadKind::Smd,
        layer: TOP,
        drill: None,
     copper_shapes: Default::default(),})
}

fn placed(designator: &str, at: Point, nets: &[(&str, &str)]) -> Item {
    Item::Footprint(PlacedFootprint {
        board_only: false,
        designator: designator.into(),
        value: String::new(),
        footprint: "L/P".into(),
        pos: at,
        rotation: 0.0,
        side: Side::Top,
        locked: false,
        pad_nets: nets.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect::<BTreeMap<_, _>>(),
        component: String::new(),
        parameters: Default::default(),
    })
}

/// A board whose single-pad footprint "L/P" is a `w`×`h` mm rect pad.
fn board(items: Vec<Item>, pad_w: f64, pad_h: f64) -> Document {
    let mut all = vec![Item::Board(BoardSettings::default())];
    all.extend(items);
    let mut doc = Document::from_item_list(DocKind::Pcb, all);
    let mut f = Footprint::new("P");
    f.items = vec![pad("1", 0.0, 0.0, pad_w, pad_h)];
    doc.put_cached_footprint("L/P", f);
    doc
}

fn req(net: &str, a: Point, b: Point, mode: RouteMode) -> RouteRequest {
    RouteRequest { net: net.into(), layer: TOP, start: a, end: b, width: mm(0.25), mode, corner: CornerStyle::Diagonal45, ignore: vec![], posture_flip: false }
}

fn rect(c: Point, w: f64, h: f64) -> Vec<V> {
    let (x, y, hw, hh) = (c.x as f64, c.y as f64, mm(w) as f64 / 2.0, mm(h) as f64 / 2.0);
    vec![V::new(x - hw, y - hh), V::new(x + hw, y - hh), V::new(x + hw, y + hh), V::new(x - hw, y + hh)]
}

fn assert_connected(tracks: &[Track], from: Point, to: Option<Point>) {
    assert!(!tracks.is_empty());
    assert_eq!(tracks[0].a, from);
    for w in tracks.windows(2) {
        assert_eq!(w[0].b, w[1].a, "chain broken");
    }
    if let Some(to) = to {
        assert_eq!(tracks.last().unwrap().b, to);
    }
}

fn assert_octilinear(tracks: &[Track]) {
    for t in tracks {
        assert!(octant(t.b.x - t.a.x, t.b.y - t.a.y).is_some(), "not octilinear: {t:?}");
    }
}

/// Minimum edge-to-edge gap between the result tracks and other-net copper.
fn min_gap(doc: &Document, tracks: &[Track], net: &str, pad_w: f64, pad_h: f64) -> f64 {
    let mut gap = f64::INFINITY;
    for t in tracks {
        let (a, b, hw) = (V::of(t.a), V::of(t.b), t.width as f64 / 2.0);
        for (_, item) in doc.items() {
            match item {
                Item::Track(o) if o.net != net && o.layer == t.layer => {
                    gap = gap.min(seg_seg_dist(a, b, V::of(o.a), V::of(o.b)) - hw - o.width as f64 / 2.0);
                }
                Item::Footprint(fp) if fp.net_of("1") != net => {
                    gap = gap.min(seg_poly_dist(a, b, &rect(fp.pos, pad_w, pad_h)) - hw);
                }
                _ => {}
            }
        }
    }
    gap
}

#[test]
fn straight_route_on_empty_board() {
    let doc = board(vec![], 1.0, 1.0);
    let r = route(&doc, &req("A", pt(0.0, 0.0), pt(10.0, 0.0), RouteMode::Walkaround));
    assert!(r.reached);
    assert_eq!(r.tracks.len(), 1);
    assert_eq!((r.tracks[0].a, r.tracks[0].b), (pt(0.0, 0.0), pt(10.0, 0.0)));
    assert_eq!(r.tracks[0].net, "A");

    let r = route(&doc, &req("A", pt(0.0, 0.0), pt(10.0, 4.0), RouteMode::Walkaround));
    assert!(r.reached);
    assert_eq!(r.tracks.len(), 2);
    assert_eq!(r.tracks[0].b, pt(6.0, 0.0), "straight leg first");
    assert_octilinear(&r.tracks);

    let mut q = req("A", pt(0.0, 0.0), pt(10.0, 4.0), RouteMode::Ignore);
    q.posture_flip = true;
    let r = route(&doc, &q);
    assert!(r.reached);
    assert_eq!(r.tracks[0].b, pt(4.0, 4.0), "diagonal leg first");
    q.corner = CornerStyle::Orthogonal90;
    q.posture_flip = false;
    assert_eq!(route(&doc, &q).tracks[0].b, pt(10.0, 0.0));
}

#[test]
fn walkaround_avoids_pad_and_track() {
    let doc = board(vec![placed("U1", pt(5.0, 0.0), &[("1", "B")]), track(pt(9.0, -1.5), pt(9.0, 1.5), "C")], 1.5, 1.5);
    for corner in [CornerStyle::Diagonal45, CornerStyle::Orthogonal90, CornerStyle::Any] {
        let mut q = req("A", pt(0.0, 0.0), pt(13.0, 0.0), RouteMode::Walkaround);
        q.corner = corner;
        let r = route(&doc, &q);
        assert!(r.reached, "{corner:?}: {}", r.message);
        assert_connected(&r.tracks, q.start, Some(q.end));
        let gap = min_gap(&doc, &r.tracks, "A", 1.5, 1.5);
        assert!(gap >= mm(0.2) as f64 - 1.0, "{corner:?}: clearance violated, gap {gap}");
        if corner == CornerStyle::Diagonal45 {
            assert_octilinear(&r.tracks);
            assert!(r.tracks.len() <= 9, "too many corners: {}", r.tracks.len());
        }
        if corner == CornerStyle::Orthogonal90 {
            assert!(r.tracks.iter().all(|t| t.a.x == t.b.x || t.a.y == t.b.y));
        }
    }
}

#[test]
fn same_net_obstacles_are_ignored() {
    let doc = board(vec![placed("U1", pt(5.0, 0.0), &[("1", "A")]), track(pt(9.0, -1.5), pt(9.0, 1.5), "A")], 1.5, 1.5);
    let r = route(&doc, &req("A", pt(0.0, 0.0), pt(13.0, 0.0), RouteMode::Walkaround));
    assert!(r.reached);
    assert_eq!(r.tracks.len(), 1);
}

#[test]
fn ignored_items_are_not_obstacles() {
    let doc = board(vec![track(pt(5.0, -1.5), pt(5.0, 1.5), "B")], 1.0, 1.0);
    let id = doc.items().find(|(_, i)| matches!(i, Item::Track(_))).unwrap().0;
    let mut q = req("A", pt(0.0, 0.0), pt(10.0, 0.0), RouteMode::Walkaround);
    assert!(route(&doc, &q).tracks.len() > 1);
    q.ignore = vec![id];
    assert_eq!(route(&doc, &q).tracks.len(), 1);
}

#[test]
fn unreachable_end_returns_partial() {
    // The end sits inside a closed box of another net's tracks.
    let (a, b, c, d) = (pt(8.0, -2.0), pt(12.0, -2.0), pt(12.0, 2.0), pt(8.0, 2.0));
    let doc = board(vec![track(a, b, "B"), track(b, c, "B"), track(c, d, "B"), track(d, a, "B")], 1.0, 1.0);
    let r = route(&doc, &req("A", pt(0.0, 0.0), pt(10.0, 0.0), RouteMode::Walkaround));
    assert!(!r.reached);
    let at = r.blocked_at.expect("blocked point");
    assert_connected(&r.tracks, pt(0.0, 0.0), Some(at));
    let inside_box = at.x > mm(8.0) && at.x < mm(12.0) && at.y > mm(-2.0) && at.y < mm(2.0);
    assert!(!inside_box, "partial route entered the enclosure");
    assert!(min_gap(&doc, &r.tracks, "A", 1.0, 1.0) >= mm(0.2) as f64 - 1.0);

    // End inside another net's pad: route up to it, not into it.
    let doc = board(vec![placed("U1", pt(10.0, 0.0), &[("1", "B")])], 2.0, 2.0);
    let r = route(&doc, &req("A", pt(0.0, 0.0), pt(10.0, 0.0), RouteMode::Walkaround));
    assert!(!r.reached);
    assert!(r.blocked_at.is_some());
    assert!(min_gap(&doc, &r.tracks, "A", 2.0, 2.0) >= mm(0.2) as f64 - 1.0);
    assert!(r.tracks.last().unwrap().b.x > mm(8.0));
}

#[test]
fn respects_board_outline() {
    let outline: Vec<Item> = [(0.0, -1.0, 20.0, -1.0), (20.0, -1.0, 20.0, 5.0), (20.0, 5.0, 0.0, 5.0), (0.0, 5.0, 0.0, -1.0)]
        .iter()
        .map(|&(x0, y0, x1, y1)| Item::Line(Line { a: pt(x0, y0), b: pt(x1, y1), width: mm(0.1), layer: layer::BOARD_OUTLINE }))
        .collect();
    let mut items = outline;
    items.push(placed("U1", pt(10.0, 0.5), &[("1", "B")]));
    let doc = board(items, 2.0, 2.0);
    let r = route(&doc, &req("A", pt(2.0, 0.0), pt(18.0, 0.0), RouteMode::Walkaround));
    assert!(r.reached, "{}", r.message);
    // The pad leaves no room below (outline at y = -1), so the route goes above it.
    let max_y = r.tracks.iter().map(|t| t.a.y.max(t.b.y)).max().unwrap();
    assert!(max_y > mm(1.5));
    let min_y = r.tracks.iter().map(|t| t.a.y.min(t.b.y)).min().unwrap();
    assert!(min_y >= mm(-1.0 + 0.3 + 0.125) - 1);
}

fn moved_chain(r: &RouteResult, id: ItemId) -> Vec<Track> {
    r.moved
        .iter()
        .filter(|m| m.id == id)
        .map(|m| match &m.item {
            Item::Track(t) => t.clone(),
            _ => panic!("moved item is not a track"),
        })
        .collect()
}

#[test]
fn shove_pushes_parallel_track() {
    let (ta, tb) = (pt(0.0, 0.4), pt(20.0, 0.4));
    let (ua, ub) = (pt(0.0, 0.85), pt(20.0, 0.85));
    let doc = board(vec![track(ta, tb, "B"), track(ua, ub, "C")], 1.0, 1.0);
    let ids: Vec<ItemId> = doc.items().filter(|(_, i)| matches!(i, Item::Track(_))).map(|(id, _)| id).collect();
    let r = route(&doc, &req("A", pt(5.0, 0.0), pt(15.0, 0.0), RouteMode::Shove));
    assert!(r.reached, "{}", r.message);
    assert_eq!(r.tracks.len(), 1, "shove keeps the straight route");
    let b = moved_chain(&r, ids[0]);
    let c = moved_chain(&r, ids[1]);
    assert!(b.len() >= 3 && c.len() >= 3, "both tracks pushed (propagation)");
    assert_connected(&b, ta, Some(tb));
    assert_connected(&c, ua, Some(ub));
    assert_octilinear(&b);
    let route_seg = (V::of(r.tracks[0].a), V::of(r.tracks[0].b));
    let need = mm(0.25) as f64 + mm(0.2) as f64;
    for t in &b {
        assert!(seg_seg_dist(route_seg.0, route_seg.1, V::of(t.a), V::of(t.b)) >= need - 1.0);
        for u in &c {
            assert!(seg_seg_dist(V::of(t.a), V::of(t.b), V::of(u.a), V::of(u.b)) >= need - 1.0);
        }
    }
    assert!(b.iter().all(|t| t.net == "B" && t.width == mm(0.25)));
}


#[test]
fn shove_wraps_crossing_and_diagonal_tracks() {
    // A perpendicular track is pushed around the route's head; a diagonal
    // neighbor keeps exact 45° geometry.
    let (ca, cb) = (pt(10.0, -5.0), pt(10.0, 5.0));
    let (da, db) = (pt(0.0, -0.6), pt(6.0, 5.4));
    let doc = board(vec![track(ca, cb, "B"), track(da, db, "C")], 1.0, 1.0);
    let ids: Vec<ItemId> = doc.items().filter(|(_, i)| matches!(i, Item::Track(_))).map(|(id, _)| id).collect();
    let r = route(&doc, &req("A", pt(1.0, 0.0), pt(12.0, 0.0), RouteMode::Shove));
    assert!(r.reached, "{}", r.message);
    assert!(r.moved.iter().all(|m| ids.contains(&m.id)));
    for (id, (a, b)) in ids.iter().zip([(ca, cb), (da, db)]) {
        let chain = moved_chain(&r, *id);
        assert!(chain.len() >= 3, "track {id:?} not pushed");
        assert_connected(&chain, a, Some(b));
        assert_octilinear(&chain);
    }
    let mut moved_doc_items = vec![];
    for m in &r.moved {
        moved_doc_items.push(m.item.clone());
    }
    let after = board(moved_doc_items, 1.0, 1.0);
    assert!(min_gap(&after, &r.tracks, "A", 1.0, 1.0) >= mm(0.2) as f64 - 1.0);
}

#[test]
fn shove_falls_back_to_walkaround_for_pads() {
    let doc = board(vec![placed("U1", pt(10.0, 0.0), &[("1", "B")])], 1.0, 1.0);
    let r = route(&doc, &req("A", pt(0.0, 0.0), pt(20.0, 0.0), RouteMode::Shove));
    assert!(r.reached);
    assert!(r.moved.is_empty());
    assert!(r.tracks.len() > 1);
    assert!(min_gap(&doc, &r.tracks, "A", 1.0, 1.0) >= mm(0.2) as f64 - 1.0);
}

fn line_dist(t: &Track, p: Point) -> f64 {
    let (a, b) = (V::of(t.a), V::of(t.b));
    ((b - a).cross(V::of(p) - a) / (b - a).len()).abs()
}

#[test]
fn diff_pair_keeps_spacing() {
    let gap = mm(0.2);
    for (obstacle, corner) in [(false, CornerStyle::Diagonal45), (true, CornerStyle::Diagonal45), (false, CornerStyle::Orthogonal90)] {
        let items = if obstacle { vec![placed("U1", pt(6.0, 0.0), &[("1", "X")])] } else { vec![] };
        let doc = board(items, 1.5, 1.5);
        let mut q = req("D_P", pt(0.0, 0.0), pt(12.0, 4.0), RouteMode::Walkaround);
        q.width = mm(0.2);
        q.corner = corner;
        let (p, n) = route_diff_pair(&doc, &q, "D_N", gap);
        assert!(p.reached && n.reached, "{corner:?} {obstacle}: {}", p.message);
        assert_eq!(p.tracks.len(), n.tracks.len());
        assert_connected(&p.tracks, q.start, Some(q.end));
        assert!(n.tracks.iter().all(|t| t.net == "D_N"));
        let c = (q.width + gap) as f64;
        for (a, b) in p.tracks.iter().zip(&n.tracks) {
            let (da, db) = (V::of(a.b) - V::of(a.a), V::of(b.b) - V::of(b.a));
            assert!(da.norm().cross(db.norm()).abs() < 1e-4, "segments not parallel");
            assert!(da.norm().dot(db.norm()) > 0.0);
            assert!((line_dist(a, b.a) - c).abs() < 3.0, "pair distance {} != {c}", line_dist(a, b.a));
        }
        if obstacle {
            let mut all = p.tracks.clone();
            all.extend(n.tracks.clone());
            assert!(min_gap(&doc, &all, "D_P", 1.5, 1.5) >= mm(0.2) as f64 - 2.0);
        }
    }
}

#[test]
fn tune_length_hits_target() {
    for style in [TuneStyle::Accordion, TuneStyle::Trombone] {
        let doc = board(vec![track(pt(0.0, 0.0), pt(20.0, 0.0), "A"), track(pt(20.0, 0.0), pt(20.0, 10.0), "A"), track(pt(30.0, 5.0), pt(40.0, 5.0), "B")], 1.0, 1.0);
        let ids: Vec<ItemId> = doc.items().filter(|(_, i)| matches!(i, Item::Track(t) if t.net == "A")).map(|(id, _)| id).collect();
        let target = mm(42.0);
        let out = tune_length(&doc, &TuneRequest { tracks: ids.clone(), target_length: target, amplitude: mm(1.0), spacing: mm(0.6), style }).expect("tuned");
        let total: f64 = out.iter().map(|t| (V::of(t.b) - V::of(t.a)).len()).sum();
        assert!((total - target as f64).abs() <= target as f64 * 0.005, "{style:?}: {total}");
        assert_connected(&out, pt(0.0, 0.0), Some(pt(20.0, 10.0)));
        assert_octilinear(&out);
        // Never beyond the amplitude.
        assert!(out.iter().all(|t| t.a.y.abs().max(t.b.y.abs()) <= mm(10.0) && (t.a.y >= -mm(1.0) - 1)));
        assert!(min_gap(&doc, &out, "A", 1.0, 1.0) >= mm(0.2) as f64 - 1.0);

        // Impossible targets are reported.
        let err = tune_length(&doc, &TuneRequest { tracks: ids.clone(), target_length: mm(500.0), amplitude: mm(1.0), spacing: mm(0.6), style });
        assert!(err.is_err());
        let err = tune_length(&doc, &TuneRequest { tracks: ids, target_length: mm(10.0), amplitude: mm(1.0), spacing: mm(0.6), style });
        assert!(err.is_err());
    }
}

#[test]
fn tune_length_avoids_other_nets() {
    // A parallel neighbor right above the track forces meanders below it.
    let doc = board(vec![track(pt(0.0, 0.0), pt(20.0, 0.0), "A"), track(pt(0.0, 0.6), pt(20.0, 0.6), "B")], 1.0, 1.0);
    let id = doc.items().find(|(_, i)| matches!(i, Item::Track(t) if t.net == "A")).unwrap().0;
    let out = tune_length(&doc, &TuneRequest { tracks: vec![id], target_length: mm(24.0), amplitude: mm(1.0), spacing: mm(0.6), style: TuneStyle::Accordion }).unwrap();
    assert!(out.iter().all(|t| t.a.y <= 0 && t.b.y <= 0));
    assert!(min_gap(&doc, &out, "A", 1.0, 1.0) >= mm(0.2) as f64 - 1.0);
}

#[test]
fn net_length_and_vias() {
    let mut items = vec![track(pt(0.0, 0.0), pt(3.0, 4.0), "A"), track(pt(0.0, 0.0), pt(1.0, 0.0), "B")];
    items.push(Item::ArcTrack(crate::pcb::items::ArcTrack { locked: false, center: pt(0.0, 0.0), radius: mm(1.0), start_angle: 0.0, sweep: 180.0, width: mm(0.2), layer: TOP, net: "A".into() }));
    let doc = board(items, 1.0, 1.0);
    let expect = mm(5.0) as f64 + mm(1.0) as f64 * std::f64::consts::PI;
    assert!((net_length(&doc, "A") as f64 - expect).abs() <= 1.0);
    let v = via_for(&doc, "A", pt(1.0, 1.0));
    assert_eq!((v.diameter, v.drill), (mm(0.6), mm(0.3)));
    assert_eq!((v.from_layer, v.to_layer), (layer::TOP_COPPER, layer::BOTTOM_COPPER));
    // A via of another net blocks the layer it spans.
    let doc2 = board(vec![Item::Via(via_for(&doc, "B", pt(5.0, 0.0)))], 1.0, 1.0);
    let r = route(&doc2, &req("A", pt(0.0, 0.0), pt(10.0, 0.0), RouteMode::Walkaround));
    assert!(r.reached && r.tracks.len() > 1);
}

/// A dense board: 2000 pads and ~1000 tracks of many nets.
fn dense_board() -> Document {
    let mut items = Vec::new();
    for i in 0..50 {
        for j in 0..40 {
            let net = format!("N{}", (i * 40 + j) % 300);
            items.push(placed(&format!("R{i}_{j}"), pt(i as f64 * 2.0, j as f64 * 2.5), &[("1", net.as_str())]));
        }
    }
    for i in 0..50 {
        for j in 0..20 {
            let (x, y) = (i as f64 * 2.0 + 1.0, j as f64 * 5.0 + 0.6);
            items.push(track(pt(x - 0.3, y), pt(x + 0.3, y + 1.0), &format!("T{}", (i + j) % 200)));
        }
    }
    let (a, b, c, d) = (pt(60.5, 23.25), pt(61.5, 23.25), pt(61.5, 24.25), pt(60.5, 24.25));
    items.extend([track(a, b, "BOX"), track(b, c, "BOX"), track(c, d, "BOX"), track(d, a, "BOX")]);
    board(items, 0.8, 0.8)
}

#[test]
#[ignore = "performance benchmark; run with --release -- --ignored"]
fn interactive_latency() {
    let doc = dense_board();
    let t0 = std::time::Instant::now();
    let n = 60;
    let mut worst = std::time::Duration::ZERO;
    let mut reached = 0;
    for k in 0..n {
        let end = pt(2.0 * (5 + (k * 7) % 40) as f64 + 1.0, 5.0 * ((k * 3) % 8) as f64 + 3.75);
        let t = std::time::Instant::now();
        let r = route(&doc, &req("NEW", pt(1.0, 3.75), end, RouteMode::Walkaround));
        worst = worst.max(t.elapsed());
        reached += r.reached as usize;
    }
    let avg = t0.elapsed() / n;
    let t = std::time::Instant::now();
    let _ = World::build(&doc, TOP, &[]);
    let build = t.elapsed();
    println!("objects: {} items; index build {build:?}; walkaround avg {avg:?}, worst {worst:?}, reached {reached}/{n}", doc.len());
    let t = std::time::Instant::now();
    for k in 0..n {
        let y = 3.75 + (k % 5) as f64 * 0.05;
        let _ = route(&doc, &req("NEW", pt(1.0, y), pt(30.0, y + 0.0), RouteMode::Shove));
    }
    println!("shove avg {:?}", t.elapsed() / n);
    // Worst case: the end is enclosed, so the search exhausts its budget.
    let t = std::time::Instant::now();
    let r = route(&doc, &req("NEW", pt(1.0, 3.75), pt(61.0, 23.75), RouteMode::Walkaround));
    println!("enclosed end: {:?} (reached {})", t.elapsed(), r.reached);
    assert!(avg < std::time::Duration::from_millis(if cfg!(debug_assertions) { 400 } else { 20 }));
}

#[test]
fn fanout_through_other_net_pour_refills_and_undoes_atomically() {
    use crate::{Command, Session};
    use crate::document::Edit;
    use crate::pcb::{fill, drc};
    let zone: Item = serde_json::from_value(serde_json::json!({
        "kind":"zone", "outline":[{"x":-5000000,"y":-5000000},{"x":15000000,"y":-5000000},{"x":15000000,"y":5000000},{"x":-5000000,"y":5000000}],
        "layer":0,"net":"GND","removeIslands":false
    })).unwrap();
    let mut doc = board(vec![placed("U1",pt(0.0,0.0),&[("1","SIGNAL")]),zone],0.85,0.95);
    for (id,polygons) in fill::fill_all(&doc) {
        let before=doc.get(id).cloned();
        if let Some(Item::Zone(mut zone))=before.clone(){zone.fill=polygons;doc.apply(&[Edit{id,before,after:Some(Item::Zone(zone))}]);}
    }
    let original=doc.clone();
    let q=req("SIGNAL",pt(0.0,0.0),pt(8.0,0.0),RouteMode::Walkaround);
    let preview=route(&doc,&q);
    assert!(preview.reached,"{}",preview.message);
    assert_connected(&preview.tracks,q.start,Some(q.end));
    let mut session=Session::with_document(doc);
    let delta=session.execute(Command::Route{delete:vec![],replace:vec![],add:preview.tracks.into_iter().map(Item::Track).collect()}).unwrap();
    assert!(delta.upserted.iter().any(|entry|matches!(entry.item,Item::Zone(_))),"pour must change in the routing transaction");
    let errors:Vec<_>=drc::run(session.document()).violations.into_iter().filter(|v|matches!(v.code,"short"|"clearance")).collect();
    assert!(errors.is_empty(),"{errors:?}");
    let after=session.document().clone();
    session.undo().unwrap();assert_eq!(crate::io::document_to_string(session.document()).unwrap(),crate::io::document_to_string(&original).unwrap());
    session.redo().unwrap();assert_eq!(session.document(),&after);
}

#[test]
fn ignoring_pour_fills_does_not_ignore_other_net_pads_or_keepouts() {
    let keepout:Item=serde_json::from_value(serde_json::json!({
        "kind":"zone","outline":[{"x":2000000,"y":-2000000},{"x":4000000,"y":-2000000},{"x":4000000,"y":2000000},{"x":2000000,"y":2000000}],"layer":0,"keepout":true
    })).unwrap();
    let doc=board(vec![placed("U1",pt(0.0,0.0),&[("1","SIGNAL")]),placed("U2",pt(6.0,0.0),&[("1","OTHER")]),keepout],1.0,1.0);
    let q=req("SIGNAL",pt(0.0,0.0),pt(9.0,0.0),RouteMode::Walkaround);
    let r=route(&doc,&q);assert!(r.reached,"{}",r.message);
    assert_connected(&r.tracks,q.start,Some(q.end));
    assert!(min_gap(&doc,&r.tracks,"SIGNAL",1.0,1.0)>=mm(0.2)as f64-1.0);
    let world=World::build(&doc,TOP,&[]);let mut ids=World::build(&doc,TOP,&[]);let net=ids.net_id("SIGNAL");let excl=[net];
    let probe=Probe{hw:mm(0.25)as f64/2.0,net,excl:&excl};
    let path:Vec<_>=std::iter::once(q.start).chain(r.tracks.iter().map(|t|t.b)).collect();
    assert!(world.path_clear(&path,&probe),"keep-out clearance must remain enforced");
}

#[test]
fn failed_routing_commit_does_not_change_document_or_history() {
    use crate::{Command,Session};
    let doc=board(vec![],1.0,1.0);let original=doc.clone();let mut session=Session::with_document(doc);
    let invalid=Item::Track(Track{ locked: false,a:pt(0.0,0.0),b:pt(0.0,0.0),width:0,layer:TOP,net:"S".into()});
    assert!(session.execute(Command::Route{delete:vec![],replace:vec![],add:vec![invalid]}).is_err());
    assert_eq!(session.document(),&original);assert!(!session.can_undo());
}

/// 4-layer board with a Top→Inner 1 microvia and a Top→Inner 2 blind via defined.
fn hdi_board(items: Vec<Item>) -> Document {
    use crate::pcb::board::{Stackup, ViaType};
    let in1 = layer::inner_copper(1);
    let in2 = layer::inner_copper(2);
    let settings = BoardSettings {
        stackup: Stackup::with_copper(4),
        via_types: vec![
            ViaType { name: "µvia".into(), from_layer: TOP, to_layer: in1, diameter: mm(0.25), drill: mm(0.1) },
            ViaType { name: "Blind".into(), from_layer: TOP, to_layer: in2, diameter: mm(0.45), drill: mm(0.2) },
        ],
        ..BoardSettings::default()
    };
    let mut all = vec![Item::Board(settings)];
    all.extend(items);
    Document::from_item_list(DocKind::Pcb, all)
}

#[test]
fn via_kinds_follow_the_copper_span_and_drill() {
    use crate::pcb::board::{Stackup, ViaKind};
    let s = Stackup::with_copper(4);
    let (in1, in2) = (layer::inner_copper(1), layer::inner_copper(2));
    assert_eq!(s.via_kind(TOP, layer::BOTTOM_COPPER, mm(0.3)), ViaKind::Through);
    assert_eq!(s.via_kind(TOP, in1, mm(0.1)), ViaKind::Micro);
    assert_eq!(s.via_kind(TOP, in1, mm(0.3)), ViaKind::Blind);
    assert_eq!(s.via_kind(TOP, in2, mm(0.2)), ViaKind::Blind);
    assert_eq!(s.via_kind(in1, in2, mm(0.3)), ViaKind::Buried);
}

#[test]
fn layer_changes_use_the_smallest_matching_via_type() {
    let doc = hdi_board(vec![]);
    let in1 = layer::inner_copper(1);
    let micro = via_between(&doc, "A", pt(1.0, 1.0), TOP, in1);
    assert_eq!((micro.from_layer, micro.to_layer, micro.drill), (TOP, in1, mm(0.1)));
    let blind = via_between(&doc, "A", pt(1.0, 1.0), in1, layer::inner_copper(2));
    assert_eq!((blind.to_layer, blind.drill), (layer::inner_copper(2), mm(0.2)));
    // No type reaches Bottom: a through via sized by the rules.
    let through = via_between(&doc, "A", pt(1.0, 1.0), TOP, layer::BOTTOM_COPPER);
    assert_eq!((through.from_layer, through.to_layer, through.drill), (TOP, layer::BOTTOM_COPPER, mm(0.3)));
}

#[test]
fn drc_accepts_defined_microvias_but_flags_undefined_small_vias() {
    let in1 = layer::inner_copper(1);
    let ok = Via { locked: false, pos: pt(5.0, 5.0), diameter: mm(0.25), drill: mm(0.1), from_layer: TOP, to_layer: in1, net: "A".into() };
    let odd = Via { locked: false, pos: pt(9.0, 5.0), diameter: mm(0.25), drill: mm(0.1), from_layer: TOP, to_layer: layer::BOTTOM_COPPER, net: "A".into() };
    let doc = hdi_board(vec![Item::Via(ok), Item::Via(odd)]);
    let report = crate::pcb::drc::run(&doc);
    let flagged: Vec<_> = report.violations.iter().filter(|v| v.code == "via-style" || v.code == "hole-size").flat_map(|v| v.items.clone()).collect();
    let ids: Vec<_> = doc.items().filter(|(_, i)| matches!(i, Item::Via(_))).map(|(id, _)| id).collect();
    assert!(!flagged.contains(&ids[0]), "the defined microvia is an approved style");
    assert!(flagged.contains(&ids[1]), "a small through via still violates the via-style rule");
}

#[test]
fn tuning_rejects_locked_tracks_without_mutating_board() {
    let mut locked=track(pt(0.0,0.0),pt(20.0,0.0),"A");
    if let Item::Track(t)=&mut locked {t.locked=true;}
    let doc=board(vec![locked],1.0,1.0);
    let before=crate::io::document_to_string(&doc).unwrap();
    let id=doc.items().find(|(_,i)|matches!(i,Item::Track(_))).unwrap().0;
    let err=tune_length(&doc,&TuneRequest{tracks:vec![id],target_length:mm(24.0),amplitude:mm(1.0),spacing:mm(0.6),style:TuneStyle::Accordion}).unwrap_err();
    assert!(err.contains("locked")); assert_eq!(crate::io::document_to_string(&doc).unwrap(),before);
}

#[test]
fn tuning_checks_the_actual_height_instead_of_only_maximum_amplitude() {
    let doc=board(vec![track(pt(0.,0.),pt(20.,0.),"A"),placed("U1",pt(2.65,0.45),&[("1","B")])],0.04,0.04);
    let id=doc.items().find(|(_,i)|matches!(i,Item::Track(t)if t.net=="A")).unwrap().0;
    let out=tune_length(&doc,&TuneRequest{tracks:vec![id],target_length:mm(20.4),amplitude:mm(1.),spacing:mm(1.6),style:TuneStyle::Accordion}).unwrap();
    assert!(min_gap(&doc,&out,"A",0.04,0.04)>=mm(0.2) as f64-2.,"final meander collides after shrinking its height");
}

#[test]
fn tuning_retains_a_via_touching_the_middle_of_a_selected_track() {
    let at=pt(1.05,0.);
    let doc=board(vec![track(pt(0.,0.),pt(20.,0.),"A"),Item::Via(Via{locked:false,pos:at,diameter:mm(0.3),drill:mm(0.15),from_layer:TOP,to_layer:layer::BOTTOM_COPPER,net:"A".into()})],1.,1.);
    let id=doc.items().find(|(_,i)|matches!(i,Item::Track(_))).unwrap().0;
    let out=tune_length(&doc,&TuneRequest{tracks:vec![id],target_length:mm(21.4),amplitude:mm(1.),spacing:mm(0.6),style:TuneStyle::Accordion}).unwrap();
    assert!(out.iter().any(|t|super::geom::point_seg_dist(V::of(at),V::of(t.a),V::of(t.b)) <= (t.width as f64+mm(0.3) as f64)/2.),"interior via became disconnected");
}

#[test]
fn tuning_uses_smaller_legal_heights_when_the_maximum_does_not_fit() {
    let doc=board(vec![track(pt(0.,0.),pt(20.,0.),"A"),track(pt(0.,0.85),pt(20.,0.85),"B"),track(pt(0.,-0.85),pt(20.,-0.85),"C")],1.,1.);
    let id=doc.items().find(|(_,i)|matches!(i,Item::Track(t)if t.net=="A")).unwrap().0;
    let out=tune_length(&doc,&TuneRequest{tracks:vec![id],target_length:mm(21.),amplitude:mm(2.),spacing:mm(0.6),style:TuneStyle::Trombone}).unwrap();
    assert!(min_gap(&doc,&out,"A",1.,1.)>=mm(0.2) as f64-2.);
    assert!(out.iter().all(|t|t.a.y.abs()<=mm(0.4)&&t.b.y.abs()<=mm(0.4)));
    let total:f64=out.iter().map(|t|(V::of(t.b)-V::of(t.a)).len()).sum();assert!((total-mm(21.) as f64).abs()<=mm(21.) as f64*0.005);
}

#[test]
fn tuning_keeps_fixed_branch_junctions_and_recovers_in_one_step() {
    use crate::Session;
    let at=pt(1.05,0.);let doc=board(vec![track(pt(0.,0.),pt(20.,0.),"A"),track(at,pt(1.05,-2.),"A")],1.,1.);
    let id=doc.items().find(|(_,i)|matches!(i,Item::Track(t)if t.b==pt(20.,0.))).unwrap().0;
    let request=TuneRequest{tracks:vec![id],target_length:mm(21.4),amplitude:mm(1.),spacing:mm(0.6),style:TuneStyle::Accordion};
    let plan=tune_length(&doc,&request).unwrap();let verified=tune_length_checked(&doc,&request,Some(&plan)).unwrap();
    assert!(verified.iter().any(|t|super::geom::point_seg_dist(V::of(at),V::of(t.a),V::of(t.b))<1.));
    let command=tune_length_command(&doc,&request,Some(&verified)).unwrap();
    let mut session=Session::with_document(doc.clone());session.execute(command).unwrap();
    let after=crate::io::document_to_string(session.document()).unwrap();session.undo().unwrap();assert_eq!(crate::io::document_to_string(session.document()).unwrap(),crate::io::document_to_string(&doc).unwrap());assert!(!session.can_undo());
    session.redo().unwrap();assert_eq!(crate::io::document_to_string(session.document()).unwrap(),after);
    assert_eq!(crate::io::document_to_string(&crate::io::document_from_str(DocKind::Pcb,&after).unwrap()).unwrap(),after);
}

#[test]
fn tuning_rejects_duplicate_degenerate_and_too_tightly_spaced_input() {
    let doc=board(vec![track(pt(0.,0.),pt(20.,0.),"A")],1.,1.);let id=ItemId(1);
    let mut request=TuneRequest{tracks:vec![id,id],target_length:mm(22.),amplitude:mm(1.),spacing:mm(0.6),style:TuneStyle::Accordion};
    assert!(tune_length(&doc,&request).unwrap_err().contains("once"));
    request.tracks=vec![id];request.spacing=mm(0.1);assert!(tune_length(&doc,&request).unwrap_err().contains("spacing"));
    let degenerate=board(vec![track(pt(0.,0.),pt(0.,0.),"A")],1.,1.);request.spacing=mm(0.6);
    assert!(tune_length(&degenerate,&request).unwrap_err().contains("degenerate"));
}

#[test]
fn tuning_apply_rejects_a_changed_reviewed_shape_before_any_document_edit() {
    use crate::{Command,Session};
    let doc=board(vec![track(pt(0.,0.),pt(20.,0.),"A")],1.,1.);
    let request=TuneRequest{tracks:vec![ItemId(1)],target_length:mm(21.),amplitude:mm(1.),spacing:mm(0.6),style:TuneStyle::Accordion};
    let mut reviewed=tune_length(&doc,&request).unwrap();reviewed[0].b.x+=1;
    let mut session=Session::with_document(doc);session.mark_saved();let before=session.document().clone();
    assert!(tune_length_checked(session.document(),&request,Some(&reviewed)).unwrap_err().contains("preview"));
    assert_eq!(session.document(),&before);assert!(!session.can_undo()&&!session.is_dirty());
    let actual=tune_length(session.document(),&request).unwrap();
    session.execute(Command::Add{items:vec![track(pt(0.,0.6),pt(20.,0.6),"B")]}).unwrap();let changed=session.document().clone();
    assert!(tune_length_checked(session.document(),&request,Some(&actual)).is_err());assert_eq!(session.document(),&changed);
}

#[test]
fn differential_pair_continuation_retains_both_endpoints_through_an_outer_turn() {
    let doc=board(vec![],1.,1.);let gap=mm(0.2);
    let mut request=req("D_P",pt(0.,0.),pt(10.,0.),RouteMode::Walkaround);request.width=mm(0.2);
    let (first,other)=route_diff_pair(&doc,&request,"D_N",gap);assert!(first.reached&&other.reached);
    let anchor=other.tracks.last().unwrap().b;request.start=pt(10.,0.);request.end=pt(15.,5.);
    let (next,partner)=route_diff_pair_from(&doc,&request,"D_N",gap,Some(anchor));assert!(next.reached&&partner.reached,"{}",next.message);
    assert_connected(&next.tracks,request.start,Some(request.end));assert_connected(&partner.tracks,anchor,None);
    for p in &next.tracks {for n in &partner.tracks{assert!(seg_seg_dist(V::of(p.a),V::of(p.b),V::of(n.a),V::of(n.b))+4.>=(request.width+gap) as f64);}}
}

#[test]
fn invalid_pair_dimensions_or_shorting_partner_anchor_return_no_committable_geometry() {
    let doc=board(vec![],1.,1.);let request=req("D_P",pt(0.,0.),pt(10.,0.),RouteMode::Walkaround);
    for (other,gap) in [("D_P",mm(0.2)),("D_N",0),("D_N",-1)] {let (p,n)=route_diff_pair(&doc,&request,other,gap);assert!(!p.reached&&!n.reached);assert!(p.tracks.is_empty()&&n.tracks.is_empty());}
    let (p,n)=route_diff_pair_from(&doc,&request,"D_N",mm(0.2),Some(request.start));assert!(!p.reached&&!n.reached);assert!(p.tracks.is_empty()&&n.tracks.is_empty());assert!(p.moved.is_empty()&&n.moved.is_empty());
}

#[test]
fn tuning_an_already_in_range_chain_keeps_ids_dirty_state_and_undo_history() {
    use crate::Session;
    let doc=board(vec![track(pt(20.,0.),pt(0.,0.),"A")],1.,1.);
    let request=TuneRequest{tracks:vec![ItemId(1)],target_length:mm(20.05),amplitude:mm(1.),spacing:mm(0.6),style:TuneStyle::Accordion};
    let preview=tune_length(&doc,&request).unwrap();let command=tune_length_command(&doc,&request,Some(&preview)).unwrap();
    let mut session=Session::with_document(doc.clone());session.mark_saved();session.execute(command).unwrap();
    assert_eq!(session.document(),&doc);assert!(!session.is_dirty()&&!session.can_undo());
}

#[test]
fn tuning_refills_dependent_pours_and_recovers_in_one_history_step() {
    use crate::{Session,document::Edit,pcb::{fill,drc}};
    let zone:Item=serde_json::from_value(serde_json::json!({
        "kind":"zone","outline":[{"x":-5000000,"y":-5000000},{"x":25000000,"y":-5000000},{"x":25000000,"y":5000000},{"x":-5000000,"y":5000000}],
        "layer":0,"net":"GND","removeIslands":false
    })).unwrap();
    let mut doc=board(vec![track(pt(0.,0.),pt(20.,0.),"SIGNAL"),zone],1.,1.);
    for (id,polygons) in fill::fill_all(&doc) {
        let before=doc.get(id).cloned();
        if let Some(Item::Zone(mut zone))=before.clone(){zone.fill=polygons;doc.apply(&[Edit{id,before,after:Some(Item::Zone(zone))}]);}
    }
    let request=TuneRequest{tracks:vec![ItemId(1)],target_length:mm(21.4),amplitude:mm(1.),spacing:mm(0.6),style:TuneStyle::Accordion};
    let preview=tune_length(&doc,&request).unwrap();
    let command=tune_length_command(&doc,&request,Some(&preview)).unwrap();
    let original=crate::io::document_to_string(&doc).unwrap();let mut session=Session::with_document(doc);
    let delta=session.execute(command).unwrap();
    assert!(delta.upserted.iter().any(|i|matches!(i.item,Item::Zone(_))),"tuning must refill the dependent pour in its transaction");
    let errors:Vec<_>=drc::run(session.document()).violations.into_iter().filter(|v|matches!(v.code,"short"|"clearance")).collect();
    assert!(errors.is_empty(),"{errors:?}");
    let after=crate::io::document_to_string(session.document()).unwrap();
    session.undo().unwrap();assert_eq!(crate::io::document_to_string(session.document()).unwrap(),original);assert!(!session.can_undo());
    session.redo().unwrap();assert_eq!(crate::io::document_to_string(session.document()).unwrap(),after);
    let reopened=crate::io::document_from_str(DocKind::Pcb,&after).unwrap();assert_eq!(crate::io::document_to_string(&reopened).unwrap(),after);
}

#[test]
fn pair_escape_uses_real_pad_centers_and_respects_adjacent_ground() {
    for rotation in [0.0,90.0] {
        let mut part=placed("U1",pt(5.,5.),&[("1","D_P"),("2","D_N"),("3","GND")]);
        if let Item::Footprint(fp)=&mut part {fp.rotation=rotation;}
        let mut doc=board(vec![part],0.3,0.3);
        let mut footprint=Footprint::new("P");
        footprint.items=vec![pad("1",0.,0.,0.3,0.3),pad("2",0.,1.0,0.3,0.3),pad("3",0.,-0.8,0.3,0.3)];
        doc.put_cached_footprint("L/P",footprint);
        let pads=crate::pcb::geometry::all_pads(&doc,world::board_of(&doc));
        let positive=pads.iter().find(|p|p.net=="D_P").unwrap().pos;
        let negative=pads.iter().find(|p|p.net=="D_N").unwrap().pos;
        let end=if rotation==0.0 {pt(15.,5.)}else{pt(5.,15.)};
        let mut request=req("D_P",positive,end,RouteMode::Walkaround);request.width=mm(0.2);
        let (p,n)=route_diff_pair(&doc,&request,"D_N",mm(0.2));
        assert!(p.reached&&n.reached,"rotation {rotation}: {}",p.message);
        assert_connected(&p.tracks,positive,Some(end));assert_connected(&n.tracks,negative,None);
        let mut session=crate::Session::with_document(doc);
        let diff=session.execute(crate::Command::Add{items:p.tracks.into_iter().chain(n.tracks).map(Item::Track).collect()}).unwrap();
        assert!(crate::pcb::drc::run_near(session.document(),&diff.upserted.iter().map(|i|i.id).collect::<Vec<_>>()).iter().all(|v|!matches!(v.code,"short"|"clearance")),"pair neck must clear opposite pads and ground");
    }
}

#[test]
fn pair_escape_does_not_guess_partner_on_another_component() {
    let doc=board(vec![placed("U1",pt(0.,0.),&[("1","D_P")]),placed("U2",pt(0.,10.),&[("1","D_N")])],0.3,0.3);
    let request=req("D_P",pt(0.,0.),pt(10.,0.),RouteMode::Walkaround);
    let (_,n)=route_diff_pair(&doc,&request,"D_N",mm(0.2));
    assert!(n.tracks.first().is_none_or(|t|t.a!=pt(0.,10.)),"unrelated component must not become the source pad");
}

#[test]
fn pair_continues_from_widened_vias_on_the_destination_layer() {
    let mut doc=board(vec![],0.3,0.3);
    let p=pt(12.65001,10.200005);let n=pt(12.65001,9.349995);
    let pv=via_for(&doc,"D_P",p);let nv=via_for(&doc,"D_N",n);
    let mut setup=crate::Session::with_document(doc);
    setup.execute(crate::Command::Add{items:vec![Item::Via(pv),Item::Via(nv)]}).unwrap();doc=setup.document().clone();
    let mut request=req("D_P",p,pt(20.,10.200005),RouteMode::Walkaround);request.layer=layer::BOTTOM_COPPER;request.width=mm(0.2);
    let (positive,negative)=route_diff_pair_from(&doc,&request,"D_N",mm(0.25),Some(n));
    assert!(positive.reached&&negative.reached,"{}",positive.message);
    assert_connected(&positive.tracks,p,Some(request.end));assert_connected(&negative.tracks,n,None);
    assert!(positive.tracks.iter().chain(&negative.tracks).all(|t|t.layer==layer::BOTTOM_COPPER));
    let diff=setup.execute(crate::Command::Add{items:positive.tracks.into_iter().chain(negative.tracks).map(Item::Track).collect()}).unwrap();
    assert!(crate::pcb::drc::run_near(setup.document(),&diff.upserted.iter().map(|i|i.id).collect::<Vec<_>>()).iter().all(|v|!matches!(v.code,"short"|"clearance")));
}

#[test]
fn pair_resume_uses_existing_partner_endpoint_instead_of_a_virtual_offset() {
    let doc=board(vec![track(pt(0.,0.),pt(5.,0.),"D_P"),track(pt(0.,1.),pt(5.,1.),"D_N")],0.3,0.3);
    let mut request=req("D_P",pt(5.,0.),pt(15.,0.),RouteMode::Walkaround);request.width=mm(0.2);
    let (p,n)=route_diff_pair(&doc,&request,"D_N",mm(0.25));
    assert!(p.reached&&n.reached,"{}",p.message);
    assert_connected(&p.tracks,request.start,Some(request.end));assert_connected(&n.tracks,pt(5.,1.),None);
}

#[test]
fn dense_pair_resumes_on_its_own_copper_and_trims_both_virtual_corner_ends() {
    for rotation in [0.0_f64,45.0,90.0,180.0] {
        let point=|x:f64,y:f64|{let r=rotation.to_radians();pt(x*r.cos()-y*r.sin(),x*r.sin()+y*r.cos())};
        let items=[((0.,0.),(10.,0.),"D_P"),((-2.,-2.7),(0.2,-0.5),"D_N"),((0.2,-0.5),(9.8,-0.5),"D_N"),((9.8,-0.5),(12.,-2.7),"D_N")].into_iter().map(|(a,b,net)|{
            let mut item=track(point(a.0,a.1),point(b.0,b.1),net);if let Item::Track(t)=&mut item{t.width=mm(0.2);}item
        }).collect();
        let doc=board(items,0.3,0.3);let before=crate::io::document_to_string(&doc).unwrap();
        let mut request=req("D_P",point(0.,0.),point(10.,0.),RouteMode::Walkaround);request.width=mm(0.2);
        let (p,n)=route_diff_pair(&doc,&request,"D_N",mm(0.3));
        assert!(p.reached&&n.reached,"rotation {rotation}: {}",p.message);
        assert_connected(&p.tracks,request.start,Some(request.end));assert_connected(&n.tracks,point(0.2,-0.5),Some(point(9.8,-0.5)));
        assert!(p.moved.is_empty()&&n.moved.is_empty());assert_eq!(crate::io::document_to_string(&doc).unwrap(),before);
        let mut session=crate::Session::with_document(doc);
        let delta=session.execute(crate::Command::Add{items:p.tracks.into_iter().chain(n.tracks).map(Item::Track).collect()}).unwrap();
        assert!(crate::pcb::drc::run_near(session.document(),&delta.upserted.iter().map(|i|i.id).collect::<Vec<_>>()).iter().all(|v|!matches!(v.code,"short"|"clearance")));
        session.undo().unwrap();assert_eq!(crate::io::document_to_string(session.document()).unwrap(),before);
    }
}

#[test]
fn own_copper_exclusion_still_checks_foreign_pads_and_the_other_pair_net() {
    let doc=board(vec![track(pt(0.,0.),pt(10.,0.),"D_P"),track(pt(0.,-0.2),pt(10.,-0.2),"D_N"),placed("U1",pt(0.,0.),&[("1","GND")])],1.,1.);
    let mut request=req("D_P",pt(0.,0.),pt(10.,0.),RouteMode::Walkaround);request.width=mm(0.2);
    let (p,n)=route_diff_pair_from(&doc,&request,"D_N",mm(0.3),Some(pt(0.,-0.5)));
    assert!(!p.reached&&!n.reached);assert!(p.tracks.is_empty()&&n.tracks.is_empty());
}

#[test]
fn dense_pair_spacing_mismatch_reports_dimensions_instead_of_backtracking() {
    let doc=board(vec![track(pt(0.,0.),pt(8.,0.),"D_P"),track(pt(0.,-0.3),pt(8.,-0.3),"D_N")],0.3,0.3);
    let mut request=req("D_P",pt(0.,0.),pt(8.,0.),RouteMode::Walkaround);request.width=mm(0.2);
    let (p,n)=route_diff_pair(&doc,&request,"D_N",mm(0.3));
    assert!(!p.reached&&!n.reached);assert!(p.tracks.is_empty()&&n.tracks.is_empty());assert!(p.message.contains("0.3000 mm apart; 0.5000 mm required"),"{}",p.message);
}

#[test]
fn tight_pad_pitch_can_fan_out_to_pair_gap_without_relaxing_electrical_clearance() {
    let mut doc=board(vec![placed("J1",pt(5.,5.),&[("1","P"),("2","N")])],0.2,0.2);
    let mut geometry=Footprint::new("P");geometry.items=vec![pad("1",0.,0.,0.2,0.2),pad("2",0.,0.4,0.2,0.2)];doc.put_cached_footprint("L/P",geometry);
    let mut request=req("P",pt(5.,5.),pt(10.,5.),RouteMode::Walkaround);request.width=mm(0.178);
    let (p,n)=route_diff_pair(&doc,&request,"N",mm(0.25));
    assert!(p.reached&&n.reached,"{}",p.message);assert_connected(&p.tracks,request.start,Some(request.end));assert_connected(&n.tracks,pt(5.,5.4),None);
    assert!((n.tracks.last().unwrap().b.y-p.tracks.last().unwrap().b.y).abs()>=mm(0.428)-4);
    let mut session=crate::Session::with_document(doc);let delta=session.execute(crate::Command::Add{items:p.tracks.into_iter().chain(n.tracks).map(Item::Track).collect()}).unwrap();
    assert!(crate::pcb::drc::run_near(session.document(),&delta.upserted.iter().map(|i|i.id).collect::<Vec<_>>()).iter().all(|v|!matches!(v.code,"short"|"clearance")));
}

#[test]
fn pad_launches_keep_centers_and_follow_rotated_long_axes() {
    for (rotation,end) in [(0.0,pt(4.,8.)),(90.0,pt(-8.,4.))] {
        let mut part=placed("U1",pt(0.,0.),&[("1","A")]);
        if let Item::Footprint(fp)=&mut part{fp.rotation=rotation;}
        let doc=board(vec![part],4.,1.);
        let result=route(&doc,&req("A",pt(0.,0.),end,RouteMode::Walkaround));
        assert!(result.reached,"{}",result.message);assert_connected(&result.tracks,pt(0.,0.),Some(end));assert_octilinear(&result.tracks);
        let first=&result.tracks[0];
        if rotation==0.0{assert_eq!(first.b.y,0);assert!(first.b.x>mm(2.));}
        else{assert_eq!(first.b.x,0);assert!(first.b.y>mm(2.));}
    }
}

#[test]
fn long_pad_empty_corners_are_not_fixed_electrical_anchors() {
    let doc=board(vec![placed("U1",pt(0.,0.),&[("1","A")])],4.,0.4);
    let topology=crate::pcb::elastic::Topology::new(&doc);
    let Item::Track(t)=track(pt(0.,0.),pt(5.,0.),"A") else{unreachable!()};
    assert!(!topology.anchor(pt(0.,1.),&t,&Default::default()));
    assert!(topology.anchor(pt(1.9,0.),&t,&Default::default()));
}

#[test]
fn shove_uses_the_clear_side_and_reverses_without_accumulating_changes() {
    let doc=board(vec![track(pt(-10.,0.),pt(10.,0.),"B"),placed("U1",pt(0.,-0.8),&[("1","C")])],2.,0.6);
    let before=crate::io::document_to_string(&doc).unwrap();
    let request=req("A",pt(-2.,0.15),pt(2.,0.15),RouteMode::Shove);
    let first=route(&doc,&request);assert!(first.reached,"{}",first.message);assert!(!first.moved.is_empty());
    assert!(first.moved.iter().any(|m|matches!(&m.item,Item::Track(t)if t.a.y>mm(0.4)||t.b.y>mm(0.4))));
    let away=route(&doc,&req("A",pt(-2.,2.),pt(2.,2.),RouteMode::Shove));assert!(away.reached&&away.moved.is_empty());
    let restored=route(&doc,&request);assert_eq!(format!("{:?}",first.moved),format!("{:?}",restored.moved));
    assert_eq!(crate::io::document_to_string(&doc).unwrap(),before);
}

fn transition_request(doc:&Document)->PairViaRequest {
    let positive=via_between(doc,"P",pt(10.,10.),TOP,layer::BOTTOM_COPPER);
    let negative=via_between(doc,"N",pt(10.,9.55),TOP,layer::BOTTOM_COPPER);
    PairViaRequest{positive,negative,source:TOP,target:layer::BOTTOM_COPPER,direction:pt(1.,0.),width:mm(0.2),corner:CornerStyle::Diagonal45,mode:RouteMode::Walkaround,
        tracks:vec![Track{a:pt(5.,10.),b:pt(10.,10.),width:mm(0.2),layer:TOP,net:"P".into(),locked:false},Track{a:pt(5.,9.55),b:pt(10.,9.55),width:mm(0.2),layer:TOP,net:"N".into(),locked:false}],moved:vec![]}
}

#[test]
fn paired_transition_finds_an_alternate_and_keeps_both_necks_connected() {
    let mut obstacle=placed("U1",pt(10.7,9.8),&[("1","BLOCK")]);
    if let Item::Footprint(fp)=&mut obstacle{fp.side=Side::Bottom;}
    let doc=board(vec![obstacle],0.6,2.);
    let before=crate::io::document_to_string(&doc).unwrap();let request=transition_request(&doc);
    let plan=pair_via_plan(&doc,&request).unwrap();assert_eq!(plan.vias.len(),2);
    for via in &plan.vias{assert!(plan.tracks.iter().any(|t|t.net==via.net&&t.b==via.pos));}
    let mut session=crate::Session::with_document(doc);
    session.execute_with_via_checks(crate::Command::Route{delete:vec![],replace:vec![],add:plan.tracks.into_iter().map(Item::Track).chain(plan.vias.into_iter().map(Item::Via)).collect()},true).unwrap();
    let after=crate::io::document_to_string(session.document()).unwrap();session.undo().unwrap();assert_eq!(crate::io::document_to_string(session.document()).unwrap(),before);session.redo().unwrap();assert_eq!(crate::io::document_to_string(session.document()).unwrap(),after);
}

#[test]
fn paired_transition_rejects_foreign_copper_on_every_crossed_layer() {
    for blocked in crate::pcb::board::Stackup::with_copper(6).copper_layers(){
        let mut blocker=track(pt(0.,10.),pt(30.,10.),"BLOCK");if let Item::Track(t)=&mut blocker{t.layer=blocked;t.width=mm(10.);}
        let mut doc=board(vec![blocker],1.,1.);let board_id=doc.items().find_map(|(id,item)|matches!(item,Item::Board(_)).then_some(id)).unwrap();
        let mut settings=world::board_of(&doc).clone();settings.stackup=crate::pcb::board::Stackup::with_copper(6);
        doc.apply(&[crate::document::Edit{id:board_id,before:doc.get(board_id).cloned(),after:Some(Item::Board(settings))}]);
        let mut request=transition_request(&doc);request.source=layer::inner_copper(1);request.target=layer::inner_copper(4);for t in &mut request.tracks{t.layer=request.source;}
        let before=crate::io::document_to_string(&doc).unwrap();let message=pair_via_plan(&doc,&request).unwrap_err();
        let name=world::board_of(&doc).stackup.layers.iter().find(|l|l.layer==Some(blocked)).unwrap().name.as_str();
        assert!(message.contains(name)&&message.contains("BLOCK"),"crossed layer {}: {message}",blocked.0);assert_eq!(crate::io::document_to_string(&doc).unwrap(),before);
    }
}

#[test]
fn connected_track_edit_pushes_multiple_tracks_in_one_undo_step() {
    let doc=board(vec![track(pt(5.,-2.),pt(15.,-2.),"A"),track(pt(0.,0.4),pt(20.,0.4),"B"),track(pt(0.,0.85),pt(20.,0.85),"C")],1.,1.);
    let (id,item)=doc.items().find(|(_,i)|matches!(i,Item::Track(t)if t.net=="A")).unwrap();let Item::Track(mut next)=item.clone() else{unreachable!()};next.a.y=0;next.b.y=0;
    let before=crate::io::document_to_string(&doc).unwrap();
    let result=plan_track_edit(&doc,vec![IdItem{id,item:Item::Track(next.clone())}],vec![],RouteMode::Shove).unwrap();
    assert_eq!(result.rerouted,2);assert_eq!(result.delete.len(),2);assert_eq!(result.replace[0].id,id);
    assert_eq!(crate::io::document_to_string(&doc).unwrap(),before);
    let mut session=crate::Session::with_document(doc);session.execute(crate::Command::Route{delete:result.delete,replace:result.replace,add:result.add}).unwrap();
    assert!(!super::super::drc::run(session.document()).violations.iter().any(|v|v.severity==super::super::drc::Severity::Error));
    let after=crate::io::document_to_string(session.document()).unwrap();session.undo().unwrap();assert_eq!(crate::io::document_to_string(session.document()).unwrap(),before);session.redo().unwrap();assert_eq!(crate::io::document_to_string(session.document()).unwrap(),after);
    assert_eq!(crate::io::document_to_string(&crate::io::document_from_str(DocKind::Pcb,&after).unwrap()).unwrap(),after);
}

#[test]
fn connected_track_edit_rejects_locked_obstacles_and_avoid_collisions_without_edits() {
    for locked in [false,true] {
        let mut other=track(pt(0.,0.4),pt(20.,0.4),"B");if let Item::Track(t)=&mut other{t.locked=locked;}
        let doc=board(vec![track(pt(5.,-2.),pt(15.,-2.),"A"),other],1.,1.);let before=crate::io::document_to_string(&doc).unwrap();
        let (id,item)=doc.items().find(|(_,i)|matches!(i,Item::Track(t)if t.net=="A")).unwrap();let Item::Track(mut next)=item.clone() else{unreachable!()};next.a.y=0;next.b.y=0;
        assert!(plan_track_edit(&doc,vec![IdItem{id,item:Item::Track(next.clone())}],vec![],if locked{RouteMode::Shove}else{RouteMode::Walkaround}).is_err());
        next.net="CHANGED".into();assert!(plan_track_edit(&doc,vec![IdItem{id,item:Item::Track(next)}],vec![],RouteMode::Ignore).is_err());
        assert_eq!(crate::io::document_to_string(&doc).unwrap(),before);
    }
}

#[test]
fn connected_track_edit_requires_a_fixed_pad_join() {
    let doc=board(vec![placed("J1",pt(5.,0.),&[("1","A")]),track(pt(5.,0.),pt(12.,0.),"A")],0.5,0.5);
    let (id,item)=doc.items().find(|(_,i)|matches!(i,Item::Track(_))).unwrap();let Item::Track(mut next)=item.clone() else{unreachable!()};next.a.y=mm(2.);next.b.y=mm(2.);
    assert!(plan_track_edit(&doc,vec![IdItem{id,item:Item::Track(next.clone())}],vec![],RouteMode::Shove).unwrap_err().contains("detach"));
    let join=Item::Track(Track{a:pt(5.,0.),b:next.a,..next.clone()});
    let result=plan_track_edit(&doc,vec![IdItem{id,item:Item::Track(next)}],vec![join],RouteMode::Shove).unwrap();assert!(result.delete.is_empty());assert_eq!(result.add.len(),1);
}

#[test]
fn multi_layer_track_edit_shares_local_shove_work_and_fails_without_partial_edits() {
    let mut items=Vec::new();
    for layer in [TOP,layer::BOTTOM_COPPER] {
        let mut root=track(pt(-1.,2.),pt(1.,2.),"ROOT");
        let mut neighbor=track(pt(-10.,0.),pt(10.,0.),"OTHER");
        if let Item::Track(t)=&mut root {t.layer=layer;}
        if let Item::Track(t)=&mut neighbor {t.layer=layer;}
        items.extend([root,neighbor]);
    }
    let source=board(items,0.6,0.3);let before=crate::io::document_to_string(&source).unwrap();
    let replace:Vec<_>=source.items().filter_map(|(id,item)|match item{
        Item::Track(t)if t.net=="ROOT"=>{let mut moved=t.clone();moved.a.y=mm(0.2);moved.b.y=mm(0.2);Some(IdItem{id,item:Item::Track(moved)})},_=>None
    }).collect();
    {
        let _work=WorkScope::enter();let _slice=WorkSlice::enter(12);
        let error=plan_track_edit(&source,replace.clone(),vec![],RouteMode::Shove).unwrap_err();
        assert!(error.contains("search limit"),"{error}");
        assert!(engine::work_exhausted());
        assert_eq!(crate::io::document_to_string(&source).unwrap(),before);
    }
    let plan=plan_track_edit(&source,replace,vec![],RouteMode::Shove).unwrap();
    assert_eq!(plan.rerouted,2);
    let mut session=crate::Session::with_document(source.clone());
    let delta=session.execute_with_via_checks(crate::Command::Route{replace:plan.replace,delete:plan.delete,add:plan.add},true).unwrap();
    let touched:Vec<_>=delta.upserted.iter().map(|r|r.id).collect();
    assert!(!crate::pcb::drc::run_near(session.document(),&touched).iter().any(|v|v.severity==crate::pcb::drc::Severity::Error));
    let after=crate::io::document_to_string(session.document()).unwrap();
    session.undo().unwrap();assert_eq!(crate::io::document_to_string(session.document()).unwrap(),before);
    session.redo().unwrap();assert_eq!(crate::io::document_to_string(session.document()).unwrap(),after);
    assert_eq!(crate::io::document_to_string(&crate::io::document_from_str(DocKind::Pcb,&after).unwrap()).unwrap(),after);
}

#[test]
fn shove_reshapes_a_clear_section_without_moving_its_interior_branch_contact() {
    for heading in 0..3 {for turns in 0..4 {for reverse in [false,true] {
        let rotate=|p:Point|{let mut p=match heading{0=>p,1=>Point::new(p.x-p.y,p.x+p.y),_=>Point::new(2*p.x-p.y,p.x+2*p.y)};for _ in 0..turns {p=Point::new(-p.y,p.x);}p};
        let mut items=vec![track(pt(-10.,0.),pt(20.,0.),"BRANCH"),track(pt(0.,0.),pt(0.,-3.),"BRANCH"),track(pt(14.,0.),pt(14.,3.),"BRANCH"),track(pt(7.,2.),pt(9.,2.),"ROOT"),
            Item::Via(Via{pos:pt(0.,0.),diameter:mm(0.6),drill:mm(0.3),from_layer:TOP,to_layer:layer::BOTTOM_COPPER,net:"BRANCH".into(),locked:false})];
        for item in &mut items {match item{
            Item::Track(t)=>{t.a=rotate(t.a);t.b=rotate(t.b);if reverse {std::mem::swap(&mut t.a,&mut t.b);}},
            Item::Via(v)=>v.pos=rotate(v.pos),_=>{}
        }}
        let source=board(items,0.6,0.3);let before=crate::io::document_to_string(&source).unwrap();
        let (id,item)=source.items().find(|(_,i)|matches!(i,Item::Track(t)if t.net=="ROOT")).unwrap();
        let Item::Track(mut moved)=item.clone()else{unreachable!()};let shift=rotate(pt(0.,-1.8));
        moved.a=Point::new(moved.a.x+shift.x,moved.a.y+shift.y);moved.b=Point::new(moved.b.x+shift.x,moved.b.y+shift.y);
        let plan=plan_track_edit(&source,vec![IdItem{id,item:Item::Track(moved)}],vec![],RouteMode::Shove).expect("the junction is far outside the affected section");
        assert_eq!(plan.rerouted,1);
        let mut session=crate::Session::with_document(source.clone());
        let delta=session.execute_with_via_checks(crate::Command::Route{replace:plan.replace,delete:plan.delete,add:plan.add},true).unwrap();
        let touched:Vec<_>=delta.upserted.iter().map(|r|r.id).collect();
        assert!(!crate::pcb::drc::run_near(session.document(),&touched).iter().any(|v|v.severity==crate::pcb::drc::Severity::Error));
        for(id,item)in source.items().filter(|(_,item)|matches!(item,Item::Via(_))||matches!(item,Item::Track(t)if t.net=="BRANCH"&&[rotate(pt(0.,-3.)),rotate(pt(14.,3.))].iter().any(|p|t.a==*p||t.b==*p))) {assert_eq!(session.document().get(id),Some(item));}
        let fixed=rotate(pt(0.,0.));
        assert!(session.document().items().any(|(_,item)|matches!(item,Item::Track(t)if t.net=="BRANCH"&&(t.a==fixed||t.b==fixed))));
        assert_eq!(crate::pcb::connectivity::ratsnest(session.document()),crate::pcb::connectivity::ratsnest(&source));
        let after=crate::io::document_to_string(session.document()).unwrap();
        session.undo().unwrap();assert_eq!(crate::io::document_to_string(session.document()).unwrap(),before);
        session.redo().unwrap();assert_eq!(crate::io::document_to_string(session.document()).unwrap(),after);
        assert_eq!(crate::io::document_to_string(&crate::io::document_from_str(DocKind::Pcb,&after).unwrap()).unwrap(),after);
        assert_eq!(crate::io::document_to_string(&source).unwrap(),before);
    }}}
}
