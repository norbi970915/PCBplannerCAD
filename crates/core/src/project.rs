use serde::{Deserialize, Serialize};

/// A design project: the documents and libraries that belong together.
/// Paths are relative to the project file's directory.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub name: String,
    /// Schematic sheets (`.arsch`), in display order. Sheets referenced by
    /// sheet symbols are children; the rest are top-level sheets.
    #[serde(default)]
    pub sheets: Vec<String>,
    #[serde(default)]
    pub pcb: Option<String>,
    /// Project libraries (`.arlib`).
    #[serde(default)]
    pub libraries: Vec<String>,
    /// Assembly variants: which parts are not fitted in each build.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub variants: Vec<Variant>,
}

/// A build variant; designators in `dnp` are left out of BOM and assembly outputs.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Variant {
    pub name: String,
    #[serde(default)]
    pub dnp: Vec<String>,
}
