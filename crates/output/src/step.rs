//! 3D board model and faceted STEP export.
//!
//! Coordinates: millimeters, right-handed, X/Y as on the board (Y up in the
//! 2D editor), Z up out of the top side. The laminate occupies
//! `0 ≤ z ≤ thickness`: the bottom face is at z = 0, the top face at
//! z = thickness.
//!
//! - [`board_model`]: the laminate solid (outline with cutouts, drills and
//!   slots) as a watertight triangle mesh plus every component's placement.
//! - [`write_step`]: STEP AP214 export of triangle meshes (board + component
//!   models triangulated by the UI), one named body per mesh.
//! - [`mesh_to_obj`]: Wavefront OBJ dump of the same meshes.

use std::fmt::Write as _;

use pcbplannercad_core::Document;
use pcbplannercad_core::geom::normalize_deg;
use pcbplannercad_core::item::{Item, PadShape};
use pcbplannercad_core::layer;
use pcbplannercad_core::library::{Footprint, Model3d};
use pcbplannercad_core::pcb::geometry::{all_pads, arc_points, board_settings, capsule_polygon, circle_polygon, outline_loops, round_rect_polygon};
use pcbplannercad_core::pcb::{BoardSettings, PlacedFootprint, Side};
use pcbplannercad_core::units::{Coord, Point, mm, to_mm};
use serde::{Deserialize, Serialize};

use crate::OutputError;

#[path = "step_poly.rs"]
mod poly;
#[path = "step_write.rs"]
mod writer;

use poly::{Region, Ring};

/// A triangle mesh in millimeters.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Mesh {
    pub name: String,
    /// Flat `x, y, z` triples in mm.
    pub positions: Vec<f32>,
    /// Triangle vertex indices, counter-clockwise seen from outside.
    pub indices: Vec<u32>,
    /// sRGB, 0..1.
    pub color: [f32; 3],
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardModel {
    /// Laminate solid with holes and cutouts (shared vertices; use flat
    /// shading or split vertices for rendering).
    pub board: Mesh,
    pub thickness_mm: f64,
    /// False when no closed board outline exists and a 100 × 80 mm
    /// placeholder rectangle was used.
    pub outline_found: bool,
    pub components: Vec<ComponentPlacement>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentPlacement {
    pub designator: String,
    /// Footprint cache key ("Library/Name").
    pub footprint: String,
    /// 3D model of the cached footprint (None if absent or footprint not cached).
    pub model: Option<Model3d>,
    /// 4×4 column-major matrix (`m[col * 4 + row]`) from footprint-local mm
    /// (x, y as in the footprint editor, z up away from the mounting surface)
    /// to board mm. Top: `T(x, y, thickness) · Rz(rotation)`. Bottom:
    /// `T(x, y, 0) · Rz(rotation) · Ry(180°)` where `Ry(180°) = diag(-1, 1, -1)`
    /// mirrors local X (exactly like `PlacedFootprint::placement()`) and points
    /// local +Z down, away from the bottom face.
    pub transform: [f64; 16],
    /// Fallback body box in footprint-local mm `[minx, miny, minz, maxx, maxy, maxz]`
    /// (courtyard, else assembly, else pad bounds; 1 mm tall from z = 0).
    pub fallback_box: [f64; 6],
}

/// Outline chains whose ends are closer than this are joined.
const OUTLINE_TOL: Coord = 10_000;
/// Smallest drill that is cut into the 3D laminate.
const MIN_DRILL: Coord = 200_000;
const FALLBACK_HEIGHT: f64 = 1.0;
const DEFAULT_MASK: [f32; 3] = [0.122, 0.373, 0.165];

/// Builds the 3D board: laminate solid and component placements.
pub fn board_model(doc: &Document) -> BoardModel {
    let default_settings = BoardSettings::default();
    let settings = board_settings(doc).map_or(&default_settings, |(_, b)| b);
    let mut thickness = to_mm(settings.stackup.thickness());
    if thickness.is_nan() || thickness <= 0.0 {
        thickness = 1.6;
    }

    let loops: Vec<Ring> = outline_loops(doc, OUTLINE_TOL).iter().map(|l| poly::clean_ring(l)).filter(|r| r.len() >= 3).collect();
    let mut regions = poly::nest(loops);
    let outline_found = !regions.is_empty();
    if !outline_found {
        regions = vec![fallback_region(doc)];
    }
    poly::add_holes(&mut regions, drill_holes(doc, settings));

    let color = parse_color(&settings.mask_color).unwrap_or(DEFAULT_MASK);
    let board = extrude(&regions, thickness, "Board", color);

    let components = doc
        .items()
        .filter_map(|(_, i)| if let Item::Footprint(fp) = i { Some(fp) } else { None })
        .map(|fp| {
            let cached = doc.cached_footprint(&fp.footprint);
            ComponentPlacement {
                designator: fp.designator.clone(),
                footprint: fp.footprint.clone(),
                model: cached.and_then(|f| f.model.clone()),
                transform: component_transform(fp, thickness),
                fallback_box: cached.map_or([-0.5, -0.5, 0.0, 0.5, 0.5, FALLBACK_HEIGHT], fallback_box),
            }
        })
        .collect();

    BoardModel { board, thickness_mm: thickness, outline_found, components }
}

/// Writes meshes as a STEP AP214 file (see the module docs of the writer for
/// the entity structure). Each mesh with at least one non-degenerate triangle
/// becomes a separately named, colored body.
pub fn write_step(name: &str, meshes: &[Mesh]) -> Result<String, OutputError> {
    writer::write(name, meshes, &writer::now_timestamp())
}

/// Wavefront OBJ of all meshes: one `o` object per mesh with a
/// `usemtl c_rrggbb` material named after its color (a matching
/// `newmtl … Kd r g b` definition is included as comments).
pub fn mesh_to_obj(meshes: &[Mesh]) -> String {
    let tris: usize = meshes.iter().map(|m| m.indices.len() / 3).sum();
    let verts: usize = meshes.iter().map(|m| m.positions.len() / 3).sum();
    let mut out = String::with_capacity(verts * 32 + tris * 24 + 256);
    out.push_str("# PCBPlanner Studio 3D export\n");
    let mut seen = std::collections::BTreeSet::new();
    for m in meshes {
        let (mat, _) = material(m.color);
        if seen.insert(mat.clone()) {
            let c = m.color;
            let _ = writeln!(out, "# newmtl {mat}\n# Kd {} {} {}", c[0], c[1], c[2]);
        }
    }
    let mut base = 1usize;
    for (i, m) in meshes.iter().enumerate() {
        let name: String = if m.name.trim().is_empty() { format!("Body{}", i + 1) } else { m.name.split_whitespace().collect::<Vec<_>>().join("_") };
        let _ = writeln!(out, "o {name}\nusemtl {}", material(m.color).0);
        for p in m.positions.chunks_exact(3) {
            let _ = writeln!(out, "v {} {} {}", p[0], p[1], p[2]);
        }
        let nv = m.positions.len() / 3;
        for t in m.indices.chunks_exact(3) {
            if t.iter().all(|&i| (i as usize) < nv) {
                let _ = writeln!(out, "f {} {} {}", base + t[0] as usize, base + t[1] as usize, base + t[2] as usize);
            }
        }
        base += nv;
    }
    out
}

fn material(c: [f32; 3]) -> (String, [u8; 3]) {
    let b = c.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
    (format!("c_{:02x}{:02x}{:02x}", b[0], b[1], b[2]), b)
}

fn parse_color(s: &str) -> Option<[f32; 3]> {
    let h = s.trim().trim_start_matches('#');
    if h.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(h, 16).ok()?;
    Some([(v >> 16) & 0xff, (v >> 8) & 0xff, v & 0xff].map(|c| c as f32 / 255.0))
}

/// Footprint-local → board transform (see [`ComponentPlacement::transform`]).
fn component_transform(fp: &PlacedFootprint, thickness: f64) -> [f64; 16] {
    let (s, c) = match normalize_deg(fp.rotation) {
        0.0 => (0.0, 1.0),
        90.0 => (1.0, 0.0),
        180.0 => (0.0, -1.0),
        270.0 => (-1.0, 0.0),
        d => d.to_radians().sin_cos(),
    };
    let (x, y) = (to_mm(fp.pos.x), to_mm(fp.pos.y));
    match fp.side {
        Side::Top => [c, s, 0.0, 0.0, -s, c, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, x, y, thickness, 1.0],
        // Rz(θ) · diag(-1, 1, -1): column 0 negated, column 2 = -Z.
        Side::Bottom => [-c, -s, 0.0, 0.0, -s, c, 0.0, 0.0, 0.0, 0.0, -1.0, 0.0, x, y, 0.0, 1.0],
    }
}

fn item_layer(it: &Item) -> Option<pcbplannercad_core::LayerId> {
    Some(match it {
        Item::Line(x) => x.layer,
        Item::Arc(x) => x.layer,
        Item::Circle(x) => x.layer,
        Item::Rect(x) => x.layer,
        Item::Polyline(x) => x.layer,
        _ => return None,
    })
}

fn item_points(it: &Item, out: &mut Vec<Point>) {
    match it {
        Item::Line(l) => out.extend([l.a, l.b]),
        Item::Arc(a) => out.extend(arc_points(a)),
        Item::Circle(c) => out.extend([c.center.translated(-c.radius, -c.radius), c.center.translated(c.radius, c.radius)]),
        Item::Rect(r) => out.extend([r.a, r.b]),
        Item::Polyline(p) => out.extend(p.points.iter().copied()),
        Item::Pad(p) => {
            let (w, h) = if p.shape == PadShape::Circle { (p.width, p.width) } else { (p.width, p.height) };
            out.extend(round_rect_polygon(p.pos, w, h, p.rotation, 0));
        }
        _ => {}
    }
}

/// Body box from courtyard, else assembly, else pad bounds (footprint-local mm).
fn fallback_box(f: &Footprint) -> [f64; 6] {
    let pick = |pred: &dyn Fn(&Item) -> bool| {
        let mut pts = Vec::new();
        for it in f.items.iter().filter(|i| pred(i)) {
            item_points(it, &mut pts);
        }
        pts
    };
    let on = |a: pcbplannercad_core::LayerId, b: pcbplannercad_core::LayerId| move |i: &Item| item_layer(i).is_some_and(|l| l == a || l == b);
    let mut pts = pick(&on(layer::TOP_COURTYARD, layer::BOTTOM_COURTYARD));
    if pts.len() < 2 {
        pts = pick(&on(layer::TOP_ASSEMBLY, layer::BOTTOM_ASSEMBLY));
    }
    if pts.len() < 2 {
        pts = pick(&|i: &Item| matches!(i, Item::Pad(_)));
    }
    if pts.is_empty() {
        return [-0.5, -0.5, 0.0, 0.5, 0.5, FALLBACK_HEIGHT];
    }
    let b = poly::bounds(&pts);
    let (mut x0, mut y0, mut x1, mut y1) = (to_mm(b[0]), to_mm(b[1]), to_mm(b[2]), to_mm(b[3]));
    // Keep a minimal footprint for degenerate (single line) bounds.
    if x1 - x0 < 0.1 {
        x0 -= 0.05;
        x1 += 0.05;
    }
    if y1 - y0 < 0.1 {
        y0 -= 0.05;
        y1 += 0.05;
    }
    [x0, y0, 0.0, x1, y1, FALLBACK_HEIGHT]
}

/// 100 × 80 mm placeholder, centered on the placed footprints (or at 0..100, 0..80).
fn fallback_region(doc: &Document) -> Region {
    let positions: Vec<Point> = doc.items().filter_map(|(_, i)| if let Item::Footprint(fp) = i { Some(fp.pos) } else { None }).collect();
    let (w, h) = (mm(100.0), mm(80.0));
    let (x0, y0) = if positions.is_empty() {
        (0, 0)
    } else {
        let b = poly::bounds(&positions);
        ((b[0] + b[2]) / 2 - w / 2, (b[1] + b[3]) / 2 - h / 2)
    };
    Region { outer: vec![Point::new(x0, y0), Point::new(x0 + w, y0), Point::new(x0 + w, y0 + h), Point::new(x0, y0 + h)], holes: Vec::new() }
}

fn circle_segments(d: Coord) -> usize {
    let n = (to_mm(d) * 12.0 / 4.0).ceil() as usize * 4;
    n.clamp(12, 64)
}

/// Drill or slot outline: `w × h` oriented by `rot` degrees.
fn drill_ring(c: Point, w: Coord, h: Coord, rot: f64) -> Ring {
    if w == h {
        return circle_polygon(c, w / 2, circle_segments(w));
    }
    let (long, short) = (w.max(h), w.min(h));
    let along = rot + if w >= h { 0.0 } else { 90.0 };
    let half = (long - short) / 2;
    let a = pcbplannercad_core::geom::rotate(c.translated(-half, 0), c, along);
    let b = pcbplannercad_core::geom::rotate(c.translated(half, 0), c, along);
    capsule_polygon(a, b, short, 0)
}

fn drill_holes(doc: &Document, settings: &BoardSettings) -> Vec<Ring> {
    let mut out = Vec::new();
    for pad in all_pads(doc, settings) {
        if let Some(d) = pad.drill {
            if d.width.min(d.height) >= MIN_DRILL {
                out.push(drill_ring(pad.pos, d.width, d.height, pad.rotation));
            }
        }
    }
    for (_, it) in doc.items() {
        if let Item::Via(v) = it {
            if v.drill >= MIN_DRILL {
                out.push(drill_ring(v.pos, v.drill, v.drill, 0.0));
            }
        }
    }
    out
}

/// Extrudes regions into a closed solid: top face at `thickness`, bottom at 0,
/// side walls along every ring. Vertices are shared between faces and walls.
fn extrude(regions: &[Region], thickness: f64, name: &str, color: [f32; 3]) -> Mesh {
    let mut positions: Vec<f32> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();
    for r in regions {
        let tris = poly::triangulate(r);
        let rings: Vec<&Ring> = std::iter::once(&r.outer).chain(r.holes.iter()).collect();
        let n: usize = rings.iter().map(|x| x.len()).sum();
        let base = (positions.len() / 3) as u32;
        for z in [0.0, thickness] {
            for p in rings.iter().flat_map(|x| x.iter()) {
                positions.extend([to_mm(p.x) as f32, to_mm(p.y) as f32, z as f32]);
            }
        }
        let bot = |i: u32| base + i;
        let top = |i: u32| base + n as u32 + i;
        for t in &tris {
            indices.extend([top(t[0]), top(t[1]), top(t[2])]);
            indices.extend([bot(t[0]), bot(t[2]), bot(t[1])]);
        }
        // Outer rings are CCW and holes CW, so (edge × up) points out of the material.
        let mut start = 0u32;
        for ring in &rings {
            let m = ring.len() as u32;
            for i in 0..m {
                let (a, b) = (start + i, start + (i + 1) % m);
                indices.extend([bot(a), bot(b), top(b), bot(a), top(b), top(a)]);
            }
            start += m;
        }
    }
    Mesh { name: name.into(), positions, indices, color }
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};

    use super::*;
    use pcbplannercad_core::DocKind;
    use pcbplannercad_core::item::{Drill, Line, Pad, PadKind, Polyline};
    use pcbplannercad_core::pcb::Via;
    use pcbplannercad_core::pcb::geometry::world_pads;

    fn line(a: (f64, f64), b: (f64, f64)) -> Item {
        Item::Line(Line { a: Point::new(mm(a.0), mm(a.1)), b: Point::new(mm(b.0), mm(b.1)), width: mm(0.1), layer: layer::BOARD_OUTLINE })
    }

    fn rect_outline(w: f64, h: f64) -> Vec<Item> {
        let pts = [(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)];
        (0..4).map(|i| line(pts[i], pts[(i + 1) % 4])).collect()
    }

    fn via(x: f64, y: f64, drill: f64) -> Item {
        Item::Via(Via { locked: false, pos: Point::new(mm(x), mm(y)), diameter: mm(drill + 0.3), drill: mm(drill), from_layer: layer::TOP_COPPER, to_layer: layer::BOTTOM_COPPER, net: String::new() })
    }

    fn pad(number: &str, x: f64, y: f64, kind: PadKind, drill: Option<(f64, f64)>) -> Item {
        Item::Pad(Pad {
            number: number.into(),
            pos: Point::new(mm(x), mm(y)),
            rotation: 0.0,
            shape: PadShape::Rect,
            width: mm(1.8),
            height: mm(1.8),
            corner_ratio: 0.0,
            kind,
            layer: if kind == PadKind::Smd { layer::TOP_COPPER } else { layer::MULTI_LAYER },
            drill: drill.map(|(w, h)| Drill { width: mm(w), height: mm(h) }),
         copper_shapes: Default::default(),})
    }

    fn placed(designator: &str, key: &str, x: f64, y: f64, rot: f64, side: Side) -> Item {
        Item::Footprint(PlacedFootprint {
            board_only: false,
            designator: designator.into(),
            value: String::new(),
            footprint: key.into(),
            pos: Point::new(mm(x), mm(y)),
            rotation: rot,
            side,
            locked: false,
            pad_nets: Default::default(),
            component: String::new(),
            parameters: Default::default(),
        })
    }

    fn pcb(items: Vec<Item>) -> Document {
        let mut all = vec![Item::Board(BoardSettings::default())];
        all.extend(items);
        Document::from_item_list(DocKind::Pcb, all)
    }

    fn vertex(m: &Mesh, i: u32) -> [f64; 3] {
        let i = i as usize * 3;
        [m.positions[i] as f64, m.positions[i + 1] as f64, m.positions[i + 2] as f64]
    }

    /// Every directed edge has exactly one opposite (closed, consistently oriented 2-manifold).
    fn assert_watertight(m: &Mesh) {
        let mut edges: HashMap<(u32, u32), u32> = HashMap::new();
        for t in m.indices.chunks_exact(3) {
            for k in 0..3 {
                *edges.entry((t[k], t[(k + 1) % 3])).or_insert(0) += 1;
            }
        }
        for (&(a, b), &c) in &edges {
            assert_eq!(c, 1, "edge {a}-{b} used {c} times in the same direction");
            assert_eq!(edges.get(&(b, a)), Some(&1), "edge {a}-{b} has no opposite");
        }
    }

    fn volume(m: &Mesh) -> f64 {
        m.indices
            .chunks_exact(3)
            .map(|t| {
                let (a, b, c) = (vertex(m, t[0]), vertex(m, t[1]), vertex(m, t[2]));
                (a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0]) + a[2] * (b[0] * c[1] - b[1] * c[0])) / 6.0
            })
            .sum()
    }

    fn polygon_area_mm2(r: &[Point]) -> f64 {
        (poly::area2(r) as f64 / 2.0).abs() / 1e12
    }

    #[test]
    fn rectangle_with_two_holes_is_watertight_with_correct_volume() {
        let mut items = rect_outline(50.0, 30.0);
        items.push(via(10.0, 10.0, 3.0));
        items.push(via(40.0, 20.0, 1.0));
        items.push(via(25.0, 15.0, 0.15)); // below the 0.2 mm limit: not cut
        let doc = pcb(items);
        let model = board_model(&doc);
        assert!(model.outline_found);
        assert!((model.thickness_mm - 1.6).abs() < 1e-6);
        assert_watertight(&model.board);
        let holes: f64 = [(3.0, circle_segments(mm(3.0))), (1.0, circle_segments(mm(1.0)))]
            .iter()
            .map(|&(d, n)| polygon_area_mm2(&circle_polygon(Point::default(), mm(d / 2.0), n)))
            .sum();
        let expected = (50.0 * 30.0 - holes) * 1.6;
        let v = volume(&model.board);
        assert!((v - expected).abs() < 1e-3 * expected, "volume {v} vs {expected}");
        // Circle approximations are close to the true circles.
        let exact = (50.0 * 30.0 - std::f64::consts::PI * (1.5f64.powi(2) + 0.25)) * 1.6;
        assert!((v - exact).abs() < 0.01 * exact);
        let zs: HashSet<u32> = model.board.positions.chunks_exact(3).map(|p| p[2].to_bits()).collect();
        assert_eq!(zs.len(), 2);
    }

    #[test]
    fn cutout_loop_and_slot_are_cut() {
        let mut items = rect_outline(60.0, 40.0);
        // Inner rectangular cutout as a closed polyline.
        items.push(Item::Polyline(Polyline {
            points: [(30.0, 10.0), (50.0, 10.0), (50.0, 30.0), (30.0, 30.0)].iter().map(|&(x, y)| Point::new(mm(x), mm(y))).collect(),
            closed: true,
            width: mm(0.1),
            layer: layer::BOARD_OUTLINE,
            fill: Default::default(),
        }));
        // A footprint with a slotted THT pad (rotated 90° by the placement) and an NPTH hole.
        let mut f = Footprint::new("J");
        f.items = vec![pad("1", 0.0, 0.0, PadKind::Tht, Some((3.0, 1.0))), pad("2", 5.0, 0.0, PadKind::Npth, Some((2.0, 2.0)))];
        items.push(placed("J1", "L/J", 10.0, 20.0, 90.0, Side::Top));
        // A via inside the cutout must be ignored.
        items.push(via(40.0, 20.0, 0.5));
        let mut doc = pcb(items);
        doc.put_cached_footprint("L/J", f);
        let model = board_model(&doc);
        assert!(model.outline_found);
        assert_watertight(&model.board);
        let slot = polygon_area_mm2(&drill_ring(Point::default(), mm(3.0), mm(1.0), 90.0));
        let npth = polygon_area_mm2(&circle_polygon(Point::default(), mm(1.0), circle_segments(mm(2.0))));
        let expected = (60.0 * 40.0 - 20.0 * 20.0 - slot - npth) * 1.6;
        let v = volume(&model.board);
        assert!((v - expected).abs() < 1e-3 * expected, "volume {v} vs {expected}");
        // Slot exact area: 2 × 1 rectangle + Ø1 circle.
        assert!((slot - (2.0 + std::f64::consts::PI * 0.25)).abs() < 0.02);
        // The slot runs along Y after the 90° placement: its wall vertices span y 18.5..21.5 at x≈10.
        let near_slot: Vec<[f64; 3]> =
            (0..model.board.positions.len() as u32 / 3).map(|i| vertex(&model.board, i)).filter(|p| (p[0] - 10.0).abs() < 0.6 && (p[1] - 20.0).abs() < 1.6).collect();
        let ys = near_slot.iter().map(|p| p[1]).fold((f64::MAX, f64::MIN), |(a, b), y| (a.min(y), b.max(y)));
        assert!((ys.0 - 18.5).abs() < 1e-3 && (ys.1 - 21.5).abs() < 1e-3, "{ys:?}");
    }

    #[test]
    fn missing_outline_uses_placeholder() {
        let model = board_model(&pcb(vec![]));
        assert!(!model.outline_found);
        assert_watertight(&model.board);
        assert!((volume(&model.board) - 100.0 * 80.0 * 1.6).abs() < 1e-2);
        assert_eq!(model.board.color, parse_color("#1f5f2a").unwrap());
    }

    fn apply(m: &[f64; 16], p: [f64; 3]) -> [f64; 3] {
        let mut out = [0.0; 3];
        for (r, o) in out.iter_mut().enumerate() {
            *o = m[r] * p[0] + m[4 + r] * p[1] + m[8 + r] * p[2] + m[12 + r];
        }
        out
    }

    #[test]
    fn component_transforms_match_pad_placement() {
        let mut f = Footprint::new("R");
        f.items = vec![pad("1", -1.0, 0.5, PadKind::Smd, None), pad("2", 1.25, -0.3, PadKind::Smd, None)];
        f.model = Some(Model3d { path: "r.step".into(), offset: [0.0; 3], rotation: [0.0; 3], scale: [1.0; 3] });
        let items = vec![placed("R1", "L/R", 10.0, 5.0, 90.0, Side::Bottom), placed("R2", "L/R", 20.0, 7.0, 30.0, Side::Top), placed("R3", "L/R", 30.0, 9.0, 33.0, Side::Bottom)];
        let mut doc = pcb(items);
        doc.put_cached_footprint("L/R", f.clone());
        let model = board_model(&doc);
        assert_eq!(model.components.len(), 3);
        let settings = BoardSettings::default();
        for (id, item) in doc.items() {
            let Item::Footprint(fp) = item else { continue };
            let c = model.components.iter().find(|c| c.designator == fp.designator).unwrap();
            assert_eq!(c.model.as_ref().unwrap().path, "r.step");
            let pads = world_pads(id, fp, &f, &settings);
            for (local, world) in f.items.iter().zip(&pads) {
                let Item::Pad(p) = local else { unreachable!() };
                let t = apply(&c.transform, [to_mm(p.pos.x), to_mm(p.pos.y), 0.0]);
                assert!((t[0] - to_mm(world.pos.x)).abs() < 1e-5 && (t[1] - to_mm(world.pos.y)).abs() < 1e-5, "{} pad {}: {t:?} vs {:?}", fp.designator, p.number, world.pos);
            }
            let up = apply(&c.transform, [0.0, 0.0, 1.0]);
            if fp.side == Side::Bottom {
                assert!((up[2] + 1.0).abs() < 1e-12, "bottom +Z must point down below the board");
                assert!(apply(&c.transform, [0.0; 3])[2].abs() < 1e-12);
            } else {
                assert!((up[2] - (model.thickness_mm + 1.0)).abs() < 1e-12);
            }
            // Rotation part is proper (det = +1): models are not mirrored.
            let m = &c.transform;
            let det = m[0] * (m[5] * m[10] - m[9] * m[6]) - m[4] * (m[1] * m[10] - m[9] * m[2]) + m[8] * (m[1] * m[6] - m[5] * m[2]);
            assert!((det - 1.0).abs() < 1e-12);
        }
        // R1 (bottom, 90°): pad 1 local (-1, 0.5) → mirror (1, 0.5) → rot 90 (-0.5, 1) → (9.5, 6).
        let r1 = model.components.iter().find(|c| c.designator == "R1").unwrap();
        let p = apply(&r1.transform, [-1.0, 0.5, 0.0]);
        assert!((p[0] - 9.5).abs() < 1e-12 && (p[1] - 6.0).abs() < 1e-12);
        // Pad bounds fallback box: pads 1.8 mm square at x -1 / 1.25.
        let b = r1.fallback_box;
        assert!((b[0] + 1.9).abs() < 1e-6 && (b[3] - 2.15).abs() < 1e-6 && b[2] == 0.0 && b[5] == 1.0, "{b:?}");
    }

    // ---------- STEP validation ----------

    /// Splits the DATA section into `(id, body)` statements, honoring quotes.
    fn parse_step(s: &str) -> Vec<(u64, String)> {
        assert!(s.starts_with("ISO-10303-21;\nHEADER;\n"));
        assert!(s.trim_end().ends_with("END-ISO-10303-21;"));
        let header_end = s.find("ENDSEC;\nDATA;\n").expect("header section");
        let header = &s[..header_end];
        for k in ["FILE_DESCRIPTION(", "FILE_NAME(", "FILE_SCHEMA(('AUTOMOTIVE_DESIGN"] {
            assert!(header.contains(k), "{k} missing");
        }
        let data_start = header_end + "ENDSEC;\nDATA;\n".len();
        let data_end = s.rfind("ENDSEC;\nEND-ISO-10303-21;").expect("data section end");
        let data = &s[data_start..data_end];
        let mut out = Vec::new();
        let mut cur = String::new();
        let mut in_str = false;
        for ch in data.chars() {
            if ch == '\'' {
                in_str = !in_str; // '' escapes toggle twice
            }
            if ch == ';' && !in_str {
                let stmt = cur.trim().to_string();
                cur.clear();
                let eq = stmt.find('=').expect("entity assignment");
                assert!(stmt.starts_with('#'), "{stmt}");
                let id: u64 = stmt[1..eq].parse().expect("entity id");
                out.push((id, stmt[eq + 1..].to_string()));
            } else {
                cur.push(ch);
            }
        }
        assert!(!in_str && cur.trim().is_empty(), "unterminated statement");
        out
    }

    fn refs(body: &str) -> Vec<u64> {
        let mut out = Vec::new();
        let b = body.as_bytes();
        let mut in_str = false;
        let mut i = 0;
        while i < b.len() {
            match b[i] {
                b'\'' => in_str = !in_str,
                b'#' if !in_str => {
                    let j = (i + 1..b.len()).find(|&j| !b[j].is_ascii_digit()).unwrap_or(b.len());
                    out.push(body[i + 1..j].parse().unwrap());
                    i = j;
                    continue;
                }
                _ => {}
            }
            i += 1;
        }
        out
    }

    fn validate(step: &str) -> HashMap<u64, String> {
        let ents = parse_step(step);
        let mut map = HashMap::new();
        for (id, body) in ents {
            assert!(map.insert(id, body).is_none(), "duplicate id #{id}");
        }
        for body in map.values() {
            for r in refs(body) {
                assert!(map.contains_key(&r), "unresolved #{r} in {body}");
            }
            let open = body.chars().filter(|&c| c == '(').count();
            assert_eq!(open, body.chars().filter(|&c| c == ')').count(), "unbalanced {body}");
        }
        map
    }

    fn count(map: &HashMap<u64, String>, kind: &str) -> usize {
        map.values().filter(|b| b.starts_with(kind) && b[kind.len()..].starts_with('(')).count()
    }

    #[test]
    fn step_output_is_well_formed() {
        let mut items = rect_outline(20.0, 10.0);
        items.push(via(5.0, 5.0, 1.0));
        let model = board_model(&pcb(items));
        // An open mesh (single quad) with a quote in its name, and a duplicated vertex.
        let sheet = Mesh {
            name: "U1 'chip'".into(),
            positions: vec![0.0, 0.0, 2.0, 1.0, 0.0, 2.0, 1.0, 1.0, 2.0, 0.0, 1.0, 2.0, 1.0, 1.0, 2.0],
            indices: vec![0, 1, 2, 0, 4, 3, 0, 0, 1],
            color: [0.1, 0.1, 0.1],
        };
        let meshes = vec![model.board.clone(), sheet];
        let step = write_step("demo", &meshes).unwrap();
        let map = validate(&step);
        let board_tris = model.board.indices.len() / 3;
        assert_eq!(count(&map, "FACE_SURFACE"), board_tris + 2);
        assert_eq!(count(&map, "POLY_LOOP"), board_tris + 2);
        assert_eq!(count(&map, "FACE_OUTER_BOUND"), board_tris + 2);
        assert_eq!(count(&map, "FACETED_BREP"), 1);
        assert_eq!(count(&map, "CLOSED_SHELL"), 1);
        assert_eq!(count(&map, "SHELL_BASED_SURFACE_MODEL"), 1);
        assert_eq!(count(&map, "OPEN_SHELL"), 1);
        assert_eq!(count(&map, "PRODUCT"), 3);
        assert_eq!(count(&map, "NEXT_ASSEMBLY_USAGE_OCCURRENCE"), 2);
        assert_eq!(count(&map, "STYLED_ITEM"), 2);
        assert_eq!(count(&map, "COLOUR_RGB"), 2);
        // Shared vertices: board points = unique positions; the sheet merges its duplicate.
        let unique: HashSet<[u32; 3]> = model.board.positions.chunks_exact(3).map(|p| [p[0].to_bits(), p[1].to_bits(), p[2].to_bits()]).collect();
        assert_eq!(count(&map, "CARTESIAN_POINT"), unique.len() + 4 + 1);
        assert!(step.contains("SI_UNIT(.MILLI.,.METRE.)"));
        assert!(step.contains("PRODUCT('U1 ''chip''','U1 ''chip'''"));
        assert!(step.contains("FACETED_BREP('Board',"));
        // Coordinates are valid STEP reals (always with a decimal point).
        for body in map.values().filter(|b| b.starts_with("CARTESIAN_POINT(") || b.starts_with("DIRECTION(")) {
            let inner = &body[body.find("',(").unwrap() + 3..body.rfind("))").unwrap()];
            for v in inner.split(',') {
                assert!(v.contains('.') && v.parse::<f64>().is_ok(), "{body}");
            }
        }
        assert!(write_step("x", &[Mesh::default()]).is_err());
        assert!(write_step("x", &[Mesh { indices: vec![0, 1, 5], positions: vec![0.0; 9], ..Default::default() }]).is_err());
    }

    #[test]
    fn obj_export_lists_all_meshes() {
        let model = board_model(&pcb(rect_outline(10.0, 10.0)));
        let obj = mesh_to_obj(&[model.board.clone()]);
        assert!(obj.contains("o Board\nusemtl c_1f5f2a\n"));
        assert_eq!(obj.lines().filter(|l| l.starts_with("v ")).count(), model.board.positions.len() / 3);
        assert_eq!(obj.lines().filter(|l| l.starts_with("f ")).count(), model.board.indices.len() / 3);
    }

    fn grid_mesh(n: usize) -> Mesh {
        // n×n quads on a wavy surface: 2·n² triangles.
        let mut positions = Vec::new();
        for j in 0..=n {
            for i in 0..=n {
                let (x, y) = (i as f32 * 0.1, j as f32 * 0.1);
                positions.extend([x, y, (x * 3.0).sin() * (y * 2.0).cos()]);
            }
        }
        let mut indices = Vec::new();
        let w = (n + 1) as u32;
        for j in 0..n as u32 {
            for i in 0..n as u32 {
                let a = j * w + i;
                indices.extend([a, a + 1, a + w + 1, a, a + w + 1, a + w]);
            }
        }
        Mesh { name: "Grid".into(), positions, indices, color: [0.5, 0.5, 0.5] }
    }

    #[test]
    #[ignore]
    fn step_perf_100k_triangles() {
        let mesh = grid_mesh(224); // 100 352 triangles
        let t0 = std::time::Instant::now();
        let step = write_step("perf", std::slice::from_ref(&mesh)).unwrap();
        let dt = t0.elapsed();
        eprintln!("100k triangles: {:?}, {} MB", dt, step.len() / 1_000_000);
        assert!(dt.as_secs_f64() < 2.0, "{dt:?}");
        let mut items = rect_outline(200.0, 150.0);
        for k in 0..3000 {
            items.push(via(5.0 + (k % 60) as f64 * 3.2, 5.0 + (k / 60) as f64 * 2.8, 0.3));
        }
        let t0 = std::time::Instant::now();
        let model = board_model(&pcb(items));
        eprintln!("board with 3000 vias: {:?}, {} triangles", t0.elapsed(), model.board.indices.len() / 3);
        assert_watertight(&model.board);
    }
}
