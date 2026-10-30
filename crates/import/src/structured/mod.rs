//! Structured S-expression projects, boards, schematics and libraries:
//! symbol libraries (`.kicad_sym`), footprints (`.kicad_mod`, `.pretty`
//! folders), schematics (`.kicad_sch`, with hierarchy), boards (`.kicad_pcb`) and
//! projects (`.kicad_pro`).
//!
//! Coordinate conventions: symbol libraries are Y-up like the PCBPlanner Studio world
//! and are read as-is; footprints and schematics are Y-down and get Y negated.
//! Source angles are counter-clockwise on screen, so negating Y keeps them
//! unchanged in our Y-up world.

mod board;
mod common;
mod footprint;
mod json;
mod schematic;
pub mod sexpr;
mod symbols;

#[cfg(test)]
mod board_tests;
#[cfg(test)]
mod tests;

use std::path::Path;

use pcbplannercad_core::Project;

pub use board::{import_board, parse_board};
pub use footprint::{import_footprint_dir, parse_footprint};
pub use schematic::import_schematic;
pub use symbols::parse_symbol_library;

use crate::ProjectImport;
use crate::report::ImportError;

/// Imports a `.kicad_pro` project: its root schematic `<stem>.kicad_sch`
/// (with all child sheets), its board `<stem>.kicad_pcb` when present, and a
/// [`Project`] listing them.
pub fn import_project(pro: &Path) -> Result<ProjectImport, ImportError> {
    let name = pro
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .ok_or_else(|| ImportError::Invalid(format!("not a project file: {}", pro.display())))?;
    if !pro.is_file() {
        return Err(ImportError::Io(std::io::Error::new(std::io::ErrorKind::NotFound, format!("{} not found", pro.display()))));
    }
    let root = pro.with_file_name(format!("{name}.kicad_sch"));
    let schematic = import_schematic(&root)?;
    let pcb_path = pro.with_file_name(format!("{name}.kicad_pcb"));
    let board = if pcb_path.is_file() { Some(import_board(&pcb_path)?) } else { None };
    let project = Project {
        name,
        sheets: schematic.sheets.iter().map(|s| s.file_name.clone()).collect(),
        pcb: board.as_ref().map(|b| b.file_name.clone()),
        libraries: Vec::new(),
        variants: Vec::new(),
    };
    Ok(ProjectImport { project, schematic, board })
}
