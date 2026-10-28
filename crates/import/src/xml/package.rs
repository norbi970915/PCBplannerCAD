//! XML `<package>` → pCAD [`Footprint`].

use pcbplannercad_core::geom::{BBox, rotate};
use pcbplannercad_core::item::{DESIGNATOR_TOKEN, Drill, FillStyle, Item, Pad, PadKind, PadShape, Rect, Text, VALUE_TOKEN};
use pcbplannercad_core::layer;
use pcbplannercad_core::units::{Coord, Point, mm};
use pcbplannercad_core::Footprint;
use roxmltree::Node;

use crate::ImportReport;

use super::util::{LayerSkips, Rot, attr, attr_str, board_layer, coord, description, elements, graphic, layer_num, num, num_or, placeholder, point, text_item};

pub(crate) const COURTYARD_MARGIN_MM: f64 = 0.25;

/// Counters for approximations made while converting packages.
#[derive(Debug, Default)]
pub(crate) struct PackageNotes {
    pub octagon_pads: usize,
    pub offset_pads: usize,
    pub slot_pads: usize,
    pub courtyards: usize,
    pub bad_pads: usize,
    pub mirrored_pads: usize,
}

impl PackageNotes {
    pub(crate) fn report(&self, context: &str, report: &mut ImportReport) {
        if self.courtyards > 0 {
            report.info(context, format!("{} footprint(s): synthesized courtyard rectangle around pads and tPlace (0.25 mm margin)", self.courtyards));
        }
        if self.octagon_pads > 0 {
            report.info(context, format!("{} octagon pad(s) approximated as rounded rectangles", self.octagon_pads));
        }
        if self.offset_pads > 0 {
            report.warn(context, format!("{} offset pad(s) approximated as ovals centered on the drill", self.offset_pads));
        }
        if self.slot_pads > 0 {
            report.info(context, format!("{} slotted pad(s) converted to ovals with slot drills", self.slot_pads));
        }
        if self.mirrored_pads > 0 {
            report.warn(context, format!("{} mirrored SMD pad(s): mirror flag ignored", self.mirrored_pads));
        }
        if self.bad_pads > 0 {
            report.warn(context, format!("{} pad(s) without valid size or drill skipped", self.bad_pads));
        }
    }
}

/// Pad geometry for an XML THT `<pad>`: `(shape, width, height, corner_ratio)`.
/// `diameter` 0 or absent means "automatic" (drill + 2 × max(0.25 mm, 25 % of drill)).
pub(crate) fn tht_pad_geometry(shape: &str, drill_mm: f64, diameter_mm: Option<f64>) -> (PadShape, f64, f64, f64) {
    let d = match diameter_mm {
        Some(d) if d > 0.0 => d,
        _ => drill_mm + 2.0 * (0.25f64).max(0.25 * drill_mm),
    };
    match shape {
        "square" => (PadShape::Rect, d, d, 0.0),
        "octagon" => (PadShape::RoundRect, d, d, 0.29),
        "long" | "offset" => (PadShape::Oval, 2.0 * d, d, 0.0),
        _ => (PadShape::Circle, d, d, 0.0),
    }
}

/// SMD pad shape from XML `roundness` (0–100 %).
pub(crate) fn smd_pad_shape(dx: Coord, dy: Coord, roundness: f64) -> (PadShape, f64) {
    let r = roundness.clamp(0.0, 100.0);
    if r <= 0.0 {
        (PadShape::Rect, 0.0)
    } else if r >= 100.0 {
        if dx == dy { (PadShape::Circle, 0.0) } else { (PadShape::Oval, 0.0) }
    } else {
        (PadShape::RoundRect, r / 200.0)
    }
}

fn smd(n: Node<'_, '_>, notes: &mut PackageNotes) -> Option<Item> {
    let (w, h) = (coord(n, "dx").abs(), coord(n, "dy").abs());
    let side = match layer_num(n) {
        Some(16) => layer::BOTTOM_COPPER,
        _ => layer::TOP_COPPER,
    };
    if w <= 0 || h <= 0 {
        notes.bad_pads += 1;
        return None;
    }
    let rot = Rot::of(n);
    if rot.mirror {
        notes.mirrored_pads += 1;
    }
    let (shape, corner_ratio) = smd_pad_shape(w, h, num_or(n, "roundness", 0.0));
    Some(Item::Pad(Pad {
        number: attr_str(n, "name"),
        pos: point(n, "x", "y"),
        rotation: rot.angle,
        shape,
        width: w,
        height: h,
        corner_ratio,
        kind: PadKind::Smd,
        layer: side,
        drill: None,
    }))
}

/// How THT pad diameters are derived from drills.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PadRules {
    /// Restring as a fraction of the drill, clamped to `[min_mm, max_mm]`.
    pub rv: f64,
    pub min_mm: f64,
    pub max_mm: f64,
    /// Board mode: the restring rule is a lower bound even for explicit
    /// diameters (XML applies design rules to every pad on a board).
    pub board: bool,
    /// `long`/`offset` pad length in % of the diameter beyond the circle (100 = 2:1).
    pub elongation: f64,
}

impl Default for PadRules {
    /// Library defaults: drill + 2 × max(0.25 mm, 25 % of drill).
    fn default() -> Self {
        PadRules { rv: 0.25, min_mm: 0.25, max_mm: f64::INFINITY, board: false, elongation: 100.0 }
    }
}

impl PadRules {
    pub(crate) fn diameter(&self, drill: f64, given: Option<f64>) -> f64 {
        let (lo, hi) = (self.min_mm.min(self.max_mm), self.min_mm.max(self.max_mm));
        let auto = drill + 2.0 * (self.rv * drill).max(lo).min(hi);
        match given {
            Some(d) if d > 0.0 => {
                if self.board { d.max(auto) } else { d }
            }
            _ => auto,
        }
    }
}

fn tht(n: Node<'_, '_>, notes: &mut PackageNotes, rules: &PadRules) -> Option<Item> {
    let drill = num(n, "drill").unwrap_or(0.0);
    if drill <= 0.0 {
        notes.bad_pads += 1;
        return None;
    }
    let shape_name = attr(n, "shape").unwrap_or("round");
    let rot = Rot::of(n);
    let d = rules.diameter(drill, num(n, "diameter"));
    let (shape, w, h, corner_ratio, drill_xy) = if shape_name == "slot" {
        // XML 9.6+ slotted pad: `slotLength` long, `drill` wide.
        notes.slot_pads += 1;
        let len = num(n, "slotLength").filter(|l| *l > drill).unwrap_or(drill);
        (PadShape::Oval, len + (d - drill), d, 0.0, (len, drill))
    } else {
        match shape_name {
            "octagon" => notes.octagon_pads += 1,
            "offset" => notes.offset_pads += 1,
            _ => {}
        }
        let (s, mut w, h, c) = tht_pad_geometry(shape_name, drill, Some(d));
        if matches!(shape_name, "long" | "offset") && rules.elongation > 0.0 {
            w = d * (1.0 + rules.elongation / 100.0);
        }
        (s, w, h, c, (drill, drill))
    };
    let (w, h) = (mm(w), mm(h));
    if w <= 0 || h <= 0 {
        notes.bad_pads += 1;
        return None;
    }
    Some(Item::Pad(Pad {
        number: attr_str(n, "name"),
        pos: point(n, "x", "y"),
        rotation: rot.angle,
        shape,
        width: w,
        height: h,
        corner_ratio,
        kind: PadKind::Tht,
        layer: layer::MULTI_LAYER,
        drill: Some(Drill { width: mm(drill_xy.0), height: mm(drill_xy.1) }),
    }))
}

fn hole(n: Node<'_, '_>, notes: &mut PackageNotes) -> Option<Item> {
    let drill = coord(n, "drill");
    if drill <= 0 {
        notes.bad_pads += 1;
        return None;
    }
    Some(Item::Pad(Pad {
        number: String::new(),
        pos: point(n, "x", "y"),
        rotation: 0.0,
        shape: PadShape::Circle,
        width: drill,
        height: drill,
        corner_ratio: 0.0,
        kind: PadKind::Npth,
        layer: layer::MULTI_LAYER,
        drill: Some(Drill { width: drill, height: drill }),
    }))
}

/// Corner points covering an item's extent (for the synthesized courtyard).
fn extent(item: &Item) -> Vec<Point> {
    let grow = |p: Point, r: Coord| [p.translated(-r, -r), p.translated(r, r)];
    match item {
        Item::Pad(p) => {
            let (hw, hh) = (p.width / 2, p.height / 2);
            [(-hw, -hh), (hw, -hh), (hw, hh), (-hw, hh)]
                .into_iter()
                .map(|(dx, dy)| rotate(p.pos.translated(dx, dy), p.pos, p.rotation))
                .collect()
        }
        Item::Line(l) => [grow(l.a, l.width / 2), grow(l.b, l.width / 2)].concat(),
        Item::Arc(a) => (0..=8).flat_map(|k| grow(a.point_at(a.start_angle + a.sweep * k as f64 / 8.0), a.width / 2)).collect(),
        Item::Circle(c) => grow(c.center, c.radius + c.width / 2).to_vec(),
        Item::Rect(r) => vec![r.a, r.b],
        Item::Polyline(p) => p.points.clone(),
        _ => Vec::new(),
    }
}

pub(crate) fn convert_package(n: Node<'_, '_>, skips: &mut LayerSkips, notes: &mut PackageNotes, rules: &PadRules) -> Footprint {
    let mut fp = Footprint::new(attr_str(n, "name"));
    fp.description = description(n);
    let mut outline_pts: Vec<Point> = Vec::new();
    for e in elements(n) {
        let tag = e.tag_name().name();
        let item = match tag {
            "smd" => smd(e, notes).into_iter().collect(),
            "pad" => tht(e, notes, rules).into_iter().collect(),
            "hole" => hole(e, notes).into_iter().collect(),
            "description" => continue,
            "text" | "wire" | "circle" | "rectangle" | "polygon" | "polygonshape" | "frame" => {
                let ln = layer_num(e);
                let Some(layer) = ln.and_then(board_layer) else {
                    skips.add(ln);
                    continue;
                };
                let items: Vec<Item> = if tag == "text" {
                    let raw = e.text().unwrap_or("");
                    let text = match placeholder(raw).as_deref() {
                        Some("NAME") => DESIGNATOR_TOKEN.to_string(),
                        Some("VALUE") => VALUE_TOKEN.to_string(),
                        _ => raw.to_string(),
                    };
                    text_item(e, &text, layer, false).into_iter().collect()
                } else {
                    graphic(e, layer).unwrap_or_default()
                };
                if ln == Some(21) {
                    outline_pts.extend(items.iter().filter(|i| !matches!(i, Item::Text(Text { .. }))).flat_map(extent));
                }
                items
            }
            _ => continue,
        };
        for i in item {
            if matches!(i, Item::Pad(_)) {
                outline_pts.extend(extent(&i));
            }
            fp.items.push(i);
        }
    }
    if let Some(b) = BBox::from_points(outline_pts) {
        let m = mm(COURTYARD_MARGIN_MM);
        fp.items.push(Item::Rect(Rect {
            a: b.min.translated(-m, -m),
            b: b.max.translated(m, m),
            width: mm(0.05),
            layer: layer::TOP_COURTYARD,
            fill: FillStyle::None,
        }));
        notes.courtyards += 1;
    }
    fp
}
