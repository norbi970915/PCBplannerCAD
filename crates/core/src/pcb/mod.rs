//! Printed circuit board model and engines.
//!
//! - [`items`] / [`board`]: board objects and settings (stack, rules, classes).
//! - [`geometry`]: pads/graphics in world coordinates, outline loops.
//! - [`query`]: design-rule scope expressions (`InNet('GND') and OnLayer(...)`).
//! - [`connectivity`]: copper connectivity, ratsnest.
//! - [`drc`]: design rule check.
//! - [`fill`]: copper pour filling.
//! - [`router`]: interactive routing (walkaround / push), diff pairs, length tuning.
//! - [`sync`]: schematic → board engineering change orders.

pub mod board;
pub mod cleanup;
pub mod route_cleanup;
pub mod connectivity;
pub mod dimension;
pub mod drag;
pub mod elastic;
pub mod drc;
pub mod fill;
pub mod geometry;
pub mod items;
pub mod panel;
pub mod polygon;
pub mod query;
pub mod length;
pub mod router;
pub mod stitch;
pub mod teardrop;
pub mod sync;

pub use board::{BoardSettings, NetClass, Rule, RuleKind, Stackup};
pub use items::{ArcTrack, PlacedFootprint, Side, Track, Via, Zone, ZoneConnect, ZonePolygon};

#[cfg(test)]
mod editing_tests;

#[cfg(test)]
mod via_guard_tests;

pub mod setup;
