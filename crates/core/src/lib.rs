//! PCBPlanner Studio core: the editor-agnostic design model.
//!
//! All geometry is stored as integer nanometers ([`Coord`]) so that snapping,
//! rule checks and manufacturing output never suffer from float rounding.
//! Every mutation goes through a [`Command`], which resolves into a list of
//! concrete [`Edit`]s; those edits are what undo/redo and the UI deltas use.

pub mod command;
pub mod document;
pub mod error;
pub mod font;
pub mod geom;
pub mod io;
pub mod item;
pub mod layer;
pub mod library;
pub mod libgen;
pub mod pcb;
pub mod project;
pub mod schematic;
pub mod session;
pub mod storage;
pub mod units;

pub use command::Command;
pub use document::{DocKind, Document, Edit, IdItem, ItemId, LayerId};
pub use error::CoreError;
pub use item::Item;
pub use library::{CachedPart, Component, Footprint, Library, Symbol};
pub use project::Project;
pub use session::{Delta, Session};
pub use units::{Coord, Point};

pub mod recovery;
