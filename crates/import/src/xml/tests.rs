use std::collections::HashMap;
use std::path::{Path, PathBuf};

use pcbplannercad_core::geom::on_segment;
use pcbplannercad_core::item::{FIELD_DESIGNATOR, FIELD_VALUE, HAlign, Item, LabelScope, PadShape, PinShape, PinType, PowerStyle, VAlign};
use pcbplannercad_core::layer;
use pcbplannercad_core::units::{Point, mm};
use pcbplannercad_core::Document;

use super::library::full_device_name;
use super::package::{smd_pad_shape, tht_pad_geometry};
use super::util::{Rot, parse_rot, resolve_text, strip_html};
use super::*;


// ---------- Unit tests ----------

#[test]
fn rotation_strings() {
    let r = |s| parse_rot(s);
    assert_eq!(r("R90"), Rot { mirror: false, spin: false, angle: 90.0 });
    assert_eq!(r("MR180"), Rot { mirror: true, spin: false, angle: 180.0 });
    assert_eq!(r("SR45"), Rot { mirror: false, spin: true, angle: 45.0 });
    assert_eq!(r("SMR270"), Rot { mirror: true, spin: true, angle: 270.0 });
    assert_eq!(r("MR0"), Rot { mirror: true, spin: false, angle: 0.0 });
    assert_eq!(r("R0"), Rot::default());
    assert_eq!(r(""), Rot::default());
    assert_eq!(r("R360"), Rot::default());
    assert_eq!(r("R-90").angle, 270.0);
    assert_eq!(r("garbage").angle, 0.0);
    // XML instances rotate first and then mirror: MR90 maps local +X to world +Y
    // (core: mirror, then rotate by -90°).
    let p = r("MR90").placement(Point::new(mm(10.0), 0));
    assert_eq!((p.rotation, p.mirror), (270.0, true));
    assert_eq!(p.apply(Point::new(mm(1.0), 0)), Point::new(mm(10.0), mm(1.0)));
    assert_eq!(p.apply(Point::new(0, mm(-5.08))), Point::new(mm(4.92), 0));
    assert_eq!(r("MR90").direction(), 90.0);
    // Texts mirror first and then rotate: MR90 reads downward (270°).
    assert_eq!(r("MR90").text_direction(), 270.0);
    // An R0 text in an MR90 instance is written by XML as MR270.
    assert_eq!(Rot::default().within(r("MR90")), r("MR270"));
    // A text at R0 inside an instance placed MR180 reads as MR180 (XML writes the same).
    assert_eq!(Rot::default().within(r("MR180")), r("MR180"));
    assert_eq!(r("R90").within(r("MR0")), Rot { mirror: true, spin: false, angle: 270.0 });
}

#[test]
fn text_orientation_is_readable() {
    let (h, v) = (HAlign::Left, VAlign::Bottom);
    assert_eq!(resolve_text(parse_rot("R0"), h, v, true), (0.0, HAlign::Left, VAlign::Bottom));
    assert_eq!(resolve_text(parse_rot("R180"), h, v, true), (0.0, HAlign::Right, VAlign::Top));
    assert_eq!(resolve_text(parse_rot("R270"), h, v, true), (90.0, HAlign::Right, VAlign::Top));
    assert_eq!(resolve_text(parse_rot("MR0"), h, v, true), (0.0, HAlign::Right, VAlign::Bottom));
    assert_eq!(resolve_text(parse_rot("SR180"), h, v, true), (180.0, HAlign::Left, VAlign::Bottom));
}

#[test]
fn pad_shape_table() {
    // Automatic diameter: drill + 2 × max(0.25 mm, 25 % drill).
    assert_eq!(tht_pad_geometry("round", 1.0, None), (PadShape::Circle, 1.5, 1.5, 0.0));
    assert_eq!(tht_pad_geometry("round", 2.0, Some(0.0)), (PadShape::Circle, 3.0, 3.0, 0.0));
    assert_eq!(tht_pad_geometry("square", 1.0, Some(1.8)), (PadShape::Rect, 1.8, 1.8, 0.0));
    assert_eq!(tht_pad_geometry("octagon", 1.0, Some(1.8)), (PadShape::RoundRect, 1.8, 1.8, 0.29));
    assert_eq!(tht_pad_geometry("long", 1.0, Some(1.5)), (PadShape::Oval, 3.0, 1.5, 0.0));
    assert_eq!(tht_pad_geometry("offset", 1.0, Some(1.5)), (PadShape::Oval, 3.0, 1.5, 0.0));
    assert_eq!(smd_pad_shape(mm(1.0), mm(0.5), 0.0), (PadShape::Rect, 0.0));
    assert_eq!(smd_pad_shape(mm(1.0), mm(0.5), 50.0), (PadShape::RoundRect, 0.25));
    assert_eq!(smd_pad_shape(mm(1.0), mm(0.5), 100.0), (PadShape::Oval, 0.0));
    assert_eq!(smd_pad_shape(mm(1.0), mm(1.0), 100.0), (PadShape::Circle, 0.0));
}

#[test]
fn device_names_follow_wildcards() {
    assert_eq!(full_device_name("R-US_", "0603", ""), "R-US_0603");
    assert_eq!(full_device_name("MC33269?*", "D", "-5.0"), "MC33269D-5.0");
    assert_eq!(full_device_name("ATMEGA*", "AU", "1280-16"), "ATMEGA1280-16AU");
    assert_eq!(full_device_name("LED", "3MM", ""), "LED3MM");
}

#[test]
fn html_descriptions_are_stripped() {
    assert_eq!(strip_html("<b>PIN HEADER</b>"), "PIN HEADER");
    assert_eq!(strip_html("<b>OP AMP</b> also LM158<p>\nSource: x &amp; y"), "OP AMP also LM158\nSource: x & y");
}

const TEST_LBR: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<!DOCTYPE eagle SYSTEM "eagle.dtd">
<eagle version="9.6.2"><drawing><library>
<packages>
<package name="0603">
<description>&lt;b&gt;Chip&lt;/b&gt;</description>
<smd name="1" x="-0.8" y="0" dx="0.9" dy="1" layer="1" roundness="50"/>
<smd name="2" x="0.8" y="0" dx="0.9" dy="1" layer="1" rot="R180"/>
<wire x1="-1.5" y1="0.8" x2="1.5" y2="0.8" width="0.2" layer="21"/>
<text x="0" y="1" size="1" layer="25" align="bottom-center">&gt;NAME</text>
<text x="0" y="-1" size="1" layer="27">&gt;VALUE</text>
<rectangle x1="-0.2" y1="-0.3" x2="0.2" y2="0.3" layer="35"/>
</package>
<package name="DIL8">
<pad name="1" x="-3.81" y="-3.81" drill="0.8" shape="long" rot="R90"/>
<pad name="2" x="-1.27" y="-3.81" drill="0.8" shape="octagon"/>
<pad name="3" x="1.27" y="-3.81" drill="0.8"/>
<pad name="4" x="3.81" y="-3.81" drill="0.8" diameter="1.6" shape="square"/>
<pad name="5" x="3.81" y="3.81" drill="0.8"/>
<pad name="6" x="1.27" y="3.81" drill="0.8"/>
<pad name="7" x="-1.27" y="3.81" drill="0.8"/>
<pad name="8" x="-3.81" y="3.81" drill="0.8"/>
<hole x="0" y="0" drill="3.2"/>
<wire x1="-5" y1="-2" x2="5" y2="-2" width="0.15" layer="21" curve="-90"/>
</package>
</packages>
<symbols>
<symbol name="R">
<wire x1="-2.54" y1="0.889" x2="2.54" y2="0.889" width="0.254" layer="94"/>
<text x="-3.81" y="1.4" size="1.778" layer="95">&gt;NAME</text>
<text x="-3.81" y="-3.2" size="1.778" layer="96">&gt;VALUE</text>
<pin name="1" x="-5.08" y="0" visible="off" length="short" direction="pas" swaplevel="1"/>
<pin name="2" x="5.08" y="0" visible="off" length="short" direction="pas" swaplevel="1" rot="R180"/>
</symbol>
<symbol name="OPAMP">
<polygon width="0.4" layer="94"><vertex x="-5" y="5"/><vertex x="5" y="0"/><vertex x="-5" y="-5"/></polygon>
<text x="2.54" y="3" size="1.778" layer="95">&gt;NAME</text>
<pin name="-IN" x="-7.62" y="-2.54" visible="pad" length="short" direction="in"/>
<pin name="+IN" x="-7.62" y="2.54" visible="pad" length="short" direction="in"/>
<pin name="OUT" x="7.62" y="0" visible="pad" length="short" direction="out" function="dot" rot="R180"/>
</symbol>
<symbol name="PWR+-">
<pin name="V+" x="0" y="7.62" visible="pad" length="middle" direction="pwr" rot="R270"/>
<pin name="V-" x="0" y="-7.62" visible="pad" length="middle" direction="pwr" rot="R90"/>
</symbol>
<symbol name="GND">
<wire x1="-1.905" y1="0" x2="1.905" y2="0" width="0.254" layer="94"/>
<text x="-2.54" y="-2.54" size="1.778" layer="96">&gt;VALUE</text>
<pin name="GND" x="0" y="2.54" visible="off" length="short" direction="sup" rot="R270"/>
</symbol>
<symbol name="REG">
<pin name="IN" x="-7.62" y="0" length="short" direction="pwr"/>
<pin name="OUT" x="7.62" y="0" length="short" direction="sup" rot="R180"/>
<pin name="OUT@1" x="7.62" y="2.54" length="short" direction="sup" rot="R180"/>
<pin name="GND" x="0" y="-5.08" length="short" direction="pwr" rot="R90"/>
</symbol>
</symbols>
<devicesets>
<deviceset name="R" prefix="R" uservalue="yes">
<description>&lt;b&gt;Resistor&lt;/b&gt;</description>
<gates><gate name="G$1" symbol="R" x="0" y="0"/></gates>
<devices><device name="0603" package="0603">
<connects><connect gate="G$1" pin="1" pad="1"/><connect gate="G$1" pin="2" pad="2"/></connects>
<technologies><technology name=""><attribute name="MPN" value="RC0603" constant="no"/></technology></technologies>
</device></devices>
</deviceset>
<deviceset name="LM358" prefix="IC">
<gates>
<gate name="A" symbol="OPAMP" x="0" y="10"/>
<gate name="B" symbol="OPAMP" x="0" y="-10"/>
<gate name="P" symbol="PWR+-" x="0" y="0" addlevel="request"/>
</gates>
<devices>
<device name="N" package="DIL8">
<connects>
<connect gate="A" pin="+IN" pad="3"/><connect gate="A" pin="-IN" pad="2"/><connect gate="A" pin="OUT" pad="1"/>
<connect gate="B" pin="+IN" pad="5"/><connect gate="B" pin="-IN" pad="6"/><connect gate="B" pin="OUT" pad="7"/>
<connect gate="P" pin="V+" pad="8"/><connect gate="P" pin="V-" pad="4"/>
</connects>
<technologies><technology name=""/></technologies>
</device>
<device name="D" package="DIL8">
<connects>
<connect gate="A" pin="+IN" pad="3"/><connect gate="A" pin="-IN" pad="2"/><connect gate="A" pin="OUT" pad="1"/>
<connect gate="B" pin="+IN" pad="5"/><connect gate="B" pin="-IN" pad="6"/><connect gate="B" pin="OUT" pad="7"/>
<connect gate="P" pin="V+" pad="8"/><connect gate="P" pin="V-" pad="4"/>
</connects>
<technologies><technology name=""/></technologies>
</device>
</devices>
</deviceset>
<deviceset name="GND" prefix="GND">
<gates><gate name="1" symbol="GND" x="0" y="0"/></gates>
<devices><device name=""><technologies><technology name=""/></technologies></device></devices>
</deviceset>
<deviceset name="REG*" prefix="U">
<gates><gate name="G$1" symbol="REG" x="0" y="0"/></gates>
<devices><device name="" package="DIL8">
<connects>
<connect gate="G$1" pin="IN" pad="3"/><connect gate="G$1" pin="OUT" pad="2"/>
<connect gate="G$1" pin="OUT@1" pad="6"/><connect gate="G$1" pin="GND" pad="1 5"/>
</connects>
<technologies><technology name="-3.3"/><technology name="-5.0"/></technologies>
</device></devices>
</deviceset>
</devicesets>
</library></drawing></eagle>
"#;

#[test]
fn pin_numbering_rule() {
    let (lib, _report) = parse_library(TEST_LBR, "Test").unwrap();

    // One packaged device, one pad per pin → pins carry pad names.
    let r = lib.symbol("R").unwrap();
    assert!(r.all_pins().all(|(_, p)| p.number == p.name));
    let c = lib.component("R0603").unwrap();
    assert!(c.pin_map.is_empty());
    assert_eq!(c.footprints, vec!["0603".to_string()]);
    assert_eq!(c.parameter("Value"), Some("R0603"));
    assert_eq!(c.parameter("MPN"), Some("RC0603"));
    assert_eq!(c.description, "Resistor");
    assert_eq!(r.prefix, "R");
    assert!(!r.show_pin_numbers, "pins are visible=off");
    let names: Vec<&str> = r.fields.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, vec![FIELD_DESIGNATOR, FIELD_VALUE]);

    // Two packaged devices → pin names as numbers, explicit pin maps, gate prefixes for repeats.
    let op = lib.symbol("LM358").unwrap();
    assert_eq!(op.units.len(), 3);
    assert_eq!(op.units[1].name, "B");
    assert!(!op.show_pin_numbers);
    let nums: Vec<String> = op.all_pins().map(|(u, p)| format!("{u}:{}", p.number)).collect();
    assert!(nums.contains(&"1:A.+IN".to_string()), "{nums:?}");
    assert!(nums.contains(&"3:V+".to_string()), "{nums:?}");
    let comp = lib.component("LM358N").unwrap();
    assert_eq!(comp.pads_for(2, "B.OUT"), vec!["7".to_string()]);
    assert_eq!(comp.pads_for(3, "V-"), vec!["4".to_string()]);
    let out = op.unit_pins(1).find(|p| p.name == "OUT").unwrap();
    assert_eq!(out.shape, PinShape::Inverted);
    assert_eq!(out.electrical, PinType::Output);
    assert_eq!(out.rotation, 180.0);

    // A pin on two pads → name numbering; `@n` suffix kept in the number only.
    let reg = lib.symbol("REG*").unwrap();
    let out1 = reg.all_pins().find(|(_, p)| p.number == "OUT@1").unwrap().1;
    assert_eq!(out1.name, "OUT");
    let reg5 = lib.component("REG-5.0").unwrap();
    assert!(lib.component("REG-3.3").is_some());
    assert_eq!(reg5.pads_for(1, "GND"), vec!["1".to_string(), "5".to_string()]);
    assert_eq!(reg5.pads_for(1, "OUT@1"), vec!["6".to_string()]);
}

#[test]
fn supply_symbol_detection() {
    let (lib, _) = parse_library(TEST_LBR, "Test").unwrap();
    assert!(lib.symbol("GND").unwrap().is_power);
    assert!(!lib.symbol("R").unwrap().is_power);
    // REG has `sup` pins but also a package.
    assert!(!lib.symbol("REG*").unwrap().is_power);
    let gnd = lib.symbol("GND").unwrap().unit_pins(1).next().unwrap();
    assert_eq!(gnd.electrical, PinType::PowerOut);
    assert_eq!(gnd.rotation, 270.0);
    assert_eq!(gnd.pos, Point::new(0, mm(2.54)));
    assert_eq!(gnd.body_end(), Point::new(0, 0));
}

#[test]
fn footprints_convert_pads_and_graphics() {
    let (lib, report) = parse_library(TEST_LBR, "Test").unwrap();
    let chip = lib.footprint("0603").unwrap();
    let pads: Vec<_> = chip.items.iter().filter_map(|i| if let Item::Pad(p) = i { Some(p) } else { None }).collect();
    assert_eq!(pads.len(), 2);
    assert_eq!(pads[0].shape, PadShape::RoundRect);
    assert_eq!(pads[0].corner_ratio, 0.25);
    assert_eq!(pads[0].layer, layer::TOP_COPPER);
    assert_eq!(pads[1].rotation, 180.0);
    assert!(chip.items.iter().any(|i| matches!(i, Item::Text(t) if t.text == pcbplannercad_core::item::DESIGNATOR_TOKEN && t.layer == layer::TOP_SILK)));
    assert!(chip.items.iter().any(|i| matches!(i, Item::Text(t) if t.text == pcbplannercad_core::item::VALUE_TOKEN)));
    let cy = chip.items.iter().find_map(|i| if let Item::Rect(r) = i { (r.layer == layer::TOP_COURTYARD).then_some(r) } else { None }).unwrap();
    // Silk line spans x ±1.5 (+0.1 half width) → courtyard at ±1.85.
    assert_eq!(cy.a.x, mm(-1.85));
    assert_eq!(cy.b.x, mm(1.85));

    let dil = lib.footprint("DIL8").unwrap();
    let pads: Vec<_> = dil.items.iter().filter_map(|i| if let Item::Pad(p) = i { Some(p) } else { None }).collect();
    assert_eq!(pads.len(), 9);
    assert_eq!((pads[0].shape, pads[0].width, pads[0].height, pads[0].rotation), (PadShape::Oval, mm(2.6), mm(1.3), 90.0));
    assert_eq!(pads[1].shape, PadShape::RoundRect);
    assert_eq!((pads[2].shape, pads[2].width), (PadShape::Circle, mm(1.3)));
    assert_eq!((pads[3].shape, pads[3].width), (PadShape::Rect, mm(1.6)));
    assert_eq!(pads[8].kind, pcbplannercad_core::item::PadKind::Npth);
    assert!(pads[8].number.is_empty());
    for p in &pads {
        assert!(Item::Pad((*p).clone()).validate().is_ok());
    }
    let arc = dil.items.iter().find_map(|i| if let Item::Arc(a) = i { Some(a) } else { None }).unwrap();
    assert_eq!(arc.sweep, -90.0);
    let (s, e) = (arc.start(), arc.end());
    assert!((s.x - mm(-5.0)).abs() <= 2 && (s.y - mm(-2.0)).abs() <= 2, "{s:?}");
    assert!((e.x - mm(5.0)).abs() <= 2 && (e.y - mm(-2.0)).abs() <= 2, "{e:?}");
    // Layer 35 (tGlue) is not mapped and gets reported.
    assert!(report.messages.iter().any(|m| m.text.contains("layer 35")));
}

#[test]
fn bad_input_is_an_error_not_a_panic() {
    assert!(parse_library("", "X").is_err());
    assert!(parse_library("<eagle><drawing/>", "X").is_err());
    assert!(parse_library("<foo/>", "X").is_err());
    assert!(parse_library("<eagle/>", "X").is_err());
    assert!(parse_schematic("<eagle><drawing><library/></drawing></eagle>", "x").is_err());
    // Junk attributes are tolerated.
    let junk = r#"<eagle><drawing><library><packages><package name="P"><smd name="1" x="NaN" y="1e999" dx="-1" dy="abc" layer="x"/>
        <pad name="2" x="0" y="0" drill="-1"/><wire x1="0" y1="0" x2="0" y2="0" curve="90" layer="21"/></package></packages>
        <symbols><symbol name="S"><pin name="1" rot="Rxyz" length="huge"/></symbol></symbols>
        <devicesets><deviceset name="D"><gates><gate name="G" symbol="S"/><gate name="H" symbol="MISSING"/></gates>
        <devices><device name="" package="NOPE"><connects><connect gate="G" pin="1" pad=""/></connects></device></devices>
        </deviceset></devicesets></library></drawing></eagle>"#;
    let (lib, report) = parse_library(junk, "J").unwrap();
    assert_eq!(lib.symbols.len(), 1);
    assert!(!report.messages.is_empty());
}

/// Two sheets; net SIG has one segment on each (no labels) → synthesized labels.
const TEST_SCH_TEMPLATE: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<eagle version="9.6.2"><drawing><schematic>
<libraries>LIB</libraries>
<parts>
<part name="R1" library="Test" deviceset="R" device="0603" value="10k"/>
<part name="R2" library="Test" deviceset="R" device="0603"/>
<part name="IC1" library="Test" deviceset="LM358" device="N"/>
<part name="GND1" library="Test" deviceset="GND" device=""/>
</parts>
<sheets>
<sheet>
<plain><text x="0" y="50" size="2" layer="97">Title</text><frame x1="0" y1="0" x2="100" y2="80" columns="4" rows="4" layer="94"/></plain>
<instances>
<instance part="R1" gate="G$1" x="20" y="20" rot="R90"/>
<instance part="GND1" gate="1" x="20" y="10"/>
<instance part="IC1" gate="A" x="50" y="20" rot="MR0" smashed="yes">
<attribute name="NAME" x="45" y="25" size="1.778" layer="95" rot="MR0"/>
</instance>
</instances>
<nets>
<net name="SIG">
<segment>
<wire x1="20" y1="25.08" x2="20" y2="30" width="0.1524" layer="91"/>
<wire x1="20" y1="30" x2="30" y2="30" width="0.1524" layer="91"/>
<pinref part="R1" gate="G$1" pin="2"/>
</segment>
</net>
<net name="GND">
<segment>
<wire x1="20" y1="14.92" x2="20" y2="12.54" width="0.1524" layer="91"/>
<pinref part="R1" gate="G$1" pin="1"/>
<pinref part="GND1" gate="1" pin="GND"/>
</segment>
</net>
<net name="N$1">
<segment>
<wire x1="42.38" y1="20" x2="40" y2="20" width="0.1524" layer="91"/>
<pinref part="IC1" gate="A" pin="OUT"/>
<label x="41" y="21" size="1.778" layer="95"/>
</segment>
</net>
</nets>
</sheet>
<sheet>
<instances>
<instance part="R2" gate="G$1" x="10" y="10"/>
</instances>
<nets>
<net name="SIG">
<segment>
<wire x1="4.92" y1="10" x2="0" y2="10" width="0.1524" layer="91"/>
<pinref part="R2" gate="G$1" pin="1"/>
</segment>
</net>
</nets>
</sheet>
</sheets>
</schematic></drawing></eagle>
"#;

fn test_schematic() -> String {
    let start = TEST_LBR.find("<library>").unwrap();
    let end = TEST_LBR.find("</library>").unwrap() + "</library>".len();
    let lib = TEST_LBR[start..end].replacen("<library>", r#"<library name="Test">"#, 1);
    TEST_SCH_TEMPLATE.replace("LIB", &lib)
}

fn items(doc: &Document) -> Vec<&Item> {
    doc.items().map(|(_, i)| i).collect()
}

#[test]
fn schematic_sheets_parts_ports_and_labels() {
    let imp = parse_schematic(&test_schematic(), "demo").unwrap();
    assert_eq!(imp.sheets.len(), 2);
    assert_eq!(imp.sheets[0].file_name, "demo_sheet1.arsch");
    assert_eq!(imp.sheets[1].file_name, "demo_sheet2.arsch");
    assert_eq!(imp.libraries.len(), 1);
    let s1 = &imp.sheets[0].document;
    let all = items(s1);

    // R1 at R90: pin 2 (local 5.08, 0) lands at (20, 25.08).
    let r1 = all.iter().find_map(|i| if let Item::Part(p) = i { (p.designator() == "R1").then_some(p) } else { None }).unwrap();
    assert_eq!(r1.lib_ref, "Test/R0603");
    assert_eq!(r1.field_value(FIELD_VALUE), "10k");
    let cached = s1.cached_part(&r1.lib_ref).unwrap();
    let world: Vec<Point> = cached.symbol.unit_pins(1).map(|p| r1.placement().apply(p.pos)).collect();
    assert!(world.contains(&Point::new(mm(20.0), mm(25.08))));
    assert!(world.contains(&Point::new(mm(20.0), mm(14.92))));
    // Unsmashed: the >NAME template (-3.81, 1.4) rotated by 90°.
    let name = r1.field(FIELD_DESIGNATOR).unwrap();
    assert_eq!(name.offset, Point::new(mm(-1.4), mm(-3.81)));
    assert_eq!(name.rotation, 90.0);
    assert!(name.visible);

    // Mirrored, smashed op-amp: OUT (local 7.62, 0) → (42.38, 20); NAME keeps its absolute spot.
    let ic = all.iter().find_map(|i| if let Item::Part(p) = i { (p.designator() == "IC1").then_some(p) } else { None }).unwrap();
    assert!(ic.mirror);
    assert_eq!(ic.unit, 1);
    let cached = s1.cached_part(&ic.lib_ref).unwrap();
    let out = cached.symbol.unit_pins(1).find(|p| p.name == "OUT").unwrap();
    assert_eq!(ic.placement().apply(out.pos), Point::new(mm(42.38), mm(20.0)));
    let f = ic.field(FIELD_DESIGNATOR).unwrap();
    assert_eq!(f.offset, Point::new(mm(-5.0), mm(5.0)));
    assert_eq!(f.h_align, HAlign::Right);

    // Supply symbol → power port at the pin, pointing down.
    let port = all.iter().find_map(|i| if let Item::PowerPort(p) = i { Some(p) } else { None }).unwrap();
    assert_eq!(port.net, "GND");
    assert_eq!(port.pos, Point::new(mm(20.0), mm(12.54)));
    assert_eq!(port.rotation, 270.0);
    assert_eq!(port.style, PowerStyle::Ground);
    assert!(s1.cache().keys().all(|k| !k.ends_with("/GND")));

    // N$1 label sat beside its wire and is snapped onto it.
    let labels: Vec<_> = all.iter().filter_map(|i| if let Item::NetLabel(l) = i { Some(l) } else { None }).collect();
    let n1 = labels.iter().find(|l| l.text == "N$1").unwrap();
    assert_eq!(n1.pos, Point::new(mm(40.0), mm(20.0)), "snapped onto the wire, then moved to its free end");
    assert_eq!(n1.scope, LabelScope::Global);

    // SIG spans two sheets: each segment gets a label at its dangling end.
    let sig = labels.iter().find(|l| l.text == "SIG").unwrap();
    assert_eq!(sig.pos, Point::new(mm(30.0), mm(30.0)));
    assert_eq!(sig.rotation, 0.0);
    let s2 = items(&imp.sheets[1].document);
    let sig2 = s2.iter().find_map(|i| if let Item::NetLabel(l) = i { Some(l) } else { None }).unwrap();
    assert_eq!((sig2.text.as_str(), sig2.pos, sig2.rotation), ("SIG", Point::new(0, mm(10.0)), 180.0));
    assert!(imp.report.messages.iter().any(|m| m.text.contains("2 global net label(s) synthesized")));
    // GND (single segment, has a port) needs none.
    assert!(!labels.iter().any(|l| l.text == "GND"));
    assert!(all.iter().any(|i| matches!(i, Item::Rect(_))), "frame");
    for i in all {
        assert!(i.validate().is_ok(), "{i:?}");
    }
}

// ---------- Real files ----------

fn examples() -> Option<PathBuf> {
    let p = std::env::var_os("PCAD_TEST_XML_EXAMPLES").map(PathBuf::from)?;
    if p.is_dir() {
        Some(p)
    } else {
        println!("note: XML fixtures are unavailable; skipping");
        None
    }
}

fn find_files(dir: &Path, ext: &str, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            find_files(&p, ext, out);
        } else if p.extension().is_some_and(|e| e.eq_ignore_ascii_case(ext)) {
            out.push(p);
        }
    }
}

#[test]
fn parses_all_example_libraries() {
    let Some(root) = examples() else { return };
    let mut files = Vec::new();
    find_files(&root.join("libraries"), "lbr", &mut files);
    let (mut syms, mut fps, mut comps, mut failures) = (0, 0, 0, Vec::new());
    for f in &files {
        let text = std::fs::read_to_string(f).unwrap();
        let name = f.file_stem().unwrap().to_string_lossy().into_owned();
        match parse_library(&text, &name) {
            Ok((lib, report)) => {
                println!(
                    "{name}: {} symbols, {} footprints, {} components, {} report message(s)",
                    lib.symbols.len(),
                    lib.footprints.len(),
                    lib.components.len(),
                    report.messages.len()
                );
                syms += lib.symbols.len();
                fps += lib.footprints.len();
                comps += lib.components.len();
                for s in &lib.symbols {
                    assert!(!s.units.is_empty());
                    for u in &s.units {
                        for i in &u.items {
                            assert!(i.validate().is_ok(), "{name}/{}: {i:?}", s.name);
                        }
                    }
                    let mut nums: Vec<&str> = s.all_pins().map(|(_, p)| p.number.as_str()).collect();
                    let n = nums.len();
                    nums.sort_unstable();
                    nums.dedup();
                    assert_eq!(nums.len(), n, "{name}/{}: duplicate pin numbers", s.name);
                }
                for fp in &lib.footprints {
                    for i in &fp.items {
                        assert!(i.validate().is_ok(), "{name}/{}: {i:?}", fp.name);
                    }
                }
                for c in &lib.components {
                    assert!(lib.cached_part(&c.name).is_some(), "{name}/{}", c.name);
                }
            }
            Err(e) => failures.push(format!("{name}: {e}")),
        }
    }
    println!("{} libraries: {syms} symbols, {fps} footprints, {comps} components, {} failure(s)", files.len(), failures.len());
    assert!(failures.is_empty(), "{failures:?}");
    assert!(files.len() >= 39, "expected the 39 example libraries, found {}", files.len());
}

struct Connectivity {
    endpoints: usize,
    connected: usize,
    pinrefs: usize,
    pinrefs_ok: usize,
}

fn sheet_connectivity(doc: &Document, sheet: roxmltree::Node<'_, '_>) -> Connectivity {
    let mut pins: Vec<Point> = Vec::new();
    let mut part_pins: HashMap<(String, String), Vec<(String, Point)>> = HashMap::new();
    let (mut ports, mut junctions, mut labels, mut wires) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for (_, item) in doc.items() {
        match item {
            Item::Part(p) => {
                let cached = doc.cached_part(&p.lib_ref).expect("cached part");
                let unit_name = cached.symbol.units[p.unit as usize - 1].name.clone();
                for pin in cached.symbol.unit_pins(p.unit) {
                    let w = p.placement().apply(pin.pos);
                    pins.push(w);
                    part_pins.entry((p.designator().to_string(), unit_name.clone())).or_default().push((pin.name.clone(), w));
                }
            }
            Item::PowerPort(p) => ports.push(p.pos),
            Item::Junction(j) => junctions.push(j.pos),
            Item::NetLabel(l) => labels.push(l.pos),
            Item::Wire(w) => wires.push((w.a, w.b)),
            _ => {}
        }
    }
    let touches_wire = |p: Point, skip: Option<usize>| wires.iter().enumerate().any(|(j, &(a, b))| Some(j) != skip && on_segment(p, a, b));
    let mut c = Connectivity { endpoints: 0, connected: 0, pinrefs: 0, pinrefs_ok: 0 };
    for (i, &(a, b)) in wires.iter().enumerate() {
        for e in [a, b] {
            c.endpoints += 1;
            if pins.contains(&e) || ports.contains(&e) || junctions.contains(&e) || labels.contains(&e) || touches_wire(e, Some(i)) {
                c.connected += 1;
            }
        }
    }
    // Every XML pinref must land on a wire, a port or another pin.
    for pr in sheet.descendants().filter(|n| n.has_tag_name("pinref")) {
        let key = (pr.attribute("part").unwrap_or("").to_string(), pr.attribute("gate").unwrap_or("").to_string());
        let Some(list) = part_pins.get(&key) else { continue }; // supply parts are ports
        c.pinrefs += 1;
        let pin = super::util::strip_at(pr.attribute("pin").unwrap_or(""));
        let ok = list.iter().filter(|(n, _)| n == pin).any(|&(_, w)| {
            touches_wire(w, None) || ports.contains(&w) || pins.iter().filter(|p| **p == w).count() > 1
        });
        if ok {
            c.pinrefs_ok += 1;
        }
    }
    c
}

#[test]
fn example_schematics_keep_connectivity() {
    let Some(root) = examples() else { return };
    let mut files = Vec::new();
    find_files(&root, "sch", &mut files);
    assert!(!files.is_empty(), "no schematic fixtures found");
    for f in files {
        let imp = import_schematic(&f).unwrap();
        let text = std::fs::read_to_string(&f).unwrap();
        let xml = super::util::parse_xml(&text).unwrap();
        let sheets: Vec<_> = xml.descendants().filter(|n| n.has_tag_name("sheet")).collect();
        assert_eq!(sheets.len(), imp.sheets.len());
        let (mut ep, mut ok, mut pr, mut pr_ok, mut parts, mut ports) = (0, 0, 0, 0, 0, 0);
        for (s, node) in imp.sheets.iter().zip(&sheets) {
            let c = sheet_connectivity(&s.document, *node);
            ep += c.endpoints;
            ok += c.connected;
            pr += c.pinrefs;
            pr_ok += c.pinrefs_ok;
            parts += s.document.items().filter(|(_, i)| matches!(i, Item::Part(_))).count();
            ports += s.document.items().filter(|(_, i)| matches!(i, Item::PowerPort(_))).count();
            for (_, i) in s.document.items() {
                assert!(i.validate().is_ok(), "{i:?}");
            }
        }
        let pct = 100.0 * ok as f64 / ep.max(1) as f64;
        let pr_pct = 100.0 * pr_ok as f64 / pr.max(1) as f64;
        println!(
            "{}: {} sheet(s), {parts} parts, {ports} power ports, {} libraries; wire endpoints connected {ok}/{ep} ({pct:.2}%), pinrefs on a wire/pin/port {pr_ok}/{pr} ({pr_pct:.2}%)",
            f.display(),
            imp.sheets.len(),
            imp.libraries.len()
        );
        for m in &imp.report.messages {
            println!("  {m}");
        }
        assert!(pct >= 98.0, "{}: only {pct:.2}% of wire endpoints connected", f.display());
        assert!(pr_pct >= 98.0, "{}: only {pr_pct:.2}% of pinrefs reproduced", f.display());
    }
}
