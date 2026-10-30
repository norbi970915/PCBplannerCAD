//! The built-in "PCBPlanner Basic" library: hand-drawn symbols, wizard
//! footprints and components tying them together. Everything is generated
//! here from first principles (no external library data).

use super::builder::{FpBuilder, Pin1Corner, silk_keepout};
use super::symbols;
use super::wizard::{
    AxialSpec, ChipSize, ChipSpec, CrystalPackage, CrystalSpec, DipSpec, DualRowSpec, FootprintSpec, PinHeaderSpec, RadialSpec, SodPackage, SodSpec, SotPackage,
    SotSpec, generate,
};
use crate::item::PadShape;
use crate::library::{Component, Footprint, Library, Parameter, PinMapping, Symbol};

pub const STARTER_LIBRARY_NAME: &str = "PCBPlanner Basic";

fn chip(prefix: &str, size: ChipSize, polarity: bool) -> FootprintSpec {
    FootprintSpec::Chip(ChipSpec { size, prefix: prefix.into(), polarity })
}

fn sod(pkg: SodPackage) -> FootprintSpec {
    FootprintSpec::Sod(SodSpec { package: pkg, prefix: "D".into() })
}

fn header(n: u32, pitch: f64) -> FootprintSpec {
    FootprintSpec::PinHeader(PinHeaderSpec { rows: 1, pins_per_row: n, pitch, ..Default::default() })
}

fn radial(d: f64, pitch: f64) -> FootprintSpec {
    FootprintSpec::Radial(RadialSpec { diameter: d, pitch, ..Default::default() })
}

/// 6 × 6 mm tactile switch, THT; pads with the same number are connected
/// inside the switch.
fn sw_push_6mm() -> Footprint {
    let mut b = FpBuilder::new("SW_PUSH_6mm", "6x6 mm through-hole tactile switch; equal pad numbers are internally connected");
    b.tht("1", -3.25, 2.25, 2.0, 2.0, PadShape::Rect, 1.1);
    b.tht("1", 3.25, 2.25, 2.0, 2.0, PadShape::Circle, 1.1);
    b.tht("2", -3.25, -2.25, 2.0, 2.0, PadShape::Circle, 1.1);
    b.tht("2", 3.25, -2.25, 2.0, 2.0, PadShape::Circle, 1.1);
    b.body(0.0, 0.0, 6.0, 6.0, Pin1Corner::TopLeft);
    b.silk_rect(-3.11, -3.11, 3.11, 3.11);
    b.silk_circle(0.0, 0.0, 1.75);
    b.build()
}

fn test_point_smd() -> Footprint {
    let mut b = FpBuilder::new("TestPoint_Pad_D1.5mm", "SMD test point, 1.5 mm round pad");
    b.smd_shape("1", 0.0, 0.0, 1.5, 1.5, PadShape::Circle, 0.0);
    b.body(0.0, 0.0, 1.5, 1.5, Pin1Corner::TopLeft);
    // Clear of the pad's bounding square, not just the round pad.
    b.silk_circle(0.0, 0.0, 0.75 * std::f64::consts::SQRT_2 + silk_keepout() + 0.01);
    b.build()
}

fn test_point_tht() -> Footprint {
    let mut b = FpBuilder::new("TestPoint_THTPad_D2.0mm_Drill1.0mm", "Through-hole test point, 2.0 mm pad, 1.0 mm drill");
    b.tht("1", 0.0, 0.0, 2.0, 2.0, PadShape::Circle, 1.0);
    b.body(0.0, 0.0, 2.0, 2.0, Pin1Corner::TopLeft);
    b.silk_circle(0.0, 0.0, std::f64::consts::SQRT_2 + silk_keepout() + 0.01);
    b.build()
}

fn component(name: &str, description: &str, symbol: &Symbol, footprints: &[&Footprint], value: &str) -> Component {
    Component {
        name: name.into(),
        description: description.into(),
        symbol: symbol.name.clone(),
        footprints: footprints.iter().map(|f| f.name.clone()).collect(),
        pin_map: Vec::new(),
        parameters: vec![Parameter { name: "Value".into(), value: value.into() }],
    }
}

struct Lib {
    lib: Library,
}

impl Lib {
    /// Generates (once) and returns the footprint for `spec`.
    fn fp(&mut self, spec: FootprintSpec) -> Footprint {
        let f = generate(&spec).expect("starter footprint spec is valid");
        if self.lib.footprint(&f.name).is_none() {
            self.lib.footprints.push(f.clone());
        }
        f
    }

    fn add_fp(&mut self, f: Footprint) -> Footprint {
        if self.lib.footprint(&f.name).is_none() {
            self.lib.footprints.push(f.clone());
        }
        f
    }

    fn sym(&mut self, s: Symbol) -> Symbol {
        self.lib.symbols.push(s.clone());
        s
    }

    fn comp(&mut self, c: Component) {
        self.lib.components.push(c);
    }
}

/// The built-in starter library.
pub fn starter_library() -> Library {
    use ChipSize::*;
    let mut l = Lib { lib: Library { name: STARTER_LIBRARY_NAME.into(), notices: vec!["Original PCBPlanner Studio starter assets. Copyright 2026 Buchmüller Norbert. You may use, modify and include these original assets in personal, educational and commercial designs, including designs you share or sell, without attribution or a requirement to publish your design. Redistribution of the starter library as a collection is permitted free of charge with this notice retained. Third-party assets imported separately keep their own terms.".into()], ..Default::default() } };

    // Passives.
    let r = l.sym(symbols::resistor());
    let fps: Vec<Footprint> = [I0603, I0402, I0805, I1206].iter().map(|&s| l.fp(chip("R", s, false))).collect();
    let r_ax = l.fp(FootprintSpec::Axial(AxialSpec::default()));
    let mut refs: Vec<&Footprint> = fps.iter().collect();
    refs.push(&r_ax);
    l.comp(component("Resistor", "Resistor", &r, &refs, "10k"));

    let c = l.sym(symbols::capacitor());
    let fps: Vec<Footprint> = [I0603, I0402, I0805, I1206].iter().map(|&s| l.fp(chip("C", s, false))).collect();
    l.comp(component("Capacitor", "Unpolarized capacitor (ceramic)", &c, &fps.iter().collect::<Vec<_>>(), "100n"));

    let cp = l.sym(symbols::capacitor_polarized());
    let fps = [l.fp(radial(5.0, 2.0)), l.fp(radial(6.3, 2.5)), l.fp(radial(8.0, 3.5)), l.fp(chip("CP", I1206, true)), l.fp(chip("CP", I1210, true))];
    l.comp(component("Capacitor_Polarized", "Polarized capacitor (electrolytic / tantalum), pin 1 = +", &cp, &fps.iter().collect::<Vec<_>>(), "10u"));

    let ind = l.sym(symbols::inductor());
    let fps: Vec<Footprint> = [I0603, I0805, I1206, I1210].iter().map(|&s| l.fp(chip("L", s, false))).collect();
    l.comp(component("Inductor", "Inductor", &ind, &fps.iter().collect::<Vec<_>>(), "10u"));

    let fb = l.sym(symbols::ferrite_bead());
    let fps: Vec<Footprint> = [I0603, I0805].iter().map(|&s| l.fp(chip("L", s, false))).collect();
    l.comp(component("Ferrite_Bead", "Ferrite bead", &fb, &fps.iter().collect::<Vec<_>>(), "600R@100MHz"));

    let fuse = l.sym(symbols::fuse());
    let fps: Vec<Footprint> = [I1206, I0603, I1812].iter().map(|&s| l.fp(chip("Fuse", s, false))).collect();
    l.comp(component("Fuse", "Fuse / PTC resettable fuse", &fuse, &fps.iter().collect::<Vec<_>>(), "500mA"));

    // Diodes (pin 1 = cathode = pad 1).
    let d = l.sym(symbols::diode());
    let d_ax = FootprintSpec::Axial(AxialSpec { prefix: "D".into(), body_length: 5.2, body_diameter: 2.7, polarity: true, ..Default::default() });
    let fps = [l.fp(sod(SodPackage::Sod123)), l.fp(sod(SodPackage::Sod323)), l.fp(sod(SodPackage::Sma)), l.fp(d_ax)];
    l.comp(component("Diode", "General-purpose diode, pin 1 = cathode", &d, &fps.iter().collect::<Vec<_>>(), "1N4148"));

    let dz = l.sym(symbols::zener());
    let fps = [l.fp(sod(SodPackage::Sod123)), l.fp(sod(SodPackage::Sod323)), l.fp(sod(SodPackage::Sma))];
    l.comp(component("Diode_Zener", "Zener diode, pin 1 = cathode", &dz, &fps.iter().collect::<Vec<_>>(), "5V1"));

    let ds = l.sym(symbols::schottky());
    let fps = [l.fp(sod(SodPackage::Sma)), l.fp(sod(SodPackage::Sod123)), l.fp(sod(SodPackage::Smb))];
    l.comp(component("Diode_Schottky", "Schottky diode, pin 1 = cathode", &ds, &fps.iter().collect::<Vec<_>>(), "SS14"));

    let led = l.sym(symbols::led());
    let fps = [l.fp(chip("LED", I0603, true)), l.fp(chip("LED", I0805, true))];
    l.comp(component("LED", "Light-emitting diode, pin 1 = cathode", &led, &fps.iter().collect::<Vec<_>>(), "LED"));

    // Transistors in SOT-23.
    let sot23 = l.fp(FootprintSpec::Sot(SotSpec { package: SotPackage::Sot23 }));
    for (sym, name, desc, value) in [
        (symbols::npn(), "Q_NPN", "NPN bipolar transistor, SOT-23 (1 = B, 2 = E, 3 = C)", "MMBT3904"),
        (symbols::pnp(), "Q_PNP", "PNP bipolar transistor, SOT-23 (1 = B, 2 = E, 3 = C)", "MMBT3906"),
        (symbols::nmos(), "Q_NMOS", "N-channel MOSFET, SOT-23 (1 = G, 2 = S, 3 = D)", "2N7002"),
        (symbols::pmos(), "Q_PMOS", "P-channel MOSFET, SOT-23 (1 = G, 2 = S, 3 = D)", "BSS84"),
    ] {
        let s = l.sym(sym);
        l.comp(component(name, desc, &s, &[&sot23], value));
    }

    // Crystals.
    let y = l.sym(symbols::crystal());
    let hc49 = l.fp(FootprintSpec::Crystal(CrystalSpec { package: CrystalPackage::Hc49 }));
    l.comp(component("Crystal", "Two-pin crystal, HC-49 through-hole", &y, &[&hc49], "16MHz"));
    let fps = [
        l.fp(FootprintSpec::Crystal(CrystalSpec { package: CrystalPackage::Smd3225 })),
        l.fp(FootprintSpec::Crystal(CrystalSpec { package: CrystalPackage::Smd2016 })),
    ];
    let mut xc = component("Crystal_SMD", "Crystal in a 4-pad SMD package (pads 1/3 = crystal, 2/4 = case, left open)", &y, &fps.iter().collect::<Vec<_>>(), "16MHz");
    xc.pin_map = vec![
        PinMapping { unit: 1, pin: "1".into(), pads: vec!["1".into()] },
        PinMapping { unit: 1, pin: "2".into(), pads: vec!["3".into()] },
    ];
    l.comp(xc);

    // Switch and test point.
    let sw = l.sym(symbols::push_button());
    let sw_fp = l.add_fp(sw_push_6mm());
    l.comp(component("Push_Button", "Push button, normally open", &sw, &[&sw_fp], "SW_Push"));

    let tp = l.sym(symbols::test_point());
    let fps = [l.add_fp(test_point_smd()), l.add_fp(test_point_tht())];
    l.comp(component("TestPoint", "Test point", &tp, &fps.iter().collect::<Vec<_>>(), "TP"));

    // ICs.
    let soic8 = l.fp(FootprintSpec::Soic(DualRowSpec { pins: Some(8), ..Default::default() }));
    let dip8 = l.fp(FootprintSpec::Dip(DipSpec::default()));
    let oa = l.sym(symbols::opamp());
    l.comp(component("Opamp", "Single operational amplifier (2 = -IN, 3 = +IN, 4 = V-, 6 = OUT, 7 = V+)", &oa, &[&soic8, &dip8], "TL071"));
    let oa2 = l.sym(symbols::opamp_dual());
    l.comp(component("Opamp_Dual", "Dual operational amplifier, LM358 pinout", &oa2, &[&soic8, &dip8], "LM358"));

    let reg = l.sym(symbols::regulator());
    let sot223 = l.fp(FootprintSpec::Sot(SotSpec { package: SotPackage::Sot223 }));
    l.comp(component("Regulator_3Pin", "Fixed linear regulator, SOT-223 (1 = GND, 2 = VOUT/tab, 3 = VIN)", &reg, &[&sot223], "AMS1117-3.3"));

    // Connectors.
    for n in 2..=10 {
        let s = l.sym(symbols::connector(n));
        let fps = [l.fp(header(n, 2.54)), l.fp(header(n, 2.0))];
        let name = s.name.clone();
        l.comp(component(&name, &format!("Single-row connector, {n} pins"), &s, &fps.iter().collect::<Vec<_>>(), &name));
    }

    l.lib
}
