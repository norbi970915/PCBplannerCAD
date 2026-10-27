//! Board-level settings: layer stack, design rules and net classes. Stored as
//! the singleton `Item::Board` of a PCB document so edits are undoable.

use serde::{Deserialize, Serialize};

use crate::document::LayerId;
use crate::layer;
use crate::units::{Coord, Point, mm};

use super::items::ZoneConnect;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StackKind {
    Copper,
    Dielectric,
    SolderMask,
    Silkscreen,
    Paste,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StackLayer {
    pub name: String,
    pub kind: StackKind,
    /// Drawing layer for copper/mask/silk/paste entries.
    #[serde(default)]
    pub layer: Option<LayerId>,
    pub thickness: Coord,
    #[serde(default)]
    pub material: String,
    /// Relative permittivity (dielectrics), for impedance calculation.
    #[serde(default)]
    pub epsilon_r: f64,
}

/// Physical layer stack, listed top to bottom.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Stackup {
    pub layers: Vec<StackLayer>,
}

impl Stackup {
    /// Standard FR-4 stack with `copper` copper layers (2, 4, 6 … 32), 1.6 mm total.
    pub fn with_copper(copper: u16) -> Stackup {
        let copper = copper.clamp(1, 32);
        let cu = mm(0.035);
        let mask = mm(0.01);
        let total = mm(1.6);
        let gaps = copper.saturating_sub(1).max(1) as i64;
        let dielectric = (total - cu * copper as i64 - 2 * mask) / gaps;
        let mut layers = vec![
            StackLayer { name: "Top Overlay".into(), kind: StackKind::Silkscreen, layer: Some(layer::TOP_SILK), thickness: 0, material: String::new(), epsilon_r: 0.0 },
            StackLayer { name: "Top Paste".into(), kind: StackKind::Paste, layer: Some(layer::TOP_PASTE), thickness: 0, material: String::new(), epsilon_r: 0.0 },
            StackLayer { name: "Top Solder".into(), kind: StackKind::SolderMask, layer: Some(layer::TOP_MASK), thickness: mask, material: "Solder Resist".into(), epsilon_r: 3.5 },
        ];
        for i in 0..copper {
            let (name, id) = if i == 0 {
                ("Top Layer".to_string(), layer::TOP_COPPER)
            } else if i == copper - 1 {
                ("Bottom Layer".to_string(), layer::BOTTOM_COPPER)
            } else {
                (format!("Inner {i}"), layer::inner_copper(i))
            };
            layers.push(StackLayer { name, kind: StackKind::Copper, layer: Some(id), thickness: cu, material: "Copper".into(), epsilon_r: 0.0 });
            if i + 1 < copper {
                let material = if i % 2 == 0 { "FR-4 Core" } else { "FR-4 Prepreg" };
                layers.push(StackLayer { name: format!("Dielectric {}", i + 1), kind: StackKind::Dielectric, layer: None, thickness: dielectric, material: material.into(), epsilon_r: 4.2 });
            }
        }
        layers.extend([
            StackLayer { name: "Bottom Solder".into(), kind: StackKind::SolderMask, layer: Some(layer::BOTTOM_MASK), thickness: mask, material: "Solder Resist".into(), epsilon_r: 3.5 },
            StackLayer { name: "Bottom Paste".into(), kind: StackKind::Paste, layer: Some(layer::BOTTOM_PASTE), thickness: 0, material: String::new(), epsilon_r: 0.0 },
            StackLayer { name: "Bottom Overlay".into(), kind: StackKind::Silkscreen, layer: Some(layer::BOTTOM_SILK), thickness: 0, material: String::new(), epsilon_r: 0.0 },
        ]);
        Stackup { layers }
    }

    /// Copper layer ids, top to bottom.
    pub fn copper_layers(&self) -> Vec<LayerId> {
        self.layers.iter().filter(|l| l.kind == StackKind::Copper).filter_map(|l| l.layer).collect()
    }

    pub fn is_copper(&self, id: LayerId) -> bool {
        self.copper_layers().contains(&id)
    }

    /// Copper layers a via from `a` to `b` passes through (inclusive).
    pub fn span(&self, a: LayerId, b: LayerId) -> Vec<LayerId> {
        let cu = self.copper_layers();
        let (Some(i), Some(j)) = (cu.iter().position(|&l| l == a), cu.iter().position(|&l| l == b)) else { return cu };
        let (lo, hi) = if i <= j { (i, j) } else { (j, i) };
        cu[lo..=hi].to_vec()
    }

    pub fn thickness(&self) -> Coord {
        self.layers.iter().map(|l| l.thickness).sum()
    }

    pub fn layer_name(&self, id: LayerId) -> Option<&str> {
        self.layers.iter().find(|l| l.layer == Some(id)).map(|l| l.name.as_str())
    }

    /// Through, blind, buried or micro, from the copper span and drill.
    pub fn via_kind(&self, from: LayerId, to: LayerId, drill: Coord) -> ViaKind {
        let cu = self.copper_layers();
        let span = self.span(from, to);
        let outer = |l: LayerId| cu.first() == Some(&l) || cu.last() == Some(&l);
        if span.len() == cu.len() {
            ViaKind::Through
        } else if span.len() == 2 && drill <= MICROVIA_MAX_DRILL {
            ViaKind::Micro
        } else if outer(from) || outer(to) {
            ViaKind::Blind
        } else {
            ViaKind::Buried
        }
    }
}

/// Laser-drilled microvias span one dielectric with a drill up to this size.
pub const MICROVIA_MAX_DRILL: Coord = 150_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ViaKind {
    Through,
    Blind,
    Buried,
    Micro,
}

/// A via the board may use: its copper span and pad/drill size. Routing picks
/// the type that matches a layer change; DRC accepts vias that match a type exactly.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ViaType {
    pub name: String,
    pub from_layer: LayerId,
    pub to_layer: LayerId,
    pub diameter: Coord,
    pub drill: Coord,
}

impl BoardSettings {
    /// The defined via type a placed via matches (same span, diameter and drill).
    pub fn via_type_of(&self, from: LayerId, to: LayerId, diameter: Coord, drill: Coord) -> Option<&ViaType> {
        let span = self.stackup.span(from, to);
        self.via_types.iter().find(|t| t.diameter == diameter && t.drill == drill && self.stackup.span(t.from_layer, t.to_layer) == span)
    }

    /// Smallest defined via type that connects copper layers `a` and `b`.
    pub fn via_type_between(&self, a: LayerId, b: LayerId) -> Option<&ViaType> {
        self.via_types
            .iter()
            .filter(|t| {
                let span = self.stackup.span(t.from_layer, t.to_layer);
                span.contains(&a) && span.contains(&b)
            })
            .min_by_key(|t| self.stackup.span(t.from_layer, t.to_layer).len())
    }
}

impl Default for Stackup {
    fn default() -> Self {
        Stackup::with_copper(2)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetClass {
    pub name: String,
    #[serde(default)]
    pub nets: Vec<String>,
    pub track_width: Coord,
    pub clearance: Coord,
    pub via_diameter: Coord,
    pub via_drill: Coord,
    #[serde(default)]
    pub diff_pair_width: Coord,
    #[serde(default)]
    pub diff_pair_gap: Coord,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffPair {
    pub name: String,
    pub positive: String,
    pub negative: String,
}

/// A design rule. `scope`/`first`/`second` are query expressions such as
/// `All`, `InNet('GND')`, `InNetClass('Power') and OnLayer('Top Layer')`.
/// Rules are evaluated in list order; the first matching rule of a kind wins.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum RuleKind {
    #[serde(rename_all = "camelCase")]
    Clearance { first: String, second: String, min: Coord },
    #[serde(rename_all = "camelCase")]
    Width { scope: String, min: Coord, preferred: Coord, max: Coord },
    #[serde(rename_all = "camelCase")]
    ViaStyle { scope: String, diameter: Coord, drill: Coord, min_annular_ring: Coord },
    #[serde(rename_all = "camelCase")]
    HoleSize { scope: String, min: Coord, max: Coord },
    #[serde(rename_all = "camelCase")]
    BoardOutlineClearance { min: Coord },
    /// Courtyard-to-courtyard clearance between footprints (0 = no overlap).
    #[serde(rename_all = "camelCase")]
    ComponentClearance { min: Coord },
    #[serde(rename_all = "camelCase")]
    SolderMaskExpansion { scope: String, expansion: Coord },
    #[serde(rename_all = "camelCase")]
    PasteMaskExpansion { scope: String, expansion: Coord },
    #[serde(rename_all = "camelCase")]
    ZoneConnect { scope: String, style: ZoneConnect, gap: Coord, spoke_width: Coord },
    #[serde(rename_all = "camelCase")]
    DiffPair { scope: String, width: Coord, gap: Coord },
    #[serde(rename_all = "camelCase")]
    Length { scope: String, min: Coord, max: Coord },
    /// Silkscreen to exposed copper (pads) clearance.
    #[serde(rename_all = "camelCase")]
    SilkClearance { min: Coord },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rule {
    pub name: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(flatten)]
    pub kind: RuleKind,
}

fn default_true() -> bool {
    true
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardSettings {
    #[serde(default)]
    pub stackup: Stackup,
    #[serde(default = "default_rules")]
    pub rules: Vec<Rule>,
    #[serde(default = "default_classes")]
    pub net_classes: Vec<NetClass>,
    #[serde(default)]
    pub diff_pairs: Vec<DiffPair>,
    /// User origin for coordinates and pick-and-place output.
    #[serde(default)]
    pub origin: Point,
    /// Solder mask color for the 3D view ("#1f5f2a" green by default).
    #[serde(default = "default_mask_color")]
    pub mask_color: String,
    #[serde(default = "default_silk_color")]
    pub silk_color: String,
    /// Through, blind, buried and micro via definitions (empty: through vias from the rules).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub via_types: Vec<ViaType>,
}

fn default_mask_color() -> String {
    "#1f5f2a".into()
}

fn default_silk_color() -> String {
    "#f2f2f2".into()
}

fn rule(name: &str, kind: RuleKind) -> Rule {
    Rule { name: name.into(), enabled: true, kind }
}

fn default_rules() -> Vec<Rule> {
    vec![
        rule("Clearance", RuleKind::Clearance { first: "All".into(), second: "All".into(), min: mm(0.2) }),
        rule("Width", RuleKind::Width { scope: "All".into(), min: mm(0.15), preferred: mm(0.25), max: mm(3.0) }),
        rule("RoutingVias", RuleKind::ViaStyle { scope: "All".into(), diameter: mm(0.6), drill: mm(0.3), min_annular_ring: mm(0.13) }),
        rule("HoleSize", RuleKind::HoleSize { scope: "All".into(), min: mm(0.2), max: mm(6.3) }),
        rule("BoardOutlineClearance", RuleKind::BoardOutlineClearance { min: mm(0.3) }),
        rule("ComponentClearance", RuleKind::ComponentClearance { min: 0 }),
        rule("SolderMaskExpansion", RuleKind::SolderMaskExpansion { scope: "All".into(), expansion: mm(0.05) }),
        rule("PasteMaskExpansion", RuleKind::PasteMaskExpansion { scope: "All".into(), expansion: 0 }),
        rule("PolygonConnect", RuleKind::ZoneConnect { scope: "All".into(), style: ZoneConnect::Thermal, gap: mm(0.3), spoke_width: mm(0.3) }),
        rule("DiffPairs", RuleKind::DiffPair { scope: "All".into(), width: mm(0.2), gap: mm(0.2) }),
        rule("SilkToPad", RuleKind::SilkClearance { min: mm(0.1) }),
    ]
}

fn default_classes() -> Vec<NetClass> {
    vec![NetClass {
        name: "Default".into(),
        nets: vec![],
        track_width: mm(0.25),
        clearance: mm(0.2),
        via_diameter: mm(0.6),
        via_drill: mm(0.3),
        diff_pair_width: mm(0.2),
        diff_pair_gap: mm(0.2),
    }]
}

impl Default for BoardSettings {
    fn default() -> Self {
        BoardSettings {
            stackup: Stackup::default(),
            rules: default_rules(),
            net_classes: default_classes(),
            diff_pairs: vec![],
            origin: Point::default(),
            mask_color: default_mask_color(),
            silk_color: default_silk_color(),
            via_types: vec![],
        }
    }
}

impl BoardSettings {
    /// Net class a net belongs to (falls back to the first class, "Default").
    pub fn class_of(&self, net: &str) -> Option<&NetClass> {
        self.net_classes.iter().find(|c| c.nets.iter().any(|n| n == net)).or_else(|| self.net_classes.first())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stackups_have_ordered_copper_and_nominal_thickness() {
        let s = Stackup::with_copper(4);
        assert_eq!(s.copper_layers(), vec![layer::TOP_COPPER, layer::inner_copper(1), layer::inner_copper(2), layer::BOTTOM_COPPER]);
        assert!((s.thickness() - mm(1.6)).abs() < mm(0.01));
        assert_eq!(s.span(layer::inner_copper(1), layer::TOP_COPPER), vec![layer::TOP_COPPER, layer::inner_copper(1)]);
    }

    #[test]
    fn settings_round_trip_through_json() {
        let b = BoardSettings::default();
        let json = serde_json::to_string(&b).unwrap();
        assert!(json.contains("\"type\":\"clearance\""));
        assert_eq!(serde_json::from_str::<BoardSettings>(&json).unwrap(), b);
    }
}
