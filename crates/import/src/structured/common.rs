//! Helpers shared by the symbol, footprint and schematic readers.

use pcbplannercad_core::LayerId;
use pcbplannercad_core::geom::normalize_deg;
use pcbplannercad_core::item::{Arc, FillStyle, HAlign, Item, Line, Polyline, VAlign};
use pcbplannercad_core::units::{Coord, Point, mm};

use super::sexpr::List;
use crate::report::{ImportReport, Severity};

/// Which way the file's Y axis points. Symbol libraries are Y-up like our
/// world; footprints and schematics are Y-down and get Y negated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum YAxis {
    Up,
    Down,
}

impl YAxis {
    pub fn pt(self, x: f64, y: f64) -> Point {
        match self {
            YAxis::Up => Point::new(mm(x), mm(y)),
            YAxis::Down => Point::new(mm(x), -mm(y)),
        }
    }

    /// Point from a list like `(xy x y)` / `(at x y)` / `(start x y)`.
    pub fn pt_of(self, l: &List) -> Option<Point> {
        let (x, y) = l.xy()?;
        Some(self.pt(x, y))
    }

    /// Point from child `name` of `parent`.
    pub fn child_pt(self, parent: &List, name: &str) -> Option<Point> {
        self.pt_of(parent.child(name)?)
    }

    /// All `(xy ..)` points of a `(pts ...)` child. Arc segments inside the
    /// point list (structured format 7+ polygons) are flattened.
    pub fn pts(self, parent: &List) -> Vec<Point> {
        let mut out = Vec::new();
        let Some(pts) = parent.child("pts") else { return out };
        for l in pts.lists() {
            match l.head() {
                "xy" => out.extend(self.pt_of(l)),
                "arc" => {
                    let (Some(s), Some(m), Some(e)) = (self.child_pt(l, "start"), self.child_pt(l, "mid"), self.child_pt(l, "end")) else {
                        continue;
                    };
                    match arc_from_3pts(s, m, e) {
                        Some(a) => out.extend(arc_polyline(&a, s, e)),
                        None => out.extend([s, e]),
                    }
                }
                _ => {}
            }
        }
        out
    }
}

/// Text appearance read from `(effects ...)` plus hide flags.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Effects {
    pub height: Coord,
    pub h_align: HAlign,
    pub v_align: VAlign,
    pub mirrored: bool,
    pub hidden: bool,
}

/// Reads the effects of a text-bearing node (property, text, fp_text, pin name…).
pub fn effects(node: &List) -> Effects {
    let mut e = Effects { height: mm(1.27), h_align: HAlign::Center, v_align: VAlign::Middle, mirrored: false, hidden: node.flag("hide") };
    if let Some(eff) = node.child("effects") {
        if let Some(h) = eff.child("font").and_then(|f| f.child("size")).and_then(|s| s.num(1)).filter(|h| *h > 0.0) {
            e.height = mm(h);
        }
        if let Some(j) = eff.child("justify") {
            for a in j.items.iter().skip(1).filter_map(|s| s.as_atom()) {
                match a {
                    "left" => e.h_align = HAlign::Left,
                    "right" => e.h_align = HAlign::Right,
                    "top" => e.v_align = VAlign::Top,
                    "bottom" => e.v_align = VAlign::Bottom,
                    "mirror" => e.mirrored = true,
                    _ => {}
                }
            }
        }
        if eff.flag("hide") {
            e.hidden = true;
        }
    }
    e
}

/// `(stroke (width w))` (structured format 6+) or `(width w)` (older).
pub fn stroke_width(node: &List) -> Coord {
    let w = node.child("stroke").and_then(|s| s.child_num("width")).or_else(|| node.child_num("width")).unwrap_or(0.0);
    mm(w.max(0.0))
}

/// `(fill (type none|outline|background|color))` (symbols, schematics) or
/// `(fill yes|no|solid|none)` (footprints). `None` when absent.
pub fn fill(node: &List) -> Option<FillStyle> {
    let f = node.child("fill")?;
    let kind = f.child_str("type").or_else(|| f.atom(1)).unwrap_or("none");
    Some(match kind {
        "outline" | "yes" | "solid" | "color" | "hatch" | "reverse_hatch" | "cross_hatch" => FillStyle::Solid,
        "background" => FillStyle::Background,
        _ => FillStyle::None,
    })
}

/// Circular arc through three points (start, any point on the arc, end),
/// normalized to a positive (CCW) sweep. `None` when the points are collinear.
pub fn arc_from_3pts(s: Point, m: Point, e: Point) -> Option<ArcGeom> {
    // Work relative to the start point to keep magnitudes small.
    let (bx, by) = ((m.x - s.x) as f64, (m.y - s.y) as f64);
    let (cx, cy) = ((e.x - s.x) as f64, (e.y - s.y) as f64);
    let d = 2.0 * (bx * cy - by * cx);
    if d.abs() < 1e-6 {
        return None;
    }
    let b2 = bx * bx + by * by;
    let c2 = cx * cx + cy * cy;
    let ux = (cy * b2 - by * c2) / d;
    let uy = (bx * c2 - cx * b2) / d;
    let radius = (ux * ux + uy * uy).sqrt();
    if !radius.is_finite() || radius < 0.5 {
        return None;
    }
    let center = Point::new(s.x + ux.round() as Coord, s.y + uy.round() as Coord);
    let ang = |p: Point| ((p.y - s.y) as f64 - uy).atan2((p.x - s.x) as f64 - ux).to_degrees();
    let (a_s, a_m, a_e) = (ang(s), ang(m), ang(e));
    let sweep_se = (a_e - a_s).rem_euclid(360.0);
    let to_mid = (a_m - a_s).rem_euclid(360.0);
    let (start, sweep) = if sweep_se == 0.0 {
        (a_s, 360.0)
    } else if to_mid < sweep_se {
        (a_s, sweep_se)
    } else {
        (a_e, 360.0 - sweep_se)
    };
    Some(ArcGeom { center, radius: radius.round() as Coord, start_angle: snap_angle(start), sweep: snap_sweep(sweep) })
}

/// Arc from a center, radius, start angle and signed sweep (any sign).
pub fn arc_from_center(center: Point, radius: Coord, start_angle: f64, sweep: f64) -> Option<ArcGeom> {
    if radius <= 0 || sweep == 0.0 || !sweep.is_finite() || !start_angle.is_finite() {
        return None;
    }
    let (start, sweep) = if sweep < 0.0 { (start_angle + sweep, -sweep) } else { (start_angle, sweep) };
    Some(ArcGeom { center, radius, start_angle: snap_angle(start), sweep: snap_sweep(sweep.min(360.0)) })
}

fn snap_angle(a: f64) -> f64 {
    let a = normalize_deg(a);
    let r = (a * 1e6).round() / 1e6;
    if r >= 360.0 { 0.0 } else { r }
}

fn snap_sweep(s: f64) -> f64 {
    (s * 1e6).round() / 1e6
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ArcGeom {
    pub center: Point,
    pub radius: Coord,
    pub start_angle: f64,
    pub sweep: f64,
}

impl ArcGeom {
    pub fn item(&self, width: Coord, layer: LayerId) -> Item {
        Item::Arc(Arc { center: self.center, radius: self.radius, start_angle: self.start_angle, sweep: self.sweep, width, layer })
    }

    fn point_at(&self, deg: f64) -> Point {
        let (s, c) = deg.to_radians().sin_cos();
        Point::new(self.center.x + (self.radius as f64 * c).round() as Coord, self.center.y + (self.radius as f64 * s).round() as Coord)
    }
}

/// Points along an arc, running from `from` to `to` (the original endpoints,
/// kept exact).
pub fn arc_polyline(a: &ArcGeom, from: Point, to: Point) -> Vec<Point> {
    let n = ((a.sweep / 10.0).ceil() as usize).clamp(2, 36);
    let mut pts: Vec<Point> = (1..n).map(|i| a.point_at(a.start_angle + a.sweep * i as f64 / n as f64)).collect();
    // `a` runs CCW from its start; reverse if `from` is the CCW end.
    let start = a.point_at(a.start_angle);
    let d = |p: Point, q: Point| (p.x - q.x).abs() + (p.y - q.y).abs();
    if d(start, from) > d(start, to) {
        pts.reverse();
    }
    let mut out = Vec::with_capacity(n + 1);
    out.push(from);
    out.extend(pts);
    out.push(to);
    out
}

/// Flattens a cubic Bézier chain (4 points, then 3 per extra segment).
pub fn bezier_points(ctrl: &[Point]) -> Vec<Point> {
    if ctrl.len() < 4 {
        return ctrl.to_vec();
    }
    const STEPS: usize = 16;
    let mut out = vec![ctrl[0]];
    let mut i = 0;
    while i + 3 < ctrl.len() {
        let [p0, p1, p2, p3] = [ctrl[i], ctrl[i + 1], ctrl[i + 2], ctrl[i + 3]].map(|p| (p.x as f64, p.y as f64));
        for k in 1..=STEPS {
            let t = k as f64 / STEPS as f64;
            let u = 1.0 - t;
            let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
            let x = a * p0.0 + b * p1.0 + c * p2.0 + d * p3.0;
            let y = a * p0.1 + b * p1.1 + c * p2.1 + d * p3.1;
            out.push(Point::new(x.round() as Coord, y.round() as Coord));
        }
        i += 3;
    }
    out
}

/// A polyline item, or a line for two points. Explicitly closed outlines
/// (first == last) and filled shapes become closed polylines.
pub fn poly_item(mut points: Vec<Point>, width: Coord, layer: LayerId, fill: FillStyle) -> Option<Item> {
    points.dedup();
    let mut closed = fill != FillStyle::None;
    if points.len() > 2 && points.first() == points.last() {
        points.pop();
        closed = true;
    }
    match points.len() {
        0 | 1 => None,
        2 if fill == FillStyle::None => Some(Item::Line(Line { a: points[0], b: points[1], width, layer })),
        2 => Some(Item::Polyline(Polyline { points, closed: false, width, layer, fill: FillStyle::None })),
        _ => Some(Item::Polyline(Polyline { points, closed, width, layer, fill })),
    }
}

/// Collects repeated notes and writes one summarized message per kind.
#[derive(Debug, Default)]
pub struct Notes {
    entries: Vec<Note>,
}

#[derive(Debug)]
struct Note {
    severity: Severity,
    text: String,
    count: usize,
    examples: Vec<String>,
}

impl Notes {
    pub fn info(&mut self, text: &str, example: &str) {
        self.add(Severity::Info, text, example);
    }

    pub fn warn(&mut self, text: &str, example: &str) {
        self.add(Severity::Warning, text, example);
    }

    fn add(&mut self, severity: Severity, text: &str, example: &str) {
        let note = match self.entries.iter_mut().find(|n| n.text == text) {
            Some(n) => n,
            None => {
                self.entries.push(Note { severity, text: text.to_string(), count: 0, examples: Vec::new() });
                self.entries.last_mut().expect("just pushed")
            }
        };
        note.count += 1;
        if !example.is_empty() && note.examples.len() < 5 && !note.examples.iter().any(|e| e == example) {
            note.examples.push(example.to_string());
        }
    }

    pub fn flush(self, report: &mut ImportReport, context: &str) {
        for n in self.entries {
            let mut text = n.text;
            if n.count > 1 || !n.examples.is_empty() {
                text.push_str(&format!(" ({}x", n.count));
                if !n.examples.is_empty() {
                    text.push_str(", e.g. ");
                    text.push_str(&n.examples.join(", "));
                }
                text.push(')');
            }
            match n.severity {
                Severity::Info => report.info(context, text),
                Severity::Warning => report.warn(context, text),
            }
        }
    }
}

/// Map a structured format angle (degrees) to [0, 360).
pub fn angle(v: f64) -> f64 {
    normalize_deg(v)
}

/// Decodes structured format's escaped characters in net and field text, e.g.
/// `VPP{slash}MCLR` → `VPP/MCLR`. Unknown `{...}` sequences are kept.
pub fn unescape(s: &str) -> String {
    if !s.contains('{') {
        return s.to_string();
    }
    const TOKENS: [(&str, &str); 10] = [
        ("{slash}", "/"),
        ("{backslash}", "\\"),
        ("{lt}", "<"),
        ("{gt}", ">"),
        ("{colon}", ":"),
        ("{dblquote}", "\""),
        ("{quote}", "'"),
        ("{space}", " "),
        ("{tab}", "\t"),
        ("{return}", "\n"),
    ];
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    'outer: while let Some(i) = rest.find('{') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        for (tok, ch) in TOKENS {
            if let Some(r) = rest.strip_prefix(tok) {
                out.push_str(ch);
                rest = r;
                continue 'outer;
            }
        }
        out.push('{');
        rest = &rest[1..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod unescape_tests {
    use super::unescape;

    #[test]
    fn decodes_structured_escapes() {
        assert_eq!(unescape("VPP{slash}MCLR"), "VPP/MCLR");
        assert_eq!(unescape("A{lt}B{gt}"), "A<B>");
        assert_eq!(unescape("~{RESET}"), "~{RESET}");
        assert_eq!(unescape("plain"), "plain");
    }
}
