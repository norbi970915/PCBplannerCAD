use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Instant;

use pcbplannercad_core::geom::{Placement, on_segment};
use pcbplannercad_core::item::{
    DESIGNATOR_TOKEN, FIELD_DESIGNATOR, FillStyle, HAlign, Item, PadKind, PadShape, PinShape, PinType, PowerStyle, VALUE_TOKEN, VAlign,
};
use pcbplannercad_core::layer;
use pcbplannercad_core::units::{Point, mm};

use super::common::{YAxis, arc_from_3pts};
use super::schematic::{arsch_name, orientation, place_text, power_style};
use super::sexpr::{Sexpr, parse};
use super::*;
use crate::SchematicImport;
use crate::report::ImportError;

// ---------- helpers ----------

fn library_share() -> Option<PathBuf> {
    let p=std::env::var_os("PCAD_TEST_STRUCTURED_SHARE").map(PathBuf::from)?;
    p.is_dir().then_some(p)
}

fn files_with_ext(dir: &Path, ext: &str) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == ext)).collect())
        .unwrap_or_default();
    v.sort();
    v
}

// ---------- S-expressions ----------

#[test]
fn structured_sexpr_parses_atoms_strings_and_lists() {
    let l = parse("(a 1 -2.5 \"q \\\"x\\\"\\n\" (b) (c \"\"))").unwrap();
    assert_eq!(l.head(), "a");
    assert_eq!(l.num(1), Some(1.0));
    assert_eq!(l.num(2), Some(-2.5));
    assert_eq!(l.atom(3), Some("q \"x\"\n"));
    assert!(matches!(&l.items[3], Sexpr::Atom(a) if a.quoted));
    assert!(l.child("b").is_some());
    assert_eq!(l.child_str("c"), Some(""));
    // A quoted "b" is not a keyword.
    let q = parse("(x (\"b\" 1))").unwrap();
    assert!(q.child("b").is_none());
}

#[test]
fn structured_sexpr_flags_in_all_spellings() {
    let l = parse("(p hide (a) (b yes) (c no) (d true))").unwrap();
    assert!(l.flag("hide") && l.flag("a") && l.flag("b") && l.flag("d"));
    assert!(!l.flag("c") && !l.flag("e"));
}

#[test]
fn structured_sexpr_tracks_lines_and_reports_errors() {
    let l = parse("(root\n  (a\n    \"multi\nline\")\n  (b))").unwrap();
    assert_eq!(l.child("a").unwrap().line, 2);
    assert_eq!(l.child("b").unwrap().line, 5);

    match parse("(root\n (a\n") {
        Err(ImportError::Syntax { line, .. }) => assert_eq!(line, 1),
        other => panic!("expected syntax error, got {other:?}"),
    }
    match parse("(a \"unterminated\n\n") {
        Err(ImportError::Syntax { line, message }) => {
            assert_eq!(line, 1);
            assert!(message.contains("unterminated"));
        }
        other => panic!("expected syntax error, got {other:?}"),
    }
    assert!(matches!(parse(")"), Err(ImportError::Syntax { line: 1, .. })));
    assert!(matches!(parse("   "), Err(ImportError::Syntax { .. })));
    assert!(matches!(parse("atom"), Err(ImportError::Syntax { .. })));
    let deep = "(".repeat(100_000);
    assert!(matches!(parse(&deep), Err(ImportError::Syntax { .. })));
    // Unicode and unknown escapes survive.
    let u = parse("(t \"Ω \\x µ\")").unwrap();
    assert_eq!(u.atom(1), Some("Ω \\x µ"));
    // A BOM is skipped.
    assert!(parse("\u{feff}(x)").is_ok());
}

#[test]
fn structured_importers_never_panic_on_truncated_input() {
    let sym =
        "(kicad_symbol_lib (version 20241209) (symbol \"R\" (property \"Reference\" \"R\" (at 2 0 90) (effects (font (size 1.27 1.27))))
        (symbol \"R_0_1\" (rectangle (start -1 -2.5) (end 1 2.5) (stroke (width 0.254)) (fill (type none))))
        (symbol \"R_1_1\" (pin passive line (at 0 3.81 270) (length 1.27) (name \"~\") (number \"1\")))))";
    let fp = "(footprint \"X\" (layer \"F.Cu\") (pad \"1\" thru_hole oval (at 1 2 90) (size 1 2) (drill oval 0.5 1) (layers \"*.Cu\" \"*.Mask\"))
        (fp_arc (start 0 0) (mid 1 1) (end 2 0) (stroke (width 0.1)) (layer \"F.SilkS\")))";
    for cut in 0..sym.len() {
        if sym.is_char_boundary(cut) {
            let _ = parse_symbol_library(&sym[..cut], "T");
        }
    }
    for cut in 0..fp.len() {
        if fp.is_char_boundary(cut) {
            let _ = parse_footprint(&fp[..cut]);
        }
    }
    // Wrong file kinds are rejected, not misread.
    assert!(matches!(parse_symbol_library(fp, "T"), Err(ImportError::Invalid(_))));
    assert!(matches!(parse_footprint(sym), Err(ImportError::Invalid(_))));
    // Nonsense values inside valid structure are tolerated.
    let weird =
        "(kicad_symbol_lib (symbol \"A\" (symbol \"A_x_y\" (pin bogus weird (at a b c)) (arc (start 0 0) (mid 0 0) (end 0 0)) (circle))))";
    let (lib, report) = parse_symbol_library(weird, "W").unwrap();
    assert_eq!(lib.symbols.len(), 1);
    assert!(!report.messages.is_empty());
}

// ---------- geometry ----------

/// Angle difference in degrees, wrapped to [-180, 180).
fn adiff(a: f64, b: f64) -> f64 {
    (a - b + 180.0).rem_euclid(360.0) - 180.0
}

#[test]
fn structured_arc_from_three_points() {
    // Quarter circle around the origin, counter-clockwise from +X to +Y.
    let r = mm(1.0);
    let s = Point::new(r, 0);
    let m = Point::new(mm(std::f64::consts::FRAC_1_SQRT_2), mm(std::f64::consts::FRAC_1_SQRT_2));
    let e = Point::new(0, r);
    let a = arc_from_3pts(s, m, e).unwrap();
    assert!(a.center.x.abs() <= 1 && a.center.y.abs() <= 1, "{a:?}");
    assert!((a.radius - r).abs() <= 1);
    assert!(adiff(a.start_angle, 0.0).abs() < 1e-3 && (a.sweep - 90.0).abs() < 1e-3, "{a:?}");

    // The same arc walked clockwise is normalized to a CCW sweep from the other end.
    let b = arc_from_3pts(e, m, s).unwrap();
    assert!(adiff(b.start_angle, 0.0).abs() < 1e-3 && (b.sweep - 90.0).abs() < 1e-3, "{b:?}");

    // Major arc: start (1,0) through (-1,0) to (0,-1) → 270° CCW... from (1,0).
    let c = arc_from_3pts(Point::new(r, 0), Point::new(-r, 0), Point::new(0, -r)).unwrap();
    assert!(adiff(c.start_angle, 0.0).abs() < 1e-3 && (c.sweep - 270.0).abs() < 1e-3, "{c:?}");

    // Collinear points are rejected.
    assert!(arc_from_3pts(Point::new(0, 0), Point::new(1000, 0), Point::new(2000, 0)).is_none());

    // Y-down input: structured format's footprint arc from (1,0) via (0.707,-0.707) to (0,-1) is an
    // upward, counter-clockwise-on-screen quarter arc → in Y-up it is 0..90.
    let y = YAxis::Down;
    let d =
        arc_from_3pts(y.pt(1.0, 0.0), y.pt(std::f64::consts::FRAC_1_SQRT_2, -std::f64::consts::FRAC_1_SQRT_2), y.pt(0.0, -1.0)).unwrap();
    assert!(adiff(d.start_angle, 0.0).abs() < 1e-3 && (d.sweep - 90.0).abs() < 1e-3, "{d:?}");
}

#[test]
fn structured_orientation_and_text_frames() {
    assert_eq!(orientation(90.0, None), (90.0, false));
    assert_eq!(orientation(0.0, Some("y")), (0.0, true));
    assert_eq!(orientation(0.0, Some("x")), (180.0, true));
    // structured format mirrors after rotating: `(at x y 90) (mirror x)` is a quarter turn, then up/down flip.
    assert_eq!(orientation(90.0, Some("x")), (90.0, true));
    assert_eq!(orientation(90.0, Some("y")), (270.0, true));
    let (rot, mirror) = orientation(90.0, Some("x"));
    let pl = Placement { offset: Point::default(), rotation: rot, mirror };
    // local (1,0) → rotate 90 → (0,1) → flip up/down → (0,-1)
    assert_eq!(pl.apply(Point::new(1, 0)), Point::new(0, -1));
    // mirror about X really negates Y on screen.
    let (rot, mirror) = orientation(0.0, Some("x"));
    let pl = Placement { offset: Point::default(), rotation: rot, mirror };
    assert_eq!(pl.apply(Point::new(10, 20)), Point::new(10, -20));

    let id = Placement::default();
    assert_eq!(place_text(0.0, HAlign::Left, VAlign::Bottom, &id), (0.0, HAlign::Left, VAlign::Bottom));
    let r180 = Placement { rotation: 180.0, ..Default::default() };
    assert_eq!(place_text(0.0, HAlign::Left, VAlign::Bottom, &r180), (0.0, HAlign::Right, VAlign::Top));
    let r90 = Placement { rotation: 90.0, ..Default::default() };
    assert_eq!(place_text(90.0, HAlign::Left, VAlign::Middle, &r90), (0.0, HAlign::Right, VAlign::Middle));
    let my = Placement { mirror: true, ..Default::default() };
    assert_eq!(place_text(0.0, HAlign::Left, VAlign::Bottom, &my), (0.0, HAlign::Right, VAlign::Bottom));

    assert_eq!(power_style("GND"), PowerStyle::Ground);
    assert_eq!(power_style("GNDA"), PowerStyle::SignalGround);
    assert_eq!(power_style("Earth_Protective"), PowerStyle::Earth);
    assert_eq!(power_style("+3V3"), PowerStyle::Bar);
    assert_eq!(arsch_name("sub\\amp.kicad_sch"), "sub/amp.arsch");
}

// ---------- footprints ----------

const FP: &str = r#"(footprint "Test:FP"
  (version 20240108) (generator "pcbnew") (layer "F.Cu")
  (descr "A test footprint")
  (property "Reference" "REF**" (at 0 -2 0) (layer "F.SilkS") (effects (font (size 1 1) (thickness 0.15))))
  (property "Value" "FP" (at 0 2 90) (layer "F.Fab") (effects (font (size 0.8 0.8))))
  (property "Datasheet" "" (at 0 0 0) (layer "F.Fab") (hide yes) (effects (font (size 1 1))))
  (fp_text user "${REFERENCE}" (at 0 0.5 0) (layer "F.Fab") (effects (font (size 0.5 0.5))))
  (fp_line (start -1 -1) (end 1 -1) (stroke (width 0.12) (type solid)) (layer "F.SilkS"))
  (fp_rect (start -2 -1.5) (end 2 1.5) (stroke (width 0.05) (type solid)) (fill no) (layer "F.CrtYd"))
  (fp_circle (center 0 0) (end 0.5 0) (stroke (width 0.1)) (fill yes) (layer "F.Fab"))
  (fp_arc (start 1 0) (mid 0.7071 -0.7071) (end 0 -1) (stroke (width 0.1)) (layer "F.Fab"))
  (fp_poly (pts (xy 0 0) (xy 1 0) (xy 1 1)) (stroke (width 0)) (fill solid) (layer "B.SilkS"))
  (fp_line (start 0 0) (end 1 1) (stroke (width 0.1)) (layer "Cmts.User"))
  (fp_line (start 0 0) (end 1 1) (stroke (width 0.1)) (layer "F.Adhes"))
  (pad "1" smd roundrect (at -1.5 0.75 90) (size 0.8 0.95) (layers "F.Cu" "F.Mask" "F.Paste") (roundrect_rratio 0.25))
  (pad "2" smd rect (at 1.5 0.75) (size 0.8 0.95) (layers "B.Cu" "B.Mask"))
  (pad "3" thru_hole oval (at 0 3) (size 1.2 2) (drill oval 0.6 1.2) (layers "*.Cu" "*.Mask"))
  (pad "" np_thru_hole circle (at 0 -3) (size 1 1) (drill 1) (layers "*.Cu" "*.Mask"))
  (pad "" smd rect (at 0 0) (size 0.5 0.5) (layers "F.Paste"))
  (pad "4" smd trapezoid (at 2 2) (size 1 1) (rect_delta 0 0.2) (layers "F.Cu"))
  (model "${STRUCTURED10_3DMODEL_DIR}/X.3dshapes/X.step" (offset (xyz 0 0 0.5)) (scale (xyz 1 1 1)) (rotate (xyz 0 0 90)))
)"#;

#[test]
fn structured_footprint_flips_y_and_maps_layers() {
    let (fp, report) = parse_footprint(FP).unwrap();
    assert_eq!(fp.name, "FP");
    assert_eq!(fp.description, "A test footprint");
    let pads: Vec<_> = fp.items.iter().filter_map(|i| if let Item::Pad(p) = i { Some(p) } else { None }).collect();
    assert_eq!(pads.len(), 5);
    let p1 = pads[0];
    assert_eq!(
        (p1.pos, p1.rotation, p1.shape, p1.layer, p1.kind),
        (Point::new(mm(-1.5), mm(-0.75)), 90.0, PadShape::RoundRect, layer::TOP_COPPER, PadKind::Smd)
    );
    assert_eq!(p1.corner_ratio, 0.25);
    assert_eq!(pads[1].layer, layer::BOTTOM_COPPER);
    let p3 = pads[2];
    assert_eq!((p3.kind, p3.layer, p3.shape, p3.pos), (PadKind::Tht, layer::MULTI_LAYER, PadShape::Oval, Point::new(0, mm(-3.0))));
    assert_eq!(p3.drill.map(|d| (d.width, d.height)), Some((mm(0.6), mm(1.2))));
    assert_eq!((pads[3].kind, pads[3].pos), (PadKind::Npth, Point::new(0, mm(3.0))));
    assert_eq!(pads[4].shape, PadShape::Rect); // trapezoid
    for p in &pads {
        Item::Pad((*p).clone()).validate().unwrap();
    }

    let texts: Vec<_> = fp.items.iter().filter_map(|i| if let Item::Text(t) = i { Some(t) } else { None }).collect();
    assert_eq!(texts.len(), 3, "hidden property skipped");
    assert_eq!((texts[0].text.as_str(), texts[0].pos, texts[0].layer), (DESIGNATOR_TOKEN, Point::new(0, mm(2.0)), layer::TOP_SILK));
    assert_eq!((texts[1].text.as_str(), texts[1].rotation, texts[1].height), (VALUE_TOKEN, 90.0, mm(0.8)));
    assert_eq!(texts[2].text, DESIGNATOR_TOKEN);

    let line = fp.items.iter().find_map(|i| if let Item::Line(l) = i { Some(l) } else { None }).unwrap();
    assert_eq!(
        (line.a, line.b, line.width, line.layer),
        (Point::new(mm(-1.0), mm(1.0)), Point::new(mm(1.0), mm(1.0)), mm(0.12), layer::TOP_SILK)
    );
    assert!(fp.items.iter().any(|i| matches!(i, Item::Rect(r) if r.layer == layer::TOP_COURTYARD && r.fill == FillStyle::None)));
    assert!(fp.items.iter().any(|i| matches!(i, Item::Circle(c) if c.radius == mm(0.5) && c.fill == FillStyle::Solid)));
    let arc = fp.items.iter().find_map(|i| if let Item::Arc(a) = i { Some(a) } else { None }).unwrap();
    assert!(adiff(arc.start_angle, 0.0).abs() < 0.05 && (arc.sweep - 90.0).abs() < 0.05, "{arc:?}");
    assert!(fp.items.iter().any(
        |i| matches!(i, Item::Polyline(p) if p.layer == layer::BOTTOM_SILK && p.closed && p.points[2] == Point::new(mm(1.0), mm(-1.0)))
    ));
    assert!(fp.items.iter().any(|i| matches!(i, Item::Line(l) if l.layer == layer::MECHANICAL_1)));
    // The paste-only aperture became a filled rectangle on top paste.
    assert!(fp.items.iter().any(|i| matches!(i, Item::Rect(r) if r.layer == layer::TOP_PASTE && r.fill == FillStyle::Solid)));

    let model = fp.model.unwrap();
    assert_eq!(model.path, "${STRUCTURED10_3DMODEL_DIR}/X.3dshapes/X.step");
    assert_eq!((model.offset, model.rotation, model.scale), ([0.0, 0.0, 0.5], [0.0, 0.0, 90.0], [1.0; 3]));

    let all = report.messages.iter().map(|m| m.text.as_str()).collect::<Vec<_>>().join("\n");
    assert!(all.contains("Mechanical 1") && all.contains("F.Adhes") && all.contains("trapezoid"), "{all}");
}

#[test]
fn structured_footprint_legacy_structured6_syntax() {
    let fp6 = r#"(footprint "Old" (layer "F.Cu") (tedit 5B307E4C)
      (fp_text reference "REF**" (at 0 -1.5) (layer "F.SilkS") (effects (font (size 1 1) (thickness 0.15))))
      (fp_text value "Old" (at 0 1.5) (layer "F.Fab") hide (effects (font (size 1 1))))
      (fp_line (start 0 0) (end 1 0) (layer "F.SilkS") (width 0.12))
      (fp_arc (start 0 0) (end 1 0) (angle -90) (layer "F.SilkS") (width 0.12))
      (pad 1 thru_hole circle (at 0 0) (size 1.7 1.7) (drill 1) (layers *.Cu *.Mask))
      (model ${KISYS3DMOD}/X.wrl (at (xyz 0 0 0.1)) (scale (xyz 1 1 1)) (rotate (xyz 0 0 0))))"#;
    let (fp, _) = parse_footprint(fp6).unwrap();
    let texts = fp.items.iter().filter(|i| matches!(i, Item::Text(_))).count();
    assert_eq!(texts, 1, "hidden value skipped");
    assert!(fp.items.iter().any(|i| matches!(i, Item::Line(l) if l.width == mm(0.12))));
    // angle -90 = counter-clockwise on screen: from +X up to +Y (world).
    let arc = fp.items.iter().find_map(|i| if let Item::Arc(a) = i { Some(a) } else { None }).unwrap();
    assert!(adiff(arc.start_angle, 0.0).abs() < 1e-3 && (arc.sweep - 90.0).abs() < 1e-3, "{arc:?}");
    assert!(fp.items.iter().any(|i| matches!(i, Item::Pad(p) if p.number == "1" && p.kind == PadKind::Tht && p.shape == PadShape::Circle)));
    assert!((fp.model.unwrap().offset[2] - 2.54).abs() < 1e-9);
}

#[test]
fn structured_footprint_libraries_parse() {
    let Some(share) = library_share() else { return };
    let root = share.join("footprints");
    // Every footprint of every library (STRUCTURED_FOOTPRINT_SAMPLE=n limits it to n per library).
    let per_lib = std::env::var("STRUCTURED_FOOTPRINT_SAMPLE").ok().and_then(|s| s.parse().ok()).unwrap_or(usize::MAX);
    let t = Instant::now();
    let mut libs: Vec<PathBuf> =
        std::fs::read_dir(&root).unwrap().filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.is_dir()).collect();
    libs.sort();
    let (mut ok, mut pads, mut failed) = (0usize, 0usize, Vec::new());
    let mut messages = 0;
    for lib in &libs {
        for f in files_with_ext(lib, "kicad_mod").into_iter().take(per_lib) {
            let text = std::fs::read_to_string(&f).unwrap();
            match parse_footprint(&text) {
                Ok((fp, r)) => {
                    ok += 1;
                    messages += r.messages.len();
                    for i in &fp.items {
                        i.validate().unwrap_or_else(|e| panic!("{}: invalid {:?}: {e}", f.display(), i));
                        if matches!(i, Item::Pad(_)) {
                            pads += 1;
                        }
                    }
                }
                Err(e) => failed.push(format!("{}: {e}", f.display())),
            }
        }
    }
    println!("footprints: {} libraries, {ok} footprints, {pads} pads, {messages} report messages, {:?}", libs.len(), t.elapsed());
    assert!(failed.is_empty(), "{failed:#?}");
    assert!(libs.len() > 100 && ok > 10_000 || per_lib < usize::MAX);

    // Whole-folder import.
    let (lib, _) = import_footprint_dir(&root.join("Resistor_SMD.pretty"), "Resistor_SMD").unwrap();
    let r0603 = lib.footprint("R_0603_1608Metric").expect("R_0603 present");
    assert_eq!(r0603.items.iter().filter(|i| matches!(i, Item::Pad(_))).count(), 2);
    assert!(r0603.model.as_ref().is_some_and(|m| m.path.contains("3DMODEL_DIR")));
}

// ---------- symbols ----------

#[test]
fn structured_symbol_features_inline() {
    let lib = r##"(kicad_symbol_lib (version 20211014) (generator structured_symbol_editor)
      (symbol "BASE" (pin_names (offset 1.016) hide) (pin_numbers hide) (in_bom yes) (on_board yes)
        (property "Reference" "U" (id 0) (at -2.54 5.08 0) (effects (font (size 1.27 1.27)) (justify left)))
        (property "Value" "BASE" (id 1) (at 0 -5.08 0) (effects (font (size 1.524 1.524)) hide))
        (property "Footprint" "Package_DIP:DIP-14_W7.62mm" (id 2) (at 0 0 0) (effects (font (size 1.27 1.27)) hide))
        (property "ki_description" "Base part" (id 4) (at 0 0 0) (effects (font (size 1.27 1.27)) hide))
        (property "MPN" "X123" (id 5) (at 0 0 0) (effects (font (size 1.27 1.27)) hide))
        (symbol "BASE_0_1" (rectangle (start -2.54 2.54) (end 2.54 -2.54) (stroke (width 0.254) (type default)) (fill (type background))))
        (symbol "BASE_1_1" (pin input line (at -5.08 0 0) (length 2.54) (name "A" (effects (font (size 1.27 1.27)))) (number "1" (effects (font (size 1.27 1.27))))))
        (symbol "BASE_1_2" (circle (center 0 0) (radius 1) (stroke (width 0)) (fill (type none))))
        (symbol "BASE_2_1" (unit_name "Power") (pin power_in line (at 0 5.08 270) (length 2.54) hide (name "VCC" (effects (font (size 1.27 1.27)))) (number "14" (effects (font (size 1.27 1.27)))))
          (text "Hi" (at 0 0 900) (effects (font (size 1 1))))
          (arc (start 1 0) (mid 0 1) (end -1 0) (stroke (width 0)) (fill (type none)))
          (polyline (pts (xy 0 0) (xy 1 0) (xy 1 1) (xy 0 0)) (stroke (width 0)) (fill (type outline)))
          (pin output inverted_clock (at 5.08 0 180) (length 2.54) (name "~" (effects (font (size 1.27 1.27)))) (number "2" (effects (font (size 1.27 1.27)))))
          (pin bidirectional output_low (at 5.08 2.54 180) (length 2.54) (name "Q" (effects (font (size 1.27 1.27)))) (number "3" (effects (font (size 1.27 1.27)))))))
      (symbol "DERIVED" (extends "BASE")
        (property "Reference" "IC" (id 0) (at 0 7 0) (effects (font (size 1.27 1.27))))
        (property "Value" "DERIVED" (id 1) (at 0 -7 0) (effects (font (size 1.27 1.27))))
        (property "Footprint" "" (id 2) (at 0 0 0) (effects (font (size 1.27 1.27)) hide)))
      (symbol "VCC" (power) (pin_names (offset 0) hide)
        (property "Reference" "#PWR" (id 0) (at 0 -3.81 0) (effects (font (size 1.27 1.27)) hide))
        (property "Value" "VCC" (id 1) (at 0 3.81 0) (effects (font (size 1.27 1.27))))
        (symbol "VCC_0_1" (polyline (pts (xy -0.762 1.27) (xy 0 2.54)) (stroke (width 0)) (fill (type none))))
        (symbol "VCC_1_1" (pin power_in line (at 0 0 90) (length 0) hide (name "VCC" (effects (font (size 1.27 1.27)))) (number "1" (effects (font (size 1.27 1.27))))))))"##;
    let (lib, report) = parse_symbol_library(lib, "T").unwrap();
    assert_eq!(lib.symbols.len(), 3);
    let base = lib.symbol("BASE").unwrap();
    assert_eq!(base.units.len(), 2);
    assert_eq!(base.units[1].name, "Power");
    assert!(!base.show_pin_names && !base.show_pin_numbers);
    assert_eq!(base.pin_name_offset, mm(1.016));
    assert_eq!(base.prefix, "U");
    assert_eq!(base.description, "Base part");
    // Unit 0 graphics are in both units; the De Morgan circle is not.
    for u in &base.units {
        assert!(u.items.iter().any(|i| matches!(i, Item::Rect(r) if r.fill == FillStyle::Background)));
        assert!(!u.items.iter().any(|i| matches!(i, Item::Circle(_))));
    }
    let pins: Vec<_> = base.unit_pins(2).collect();
    assert_eq!(pins.len(), 3);
    assert!(pins[0].hidden && pins[0].electrical == PinType::PowerIn && pins[0].rotation == 270.0);
    assert_eq!((pins[1].name.as_str(), pins[1].shape), ("", PinShape::InvertedClock));
    assert_eq!(pins[2].shape, PinShape::Line);
    let u2 = &base.units[1].items;
    assert!(u2.iter().any(|i| matches!(i, Item::Text(t) if t.rotation == 90.0 && t.height == mm(1.0))));
    assert!(u2.iter().any(|i| matches!(i, Item::Arc(a) if (a.sweep - 180.0).abs() < 1e-3 && a.radius == mm(1.0))));
    assert!(u2.iter().any(|i| matches!(i, Item::Polyline(p) if p.closed && p.points.len() == 3 && p.fill == FillStyle::Solid)));
    let fields: Vec<_> = base.fields.iter().map(|f| (f.name.as_str(), f.value.as_str(), f.visible)).collect();
    assert_eq!(fields, vec![(FIELD_DESIGNATOR, "U", true), ("Value", "BASE", false)]);
    assert_eq!(base.fields[0].offset, Point::new(mm(-2.54), mm(5.08)));
    assert_eq!(base.fields[0].h_align, HAlign::Left);
    assert_eq!(base.fields[1].height, mm(1.524));

    let c = lib.component("BASE").unwrap();
    assert_eq!(c.footprints, vec!["Package_DIP:DIP-14_W7.62mm".to_string()]);
    assert_eq!(c.parameter("Value"), Some("BASE"));
    assert_eq!(c.parameter("MPN"), Some("X123"));
    assert_eq!(c.parameter("ki_description"), Some("Base part"));

    let d = lib.symbol("DERIVED").unwrap();
    assert_eq!(d.units, base.units);
    assert_eq!(d.prefix, "IC");
    assert!(!d.show_pin_names);
    let dc = lib.component("DERIVED").unwrap();
    assert!(dc.footprints.is_empty(), "derived symbol overrides the footprint");
    assert_eq!(dc.parameter("MPN"), Some("X123"), "inherited user property");
    assert_eq!(dc.parameter("Value"), Some("DERIVED"));

    let vcc = lib.symbol("VCC").unwrap();
    assert!(vcc.is_power);
    assert_eq!(vcc.prefix, "#PWR");

    let all = report.messages.iter().map(|m| m.to_string()).collect::<Vec<_>>().join("\n");
    assert!(all.contains("De Morgan") && all.contains("output_low"), "{all}");
}

#[test]
fn structured_device_r_symbol() {
    let Some(share) = library_share() else { return };
    let text = std::fs::read_to_string(share.join("symbols/Device.kicad_sym")).unwrap();
    let (lib, _) = parse_symbol_library(&text, "Device").unwrap();
    let r = lib.symbol("R").expect("R");
    assert_eq!(r.prefix, "R");
    assert_eq!(r.units.len(), 1);
    assert!(!r.show_pin_numbers);
    assert_eq!(r.pin_name_offset, 0);
    let pins: Vec<_> = r.unit_pins(1).collect();
    assert_eq!(pins.len(), 2);
    let p1 = pins.iter().find(|p| p.number == "1").unwrap();
    let p2 = pins.iter().find(|p| p.number == "2").unwrap();
    assert_eq!((p1.pos, p1.rotation, p1.length), (Point::new(0, mm(3.81)), 270.0, mm(1.27)));
    assert_eq!((p2.pos, p2.rotation), (Point::new(0, mm(-3.81)), 90.0));
    // Pins point into the body: body end of pin 1 touches the top of the rectangle.
    assert_eq!(p1.body_end(), Point::new(0, mm(2.54)));
    assert!(r.units[0].items.iter().any(|i| matches!(i, Item::Rect(x) if x.width == mm(0.254))));
    let reference = r.fields.iter().find(|f| f.name == FIELD_DESIGNATOR).unwrap();
    assert_eq!((reference.offset, reference.rotation, reference.value.as_str()), (Point::new(mm(2.032), 0), 90.0, "R"));
    let c = lib.component("R").unwrap();
    assert_eq!(c.symbol, "R");
    assert_eq!(c.parameter("Description"), Some("Resistor"));
    assert_eq!(c.parameter("ki_keywords"), Some("R res resistor"));
    assert!(c.footprints.is_empty());
}

#[test]
fn structured_all_symbol_libraries_parse() {
    let Some(share) = library_share() else { return };
    let t = Instant::now();
    let files = files_with_ext(&share.join("symbols"), "kicad_sym");
    let (mut symbols, mut pins, mut derived, mut messages) = (0usize, 0usize, 0usize, 0usize);
    let mut failed = Vec::new();
    for f in &files {
        let text = std::fs::read_to_string(f).unwrap();
        let name = f.file_stem().unwrap().to_string_lossy().into_owned();
        match parse_symbol_library(&text, &name) {
            Ok((lib, report)) => {
                assert_eq!(lib.symbols.len(), lib.components.len());
                symbols += lib.symbols.len();
                messages += report.messages.len();
                derived += text.matches("(extends ").count();
                for s in &lib.symbols {
                    assert!(!s.units.is_empty());
                    for u in &s.units {
                        for i in &u.items {
                            i.validate().unwrap_or_else(|e| panic!("{name}:{}: invalid {i:?}: {e}", s.name));
                        }
                    }
                    pins += s.all_pins().count();
                }
            }
            Err(e) => failed.push(format!("{}: {e}", f.display())),
        }
    }
    println!(
        "symbol libraries: {} files, {symbols} symbols ({derived} derived), {pins} pins, {messages} report messages, {:?}",
        files.len(),
        t.elapsed()
    );
    assert!(failed.is_empty(), "{failed:#?}");
    assert!(files.len() > 200 && symbols > 10_000);
}

// ---------- schematics ----------

#[derive(Default, Debug)]
struct Connectivity {
    endpoints: usize,
    endpoints_ok: usize,
    /// Endpoints not touching another wire: they must sit on a pin, label, port…
    terminals: usize,
    terminals_ok: usize,
    label_stubs: usize,
    power_ports: usize,
    power_ok: usize,
    parts: usize,
    missing_symbols: usize,
}

impl Connectivity {
    fn add(&mut self, o: &Connectivity) {
        self.endpoints += o.endpoints;
        self.endpoints_ok += o.endpoints_ok;
        self.terminals += o.terminals;
        self.terminals_ok += o.terminals_ok;
        self.label_stubs += o.label_stubs;
        self.power_ports += o.power_ports;
        self.power_ok += o.power_ok;
        self.parts += o.parts;
        self.missing_symbols += o.missing_symbols;
    }

    fn terminal_ratio(&self) -> f64 {
        if self.terminals == 0 { 1.0 } else { self.terminals_ok as f64 / self.terminals as f64 }
    }

    fn endpoint_ratio(&self) -> f64 {
        if self.endpoints == 0 { 1.0 } else { self.endpoints_ok as f64 / self.endpoints as f64 }
    }
}

fn connectivity(imp: &SchematicImport) -> Connectivity {
    let mut total = Connectivity::default();
    for sheet in &imp.sheets {
        let doc = &sheet.document;
        let mut anchors: HashSet<Point> = HashSet::new();
        let mut pins: HashSet<Point> = HashSet::new();
        let mut wires = Vec::new();
        let mut buses = Vec::new();
        let mut labels = Vec::new();
        let mut powers = Vec::new();
        let mut c = Connectivity::default();
        for (_, item) in doc.items() {
            match item {
                Item::Wire(w) => wires.push((w.a, w.b)),
                Item::Bus(b) => buses.push((b.a, b.b)),
                Item::Part(p) => {
                    c.parts += 1;
                    match doc.cached_part(&p.lib_ref) {
                        Some(cp) => {
                            let pl = p.placement();
                            for pin in cp.symbol.unit_pins(p.unit) {
                                pins.insert(pl.apply(pin.pos));
                            }
                        }
                        None => c.missing_symbols += 1,
                    }
                }
                Item::PowerPort(pp) => {
                    anchors.insert(pp.pos);
                    powers.push(pp.pos);
                }
                Item::NetLabel(l) => {
                    anchors.insert(l.pos);
                    labels.push(l.pos);
                }
                Item::Junction(j) => {
                    anchors.insert(j.pos);
                }
                Item::NoConnect(n) => {
                    anchors.insert(n.pos);
                }
                Item::BusEntry(b) => {
                    anchors.insert(b.pos);
                    anchors.insert(b.pos.translated(b.size.x, b.size.y));
                }
                Item::Sheet(s) => {
                    for e in &s.entries {
                        anchors.insert(s.entry_point(e));
                    }
                }
                _ => {}
            }
        }
        anchors.extend(pins.iter().copied());
        let status = |i: usize, e: Point| {
            let on_wire = wires.iter().enumerate().any(|(j, &(c0, c1))| j != i && on_segment(e, c0, c1));
            // Old designs end wires on bus lines (connectivity comes from labels).
            let anchored = anchors.contains(&e) || buses.iter().any(|&(c0, c1)| on_segment(e, c0, c1));
            (on_wire, anchored)
        };
        for (i, &(a, b)) in wires.iter().enumerate() {
            for (e, other) in [(a, b), (b, a)] {
                let (on_wire, anchored) = status(i, e);
                if !on_wire && !anchored {
                    // A "label stub": a wire from a connected point carrying a net label,
                    // whose far end is left open (it is dangling in structured format too).
                    let (ow, oa) = status(i, other);
                    if (ow || oa) && labels.iter().any(|&l| on_segment(l, a, b)) {
                        c.label_stubs += 1;
                        continue;
                    }
                }
                c.endpoints += 1;
                if on_wire || anchored {
                    c.endpoints_ok += 1;
                }
                if !on_wire {
                    c.terminals += 1;
                    if anchored {
                        c.terminals_ok += 1;
                    }
                }
            }
        }
        for p in powers {
            c.power_ports += 1;
            if pins.contains(&p) || wires.iter().any(|&(a, b)| on_segment(p, a, b)) {
                c.power_ok += 1;
            }
        }
        total.add(&c);
    }
    total
}

fn demo_roots(share: &Path) -> Vec<PathBuf> {
    let demos = share.join("demos");
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(&demos) else { return out };
    let mut dirs: Vec<PathBuf> = rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.is_dir()).collect();
    dirs.sort();
    for d in dirs {
        for pro in files_with_ext(&d, "kicad_pro") {
            let sch = pro.with_extension("kicad_sch");
            if sch.is_file() {
                out.push(sch);
            }
        }
    }
    out
}

#[test]
fn structured_demo_schematics_stay_connected() {
    let Some(share) = library_share() else { return };
    let roots = demo_roots(&share);
    assert!(!roots.is_empty());
    let mut total = Connectivity::default();
    let mut failures = Vec::new();
    let t = Instant::now();
    let mut sheet_count = 0;
    for root in &roots {
        let imp = import_schematic(root).unwrap_or_else(|e| panic!("{}: {e}", root.display()));
        sheet_count += imp.sheets.len();
        for s in &imp.sheets {
            for (_, i) in s.document.items() {
                i.validate().unwrap_or_else(|e| panic!("{} {}: invalid {i:?}: {e}", root.display(), s.file_name));
            }
        }
        let c = connectivity(&imp);
        println!(
            "{:<45} sheets {:>3}  parts {:>5}  wire endpoints {:>6} ok {:>6.2}%  terminal endpoints {:>5} on pin/label/port {:>6.2}%  power ports {:>4} connected {:>4}  label stubs {:>4}  report {:>3}",
            root.file_name().unwrap().to_string_lossy(),
            imp.sheets.len(),
            c.parts,
            c.endpoints,
            100.0 * c.endpoint_ratio(),
            c.terminals,
            100.0 * c.terminal_ratio(),
            c.power_ports,
            c.power_ok,
            c.label_stubs,
            imp.report.messages.len(),
        );
        if c.terminal_ratio() < 0.95 || c.endpoint_ratio() < 0.95 {
            failures.push(format!("{}: {c:?}", root.display()));
        }
        assert_eq!(c.missing_symbols, 0, "{}", root.display());
        total.add(&c);
    }
    println!(
        "TOTAL: {} designs, {sheet_count} sheets, {} parts, endpoints ok {:.2}%, terminal endpoints on pins/labels/ports {:.2}% ({}/{}), power ports connected {}/{}, open label-stub ends excluded: {}, {:?}",
        roots.len(),
        total.parts,
        100.0 * total.endpoint_ratio(),
        100.0 * total.terminal_ratio(),
        total.terminals_ok,
        total.terminals,
        total.power_ok,
        total.power_ports,
        total.label_stubs,
        t.elapsed()
    );
    assert!(failures.is_empty(), "{failures:#?}");
    assert!(total.power_ok as f64 >= 0.95 * total.power_ports as f64);
}

#[test]
fn structured_complex_hierarchy_imports_one_copy_per_instance() {
    let Some(share) = library_share() else { return };
    let pro = share.join("demos/complex_hierarchy/complex_hierarchy.kicad_pro");
    let imp = import_project(&pro).unwrap();
    assert_eq!(imp.project.name, "complex_hierarchy");
    assert_eq!(imp.project.sheets, ["complex_hierarchy.arsch", "ampli_ht.arsch", "ampli_ht_2.arsch"].map(String::from).to_vec());
    let msgs: Vec<_> = imp.schematic.report.messages.iter().map(|m| m.text.as_str()).collect();
    assert!(msgs.iter().any(|t| t.contains("one copy per instance: ampli_ht.arsch, ampli_ht_2.arsch")), "{msgs:?}");
    let root = &imp.schematic.sheets[0].document;
    let files: Vec<_> = root.items().filter_map(|(_, i)| if let Item::Sheet(s) = i { Some(s.file.clone()) } else { None }).collect();
    assert_eq!(files, ["ampli_ht.arsch", "ampli_ht_2.arsch"].map(String::from).to_vec());
    assert!(!imp.schematic.libraries.is_empty());
    // Every placed part resolves through its sheet's cache; designators are unique across copies.
    let mut seen = HashSet::new();
    for s in &imp.schematic.sheets {
        for (_, i) in s.document.items() {
            if let Item::Part(p) = i {
                assert!(s.document.cached_part(&p.lib_ref).is_some(), "{}", p.lib_ref);
                assert!(!p.designator().is_empty());
                assert!(seen.insert((p.designator().to_string(), p.unit)), "duplicate {} in {}", p.designator(), s.file_name);
            }
        }
    }
}

/// A tiny structured format sheet file: `sheets` are (uuid, file); `instances` are structured format 7+
/// (path, reference) pairs for its one part; `k6` fills a root `symbol_instances`.
fn synthetic_sheet(uuid: &str, sheets: &[(&str, &str)], part_uuid: &str, instances: &[(&str, &str)], k6: &[(&str, &str)]) -> String {
    let mut s = format!(
        "(kicad_sch (version 20231120) (uuid \"{uuid}\")\n (lib_symbols (symbol \"Device:R\" (property \"Reference\" \"R\" (at 0 0 0))\n  \
         (symbol \"R_1_1\" (pin passive line (at 0 3.81 270) (length 1.27) (name \"~\") (number \"1\")))))\n"
    );
    for (i, (su, file)) in sheets.iter().enumerate() {
        let y = 20.0 + 30.0 * i as f64;
        s += &format!(
            " (sheet (at 10 {y}) (size 20 10) (uuid \"{su}\") (property \"Sheetname\" \"S{i}\" (at 10 {y} 0))\n  \
             (property \"Sheetfile\" \"{file}\" (at 10 {y} 0)) (pin \"IN\" input (at 10 {} 180)))\n",
            y + 2.54
        );
    }
    if !part_uuid.is_empty() {
        s += &format!(
            " (symbol (lib_id \"Device:R\") (at 50 50 0) (unit 1) (uuid \"{part_uuid}\")\n  (property \"Reference\" \"R?\" (at 52 50 0))"
        );
        if !instances.is_empty() {
            s += " (instances (project \"p\"";
            for (path, r) in instances {
                s += &format!(" (path \"{path}\" (reference \"{r}\") (unit 1))");
            }
            s += "))";
        }
        s += ")\n";
    }
    if !k6.is_empty() {
        s += " (symbol_instances";
        for (path, r) in k6 {
            s += &format!(" (path \"{path}\" (reference \"{r}\") (unit 1))");
        }
        s += ")\n";
    }
    s + ")\n"
}

fn designators(imp: &SchematicImport) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for s in &imp.sheets {
        for (_, i) in s.document.items() {
            if let Item::Part(p) = i {
                out.push((s.file_name.clone(), p.designator().to_string()));
            }
        }
    }
    out
}

fn sheet_files(imp: &SchematicImport, file: &str) -> Vec<String> {
    let doc = &imp.sheets.iter().find(|s| s.file_name == file).unwrap().document;
    doc.items().filter_map(|(_, i)| if let Item::Sheet(s) = i { Some(s.file.clone()) } else { None }).collect()
}

#[test]
fn structured_reused_sheets_get_one_copy_per_instance() {
    let dir = std::env::temp_dir().join(format!("pcbplannercad-structured-reuse-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let names = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let pair = |f: &str, r: &str| (f.to_string(), r.to_string());

    // structured format 7+: root → mid twice; mid → leaf twice (nested reuse, 4 leaf instances).
    std::fs::write(dir.join("top.kicad_sch"), synthetic_sheet("R", &[("S1", "mid.kicad_sch"), ("S2", "mid.kicad_sch")], "", &[], &[]))
        .unwrap();
    std::fs::write(
        dir.join("mid.kicad_sch"),
        synthetic_sheet("M", &[("T1", "leaf.kicad_sch"), ("T2", "leaf.kicad_sch")], "PM", &[("/R/S1", "R10"), ("/R/S2", "R11")], &[]),
    )
    .unwrap();
    std::fs::write(
        dir.join("leaf.kicad_sch"),
        synthetic_sheet("L", &[], "PL", &[("/R/S1/T1", "R1"), ("/R/S1/T2", "R2"), ("/R/S2/T1", "R3"), ("/R/S2/T2", "R4")], &[]),
    )
    .unwrap();
    let imp = import_schematic(&dir.join("top.kicad_sch")).unwrap();
    let files: Vec<_> = imp.sheets.iter().map(|s| s.file_name.as_str()).collect();
    assert_eq!(files, ["top.arsch", "mid.arsch", "mid_2.arsch", "leaf.arsch", "leaf_2.arsch", "leaf_3.arsch", "leaf_4.arsch"]);
    assert_eq!(sheet_files(&imp, "top.arsch"), names(&["mid.arsch", "mid_2.arsch"]));
    assert_eq!(sheet_files(&imp, "mid.arsch"), names(&["leaf.arsch", "leaf_2.arsch"]));
    assert_eq!(sheet_files(&imp, "mid_2.arsch"), names(&["leaf_3.arsch", "leaf_4.arsch"]));
    assert_eq!(
        designators(&imp),
        vec![
            pair("mid.arsch", "R10"),
            pair("mid_2.arsch", "R11"),
            pair("leaf.arsch", "R1"),
            pair("leaf_2.arsch", "R2"),
            pair("leaf_3.arsch", "R3"),
            pair("leaf_4.arsch", "R4"),
        ]
    );
    let notes: Vec<_> = imp.report.messages.iter().map(|m| m.text.as_str()).collect();
    assert!(notes.iter().any(|t| t.contains("one copy per instance: mid.arsch, mid_2.arsch")), "{notes:?}");
    assert!(notes.iter().any(|t| t.contains("leaf.arsch, leaf_2.arsch, leaf_3.arsch, leaf_4.arsch")), "{notes:?}");
    assert!(imp.report.messages.iter().all(|m| m.severity == crate::Severity::Info), "{notes:?}");

    // structured format 6: references in the root's symbol_instances, paths without the root uuid.
    let k6 = [("/S1/PM", "R10"), ("/S2/PM", "R11"), ("/S1/T1/PL", "R1"), ("/S1/T2/PL", "R2"), ("/S2/T1/PL", "R3"), ("/S2/T2/PL", "R4")];
    std::fs::write(dir.join("top6.kicad_sch"), synthetic_sheet("R6", &[("S1", "mid6.kicad_sch"), ("S2", "mid6.kicad_sch")], "", &[], &k6))
        .unwrap();
    std::fs::write(
        dir.join("mid6.kicad_sch"),
        synthetic_sheet("M6", &[("T1", "leaf6.kicad_sch"), ("T2", "leaf6.kicad_sch")], "PM", &[], &[]),
    )
    .unwrap();
    std::fs::write(dir.join("leaf6.kicad_sch"), synthetic_sheet("L6", &[], "PL", &[], &[])).unwrap();
    let imp6 = import_schematic(&dir.join("top6.kicad_sch")).unwrap();
    let d6: Vec<_> = designators(&imp6).into_iter().map(|(_, d)| d).collect();
    assert_eq!(d6, names(&["R10", "R11", "R1", "R2", "R3", "R4"]));

    // A sheet that includes itself stops instead of recursing forever.
    std::fs::write(dir.join("loop.kicad_sch"), synthetic_sheet("LP", &[("X", "loop.kicad_sch")], "", &[], &[])).unwrap();
    let lp = import_schematic(&dir.join("loop.kicad_sch")).unwrap();
    assert_eq!(lp.sheets.len(), 1);
    assert_eq!(sheet_files(&lp, "loop.arsch"), names(&["loop.arsch"]));
    assert!(lp.report.messages.iter().any(|m| m.text.contains("includes itself")));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn structured_video_sheet_entries_and_parts() {
    let Some(share) = library_share() else { return };
    let imp = import_schematic(&share.join("demos/video/video.kicad_sch")).unwrap();
    assert!(imp.sheets.len() > 1);
    let root = &imp.sheets[0].document;
    let sheet = root
        .items()
        .find_map(|(_, i)| if let Item::Sheet(s) = i { (s.name == "muxdata").then_some(s) } else { None })
        .expect("muxdata sheet");
    assert_eq!(sheet.file, "muxdata.arsch");
    assert_eq!(sheet.pos, Point::new(mm(285.75), mm(-106.68)));
    assert_eq!(sheet.size, Point::new(mm(41.91), mm(49.53)));
    let din = sheet.entries.iter().find(|e| e.name == "X_DIN").unwrap();
    assert_eq!(din.side, pcbplannercad_core::item::SheetSide::Left);
    assert_eq!(sheet.entry_point(din), Point::new(mm(285.75), mm(-109.22)));
    assert!(imp.sheets.iter().any(|s| s.file_name == "muxdata.arsch"));
}

#[test]
fn structured_ecc83_parts_power_and_fields() {
    let Some(share) = library_share() else { return };
    let imp = import_schematic(&share.join("demos/ecc83/ecc83-pp.kicad_sch")).unwrap();
    let doc = &imp.sheets[0].document;
    let parts: Vec<_> = doc.items().filter_map(|(_, i)| if let Item::Part(p) = i { Some(p) } else { None }).collect();
    let r1 = parts.iter().find(|p| p.designator() == "R1").expect("R1");
    assert_eq!(r1.pos, Point::new(mm(157.48), mm(-85.09)));
    assert_eq!((r1.rotation, r1.mirror), (180.0, false));
    assert_eq!(r1.field_value("Value"), "1.5K");
    assert_eq!(r1.field_value("Footprint"), "Resistor_THT:R_Axial_DIN0207_L6.3mm_D2.5mm_P7.62mm_Horizontal");
    let des = r1.field(FIELD_DESIGNATOR).unwrap();
    assert_eq!(des.offset, Point::new(mm(154.94 - 157.48), 0));
    assert!(r1.lib_ref.starts_with("ecc83-pp/"));
    let powers = doc.items().filter(|(_, i)| matches!(i, Item::PowerPort(_))).count();
    assert!(powers > 0);
    assert!(imp.libraries.iter().any(|l| l.name == "ecc83-pp" && l.symbol("R").is_some()));
}

#[test]
fn structured_user_project_imports() {
    let Some(pro) = std::env::var_os("PCAD_TEST_STRUCTURED_PROJECT").map(PathBuf::from) else { return };
    if !pro.is_file() {
        println!("NOTE: {} not found; skipping", pro.display());
        return;
    }
    let imp = import_project(&pro).unwrap();
    assert!(!imp.project.name.is_empty());
    assert!(!imp.project.sheets.is_empty());
    println!("user project: {} sheet(s), {} items", imp.schematic.sheets.len(), imp.schematic.sheets[0].document.len());
}

#[test]
fn structured_missing_files_are_errors() {
    assert!(matches!(import_schematic(Path::new("does/not/exist.kicad_sch")), Err(ImportError::Io(_))));
    assert!(matches!(import_project(Path::new("does/not/exist.kicad_pro")), Err(ImportError::Io(_))));
    assert!(import_footprint_dir(Path::new("does/not/exist.pretty"), "X").is_err());
}
