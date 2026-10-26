//! Component libraries. A *component* is what a designer places: it ties a
//! symbol (schematic view) to footprints (board view), a 3D model and
//! parameters. Symbols and footprints are stored once and shared.

use serde::{Deserialize, Serialize};

use crate::item::{Field, Item, Pin};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SymbolUnit {
    /// Optional display name ("A", "Power", …); empty means use the letter.
    #[serde(default)]
    pub name: String,
    pub items: Vec<Item>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Symbol {
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Designator prefix, e.g. "R", "U".
    #[serde(default = "default_prefix")]
    pub prefix: String,
    /// At least one unit. Items common to all units are duplicated into each.
    pub units: Vec<SymbolUnit>,
    #[serde(default = "default_true")]
    pub show_pin_numbers: bool,
    #[serde(default = "default_true")]
    pub show_pin_names: bool,
    /// Distance of pin names inside the body from the pin's body end;
    /// 0 places names outside, above the pin line.
    #[serde(default = "default_pin_name_offset")]
    pub pin_name_offset: crate::units::Coord,
    /// Power symbols (GND, VCC…) become power ports on import.
    #[serde(default)]
    pub is_power: bool,
    /// Field templates (Designator, Value, …) with offsets relative to the
    /// symbol origin; copied onto parts when placed.
    #[serde(default)]
    pub fields: Vec<Field>,
}

fn default_prefix() -> String {
    "U".into()
}

fn default_true() -> bool {
    true
}

fn default_pin_name_offset() -> crate::units::Coord {
    crate::units::mm(0.508)
}

impl Symbol {
    pub fn new(name: impl Into<String>, prefix: impl Into<String>) -> Self {
        Symbol {
            name: name.into(),
            description: String::new(),
            prefix: prefix.into(),
            units: vec![SymbolUnit::default()],
            show_pin_numbers: true,
            show_pin_names: true,
            pin_name_offset: default_pin_name_offset(),
            is_power: false,
            fields: Vec::new(),
        }
    }

    /// Flag symbols (`PWR_FLAG`) are power symbols that only mark a
    /// net as driven; they don't name or join nets.
    pub fn is_power_flag(&self) -> bool {
        self.is_power && self.name.eq_ignore_ascii_case("PWR_FLAG")
    }

    /// Power symbols (GND, VCC…) connect every pin to the global net named
    /// by the part's Value.
    pub fn names_global_net(&self) -> bool {
        self.is_power && !self.is_power_flag()
    }

    /// Pins of a 1-based unit.
    pub fn unit_pins(&self, unit: u32) -> impl Iterator<Item = &Pin> {
        self.units
            .get(unit.saturating_sub(1) as usize)
            .into_iter()
            .flat_map(|u| u.items.iter())
            .filter_map(|i| if let Item::Pin(p) = i { Some(p) } else { None })
    }

    pub fn all_pins(&self) -> impl Iterator<Item = (u32, &Pin)> {
        self.units.iter().enumerate().flat_map(|(i, u)| {
            u.items.iter().filter_map(move |it| if let Item::Pin(p) = it { Some((i as u32 + 1, p)) } else { None })
        })
    }
}

/// Reference to an external 3D model, positioned relative to the footprint.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Model3d {
    pub path: String,
    /// Millimeters.
    #[serde(default)]
    pub offset: [f64; 3],
    /// Degrees about X, Y, Z.
    #[serde(default)]
    pub rotation: [f64; 3],
    #[serde(default = "unit_scale")]
    pub scale: [f64; 3],
}

fn unit_scale() -> [f64; 3] {
    [1.0; 3]
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Footprint {
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Pads plus graphics on silk, courtyard, assembly, mask, paste layers.
    pub items: Vec<Item>,
    #[serde(default)]
    pub model: Option<Model3d>,
}

impl Footprint {
    pub fn new(name: impl Into<String>) -> Self {
        Footprint { name: name.into(), description: String::new(), items: Vec::new(), model: None }
    }
}

/// Explicit symbol-pin → pad mapping for one pin. Components without a
/// pin map connect pins to the pads with the same number.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PinMapping {
    pub unit: u32,
    /// The symbol pin's number.
    pub pin: String,
    pub pads: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Parameter {
    pub name: String,
    pub value: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Component {
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Symbol name in the same library.
    pub symbol: String,
    /// Footprint names; `Lib:Name` refers to another library. First is default.
    #[serde(default)]
    pub footprints: Vec<String>,
    #[serde(default)]
    pub pin_map: Vec<PinMapping>,
    /// Includes "Value" and anything else (Manufacturer, MPN, Datasheet…).
    #[serde(default)]
    pub parameters: Vec<Parameter>,
}

impl Component {
    pub fn parameter(&self, name: &str) -> Option<&str> {
        self.parameters.iter().find(|p| p.name.eq_ignore_ascii_case(name)).map(|p| p.value.as_str())
    }

    /// Pads a symbol pin connects to.
    pub fn pads_for(&self, unit: u32, pin_number: &str) -> Vec<String> {
        if self.pin_map.is_empty() {
            return vec![pin_number.to_string()];
        }
        self.pin_map
            .iter()
            .find(|m| m.unit == unit && m.pin == pin_number)
            .map(|m| m.pads.clone())
            .unwrap_or_default()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Library {
    pub name: String,
    #[serde(default)]
    pub symbols: Vec<Symbol>,
    #[serde(default)]
    pub footprints: Vec<Footprint>,
    #[serde(default)]
    pub components: Vec<Component>,
}

impl Library {
    pub fn symbol(&self, name: &str) -> Option<&Symbol> {
        self.symbols.iter().find(|s| s.name == name)
    }

    pub fn footprint(&self, name: &str) -> Option<&Footprint> {
        self.footprints.iter().find(|f| f.name == name)
    }

    pub fn component(&self, name: &str) -> Option<&Component> {
        self.components.iter().find(|c| c.name == name)
    }

    /// Snapshot of a component with its symbol, ready to cache in a schematic.
    pub fn cached_part(&self, component: &str) -> Option<CachedPart> {
        let component = self.component(component)?.clone();
        let symbol = self.symbol(&component.symbol)?.clone();
        Some(CachedPart { library: self.name.clone(), component, symbol })
    }

    /// Adds or replaces symbols/footprints/components by name.
    pub fn merge(&mut self, other: Library) {
        fn upsert<T>(dst: &mut Vec<T>, src: Vec<T>, name: impl Fn(&T) -> &str) {
            for item in src {
                match dst.iter().position(|d| name(d) == name(&item)) {
                    Some(i) => dst[i] = item,
                    None => dst.push(item),
                }
            }
        }
        upsert(&mut self.symbols, other.symbols, |s| &s.name);
        upsert(&mut self.footprints, other.footprints, |f| &f.name);
        upsert(&mut self.components, other.components, |c| &c.name);
    }
}

/// The library data a schematic keeps for each placed component, so the
/// design opens identically even if the source library changes or is gone.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CachedPart {
    pub library: String,
    pub component: Component,
    pub symbol: Symbol,
}

impl CachedPart {
    /// Key used by [`crate::item::Part::lib_ref`].
    pub fn key(&self) -> String {
        format!("{}/{}", self.library, self.component.name)
    }
}
