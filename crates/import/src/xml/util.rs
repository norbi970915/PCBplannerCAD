//! Shared helpers: XML access, XML rotation strings, text orientation,
//! layer mapping and graphics conversion (wires, arcs, polygons…).

use std::collections::BTreeMap;

use pcbplannercad_core::geom::{Placement, normalize_deg};
use pcbplannercad_core::item::{Arc, Circle, FillStyle, HAlign, Item, Line, Polyline, Rect, Text, VAlign};
use pcbplannercad_core::layer;
use pcbplannercad_core::units::{Coord, Point, mm};
use pcbplannercad_core::LayerId;
use roxmltree::{Document, Node, ParsingOptions};

use crate::ImportError;

// ---------- XML ----------

pub(crate) fn parse_xml(text: &str) -> Result<Document<'_>, ImportError> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let opts = ParsingOptions { allow_dtd: true, ..ParsingOptions::default() };
    Document::parse_with_options(text, opts)
        .map_err(|e| ImportError::Syntax { line: e.pos().row as usize, message: e.to_string() })
}

/// Element children with the given tag name.
pub(crate) fn children<'a, 'i: 'a>(n: Node<'a, 'i>, tag: &'static str) -> impl Iterator<Item = Node<'a, 'i>> + 'a {
    n.children().filter(move |c| c.is_element() && c.tag_name().name() == tag)
}

/// All element children.
pub(crate) fn elements<'a, 'i: 'a>(n: Node<'a, 'i>) -> impl Iterator<Item = Node<'a, 'i>> + 'a {
    n.children().filter(|c| c.is_element())
}

pub(crate) fn child<'a, 'i: 'a>(n: Node<'a, 'i>, tag: &'static str) -> Option<Node<'a, 'i>> {
    children(n, tag).next()
}

/// Children of `<container>` (e.g. `<packages>`) that are `<tag>` elements.
pub(crate) fn grandchildren<'a, 'i: 'a>(
    n: Node<'a, 'i>,
    container: &'static str,
    tag: &'static str,
) -> impl Iterator<Item = Node<'a, 'i>> + 'a {
    children(n, container).flat_map(move |c| children(c, tag))
}

pub(crate) fn attr<'a>(n: Node<'a, '_>, name: &str) -> Option<&'a str> {
    n.attribute(name)
}

pub(crate) fn attr_str(n: Node<'_, '_>, name: &str) -> String {
    n.attribute(name).unwrap_or("").to_string()
}

/// Largest coordinate magnitude accepted (mm); keeps integer math overflow-free.
const MAX_MM: f64 = 1.0e6;

pub(crate) fn num(n: Node<'_, '_>, name: &str) -> Option<f64> {
    n.attribute(name)?.trim().parse::<f64>().ok().filter(|v| v.is_finite()).map(|v| v.clamp(-MAX_MM, MAX_MM))
}

pub(crate) fn num_or(n: Node<'_, '_>, name: &str, default: f64) -> f64 {
    num(n, name).unwrap_or(default)
}

pub(crate) fn coord(n: Node<'_, '_>, name: &str) -> Coord {
    mm(num_or(n, name, 0.0))
}

pub(crate) fn point(n: Node<'_, '_>, x: &str, y: &str) -> Point {
    Point::new(coord(n, x), coord(n, y))
}

pub(crate) fn fpoint(n: Node<'_, '_>, x: &str, y: &str) -> (f64, f64) {
    (num_or(n, x, 0.0), num_or(n, y, 0.0))
}

pub(crate) fn is_yes(n: Node<'_, '_>, name: &str) -> bool {
    n.attribute(name).is_some_and(|v| v.eq_ignore_ascii_case("yes"))
}

pub(crate) fn layer_num(n: Node<'_, '_>) -> Option<u32> {
    n.attribute("layer")?.trim().parse().ok()
}

/// Strips HTML markup from an XML `<description>` and collapses whitespace.
pub(crate) fn strip_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '<' {
            let mut tag = String::new();
            for t in chars.by_ref() {
                if t == '>' {
                    break;
                }
                tag.push(t);
            }
            let t = tag.trim_start_matches('/').to_ascii_lowercase();
            let t = t.split(|c: char| c.is_whitespace() || c == '/').next().unwrap_or("");
            if matches!(t, "p" | "br" | "li" | "tr" | "div" | "h1" | "h2" | "h3" | "h4" | "ul" | "ol" | "table") {
                out.push('\n');
            } else {
                out.push(' ');
            }
        } else {
            out.push(c);
        }
    }
    let out = out
        .replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&");
    let lines: Vec<String> = out
        .lines()
        .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|l| !l.is_empty())
        .collect();
    lines.join("\n")
}

pub(crate) fn description(n: Node<'_, '_>) -> String {
    child(n, "description").and_then(|d| d.text()).map(strip_html).unwrap_or_default()
}

// ---------- Rotation strings ----------

/// A parsed XML rotation string such as `R90`, `MR180` or `SMR45`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Rot {
    pub mirror: bool,
    /// "Spin": text is not flipped for readability.
    pub spin: bool,
    /// Degrees CCW in `[0, 360)`.
    pub angle: f64,
}

pub(crate) fn parse_rot(s: &str) -> Rot {
    let mut r = Rot::default();
    let mut rest = s.trim();
    loop {
        if let Some(t) = rest.strip_prefix(['S', 's']) {
            r.spin = true;
            rest = t;
        } else if let Some(t) = rest.strip_prefix(['M', 'm']) {
            r.mirror = true;
            rest = t;
        } else {
            break;
        }
    }
    let rest = rest.strip_prefix(['R', 'r']).unwrap_or(rest);
    r.angle = rest.trim().parse::<f64>().ok().filter(|a| a.is_finite()).map(normalize_deg).unwrap_or(0.0);
    r
}

impl Rot {
    pub(crate) fn of(n: Node<'_, '_>) -> Rot {
        n.attribute("rot").map(parse_rot).unwrap_or_default()
    }

    /// Placement of an instance, pin or element. XML rotates first and
    /// then mirrors (`MR90` = mirror ∘ rotate 90°), which in core terms
    /// (mirror, then rotate) is a rotation by `-angle`.
    pub(crate) fn placement(self, offset: Point) -> Placement {
        let rotation = if self.mirror { normalize_deg(-self.angle) } else { self.angle };
        Placement { offset, rotation, mirror: self.mirror }
    }

    /// World direction of the local +X axis of a placed object (pin, instance).
    pub(crate) fn direction(self) -> f64 {
        self.placement(Point::default()).apply_angle(0.0)
    }

    /// Reading direction of a text or label. Texts use the opposite order
    /// from instances: `MRa` mirrors first and then rotates by `a`.
    pub(crate) fn text_direction(self) -> f64 {
        normalize_deg(self.angle + if self.mirror { 180.0 } else { 0.0 })
    }

    /// A text orientation `self`, local to an instance placed with `outer`,
    /// expressed in world terms (as XML writes smashed attributes: an
    /// `R0` text in an `MR90` instance becomes `MR270`).
    pub(crate) fn within(self, outer: Rot) -> Rot {
        let r = outer.placement(Point::default()).rotation;
        let angle = if outer.mirror { r - self.angle } else { r + self.angle };
        Rot { mirror: self.mirror ^ outer.mirror, spin: self.spin, angle: normalize_deg(angle) }
    }
}

fn v_flipped(v: VAlign) -> VAlign {
    match v {
        VAlign::Bottom => VAlign::Top,
        VAlign::Middle => VAlign::Middle,
        VAlign::Top => VAlign::Bottom,
    }
}

/// Converts an XML text orientation into a readable pCAD rotation and
/// alignment. Mirroring keeps glyphs readable but flips the horizontal
/// alignment (`flip_mirror`, schematic texts); texts turned upside down
/// (90° < angle ≤ 270°) are shown rotated by 180° with both alignments
/// flipped, as XML does unless the text "spins".
pub(crate) fn resolve_text(rot: Rot, h: HAlign, v: VAlign, flip_mirror: bool) -> (f64, HAlign, VAlign) {
    let mut h = if rot.mirror && flip_mirror { h.flipped() } else { h };
    let mut v = v;
    let mut a = normalize_deg(rot.angle);
    if !rot.spin && a > 90.0 && a <= 270.0 {
        a = normalize_deg(a - 180.0);
        h = h.flipped();
        v = v_flipped(v);
    }
    (a, h, v)
}

pub(crate) fn parse_align(s: Option<&str>) -> (HAlign, VAlign) {
    match s.unwrap_or("bottom-left").trim() {
        "bottom-center" => (HAlign::Center, VAlign::Bottom),
        "bottom-right" => (HAlign::Right, VAlign::Bottom),
        "center-left" => (HAlign::Left, VAlign::Middle),
        "center" => (HAlign::Center, VAlign::Middle),
        "center-right" => (HAlign::Right, VAlign::Middle),
        "top-left" => (HAlign::Left, VAlign::Top),
        "top-center" => (HAlign::Center, VAlign::Top),
        "top-right" => (HAlign::Right, VAlign::Top),
        _ => (HAlign::Left, VAlign::Bottom),
    }
}

// ---------- Layers ----------

/// Board-side layer mapping used for packages.
pub(crate) fn board_layer(n: u32) -> Option<LayerId> {
    Some(match n {
        1 => layer::TOP_COPPER,
        16 => layer::BOTTOM_COPPER,
        21 | 25 | 27 => layer::TOP_SILK,
        22 | 26 | 28 => layer::BOTTOM_SILK,
        29 => layer::TOP_MASK,
        30 => layer::BOTTOM_MASK,
        31 => layer::TOP_PASTE,
        32 => layer::BOTTOM_PASTE,
        39 | 41 => layer::KEEP_OUT,
        51 => layer::TOP_ASSEMBLY,
        52 => layer::BOTTOM_ASSEMBLY,
        20 | 46 => layer::BOARD_OUTLINE,
        47..=49 => layer::MECHANICAL_1,
        _ => return None,
    })
}

/// Schematic drawing layers (91 Nets … 98 Guide) all map to [`layer::SCHEMATIC`].
pub(crate) fn schematic_layer(n: u32) -> Option<LayerId> {
    (91..=98).contains(&n).then_some(layer::SCHEMATIC)
}

/// Counts items dropped because their layer has no pCAD equivalent.
#[derive(Debug, Default)]
pub(crate) struct LayerSkips(pub BTreeMap<u32, usize>);

impl LayerSkips {
    pub(crate) fn add(&mut self, layer: Option<u32>) {
        *self.0.entry(layer.unwrap_or(0)).or_default() += 1;
    }

    pub(crate) fn summary(&self) -> Option<String> {
        if self.0.is_empty() {
            return None;
        }
        let total: usize = self.0.values().sum();
        let list: Vec<String> = self.0.iter().map(|(l, c)| format!("layer {l}: {c}")).collect();
        Some(format!("{total} item(s) on unsupported layers skipped ({})", list.join(", ")))
    }
}

// ---------- Graphics ----------

/// A straight line, or an arc when `curve` (degrees, CCW from a to b) is set.
pub(crate) fn wire_item(a: (f64, f64), b: (f64, f64), curve: f64, width: Coord, layer: LayerId) -> Option<Item> {
    let (pa, pb) = (Point::new(mm(a.0), mm(a.1)), Point::new(mm(b.0), mm(b.1)));
    if pa == pb {
        return None;
    }
    if curve.abs() < 1e-6 || curve.abs() >= 360.0 {
        return Some(Item::Line(Line { a: pa, b: pb, width, layer }));
    }
    let Some((c, r)) = arc_center(a, b, curve) else {
        return Some(Item::Line(Line { a: pa, b: pb, width, layer }));
    };
    let radius = mm(r);
    if radius <= 0 {
        return Some(Item::Line(Line { a: pa, b: pb, width, layer }));
    }
    let start = (a.1 - c.1).atan2(a.0 - c.0).to_degrees();
    Some(Item::Arc(Arc {
        center: Point::new(mm(c.0), mm(c.1)),
        radius,
        start_angle: normalize_deg(start),
        sweep: curve,
        width,
        layer,
    }))
}

/// Center and radius (mm) of the arc from `a` to `b` sweeping `curve` degrees CCW.
pub(crate) fn arc_center(a: (f64, f64), b: (f64, f64), curve: f64) -> Option<((f64, f64), f64)> {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let chord = dx.hypot(dy);
    let half = curve.to_radians() / 2.0;
    if chord <= 0.0 || half.sin().abs() < 1e-12 {
        return None;
    }
    let r = chord / (2.0 * half.sin().abs());
    let d = (chord / 2.0) / half.tan();
    let (nx, ny) = (-dy / chord, dx / chord);
    let c = ((a.0 + b.0) / 2.0 + nx * d, (a.1 + b.1) / 2.0 + ny * d);
    (c.0.is_finite() && c.1.is_finite() && r.is_finite()).then_some((c, r))
}

/// Polygon outline points from `<vertex x y curve>` children, tessellating curved edges.
pub(crate) fn polygon_points(n: Node<'_, '_>) -> Vec<Point> {
    let verts: Vec<((f64, f64), f64)> = children(n, "vertex").map(|v| (fpoint(v, "x", "y"), num_or(v, "curve", 0.0))).collect();
    let mut pts = Vec::new();
    for (i, &(p, curve)) in verts.iter().enumerate() {
        pts.push(Point::new(mm(p.0), mm(p.1)));
        if curve.abs() < 1e-6 || curve.abs() >= 360.0 {
            continue;
        }
        let q = verts[(i + 1) % verts.len()].0;
        if let Some((c, r)) = arc_center(p, q, curve) {
            let start = (p.1 - c.1).atan2(p.0 - c.0);
            let steps = ((curve.abs() / 10.0).ceil() as usize).clamp(1, 36);
            for k in 1..steps {
                let t = start + curve.to_radians() * k as f64 / steps as f64;
                pts.push(Point::new(mm(c.0 + r * t.cos()), mm(c.1 + r * t.sin())));
            }
        }
    }
    pts.dedup();
    if pts.len() > 1 && pts.first() == pts.last() {
        pts.pop();
    }
    pts
}

/// Rectangle `x1 y1 x2 y2 rot` (filled), rotated about its center.
pub(crate) fn rectangle_item(n: Node<'_, '_>, layer: LayerId) -> Option<Item> {
    let (a, b) = (point(n, "x1", "y1"), point(n, "x2", "y2"));
    if a.x == b.x || a.y == b.y {
        return None;
    }
    let rot = Rot::of(n);
    let item = Item::Rect(Rect { a, b, width: 0, layer, fill: FillStyle::Solid });
    if rot.angle == 0.0 {
        return Some(item);
    }
    let c = Point::new((a.x + b.x) / 2, (a.y + b.y) / 2);
    Some(item.rotated(c, rot.angle))
}

pub(crate) fn circle_item(n: Node<'_, '_>, layer: LayerId) -> Option<Item> {
    let radius = coord(n, "radius");
    if radius <= 0 {
        return None;
    }
    let width = coord(n, "width").max(0);
    let fill = if width == 0 { FillStyle::Solid } else { FillStyle::None };
    Some(Item::Circle(Circle { center: point(n, "x", "y"), radius, width, layer, fill }))
}

pub(crate) fn polyline_item(points: Vec<Point>, width: Coord, layer: LayerId) -> Option<Item> {
    (points.len() >= 2).then(|| Item::Polyline(Polyline { points, closed: true, width: width.max(0), layer, fill: FillStyle::Solid }))
}

/// Converts the purely graphical elements (wire, circle, rectangle, polygon,
/// polygonshape, frame) of any XML drawing; returns `None` for elements
/// that are not graphics (texts, pins, pads…).
pub(crate) fn graphic(n: Node<'_, '_>, layer: LayerId) -> Option<Vec<Item>> {
    let out = match n.tag_name().name() {
        "wire" => wire_item(fpoint(n, "x1", "y1"), fpoint(n, "x2", "y2"), num_or(n, "curve", 0.0), coord(n, "width").max(0), layer)
            .into_iter()
            .collect(),
        "circle" => circle_item(n, layer).into_iter().collect(),
        "rectangle" => rectangle_item(n, layer).into_iter().collect(),
        "polygon" => polyline_item(polygon_points(n), coord(n, "width"), layer).into_iter().collect(),
        "polygonshape" => {
            let outline = child(n, "polygonoutlineobjects").or_else(|| child(n, "polygonoutlinesegments"));
            outline.and_then(|o| polyline_item(polygon_points(o), coord(n, "width"), layer)).into_iter().collect()
        }
        "frame" => {
            let (a, b) = (point(n, "x1", "y1"), point(n, "x2", "y2"));
            if a.x == b.x || a.y == b.y {
                Vec::new()
            } else {
                vec![Item::Rect(Rect { a, b, width: 0, layer, fill: FillStyle::None })]
            }
        }
        _ => return None,
    };
    Some(out)
}

/// A plain (non-placeholder) text item.
pub(crate) fn text_item(n: Node<'_, '_>, text: &str, layer: LayerId, schematic: bool) -> Option<Item> {
    if text.trim().is_empty() {
        return None;
    }
    let height = coord(n, "size");
    let height = if height > 0 { height } else { mm(1.778) };
    let rot = Rot::of(n);
    let (h, v) = parse_align(attr(n, "align"));
    let (rotation, h_align, v_align) = resolve_text(rot, h, v, schematic);
    Some(Item::Text(Text {
        pos: point(n, "x", "y"),
        text: text.to_string(),
        height,
        rotation,
        h_align,
        v_align,
        layer,
        mirrored: !schematic && rot.mirror,
    }))
}

/// `>NAME` → `Some("NAME")` for attribute placeholder texts.
pub(crate) fn placeholder(text: &str) -> Option<String> {
    let t = text.trim();
    let name = t.strip_prefix('>')?;
    (!name.is_empty() && !name.contains(char::is_whitespace)).then(|| name.to_ascii_uppercase())
}

/// `GND@1` → `GND`.
pub(crate) fn strip_at(name: &str) -> &str {
    match name.rfind('@') {
        Some(i) if i > 0 => &name[..i],
        _ => name,
    }
}
