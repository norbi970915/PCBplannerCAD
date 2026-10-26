//! Importers that turn foreign design files into PCBPlannerCAD core types.
//!
//! - [`structured`]: structured format 6+ S-expression files (`.kicad_sym`, `.kicad_mod`, `.kicad_sch`, `.kicad_pro`).
//! - [`xml`]: XML 6+ XML files (`.lbr`, `.sch`), which is also what
//!   XML design exports.

pub mod xml;
pub mod paths;
pub mod structured;
mod report;

use pcbplannercad_core::{Document, Library, Project};

pub use report::{ImportError, ImportReport, ReportMessage, Severity};

/// One imported schematic sheet, ready to be saved as `file_name`.
#[derive(Debug)]
pub struct ImportedSheet {
    /// Suggested file name relative to the project directory, e.g. "power.arsch".
    pub file_name: String,
    /// A `DocKind::Schematic` document whose part cache covers every placed part.
    pub document: Document,
}

#[derive(Debug, Default)]
pub struct SchematicImport {
    /// Root sheet(s) first, then children in discovery order.
    pub sheets: Vec<ImportedSheet>,
    /// Libraries recovered from the design's embedded library data, so users
    /// can keep editing/placing the same parts.
    pub libraries: Vec<Library>,
    pub report: ImportReport,
}

#[derive(Debug)]
pub struct ProjectImport {
    pub project: Project,
    pub schematic: SchematicImport,
    /// The project's board, when it has one.
    pub board: Option<BoardImport>,
}

/// An imported board: a `DocKind::Pcb` document with its `Item::Board`
/// settings, footprint cache, placed footprints, copper and outline.
#[derive(Debug)]
pub struct BoardImport {
    /// Suggested file name, e.g. "board.arpcb".
    pub file_name: String,
    pub document: Document,
    /// Footprint libraries recovered from the board's embedded footprints.
    pub libraries: Vec<Library>,
    pub report: ImportReport,
}
