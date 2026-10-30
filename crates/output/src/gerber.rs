//! Gerber X2 writer (Gerber Layer Format Specification 2023+).
//!
//! Conventions:
//! - Units mm, format `%FSLAX4<d>Y4<d>*%` with `d` = [`GerberOptions::precision`]
//!   decimals (3..=6, default 6 = exact nanometers). Coordinates are rounded
//!   half away from zero. Y is up, as in the board model; bottom layers are
//!   written as seen from the top (never mirrored).
//! - Dark polarity only (`%LPD*%`). Zone fills with holes are written as one
//!   G36/G37 contour per polygon using cut-in fracturing (zero-width bridges),
//!   so no clear polarity can ever erase other copper.
//! - Solder mask files are `%TF.FilePolarity,Negative*%`: the image is the mask
//!   openings (as the spec recommends); every other layer is Positive.
//! - Pads are flashed: C / R / O standard apertures (quarter-turn rotations by
//!   swapping width and height), aperture macros `RotRect` (primitive 21),
//!   `RotOval` (primitives 1 + 20) and `RoundRect` (primitives 4 + 1 + 20) for
//!   other rotations and rounded rectangles.
//! - Copper objects carry `TO.N` (net), pads also `TO.P` / `TO.C`.
//! - Arcs are written in multi-quadrant mode (`G75`) with G02/G03 and I/J.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::time::{SystemTime, UNIX_EPOCH};

use pcbplannercad_core::document::{Document, LayerId};
use pcbplannercad_core::font::{self, TextStyle};
use pcbplannercad_core::geom::{normalize_deg, rotate};
use pcbplannercad_core::item::{Arc, DESIGNATOR_TOKEN, FillStyle, Item, PadKind, PadShape, VALUE_TOKEN};
use pcbplannercad_core::layer;
use pcbplannercad_core::pcb::BoardSettings;
use pcbplannercad_core::pcb::geometry::{all_pads, board_settings, signed_area, world_graphics};
use pcbplannercad_core::pcb::query::{Obj, ObjKind, RuleSet};
use pcbplannercad_core::units::{Coord, Point, mm};
use serde::{Deserialize, Serialize};

use crate::OutputError;

/// "major.minor" of this crate, written into file attributes.
pub(crate) const VERSION: &str = concat!(env!("CARGO_PKG_VERSION_MAJOR"), ".", env!("CARGO_PKG_VERSION_MINOR"));

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GerberOptions {
    /// Cover vias with solder mask (no mask openings for vias).
    pub tent_vias: bool,
    /// Plot footprint designator texts on silkscreen.
    pub include_ref_des: bool,
    /// Plot footprint value texts on silkscreen.
    pub include_values: bool,
    /// Add the board profile to every layer file.
    pub plot_outline_on_all: bool,
    /// Decimals in mm (clamped to 3..=6).
    pub precision: u8,
    pub project_name: String,
}

impl Default for GerberOptions {
    fn default() -> Self {
        GerberOptions { tent_vias: true, include_ref_des: true, include_values: false, plot_outline_on_all: false, precision: 6, project_name: String::new() }
    }
}

/// What a Gerber file images.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LayerFunction {
    Copper(LayerId),
    SolderMask { bottom: bool },
    Paste { bottom: bool },
    Silk { bottom: bool },
    Outline,
    Assembly { bottom: bool },
    Courtyard { bottom: bool },
}

impl LayerFunction {
    /// The drawing layer whose items this file plots.
    pub fn layer(self) -> LayerId {
        let side = |b: bool, top: LayerId, bot: LayerId| if b { bot } else { top };
        match self {
            LayerFunction::Copper(l) => l,
            LayerFunction::SolderMask { bottom } => side(bottom, layer::TOP_MASK, layer::BOTTOM_MASK),
            LayerFunction::Paste { bottom } => side(bottom, layer::TOP_PASTE, layer::BOTTOM_PASTE),
            LayerFunction::Silk { bottom } => side(bottom, layer::TOP_SILK, layer::BOTTOM_SILK),
            LayerFunction::Outline => layer::BOARD_OUTLINE,
            LayerFunction::Assembly { bottom } => side(bottom, layer::TOP_ASSEMBLY, layer::BOTTOM_ASSEMBLY),
            LayerFunction::Courtyard { bottom } => side(bottom, layer::TOP_COURTYARD, layer::BOTTOM_COURTYARD),
        }
    }

    /// Human-readable name ("Top Layer", "Bottom Solder Mask", …).
    pub fn label(self, board: &BoardSettings) -> String {
        let side = |b: bool| if b { "Bottom" } else { "Top" };
        match self {
            LayerFunction::Copper(l) => board.stackup.layer_name(l).map_or_else(|| format!("Copper {}", l.0), str::to_string),
            LayerFunction::SolderMask { bottom } => format!("{} Solder Mask", side(bottom)),
            LayerFunction::Paste { bottom } => format!("{} Paste", side(bottom)),
            LayerFunction::Silk { bottom } => format!("{} Silkscreen", side(bottom)),
            LayerFunction::Outline => "Board Outline".into(),
            LayerFunction::Assembly { bottom } => format!("{} Assembly", side(bottom)),
            LayerFunction::Courtyard { bottom } => format!("{} Courtyard", side(bottom)),
        }
    }
}

/// `(%TF.FileFunction value, polarity)` of a layer file.
pub fn file_function(board: &BoardSettings, f: LayerFunction) -> Result<(String, &'static str), OutputError> {
    let side = |b: bool| if b { "Bot" } else { "Top" };
    Ok(match f {
        LayerFunction::Copper(l) => {
            let cu = board.stackup.copper_layers();
            let i = cu.iter().position(|&c| c == l).ok_or_else(|| OutputError::Invalid(format!("layer {} is not a copper layer of the stack", l.0)))?;
            let pos = if i == 0 {
                "Top"
            } else if i + 1 == cu.len() {
                "Bot"
            } else {
                "Inr"
            };
            (format!("Copper,L{},{pos}", i + 1), "Positive")
        }
        LayerFunction::SolderMask { bottom } => (format!("Soldermask,{}", side(bottom)), "Negative"),
        LayerFunction::Paste { bottom } => (format!("Paste,{}", side(bottom)), "Positive"),
        LayerFunction::Silk { bottom } => (format!("Legend,{}", side(bottom)), "Positive"),
        LayerFunction::Outline => ("Profile,NP".into(), "Positive"),
        LayerFunction::Assembly { bottom } => (format!("AssemblyDrawing,{}", side(bottom)), "Positive"),
        LayerFunction::Courtyard { bottom } => (format!("Other,Courtyard_{}", side(bottom)), "Positive"),
    })
}

/// The manufacturing layer set of a board with conventional fabrication extensions
/// (`.GTL`, `.G1`…, `.GBL`, `.GTS`, `.GBS`, `.GTP`, `.GBP`, `.GTO`, `.GBO`, `.GKO`).
/// The string is the extension including the dot; prefix it with the project
/// name to get a file name (see [`layer_file_name`]).
pub fn standard_layers(doc: &Document) -> Vec<(LayerFunction, String)> {
    let default = BoardSettings::default();
    let board = board_settings(doc).map_or(&default, |(_, b)| b);
    let cu = board.stackup.copper_layers();
    let mut out = Vec::new();
    for (i, &l) in cu.iter().enumerate() {
        let ext = if i == 0 {
            ".GTL".to_string()
        } else if i + 1 == cu.len() {
            ".GBL".to_string()
        } else {
            format!(".G{i}")
        };
        out.push((LayerFunction::Copper(l), ext));
    }
    out.extend([
        (LayerFunction::SolderMask { bottom: false }, ".GTS".to_string()),
        (LayerFunction::SolderMask { bottom: true }, ".GBS".to_string()),
        (LayerFunction::Paste { bottom: false }, ".GTP".to_string()),
        (LayerFunction::Paste { bottom: true }, ".GBP".to_string()),
        (LayerFunction::Silk { bottom: false }, ".GTO".to_string()),
        (LayerFunction::Silk { bottom: true }, ".GBO".to_string()),
        (LayerFunction::Outline, ".GKO".to_string()),
    ]);
    out
}

/// `<project><ext>`, with characters that are invalid in file names replaced.
pub fn layer_file_name(project: &str, ext: &str) -> String {
    format!("{}{ext}", sanitize_file_stem(project))
}

pub(crate) fn sanitize_file_stem(name: &str) -> String {
    let s: String = name.trim().chars().map(|c| if c.is_control() || r#"<>:"/\|?*"#.contains(c) { '_' } else { c }).collect();
    if s.is_empty() { "board".into() } else { s }
}

/// Writes one Gerber X2 layer file.
pub fn write_layer(doc: &Document, f: LayerFunction, opts: &GerberOptions) -> Result<String, OutputError> {
    let default = BoardSettings::default();
    let board = board_settings(doc).map_or(&default, |(_, b)| b);
    let (function, polarity) = file_function(board, f)?;
    let mut w = Writer::new(opts.precision);
    let ctx = Ctx { doc, board, rules: RuleSet::new(board), opts };
    match f {
        LayerFunction::Copper(l) => ctx.copper(&mut w, l),
        LayerFunction::SolderMask { bottom } => ctx.mask(&mut w, bottom),
        LayerFunction::Paste { bottom } => ctx.paste(&mut w, bottom),
        LayerFunction::Silk { .. } => ctx.graphics(&mut w, f.layer(), "Other,Silkscreen", true),
        LayerFunction::Outline => ctx.graphics(&mut w, layer::BOARD_OUTLINE, "Profile", false),
        LayerFunction::Assembly { .. } => ctx.graphics(&mut w, f.layer(), "Other,Assembly", false),
        LayerFunction::Courtyard { .. } => ctx.graphics(&mut w, f.layer(), "Other,Courtyard", false),
    }
    if opts.plot_outline_on_all && f != LayerFunction::Outline {
        w.set_obj(Vec::new());
        ctx.graphics(&mut w, layer::BOARD_OUTLINE, "Profile", false);
    }
    let project = if opts.project_name.trim().is_empty() { "PCBPlanner Studio" } else { opts.project_name.trim() };
    let mut head = String::new();
    let _ = writeln!(head, "%TF.GenerationSoftware,PCBPlanner Studio,PCBPlanner Studio,{VERSION}*%");
    let _ = writeln!(head, "%TF.CreationDate,{}*%", creation_date());
    let _ = writeln!(head, "%TF.ProjectId,{},{},rev1*%", escape(project), project_guid(project));
    let _ = writeln!(head, "%TF.SameCoordinates,Original*%");
    let _ = writeln!(head, "%TF.FileFunction,{function}*%");
    let _ = writeln!(head, "%TF.FilePolarity,{polarity}*%");
    let _ = writeln!(head, "%TF.Part,Single*%");
    let _ = writeln!(head, "G04 {}*", escape_comment(&f.label(board)));
    Ok(w.finish(head))
}

// ---------- layer content ----------

struct Ctx<'a> {
    doc: &'a Document,
    board: &'a BoardSettings,
    rules: RuleSet<'a>,
    opts: &'a GerberOptions,
}

/// Pad geometry to flash.
#[derive(Clone, Copy)]
struct PadGeom {
    pos: Point,
    shape: PadShape,
    width: Coord,
    height: Coord,
    rotation: f64,
    /// Corner radius (RoundRect only).
    radius: Coord,
}

impl<'a> Ctx<'a> {
    fn footprint_key(&self, owner: pcbplannercad_core::ItemId) -> Option<&'a str> {
        match self.doc.get(owner) {
            Some(Item::Footprint(fp)) => Some(fp.footprint.as_str()),
            _ => None,
        }
    }

    fn pads(&self) -> Vec<pcbplannercad_core::pcb::geometry::WorldPad> {
        all_pads(self.doc, self.board)
    }

    fn copper(&self, w: &mut Writer, l: LayerId) {
        for (_, item) in self.doc.items() {
            if let Item::Zone(z) = item
                && z.layer == l
                && !z.keepout
            {
                for poly in &z.fill {
                    w.set_obj(vec![format!("N,{}", escape(&z.net))]);
                    w.region(&fracture(&poly.outer, &poly.holes), "Conductor");
                }
            }
        }
        for (_, item) in self.doc.items() {
            match item {
                Item::Track(t) if t.layer == l => {
                    w.set_obj(vec![format!("N,{}", escape(&t.net))]);
                    w.stroke(&[t.a, t.b], false, t.width, "Conductor");
                }
                Item::ArcTrack(a) if a.layer == l => {
                    w.set_obj(vec![format!("N,{}", escape(&a.net))]);
                    w.stroke_arc(a.center, a.radius, a.start_angle, a.sweep, a.width, "Conductor");
                }
                _ => {}
            }
        }
        for (_, item) in self.doc.items() {
            if let Item::Via(v) = item
                && self.board.stackup.span(v.from_layer, v.to_layer).contains(&l)
            {
                w.set_obj(vec![format!("N,{}", escape(&v.net))]);
                let ap = w.circle(v.diameter, "ViaPad");
                w.flash(ap, v.pos);
            }
        }
        for physical in self.pads() {
            let p=physical.for_layer(l);
            if !p.on_layer(l) {
                continue;
            }
            w.set_obj(vec![
                format!("P,{},{}", escape(&p.designator), escape(&p.number)),
                format!("N,{}", escape(&p.net)),
                format!("C,{}", escape(&p.designator)),
            ]);
            let func = if p.kind == PadKind::Smd { "SMDPad,CuDef" } else { "ComponentPad" };
            w.flash_pad(&pad_geom(&p, 0), func);
        }
        w.set_obj(Vec::new());
        self.graphics(w, l, "NonConductor", false);
    }

    fn mask(&self, w: &mut Writer, bottom: bool) {
        let copper = if bottom { layer::BOTTOM_COPPER } else { layer::TOP_COPPER };
        for physical in self.pads() {
            let p=physical.for_layer(copper);
            let opens = match p.kind {
                PadKind::Smd => p.bottom == bottom && p.on_layer(copper),
                PadKind::Tht | PadKind::Npth => true,
            };
            if !opens {
                continue;
            }
            let obj = Obj { kind: ObjKind::Pad, net: &p.net, layer: Some(copper), designator: Some(&p.designator), footprint: self.footprint_key(p.owner), smd: p.kind == PadKind::Smd };
            let e = self.rules.mask_expansion(&obj);
            let func = if p.kind == PadKind::Smd { "SMDPad,CuDef" } else { "ComponentPad" };
            w.flash_pad(&pad_geom(&p, e), func);
        }
        if !self.opts.tent_vias {
            for (_, item) in self.doc.items() {
                if let Item::Via(v) = item
                    && self.board.stackup.span(v.from_layer, v.to_layer).contains(&copper)
                {
                    let e = self.rules.mask_expansion(&Obj::new(ObjKind::Via, &v.net, Some(copper)));
                    let d = v.diameter + 2 * e;
                    if d > 0 {
                        let ap = w.circle(d, "ViaPad");
                        w.flash(ap, v.pos);
                    }
                }
            }
        }
        let l = if bottom { layer::BOTTOM_MASK } else { layer::TOP_MASK };
        self.graphics(w, l, "Other,SolderMask", false);
    }

    fn paste(&self, w: &mut Writer, bottom: bool) {
        let copper = if bottom { layer::BOTTOM_COPPER } else { layer::TOP_COPPER };
        for physical in self.pads() {
            let p=physical.for_layer(copper);
            if p.kind != PadKind::Smd || p.bottom != bottom || !p.on_layer(copper) {
                continue;
            }
            let obj = Obj { kind: ObjKind::Pad, net: &p.net, layer: Some(copper), designator: Some(&p.designator), footprint: self.footprint_key(p.owner), smd: true };
            let e = self.rules.paste_expansion(&obj);
            w.flash_pad(&pad_geom(&p, e), "SMDPad,CuDef");
        }
        let l = if bottom { layer::BOTTOM_PASTE } else { layer::TOP_PASTE };
        self.graphics(w, l, "Other,Paste", false);
    }

    /// Board graphics and footprint graphics on drawing layer `l`. With
    /// `filter_texts`, footprint designator / value texts follow the options.
    fn graphics(&self, w: &mut Writer, l: LayerId, func: &str, filter_texts: bool) {
        for (_, item) in self.doc.items() {
            emit_item(w, item, l, func);
        }
        for (_, item) in self.doc.items() {
            let Item::Footprint(fp) = item else { continue };
            let Some(footprint) = self.doc.cached_footprint(&fp.footprint) else { continue };
            let originals = footprint.items.iter().filter(|i| !matches!(i, Item::Pad(_)));
            for (orig, it) in originals.zip(world_graphics(fp, footprint)) {
                if filter_texts && let Item::Text(t) = orig {
                    if t.text == DESIGNATOR_TOKEN && !self.opts.include_ref_des {
                        continue;
                    }
                    if t.text == VALUE_TOKEN && !self.opts.include_values {
                        continue;
                    }
                }
                emit_item(w, &it, l, func);
            }
        }
    }
}

fn pad_geom(p: &pcbplannercad_core::pcb::geometry::WorldPad, grow: Coord) -> PadGeom {
    let radius = if p.shape == PadShape::RoundRect { ((p.width.min(p.height) as f64 * p.corner_ratio).round() as Coord + grow).max(0) } else { 0 };
    PadGeom { pos: p.pos, shape: p.shape, width: p.width + 2 * grow, height: p.height + 2 * grow, rotation: p.rotation, radius }
}

fn stroke_width(w: Coord) -> Coord {
    if w > 0 { w } else { mm(0.1) }
}

fn emit_item(w: &mut Writer, item: &Item, l: LayerId, func: &str) {
    match item {
        Item::Line(x) if x.layer == l => w.stroke(&[x.a, x.b], false, stroke_width(x.width), func),
        Item::Arc(x) if x.layer == l => w.stroke_arc(x.center, x.radius, x.start_angle, x.sweep, stroke_width(x.width), func),
        Item::Circle(x) if x.layer == l => {
            if x.fill != FillStyle::None {
                let ap = w.circle(2 * x.radius + x.width.max(0), func);
                w.flash(ap, x.center);
            } else {
                w.stroke_arc(x.center, x.radius, 0.0, 360.0, stroke_width(x.width), func);
            }
        }
        Item::Rect(x) if x.layer == l => {
            let pts = [x.a, Point::new(x.b.x, x.a.y), x.b, Point::new(x.a.x, x.b.y)];
            if x.fill != FillStyle::None {
                w.region(&pts, func);
            }
            if x.fill == FillStyle::None || x.width > 0 {
                w.stroke(&pts, true, stroke_width(x.width), func);
            }
        }
        Item::Polyline(x) if x.layer == l && !x.points.is_empty() => {
            let filled = x.fill != FillStyle::None && x.closed && x.points.len() >= 3;
            if filled {
                w.region(&x.points, func);
            }
            if !filled || x.width > 0 {
                w.stroke(&x.points, x.closed, stroke_width(x.width), func);
            }
        }
        Item::Dimension(d) if d.layer == l => {
            for prim in d.primitives() {
                emit_item(w, &prim, l, func);
            }
        }
        Item::Text(t) if t.layer == l && !t.text.is_empty() && t.height > 0 => {
            let segs = font::text_segments(&t.text, t.pos, TextStyle::of(t));
            if segs.is_empty() {
                return;
            }
            let ap = w.circle(font::stroke_width(t.height).max(1), func);
            w.select(ap);
            for (a, b) in segs {
                w.move_to(a);
                w.line_to(b);
            }
        }
        _ => {}
    }
}

// ---------- cut-in fracturing ----------

/// Merges `holes` into `outer` with zero-width bridges, producing one contour
/// (outer counter-clockwise, holes clockwise) that fills exactly the polygon
/// with holes. Holes are bridged in order of their leftmost vertex, each to
/// the nearest contour edge hit by a ray going left (−X) from that vertex.
pub(crate) fn fracture(outer: &[Point], holes: &[Vec<Point>]) -> Vec<Point> {
    let mut contour = dedup(outer.to_vec());
    if contour.len() < 3 {
        return contour;
    }
    if signed_area(&contour) < 0.0 {
        contour.reverse();
    }
    let mut hs: Vec<Vec<Point>> = holes
        .iter()
        .map(|h| dedup(h.clone()))
        .filter(|h| h.len() >= 3)
        .map(|mut h| {
            if signed_area(&h) > 0.0 {
                h.reverse();
            }
            h
        })
        .collect();
    hs.sort_by_key(|h| h.iter().map(|p| (p.x, p.y)).min());
    for h in hs {
        let (k, p0) = h.iter().copied().enumerate().min_by_key(|(_, p)| (p.x, p.y)).expect("hole has points");
        let n = contour.len();
        let mut best: Option<(usize, Coord)> = None;
        for i in 0..n {
            let (a, b) = (contour[i], contour[(i + 1) % n]);
            if (a.y <= p0.y && p0.y < b.y) || (b.y <= p0.y && p0.y < a.y) {
                let num = (p0.y - a.y) as i128 * (b.x - a.x) as i128;
                let den = (b.y - a.y) as i128;
                let x = a.x + div_round_i128(num, den) as Coord;
                if x <= p0.x && best.is_none_or(|(_, bx)| x > bx) {
                    best = Some((i, x));
                }
            }
        }
        let Some((i, x)) = best else { continue }; // hole outside the outline: nothing to cut
        let bridge = Point::new(x, p0.y);
        let mut next = Vec::with_capacity(n + h.len() + 3);
        next.extend_from_slice(&contour[..=i]);
        next.push(bridge);
        next.extend_from_slice(&h[k..]);
        next.extend_from_slice(&h[..k]);
        next.push(p0);
        next.push(bridge);
        next.extend_from_slice(&contour[i + 1..]);
        contour = dedup(next);
    }
    contour
}

fn dedup(mut pts: Vec<Point>) -> Vec<Point> {
    pts.dedup();
    while pts.len() > 1 && pts.first() == pts.last() {
        pts.pop();
    }
    pts
}

fn div_round_i128(a: i128, b: i128) -> i128 {
    let (a, b) = if b < 0 { (-a, -b) } else { (a, b) };
    let q = a / b;
    let r = a % b;
    if 2 * r.abs() >= b { q + a.signum() } else { q }
}

/// `a / b` rounded half away from zero (`b > 0`).
pub(crate) fn div_round(a: i64, b: i64) -> i64 {
    div_round_i128(a as i128, b as i128) as i64
}

/// Fixed-point decimal of integer `u` with `dec` decimals ("-1.250000").
pub(crate) fn fmt_fixed(u: i64, dec: u32) -> String {
    let sign = if u < 0 { "-" } else { "" };
    let a = u.unsigned_abs();
    if dec == 0 {
        return format!("{sign}{a}");
    }
    let p = 10u64.pow(dec);
    format!("{sign}{}.{:0w$}", a / p, a % p, w = dec as usize)
}

// ---------- low-level writer ----------

const MACRO_ROT_RECT: &str = "%AMRotRect*\n0 Rotated rectangle. $1 width $2 height $3 rotation in degrees CCW*\n21,1,$1,$2,0,0,$3*%";
const MACRO_ROT_OVAL: &str = "%AMRotOval*\n0 Obround. $1 diameter. $2 $3 and $4 $5 end centers*\n1,1,$1,$2,$3*\n1,1,$1,$4,$5*\n20,1,$1,$2,$3,$4,$5,0*%";
const MACRO_ROUND_RECT: &str = "%AMRoundRect*\n0 Rounded rectangle. $1 corner radius. $2 to $9 corner circle centers*\n4,1,4,$2,$3,$4,$5,$6,$7,$8,$9,$2,$3,0*\n1,1,$1+$1,$2,$3*\n1,1,$1+$1,$4,$5*\n1,1,$1+$1,$6,$7*\n1,1,$1+$1,$8,$9*\n20,1,$1+$1,$2,$3,$4,$5,0*\n20,1,$1+$1,$4,$5,$6,$7,0*\n20,1,$1+$1,$6,$7,$8,$9,0*\n20,1,$1+$1,$8,$9,$2,$3,0*%";

struct Writer {
    dec: u32,
    /// Nanometers per file unit.
    scale: i64,
    /// (template, aperture function) in D-code order starting at D10.
    apertures: Vec<(String, String)>,
    index: HashMap<(String, String), usize>,
    macros: [bool; 3],
    body: String,
    cur_ap: Option<usize>,
    interp: u8,
    at: Option<(i64, i64)>,
    obj: Vec<String>,
}

impl Writer {
    fn new(precision: u8) -> Writer {
        let dec = (precision as u32).clamp(3, 6);
        Writer {
            dec,
            scale: 10i64.pow(6 - dec),
            apertures: Vec::new(),
            index: HashMap::new(),
            macros: [false; 3],
            body: String::new(),
            cur_ap: None,
            interp: 1,
            at: None,
            obj: Vec::new(),
        }
    }

    fn units(&self, nm: Coord) -> i64 {
        div_round(nm, self.scale)
    }

    fn num(&self, nm: Coord) -> String {
        fmt_fixed(self.units(nm), self.dec)
    }

    fn xy(&self, p: Point) -> (i64, i64) {
        (self.units(p.x), self.units(p.y))
    }

    fn aperture(&mut self, template: String, func: &str) -> usize {
        let key = (template, func.to_string());
        if let Some(&i) = self.index.get(&key) {
            return i;
        }
        let i = self.apertures.len();
        self.apertures.push(key.clone());
        self.index.insert(key, i);
        i
    }

    fn circle(&mut self, d: Coord, func: &str) -> usize {
        let t = format!("C,{}", self.num(d.max(0)));
        self.aperture(t, func)
    }

    fn select(&mut self, ap: usize) {
        if self.cur_ap != Some(ap) {
            let _ = writeln!(self.body, "D{}*", ap + 10);
            self.cur_ap = Some(ap);
        }
    }

    fn set_interp(&mut self, g: u8) {
        if self.interp != g {
            let _ = writeln!(self.body, "G0{g}*");
            self.interp = g;
        }
    }

    fn set_obj(&mut self, attrs: Vec<String>) {
        if attrs == self.obj {
            return;
        }
        if !self.obj.is_empty() {
            self.body.push_str("%TD*%\n");
        }
        for a in &attrs {
            let _ = writeln!(self.body, "%TO.{a}*%");
        }
        self.obj = attrs;
    }

    fn move_to(&mut self, p: Point) {
        let c = self.xy(p);
        if self.at != Some(c) {
            let _ = writeln!(self.body, "X{}Y{}D02*", c.0, c.1);
            self.at = Some(c);
        }
    }

    fn line_to(&mut self, p: Point) {
        self.set_interp(1);
        let c = self.xy(p);
        let _ = writeln!(self.body, "X{}Y{}D01*", c.0, c.1);
        self.at = Some(c);
    }

    fn flash(&mut self, ap: usize, p: Point) {
        self.select(ap);
        let c = self.xy(p);
        let _ = writeln!(self.body, "X{}Y{}D03*", c.0, c.1);
        self.at = Some(c);
    }

    fn stroke(&mut self, pts: &[Point], closed: bool, width: Coord, func: &str) {
        if pts.is_empty() {
            return;
        }
        let ap = self.circle(width, func);
        self.select(ap);
        self.move_to(pts[0]);
        if pts.len() == 1 {
            self.line_to(pts[0]);
        }
        for &p in &pts[1..] {
            self.line_to(p);
        }
        if closed && pts.len() > 2 {
            self.line_to(pts[0]);
        }
    }

    fn stroke_arc(&mut self, center: Point, radius: Coord, start_angle: f64, sweep: f64, width: Coord, func: &str) {
        if radius <= 0 || sweep == 0.0 {
            return;
        }
        let full = sweep.abs() >= 360.0 - 1e-9;
        let arc = Arc { center, radius, start_angle, sweep, width, layer: LayerId(0) };
        let (start, end) = (arc.start(), if full { arc.start() } else { arc.end() });
        let ap = self.circle(width, func);
        self.select(ap);
        self.move_to(start);
        let (s, e, c) = (self.xy(start), self.xy(end), self.xy(center));
        if s == e && !full {
            // Too short to resolve as an arc: a zero-length G75 arc would be a full circle.
            self.line_to(end);
            return;
        }
        self.set_interp(if sweep > 0.0 { 3 } else { 2 });
        let _ = writeln!(self.body, "X{}Y{}I{}J{}D01*", e.0, e.1, c.0 - s.0, c.1 - s.1);
        self.at = Some(e);
        self.set_interp(1);
    }

    fn region(&mut self, contour: &[Point], func: &str) {
        if contour.len() < 3 {
            return;
        }
        let _ = writeln!(self.body, "%TA.AperFunction,{func}*%");
        self.set_interp(1);
        self.body.push_str("G36*\n");
        let c = self.xy(contour[0]);
        let _ = writeln!(self.body, "X{}Y{}D02*", c.0, c.1);
        self.at = Some(c);
        for &p in &contour[1..] {
            self.line_to(p);
        }
        if self.at != Some(c) {
            self.line_to(contour[0]);
        }
        self.body.push_str("G37*\n%TD.AperFunction*%\n");
    }

    fn flash_pad(&mut self, g: &PadGeom, func: &str) {
        let (w, h) = (g.width, g.height);
        if w <= 0 || h <= 0 {
            return;
        }
        let rot = normalize_deg(g.rotation);
        let quarter = rot % 90.0 == 0.0;
        let swapped = rot == 90.0 || rot == 270.0;
        let (qw, qh) = if swapped { (h, w) } else { (w, h) };
        let mut shape = g.shape;
        let r = g.radius.min(w / 2).min(h / 2);
        if shape == PadShape::RoundRect {
            if r <= 0 {
                shape = PadShape::Rect;
            } else if 2 * r >= w.min(h) {
                shape = PadShape::Oval;
            }
        }
        if shape == PadShape::Oval && w == h {
            shape = PadShape::Circle;
        }
        let template = match shape {
            PadShape::Circle => format!("C,{}", self.num(w)),
            PadShape::Rect if quarter => format!("R,{}X{}", self.num(qw), self.num(qh)),
            PadShape::Oval if quarter => format!("O,{}X{}", self.num(qw), self.num(qh)),
            PadShape::Rect => {
                self.macros[0] = true;
                format!("RotRect,{}X{}X{}", self.num(w), self.num(h), fmt_angle(rot))
            }
            PadShape::Oval => {
                self.macros[1] = true;
                let (long, short) = (w.max(h), w.min(h));
                let along = if w >= h { rot } else { rot + 90.0 };
                let half = (long - short) / 2;
                let a = rotate(Point::new(-half, 0), Point::default(), along);
                let b = rotate(Point::new(half, 0), Point::default(), along);
                format!("RotOval,{}X{}X{}X{}X{}", self.num(short), self.num(a.x), self.num(a.y), self.num(b.x), self.num(b.y))
            }
            PadShape::RoundRect => {
                self.macros[2] = true;
                let (cx, cy) = (w / 2 - r, h / 2 - r);
                let mut t = format!("RoundRect,{}", self.num(r));
                for (sx, sy) in [(1, 1), (-1, 1), (-1, -1), (1, -1)] {
                    let p = rotate(Point::new(sx * cx, sy * cy), Point::default(), rot);
                    let _ = write!(t, "X{}X{}", self.num(p.x), self.num(p.y));
                }
                t
            }
        };
        let ap = self.aperture(template, func);
        self.flash(ap, g.pos);
    }

    fn finish(self, mut head: String) -> String {
        let _ = writeln!(head, "%FSLAX4{0}Y4{0}*%", self.dec);
        let _ = writeln!(head, "G04 Gerber Fmt 4.{}, Leading zero omitted, Abs format (unit mm)*", self.dec);
        head.push_str("%MOMM*%\n%LPD*%\nG01*\nG75*\n");
        for (used, m) in self.macros.iter().zip([MACRO_ROT_RECT, MACRO_ROT_OVAL, MACRO_ROUND_RECT]) {
            if *used {
                head.push_str(m);
                head.push('\n');
            }
        }
        head.push_str("G04 Aperture list*\n");
        let mut cur_fn: Option<&str> = None;
        for (i, (t, f)) in self.apertures.iter().enumerate() {
            if cur_fn != Some(f.as_str()) {
                let _ = writeln!(head, "%TA.AperFunction,{f}*%");
                cur_fn = Some(f);
            }
            let _ = writeln!(head, "%ADD{}{t}*%", i + 10);
        }
        if cur_fn.is_some() {
            head.push_str("%TD*%\n");
        }
        head.push_str("G04 Aperture list end*\n");
        head.push_str(&self.body);
        if !self.obj.is_empty() {
            head.push_str("%TD*%\n");
        }
        head.push_str("M02*\n");
        head
    }
}

fn fmt_angle(deg: f64) -> String {
    let s = format!("{deg:.6}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-0" { "0".into() } else { s.to_string() }
}

/// Escapes an attribute field value (`,` `*` `%` `\` become `\uXXXX`).
pub(crate) fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            ',' | '*' | '%' | '\\' => {
                let _ = write!(out, "\\u{:04X}", c as u32);
            }
            c if c.is_control() => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

fn escape_comment(s: &str) -> String {
    s.chars().map(|c| if c == '*' || c == '%' || c.is_control() { ' ' } else { c }).collect()
}

/// Deterministic RFC 4122-shaped GUID from the project name (FNV-1a based).
fn project_guid(name: &str) -> String {
    let fnv = |seed: u64| name.bytes().fold(seed, |h, b| (h ^ b as u64).wrapping_mul(0x100_0000_01b3));
    let (a, b) = (fnv(0xcbf2_9ce4_8422_2325), fnv(0x6c62_272e_07bb_0142));
    let b = (b & !(0xc000u64 << 48)) | (0x8000u64 << 48); // RFC 4122 variant
    let a = (a & !0xf000) | 0x5000; // version 5 nibble
    format!("{:08x}-{:04x}-{:04x}-{:04x}-{:012x}", a >> 32, (a >> 16) & 0xffff, a & 0xffff, b >> 48, b & 0xffff_ffff_ffff)
}

/// Current UTC time as ISO 8601 (`2026-10-26T12:34:56+00:00`).
pub(crate) fn creation_date() -> String {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
    let (days, sod) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}+00:00", sod / 3600, sod % 3600 / 60, sod % 60)
}

#[cfg(test)]
#[path = "gerber_test_board.rs"]
pub(crate) mod test_board;

#[cfg(test)]
#[path = "gerber_tests.rs"]
mod tests;
