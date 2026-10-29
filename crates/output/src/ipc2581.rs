//! IPC-2581 revision B writer.
//!
//! One self-contained XML file with the BOM, layer stack, board profile,
//! packages, components, logical nets and per-layer features (tracks, pads,
//! vias, pours, graphics, text, drills). Units are millimeters; coordinates
//! are relative to the board's user origin.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use pcbplannercad_core::document::LayerId;
use pcbplannercad_core::font::{TextStyle, stroke_width, text_segments};
use pcbplannercad_core::item::{Item, PadKind, PadShape};
use pcbplannercad_core::library::Footprint;
use pcbplannercad_core::pcb::board::StackKind;
use pcbplannercad_core::pcb::geometry::{all_pads, board_settings, capsule_polygon, circle_polygon, outline_loops, world_graphics, WorldPad};
use pcbplannercad_core::pcb::query::{Obj, ObjKind, RuleSet};
use pcbplannercad_core::pcb::{BoardSettings, PlacedFootprint, Side};
use pcbplannercad_core::{Coord, DocKind, Document, ItemId, Point, layer};

use crate::OutputError;

#[path = "ipc2581_xml.rs"]
mod xml;
use xml::Xml;

const STEP: &str = "board";

/// Millimeters with up to 6 decimals, trailing zeros trimmed.
fn mm(c: Coord) -> String {
    num(c as f64 / 1e6)
}

fn num(v: f64) -> String {
    let s = format!("{v:.6}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" || s.is_empty() { "0".into() } else { s.to_string() }
}

/// Names usable as IPC-2581 ids/references (no spaces or odd characters).
fn ident(s: &str) -> String {
    let out: String = s.chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.') { c } else { '_' }).collect();
    if out.is_empty() { "_".into() } else { out }
}

// ---------------------------------------------------------------------------
// Layers
// ---------------------------------------------------------------------------

struct LayerDef {
    name: String,
    function: &'static str,
    side: &'static str,
    /// Drill span (from, to) copper layer names.
    span: Option<(String, String)>,
    /// Stackup data: thickness, material, εr.
    stack: Option<(Coord, String, f64)>,
}

struct Layers {
    defs: Vec<LayerDef>,
    by_id: HashMap<LayerId, usize>,
}

impl Layers {
    fn new(board: &BoardSettings) -> Layers {
        let mut l = Layers { defs: Vec::new(), by_id: HashMap::new() };
        let copper = board.stackup.copper_layers();
        for s in &board.stackup.layers {
            let (function, side) = match s.kind {
                StackKind::Copper => {
                    let side = match s.layer {
                        Some(id) if id == layer::TOP_COPPER || Some(&id) == copper.first() => "TOP",
                        Some(id) if id == layer::BOTTOM_COPPER || Some(&id) == copper.last() => "BOTTOM",
                        _ => "INTERNAL",
                    };
                    ("SIGNAL", side)
                }
                StackKind::Dielectric => (if s.material.to_ascii_lowercase().contains("prepreg") { "DIELPREG" } else { "DIELCORE" }, "INTERNAL"),
                StackKind::SolderMask => ("SOLDERMASK", side_of(s.layer)),
                StackKind::Silkscreen => ("SILKSCREEN", side_of(s.layer)),
                StackKind::Paste => ("SOLDERPASTE", side_of(s.layer)),
            };
            l.add(s.layer, &s.name, function, side, Some((s.thickness, s.material.clone(), s.epsilon_r)));
        }
        let extra: [(LayerId, &str, &'static str, &'static str); 14] = [
            (layer::TOP_SILK, "Top Overlay", "SILKSCREEN", "TOP"),
            (layer::BOTTOM_SILK, "Bottom Overlay", "SILKSCREEN", "BOTTOM"),
            (layer::TOP_MASK, "Top Solder", "SOLDERMASK", "TOP"),
            (layer::BOTTOM_MASK, "Bottom Solder", "SOLDERMASK", "BOTTOM"),
            (layer::TOP_PASTE, "Top Paste", "SOLDERPASTE", "TOP"),
            (layer::BOTTOM_PASTE, "Bottom Paste", "SOLDERPASTE", "BOTTOM"),
            (layer::BOARD_OUTLINE, "Board Outline", "BOARD_OUTLINE", "ALL"),
            (layer::MECHANICAL_1, "Mechanical 1", "DOCUMENT", "NONE"),
            (layer::KEEP_OUT, "Keep-Out", "DOCUMENT", "NONE"),
            (layer::TOP_COURTYARD, "Top Courtyard", "DOCUMENT", "TOP"),
            (layer::BOTTOM_COURTYARD, "Bottom Courtyard", "DOCUMENT", "BOTTOM"),
            (layer::TOP_ASSEMBLY, "Top Assembly", "ASSEMBLY", "TOP"),
            (layer::BOTTOM_ASSEMBLY, "Bottom Assembly", "ASSEMBLY", "BOTTOM"),
            (layer::TOP_COPPER, "Top Layer", "SIGNAL", "TOP"),
        ];
        for (id, name, f, side) in extra {
            if !l.by_id.contains_key(&id) {
                l.add(Some(id), name, f, side, None);
            }
        }
        if !l.by_id.contains_key(&layer::BOTTOM_COPPER) && copper.len() != 1 {
            l.add(Some(layer::BOTTOM_COPPER), "Bottom Layer", "SIGNAL", "BOTTOM", None);
        }
        l
    }

    fn add(&mut self, id: Option<LayerId>, name: &str, function: &'static str, side: &'static str, stack: Option<(Coord, String, f64)>) -> usize {
        let base = ident(name);
        let mut name = base.clone();
        let mut n = 2;
        while self.defs.iter().any(|d| d.name == name) {
            name = format!("{base}_{n}");
            n += 1;
        }
        self.defs.push(LayerDef { name, function, side, span: None, stack });
        let i = self.defs.len() - 1;
        if let Some(id) = id {
            self.by_id.entry(id).or_insert(i);
        }
        i
    }

    /// Index of a drawing layer, creating a documentation layer for unknown ids.
    fn index(&mut self, id: LayerId) -> usize {
        match self.by_id.get(&id) {
            Some(i) => *i,
            None => self.add(Some(id), &format!("Layer {}", id.0), "DOCUMENT", "NONE", None),
        }
    }

    fn name(&mut self, id: LayerId) -> String {
        let i = self.index(id);
        self.defs[i].name.clone()
    }
}

fn side_of(id: Option<LayerId>) -> &'static str {
    match id {
        Some(l) if l == layer::BOTTOM_SILK || l == layer::BOTTOM_MASK || l == layer::BOTTOM_PASTE => "BOTTOM",
        _ => "TOP",
    }
}

// ---------------------------------------------------------------------------
// Dictionaries
// ---------------------------------------------------------------------------

#[derive(Default)]
struct Dicts {
    /// key → (id, xml of the primitive element)
    standard: Vec<(String, String, PrimShape)>,
    std_index: HashMap<String, usize>,
    lines: BTreeMap<Coord, String>,
}

#[derive(Clone, Copy)]
enum PrimShape {
    Circle(Coord),
    Rect(Coord, Coord),
    Oval(Coord, Coord),
    RoundRect(Coord, Coord, Coord),
}

impl Dicts {
    fn prim(&mut self, shape: PrimShape) -> String {
        let key = match shape {
            PrimShape::Circle(d) => format!("C{d}"),
            PrimShape::Rect(w, h) => format!("R{w}x{h}"),
            PrimShape::Oval(w, h) => format!("O{w}x{h}"),
            PrimShape::RoundRect(w, h, r) => format!("RR{w}x{h}r{r}"),
        };
        if let Some(i) = self.std_index.get(&key) {
            return self.standard[*i].1.clone();
        }
        let id = format!("PRIM_{}", self.standard.len() + 1);
        self.std_index.insert(key.clone(), self.standard.len());
        self.standard.push((key, id.clone(), shape));
        id
    }

    /// Primitive of a pad grown by `inflate`.
    fn pad_prim(&mut self, shape: PadShape, w: Coord, h: Coord, corner_ratio: f64, inflate: Coord) -> String {
        let (w, h) = ((w + 2 * inflate).max(1), (h + 2 * inflate).max(1));
        let p = match shape {
            PadShape::Circle => PrimShape::Circle(w),
            PadShape::Oval if w == h => PrimShape::Circle(w),
            PadShape::Oval => PrimShape::Oval(w, h),
            PadShape::Rect if inflate > 0 => PrimShape::RoundRect(w, h, inflate),
            PadShape::Rect => PrimShape::Rect(w, h),
            PadShape::RoundRect => {
                let r = (((w.min(h) - 2 * inflate) as f64 * corner_ratio) as Coord + inflate).clamp(0, w.min(h) / 2);
                if r == 0 { PrimShape::Rect(w, h) } else { PrimShape::RoundRect(w, h, r) }
            }
        };
        self.prim(p)
    }

    fn line(&mut self, width: Coord) -> String {
        let n = self.lines.len() + 1;
        self.lines.entry(width.max(0)).or_insert_with(|| format!("LINE_{n}")).clone()
    }
}

// ---------------------------------------------------------------------------
// Features
// ---------------------------------------------------------------------------

enum Feat {
    Line { a: Point, b: Point, w: Coord },
    Arc { c: Point, r: Coord, start: f64, sweep: f64, w: Coord },
    Polyline { pts: Vec<Point>, w: Coord },
    Contour { outer: Vec<Point>, holes: Vec<Vec<Point>> },
    Pad { pos: Point, rot: f64, prim: String, pin: Option<(String, String)>, via: bool },
    Hole { pos: Point, d: Coord, plating: &'static str },
    Slot { outline: Vec<Point>, plating: &'static str },
}

impl Feat {
    /// Sets group features by category so each Set holds one kind.
    fn category(&self) -> u8 {
        match self {
            Feat::Pad { via: false, .. } => 0,
            Feat::Pad { via: true, .. } => 1,
            Feat::Hole { .. } | Feat::Slot { .. } => 2,
            _ => 3,
        }
    }
}

#[derive(Default)]
struct Features {
    /// layer index → net → features
    by_layer: BTreeMap<usize, BTreeMap<String, Vec<Feat>>>,
}

impl Features {
    fn push(&mut self, layer: usize, net: &str, f: Feat) {
        self.by_layer.entry(layer).or_default().entry(net.to_string()).or_default().push(f);
    }
}

fn graphic_feats(item: &Item, out: &mut Vec<(LayerId, Feat)>) {
    match item {
        Item::Line(l) => out.push((l.layer, Feat::Line { a: l.a, b: l.b, w: l.width })),
        Item::Arc(a) => out.push((a.layer, Feat::Arc { c: a.center, r: a.radius, start: a.start_angle, sweep: a.sweep, w: a.width })),
        Item::Circle(c) => {
            if c.fill != pcbplannercad_core::item::FillStyle::None {
                out.push((c.layer, Feat::Contour { outer: circle_polygon(c.center, c.radius + c.width / 2, 64), holes: vec![] }));
            } else {
                out.push((c.layer, Feat::Arc { c: c.center, r: c.radius, start: 0.0, sweep: 360.0, w: c.width }));
            }
        }
        Item::Rect(r) => {
            let pts = vec![r.a, Point::new(r.b.x, r.a.y), r.b, Point::new(r.a.x, r.b.y)];
            if r.fill != pcbplannercad_core::item::FillStyle::None {
                out.push((r.layer, Feat::Contour { outer: pts.clone(), holes: vec![] }));
            }
            if r.width > 0 || r.fill == pcbplannercad_core::item::FillStyle::None {
                let mut closed = pts;
                closed.push(r.a);
                out.push((r.layer, Feat::Polyline { pts: closed, w: r.width }));
            }
        }
        Item::Polyline(p) => {
            if p.points.len() < 2 {
                return;
            }
            if p.closed && p.fill != pcbplannercad_core::item::FillStyle::None && p.points.len() >= 3 {
                out.push((p.layer, Feat::Contour { outer: p.points.clone(), holes: vec![] }));
            }
            let mut pts = p.points.clone();
            if p.closed {
                pts.push(p.points[0]);
            }
            out.push((p.layer, Feat::Polyline { pts, w: p.width }));
        }
        Item::Dimension(d) => d.primitives().iter().for_each(|prim| graphic_feats(prim, out)),
        Item::Text(t) => {
            let w = stroke_width(t.height);
            for (a, b) in text_segments(&t.text, t.pos, TextStyle::of(t)) {
                out.push((t.layer, Feat::Line { a, b, w }));
            }
        }
        _ => {}
    }
}

fn slot_outline(pos: Point, w: Coord, h: Coord, rot: f64) -> Vec<Point> {
    let (long, short) = (w.max(h), w.min(h));
    let along = if w >= h { rot } else { rot + 90.0 };
    let half = (long - short) as f64 / 2.0;
    let (s, c) = along.to_radians().sin_cos();
    let d = Point::new((half * c).round() as Coord, (half * s).round() as Coord);
    capsule_polygon(Point::new(pos.x - d.x, pos.y - d.y), Point::new(pos.x + d.x, pos.y + d.y), short, 0)
}

fn point_in_polygon(p: Point, poly: &[Point]) -> bool {
    let mut inside = false;
    let n = poly.len();
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        if (a.y > p.y) != (b.y > p.y) {
            let x = a.x as f64 + (p.y - a.y) as f64 * (b.x - a.x) as f64 / (b.y - a.y) as f64;
            if (p.x as f64) < x {
                inside = !inside;
            }
        }
    }
    inside
}

// ---------------------------------------------------------------------------
// Writer
// ---------------------------------------------------------------------------

struct Ctx {
    origin: Point,
    dicts: Dicts,
}

impl Ctx {
    fn x(&self, p: Point) -> String {
        mm(p.x - self.origin.x)
    }

    fn y(&self, p: Point) -> String {
        mm(p.y - self.origin.y)
    }

    fn location(&self, w: &mut Xml, p: Point) {
        w.leaf("Location", &[("x", self.x(p)), ("y", self.y(p))]);
    }

    /// `<Polygon>`-style point list (closed).
    fn poly_body(&self, w: &mut Xml, pts: &[Point]) {
        let Some(first) = pts.first() else { return };
        w.leaf("PolyBegin", &[("x", self.x(*first)), ("y", self.y(*first))]);
        for p in &pts[1..] {
            w.leaf("PolyStepSegment", &[("x", self.x(*p)), ("y", self.y(*p))]);
        }
        if pts.last() != Some(first) {
            w.leaf("PolyStepSegment", &[("x", self.x(*first)), ("y", self.y(*first))]);
        }
    }

    fn polygon(&self, w: &mut Xml, tag: &str, pts: &[Point]) {
        w.open(tag, &[]);
        self.poly_body(w, pts);
        w.close();
    }

    fn feature(&mut self, w: &mut Xml, f: &Feat, hole_no: &mut usize) {
        match f {
            Feat::Line { a, b, w: width } => {
                let id = self.dicts.line(*width);
                w.open("Features", &[]);
                w.open("Line", &[("startX", self.x(*a)), ("startY", self.y(*a)), ("endX", self.x(*b)), ("endY", self.y(*b))]);
                w.leaf("LineDescRef", &[("id", id)]);
                w.close();
                w.close();
            }
            Feat::Arc { c, r, start, sweep, w: width } => {
                let id = self.dicts.line(*width);
                // Full circles are written as two half arcs (start == end is ambiguous).
                let parts: Vec<(f64, f64)> = if sweep.abs() >= 359.999 { vec![(*start, 180.0), (start + 180.0, 180.0)] } else { vec![(*start, *sweep)] };
                for (s, sw) in parts {
                    let at = |deg: f64| {
                        let (sn, cs) = deg.to_radians().sin_cos();
                        Point::new(c.x + (*r as f64 * cs).round() as Coord, c.y + (*r as f64 * sn).round() as Coord)
                    };
                    let (a, b) = (at(s), at(s + sw));
                    w.open("Features", &[]);
                    w.open(
                        "Arc",
                        &[
                            ("startX", self.x(a)),
                            ("startY", self.y(a)),
                            ("endX", self.x(b)),
                            ("endY", self.y(b)),
                            ("centerX", self.x(*c)),
                            ("centerY", self.y(*c)),
                            ("clockwise", (sw < 0.0).to_string()),
                        ],
                    );
                    w.leaf("LineDescRef", &[("id", id.clone())]);
                    w.close();
                    w.close();
                }
            }
            Feat::Polyline { pts, w: width } => {
                let id = self.dicts.line(*width);
                w.open("Features", &[]);
                w.open("Polyline", &[]);
                let first = pts[0];
                w.leaf("PolyBegin", &[("x", self.x(first)), ("y", self.y(first))]);
                for p in &pts[1..] {
                    w.leaf("PolyStepSegment", &[("x", self.x(*p)), ("y", self.y(*p))]);
                }
                w.leaf("LineDescRef", &[("id", id)]);
                w.close();
                w.close();
            }
            Feat::Contour { outer, holes } => {
                if outer.len() < 3 {
                    return;
                }
                w.open("Features", &[]);
                w.open("Contour", &[]);
                self.polygon(w, "Polygon", outer);
                for h in holes.iter().filter(|h| h.len() >= 3) {
                    self.polygon(w, "Cutout", h);
                }
                w.close();
                w.close();
            }
            Feat::Pad { pos, rot, prim, pin, .. } => {
                w.open("Pad", &[]);
                if *rot != 0.0 {
                    w.leaf("Xform", &[("rotation", num(*rot))]);
                }
                self.location(w, *pos);
                w.leaf("StandardPrimitiveRef", &[("id", prim.clone())]);
                if let Some((comp, pin)) = pin {
                    w.leaf("PinRef", &[("componentRef", comp.clone()), ("pin", pin.clone())]);
                }
                w.close();
            }
            Feat::Hole { pos, d, plating } => {
                *hole_no += 1;
                w.leaf(
                    "Hole",
                    &[
                        ("name", format!("H{hole_no}")),
                        ("diameter", mm(*d)),
                        ("platingStatus", plating.to_string()),
                        ("plusTol", "0".into()),
                        ("minusTol", "0".into()),
                        ("x", self.x(*pos)),
                        ("y", self.y(*pos)),
                    ],
                );
            }
            Feat::Slot { outline, plating } => {
                *hole_no += 1;
                let id = self.dicts.line(0);
                w.open("SlotCavity", &[("name", format!("S{hole_no}")), ("platingStatus", plating.to_string()), ("plusTol", "0".into()), ("minusTol", "0".into())]);
                w.open("Outline", &[]);
                self.polygon(w, "Polygon", outline);
                w.leaf("LineDescRef", &[("id", id)]);
                w.close();
                w.close();
            }
        }
    }
}

fn mpn(fp: &PlacedFootprint) -> Option<&str> {
    ["MPN", "Manufacturer Part Number", "Part Number", "PartNumber", "Mfr Part Number"]
        .iter()
        .find_map(|k| fp.parameters.iter().find(|(n, v)| n.eq_ignore_ascii_case(k) && !v.trim().is_empty()).map(|(_, v)| v.as_str()))
}

fn utc_now() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()) as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Civil-from-days (Howard Hinnant).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

/// Writes the board as an IPC-2581 rev B XML document.
pub fn write_ipc2581(doc: &Document, project_name: &str) -> Result<String, OutputError> {
    if doc.kind() != DocKind::Pcb {
        return Err(OutputError::Invalid("IPC-2581 export needs a PCB document".into()));
    }
    let board = board_settings(doc).map(|(_, b)| b.clone()).unwrap_or_default();
    let rules = RuleSet::new(&board);
    let mut layers = Layers::new(&board);
    let mut ctx = Ctx { origin: board.origin, dicts: Dicts::default() };
    let mut feats = Features::default();

    let footprints: Vec<(ItemId, &PlacedFootprint)> = doc.items().filter_map(|(id, i)| if let Item::Footprint(f) = i { Some((id, f)) } else { None }).collect();
    let pads: Vec<WorldPad> = all_pads(doc, &board);
    let copper = board.stackup.copper_layers();
    let (top_cu, bottom_cu) = (copper.first().copied().unwrap_or(layer::TOP_COPPER), copper.last().copied().unwrap_or(layer::BOTTOM_COPPER));

    // ----- nets -----
    let mut nets: BTreeMap<String, BTreeSet<(String, String)>> = BTreeMap::new();
    for p in &pads {
        if !p.net.is_empty() && !p.number.is_empty() {
            nets.entry(p.net.clone()).or_default().insert((p.designator.clone(), p.number.clone()));
        }
    }
    for (_, item) in doc.items() {
        let net = match item {
            Item::Track(t) => &t.net,
            Item::ArcTrack(t) => &t.net,
            Item::Via(v) => &v.net,
            Item::Zone(z) if !z.keepout => &z.net,
            _ => continue,
        };
        if !net.is_empty() {
            nets.entry(net.clone()).or_default();
        }
    }

    // ----- drill layers -----
    let pth = {
        let i = layers.add(None, "Drill PTH", "DRILL", "ALL", None);
        layers.defs[i].span = Some((layers.name(top_cu), layers.name(bottom_cu)));
        i
    };
    let mut npth: Option<usize> = None;
    let mut via_spans: HashMap<(LayerId, LayerId), usize> = HashMap::new();

    // ----- copper, mask, paste from pads -----
    for p in &pads {
        let pin = (!p.number.is_empty()).then(|| (p.designator.clone(), p.number.clone()));
        for &l in &p.copper {
            let land=p.for_layer(l);
            let prim = ctx.dicts.pad_prim(land.shape, land.width, land.height, land.corner_ratio, 0);
            let li = layers.index(l);
            feats.push(li, &p.net, Feat::Pad { pos: p.pos, rot: p.rotation, prim: prim.clone(), pin: pin.clone(), via: false });
        }
        let obj = Obj::new(ObjKind::Pad, &p.net, p.copper.first().copied());
        let sides: Vec<bool> = match p.kind {
            PadKind::Smd => vec![p.bottom],
            _ => vec![false, true],
        };
        let mask_e = rules.mask_expansion(&obj);
        for &bottom in &sides {
            let mask = layers.index(if bottom { layer::BOTTOM_MASK } else { layer::TOP_MASK });
            let land=p.for_layer(if bottom {bottom_cu}else{top_cu});
            let mp = ctx.dicts.pad_prim(land.shape, land.width, land.height, land.corner_ratio, mask_e);
            feats.push(mask, "", Feat::Pad { pos: p.pos, rot: p.rotation, prim: mp, pin: None, via: false });
        }
        if p.kind == PadKind::Smd {
            let e = rules.paste_expansion(&obj);
            if p.width + 2 * e > 0 && p.height + 2 * e > 0 {
                let paste = layers.index(if p.bottom { layer::BOTTOM_PASTE } else { layer::TOP_PASTE });
                let pp = ctx.dicts.pad_prim(p.shape, p.width, p.height, p.corner_ratio, e);
                feats.push(paste, "", Feat::Pad { pos: p.pos, rot: p.rotation, prim: pp, pin: None, via: false });
            }
        }
        // Drill.
        let drill = match (p.kind, p.drill) {
            (PadKind::Smd, _) => None,
            (_, Some(d)) => Some((d.width, d.height)),
            (PadKind::Npth, None) => Some((p.width, p.height.min(p.width))),
            (PadKind::Tht, None) => None,
        };
        if let Some((dw, dh)) = drill {
            let (li, plating, net) = if p.kind == PadKind::Npth {
                let i = match npth {
                    Some(i) => i,
                    None => {
                        let i = layers.add(None, "Drill NPTH", "DRILL", "ALL", None);
                        layers.defs[i].span = Some((layers.name(top_cu), layers.name(bottom_cu)));
                        npth = Some(i);
                        i
                    }
                };
                (i, "NONPLATED", "")
            } else {
                (pth, "PLATED", p.net.as_str())
            };
            let f = if dw == dh { Feat::Hole { pos: p.pos, d: dw, plating } } else { Feat::Slot { outline: slot_outline(p.pos, dw, dh, p.rotation), plating } };
            feats.push(li, net, f);
        }
    }

    // ----- board items -----
    let mut graphics: Vec<(LayerId, Feat)> = Vec::new();
    for (_, item) in doc.items() {
        match item {
            Item::Track(t) => {
                let li = layers.index(t.layer);
                feats.push(li, &t.net, Feat::Line { a: t.a, b: t.b, w: t.width });
            }
            Item::ArcTrack(t) => {
                let li = layers.index(t.layer);
                feats.push(li, &t.net, Feat::Arc { c: t.center, r: t.radius, start: t.start_angle, sweep: t.sweep, w: t.width });
            }
            Item::Via(v) => {
                let span = board.stackup.span(v.from_layer, v.to_layer);
                let prim = ctx.dicts.prim(PrimShape::Circle(v.diameter));
                for l in &span {
                    let li = layers.index(*l);
                    feats.push(li, &v.net, Feat::Pad { pos: v.pos, rot: 0.0, prim: prim.clone(), pin: None, via: true });
                }
                let (from, to) = (span.first().copied().unwrap_or(top_cu), span.last().copied().unwrap_or(bottom_cu));
                let li = if (from, to) == (top_cu, bottom_cu) {
                    pth
                } else {
                    *via_spans.entry((from, to)).or_insert_with(|| {
                        let (fname, tname) = (layers.name(from), layers.name(to));
                        let i = layers.add(None, &format!("Drill {fname}-{tname}"), "DRILL", "ALL", None);
                        layers.defs[i].span = Some((fname, tname));
                        i
                    })
                };
                feats.push(li, &v.net, Feat::Hole { pos: v.pos, d: v.drill, plating: "VIA" });
            }
            Item::Zone(z) if !z.keepout => {
                let li = layers.index(z.layer);
                for poly in &z.fill {
                    feats.push(li, &z.net, Feat::Contour { outer: poly.outer.clone(), holes: poly.holes.clone() });
                }
            }
            Item::Footprint(fp) => {
                if let Some(f) = doc.cached_footprint(&fp.footprint) {
                    for g in world_graphics(fp, f) {
                        graphic_feats(&g, &mut graphics);
                    }
                }
            }
            other => graphic_feats(other, &mut graphics),
        }
    }
    for (l, f) in graphics {
        let li = layers.index(l);
        feats.push(li, "", f);
    }

    // ----- profile -----
    let loops = outline_loops(doc, 10_000);
    let (profile, cutouts): (Vec<Point>, Vec<Vec<Point>>) = match loops.split_first() {
        Some((outer, rest)) => (outer.clone(), rest.iter().filter(|l| l.first().is_some_and(|p| point_in_polygon(*p, outer))).cloned().collect()),
        None => {
            // No outline: use the extents of the copper.
            let mut pts: Vec<Point> = pads.iter().map(|p| p.pos).collect();
            for (_, i) in doc.items() {
                match i {
                    Item::Track(t) => pts.extend([t.a, t.b]),
                    Item::Via(v) => pts.push(v.pos),
                    Item::Zone(z) => pts.extend(z.outline.iter().copied()),
                    _ => {}
                }
            }
            let Some(bb) = pcbplannercad_core::geom::BBox::from_points(pts) else {
                return Err(OutputError::Invalid("board has no outline and no copper".into()));
            };
            let m = 1_000_000;
            let (a, b) = (Point::new(bb.min.x - m, bb.min.y - m), Point::new(bb.max.x + m, bb.max.y + m));
            (vec![a, Point::new(b.x, a.y), b, Point::new(a.x, b.y)], vec![])
        }
    };

    // ----- packages -----
    struct Pkg<'a> {
        name: String,
        fp: Option<&'a Footprint>,
    }
    let mut packages: BTreeMap<String, Pkg> = BTreeMap::new();
    let mut used_pkg_names: BTreeSet<String> = BTreeSet::new();
    for (_, fp) in &footprints {
        if !packages.contains_key(&fp.footprint) {
            let base = ident(&fp.footprint);
            let mut name = base.clone();
            let mut n = 2;
            while used_pkg_names.contains(&name) {
                name = format!("{base}_{n}");
                n += 1;
            }
            used_pkg_names.insert(name.clone());
            packages.insert(fp.footprint.clone(), Pkg { name, fp: doc.cached_footprint(&fp.footprint) });
        }
    }

    // ================= Ecad (written first to collect dictionaries) =================
    let mut e = Xml::new(1);
    e.open("Ecad", &[("name", project_name.to_string())]);
    e.open("CadHeader", &[("units", "MILLIMETER".into())]);
    for d in &layers.defs {
        if let Some((_, material, er)) = &d.stack {
            if material.is_empty() && *er == 0.0 {
                continue;
            }
            e.open("Spec", &[("name", format!("SPEC_{}", d.name))]);
            if !material.is_empty() {
                e.open("General", &[("type", "MATERIAL".into())]);
                e.leaf("Property", &[("text", material.clone())]);
                e.close();
            }
            if *er > 0.0 {
                e.open("Dielectric", &[("type", "DIELECTRIC_CONSTANT".into())]);
                e.leaf("Property", &[("value", num(*er))]);
                e.close();
            }
            e.close();
        }
    }
    e.close(); // CadHeader
    e.open("CadData", &[]);
    for d in &layers.defs {
        let at = vec![("name", d.name.clone()), ("layerFunction", d.function.into()), ("side", d.side.into()), ("polarity", "POSITIVE".into())];
        match &d.span {
            Some((from, to)) => {
                e.open("Layer", &at);
                e.leaf("Span", &[("fromLayer", from.clone()), ("toLayer", to.clone())]);
                e.close();
            }
            None => e.leaf("Layer", &at),
        }
    }
    // Stackup.
    let total = board.stackup.thickness();
    e.open("Stackup", &[("name", "PRIMARY".into()), ("overallThickness", mm(total)), ("whereMeasured", "METAL".into()), ("tolPlus", "0".into()), ("tolMinus", "0".into())]);
    e.open("StackupGroup", &[("name", "PRIMARY_GROUP".into()), ("thickness", mm(total)), ("tolPlus", "0".into()), ("tolMinus", "0".into())]);
    let mut seq = 0;
    for d in &layers.defs {
        if let Some((t, material, er)) = &d.stack {
            seq += 1;
            let at = vec![("layerOrGroupRef", d.name.clone()), ("thickness", mm(*t)), ("tolPlus", "0".into()), ("tolMinus", "0".into()), ("sequence", seq.to_string())];
            if material.is_empty() && *er == 0.0 {
                e.leaf("StackupLayer", &at);
            } else {
                e.open("StackupLayer", &at);
                e.leaf("SpecRef", &[("id", format!("SPEC_{}", d.name))]);
                e.close();
            }
        }
    }
    e.close();
    e.close(); // Stackup

    e.open("Step", &[("name", STEP.into())]);
    e.leaf("Datum", &[("x", "0".into()), ("y", "0".into())]);
    e.open("Profile", &[]);
    ctx.polygon(&mut e, "Polygon", &profile);
    for c in &cutouts {
        ctx.polygon(&mut e, "Cutout", c);
    }
    e.close();

    // Packages.
    let outline_id = ctx.dicts.line(0);
    for pkg in packages.values() {
        let pad_items: Vec<&pcbplannercad_core::item::Pad> = pkg.fp.map_or(vec![], |f| f.items.iter().filter_map(|i| if let Item::Pad(p) = i { Some(p) } else { None }).collect());
        let pin_one = pad_items.iter().find(|p| !p.number.is_empty()).map_or("1".to_string(), |p| p.number.clone());
        e.open("Package", &[("name", pkg.name.clone()), ("type", "OTHER".into()), ("pinOne", pin_one), ("pinOneOrientation", "OTHER".into()), ("height", "0".into())]);
        // Outline: courtyard extents, else pad/graphic extents.
        let mut court: Vec<Point> = Vec::new();
        let mut any: Vec<Point> = Vec::new();
        if let Some(f) = pkg.fp {
            for i in &f.items {
                let mut g = Vec::new();
                graphic_feats(i, &mut g);
                for (l, feat) in g {
                    let pts: Vec<Point> = match feat {
                        Feat::Line { a, b, .. } => vec![a, b],
                        Feat::Arc { c, r, .. } => vec![c.translated(-r, -r), c.translated(r, r)],
                        Feat::Polyline { pts, .. } | Feat::Contour { outer: pts, .. } => pts,
                        _ => vec![],
                    };
                    if l == layer::TOP_COURTYARD {
                        court.extend(pts.iter().copied());
                    }
                    any.extend(pts);
                }
                if let Item::Pad(p) = i {
                    let r = p.width.max(p.height) / 2;
                    any.extend([p.pos.translated(-r, -r), p.pos.translated(r, r)]);
                }
            }
        }
        let bb = pcbplannercad_core::geom::BBox::from_points(if court.is_empty() { any } else { court })
            .unwrap_or(pcbplannercad_core::geom::BBox { min: Point::new(-50_000, -50_000), max: Point::new(50_000, 50_000) });
        let rect = [bb.min, Point::new(bb.max.x, bb.min.y), bb.max, Point::new(bb.min.x, bb.max.y)];
        // Package geometry is local: no board-origin shift.
        let local = Ctx { origin: Point::default(), dicts: Dicts::default() };
        e.open("Outline", &[]);
        local.polygon(&mut e, "Polygon", &rect);
        e.leaf("LineDescRef", &[("id", outline_id.clone())]);
        e.close();
        let mut seen = BTreeSet::new();
        for p in pad_items {
            if p.number.is_empty() || !seen.insert(p.number.clone()) {
                continue;
            }
            let (ty, el) = match p.kind {
                PadKind::Smd => ("SURFACE", "ELECTRICAL"),
                PadKind::Tht => ("THRU", "ELECTRICAL"),
                PadKind::Npth => ("THRU", "MECHANICAL"),
            };
            e.open("Pin", &[("number", p.number.clone()), ("type", ty.into()), ("electricalType", el.into())]);
            if p.rotation != 0.0 {
                e.leaf("Xform", &[("rotation", num(p.rotation))]);
            }
            local.location(&mut e, p.pos);
            let prim = ctx.dicts.pad_prim(p.shape, p.width, p.height, p.corner_ratio, 0);
            e.leaf("StandardPrimitiveRef", &[("id", prim)]);
            e.close();
        }
        e.close();
    }

    // Components.
    for (_, fp) in &footprints {
        let pkg = &packages[&fp.footprint];
        let kinds: Vec<PadKind> = pkg.fp.map_or(vec![], |f| f.items.iter().filter_map(|i| if let Item::Pad(p) = i { Some(p.kind) } else { None }).collect());
        let mount = if kinds.contains(&PadKind::Tht) {
            "THMT"
        } else if kinds.contains(&PadKind::Smd) {
            "SMT"
        } else {
            "OTHER"
        };
        let side_layer = if fp.side == Side::Bottom { bottom_cu } else { top_cu };
        let part = if !fp.value.is_empty() { fp.value.clone() } else if !fp.component.is_empty() { fp.component.clone() } else { fp.footprint.clone() };
        e.open(
            "Component",
            &[("refDes", fp.designator.clone()), ("packageRef", pkg.name.clone()), ("layerRef", layers.name(side_layer)), ("part", part), ("mountType", mount.into())],
        );
        if fp.rotation != 0.0 || fp.side == Side::Bottom {
            let mut at = vec![("rotation", num(fp.rotation))];
            if fp.side == Side::Bottom {
                at.push(("mirror", "true".into()));
            }
            e.leaf("Xform", &at);
        }
        ctx.location(&mut e, fp.pos);
        e.close();
    }

    // Logical nets.
    for (net, pins) in &nets {
        if pins.is_empty() {
            e.leaf("LogicalNet", &[("name", net.clone())]);
            continue;
        }
        e.open("LogicalNet", &[("name", net.clone())]);
        for (comp, pin) in pins {
            e.leaf("PinRef", &[("componentRef", comp.clone()), ("pin", pin.clone())]);
        }
        e.close();
    }

    // Layer features, in layer order.
    let mut hole_no = 0;
    let order: Vec<usize> = (0..layers.defs.len()).collect();
    for li in order {
        let Some(by_net) = feats.by_layer.get(&li) else { continue };
        e.open("LayerFeature", &[("layerRef", layers.defs[li].name.clone())]);
        for (net, list) in by_net {
            for cat in 0..4u8 {
                let items: Vec<&Feat> = list.iter().filter(|f| f.category() == cat).collect();
                if items.is_empty() {
                    continue;
                }
                let mut at = Vec::new();
                if !net.is_empty() {
                    at.push(("net", net.clone()));
                }
                at.push(("polarity", "POSITIVE".into()));
                match cat {
                    0 => at.push(("padUsage", "TERMINATION".into())),
                    1 => at.push(("padUsage", "VIA".into())),
                    _ => {}
                }
                e.open("Set", &at);
                for f in items {
                    ctx.feature(&mut e, f, &mut hole_no);
                }
                e.close();
            }
        }
        e.close();
    }
    e.close(); // Step
    e.close(); // CadData
    e.close(); // Ecad

    // ================= Document =================
    let mut w = Xml::new(0);
    w.raw("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    w.open("IPC-2581", &[("revision", "B".into()), ("xmlns", "http://webstds.ipc.org/2581".into()), ("xmlns:xsi", "http://www.w3.org/2001/XMLSchema-instance".into())]);
    w.open("Content", &[("roleRef", "Owner".into())]);
    w.leaf("FunctionMode", &[("mode", "ASSEMBLY".into()), ("level", "3".into())]);
    w.leaf("StepRef", &[("name", STEP.into())]);
    for d in &layers.defs {
        w.leaf("LayerRef", &[("name", d.name.clone())]);
    }
    w.leaf("BomRef", &[("name", "BOM".into())]);
    w.open("DictionaryStandard", &[("units", "MILLIMETER".into())]);
    for (_, id, shape) in &ctx.dicts.standard {
        w.open("EntryStandard", &[("id", id.clone())]);
        match *shape {
            PrimShape::Circle(d) => w.leaf("Circle", &[("diameter", mm(d))]),
            PrimShape::Rect(a, b) => w.leaf("RectCenter", &[("width", mm(a)), ("height", mm(b))]),
            PrimShape::Oval(a, b) => w.leaf("Oval", &[("width", mm(a)), ("height", mm(b))]),
            PrimShape::RoundRect(a, b, r) => w.leaf(
                "RectRound",
                &[
                    ("width", mm(a)),
                    ("height", mm(b)),
                    ("radius", mm(r)),
                    ("upperRight", "true".into()),
                    ("upperLeft", "true".into()),
                    ("lowerRight", "true".into()),
                    ("lowerLeft", "true".into()),
                ],
            ),
        }
        w.close();
    }
    w.close();
    w.leaf("DictionaryUser", &[("units", "MILLIMETER".into())]);
    w.open("DictionaryLineDesc", &[("units", "MILLIMETER".into())]);
    for (width, id) in &ctx.dicts.lines {
        w.open("EntryLineDesc", &[("id", id.clone())]);
        w.leaf("LineDesc", &[("lineEnd", "ROUND".into()), ("lineWidth", mm(*width))]);
        w.close();
    }
    w.close();
    w.open("DictionaryColor", &[]);
    for (id, hex) in [("COLOR_COPPER", "#b87333"), ("COLOR_MASK", board.mask_color.as_str()), ("COLOR_SILK", board.silk_color.as_str())] {
        let rgb = parse_hex(hex).unwrap_or((128, 128, 128));
        w.open("EntryColor", &[("id", id.into())]);
        w.leaf("Color", &[("r", rgb.0.to_string()), ("g", rgb.1.to_string()), ("b", rgb.2.to_string())]);
        w.close();
    }
    w.close();
    w.close(); // Content

    w.open("LogisticHeader", &[]);
    w.leaf("Role", &[("id", "Owner".into()), ("roleFunction", "SENDER".into())]);
    w.leaf("Enterprise", &[("id", "pCAD".into()), ("code", "NONE".into())]);
    w.leaf("Person", &[("name", "pCAD".into()), ("enterpriseRef", "pCAD".into()), ("roleRef", "Owner".into())]);
    w.close();
    let now = utc_now();
    w.open("HistoryRecord", &[("number", "1".into()), ("origination", now.clone()), ("software", "pCAD".into()), ("lastChange", now)]);
    w.open("FileRevision", &[("fileRevisionId", "1".into()), ("comment", "".into()), ("label", "".into())]);
    w.open("SoftwarePackage", &[("name", "pCAD".into()), ("revision", env!("CARGO_PKG_VERSION").into()), ("vendor", "pCAD".into())]);
    w.leaf("Certification", &[("certificationStatus", "SELFTEST".into())]);
    w.close();
    w.close();
    w.close();

    // BOM: one item per (part number, value, footprint, parameters).
    w.open("Bom", &[("name", "BOM".into())]);
    w.open("BomHeader", &[("assembly", project_name.to_string()), ("revision", "1".into())]);
    w.leaf("StepRef", &[("name", STEP.into())]);
    w.close();
    type BomKey = (String, String, String, Vec<(String, String)>);
    let mut groups: BTreeMap<BomKey, Vec<&PlacedFootprint>> = BTreeMap::new();
    for (_, fp) in footprints.iter().filter(|(_,fp)|crate::assembly::include_in_bom(fp)) {
        let params: Vec<(String, String)> = fp.parameters.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        let key = (mpn(fp).unwrap_or(&fp.component).to_string(), fp.value.clone(), fp.footprint.clone(), params);
        groups.entry(key).or_default().push(fp);
    }
    for ((pn, value, footprint, params), list) in &groups {
        let oem = if !pn.is_empty() { pn.clone() } else { format!("{value} {footprint}").trim().to_string() };
        let pkg = &packages[footprint];
        let pin_count = pkg.fp.map_or(0, |f| f.items.iter().filter(|i| matches!(i, Item::Pad(_))).count());
        w.open("BomItem", &[("OEMDesignNumberRef", oem), ("quantity", list.len().to_string()), ("pinCount", pin_count.to_string()), ("category", "ELECTRICAL".into())]);
        for fp in list {
            let side_layer = if fp.side == Side::Bottom { bottom_cu } else { top_cu };
            w.leaf("RefDes", &[("name", fp.designator.clone()), ("packageRef", pkg.name.clone()), ("populate", "true".into()), ("layerRef", layers.name(side_layer))]);
        }
        w.open("Characteristics", &[("category", "ELECTRICAL".into())]);
        let mut chars: Vec<(String, String)> = vec![("Value".into(), value.clone()), ("Footprint".into(), footprint.clone())];
        if let Some(c) = list.first().map(|f| f.component.clone()).filter(|c| !c.is_empty()) {
            chars.push(("Component".into(), c));
        }
        chars.extend(params.iter().cloned());
        for (k, v) in chars {
            w.leaf("Textual", &[("definitionSource", "pCAD".into()), ("textualCharacteristicName", k), ("textualCharacteristicValue", v)]);
        }
        w.close();
        w.close();
    }
    w.close(); // Bom

    w.raw(&e.finish());
    w.close(); // IPC-2581
    Ok(w.finish())
}

fn parse_hex(s: &str) -> Option<(u8, u8, u8)> {
    let h = s.trim().strip_prefix('#')?;
    if h.len() != 6 || !h.is_ascii() {
        return None;
    }
    let p = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
    Some((p(0)?, p(2)?, p(4)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pcbplannercad_core::item::{Drill, Line, Pad, Text};
    use pcbplannercad_core::pcb::{Track, Via, Zone, ZonePolygon};
    use pcbplannercad_core::units::mm as nm;

    /// Minimal XML well-formedness check: balanced tags, quoted attributes,
    /// escaped text. Returns the element names in document order.
    fn check_xml(s: &str) -> Vec<String> {
        let mut stack: Vec<String> = Vec::new();
        let mut names = Vec::new();
        let mut rest = s;
        let mut roots = 0;
        while let Some(i) = rest.find('<') {
            let text = &rest[..i];
            assert!(!text.contains('>'), "stray '>' in text: {text}");
            for (k, _) in text.match_indices('&') {
                let ent = &text[k..];
                assert!(["&amp;", "&lt;", "&gt;", "&quot;", "&apos;"].iter().any(|e| ent.starts_with(e)), "bad entity");
            }
            let j = rest[i..].find('>').expect("unterminated tag") + i;
            let tag = &rest[i + 1..j];
            rest = &rest[j + 1..];
            if tag.starts_with('?') {
                assert!(tag.ends_with('?'));
                continue;
            }
            assert_eq!(tag.matches('"').count() % 2, 0, "unbalanced quotes in <{tag}>");
            if let Some(name) = tag.strip_prefix('/') {
                let open = stack.pop().expect("close without open");
                assert_eq!(open, name.trim(), "mismatched tag");
                continue;
            }
            let self_closing = tag.ends_with('/');
            let name = tag.trim_end_matches('/').split_whitespace().next().unwrap().to_string();
            assert!(name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | ':' | '_')), "bad name {name}");
            if stack.is_empty() {
                roots += 1;
            }
            names.push(name.clone());
            if !self_closing {
                stack.push(name);
            }
        }
        assert!(stack.is_empty(), "unclosed: {stack:?}");
        assert_eq!(roots, 1);
        names
    }

    fn pad(number: &str, x: f64, kind: PadKind, shape: PadShape) -> Item {
        Item::Pad(Pad {
            number: number.into(),
            pos: Point::new(nm(x), 0),
            rotation: 0.0,
            shape,
            width: nm(1.0),
            height: nm(0.6),
            corner_ratio: 0.25,
            kind,
            layer: if kind == PadKind::Smd { layer::TOP_COPPER } else { layer::MULTI_LAYER },
            drill: if kind == PadKind::Smd { None } else { Some(Drill { width: nm(0.5), height: nm(0.5) }) },
         copper_shapes: Default::default(),})
    }

    #[test]
    fn bom_exclusions_and_not_fitted_parts_keep_physical_features() {
        let document=board();let mut edits=Vec::new();
        for (id,item) in document.items(){if let Item::Footprint(f)=item{let mut f=f.clone();if f.designator=="R1"{f.parameters.insert("Do Not Populate".into(),"yes".into());}if f.designator=="R2"{f.parameters.insert("Exclude from BOM".into(),"true".into());}edits.push(pcbplannercad_core::IdItem{id,item:Item::Footprint(f)});}}
        let mut session=pcbplannercad_core::Session::with_document(document);session.execute(pcbplannercad_core::Command::ReplaceMany{items:edits}).unwrap();let xml=write_ipc2581(session.document(),"Assembly").unwrap();let bom=xml.split("<Bom ").nth(1).unwrap().split("</Bom>").next().unwrap();
        assert!(!bom.contains(r#"name="R1""#));assert!(!bom.contains(r#"name="R2""#));assert!(bom.contains(r#"name="J1""#));
        assert!(xml.contains(r#"refDes="R1""#));assert!(xml.contains(r#"refDes="R2""#));assert!(xml.contains("GND"));
    }

    fn board() -> Document {
        let mut items = vec![Item::Board(BoardSettings::default())];
        let pts = [(0.0, 0.0), (40.0, 0.0), (40.0, 30.0), (0.0, 30.0)];
        for i in 0..4 {
            let (a, b) = (pts[i], pts[(i + 1) % 4]);
            items.push(Item::Line(Line { a: Point::new(nm(a.0), nm(a.1)), b: Point::new(nm(b.0), nm(b.1)), width: nm(0.1), layer: layer::BOARD_OUTLINE }));
        }
        let mk = |d: &str, x: f64, side: Side, nets: &[(&str, &str)], fpk: &str| {
            Item::Footprint(PlacedFootprint {
                board_only: false,
                designator: d.into(),
                value: "10k & <1%>".into(),
                footprint: fpk.into(),
                pos: Point::new(nm(x), nm(10.0)),
                rotation: 90.0,
                side,
                locked: false,
                pad_nets: nets.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect(),
                component: "Lib/R".into(),
                parameters: [("MPN".to_string(), "RC0603\"X\"".to_string())].into(),
            })
        };
        items.push(mk("R1", 10.0, Side::Top, &[("1", "GND"), ("2", "VCC")], "Lib/R 0603"));
        items.push(mk("R2", 20.0, Side::Bottom, &[("1", "GND"), ("2", "SIG")], "Lib/R 0603"));
        items.push(mk("J1", 30.0, Side::Top, &[("1", "VCC")], "Lib/Header"));
        items.push(Item::Track(Track { locked: false, a: Point::new(nm(10.0), nm(10.0)), b: Point::new(nm(20.0), nm(10.0)), width: nm(0.25), layer: layer::TOP_COPPER, net: "GND".into() }));
        items.push(Item::Via(Via { locked: false, pos: Point::new(nm(15.0), nm(12.0)), diameter: nm(0.6), drill: nm(0.3), from_layer: layer::TOP_COPPER, to_layer: layer::BOTTOM_COPPER, net: "TRACKONLY".into() }));
        items.push(Item::Zone(Zone { locked: false,
            teardrop: false,
            outline: vec![Point::new(0, 0), Point::new(nm(5.0), 0), Point::new(nm(5.0), nm(5.0))],
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
            fill: vec![ZonePolygon {
                outer: vec![Point::new(0, 0), Point::new(nm(5.0), 0), Point::new(nm(5.0), nm(5.0)), Point::new(0, nm(5.0))],
                holes: vec![vec![Point::new(nm(1.0), nm(1.0)), Point::new(nm(2.0), nm(1.0)), Point::new(nm(2.0), nm(2.0))]],
            }],
        }));
        items.push(Item::Text(Text {
            pos: Point::new(nm(5.0), nm(25.0)),
            text: "AB".into(),
            height: nm(1.0),
            rotation: 0.0,
            h_align: Default::default(),
            v_align: Default::default(),
            layer: layer::TOP_SILK,
            mirrored: false,
        }));
        let mut doc = Document::from_item_list(DocKind::Pcb, items);
        let mut r = Footprint::new("R 0603");
        r.items = vec![pad("1", -0.8, PadKind::Smd, PadShape::RoundRect), pad("2", 0.8, PadKind::Smd, PadShape::RoundRect)];
        doc.put_cached_footprint("Lib/R 0603", r);
        let mut h = Footprint::new("Header");
        h.items = vec![pad("1", 0.0, PadKind::Tht, PadShape::Rect), pad("2", 2.54, PadKind::Tht, PadShape::Circle), pad("", 5.0, PadKind::Npth, PadShape::Circle)];
        doc.put_cached_footprint("Lib/Header", h);
        doc
    }

    fn section<'a>(s: &'a str, from: &str, to: &str) -> &'a str {
        let a = s.find(from).unwrap();
        let b = s[a..].find(to).map_or(s.len(), |b| a + b);
        &s[a..b]
    }

    #[test]
    fn writes_well_formed_ipc2581() {
        let doc = board();
        let xml = write_ipc2581(&doc, "Demo & Co").unwrap();
        let names = check_xml(&xml);
        assert_eq!(names[0], "IPC-2581");
        let count = |n: &str| names.iter().filter(|x| *x == n).count();
        // Components / packages.
        assert_eq!(count("Component"), 3);
        assert_eq!(count("Package"), 2);
        assert_eq!(count("Pin"), 2 + 2);
        // Nets: GND, SIG, VCC from pads + TRACKONLY from the via.
        assert_eq!(count("LogicalNet"), 4);
        let nets = section(&xml, "<LogicalNet", "<LayerFeature");
        assert_eq!(nets.matches("<PinRef ").count(), 5);
        // Profile with a closed 4-corner polygon.
        let profile = section(&xml, "<Profile>", "</Profile>");
        assert_eq!(profile.matches("<PolyStepSegment").count(), 4);
        // BOM groups R1+R2 (same MPN/value/footprint).
        assert_eq!(count("BomItem"), 2);
        assert!(xml.contains("quantity=\"2\""));
        assert!(xml.contains("RC0603&quot;X&quot;"));
        assert!(xml.contains("10k &amp; &lt;1%&gt;"));
        // Features.
        assert!(xml.contains("<Line startX=\"10\" startY=\"10\" endX=\"20\" endY=\"10\">"));
        assert!(count("Hole") == 1 + 2 + 1, "{}", count("Hole")); // via + 2 PTH + 1 NPTH
        assert_eq!(count("Cutout"), 1);
        assert!(xml.contains("layerFunction=\"DRILL\""));
        assert!(xml.contains("<Xform rotation=\"90\" mirror=\"true\"/>"));
        assert!(count("Stackup") == 1 && count("StackupLayer") == BoardSettings::default().stackup.layers.len());
        // Every StandardPrimitiveRef / LineDescRef resolves.
        for (r, d) in [("StandardPrimitiveRef id=\"", "EntryStandard id=\""), ("LineDescRef id=\"", "EntryLineDesc id=\"")] {
            for (i, _) in xml.match_indices(r) {
                let id = xml[i + r.len()..].split('"').next().unwrap();
                assert!(xml.contains(&format!("{d}{id}\"")), "dangling {id}");
            }
        }
        // Every layerRef resolves to a Layer.
        for (i, _) in xml.match_indices("layerRef=\"") {
            let id = xml[i + 10..].split('"').next().unwrap();
            assert!(xml.contains(&format!("<Layer name=\"{id}\"")), "dangling layer {id}");
        }
    }

    #[test]
    fn falls_back_to_copper_extents_without_outline() {
        let doc = Document::from_item_list(
            DocKind::Pcb,
            [Item::Track(Track { locked: false, a: Point::new(0, 0), b: Point::new(nm(10.0), 0), width: nm(0.2), layer: layer::TOP_COPPER, net: String::new() })],
        );
        let xml = write_ipc2581(&doc, "x").unwrap();
        check_xml(&xml);
        assert!(xml.contains("<Profile>"));
        assert!(write_ipc2581(&Document::new(DocKind::Pcb), "x").is_err());
        assert!(write_ipc2581(&Document::new(DocKind::Schematic), "x").is_err());
    }
}
