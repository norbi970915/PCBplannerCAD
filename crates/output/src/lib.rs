//! Outputs generated from PCBPlannerCAD boards and schematics.
//!
//! - [`gerber`], [`excellon`], [`assembly`] (pick-and-place, BOM), [`fab`] (zipped fab package)
//! - [`gerber_read`]: parses Gerber/Excellon back for the built-in viewer
//! - [`pdf`]: vector PDF prints of schematics and board layers
//! - [`ipc2581`]: IPC-2581 rev B
//! - [`step`]: faceted STEP AP214 export of the 3D board model

pub mod assembly;
pub mod excellon;
pub mod fab;
pub mod gerber;
pub mod gerber_read;
pub mod ipc2581;
pub mod pdf;
pub mod step;

#[derive(Debug, thiserror::Error)]
pub enum OutputError {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Invalid(String),
}
