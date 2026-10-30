//! Library generation: the editable footprint geometry wizard,
//! the box-symbol wizard, and the built-in "PCBPlanner Basic" starter library.
//!
//! * [`wizard`] — `FootprintSpec` → `Footprint`, plus [`wizard::families`]
//!   describing every family's parameters for the UI.
//! * [`symbols`] — [`symbols::box_symbol`] and the hand-drawn symbols.
//! * [`starter`] — [`starter::starter_library`].

mod builder;
pub mod starter;
pub mod symbols;
pub mod wizard;

#[cfg(test)]
mod tests;

pub use starter::{STARTER_LIBRARY_NAME, starter_library};
pub use symbols::{PinSpec, SymbolSpec, box_symbol};
pub use wizard::{FamilyInfo, FootprintSpec, ParamInfo, ParamKind, default_name, families, generate};
