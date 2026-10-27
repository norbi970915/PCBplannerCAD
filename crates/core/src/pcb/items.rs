//! Board-only items: placed footprints, copper tracks, vias, zones, and the
//! singleton board-settings item.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::document::LayerId;
use crate::geom::Placement;
use crate::layer;
use crate::units::{Coord, Point};

use super::board::BoardSettings;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Side {
    #[default]
    Top,
    Bottom,
}

impl Side {
    pub fn flipped(self) -> Side {
        match self {
            Side::Top => Side::Bottom,
            Side::Bottom => Side::Top,
        }
    }
}

/// A straight copper track segment.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Track {
    pub a: Point,
    pub b: Point,
    pub width: Coord,
    pub layer: LayerId,
    /// Net name; empty = unconnected copper.
    #[serde(default)]
    pub net: String,
}

/// A curved copper track (same convention as `Arc`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArcTrack {
    pub center: Point,
    pub radius: Coord,
    pub start_angle: f64,
    pub sweep: f64,
    pub width: Coord,
    pub layer: LayerId,
    #[serde(default)]
    pub net: String,
}

fn top() -> LayerId {
    layer::TOP_COPPER
}

fn bottom() -> LayerId {
    layer::BOTTOM_COPPER
}

/// A plated via spanning copper layers `from_layer`..`to_layer` (through vias
/// span top to bottom).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Via {
    pub pos: Point,
    pub diameter: Coord,
    pub drill: Coord,
    #[serde(default = "top")]
    pub from_layer: LayerId,
    #[serde(default = "bottom")]
    pub to_layer: LayerId,
    #[serde(default)]
    pub net: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ZoneConnect {
    /// Pads connect through thermal-relief spokes.
    #[default]
    Thermal,
    /// Pads are fully flooded.
    Solid,
    /// Pads are cleared, never connected by the zone.
    None,
}

/// One filled region: outer boundary plus holes (counter-clockwise outer,
/// clockwise holes not required — consumers must use even-odd or nonzero
/// consistently; fill output uses CCW outer / CW holes).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ZonePolygon {
    pub outer: Vec<Point>,
    #[serde(default)]
    pub holes: Vec<Vec<Point>>,
}

/// A copper pour (or keep-out area when `keepout` is set). `fill` is the
/// computed copper, refreshed by "Repour"; it is saved so boards reopen fast.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Zone {
    pub outline: Vec<Point>,
    pub layer: LayerId,
    #[serde(default)]
    pub net: String,
    #[serde(default)]
    pub name: String,
    /// Higher priority pours first and wins overlaps.
    #[serde(default)]
    pub priority: i32,
    /// Extra clearance on top of the clearance rules (0 = rules only).
    #[serde(default)]
    pub clearance: Coord,
    /// Copper narrower than this is removed.
    #[serde(default)]
    pub min_width: Coord,
    #[serde(default)]
    pub connect: ZoneConnect,
    #[serde(default)]
    pub thermal_gap: Coord,
    #[serde(default)]
    pub spoke_width: Coord,
    /// Keep-out: no copper (tracks, vias, pours) allowed inside on `layer`.
    #[serde(default)]
    pub keepout: bool,
    /// Remove fill islands not connected to any pad of the zone's net.
    #[serde(default = "default_true")]
    pub remove_islands: bool,
    #[serde(default)]
    pub fill: Vec<ZonePolygon>,
    /// Teardrop copper joining a track to a pad or via (Tools ▸ Teardrops regenerates these).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub teardrop: bool,
}

fn default_true() -> bool {
    true
}

/// A footprint placed on the board. Its geometry comes from the document's
/// footprint cache (`footprint` key); bottom-side parts are mirrored across
/// their local Y axis and their layers flipped.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlacedFootprint {
    pub designator: String,
    #[serde(default)]
    pub value: String,
    /// Key into the document's footprint cache ("Library/Name").
    pub footprint: String,
    pub pos: Point,
    #[serde(default)]
    pub rotation: f64,
    #[serde(default)]
    pub side: Side,
    #[serde(default)]
    pub locked: bool,
    /// Pad number → net name.
    #[serde(default)]
    pub pad_nets: BTreeMap<String, String>,
    /// Source component ("Library/Component") for traceability and BOMs.
    #[serde(default)]
    pub component: String,
    /// Extra parameters carried from the schematic (MPN, Manufacturer…).
    #[serde(default)]
    pub parameters: BTreeMap<String, String>,
}

impl PlacedFootprint {
    /// local → world for footprint geometry.
    pub fn placement(&self) -> Placement {
        Placement { offset: self.pos, rotation: self.rotation, mirror: self.side == Side::Bottom }
    }

    /// World layer of a footprint-local layer.
    pub fn world_layer(&self, local: LayerId) -> LayerId {
        if self.side == Side::Bottom { layer::flipped(local) } else { local }
    }

    pub fn net_of(&self, pad: &str) -> &str {
        self.pad_nets.get(pad).map_or("", |s| s.as_str())
    }
}

/// Re-exported so `Item::Board` can wrap it.
pub type Board = BoardSettings;
