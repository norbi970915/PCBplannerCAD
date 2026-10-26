//! Gerber RS-274X / X2 and Excellon reader for the built-in viewer.
//!
//! Everything is converted to a flat list of [`GerberShape`]s in nanometers
//! (Y up), in file order, so the viewer can paint dark shapes and erase clear
//! ones (`dark == false`) in sequence. Flashes are expanded to circles and
//! polygons, regions become polygons, step-and-repeat and aperture blocks are
//! expanded. The parser never panics on malformed input: unknown or broken
//! commands are skipped and reported in [`GerberFile::warnings`].

use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

use pcbplannercad_core::{Coord, Point};
use serde::Serialize;

use crate::OutputError;

#[path = "excellon_read.rs"]
mod excellon_read;

/// One drawing primitive of a parsed file, in nanometers, Y up. Angles are
/// degrees counter-clockwise (same convention as `Arc`).
///
/// JSON form (externally tagged): `{"stroke":{"a":{x,y},"b":{x,y},"width":n}}`,
/// `{"arcStroke":{"center":…,"radius":n,"startAngle":deg,"sweep":deg,"width":n}}`,
/// `{"polygon":{"points":[…],"dark":bool}}`, `{"circle":{"center":…,"radius":n,"dark":bool}}`.
/// Strokes are always dark; clear-polarity draws are emitted as clear polygons.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum GerberShape {
    #[serde(rename_all = "camelCase")]
    Stroke { a: Point, b: Point, width: Coord },
    #[serde(rename_all = "camelCase")]
    ArcStroke { center: Point, radius: Coord, start_angle: f64, sweep: f64, width: Coord },
    #[serde(rename_all = "camelCase")]
    Polygon { points: Vec<Point>, dark: bool },
    #[serde(rename_all = "camelCase")]
    Circle { center: Point, radius: Coord, dark: bool },
}

impl GerberShape {
    fn translated(&self, dx: Coord, dy: Coord) -> GerberShape {
        let t = |p: &Point| Point::new(p.x.saturating_add(dx), p.y.saturating_add(dy));
        match self {
            GerberShape::Stroke { a, b, width } => GerberShape::Stroke { a: t(a), b: t(b), width: *width },
            GerberShape::ArcStroke { center, radius, start_angle, sweep, width } => {
                GerberShape::ArcStroke { center: t(center), radius: *radius, start_angle: *start_angle, sweep: *sweep, width: *width }
            }
            GerberShape::Polygon { points, dark } => GerberShape::Polygon { points: points.iter().map(t).collect(), dark: *dark },
            GerberShape::Circle { center, radius, dark } => GerberShape::Circle { center: t(center), radius: *radius, dark: *dark },
        }
    }

    /// Flips the polarity of polygons/circles (strokes stay as they are).
    fn inverted(&self) -> GerberShape {
        match self {
            GerberShape::Polygon { points, dark } => GerberShape::Polygon { points: points.clone(), dark: !dark },
            GerberShape::Circle { center, radius, dark } => GerberShape::Circle { center: *center, radius: *radius, dark: !dark },
            GerberShape::Stroke { a, b, width } => {
                GerberShape::Polygon { points: pcbplannercad_core::pcb::geometry::capsule_polygon(*a, *b, *width, 0), dark: false }
            }
            GerberShape::ArcStroke { center, radius, start_angle, sweep, width } => GerberShape::Polygon {
                points: arc_band((center.x as f64, center.y as f64), *radius as f64, *start_angle, *sweep, *width as f64)
                    .into_iter()
                    .map(to_point)
                    .collect(),
                dark: false,
            },
        }
    }

    fn extend_bounds(&self, b: &mut Option<(Point, Point)>) {
        let mut add = |x: i64, y: i64, r: i64| {
            let (lo, hi) = (Point::new(x.saturating_sub(r), y.saturating_sub(r)), Point::new(x.saturating_add(r), y.saturating_add(r)));
            *b = Some(match *b {
                None => (lo, hi),
                Some((mn, mx)) => (Point::new(mn.x.min(lo.x), mn.y.min(lo.y)), Point::new(mx.x.max(hi.x), mx.y.max(hi.y))),
            });
        };
        match self {
            GerberShape::Stroke { a, b, width } => {
                add(a.x, a.y, width / 2);
                add(b.x, b.y, width / 2);
            }
            GerberShape::ArcStroke { center, radius, start_angle, sweep, width } => {
                for p in arc_samples((center.x as f64, center.y as f64), *radius as f64, *start_angle, *sweep) {
                    let p = to_point(p);
                    add(p.x, p.y, width / 2);
                }
            }
            GerberShape::Polygon { points, .. } => points.iter().for_each(|p| add(p.x, p.y, 0)),
            GerberShape::Circle { center, radius, .. } => add(center.x, center.y, *radius),
        }
    }
}

/// A parsed Gerber or Excellon file.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GerberFile {
    pub name: String,
    /// `TF.FileFunction` value (e.g. `Copper,L1,Top`, `Plated,1,4,PTH`).
    pub function: Option<String>,
    pub shapes: Vec<GerberShape>,
    pub warnings: Vec<String>,
    /// (min, max) of everything drawn.
    pub bounds: Option<(Point, Point)>,
}

pub use excellon_read::parse_excellon;

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Coordinates are clamped to ±10 km so later integer arithmetic never overflows.
const MAX_NM: f64 = 1e13;
/// Hard cap on emitted shapes (step-and-repeat bombs, endless files).
const MAX_SHAPES: usize = 5_000_000;
const MAX_WARNINGS: usize = 100;

type P = (f64, f64);

fn clamp_nm(v: f64) -> f64 {
    if v.is_finite() { v.clamp(-MAX_NM, MAX_NM) } else { 0.0 }
}

fn to_point(p: P) -> Point {
    Point::new(clamp_nm(p.0).round() as Coord, clamp_nm(p.1).round() as Coord)
}

fn rot(p: P, deg: f64) -> P {
    if deg == 0.0 {
        return p;
    }
    let (s, c) = deg.to_radians().sin_cos();
    (p.0 * c - p.1 * s, p.0 * s + p.1 * c)
}

/// Points along an arc including both ends (≤ 5° per step).
fn arc_samples(c: P, r: f64, start_deg: f64, sweep_deg: f64) -> Vec<P> {
    let sweep = if sweep_deg.is_finite() { sweep_deg.clamp(-360.0, 360.0) } else { 0.0 };
    let n = ((sweep.abs() / 5.0).ceil() as usize).clamp(1, 72);
    (0..=n)
        .map(|i| {
            let a = (start_deg + sweep * i as f64 / n as f64).to_radians();
            (c.0 + r * a.cos(), c.1 + r * a.sin())
        })
        .collect()
}

/// Outline of an arc stroked with a round tool of `width`.
fn arc_band(c: P, r: f64, start: f64, sweep: f64, width: f64) -> Vec<P> {
    let hw = width / 2.0;
    let outer = arc_samples(c, r + hw, start, sweep);
    let inner = arc_samples(c, (r - hw).max(0.0), start, sweep);
    let mut pts = outer;
    // Round end cap at the end point.
    let end = start + sweep;
    let e = (c.0 + r * end.to_radians().cos(), c.1 + r * end.to_radians().sin());
    let dir = if sweep >= 0.0 { 1.0 } else { -1.0 };
    for i in 1..12 {
        let a = end + dir * 180.0 * i as f64 / 12.0;
        pts.push((e.0 + hw * a.to_radians().cos(), e.1 + hw * a.to_radians().sin()));
    }
    pts.extend(inner.into_iter().rev());
    let s = (c.0 + r * start.to_radians().cos(), c.1 + r * start.to_radians().sin());
    for i in 1..12 {
        let a = start + 180.0 + dir * 180.0 * i as f64 / 12.0;
        pts.push((s.0 + hw * a.to_radians().cos(), s.1 + hw * a.to_radians().sin()));
    }
    pts
}

fn circle_pts(c: P, r: f64, n: usize, start_deg: f64) -> Vec<P> {
    (0..n)
        .map(|i| {
            let a = (start_deg + 360.0 * i as f64 / n as f64).to_radians();
            (c.0 + r * a.cos(), c.1 + r * a.sin())
        })
        .collect()
}

fn signed_area(p: &[P]) -> f64 {
    let n = p.len();
    (0..n).map(|i| p[i].0 * p[(i + 1) % n].1 - p[(i + 1) % n].0 * p[i].1).sum::<f64>() / 2.0
}

/// Convex hull (monotone chain), counter-clockwise.
fn convex_hull(mut pts: Vec<P>) -> Vec<P> {
    pts.retain(|p| p.0.is_finite() && p.1.is_finite());
    pts.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    pts.dedup();
    if pts.len() < 3 {
        return pts;
    }
    let cross = |o: P, a: P, b: P| (a.0 - o.0) * (b.1 - o.1) - (a.1 - o.1) * (b.0 - o.0);
    let mut lower: Vec<P> = Vec::new();
    for &p in &pts {
        while lower.len() >= 2 && cross(lower[lower.len() - 2], lower[lower.len() - 1], p) <= 0.0 {
            lower.pop();
        }
        lower.push(p);
    }
    let mut upper: Vec<P> = Vec::new();
    for &p in pts.iter().rev() {
        while upper.len() >= 2 && cross(upper[upper.len() - 2], upper[upper.len() - 1], p) <= 0.0 {
            upper.pop();
        }
        upper.push(p);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    lower
}

/// A convex outline (centered on the origin) with a round hole, as one
/// polygon with a cut-in so it fills correctly without hole support.
fn keyhole(mut outer: Vec<P>, hole_r: f64) -> Vec<P> {
    if outer.len() < 3 || hole_r <= 0.0 {
        return outer;
    }
    if signed_area(&outer) < 0.0 {
        outer.reverse();
    }
    let start = outer[0];
    let theta = start.1.atan2(start.0).to_degrees();
    let mut pts = outer;
    pts.push(start);
    // Hole clockwise, starting and ending at the same angle as the bridge.
    pts.extend(arc_samples((0.0, 0.0), hole_r, theta, -360.0));
    pts
}

// ---------------------------------------------------------------------------
// Expressions (aperture macros)
// ---------------------------------------------------------------------------

struct Expr<'a> {
    s: &'a [u8],
    i: usize,
    vars: &'a HashMap<usize, f64>,
    depth: usize,
}

impl Expr<'_> {
    fn eval(src: &str, vars: &HashMap<usize, f64>) -> Option<f64> {
        let cleaned: String = src.chars().filter(|c| !c.is_whitespace()).collect();
        if cleaned.is_empty() {
            return None;
        }
        let mut e = Expr { s: cleaned.as_bytes(), i: 0, vars, depth: 0 };
        let v = e.sum()?;
        if e.i != e.s.len() {
            return None;
        }
        v.is_finite().then_some(v)
    }

    fn peek(&self) -> Option<u8> {
        self.s.get(self.i).copied()
    }

    fn sum(&mut self) -> Option<f64> {
        let mut v = self.product()?;
        while let Some(c @ (b'+' | b'-')) = self.peek() {
            self.i += 1;
            let r = self.product()?;
            v = if c == b'+' { v + r } else { v - r };
        }
        Some(v)
    }

    fn product(&mut self) -> Option<f64> {
        let mut v = self.unary()?;
        while let Some(c @ (b'x' | b'X' | b'/')) = self.peek() {
            self.i += 1;
            let r = self.unary()?;
            v = if c == b'/' { if r == 0.0 { 0.0 } else { v / r } } else { v * r };
        }
        Some(v)
    }

    fn unary(&mut self) -> Option<f64> {
        self.depth += 1;
        if self.depth > 64 {
            return None;
        }
        let r = match self.peek()? {
            b'-' => {
                self.i += 1;
                self.unary().map(|v| -v)
            }
            b'+' => {
                self.i += 1;
                self.unary()
            }
            b'(' => {
                self.i += 1;
                let v = self.sum()?;
                if self.peek() != Some(b')') {
                    return None;
                }
                self.i += 1;
                Some(v)
            }
            b'$' => {
                self.i += 1;
                let st = self.i;
                while self.peek().is_some_and(|c| c.is_ascii_digit()) {
                    self.i += 1;
                }
                let n: usize = std::str::from_utf8(&self.s[st..self.i]).ok()?.parse().ok()?;
                Some(self.vars.get(&n).copied().unwrap_or(0.0))
            }
            _ => {
                let st = self.i;
                while self.peek().is_some_and(|c| c.is_ascii_digit() || c == b'.') {
                    self.i += 1;
                }
                std::str::from_utf8(&self.s[st..self.i]).ok()?.parse().ok()
            }
        };
        self.depth -= 1;
        r
    }
}

// ---------------------------------------------------------------------------
// Apertures
// ---------------------------------------------------------------------------

/// Aperture primitive in local coordinates (nm).
#[derive(Clone, Debug)]
enum Prim {
    Circle { c: P, r: f64, dark: bool },
    Poly { pts: Vec<P>, dark: bool },
}

#[derive(Clone, Debug)]
enum Draw {
    /// Round tool of this diameter.
    Round(f64),
    /// Convex tool outline (rect, obround, polygon): draws sweep its hull.
    Hull(Vec<P>),
    /// Macro apertures cannot draw.
    None,
}

#[derive(Clone, Debug)]
enum Aperture {
    Std { prims: Vec<Prim>, draw: Draw },
    Block(Vec<GerberShape>),
}

fn rect_pts(c: P, w: f64, h: f64) -> Vec<P> {
    let (hw, hh) = (w / 2.0, h / 2.0);
    vec![(c.0 + hw, c.1 + hh), (c.0 - hw, c.1 + hh), (c.0 - hw, c.1 - hh), (c.0 + hw, c.1 - hh)]
}

fn obround_pts(w: f64, h: f64) -> Vec<P> {
    let r = w.min(h) / 2.0;
    let mut pts = Vec::new();
    if w >= h {
        let d = w / 2.0 - r;
        pts.extend(arc_samples((d, 0.0), r, -90.0, 180.0));
        pts.extend(arc_samples((-d, 0.0), r, 90.0, 180.0));
    } else {
        let d = h / 2.0 - r;
        pts.extend(arc_samples((0.0, d), r, 0.0, 180.0));
        pts.extend(arc_samples((0.0, -d), r, 180.0, 180.0));
    }
    pts
}

/// Rectangle of width `w` from `s` to `e` (butt ends).
fn vector_line(s: P, e: P, w: f64) -> Vec<P> {
    let (dx, dy) = (e.0 - s.0, e.1 - s.1);
    let len = (dx * dx + dy * dy).sqrt();
    let (nx, ny) = if len > 0.0 { (-dy / len * w / 2.0, dx / len * w / 2.0) } else { (0.0, w / 2.0) };
    vec![(s.0 + nx, s.1 + ny), (s.0 - nx, s.1 - ny), (e.0 - nx, e.1 - ny), (e.0 + nx, e.1 + ny)]
}

/// One quadrant piece of a thermal: the ring between `ri` and `ro` outside a
/// cross of width `gap` (first quadrant, before rotation).
fn thermal_quadrant(ri: f64, ro: f64, gap: f64) -> Vec<P> {
    let g = gap / 2.0;
    if ro <= g * std::f64::consts::SQRT_2 {
        return vec![];
    }
    let mut pts = Vec::new();
    let a0 = (g / ro).clamp(-1.0, 1.0).asin().to_degrees();
    pts.extend(arc_samples((0.0, 0.0), ro, a0, 90.0 - 2.0 * a0));
    if ri > g * std::f64::consts::SQRT_2 {
        let b0 = (g / ri).clamp(-1.0, 1.0).asin().to_degrees();
        pts.extend(arc_samples((0.0, 0.0), ri, 90.0 - b0, -(90.0 - 2.0 * b0)));
    } else {
        pts.push((g, g));
    }
    pts
}

// ---------------------------------------------------------------------------
// Gerber parser
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
enum Interp {
    Linear,
    Cw,
    Ccw,
}

enum FrameKind {
    Main,
    StepRepeat { nx: u32, ny: u32, dx: f64, dy: f64 },
    Block(i32),
}

struct Frame {
    kind: FrameKind,
    shapes: Vec<GerberShape>,
}

struct Region {
    contours: Vec<Vec<Point>>,
    current: Vec<Point>,
}

struct Gerber {
    unit_nm: f64,
    units_seen: bool,
    int_digits: u32,
    dec_digits: u32,
    trailing_omitted: bool,
    incremental: bool,
    macros: HashMap<String, Vec<String>>,
    apertures: HashMap<i32, Aperture>,
    current: Option<i32>,
    pos: (i64, i64),
    interp: Interp,
    multi_quadrant: bool,
    last_op: i32,
    region: Option<Region>,
    dark: bool,
    mirror: (bool, bool),
    rotation: f64,
    scale: f64,
    offset: (f64, f64),
    frames: Vec<Frame>,
    emitted: usize,
    warnings: Vec<String>,
    function: Option<String>,
    ended: bool,
}

/// Splits Gerber text into extended (`%…%`) and word (`…*`) commands.
enum Token {
    Ext(String),
    Word(String),
}

fn tokenize(text: &str) -> Vec<Token> {
    let mut out = Vec::new();
    let mut word = String::new();
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match c {
            '%' if word.trim().is_empty() => {
                word.clear();
                let mut ext = String::new();
                for c in chars.by_ref() {
                    if c == '%' {
                        break;
                    }
                    if c != '\r' && c != '\n' {
                        ext.push(c);
                    }
                }
                out.push(Token::Ext(ext));
            }
            '*' => {
                let w = word.trim();
                if !w.is_empty() {
                    out.push(Token::Word(w.to_string()));
                }
                word.clear();
            }
            '\r' | '\n' => {}
            _ => word.push(c),
        }
    }
    let w = word.trim();
    if !w.is_empty() {
        out.push(Token::Word(w.to_string()));
    }
    out
}

/// Parses an RS-274X / X2 Gerber file.
pub fn parse_gerber(name: &str, text: &str) -> Result<GerberFile, OutputError> {
    if !text.contains('*') {
        return Err(OutputError::Invalid(format!("{name}: not a Gerber file")));
    }
    let mut g = Gerber {
        unit_nm: 1e6,
        units_seen: false,
        int_digits: 3,
        dec_digits: 6,
        trailing_omitted: false,
        incremental: false,
        macros: HashMap::new(),
        apertures: HashMap::new(),
        current: None,
        pos: (0, 0),
        interp: Interp::Linear,
        multi_quadrant: true,
        last_op: 1,
        region: None,
        dark: true,
        mirror: (false, false),
        rotation: 0.0,
        scale: 1.0,
        offset: (0.0, 0.0),
        frames: vec![Frame { kind: FrameKind::Main, shapes: Vec::new() }],
        emitted: 0,
        warnings: Vec::new(),
        function: None,
        ended: false,
    };
    let mut fs_seen = false;
    for tok in tokenize(text) {
        if g.ended {
            break;
        }
        match tok {
            Token::Ext(s) => {
                if s.trim_start().starts_with("FS") {
                    fs_seen = true;
                }
                g.extended(&s);
            }
            Token::Word(w) => g.word(&w),
        }
    }
    if !g.ended {
        g.warn("missing M02 end-of-file");
    }
    if !fs_seen {
        g.warn("missing %FS% format specification; assuming 3.6");
    }
    if !g.units_seen {
        g.warn("missing %MO% units; assuming millimeters");
    }
    if g.region.is_some() {
        g.warn("unterminated region (G36 without G37)");
        g.end_region();
    }
    while g.frames.len() > 1 {
        g.warn("unterminated step-and-repeat or aperture block");
        g.close_frame();
    }
    let shapes = g.frames.pop().map(|f| f.shapes).unwrap_or_default();
    let mut bounds = None;
    shapes.iter().for_each(|s| s.extend_bounds(&mut bounds));
    Ok(GerberFile { name: name.to_string(), function: g.function, shapes, warnings: g.warnings, bounds })
}

impl Gerber {
    fn warn(&mut self, msg: impl Into<String>) {
        let msg = msg.into();
        if self.warnings.len() < MAX_WARNINGS && !self.warnings.contains(&msg) {
            self.warnings.push(msg);
        }
    }

    fn emit(&mut self, s: GerberShape) {
        if self.emitted >= MAX_SHAPES {
            self.warn("too many shapes; output truncated");
            return;
        }
        self.emitted += 1;
        if let Some(f) = self.frames.last_mut() {
            f.shapes.push(s);
        }
    }

    fn num(&mut self, s: &str) -> f64 {
        match s.trim().parse::<f64>() {
            Ok(v) if v.is_finite() => v,
            _ => {
                self.warn(format!("invalid number '{s}'"));
                0.0
            }
        }
    }

    /// File coordinate string → nanometers.
    fn coord(&mut self, s: &str) -> f64 {
        let s = s.trim();
        let v = if s.contains('.') {
            s.parse::<f64>().ok()
        } else {
            let (neg, digits) = match s.as_bytes().first() {
                Some(b'-') => (true, &s[1..]),
                Some(b'+') => (false, &s[1..]),
                _ => (false, s),
            };
            if digits.is_empty() || !digits.bytes().all(|c| c.is_ascii_digit()) {
                None
            } else {
                let total = (self.int_digits + self.dec_digits) as usize;
                let mut d = digits.to_string();
                if self.trailing_omitted && d.len() < total {
                    d.extend(std::iter::repeat_n('0', total - d.len()));
                }
                d.parse::<f64>().ok().map(|v| {
                    let v = v / 10f64.powi(self.dec_digits as i32);
                    if neg { -v } else { v }
                })
            }
        };
        match v {
            Some(v) if v.is_finite() => clamp_nm(v * self.unit_nm),
            _ => {
                self.warn(format!("invalid coordinate '{s}'"));
                0.0
            }
        }
    }

    // ----- extended commands -----

    fn extended(&mut self, block: &str) {
        let block = block.trim();
        if let Some(rest) = block.strip_prefix("AM") {
            let mut parts = rest.split('*');
            let name = parts.next().unwrap_or("").trim().to_string();
            let body: Vec<String> = parts.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
            if name.is_empty() {
                self.warn("aperture macro without a name");
            } else {
                self.macros.insert(name, body);
            }
            return;
        }
        for cmd in block.split('*') {
            let cmd = cmd.trim();
            if !cmd.is_empty() {
                self.ext_command(cmd);
            }
        }
    }

    fn ext_command(&mut self, cmd: &str) {
        let code = cmd.get(..2).unwrap_or(cmd);
        let rest = cmd.get(2..).unwrap_or("");
        match code {
            "FS" => self.format_spec(rest),
            "MO" => match rest {
                "MM" => {
                    self.unit_nm = 1e6;
                    self.units_seen = true;
                }
                "IN" => {
                    self.unit_nm = 25.4e6;
                    self.units_seen = true;
                }
                _ => self.warn(format!("unknown unit '{rest}'")),
            },
            "AD" => self.aperture_def(rest),
            "LP" => match rest {
                "D" => self.dark = true,
                "C" => self.dark = false,
                _ => self.warn(format!("unknown polarity '{rest}'")),
            },
            "LM" => {
                self.mirror = match rest {
                    "N" => (false, false),
                    "X" => (true, false),
                    "Y" => (false, true),
                    "XY" => (true, true),
                    _ => {
                        self.warn(format!("unknown mirroring '{rest}'"));
                        (false, false)
                    }
                }
            }
            "LR" => self.rotation = self.num(rest),
            "LS" => {
                let s = self.num(rest);
                self.scale = if s > 0.0 { s } else { 1.0 };
            }
            "SR" => self.step_repeat(rest),
            "AB" => self.aperture_block(rest),
            "TF" => {
                if let Some(v) = rest.strip_prefix(".FileFunction,") {
                    self.function = Some(v.trim().to_string());
                }
            }
            "TA" | "TO" | "TD" | "IN" | "LN" => {}
            "IP" => {
                if rest == "NEG" {
                    self.warn("negative image polarity (%IPNEG%) is not supported; drawn as positive");
                }
            }
            "OF" => {
                let (a, b) = (field_after(rest, 'A'), field_after(rest, 'B'));
                let a = a.map(|v| self.num(&v)).unwrap_or(0.0) * self.unit_nm;
                let b = b.map(|v| self.num(&v)).unwrap_or(0.0) * self.unit_nm;
                self.offset = (clamp_nm(a), clamp_nm(b));
            }
            "SF" | "MI" | "IR" | "AS" => {
                let identity = matches!(cmd, "SFA1B1" | "SFA1.0B1.0" | "MIA0B0" | "IR0" | "ASAXBY");
                if !identity {
                    self.warn(format!("deprecated image transform %{cmd}% ignored"));
                }
            }
            _ => self.warn(format!("unknown extended command %{cmd}%")),
        }
    }

    fn format_spec(&mut self, rest: &str) {
        let b = rest.as_bytes();
        let mut i = 0;
        while i < b.len() && b[i] != b'X' {
            match b[i] {
                b'T' => self.trailing_omitted = true,
                b'L' => self.trailing_omitted = false,
                b'I' => self.incremental = true,
                b'A' => self.incremental = false,
                _ => {}
            }
            i += 1;
        }
        if i + 2 < b.len() && b[i + 1].is_ascii_digit() && b[i + 2].is_ascii_digit() {
            self.int_digits = (b[i + 1] - b'0') as u32;
            self.dec_digits = (b[i + 2] - b'0') as u32;
        } else {
            self.warn(format!("bad format specification FS{rest}"));
        }
        if self.incremental {
            self.warn("incremental coordinates are deprecated");
        }
    }

    fn aperture_def(&mut self, rest: &str) {
        let Some(rest) = rest.strip_prefix('D') else {
            self.warn(format!("bad aperture definition AD{rest}"));
            return;
        };
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        let Ok(code) = digits.parse::<i32>() else {
            self.warn(format!("bad aperture number in AD{rest}"));
            return;
        };
        let after = &rest[digits.len()..];
        let (template, params) = match after.find(',') {
            Some(i) => (&after[..i], &after[i + 1..]),
            None => (after, ""),
        };
        let mut nums = Vec::new();
        for p in params.split('X').map(str::trim).filter(|s| !s.is_empty()) {
            nums.push(self.num(p));
        }
        let u = self.unit_nm;
        let arg = |i: usize| nums.get(i).copied().unwrap_or(0.0);
        let ap = match template {
            "C" => {
                let d = arg(0) * u;
                let hole = arg(1) * u / 2.0;
                let prims = if hole > 0.0 {
                    vec![Prim::Poly { pts: keyhole(circle_pts((0.0, 0.0), d / 2.0, 64, 0.0), hole), dark: true }]
                } else {
                    vec![Prim::Circle { c: (0.0, 0.0), r: d / 2.0, dark: true }]
                };
                Aperture::Std { prims, draw: Draw::Round(d) }
            }
            "R" | "O" => {
                let (w, h) = (arg(0) * u, if nums.len() > 1 { arg(1) * u } else { arg(0) * u });
                let outline = if template == "R" { rect_pts((0.0, 0.0), w, h) } else { obround_pts(w, h) };
                let hole = arg(2) * u / 2.0;
                Aperture::Std { prims: vec![Prim::Poly { pts: keyhole(outline.clone(), hole), dark: true }], draw: Draw::Hull(outline) }
            }
            "P" => {
                let d = arg(0) * u;
                let n = (arg(1).round() as i64).clamp(3, 64) as usize;
                let outline = circle_pts((0.0, 0.0), d / 2.0, n, arg(2));
                let hole = arg(3) * u / 2.0;
                Aperture::Std { prims: vec![Prim::Poly { pts: keyhole(outline.clone(), hole), dark: true }], draw: Draw::Hull(outline) }
            }
            name => match self.macros.get(name).cloned() {
                Some(body) => Aperture::Std { prims: self.eval_macro(name, &body, &nums), draw: Draw::None },
                None => {
                    self.warn(format!("aperture D{code} uses undefined macro '{name}'"));
                    Aperture::Std { prims: vec![], draw: Draw::None }
                }
            },
        };
        self.apertures.insert(code, ap);
    }

    fn eval_macro(&mut self, name: &str, body: &[String], params: &[f64]) -> Vec<Prim> {
        let u = self.unit_nm;
        let mut vars: HashMap<usize, f64> = params.iter().enumerate().map(|(i, v)| (i + 1, *v)).collect();
        let mut prims = Vec::new();
        for stmt in body {
            let stmt = stmt.trim();
            if stmt.starts_with('$') && stmt.contains('=') {
                let (lhs, rhs) = stmt.split_once('=').unwrap_or((stmt, ""));
                let idx = lhs.trim_start_matches('$').trim().parse::<usize>();
                match (idx, Expr::eval(rhs, &vars)) {
                    (Ok(i), Some(v)) => {
                        vars.insert(i, v);
                    }
                    _ => self.warn(format!("macro {name}: bad assignment '{stmt}'")),
                }
                continue;
            }
            let fields: Vec<&str> = stmt.split(',').collect();
            let head = fields[0].trim();
            let code = match head.parse::<i32>() {
                Ok(0) => continue,
                Ok(c) => c,
                Err(_) if head.starts_with('0') => continue, // comment
                Err(_) => {
                    self.warn(format!("macro {name}: bad primitive '{stmt}'"));
                    continue;
                }
            };
            let mut v = Vec::with_capacity(fields.len());
            for f in &fields[1..] {
                if f.trim().is_empty() {
                    continue;
                }
                match Expr::eval(f, &vars) {
                    Some(x) => v.push(x),
                    None => {
                        self.warn(format!("macro {name}: bad expression '{f}'"));
                        v.push(0.0);
                    }
                }
            }
            let a = |i: usize| v.get(i).copied().unwrap_or(0.0);
            let on = a(0) != 0.0;
            let scale_pts = |pts: Vec<P>, r: f64| -> Vec<P> { pts.into_iter().map(|p| rot((p.0 * u, p.1 * u), r)).collect() };
            match code {
                1 => {
                    let c = rot((a(2) * u, a(3) * u), a(4));
                    prims.push(Prim::Circle { c, r: a(1).abs() * u / 2.0, dark: on });
                }
                2 | 20 => prims.push(Prim::Poly { pts: scale_pts(vector_line((a(2), a(3)), (a(4), a(5)), a(1)), a(6)), dark: on }),
                21 => prims.push(Prim::Poly { pts: scale_pts(rect_pts((a(3), a(4)), a(1), a(2)), a(5)), dark: on }),
                22 => {
                    let (w, h) = (a(1), a(2));
                    prims.push(Prim::Poly { pts: scale_pts(rect_pts((a(3) + w / 2.0, a(4) + h / 2.0), w, h), a(5)), dark: on });
                }
                4 => {
                    let n = (a(1).max(0.0) as usize).min(100_000);
                    let coords = &v[2.min(v.len())..];
                    let have = (coords.len() / 2).min(n + 1);
                    let rotation = if coords.len() > 2 * (n + 1) { coords[2 * (n + 1)] } else { 0.0 };
                    let mut pts: Vec<P> = (0..have).map(|k| (coords[2 * k], coords[2 * k + 1])).collect();
                    if pts.len() > 1 && pts.first() == pts.last() {
                        pts.pop();
                    }
                    if pts.len() >= 3 {
                        prims.push(Prim::Poly { pts: scale_pts(pts, rotation), dark: on });
                    }
                }
                5 => {
                    let n = (a(1).round() as i64).clamp(3, 64) as usize;
                    let pts = circle_pts((a(2), a(3)), a(4) / 2.0, n, 0.0);
                    prims.push(Prim::Poly { pts: scale_pts(pts, a(5)), dark: on });
                }
                6 => {
                    // Moiré (deprecated): rings plus crosshair.
                    let (c, mut od, ring, gap, n) = ((a(0), a(1)), a(2), a(3), a(4), a(5).max(0.0) as usize);
                    let (ct, cl, r) = (a(6), a(7), a(8));
                    for _ in 0..n.min(100) {
                        if od <= 0.0 {
                            break;
                        }
                        let outer = circle_pts((0.0, 0.0), od / 2.0, 64, 0.0);
                        let ring_pts = keyhole(outer, (od / 2.0 - ring).max(0.0));
                        prims.push(Prim::Poly { pts: scale_pts(ring_pts.into_iter().map(|p| (p.0 + c.0, p.1 + c.1)).collect(), r), dark: true });
                        od -= 2.0 * (ring + gap);
                    }
                    prims.push(Prim::Poly { pts: scale_pts(rect_pts(c, cl, ct), r), dark: true });
                    prims.push(Prim::Poly { pts: scale_pts(rect_pts(c, ct, cl), r), dark: true });
                }
                7 => {
                    let (c, od, id, gap, r) = ((a(0), a(1)), a(2), a(3), a(4), a(5));
                    let q = thermal_quadrant(id / 2.0, od / 2.0, gap);
                    if q.len() >= 3 {
                        for k in 0..4 {
                            let pts = q.iter().map(|p| {
                                let p = rot(*p, 90.0 * k as f64);
                                (p.0 + c.0, p.1 + c.1)
                            });
                            prims.push(Prim::Poly { pts: scale_pts(pts.collect(), r), dark: true });
                        }
                    }
                }
                _ => self.warn(format!("macro {name}: unsupported primitive {code}")),
            }
        }
        prims
    }

    fn step_repeat(&mut self, rest: &str) {
        if self.frames.iter().any(|f| matches!(f.kind, FrameKind::StepRepeat { .. })) {
            // An SR command always closes the open one first.
            while let Some(i) = self.frames.iter().rposition(|f| matches!(f.kind, FrameKind::StepRepeat { .. })) {
                while self.frames.len() > i {
                    self.close_frame();
                }
            }
        }
        if rest.is_empty() {
            return;
        }
        let get = |c: char| field_after(rest, c);
        let nx = get('X').and_then(|s| s.parse::<f64>().ok()).unwrap_or(1.0).clamp(1.0, 10_000.0) as u32;
        let ny = get('Y').and_then(|s| s.parse::<f64>().ok()).unwrap_or(1.0).clamp(1.0, 10_000.0) as u32;
        // At most 100 000 copies.
        let ny = ny.min((100_000 / nx).max(1));
        let dx = clamp_nm(get('I').and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0) * self.unit_nm);
        let dy = clamp_nm(get('J').and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0) * self.unit_nm);
        if nx == 1 && ny == 1 {
            return;
        }
        self.frames.push(Frame { kind: FrameKind::StepRepeat { nx, ny, dx, dy }, shapes: Vec::new() });
    }

    fn aperture_block(&mut self, rest: &str) {
        if rest.is_empty() {
            match self.frames.iter().rposition(|f| matches!(f.kind, FrameKind::Block(_))) {
                Some(i) => {
                    while self.frames.len() > i {
                        self.close_frame();
                    }
                }
                None => self.warn("%AB% close without an open block"),
            }
            return;
        }
        match rest.strip_prefix('D').and_then(|d| d.parse::<i32>().ok()) {
            Some(code) => self.frames.push(Frame { kind: FrameKind::Block(code), shapes: Vec::new() }),
            None => self.warn(format!("bad aperture block AB{rest}")),
        }
    }

    fn close_frame(&mut self) {
        if self.frames.len() <= 1 {
            return;
        }
        let Some(frame) = self.frames.pop() else { return };
        match frame.kind {
            FrameKind::Main => {}
            FrameKind::StepRepeat { nx, ny, dx, dy } => {
                let total = frame.shapes.len().saturating_mul(nx as usize * ny as usize);
                if self.emitted.saturating_add(total) > MAX_SHAPES {
                    self.warn("step-and-repeat too large; output truncated");
                }
                for iy in 0..ny {
                    for ix in 0..nx {
                        let (ox, oy) = ((dx * ix as f64).round() as Coord, (dy * iy as f64).round() as Coord);
                        for s in &frame.shapes {
                            if self.emitted >= MAX_SHAPES {
                                return;
                            }
                            let t = s.translated(ox, oy);
                            if let Some(f) = self.frames.last_mut() {
                                f.shapes.push(t);
                            }
                            self.emitted += 1;
                        }
                    }
                }
                // The original copies were counted once already.
                self.emitted = self.emitted.saturating_sub(frame.shapes.len());
            }
            FrameKind::Block(code) => {
                self.apertures.insert(code, Aperture::Block(frame.shapes));
            }
        }
    }

    // ----- word commands -----

    fn word(&mut self, w: &str) {
        if let Some(c) = w.strip_prefix("G04").or_else(|| w.strip_prefix("G4 ")) {
            let c = c.trim();
            if let Some(attr) = c.strip_prefix("#@!") {
                let attr = attr.trim();
                if let Some(v) = attr.strip_prefix("TF.FileFunction,") {
                    self.function = Some(v.trim().to_string());
                }
            }
            return;
        }
        let b = w.as_bytes();
        let mut i = 0;
        let (mut x, mut y, mut ii, mut jj): (Option<String>, Option<String>, Option<String>, Option<String>) = (None, None, None, None);
        let mut d: Option<i32> = None;
        while i < b.len() {
            let letter = b[i].to_ascii_uppercase();
            i += 1;
            let st = i;
            while i < b.len() && (b[i].is_ascii_digit() || matches!(b[i], b'+' | b'-' | b'.')) {
                i += 1;
            }
            let val = std::str::from_utf8(&b[st..i]).unwrap_or("");
            match letter {
                b'G' => match val.parse::<i32>() {
                    Ok(code) => self.g_code(code),
                    Err(_) => self.warn(format!("bad G code in '{w}'")),
                },
                b'X' => x = Some(val.to_string()),
                b'Y' => y = Some(val.to_string()),
                b'I' => ii = Some(val.to_string()),
                b'J' => jj = Some(val.to_string()),
                b'D' => d = val.parse::<i32>().ok(),
                b'M' => {
                    if matches!(val.parse::<i32>(), Ok(0..=2)) {
                        self.ended = true;
                    }
                }
                b' ' | b'\t' => {}
                _ => {
                    self.warn(format!("unknown command '{w}'"));
                    return;
                }
            }
        }
        if let Some(code) = d
            && code >= 10
        {
            self.current = Some(code);
            if !self.apertures.contains_key(&code) {
                self.warn(format!("aperture D{code} selected but not defined"));
            }
            return;
        }
        let has_coords = x.is_some() || y.is_some() || ii.is_some() || jj.is_some();
        let op = match d {
            Some(op @ 1..=3) => op,
            Some(other) => {
                self.warn(format!("unknown D code D{other}"));
                return;
            }
            None if has_coords => self.last_op,
            None => return,
        };
        self.last_op = op;
        let mut target = self.pos;
        let fx = x.map(|s| self.coord(&s) + self.offset.0);
        let fy = y.map(|s| self.coord(&s) + self.offset.1);
        if self.incremental {
            target.0 = target.0.saturating_add(fx.map_or(0, |v| v.round() as i64));
            target.1 = target.1.saturating_add(fy.map_or(0, |v| v.round() as i64));
        } else {
            if let Some(v) = fx {
                target.0 = v.round() as i64;
            }
            if let Some(v) = fy {
                target.1 = v.round() as i64;
            }
        }
        let ci = ii.map(|s| self.coord(&s)).unwrap_or(0.0);
        let cj = jj.map(|s| self.coord(&s)).unwrap_or(0.0);
        match op {
            1 => self.interpolate(target, ci, cj),
            2 => {
                if let Some(r) = &mut self.region {
                    if r.current.len() >= 3 {
                        let c = std::mem::take(&mut r.current);
                        r.contours.push(c);
                    }
                    r.current.clear();
                }
            }
            _ => {
                if self.region.is_some() {
                    self.warn("flash (D03) inside a region ignored");
                } else {
                    self.flash(target);
                }
            }
        }
        self.pos = target;
    }

    fn g_code(&mut self, code: i32) {
        match code {
            1 => self.interp = Interp::Linear,
            2 => self.interp = Interp::Cw,
            3 => self.interp = Interp::Ccw,
            36 => {
                if self.region.is_some() {
                    self.end_region();
                }
                self.region = Some(Region { contours: Vec::new(), current: Vec::new() });
            }
            37 => {
                if self.region.is_none() {
                    self.warn("G37 without G36");
                }
                self.end_region();
            }
            74 => self.multi_quadrant = false,
            75 => self.multi_quadrant = true,
            70 => {
                self.unit_nm = 25.4e6;
                self.units_seen = true;
            }
            71 => {
                self.unit_nm = 1e6;
                self.units_seen = true;
            }
            90 => self.incremental = false,
            91 => self.incremental = true,
            54 | 55 => {}
            other => self.warn(format!("unsupported G code G{other:02}")),
        }
    }

    fn end_region(&mut self) {
        let Some(mut r) = self.region.take() else { return };
        if r.current.len() >= 3 {
            r.contours.push(std::mem::take(&mut r.current));
        }
        for mut c in r.contours {
            if c.len() > 3 && c.first() == c.last() {
                c.pop();
            }
            if c.len() >= 3 {
                self.emit(GerberShape::Polygon { points: c, dark: self.dark });
            }
        }
    }

    /// Center, radius, start angle and sweep (degrees) of an arc from the
    /// current point to `end`.
    fn arc_geom(&mut self, start: (i64, i64), end: (i64, i64), i: f64, j: f64) -> (P, f64, f64, f64) {
        let s = (start.0 as f64, start.1 as f64);
        let e = (end.0 as f64, end.1 as f64);
        let cw = self.interp == Interp::Cw;
        let sweep_of = |c: P, full: bool| -> (f64, f64, f64) {
            let a0 = (s.1 - c.1).atan2(s.0 - c.0).to_degrees();
            let a1 = (e.1 - c.1).atan2(e.0 - c.0).to_degrees();
            let mut sw = if cw { -(a0 - a1).rem_euclid(360.0) } else { (a1 - a0).rem_euclid(360.0) };
            if full && sw.abs() < 1e-9 {
                sw = if cw { -360.0 } else { 360.0 };
            }
            let rs = ((s.0 - c.0).powi(2) + (s.1 - c.1).powi(2)).sqrt();
            let re = ((e.0 - c.0).powi(2) + (e.1 - c.1).powi(2)).sqrt();
            (a0, sw, (rs + re) / 2.0)
        };
        if self.multi_quadrant {
            let c = (s.0 + i, s.1 + j);
            let (a0, sw, r) = sweep_of(c, start == end);
            (c, r, a0, sw)
        } else {
            let mut best: Option<(f64, P)> = None;
            for (si, sj) in [(1.0, 1.0), (-1.0, 1.0), (1.0, -1.0), (-1.0, -1.0)] {
                let c = (s.0 + si * i.abs(), s.1 + sj * j.abs());
                let (_, sw, _) = sweep_of(c, false);
                let rs = ((s.0 - c.0).powi(2) + (s.1 - c.1).powi(2)).sqrt();
                let re = ((e.0 - c.0).powi(2) + (e.1 - c.1).powi(2)).sqrt();
                let err = (rs - re).abs() + if sw.abs() > 90.0 + 1e-6 { 1e18 } else { 0.0 };
                if best.is_none_or(|(b, _)| err < b) {
                    best = Some((err, c));
                }
            }
            let c = best.map_or(s, |(_, c)| c);
            let (a0, sw, r) = sweep_of(c, false);
            (c, r, a0, sw)
        }
    }

    fn interpolate(&mut self, end: (i64, i64), i: f64, j: f64) {
        let start = self.pos;
        let arc = if self.interp == Interp::Linear { None } else { Some(self.arc_geom(start, end, i, j)) };
        if let Some(r) = &mut self.region {
            if r.current.is_empty() {
                r.current.push(Point::new(start.0, start.1));
            }
            match arc {
                None => r.current.push(Point::new(end.0, end.1)),
                Some((c, rad, a0, sw)) => {
                    let pts = arc_samples(c, rad, a0, sw);
                    r.current.extend(pts.into_iter().skip(1).map(to_point));
                    if let Some(last) = r.current.last_mut() {
                        *last = Point::new(end.0, end.1);
                    }
                }
            }
            return;
        }
        let draw = match self.current.and_then(|c| self.apertures.get(&c)) {
            Some(Aperture::Std { draw, .. }) => draw.clone(),
            Some(Aperture::Block(_)) => {
                self.warn("draw with a block aperture ignored");
                return;
            }
            None => {
                self.warn("draw without a defined aperture; using zero width");
                Draw::Round(0.0)
            }
        };
        let (a, b) = (Point::new(start.0, start.1), Point::new(end.0, end.1));
        let sc = self.scale;
        match (arc, draw) {
            (None, Draw::Round(d)) => {
                let width = (d * sc).round() as Coord;
                let s = GerberShape::Stroke { a, b, width };
                if self.dark {
                    self.emit(s);
                } else if width > 0 {
                    self.emit(s.inverted());
                }
            }
            (None, Draw::Hull(outline)) => {
                let tool: Vec<P> = outline.iter().map(|p| self.transform(*p)).collect();
                let mut pts: Vec<P> = tool.iter().map(|p| (p.0 + start.0 as f64, p.1 + start.1 as f64)).collect();
                pts.extend(tool.iter().map(|p| (p.0 + end.0 as f64, p.1 + end.1 as f64)));
                let hull = convex_hull(pts);
                if hull.len() >= 3 {
                    self.emit(GerberShape::Polygon { points: hull.into_iter().map(to_point).collect(), dark: self.dark });
                }
            }
            (Some((c, r, a0, sw)), draw) => {
                let d = match draw {
                    Draw::Round(d) => d,
                    Draw::Hull(outline) => {
                        self.warn("arc drawn with a non-circular aperture; approximated with a round tool");
                        let hull = convex_hull(outline);
                        hull.iter().map(|p| (p.0 * p.0 + p.1 * p.1).sqrt()).fold(f64::INFINITY, f64::min).min(1e12) * 2.0
                    }
                    Draw::None => {
                        self.warn("draw with a macro aperture; drawn as a thin line");
                        0.0
                    }
                };
                let s = GerberShape::ArcStroke {
                    center: to_point(c),
                    radius: clamp_nm(r).round() as Coord,
                    start_angle: a0,
                    sweep: sw,
                    width: (d * sc).round() as Coord,
                };
                if self.dark {
                    self.emit(s);
                } else if d > 0.0 {
                    self.emit(s.inverted());
                }
            }
            (None, Draw::None) => {
                self.warn("draw with a macro aperture; drawn as a thin line");
                if self.dark {
                    self.emit(GerberShape::Stroke { a, b, width: 0 });
                }
            }
        }
    }

    /// Applies LM / LR / LS to an aperture-local point.
    fn transform(&self, p: P) -> P {
        let mut p = p;
        if self.mirror.0 {
            p.0 = -p.0;
        }
        if self.mirror.1 {
            p.1 = -p.1;
        }
        let p = rot(p, self.rotation);
        (p.0 * self.scale, p.1 * self.scale)
    }

    fn flash(&mut self, at: (i64, i64)) {
        let Some(code) = self.current else {
            self.warn("flash without a selected aperture");
            return;
        };
        let Some(ap) = self.apertures.get(&code).cloned() else {
            self.warn(format!("flash with undefined aperture D{code}"));
            return;
        };
        let o = (at.0 as f64, at.1 as f64);
        match ap {
            Aperture::Block(shapes) => {
                if self.mirror != (false, false) || self.rotation != 0.0 || self.scale != 1.0 {
                    self.warn("aperture transforms on block apertures are ignored");
                }
                for s in shapes {
                    let t = s.translated(at.0, at.1);
                    self.emit(if self.dark { t } else { t.inverted() });
                }
            }
            Aperture::Std { prims, .. } => {
                for p in prims {
                    // Clear parts of an aperture only make sense on dark flashes.
                    let (pd, keep) = match &p {
                        Prim::Circle { dark, .. } | Prim::Poly { dark, .. } => (*dark == self.dark, *dark || self.dark),
                    };
                    if !keep {
                        continue;
                    }
                    match p {
                        Prim::Circle { c, r, .. } => {
                            let c = self.transform(c);
                            let r = r * self.scale;
                            if r > 0.0 {
                                self.emit(GerberShape::Circle { center: to_point((c.0 + o.0, c.1 + o.1)), radius: clamp_nm(r).round() as Coord, dark: pd });
                            }
                        }
                        Prim::Poly { pts, .. } => {
                            let pts: Vec<Point> = pts
                                .into_iter()
                                .map(|q| {
                                    let q = self.transform(q);
                                    to_point((q.0 + o.0, q.1 + o.1))
                                })
                                .collect();
                            if pts.len() >= 3 {
                                self.emit(GerberShape::Polygon { points: pts, dark: pd });
                            }
                        }
                    }
                }
            }
        }
    }
}

/// The number following letter `c` in a parameter string like `X3Y2I5.0J4.0`.
fn field_after(s: &str, c: char) -> Option<String> {
    let i = s.find(c)?;
    let v: String = s[i + 1..].chars().take_while(|ch| ch.is_ascii_digit() || matches!(ch, '.' | '-' | '+')).collect();
    (!v.is_empty()).then_some(v)
}

// ---------------------------------------------------------------------------
// Folder / zip import
// ---------------------------------------------------------------------------

const GERBER_EXT: &[&str] = &[
    "gbr", "ger", "gtl", "gbl", "gts", "gbs", "gto", "gbo", "gtp", "gbp", "gko", "gm", "gml", "gpt", "gpb", "gtd", "gbd", "gta", "gba", "art",
    "pho", "gm1", "gm2", "gm3", "gm4", "gm13", "gm15", "gdd", "gdl", "gp1", "gp2", "gl1", "gl2", "gl3", "gl4",
];
const DRILL_EXT: &[&str] = &["drl", "xln", "exc", "drd", "tap", "nc", "ncd", "txt"];
const MAX_FILE: u64 = 256 * 1024 * 1024;

enum Kind {
    Gerber,
    Drill,
}

fn sniff(name: &str, text: &str) -> Option<Kind> {
    let lower = name.to_ascii_lowercase();
    let ext = lower.rsplit_once('.').map_or("", |(_, e)| e);
    let head: String = text.chars().take(4096).collect();
    let looks_gerber = head.contains("%FS") || head.contains("%MO") || head.contains("%ADD") || head.contains("G04 ");
    let looks_drill = head.trim_start().starts_with("M48")
        || head.lines().any(|l| matches!(l.trim(), "M48" | "METRIC" | "INCH") || l.trim().starts_with("METRIC,") || l.trim().starts_with("INCH,"));
    if ext == "gbrjob" || ext == "zip" {
        return None;
    }
    let is_gerber_ext = GERBER_EXT.contains(&ext) || (ext.len() >= 2 && ext.starts_with('g') && ext[1..].bytes().all(|c| c.is_ascii_digit()));
    if looks_drill && !looks_gerber {
        return Some(Kind::Drill);
    }
    if looks_gerber {
        return Some(Kind::Gerber);
    }
    if DRILL_EXT.contains(&ext) && ext != "txt" {
        return Some(Kind::Drill);
    }
    if is_gerber_ext && text.contains('*') {
        return Some(Kind::Gerber);
    }
    None
}

fn parse_any(name: &str, bytes: &[u8]) -> Option<GerberFile> {
    let text = String::from_utf8_lossy(bytes);
    match sniff(name, &text)? {
        Kind::Gerber => parse_gerber(name, &text).ok(),
        Kind::Drill => parse_excellon(name, &text).ok(),
    }
}

fn collect_dir(dir: &Path, depth: usize, out: &mut Vec<GerberFile>) -> Result<(), OutputError> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)?.filter_map(|e| e.ok()).map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            if depth < 4 {
                collect_dir(&p, depth + 1, out)?;
            }
            continue;
        }
        let Ok(meta) = std::fs::metadata(&p) else { continue };
        if meta.len() > MAX_FILE {
            continue;
        }
        let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if name.to_ascii_lowercase().ends_with(".zip") {
            if let Ok(mut inner) = read_zip(&p) {
                out.append(&mut inner);
            }
            continue;
        }
        if let Ok(bytes) = std::fs::read(&p)
            && let Some(f) = parse_any(&name, &bytes)
        {
            out.push(f);
        }
    }
    Ok(())
}

fn read_zip(path: &Path) -> Result<Vec<GerberFile>, OutputError> {
    let file = std::fs::File::open(path)?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| OutputError::Invalid(format!("{}: {e}", path.display())))?;
    let mut out = Vec::new();
    for i in 0..zip.len() {
        let Ok(mut entry) = zip.by_index(i) else { continue };
        if entry.is_dir() || entry.size() > MAX_FILE {
            continue;
        }
        let name = match entry.name() {
            Ok(n) => n.rsplit(['/', '\\']).next().unwrap_or("").to_string(),
            Err(_) => continue,
        };
        let mut bytes = Vec::new();
        if entry.by_ref().take(MAX_FILE).read_to_end(&mut bytes).is_err() {
            continue;
        }
        if let Some(f) = parse_any(&name, &bytes) {
            out.push(f);
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

/// Every Gerber and drill file in a folder (recursively) or a `.zip`,
/// identified by extension and content. Unreadable or unrecognized files are
/// skipped. A single Gerber/drill file path is accepted too.
pub fn read_dir_or_zip(path: &Path) -> Result<Vec<GerberFile>, OutputError> {
    if path.is_dir() {
        let mut out = Vec::new();
        collect_dir(path, 0, &mut out)?;
        return Ok(out);
    }
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    if name.to_ascii_lowercase().ends_with(".zip") {
        return read_zip(path);
    }
    let bytes = std::fs::read(path)?;
    parse_any(&name, &bytes).map(|f| vec![f]).ok_or_else(|| OutputError::Invalid(format!("{name}: not a Gerber or drill file")))
}

#[cfg(test)]
mod tests {
    use super::*;

    const MM: i64 = 1_000_000;

    fn polys(f: &GerberFile) -> Vec<&Vec<Point>> {
        f.shapes.iter().filter_map(|s| if let GerberShape::Polygon { points, .. } = s { Some(points) } else { None }).collect()
    }

    #[test]
    fn strokes_flashes_and_attributes() {
        let src = "%TF.FileFunction,Copper,L1,Top*%\n%FSLAX46Y46*%\n%MOMM*%\n%LPD*%\n%ADD10C,0.250000*%\n%ADD11R,1.000000X2.000000*%\n\
                   D10*\nX0Y0D02*\nX10000000Y0D01*\nD11*\nX5000000Y5000000D03*\nM02*\n";
        let f = parse_gerber("top.gbr", src).unwrap();
        assert_eq!(f.function.as_deref(), Some("Copper,L1,Top"));
        assert!(f.warnings.is_empty(), "{:?}", f.warnings);
        assert_eq!(f.shapes[0], GerberShape::Stroke { a: Point::new(0, 0), b: Point::new(10 * MM, 0), width: MM / 4 });
        let p = polys(&f);
        assert_eq!(p.len(), 1);
        assert!(p[0].contains(&Point::new(5 * MM + MM / 2, 6 * MM)));
        let (lo, hi) = f.bounds.unwrap();
        assert_eq!(lo, Point::new(-MM / 8, -MM / 8));
        assert_eq!(hi, Point::new(10 * MM + MM / 8, 6 * MM));
    }

    #[test]
    fn inch_units_and_trailing_zero_omission() {
        let src = "%FSTAX24Y24*%%MOIN*%%ADD10C,0.01*%D10*X01Y01D03*M02*";
        let f = parse_gerber("a.gbr", src).unwrap();
        // Trailing zeros omitted: "01" in 2.4 → 01.0000 inch.
        assert_eq!(f.shapes[0], GerberShape::Circle { center: Point::new(25_400_000, 25_400_000), radius: 127_000, dark: true });
    }

    #[test]
    fn arcs_multi_and_single_quadrant() {
        let src = "%FSLAX46Y46*%%MOMM*%%ADD10C,0.1*%D10*G75*X1000000Y0D02*G03X0Y1000000I-1000000J0D01*\
                   G74*G02X1000000Y0I0J1000000D01*X1000000Y0D02*G75*G03X1000000Y0I-1000000J0D01*M02*";
        let f = parse_gerber("a.gbr", src).unwrap();
        let arcs: Vec<_> = f.shapes.iter().filter_map(|s| if let GerberShape::ArcStroke { center, radius, start_angle, sweep, .. } = s { Some((*center, *radius, *start_angle, *sweep)) } else { None }).collect();
        assert_eq!(arcs.len(), 3);
        assert_eq!(arcs[0].0, Point::new(0, 0));
        assert_eq!(arcs[0].1, MM);
        assert!((arcs[0].2 - 0.0).abs() < 1e-9 && (arcs[0].3 - 90.0).abs() < 1e-9);
        // Single quadrant: (0,1) → (1,0) clockwise around the origin.
        assert_eq!(arcs[1].0, Point::new(0, 0));
        assert!((arcs[1].3 + 90.0).abs() < 1e-9);
        // Full circle.
        assert!((arcs[2].3 - 360.0).abs() < 1e-9);
    }

    #[test]
    fn regions_with_multiple_contours_and_clear_polarity() {
        let src = "%FSLAX46Y46*%%MOMM*%G36*X0Y0D02*G01X1000000Y0D01*X1000000Y1000000D01*X0Y1000000D01*X0Y0D01*\
                   X2000000Y0D02*X3000000Y0D01*X3000000Y1000000D01*X2000000Y0D01*G37*%LPC*%G36*X100Y100D02*X200Y100D01*X200Y200D01*G37*M02*";
        let f = parse_gerber("a.gbr", src).unwrap();
        let shapes: Vec<_> = f.shapes.iter().filter_map(|s| if let GerberShape::Polygon { points, dark } = s { Some((points.len(), *dark)) } else { None }).collect();
        assert_eq!(shapes, vec![(4, true), (3, true), (3, false)]);
    }

    #[test]
    fn macros_with_expressions_and_rotation() {
        let src = "%FSLAX46Y46*%%MOMM*%\n%AMRoundRect*\n0 comment*\n4,1,4,$2,$3,$4,$5,$6,$7,$8,$9,$2,$3,0*\n1,1,$1+$1,$2,$3*\n20,1,$1+$1,$2,$3,$4,$5,0*%\n\
                   %AMTH*7,0,0,2,1.5,0.2,45*%%AMVAR*$3=$1x2*21,1,$3,$2,0,0,0*%\n\
                   %ADD10RoundRect,0.25X-1X-0.5X1X-0.5X1X0.5X-1X0.5X0*%%ADD11TH*%%ADD12VAR,1X0.5*%\n\
                   D10*X0Y0D03*D11*X10000000Y0D03*D12*X0Y10000000D03*M02*";
        let f = parse_gerber("a.gbr", src).unwrap();
        assert!(f.warnings.is_empty(), "{:?}", f.warnings);
        // RoundRect: outline + circle + vector line; thermal: 4 pieces; VAR: one rect 2×0.5.
        assert_eq!(f.shapes.len(), 3 + 4 + 1);
        assert!(matches!(f.shapes[1], GerberShape::Circle { radius: 250_000, .. }));
        if let GerberShape::Polygon { points, .. } = &f.shapes[7] {
            assert!(points.contains(&Point::new(MM, 10 * MM + MM / 4)));
        } else {
            panic!("expected polygon");
        }
    }

    #[test]
    fn step_repeat_and_blocks_expand() {
        let src = "%FSLAX46Y46*%%MOMM*%%ADD10C,1*%%SRX3Y2I10J5*%D10*X0Y0D03*%SR*%\
                   %ABD20*%D10*X1000000Y0D03*X-1000000Y0D03*%AB*%D20*X0Y-20000000D03*X50000000Y0D03*M02*";
        let f = parse_gerber("a.gbr", src).unwrap();
        assert!(f.warnings.is_empty(), "{:?}", f.warnings);
        assert_eq!(f.shapes.len(), 6 + 4);
        assert!(f.shapes.contains(&GerberShape::Circle { center: Point::new(20 * MM, 5 * MM), radius: MM / 2, dark: true }));
        assert!(f.shapes.contains(&GerberShape::Circle { center: Point::new(51 * MM, 0), radius: MM / 2, dark: true }));
    }

    #[test]
    fn rect_aperture_draw_is_a_hull() {
        let src = "%FSLAX46Y46*%%MOMM*%%ADD10R,1X1*%D10*X0Y0D02*X10000000Y10000000D01*M02*";
        let f = parse_gerber("a.gbr", src).unwrap();
        let p = polys(&f);
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].len(), 6);
    }

    #[test]
    fn malformed_input_never_panics() {
        let inputs = [
            "%FS*%%MO*%%AD*%%ADD*%%ADDx*%%AMX*1,1*%%ADD10X,1X2X3*%D10*D03*X99999999999999999999999Y-Z*M02*",
            "%FSLAX66Y66*%%AMB*4,1,99999999,1,2*5,1,1000000,0,0,1,0*7*$1=(((*$2=1/0*%%ADD10B*%D10*X0Y0D03*",
            "%SRX1000000Y1000000I1J1*%%ADD10C,1*%D10*X0Y0D03*%SR*%M02*",
            "G36*G03X1Y1I1J1D01*G37*G37*%LPC*%%LPX*%%AB*%%ABD*%%ABD10*%%ABD11*%D11*X0Y0D03*",
            "*****%%%%G04 %stray*X1Y1D01*D9*G02*X0Y0D01*",
        ];
        for src in inputs {
            let _ = parse_gerber("bad.gbr", src);
        }
        assert!(parse_gerber("x", "hello").is_err());
    }

    #[test]
    fn json_uses_camel_case() {
        let s = GerberShape::ArcStroke { center: Point::new(0, 0), radius: 1, start_angle: 0.0, sweep: 90.0, width: 1 };
        let j = serde_json::to_string(&s).unwrap();
        assert!(j.starts_with("{\"arcStroke\":") && j.contains("\"startAngle\""), "{j}");
    }

    #[test]
    fn real_structured_files_if_present() {
        let dir = Path::new(r"C:\Program Files\structured format\10.0\share\structured\demos\tiny_tapeout\pcba\gerber\v0p9p4");
        if !dir.is_dir() {
            return;
        }
        let files = read_dir_or_zip(dir).unwrap();
        assert!(files.len() >= 14, "{}", files.len());
        for f in &files {
            assert!(f.warnings.is_empty(), "{}: {:?}", f.name, f.warnings);
            if f.name.ends_with("NPTH.drl") {
                continue;
            }
            assert!(!f.shapes.is_empty(), "{}", f.name);
            assert!(f.function.is_some(), "{}", f.name);
            let (lo, hi) = f.bounds.unwrap();
            assert!(hi.x - lo.x < 300 * MM && hi.x > lo.x, "{}", f.name);
        }
        let cu = files.iter().find(|f| f.name.ends_with("F_Cu.gbr")).unwrap();
        assert_eq!(cu.function.as_deref(), Some("Copper,L1,Top"));
        let pth = files.iter().find(|f| f.name.ends_with("-PTH.drl")).unwrap();
        assert!(pth.shapes.iter().any(|s| matches!(s, GerberShape::Circle { radius: 150_000, .. })));
        let zip = Path::new(r"C:\Program Files\structured format\10.0\share\structured\demos\royalblue54L_feather\nfc_antenna\RoyalBlue54L-NFC-Antenna\jlcpcb\production_files\GERBER-RoyalBlue54L-NFC-Antenna.zip");
        if zip.is_file() {
            let z = read_dir_or_zip(zip).unwrap();
            assert!(z.len() >= 5, "{}", z.len());
            assert!(z.iter().filter(|f| f.name.contains("Cu") || f.name.contains("Edge") || f.name.ends_with("-PTH.drl")).all(|f| !f.shapes.is_empty()));
        }
    }
}
