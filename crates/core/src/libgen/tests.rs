use std::collections::HashSet;

use super::wizard::*;
use super::*;
use crate::item::{FillStyle, Item, Pad, PadKind, PadShape, PinType, SheetSide};
use crate::layer::{MULTI_LAYER, TOP_ASSEMBLY, TOP_COPPER, TOP_COURTYARD, TOP_SILK};
use crate::library::Footprint;
use crate::units::{Coord, Point, mm, to_mm};

const CLEARANCE: f64 = 0.15;
const EPS: f64 = 1e-3;

// ---------- Geometry helpers (independent of the builder's clipping) ----------

#[derive(Clone, Copy, Debug)]
struct R {
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
}

impl R {
    fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x0 && x <= self.x1 && y >= self.y0 && y <= self.y1
    }
    fn dist(&self, x: f64, y: f64) -> f64 {
        let dx = (self.x0 - x).max(0.0).max(x - self.x1);
        let dy = (self.y0 - y).max(0.0).max(y - self.y1);
        dx.hypot(dy)
    }
    fn far(&self, x: f64, y: f64) -> f64 {
        (x - self.x0).abs().max((x - self.x1).abs()).hypot((y - self.y0).abs().max((y - self.y1).abs()))
    }
    fn corners(&self) -> [(f64, f64); 4] {
        [(self.x0, self.y0), (self.x1, self.y0), (self.x1, self.y1), (self.x0, self.y1)]
    }
}

fn pad_rect(p: &Pad) -> R {
    assert_eq!(p.rotation, 0.0, "wizard pads are axis-aligned");
    let (x, y, w, h) = (to_mm(p.pos.x), to_mm(p.pos.y), to_mm(p.width), to_mm(p.height));
    R { x0: x - w / 2.0, y0: y - h / 2.0, x1: x + w / 2.0, y1: y + h / 2.0 }
}

fn f(p: Point) -> (f64, f64) {
    (to_mm(p.x), to_mm(p.y))
}

fn point_seg_dist(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let l2 = dx * dx + dy * dy;
    let t = if l2 == 0.0 { 0.0 } else { (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / l2).clamp(0.0, 1.0) };
    (p.0 - a.0 - t * dx).hypot(p.1 - a.1 - t * dy)
}

fn segs_cross(a: (f64, f64), b: (f64, f64), c: (f64, f64), d: (f64, f64)) -> bool {
    let o = |p: (f64, f64), q: (f64, f64), r: (f64, f64)| (q.0 - p.0) * (r.1 - p.1) - (q.1 - p.1) * (r.0 - p.0);
    let (d1, d2, d3, d4) = (o(c, d, a), o(c, d, b), o(a, b, c), o(a, b, d));
    (d1 > 0.0) != (d2 > 0.0) && (d3 > 0.0) != (d4 > 0.0)
}

fn seg_rect_dist(a: (f64, f64), b: (f64, f64), r: &R) -> f64 {
    if r.contains(a.0, a.1) || r.contains(b.0, b.1) {
        return 0.0;
    }
    let c = r.corners();
    for i in 0..4 {
        if segs_cross(a, b, c[i], c[(i + 1) % 4]) {
            return 0.0;
        }
    }
    let mut d = r.dist(a.0, a.1).min(r.dist(b.0, b.1));
    for k in c {
        d = d.min(point_seg_dist(k, a, b));
    }
    d
}

fn pads(fp: &Footprint) -> Vec<&Pad> {
    fp.items.iter().filter_map(|i| if let Item::Pad(p) = i { Some(p) } else { None }).collect()
}

fn pad<'a>(fp: &'a Footprint, n: &str) -> &'a Pad {
    pads(fp).into_iter().find(|p| p.number == n).unwrap_or_else(|| panic!("{}: no pad {n}", fp.name))
}

fn courtyard(fp: &Footprint) -> R {
    let rects: Vec<R> = fp
        .items
        .iter()
        .filter_map(|i| match i {
            Item::Rect(r) if r.layer == TOP_COURTYARD => {
                Some(R { x0: to_mm(r.a.x.min(r.b.x)), y0: to_mm(r.a.y.min(r.b.y)), x1: to_mm(r.a.x.max(r.b.x)), y1: to_mm(r.a.y.max(r.b.y)) })
            }
            _ => None,
        })
        .collect();
    assert_eq!(rects.len(), 1, "{}: exactly one courtyard rectangle", fp.name);
    rects[0]
}

/// Every footprint-level rule from the spec.
fn check_footprint(fp: &Footprint) {
    let name = &fp.name;
    assert!(!name.is_empty());
    for it in &fp.items {
        it.validate().unwrap_or_else(|e| panic!("{name}: invalid item {it:?}: {e}"));
    }
    let ps = pads(fp);
    assert!(!ps.is_empty(), "{name}: no pads");
    assert!(ps.iter().any(|p| p.number == "1"), "{name}: no pin 1");

    for p in &ps {
        match p.kind {
            PadKind::Smd => {
                assert_eq!(p.layer, TOP_COPPER, "{name}: SMD pad layer");
                assert!(p.drill.is_none());
                assert!(matches!(p.shape, PadShape::RoundRect | PadShape::Circle), "{name}: SMD pads are rounded");
                if p.shape == PadShape::RoundRect {
                    assert!(p.corner_ratio > 0.0 && p.corner_ratio <= 0.25);
                }
            }
            _ => {
                assert_eq!(p.layer, MULTI_LAYER, "{name}: THT pad layer");
                let d = p.drill.expect("THT drill");
                assert!(d.width < p.width.min(p.height), "{name}: drill inside pad");
            }
        }
    }
    // THT pin 1 is the (only) square pad (single-pad parts need no marking).
    let p1 = pad(fp, "1");
    if p1.kind == PadKind::Tht && ps.iter().any(|p| p.number != "1") {
        assert_eq!(p1.shape, PadShape::Rect, "{name}: THT pin 1 must be square");
        assert!(ps.iter().filter(|p| p.number != "1").all(|p| p.shape != PadShape::Rect), "{name}: only pin 1 square");
    }

    // Pads of different numbers never overlap.
    for (i, a) in ps.iter().enumerate() {
        for b in &ps[i + 1..] {
            if a.number == b.number {
                continue;
            }
            let (ra, rb) = (pad_rect(a), pad_rect(b));
            let overlap = ra.x0 < rb.x1 - EPS && rb.x0 < ra.x1 - EPS && ra.y0 < rb.y1 - EPS && rb.y0 < ra.y1 - EPS;
            assert!(!overlap, "{name}: pads {} and {} overlap", a.number, b.number);
        }
    }

    // Silk clearance to every pad.
    let rects: Vec<R> = ps.iter().map(|p| pad_rect(p)).collect();
    let mut silk_count = 0;
    for it in &fp.items {
        match it {
            Item::Line(l) if l.layer == TOP_SILK => {
                silk_count += 1;
                let need = CLEARANCE + to_mm(l.width) / 2.0 - EPS;
                for r in &rects {
                    let d = seg_rect_dist(f(l.a), f(l.b), r);
                    assert!(d >= need, "{name}: silk line {:?}-{:?} only {d:.3} mm from a pad", f(l.a), f(l.b));
                }
            }
            Item::Circle(c) if c.layer == TOP_SILK => {
                silk_count += 1;
                let (x, y) = f(c.center);
                let (r, hw) = (to_mm(c.radius), to_mm(c.width) / 2.0);
                for pr in &rects {
                    let outside = pr.dist(x, y) >= r + hw + CLEARANCE - EPS;
                    let inside = c.fill == FillStyle::None && pr.far(x, y) <= r - hw - CLEARANCE + EPS;
                    assert!(outside || inside, "{name}: silk circle too close to a pad");
                }
            }
            Item::Arc(a) if a.layer == TOP_SILK => {
                silk_count += 1;
                let hw = to_mm(a.width) / 2.0;
                for k in 0..=360 {
                    let ang = (a.start_angle + a.sweep * k as f64 / 360.0).to_radians();
                    let (x, y) = (to_mm(a.center.x) + to_mm(a.radius) * ang.cos(), to_mm(a.center.y) + to_mm(a.radius) * ang.sin());
                    for pr in &rects {
                        assert!(pr.dist(x, y) >= hw + CLEARANCE - EPS, "{name}: silk arc too close to a pad");
                    }
                }
            }
            Item::Polyline(p) if p.layer == TOP_SILK => panic!("{name}: silk polylines are not clipped; use lines"),
            _ => {}
        }
    }
    assert!(silk_count > 0 || name.contains("0201") || name.contains("0402"), "{name}: no silk outline");

    // Courtyard: contains every pad, on the 0.01 mm grid.
    let cy = courtyard(fp);
    for r in &rects {
        assert!(cy.x0 <= r.x0 - 0.25 + EPS && cy.y0 <= r.y0 - 0.25 + EPS && cy.x1 >= r.x1 + 0.25 - EPS && cy.y1 >= r.y1 + 0.25 - EPS, "{name}: courtyard excess");
    }
    for v in [cy.x0, cy.y0, cy.x1, cy.y1] {
        assert_eq!(mm(v) % 10_000, 0, "{name}: courtyard off the 0.01 mm grid");
    }

    // Assembly outline and texts.
    assert!(
        fp.items.iter().any(|i| matches!(i, Item::Polyline(p) if p.layer == TOP_ASSEMBLY && p.closed && p.points.len() == 5)
            || matches!(i, Item::Circle(c) if c.layer == TOP_ASSEMBLY)),
        "{name}: assembly body with pin-1 bevel"
    );
    let text = |tok: &str, layer| fp.items.iter().any(|i| matches!(i, Item::Text(t) if t.text == tok && t.layer == layer));
    assert!(text(crate::item::DESIGNATOR_TOKEN, TOP_SILK), "{name}: designator text");
    assert!(text(crate::item::VALUE_TOKEN, TOP_ASSEMBLY), "{name}: value text");
    // Designator sits above the courtyard.
    for it in &fp.items {
        if let Item::Text(t) = it
            && t.layer == TOP_SILK
        {
            assert!(to_mm(t.pos.y) > cy.y1, "{name}: designator above the part");
        }
    }
}

fn sweep_specs() -> Vec<FootprintSpec> {
    let mut v = Vec::new();
    for size in ChipSize::ALL {
        for (prefix, polarity) in [("R", false), ("LED", true)] {
            v.push(FootprintSpec::Chip(ChipSpec { size, prefix: prefix.into(), polarity }));
        }
    }
    for package in SodPackage::ALL {
        v.push(FootprintSpec::Sod(SodSpec { package, prefix: "D".into() }));
    }
    for package in SotPackage::ALL {
        v.push(FootprintSpec::Sot(SotSpec { package }));
    }
    for pins in [8, 14, 16, 20] {
        v.push(FootprintSpec::Soic(DualRowSpec { pins: Some(pins), ..Default::default() }));
    }
    for pins in [8, 16, 20, 28] {
        v.push(FootprintSpec::Ssop(DualRowSpec { pins: Some(pins), ..Default::default() }));
        v.push(FootprintSpec::Tssop(DualRowSpec { pins: Some(pins), ..Default::default() }));
    }
    v.push(FootprintSpec::Tssop(DualRowSpec { pins: Some(24), pitch: Some(0.5), lead_width_min: Some(0.17), lead_width_max: Some(0.27), ..Default::default() }));
    for (n, pitch, body, ep, grid) in [(4, 0.5, 3.0, true, 0), (5, 0.5, 4.0, true, 2), (6, 0.5, 4.0, true, 3), (8, 0.5, 5.0, true, 0), (4, 0.65, 4.0, false, 0), (10, 0.4, 5.0, true, 2)] {
        let (wmin, wmax) = if pitch < 0.45 { (0.15, 0.25) } else { (0.18, 0.30) };
        v.push(FootprintSpec::Qfn(QfnSpec {
            pins_per_side: n,
            pitch,
            body_size: body,
            exposed_pad: ep,
            paste_grid: grid,
            lead_width_min: wmin,
            lead_width_max: wmax,
            ..Default::default()
        }));
    }
    for (pins, pitch, body) in [(32, 0.8, 7.0), (44, 0.8, 10.0), (48, 0.5, 7.0), (64, 0.5, 10.0), (100, 0.5, 14.0), (144, 0.5, 20.0), (64, 0.4, 7.0)] {
        v.push(FootprintSpec::Qfp(QfpSpec { pins, pitch, body_size: body, ..Default::default() }));
    }
    for (pins, row) in [(4, 7.62), (8, 7.62), (14, 7.62), (16, 7.62), (24, 15.24), (40, 15.24)] {
        v.push(FootprintSpec::Dip(DipSpec { pins, row_spacing: row, ..Default::default() }));
    }
    for (rows, n, pitch, shrouded) in [(1, 1, 2.54, false), (1, 4, 2.54, false), (2, 5, 2.54, false), (2, 20, 2.54, true), (1, 10, 2.0, false), (2, 8, 1.27, false), (3, 4, 2.54, false), (2, 7, 2.0, true)] {
        v.push(FootprintSpec::PinHeader(PinHeaderSpec { rows, pins_per_row: n, pitch, shrouded, ..Default::default() }));
    }
    v.push(FootprintSpec::Axial(AxialSpec::default()));
    v.push(FootprintSpec::Axial(AxialSpec { prefix: "D".into(), pitch: 7.62, body_length: 4.2, body_diameter: 2.0, polarity: true, ..Default::default() }));
    v.push(FootprintSpec::Axial(AxialSpec { pitch: 15.24, body_length: 10.0, body_diameter: 3.5, pad_diameter: 2.0, drill: 1.0, ..Default::default() }));
    for (d, p, pad, drill) in [(5.0, 2.0, 1.6, 0.8), (6.3, 2.5, 1.6, 0.8), (8.0, 3.5, 1.6, 0.8), (10.0, 5.0, 2.0, 1.0), (4.0, 1.5, 1.2, 0.7)] {
        v.push(FootprintSpec::Radial(RadialSpec { diameter: d, pitch: p, pad_diameter: pad, drill, polarized: true }));
    }
    v.push(FootprintSpec::Radial(RadialSpec { polarized: false, ..Default::default() }));
    for package in CrystalPackage::ALL {
        v.push(FootprintSpec::Crystal(CrystalSpec { package }));
    }
    v
}

// ---------- Footprint wizard ----------

#[test]
fn every_family_default_generates() {
    let fams = families();
    assert_eq!(fams.len(), 13);
    for fam in &fams {
        let fp = generate(&fam.default_spec).unwrap_or_else(|e| panic!("{}: {e}", fam.id));
        check_footprint(&fp);
        let json = serde_json::to_value(&fam.default_spec).unwrap();
        assert_eq!(json["family"], fam.id.as_str());
        for p in &fam.params {
            assert!(!p.default.is_null(), "{}: param {} has no default", fam.id, p.key);
            assert!(json.get(&p.key).is_some(), "{}: param {} not in the spec", fam.id, p.key);
        }
    }
}

#[test]
fn parameter_sweeps_pass_all_rules() {
    let mut names = HashSet::new();
    for spec in sweep_specs() {
        let fp = generate(&spec).unwrap_or_else(|e| panic!("{spec:?}: {e}"));
        assert_eq!(fp.name, default_name(&spec));
        check_footprint(&fp);
        assert!(names.insert(fp.name.clone()), "duplicate name {}", fp.name);
    }
}

#[test]
fn chip_0603_matches_ipc_nominal() {
    // IPC-7351B RESC1608X55N is about 0.95 × 1.00 mm lands at 1.55–1.6 mm
    // centers; accepted tolerance ±0.15 mm on pad length, ±0.1 mm elsewhere.
    let fp = generate(&FootprintSpec::Chip(ChipSpec::default())).unwrap();
    assert_eq!(fp.name, "R_0603_1608Metric");
    let (a, b) = (pad(&fp, "1"), pad(&fp, "2"));
    assert!((to_mm(a.width) - 0.95).abs() <= 0.15, "pad length {}", to_mm(a.width));
    assert!((to_mm(a.height) - 1.0).abs() <= 0.1, "pad width {}", to_mm(a.height));
    assert!((to_mm(b.pos.x - a.pos.x) - 1.6).abs() <= 0.1);
    assert_eq!(a.pos.y, 0);
    assert!(a.pos.x < 0 && b.pos.x > 0, "pad 1 left");
    // Exact values of our math: 0.85 × 0.95 at ±0.80.
    assert_eq!((a.width, a.height, b.pos.x), (mm(0.85), mm(0.95), mm(0.8)));
}

#[test]
fn soic8_rows_and_pitch() {
    let fp = generate(&FootprintSpec::Soic(DualRowSpec::default())).unwrap();
    assert_eq!(fp.name, "SOIC-8_3.9x4.9mm_P1.27mm");
    let (p1, p2, p8, p4, p5) = (pad(&fp, "1"), pad(&fp, "2"), pad(&fp, "8"), pad(&fp, "4"), pad(&fp, "5"));
    assert_eq!(p1.pos.y - p2.pos.y, mm(1.27));
    assert_eq!(p1.pos.y, p8.pos.y);
    assert_eq!(p4.pos.y, p5.pos.y);
    assert!(p1.pos.x < 0 && p1.pos.y > 0, "pin 1 top-left");
    // IPC-7351B RMS math with the JEDEC MS-012 foot length (0.40–1.27 mm)
    // puts the rows 4.95 mm apart (2.0 × 0.6 mm lands).
    // The shorter-foot pattern quoted by many
    // datasheets (5.40 mm rows, 1.55 mm lands) results from L = 0.40–0.70.
    assert!((to_mm(p8.pos.x - p1.pos.x) - 4.95).abs() <= 0.05);
    assert_eq!(p1.height, mm(0.6));
    let short = generate(&FootprintSpec::Soic(DualRowSpec { lead_length_max: Some(0.7), ..Default::default() })).unwrap();
    let (s1, s8) = (pad(&short, "1"), pad(&short, "8"));
    assert!((to_mm(s8.pos.x - s1.pos.x) - 5.4).abs() <= 0.05, "{}", to_mm(s8.pos.x - s1.pos.x));
    assert!((to_mm(s1.width) - 1.55).abs() <= 0.05);
}

#[test]
fn qfn_numbering_and_exposed_pad() {
    let fp = generate(&FootprintSpec::Qfn(QfnSpec { paste_grid: 2, ..Default::default() })).unwrap();
    assert_eq!(fp.name, "QFN-16-1EP_3x3mm_P0.5mm_EP1.7x1.7mm");
    let ep = pad(&fp, "17");
    assert_eq!((ep.pos, ep.width), (Point::new(0, 0), mm(1.7)));
    // Counter-clockwise: 1 top of left side, 5 left of bottom, 9 bottom of right, 13 right of top.
    let (p1, p4, p5, p9, p13) = (pad(&fp, "1"), pad(&fp, "4"), pad(&fp, "5"), pad(&fp, "9"), pad(&fp, "13"));
    assert!(p1.pos.x < 0 && p1.pos.y > p4.pos.y);
    assert!(p5.pos.y < 0 && p5.pos.x < 0);
    assert!(p9.pos.x > 0 && p9.pos.y < 0);
    assert!(p13.pos.y > 0 && p13.pos.x > 0);
    let paste = fp.items.iter().filter(|i| matches!(i, Item::Rect(r) if r.layer == crate::layer::TOP_PASTE)).count();
    assert_eq!(paste, 4);
    // Signal lands keep ≥ 0.2 mm from the exposed pad.
    let ep_r = pad_rect(ep);
    for p in pads(&fp).iter().filter(|p| p.number != "17") {
        let r = pad_rect(p);
        let gap = (ep_r.x0 - r.x1).max(r.x0 - ep_r.x1).max(ep_r.y0 - r.y1).max(r.y0 - ep_r.y1);
        assert!(gap >= 0.2 - EPS);
    }
}

#[test]
fn header_numbering_and_names() {
    let fp = generate(&FootprintSpec::PinHeader(PinHeaderSpec { rows: 2, pins_per_row: 5, ..Default::default() })).unwrap();
    assert_eq!(fp.name, "PinHeader_2x05_P2.54mm_Vertical");
    let (p1, p2, p3) = (pad(&fp, "1"), pad(&fp, "2"), pad(&fp, "3"));
    assert_eq!(p2.pos.x - p1.pos.x, mm(2.54));
    assert_eq!(p1.pos.y, p2.pos.y);
    assert_eq!(p1.pos.y - p3.pos.y, mm(2.54));
    assert_eq!(pads(&fp).len(), 10);
    let names: Vec<String> = [
        FootprintSpec::Dip(DipSpec::default()),
        FootprintSpec::Qfp(QfpSpec::default()),
        FootprintSpec::Tssop(DualRowSpec::default()),
        FootprintSpec::Radial(RadialSpec::default()),
        FootprintSpec::Axial(AxialSpec::default()),
        FootprintSpec::Sot(SotSpec { package: SotPackage::Sot223 }),
    ]
    .iter()
    .map(default_name)
    .collect();
    assert_eq!(
        names,
        [
            "DIP-8_W7.62mm",
            "LQFP-48_7x7mm_P0.5mm",
            "TSSOP-14_4.4x5mm_P0.65mm",
            "CP_Radial_D5.0mm_P2.00mm",
            "R_Axial_L6.3mm_D2.5mm_P10.16mm_Horizontal",
            "SOT-223-3_TabPin2"
        ]
    );
}

#[test]
fn nonsense_parameters_are_rejected() {
    let bad = [
        FootprintSpec::Soic(DualRowSpec { pins: Some(7), ..Default::default() }),
        FootprintSpec::Soic(DualRowSpec { pitch: Some(-1.0), ..Default::default() }),
        FootprintSpec::Soic(DualRowSpec { lead_span_min: Some(6.5), lead_span_max: Some(6.0), ..Default::default() }),
        FootprintSpec::Soic(DualRowSpec { pins: Some(16), body_length: Some(4.9), ..Default::default() }),
        FootprintSpec::Qfn(QfnSpec { pins_per_side: 12, ..Default::default() }),
        FootprintSpec::Qfn(QfnSpec { pitch: f64::NAN, ..Default::default() }),
        FootprintSpec::Qfp(QfpSpec { pins: 50, ..Default::default() }),
        FootprintSpec::Dip(DipSpec { drill: 1.6, ..Default::default() }),
        FootprintSpec::PinHeader(PinHeaderSpec { pins_per_row: 0, ..Default::default() }),
        FootprintSpec::Axial(AxialSpec { pitch: 5.08, ..Default::default() }),
        FootprintSpec::Radial(RadialSpec { diameter: 2.0, pitch: 5.0, ..Default::default() }),
        FootprintSpec::Chip(ChipSpec { prefix: "".into(), ..Default::default() }),
    ];
    for spec in bad {
        assert!(generate(&spec).is_err(), "accepted {spec:?}");
    }
}

#[test]
fn spec_json_shape() {
    let s: FootprintSpec = serde_json::from_str(r#"{"family":"soic","pins":14}"#).unwrap();
    assert_eq!(generate(&s).unwrap().name, "SOIC-14_3.9x8.65mm_P1.27mm");
    let s: FootprintSpec = serde_json::from_str(r#"{"family":"chip","size":"0805","prefix":"C"}"#).unwrap();
    assert_eq!(default_name(&s), "C_0805_2012Metric");
    let s: FootprintSpec = serde_json::from_str(r#"{"family":"pinHeader","rows":2,"pinsPerRow":3,"pitch":2.0}"#).unwrap();
    assert_eq!(default_name(&s), "PinHeader_2x03_P2.00mm_Vertical");
    let s: FootprintSpec = serde_json::from_str(r#"{"family":"crystal","package":"3225"}"#).unwrap();
    assert_eq!(default_name(&s), "Crystal_SMD_3225-4Pin_3.2x2.5mm");
    for spec in sweep_specs() {
        let json = serde_json::to_string(&spec).unwrap();
        assert_eq!(serde_json::from_str::<FootprintSpec>(&json).unwrap(), spec);
    }
    let fams = serde_json::to_value(families()).unwrap();
    assert_eq!(fams[0]["id"], "chip");
    assert_eq!(fams[0]["defaultSpec"]["family"], "chip");
    assert_eq!(fams[0]["params"][0]["key"], "size");
    assert_eq!(fams[0]["params"][0]["kind"], "choice");
}

// ---------- Symbol wizard ----------

fn on_grid(p: Point) -> bool {
    p.x % mm(1.27) == 0 && p.y % mm(1.27) == 0
}

fn pin(number: &str, name: &str, side: SheetSide) -> PinSpec {
    PinSpec { number: number.into(), name: name.into(), electrical: PinType::Bidirectional, side, hidden: false }
}

#[test]
fn box_symbol_pins_on_grid_and_sides() {
    for (nl, nr, nt, nb) in [(1, 0, 0, 0), (4, 4, 0, 0), (5, 3, 0, 0), (8, 7, 2, 1), (2, 2, 3, 4), (16, 16, 0, 0), (0, 0, 1, 1)] {
        let mut pins = Vec::new();
        let mut k = 0;
        for (side, n) in [(SheetSide::Left, nl), (SheetSide::Right, nr), (SheetSide::Top, nt), (SheetSide::Bottom, nb)] {
            for _ in 0..n {
                k += 1;
                let name = if k % 2 == 0 { format!("P{k}") } else { format!("SIG{k}_LONG") };
                pins.push(pin(&k.to_string(), &name, side));
            }
        }
        let spec = SymbolSpec { name: "IC".into(), prefix: "U".into(), description: String::new(), pins, body_width: None };
        let sym = box_symbol(&spec).unwrap();
        let body = sym.units[0]
            .items
            .iter()
            .find_map(|i| if let Item::Rect(r) = i { Some(r.clone()) } else { None })
            .expect("body");
        assert_eq!(body.fill, FillStyle::Background);
        assert_eq!(body.width, mm(0.254));
        let (bx0, bx1) = (body.a.x.min(body.b.x), body.a.x.max(body.b.x));
        let (by0, by1) = (body.a.y.min(body.b.y), body.a.y.max(body.b.y));
        let mut seen = HashSet::new();
        for (_, p) in sym.all_pins() {
            assert!(on_grid(p.pos), "pin {} at {:?} off grid", p.number, p.pos);
            assert_eq!(p.length, mm(2.54));
            assert!(seen.insert(p.pos), "two pins at {:?}", p.pos);
            let side = spec.pins.iter().find(|s| s.number == p.number).unwrap().side;
            let end = p.body_end();
            match side {
                SheetSide::Left => assert!(p.rotation == 0.0 && p.pos.x < bx0 && end.x == bx0),
                SheetSide::Right => assert!(p.rotation == 180.0 && p.pos.x > bx1 && end.x == bx1),
                SheetSide::Top => assert!(p.rotation == 270.0 && p.pos.y > by1 && end.y == by1),
                SheetSide::Bottom => assert!(p.rotation == 90.0 && p.pos.y < by0 && end.y == by0),
            }
            assert!(end.x >= bx0 && end.x <= bx1 && end.y >= by0 && end.y <= by1);
        }
        let names: Vec<&str> = sym.fields.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["Designator", "Value"]);
        assert!(sym.fields.iter().all(|f| f.visible && f.height == mm(1.27)));
        let des = &sym.fields[0];
        assert!(des.offset.y > by1 && des.offset.x <= bx0 + 1);
        assert!(sym.fields[1].offset.y < by0);
    }
}

#[test]
fn box_symbol_validation_and_width_override() {
    let pins = vec![pin("1", "A", SheetSide::Left), pin("1", "B", SheetSide::Right)];
    let mut spec = SymbolSpec { name: "X".into(), prefix: "U".into(), description: String::new(), pins, body_width: None };
    assert!(box_symbol(&spec).is_err());
    spec.pins[1].number = "2".into();
    spec.body_width = Some(30.0);
    let s = box_symbol(&spec).unwrap();
    let Item::Rect(r) = &s.units[0].items[0] else { panic!() };
    assert_eq!((r.b.x - r.a.x).abs(), mm(30.48));
    spec.pins.clear();
    assert!(box_symbol(&spec).is_err());
}

// ---------- Starter library ----------

#[test]
fn starter_library_is_consistent() {
    let lib = starter_library();
    assert_eq!(lib.name, "PCBPlannerCAD Basic");
    let uniq = |v: Vec<&str>| v.len() == v.iter().collect::<HashSet<_>>().len();
    assert!(uniq(lib.symbols.iter().map(|s| s.name.as_str()).collect()));
    assert!(uniq(lib.footprints.iter().map(|s| s.name.as_str()).collect()));
    assert!(uniq(lib.components.iter().map(|s| s.name.as_str()).collect()));

    for fp in &lib.footprints {
        check_footprint(fp);
    }
    for sym in &lib.symbols {
        assert!(!sym.units.is_empty());
        let mut seen = HashSet::new();
        for (unit, p) in sym.all_pins() {
            assert!(on_grid(p.pos), "{}: pin {} at {:?} off the 1.27 mm grid", sym.name, p.number, p.pos);
            assert!(seen.insert((unit, p.pos)), "{}: two pins at {:?}", sym.name, p.pos);
        }
        for it in sym.units.iter().flat_map(|u| u.items.iter()) {
            it.validate().unwrap_or_else(|e| panic!("{}: {e}", sym.name));
        }
        assert!(sym.fields.iter().any(|f| f.name == "Designator" && f.visible));
        assert!(sym.fields.iter().any(|f| f.name == "Value" && f.visible));
        if sym.all_pins().count() == 2 {
            assert!(!sym.show_pin_names, "{}: two-terminal parts hide pin names", sym.name);
        }
    }
    for c in &lib.components {
        let sym = lib.symbol(&c.symbol).unwrap_or_else(|| panic!("{}: missing symbol {}", c.name, c.symbol));
        assert!(c.parameter("Value").is_some(), "{}: Value parameter", c.name);
        assert!(!c.footprints.is_empty(), "{}: no footprints", c.name);
        for fname in &c.footprints {
            let fp = lib.footprint(fname).unwrap_or_else(|| panic!("{}: missing footprint {fname}", c.name));
            let numbers: HashSet<&str> = pads(fp).iter().map(|p| p.number.as_str()).collect();
            for (unit, p) in sym.all_pins() {
                let mapped = c.pads_for(unit, &p.number);
                assert!(!mapped.is_empty(), "{}: pin {} unmapped", c.name, p.number);
                for pad in mapped {
                    assert!(numbers.contains(pad.as_str()), "{}: pin {} → pad {pad} missing in {fname}", c.name, p.number);
                }
            }
        }
    }
    for n in 2..=10 {
        assert!(lib.component(&format!("Conn_01x{n:02}")).is_some());
    }
    let dual = lib.symbol("Opamp_Dual").unwrap();
    assert_eq!(dual.units.len(), 3);
    assert!(lib.components.len() >= 30);
}

#[test]
fn starter_two_terminal_spans() {
    let lib = starter_library();
    for name in ["R", "C", "C_Polarized", "L", "FerriteBead", "D", "D_Zener", "D_Schottky", "LED", "Crystal", "Fuse", "SW_Push"] {
        let sym = lib.symbol(name).unwrap();
        let pins: Vec<_> = sym.all_pins().map(|(_, p)| p).collect();
        assert_eq!(pins.len(), 2, "{name}");
        assert!(pins.iter().all(|p| p.length == mm(2.54)));
        let span: Coord = (pins[0].pos.x - pins[1].pos.x).abs() + (pins[0].pos.y - pins[1].pos.y).abs();
        assert!(span == mm(7.62) || span == mm(10.16), "{name}: span {}", to_mm(span));
    }
}

#[test]
fn starter_library_serde_round_trip() {
    let lib = starter_library();
    let json = serde_json::to_string(&lib).unwrap();
    let back: crate::library::Library = serde_json::from_str(&json).unwrap();
    assert_eq!(back, lib);
}
