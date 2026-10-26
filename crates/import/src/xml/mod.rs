//! XML importer.
//!
//! Reads XML 6–9 XML (`.lbr` libraries and `.sch` schematics, which is
//! also what XML design exports, and `.brd` boards). XML shares
//! PCBPlannerCAD's Y-up, CCW-degree conventions, but a mirrored placement `MRa`
//! rotates *then* mirrors, which in core terms (mirror, then rotate) is a
//! rotation by `-a`; mirrored texts instead keep `a` and flip alignment.
//!
//! - Packages become footprints (with a synthesized courtyard).
//! - Each deviceset becomes one symbol (one unit per gate); every
//!   device × technology becomes a component.
//! - Each schematic `<sheet>` becomes one schematic document; supply symbols
//!   become power ports and net segments are labeled so XML's name-based
//!   connectivity survives.
//! - A `.brd` becomes one PCB document: board settings from the design
//!   rules and net classes, placed footprints, tracks, vias and zones.

mod board;
mod library;
mod package;
mod schematic;
mod util;

#[cfg(test)]
mod board_tests;
#[cfg(test)]
mod tests;

use std::path::Path;

use pcbplannercad_core::Library;

use crate::{BoardImport, ImportError, ImportReport, SchematicImport};

/// Parses an XML `.lbr` file's text into a library named `lib_name`.
pub fn parse_library(text: &str, lib_name: &str) -> Result<(Library, ImportReport), ImportError> {
    let doc = util::parse_xml(text)?;
    let root = doc.root_element();
    if root.tag_name().name() != "eagle" {
        return Err(ImportError::Invalid("not a supported XML design file".into()));
    }
    let lib = root
        .descendants()
        .find(|n| n.is_element() && n.tag_name().name() == "library")
        .ok_or_else(|| ImportError::Invalid("not an XML library (no <library> element)".into()))?;
    let mut report = ImportReport::default();
    let converted = library::convert_library(lib, lib_name, &mut report);
    Ok((converted.library, report))
}

/// Imports an XML `.sch` file: one document per sheet plus the embedded libraries.
pub fn import_schematic(path: &Path) -> Result<SchematicImport, ImportError> {
    let bytes = std::fs::read(path)?;
    let text = String::from_utf8_lossy(&bytes);
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "schematic".into());
    parse_schematic(&text, &stem)
}

/// Like [`import_schematic`] for in-memory text; `stem` names the output sheets.
pub fn parse_schematic(text: &str, stem: &str) -> Result<SchematicImport, ImportError> {
    schematic::import_schematic_text(text, stem)
}

/// Imports an XML `.brd` file into a PCB document.
pub fn import_board(path: &Path) -> Result<BoardImport, ImportError> {
    let bytes = std::fs::read(path)?;
    let text = String::from_utf8_lossy(&bytes);
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "board".into());
    parse_board(&text, &stem)
}

/// Like [`import_board`] for in-memory text; `file_stem` names the output file.
pub fn parse_board(text: &str, file_stem: &str) -> Result<BoardImport, ImportError> {
    board::import_board_text(text, file_stem)
}
