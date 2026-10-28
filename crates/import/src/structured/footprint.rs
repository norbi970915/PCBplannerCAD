//! `.kicad_mod` footprints and `.pretty` footprint folders.

use std::path::Path;

use pcbplannercad_core::LayerId;
use pcbplannercad_core::geom::rotate;
use pcbplannercad_core::item::{
    Circle, DESIGNATOR_TOKEN, Drill, FillStyle, Item, Line, Pad, PadKind, PadShape, Polyline, Rect, Text, VALUE_TOKEN,
};
use pcbplannercad_core::layer;
use pcbplannercad_core::library::Model3d;
use pcbplannercad_core::units::{Coord, Point, mm};
use pcbplannercad_core::{Footprint, Library};

use super::common::{ArcGeom, Notes, YAxis, angle, arc_from_3pts, arc_from_center, bezier_points, effects, fill, poly_item, stroke_width};
use super::sexpr::{List, parse};
use crate::report::{ImportError, ImportReport};

/// How footprint-local structured format coordinates map to the cached (top-side, Y-up)
/// footprint. Library files are plain Y-down; footprints embedded in a board
/// store absolute pad/text angles and, on the bottom side, pre-flipped geometry.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub y: YAxis,
    /// Swap F./B. layers (bottom-side board footprints).
    pub flip: bool,
    pad_sign: f64,
    pad_offset: f64,
    text_offset: f64,
}

impl Frame {
    pub const LIBRARY: Frame = Frame { y: YAxis::Down, flip: false, pad_sign: 1.0, pad_offset: 0.0, text_offset: 0.0 };

    /// Frame for a footprint placed on a board at `rotation` (structured format degrees).
    ///
    /// Top side: local = structured format local with Y negated, angles minus the rotation.
    /// Bottom side: structured format stores the footprint flipped about its local X axis
    /// (Y-down). Reading those coordinates as Y-up undoes the flip and yields the
    /// library (top-side) geometry; our bottom placement (mirror X, then rotate)
    /// reproduces the board with rotation + 180. See [`Frame::placed_rotation`].
    pub fn placed(rotation: f64, bottom: bool) -> Frame {
        if bottom {
            let rho = rotation + 180.0;
            // structured format negates pad angles when flipping; θ − a recovers the library angle
            // (our bottom placement then shows it 180° turned, which is the same copper
            // because every pad shape is point-symmetric).
            Frame { y: YAxis::Up, flip: true, pad_sign: -1.0, pad_offset: rotation, text_offset: -rho }
        } else {
            Frame { y: YAxis::Down, flip: false, pad_sign: 1.0, pad_offset: -rotation, text_offset: -rotation }
        }
    }

    /// The `PlacedFootprint::rotation` matching [`Frame::placed`].
    pub fn placed_rotation(rotation: f64, bottom: bool) -> f64 {
        clean_angle(if bottom { rotation + 180.0 } else { rotation })
    }

    fn layer(&self, l: LayerId) -> LayerId {
        if self.flip { layer::flipped(l) } else { l }
    }

    fn pad_rot(&self, a: f64) -> f64 {
        clean_angle(self.pad_sign * a + self.pad_offset)
    }

    fn text_rot(&self, a: f64) -> f64 {
        clean_angle(a + self.text_offset)
    }
}

/// Normalized angle with float noise removed (so equal footprints compare equal).
pub fn clean_angle(a: f64) -> f64 {
    let r = (angle(a) * 1e6).round() / 1e6;
    if r >= 360.0 { 0.0 } else { r }
}

pub fn parse_footprint(text: &str) -> Result<(Footprint, ImportReport), ImportError> {
    let root = parse(text)?;
    if root.head() != "footprint" && root.head() != "module" {
        return Err(ImportError::Invalid(format!("expected a footprint, found '{}'", root.head())));
    }
    let raw_name = root.atom(1).unwrap_or("");
    let name = raw_name.rsplit_once(':').map_or(raw_name, |(_, n)| n).to_string();
    let mut notes = Notes::default();
    let fp = read_footprint(&root, &name, &Frame::LIBRARY, &mut notes);
    let mut report = ImportReport::default();
    notes.flush(&mut report, &name);
    Ok((fp, report))
}

pub fn import_footprint_dir(dir: &Path, lib_name: &str) -> Result<(Library, ImportReport), ImportError> {
    let mut files: Vec<_> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("kicad_mod")))
        .collect();
    files.sort();
    let mut lib = Library { name: lib_name.to_string(), ..Default::default() };
    let mut report = ImportReport::default();
    for path in files {
        let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let result = std::fs::read_to_string(&path).map_err(ImportError::from).and_then(|t| parse_footprint(&t));
        match result {
            Ok((mut fp, r)) => {
                // structured format identifies footprints by file name.
                if !stem.is_empty() {
                    fp.name = stem;
                }
                report.extend(r);
                lib.footprints.push(fp);
            }
            Err(e) => report.warn(format!("{lib_name}:{stem}"), format!("footprint skipped: {e}")),
        }
    }
    Ok((lib, report))
}

pub fn read_footprint(root: &List, name: &str, f: &Frame, notes: &mut Notes) -> Footprint {
    let mut fp = Footprint::new(name);
    fp.description = root.child_str("descr").unwrap_or("").to_string();
    let mut models = 0;
    for l in root.lists() {
        match l.head() {
            "pad" => fp.items.extend(read_pad(l, f, notes)),
            "fp_line" | "line" | "fp_rect" | "rect" | "fp_circle" | "circle" | "fp_arc" | "arc" | "fp_poly" | "poly" | "fp_curve"
            | "bezier" => {
                if let Some(layer) = map_layer(l.child_str("layer").unwrap_or(""), notes) {
                    fp.items.extend(read_graphic(l, f.layer(layer), f.y, notes));
                }
            }
            "fp_text" | "property" | "text" => {
                if let Some(t) = read_text(l, f, notes) {
                    fp.items.push(t);
                }
            }
            "model" => {
                models += 1;
                if models == 1 {
                    fp.model = read_model(l);
                } else {
                    notes.info("additional 3D models ignored (first one kept)", "");
                }
            }
            "zone" => notes.info("footprint zones (keep-outs, copper fills) skipped", ""),
            "fp_text_box" | "textbox" => notes.info("text boxes skipped", ""),
            "dimension" => notes.info("dimensions skipped", ""),
            "image" => notes.info("images skipped", ""),
            "table" => notes.info("tables skipped", ""),
            _ => {}
        }
    }
    fp
}

/// structured format layer name → pCAD layer; `None` (with a note) for layers we skip.
pub fn map_layer(name: &str, notes: &mut Notes) -> Option<LayerId> {
    Some(match name {
        "F.Cu" => layer::TOP_COPPER,
        "B.Cu" => layer::BOTTOM_COPPER,
        "F.SilkS" | "F.Silkscreen" => layer::TOP_SILK,
        "B.SilkS" | "B.Silkscreen" => layer::BOTTOM_SILK,
        "F.Mask" => layer::TOP_MASK,
        "B.Mask" => layer::BOTTOM_MASK,
        "F.Paste" => layer::TOP_PASTE,
        "B.Paste" => layer::BOTTOM_PASTE,
        "F.CrtYd" | "F.Courtyard" => layer::TOP_COURTYARD,
        "B.CrtYd" | "B.Courtyard" => layer::BOTTOM_COURTYARD,
        "F.Fab" => layer::TOP_ASSEMBLY,
        "B.Fab" => layer::BOTTOM_ASSEMBLY,
        "Edge.Cuts" => layer::BOARD_OUTLINE,
        n if n.starts_with("User.") || matches!(n, "Cmts.User" | "Dwgs.User" | "Eco1.User" | "Eco2.User") => {
            notes.info(&format!("graphics on {n} mapped to Mechanical 1"), "");
            layer::MECHANICAL_1
        }
        n if n.starts_with("In") && n.ends_with(".Cu") => match n[2..n.len() - 3].parse::<u16>() {
            Ok(k) if (1..=30).contains(&k) => layer::inner_copper(k),
            _ => {
                notes.info(&format!("graphics on unknown layer '{n}' skipped"), "");
                return None;
            }
        },
        n => {
            notes.info(&format!("graphics on layer '{n}' skipped"), "");
            return None;
        }
    })
}

pub fn read_graphic(l: &List, layer: LayerId, y: YAxis, notes: &mut Notes) -> Vec<Item> {
    let width = stroke_width(l);
    let item = match l.head() {
        "fp_line" | "line" | "gr_line" => match (y.child_pt(l, "start"), y.child_pt(l, "end")) {
            (Some(a), Some(b)) if a != b => Some(Item::Line(Line { a, b, width, layer })),
            _ => None,
        },
        "fp_rect" | "rect" | "gr_rect" => {
            let fill = fill(l).unwrap_or(FillStyle::None);
            match (y.child_pt(l, "start"), y.child_pt(l, "end")) {
                (Some(a), Some(b)) if a.x != b.x && a.y != b.y => Some(Item::Rect(Rect { a, b, width, layer, fill })),
                _ => None,
            }
        }
        "fp_circle" | "circle" | "gr_circle" => {
            let fill = fill(l).unwrap_or(FillStyle::None);
            match (y.child_pt(l, "center"), y.child_pt(l, "end")) {
                (Some(c), Some(e)) => {
                    let r = (((e.x - c.x) as f64).hypot((e.y - c.y) as f64)).round() as Coord;
                    (r > 0).then_some(Item::Circle(Circle { center: c, radius: r, width, layer, fill }))
                }
                _ => None,
            }
        }
        "fp_arc" | "arc" | "gr_arc" => {
            let arc = match (y.child_pt(l, "start"), y.child_pt(l, "mid"), y.child_pt(l, "end")) {
                (Some(s), Some(m), Some(e)) => arc_from_3pts(s, m, e),
                (Some(c), None, Some(s)) => legacy_arc(c, s, l.child_num("angle").map(|a| if y == YAxis::Down { -a } else { a })),
                _ => None,
            };
            if arc.is_none() {
                notes.info("degenerate arc skipped", "");
            }
            arc.map(|a| a.item(width, layer))
        }
        "fp_poly" | "poly" | "gr_poly" => poly_item(y.pts(l), width, layer, fill(l).unwrap_or(FillStyle::Solid)),
        "fp_curve" | "bezier" | "gr_curve" => poly_item(bezier_points(&y.pts(l)), width, layer, FillStyle::None),
        _ => None,
    };
    item.into_iter().collect()
}

/// structured format 5 / early 6 arcs: `start` is the center, `end` the arc start point and
/// `angle` the sweep, clockwise on screen for positive values (`sweep` is
/// already converted to our counter-clockwise convention).
fn legacy_arc(center: Point, start: Point, sweep: Option<f64>) -> Option<ArcGeom> {
    let r = ((start.x - center.x) as f64).hypot((start.y - center.y) as f64).round() as Coord;
    let a0 = ((start.y - center.y) as f64).atan2((start.x - center.x) as f64).to_degrees();
    arc_from_center(center, r, a0, sweep?)
}

pub fn read_text(l: &List, f: &Frame, notes: &mut Notes) -> Option<Item> {
    let (kind, value) = match l.head() {
        "fp_text" => (l.atom(1)?, l.atom(2)?),
        "property" => (l.atom(1)?, l.atom(2)?),
        _ => ("user", l.atom(1)?),
    };
    let e = effects(l);
    if e.hidden {
        return None;
    }
    let layer = f.layer(map_layer(l.child_str("layer")?, notes)?);
    let at = l.child("at")?;
    let text = match kind {
        "reference" | "Reference" => DESIGNATOR_TOKEN.to_string(),
        "value" | "Value" => VALUE_TOKEN.to_string(),
        _ => match value {
            "${REFERENCE}" | "%R" => DESIGNATOR_TOKEN.to_string(),
            "${VALUE}" | "%V" => VALUE_TOKEN.to_string(),
            v => v.to_string(),
        },
    };
    if text.is_empty() {
        return None;
    }
    Some(Item::Text(Text {
        pos: f.y.pt_of(at)?,
        text,
        height: e.height,
        // structured format angles are counter-clockwise on screen, as ours are; the Y flip
        // keeps the on-screen picture, so the angle carries over unchanged.
        rotation: f.text_rot(at.num(3).unwrap_or(0.0)),
        h_align: e.h_align,
        v_align: e.v_align,
        layer,
        // Bottom-side text is drawn mirrored; the cached footprint is top-side.
        mirrored: e.mirrored != f.flip,
    }))
}

fn read_pad(l: &List, f: &Frame, notes: &mut Notes) -> Vec<Item> {
    let number = l.atom(1).unwrap_or("").to_string();
    let ctx = format!("pad {number}");
    let kind_s = l.atom(2).unwrap_or("smd");
    let shape_s = l.atom(3).unwrap_or("rect");
    let Some(at) = l.child("at") else {
        notes.warn("pad without position skipped", &ctx);
        return Vec::new();
    };
    let Some(pos) = f.y.pt_of(at) else { return Vec::new() };
    let rotation = f.pad_rot(at.num(3).unwrap_or(0.0));
    let size = l.child("size").and_then(|s| s.xy()).map(|(w, h)| (mm(w), mm(h))).unwrap_or((0, 0));

    let drill = l.child("drill").and_then(|d| {
        if d.child("offset").is_some() {
            notes.info("pad drill offsets ignored", &ctx);
        }
        let nums: Vec<f64> = d.items.iter().skip(1).filter_map(|s| s.as_atom()).filter_map(super::sexpr::parse_num).collect();
        let (w, h) = match nums.as_slice() {
            [] => return None,
            [w] => (*w, *w),
            [w, h, ..] => (*w, *h),
        };
        (w > 0.0 && h > 0.0).then(|| Drill { width: mm(w), height: mm(h) })
    });

    let layers: Vec<&str> = l.child("layers").map(|ls| ls.items.iter().skip(1).filter_map(|s| s.as_atom()).collect()).unwrap_or_default();
    let has = |n: &str| layers.contains(&n);
    let any_cu = layers.iter().any(|x| x.ends_with(".Cu"));

    let (shape, corner_ratio) = match shape_s {
        "circle" => (PadShape::Circle, 0.0),
        "rect" => (PadShape::Rect, 0.0),
        "roundrect" => {
            if l.child("chamfer_ratio").is_some_and(|c| c.num(1).unwrap_or(0.0) > 0.0) && l.child("chamfer").is_some() {
                notes.info("chamfered pad corners imported as rounded rectangles", &ctx);
            }
            (PadShape::RoundRect, l.child_num("roundrect_rratio").unwrap_or(0.25).clamp(0.0, 0.5))
        }
        "oval" => (PadShape::Oval, 0.0),
        "trapezoid" | "trapezoidal" => {
            notes.info("trapezoid pads imported as rectangles", &ctx);
            (PadShape::Rect, 0.0)
        }
        "custom" => {
            notes.info("custom pad shapes imported as their anchor shape", &ctx);
            let anchor = l.child("options").and_then(|o| o.child_str("anchor")).unwrap_or("circle");
            (if anchor == "rect" { PadShape::Rect } else { PadShape::Circle }, 0.0)
        }
        other => {
            notes.warn(&format!("unknown pad shape '{other}' imported as rectangle"), &ctx);
            (PadShape::Rect, 0.0)
        }
    };
    if l.child("padstack").is_some() {
        notes.info("per-layer padstacks imported using the outer-layer shape", &ctx);
    }

    let (mut width, mut height) = size;
    if shape == PadShape::Circle {
        height = width;
    }

    let kind = match kind_s {
        "thru_hole" => PadKind::Tht,
        "np_thru_hole" => PadKind::Npth,
        "smd" => PadKind::Smd,
        "connect" => {
            notes.info("connector (aperture-less) pads imported as SMD pads", &ctx);
            PadKind::Smd
        }
        other => {
            notes.warn(&format!("unknown pad type '{other}' imported as SMD"), &ctx);
            PadKind::Smd
        }
    };

    if kind != PadKind::Smd {
        let Some(drill) = drill else {
            notes.warn("hole pad without drill skipped", &ctx);
            return Vec::new();
        };
        if width <= 0 || height <= 0 {
            (width, height) = (drill.width, drill.height);
        }
        return vec![Item::Pad(Pad {
            number,
            pos,
            rotation,
            shape,
            width,
            height,
            corner_ratio,
            kind,
            layer: layer::MULTI_LAYER,
            drill: Some(drill),
        })];
    }

    if width <= 0 || height <= 0 {
        notes.warn("pad with zero size skipped", &ctx);
        return Vec::new();
    }
    let copper = if has("F.Cu") || has("*.Cu") || has("F&B.Cu") {
        Some(layer::TOP_COPPER)
    } else if has("B.Cu") {
        Some(layer::BOTTOM_COPPER)
    } else if any_cu {
        notes.info("SMD pads on inner copper layers placed on top copper", &ctx);
        Some(layer::TOP_COPPER)
    } else {
        None
    };
    match copper {
        Some(layer) => {
            if layer == layer::TOP_COPPER && has("B.Cu") {
                notes.info("SMD pads on both copper sides imported on top copper only", &ctx);
            }
            let layer = f.layer(layer);
            vec![Item::Pad(Pad { number, pos, rotation, shape, width, height, corner_ratio, kind, layer, drill: None })]
        }
        None => {
            // Paste/mask-only apertures: keep them as filled shapes on their layer.
            notes.info("copper-less pads (paste/mask apertures) imported as filled graphics", &ctx);
            layers
                .iter()
                .filter_map(|n| {
                    let mut quiet = Notes::default();
                    map_layer(n, &mut quiet).map(|l| f.layer(l))
                })
                .map(|layer| aperture(pos, rotation, shape, width, height, layer))
                .collect()
        }
    }
}

fn aperture(pos: Point, rotation: f64, shape: PadShape, w: Coord, h: Coord, layer: LayerId) -> Item {
    if shape == PadShape::Circle {
        return Item::Circle(Circle { center: pos, radius: w / 2, width: 0, layer, fill: FillStyle::Solid });
    }
    let (hw, hh) = (w / 2, h / 2);
    let corners = [(-hw, -hh), (hw, -hh), (hw, hh), (-hw, hh)].map(|(x, y)| rotate(pos.translated(x, y), pos, rotation));
    if rotation % 90.0 == 0.0 {
        return Item::Rect(Rect { a: corners[0], b: corners[2], width: 0, layer, fill: FillStyle::Solid });
    }
    Item::Polyline(Polyline { points: corners.to_vec(), closed: true, width: 0, layer, fill: FillStyle::Solid })
}

fn read_model(l: &List) -> Option<Model3d> {
    let path = l.atom(1)?.to_string();
    let xyz = |name: &str| l.child(name).and_then(|n| n.child("xyz")).and_then(|x| Some([x.num(1)?, x.num(2)?, x.num(3)?]));
    // structured format 5 stored the offset as `(at (xyz ..))` in inches.
    let offset = xyz("offset").or_else(|| xyz("at").map(|v| v.map(|c| c * 25.4))).unwrap_or([0.0; 3]);
    Some(Model3d { path, offset, rotation: xyz("rotate").unwrap_or([0.0; 3]), scale: xyz("scale").unwrap_or([1.0; 3]) })
}
