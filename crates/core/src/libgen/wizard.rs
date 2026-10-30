//! Editable footprint geometry using user-selected land goals.
//! Default goals are uniform PCBPlanner Studio modeling choices: 0.10 mm toe, zero heel and side.
//! No family-by-family standards fillet table is used. Package dimensions are example
//! inputs; verify the actual component drawing and manufacturing process before use.
//! Existing tolerance propagation and geometric clearance checks remain engineering models.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::builder::{FpBuilder, Pin1Corner, dot_offset, silk_keepout};
use crate::error::CoreError;
use crate::item::PadShape;
use crate::library::Footprint;

// ---------- Geometry model ----------

const FAB_TOL: f64 = 0.10;
const PLACE_TOL: f64 = 0.05;
const ROUND: f64 = 0.05;
/// Silk outline offset outside the component body.
const SILK_BODY_OFFSET: f64 = 0.11;
const MIN_COPPER_GAP: f64 = 0.2;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Tol {
    pub min: f64,
    pub max: f64,
}

impl Tol {
    pub fn new(min: f64, max: f64) -> Self {
        Tol { min, max }
    }

    pub fn pm(nom: f64, tol: f64) -> Self {
        Tol { min: nom - tol, max: nom + tol }
    }

    fn range(&self) -> f64 {
        self.max - self.min
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct LandGoals { pub toe: f64, pub heel: f64, pub side: f64 }
impl Default for LandGoals { fn default() -> Self { Self { toe:0.10, heel:0.0, side:0.0 } } }
impl LandGoals { fn validate(&self)->Result<(),CoreError>{
    ensure(self.toe.is_finite()&&self.heel.is_finite()&&self.side.is_finite(),"Land goals must be finite")?;
    ensure((0.0..=2.0).contains(&self.toe)&&(-0.5..=2.0).contains(&self.heel)&&(-0.5..=2.0).contains(&self.side),"Land goals are outside the supported range")
} }

/// Outer extent `z`, inner gap `g` and land width `x` (mm).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Land {
    pub z: f64,
    pub g: f64,
    pub x: f64,
}

impl Land {
    pub fn len(&self) -> f64 {
        (self.z - self.g) / 2.0
    }

    /// Distance from the origin to each pad center.
    pub fn offset(&self) -> f64 {
        (self.z + self.g) / 4.0
    }
}

fn snap(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

fn round_up(v: f64) -> f64 {
    snap((v / ROUND - 1e-6).ceil() * ROUND)
}

fn round_down(v: f64) -> f64 {
    snap((v / ROUND + 1e-6).floor() * ROUND)
}

pub(crate) fn calculate_land(l: Tol, t: Tol, w: Tol, j: LandGoals) -> Land {
    let ft = FAB_TOL * FAB_TOL + PLACE_TOL * PLACE_TOL;
    let (cl, ct, cw) = (l.range(), t.range(), w.range());
    let s_min = l.min - 2.0 * t.max;
    let s_max = l.max - 2.0 * t.min;
    let cs = s_max - s_min;
    let cs_rms = (cl * cl + 2.0 * ct * ct).sqrt();
    let s_max_rms = s_max - (cs - cs_rms) / 2.0;
    let z = l.min + 2.0 * j.toe + (cl * cl + ft).sqrt();
    let g = s_max_rms - 2.0 * j.heel - (cs_rms * cs_rms + ft).sqrt();
    let x = w.min + 2.0 * j.side + (cw * cw + ft).sqrt();
    Land { z: round_up(z), g: round_down(g), x: round_up(x) }
}

// ---------- Validation helpers ----------

fn err(msg: &'static str) -> CoreError {
    CoreError::InvalidGeometry(msg)
}

fn ensure(ok: bool, msg: &'static str) -> Result<(), CoreError> {
    if ok { Ok(()) } else { Err(err(msg)) }
}

fn positive(v: f64, msg: &'static str) -> Result<f64, CoreError> {
    ensure(v.is_finite() && v > 0.0, msg)?;
    Ok(v)
}

fn tol(min: f64, max: f64, msg: &'static str) -> Result<Tol, CoreError> {
    ensure(min.is_finite() && max.is_finite() && min > 0.0 && max >= min, msg)?;
    Ok(Tol::new(min, max))
}

fn check_land(land: &Land) -> Result<(), CoreError> {
    ensure(land.g >= 0.1, "land pattern has no gap between opposite pads; check lead span and lead length")?;
    ensure(land.len() >= 0.15 && land.x >= 0.1, "land pattern pads are too small; check lead dimensions")
}

fn validate_prefix(p: &str) -> Result<(), CoreError> {
    ensure(
        !p.is_empty() && p.len() <= 16 && p.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'),
        "prefix must be 1-16 letters, digits, '_' or '-'",
    )
}

/// Millimeter value for names: up to 3 decimals, trailing zeros removed.
fn num(v: f64) -> String {
    let s = format!("{:.3}", v);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.to_string() }
}

// ---------- Spec types ----------

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "family", rename_all = "camelCase")]
pub enum FootprintSpec {
    Chip(ChipSpec),
    Sod(SodSpec),
    Sot(SotSpec),
    Soic(DualRowSpec),
    Ssop(DualRowSpec),
    Tssop(DualRowSpec),
    Qfn(QfnSpec),
    Qfp(QfpSpec),
    Dip(DipSpec),
    PinHeader(PinHeaderSpec),
    Axial(AxialSpec),
    Radial(RadialSpec),
    Crystal(CrystalSpec),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChipSize {
    #[serde(rename = "0201")]
    I0201,
    #[serde(rename = "0402")]
    I0402,
    #[default]
    #[serde(rename = "0603")]
    I0603,
    #[serde(rename = "0805")]
    I0805,
    #[serde(rename = "1206")]
    I1206,
    #[serde(rename = "1210")]
    I1210,
    #[serde(rename = "1812")]
    I1812,
    #[serde(rename = "2010")]
    I2010,
    #[serde(rename = "2512")]
    I2512,
}

struct ChipDims {
    code: &'static str,
    metric: &'static str,
    /// (nominal, ±tolerance) of length, width and terminal length.
    l: (f64, f64),
    w: (f64, f64),
    t: (f64, f64),
}

impl ChipSize {
    pub const ALL: [ChipSize; 9] = [
        ChipSize::I0201,
        ChipSize::I0402,
        ChipSize::I0603,
        ChipSize::I0805,
        ChipSize::I1206,
        ChipSize::I1210,
        ChipSize::I1812,
        ChipSize::I2010,
        ChipSize::I2512,
    ];

    fn dims(self) -> ChipDims {
        let d = |code, metric, l, w, t| ChipDims { code, metric, l, w, t };
        match self {
            ChipSize::I0201 => d("0201", "0603", (0.60, 0.03), (0.30, 0.03), (0.15, 0.05)),
            ChipSize::I0402 => d("0402", "1005", (1.00, 0.05), (0.50, 0.05), (0.25, 0.10)),
            ChipSize::I0603 => d("0603", "1608", (1.60, 0.10), (0.80, 0.10), (0.30, 0.15)),
            ChipSize::I0805 => d("0805", "2012", (2.00, 0.10), (1.25, 0.10), (0.40, 0.20)),
            ChipSize::I1206 => d("1206", "3216", (3.20, 0.15), (1.60, 0.15), (0.50, 0.25)),
            ChipSize::I1210 => d("1210", "3225", (3.20, 0.15), (2.50, 0.15), (0.50, 0.25)),
            ChipSize::I1812 => d("1812", "4532", (4.50, 0.20), (3.20, 0.20), (0.60, 0.30)),
            ChipSize::I2010 => d("2010", "5025", (5.00, 0.15), (2.50, 0.15), (0.60, 0.25)),
            ChipSize::I2512 => d("2512", "6332", (6.30, 0.15), (3.20, 0.15), (0.60, 0.25)),
        }
    }

    pub fn code(self) -> &'static str {
        self.dims().code
    }
}

/// Two-terminal chip (resistor, capacitor, inductor, chip diode/LED, fuse…).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct ChipSpec {
    pub size: ChipSize,
    /// Name prefix: R, C, L, D, LED, CP, Fuse… (naming only).
    pub prefix: String,
    /// Draw a silk polarity bar at pad 1 (cathode / positive).
    pub polarity: bool,
}

impl Default for ChipSpec {
    fn default() -> Self {
        ChipSpec { size: ChipSize::I0603, prefix: "R".into(), polarity: false }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SodPackage {
    #[default]
    #[serde(rename = "SOD-123")]
    Sod123,
    #[serde(rename = "SOD-323")]
    Sod323,
    #[serde(rename = "SOD-523")]
    Sod523,
    #[serde(rename = "SMA")]
    Sma,
    #[serde(rename = "SMB")]
    Smb,
    #[serde(rename = "SMC")]
    Smc,
}

impl SodPackage {
    pub const ALL: [SodPackage; 6] = [SodPackage::Sod123, SodPackage::Sod323, SodPackage::Sod523, SodPackage::Sma, SodPackage::Smb, SodPackage::Smc];

    pub fn label(self) -> &'static str {
        match self {
            SodPackage::Sod123 => "SOD-123",
            SodPackage::Sod323 => "SOD-323",
            SodPackage::Sod523 => "SOD-523",
            SodPackage::Sma => "SMA",
            SodPackage::Smb => "SMB",
            SodPackage::Smc => "SMC",
        }
    }
}

/// Polarized two-terminal molded diode packages. Pad 1 is the cathode.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct SodSpec {
    pub package: SodPackage,
    pub prefix: String,
}

impl Default for SodSpec {
    fn default() -> Self {
        SodSpec { package: SodPackage::Sod123, prefix: "D".into() }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SotPackage {
    #[default]
    #[serde(rename = "SOT-23")]
    Sot23,
    #[serde(rename = "SOT-23-5")]
    Sot23_5,
    #[serde(rename = "SOT-23-6")]
    Sot23_6,
    #[serde(rename = "SOT-223")]
    Sot223,
    #[serde(rename = "SOT-89")]
    Sot89,
}

impl SotPackage {
    pub const ALL: [SotPackage; 5] = [SotPackage::Sot23, SotPackage::Sot23_5, SotPackage::Sot23_6, SotPackage::Sot223, SotPackage::Sot89];

    pub fn label(self) -> &'static str {
        match self {
            SotPackage::Sot23 => "SOT-23",
            SotPackage::Sot23_5 => "SOT-23-5",
            SotPackage::Sot23_6 => "SOT-23-6",
            SotPackage::Sot223 => "SOT-223",
            SotPackage::Sot89 => "SOT-89",
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct SotSpec {
    pub package: SotPackage,
}

/// SOIC / SSOP / TSSOP. Every field is optional; `null` means the family
/// default (body length is derived from the pin count).
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct DualRowSpec {
    pub pins: Option<u32>,
    pub pitch: Option<f64>,
    /// Plastic body width E1 (across the rows).
    pub body_width: Option<f64>,
    /// Plastic body length D (along the rows).
    pub body_length: Option<f64>,
    /// Lead span E, tip to tip.
    pub lead_span_min: Option<f64>,
    pub lead_span_max: Option<f64>,
    /// Foot length L.
    pub lead_length_min: Option<f64>,
    pub lead_length_max: Option<f64>,
    /// Lead width b.
    pub lead_width_min: Option<f64>,
    pub lead_width_max: Option<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DualFamily {
    Soic,
    Ssop,
    Tssop,
}

/// Fully resolved dual-row parameters.
#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct DualRow {
    pins: u32,
    pitch: f64,
    body_width: f64,
    body_length: f64,
    lead_span_min: f64,
    lead_span_max: f64,
    lead_length_min: f64,
    lead_length_max: f64,
    lead_width_min: f64,
    lead_width_max: f64,
}

impl DualFamily {
    fn label(self) -> &'static str {
        match self {
            DualFamily::Soic => "SOIC",
            DualFamily::Ssop => "SSOP",
            DualFamily::Tssop => "TSSOP",
        }
    }

    fn defaults(self) -> DualRow {
        let d = |pins, pitch, bw, span: (f64, f64), foot: (f64, f64), w: (f64, f64)| DualRow {
            pins,
            pitch,
            body_width: bw,
            body_length: 0.0,
            lead_span_min: span.0,
            lead_span_max: span.1,
            lead_length_min: foot.0,
            lead_length_max: foot.1,
            lead_width_min: w.0,
            lead_width_max: w.1,
        };
        match self {
            DualFamily::Soic => d(8, 1.27, 3.9, (5.8, 6.2), (0.40, 1.27), (0.31, 0.51)),
            DualFamily::Ssop => d(20, 0.65, 5.3, (7.4, 8.2), (0.55, 0.95), (0.22, 0.38)),
            DualFamily::Tssop => d(14, 0.65, 4.4, (6.2, 6.6), (0.45, 0.75), (0.19, 0.30)),
        }
    }

    /// JEDEC body length for common pin counts, else pitch-based estimate.
    fn body_length(self, pins: u32, pitch: f64) -> f64 {
        let table: &[(u32, f64)] = match self {
            DualFamily::Soic => &[(8, 4.9), (14, 8.65), (16, 9.9)],
            DualFamily::Ssop => &[(8, 3.0), (14, 6.2), (16, 6.2), (20, 7.2), (24, 8.2), (28, 10.2)],
            DualFamily::Tssop => &[(8, 3.0), (14, 5.0), (16, 5.0), (20, 6.5), (24, 7.8), (28, 9.7)],
        };
        let standard = (self == DualFamily::Soic && (pitch - 1.27).abs() < 1e-6) || (self != DualFamily::Soic && (pitch - 0.65).abs() < 1e-6);
        if standard && let Some(&(_, l)) = table.iter().find(|(n, _)| *n == pins) {
            return l;
        }
        round_up((pins / 2).saturating_sub(1) as f64 * pitch + 1.1)
    }
}

impl DualRowSpec {
    fn resolve(&self, fam: DualFamily) -> DualRow {
        let d = fam.defaults();
        let pins = self.pins.unwrap_or(d.pins);
        let pitch = self.pitch.unwrap_or(d.pitch);
        DualRow {
            pins,
            pitch,
            body_width: self.body_width.unwrap_or(d.body_width),
            body_length: self.body_length.unwrap_or_else(|| fam.body_length(pins, pitch)),
            lead_span_min: self.lead_span_min.unwrap_or(d.lead_span_min),
            lead_span_max: self.lead_span_max.unwrap_or(d.lead_span_max),
            lead_length_min: self.lead_length_min.unwrap_or(d.lead_length_min),
            lead_length_max: self.lead_length_max.unwrap_or(d.lead_length_max),
            lead_width_min: self.lead_width_min.unwrap_or(d.lead_width_min),
            lead_width_max: self.lead_width_max.unwrap_or(d.lead_width_max),
        }
    }
}

/// Quad flat no-lead package with square body.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct QfnSpec {
    pub pins_per_side: u32,
    pub pitch: f64,
    pub body_size: f64,
    /// Terminal length L.
    pub lead_length_min: f64,
    pub lead_length_max: f64,
    /// Terminal width b.
    pub lead_width_min: f64,
    pub lead_width_max: f64,
    pub exposed_pad: bool,
    /// Exposed pad edge length; `null` = body − 1.3 mm.
    pub ep_size: Option<f64>,
    /// Split the exposed-pad paste into an N×N grid (0 or 1 = no split).
    pub paste_grid: u32,
}

impl Default for QfnSpec {
    fn default() -> Self {
        QfnSpec {
            pins_per_side: 4,
            pitch: 0.5,
            body_size: 3.0,
            lead_length_min: 0.3,
            lead_length_max: 0.5,
            lead_width_min: 0.18,
            lead_width_max: 0.30,
            exposed_pad: true,
            ep_size: None,
            paste_grid: 0,
        }
    }
}

impl QfnSpec {
    fn ep(&self) -> f64 {
        self.ep_size.unwrap_or_else(|| round_down(self.body_size - 1.3).max(0.5))
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum QfpStyle {
    #[default]
    #[serde(rename = "LQFP")]
    Lqfp,
    #[serde(rename = "TQFP")]
    Tqfp,
}

impl QfpStyle {
    fn label(self) -> &'static str {
        match self {
            QfpStyle::Lqfp => "LQFP",
            QfpStyle::Tqfp => "TQFP",
        }
    }
}

/// LQFP/TQFP with square body. `null` lead span/width = derived from body and pitch.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct QfpSpec {
    pub style: QfpStyle,
    /// Total pin count (multiple of 4).
    pub pins: u32,
    pub pitch: f64,
    pub body_size: f64,
    pub lead_span_min: Option<f64>,
    pub lead_span_max: Option<f64>,
    pub lead_length_min: f64,
    pub lead_length_max: f64,
    pub lead_width_min: Option<f64>,
    pub lead_width_max: Option<f64>,
}

impl Default for QfpSpec {
    fn default() -> Self {
        QfpSpec {
            style: QfpStyle::Lqfp,
            pins: 48,
            pitch: 0.5,
            body_size: 7.0,
            lead_span_min: None,
            lead_span_max: None,
            lead_length_min: 0.45,
            lead_length_max: 0.75,
            lead_width_min: None,
            lead_width_max: None,
        }
    }
}

impl QfpSpec {
    fn span(&self) -> Tol {
        Tol::new(self.lead_span_min.unwrap_or(self.body_size + 1.8), self.lead_span_max.unwrap_or(self.body_size + 2.2))
    }

    fn width(&self) -> Tol {
        let p = self.pitch;
        let auto = if (p - 0.4).abs() < 1e-6 {
            (0.13, 0.23)
        } else if (p - 0.5).abs() < 1e-6 {
            (0.17, 0.27)
        } else if (p - 0.65).abs() < 1e-6 {
            (0.22, 0.38)
        } else if (p - 0.8).abs() < 1e-6 {
            (0.30, 0.45)
        } else {
            (snap(p * 0.44 - 0.05), snap(p * 0.44 + 0.05))
        };
        Tol::new(self.lead_width_min.unwrap_or(auto.0), self.lead_width_max.unwrap_or(auto.1))
    }
}

/// Through-hole dual in-line package.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct DipSpec {
    pub pins: u32,
    /// Row-to-row spacing, typically 7.62 or 15.24.
    pub row_spacing: f64,
    pub pitch: f64,
    pub pad_diameter: f64,
    pub drill: f64,
    /// `null` = row spacing − 1.27.
    pub body_width: Option<f64>,
    /// `null` = pins/2 × pitch.
    pub body_length: Option<f64>,
}

impl Default for DipSpec {
    fn default() -> Self {
        DipSpec { pins: 8, row_spacing: 7.62, pitch: 2.54, pad_diameter: 1.6, drill: 0.8, body_width: None, body_length: None }
    }
}

impl DipSpec {
    fn body(&self) -> (f64, f64) {
        (self.body_width.unwrap_or(self.row_spacing - 1.27), self.body_length.unwrap_or((self.pins / 2) as f64 * self.pitch))
    }
}

/// Through-hole pin header. `rows` pins side by side (X), `pinsPerRow` down (Y).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct PinHeaderSpec {
    pub rows: u32,
    pub pins_per_row: u32,
    pub pitch: f64,
    /// `null` = 1.7 / 1.35 / 1.0 mm for 2.54 / 2.0 / 1.27 pitch.
    pub pad_diameter: Option<f64>,
    /// `null` = 1.0 / 0.8 / 0.65 mm for 2.54 / 2.0 / 1.27 pitch.
    pub drill: Option<f64>,
    /// Box header (shroud with key notch) outline.
    pub shrouded: bool,
}

impl Default for PinHeaderSpec {
    fn default() -> Self {
        PinHeaderSpec { rows: 1, pins_per_row: 4, pitch: 2.54, pad_diameter: None, drill: None, shrouded: false }
    }
}

impl PinHeaderSpec {
    fn pad_drill(&self) -> (f64, f64) {
        let p = self.pitch;
        let auto = if (p - 2.54).abs() < 1e-6 {
            (1.7, 1.0)
        } else if (p - 2.0).abs() < 1e-6 {
            (1.35, 0.8)
        } else if (p - 1.27).abs() < 1e-6 {
            (1.0, 0.65)
        } else {
            (snap(p * 0.67), snap(p * 0.4))
        };
        (self.pad_diameter.unwrap_or(auto.0), self.drill.unwrap_or(auto.1))
    }
}

/// Horizontal axial through-hole part (resistor, diode…). Pad 1 is the
/// cathode/positive end when `polarity` is set.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct AxialSpec {
    pub prefix: String,
    pub pitch: f64,
    pub body_length: f64,
    pub body_diameter: f64,
    pub pad_diameter: f64,
    pub drill: f64,
    pub polarity: bool,
}

impl Default for AxialSpec {
    fn default() -> Self {
        AxialSpec { prefix: "R".into(), pitch: 10.16, body_length: 6.3, body_diameter: 2.5, pad_diameter: 1.6, drill: 0.8, polarity: false }
    }
}

/// Radial through-hole can (electrolytic capacitor). Pad 1 is positive.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct RadialSpec {
    pub diameter: f64,
    pub pitch: f64,
    pub pad_diameter: f64,
    pub drill: f64,
    pub polarized: bool,
}

impl Default for RadialSpec {
    fn default() -> Self {
        RadialSpec { diameter: 5.0, pitch: 2.0, pad_diameter: 1.6, drill: 0.8, polarized: true }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum CrystalPackage {
    #[default]
    #[serde(rename = "HC-49")]
    Hc49,
    #[serde(rename = "3225")]
    Smd3225,
    #[serde(rename = "2016")]
    Smd2016,
}

impl CrystalPackage {
    pub const ALL: [CrystalPackage; 3] = [CrystalPackage::Hc49, CrystalPackage::Smd3225, CrystalPackage::Smd2016];
}

/// HC-49 (THT, 2 pins) or 4-pad SMD ceramic crystal (pins 1/3 = crystal, 2/4 = case).
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct CrystalSpec {
    pub package: CrystalPackage,
}

// ---------- Public API ----------

/// Builds the footprint described by `spec`, named by [`default_name`].
pub fn generate(spec: &FootprintSpec) -> Result<Footprint, CoreError> { generate_with_goals(spec, LandGoals::default()) }

pub fn generate_with_goals(spec: &FootprintSpec, goals: LandGoals) -> Result<Footprint, CoreError> {
    goals.validate()?;
    let name = default_name(spec);
    match spec {
        FootprintSpec::Chip(s) => chip(s, name, goals),
        FootprintSpec::Sod(s) => sod(s, name, goals),
        FootprintSpec::Sot(s) => sot(s, name, goals),
        FootprintSpec::Soic(s) => dual_row(&s.resolve(DualFamily::Soic), DualFamily::Soic, name, goals),
        FootprintSpec::Ssop(s) => dual_row(&s.resolve(DualFamily::Ssop), DualFamily::Ssop, name, goals),
        FootprintSpec::Tssop(s) => dual_row(&s.resolve(DualFamily::Tssop), DualFamily::Tssop, name, goals),
        FootprintSpec::Qfn(s) => qfn(s, name, goals),
        FootprintSpec::Qfp(s) => qfp(s, name, goals),
        FootprintSpec::Dip(s) => dip(s, name),
        FootprintSpec::PinHeader(s) => pin_header(s, name),
        FootprintSpec::Axial(s) => axial(s, name),
        FootprintSpec::Radial(s) => radial(s, name),
        FootprintSpec::Crystal(s) => crystal(s, name),
    }
}

/// Footprint package name, e.g. `R_0603_1608Metric`,
/// `SOIC-8_3.9x4.9mm_P1.27mm`.
pub fn default_name(spec: &FootprintSpec) -> String {
    match spec {
        FootprintSpec::Chip(s) => {
            let d = s.size.dims();
            format!("{}_{}_{}Metric", s.prefix, d.code, d.metric)
        }
        FootprintSpec::Sod(s) => format!("{}_{}", s.prefix, s.package.label()),
        FootprintSpec::Sot(s) => match s.package {
            SotPackage::Sot223 => "SOT-223-3_TabPin2".into(),
            SotPackage::Sot89 => "SOT-89-3_TabPin2".into(),
            p => p.label().into(),
        },
        FootprintSpec::Soic(s) => dual_name(&s.resolve(DualFamily::Soic), DualFamily::Soic),
        FootprintSpec::Ssop(s) => dual_name(&s.resolve(DualFamily::Ssop), DualFamily::Ssop),
        FootprintSpec::Tssop(s) => dual_name(&s.resolve(DualFamily::Tssop), DualFamily::Tssop),
        FootprintSpec::Qfn(s) => {
            let b = num(s.body_size);
            let base = format!("QFN-{}", s.pins_per_side * 4);
            if s.exposed_pad {
                let e = num(s.ep());
                format!("{base}-1EP_{b}x{b}mm_P{}mm_EP{e}x{e}mm", num(s.pitch))
            } else {
                format!("{base}_{b}x{b}mm_P{}mm", num(s.pitch))
            }
        }
        FootprintSpec::Qfp(s) => {
            let b = num(s.body_size);
            format!("{}-{}_{b}x{b}mm_P{}mm", s.style.label(), s.pins, num(s.pitch))
        }
        FootprintSpec::Dip(s) => {
            let w = num(s.row_spacing);
            if (s.pitch - 2.54).abs() < 1e-6 { format!("DIP-{}_W{w}mm", s.pins) } else { format!("DIP-{}_W{w}mm_P{}mm", s.pins, num(s.pitch)) }
        }
        FootprintSpec::PinHeader(s) => {
            let base = format!("PinHeader_{}x{:02}_P{:.2}mm_Vertical", s.rows, s.pins_per_row, s.pitch);
            if s.shrouded { base + "_Shrouded" } else { base }
        }
        FootprintSpec::Axial(s) => {
            format!("{}_Axial_L{}mm_D{}mm_P{}mm_Horizontal", s.prefix, num(s.body_length), num(s.body_diameter), num(s.pitch))
        }
        FootprintSpec::Radial(s) => format!("{}_Radial_D{:.1}mm_P{:.2}mm", if s.polarized { "CP" } else { "C" }, s.diameter, s.pitch),
        FootprintSpec::Crystal(s) => match s.package {
            CrystalPackage::Hc49 => "Crystal_HC49-U_Vertical".into(),
            CrystalPackage::Smd3225 => "Crystal_SMD_3225-4Pin_3.2x2.5mm".into(),
            CrystalPackage::Smd2016 => "Crystal_SMD_2016-4Pin_2.0x1.6mm".into(),
        },
    }
}

fn dual_name(d: &DualRow, fam: DualFamily) -> String {
    format!("{}-{}_{}x{}mm_P{}mm", fam.label(), d.pins, num(d.body_width), num(d.body_length), num(d.pitch))
}

// ---------- Shared pieces ----------

/// Two pads on the X axis, pad 1 left; silk lines above/below the lands,
/// plus a polarity bar left of pad 1 when requested.
fn two_terminal(b: &mut FpBuilder, land: &Land, body_l: f64, body_w: f64, polarity: bool) {
    let (c, len) = (land.offset(), land.len());
    b.smd("1", -c, 0.0, len, land.x);
    b.smd("2", c, 0.0, len, land.x);
    b.body(0.0, 0.0, body_l, body_w, Pin1Corner::TopLeft);
    let y = (body_w / 2.0 + SILK_BODY_OFFSET).max(land.x / 2.0 + silk_keepout());
    let xe = body_l / 2.0;
    if polarity {
        let xp = -(land.z / 2.0 + silk_keepout());
        b.silk_path(&[(xe, y), (xp, y), (xp, -y), (xe, -y)], false);
    } else {
        b.silk_line((-xe, y), (xe, y));
        b.silk_line((-xe, -y), (xe, -y));
    }
}

/// Body outline silk for parts with leads on the left/right: top and bottom
/// edges, the top edge extended to the outer edge of pad 1; optional sides
/// (clipped by the pads).
fn side_lead_silk(b: &mut FpBuilder, body_w: f64, body_l: f64, pad_top: f64, x_pin1_outer: f64, sides: bool) {
    let y = (body_l / 2.0 + SILK_BODY_OFFSET).max(pad_top + silk_keepout());
    let x = body_w / 2.0 + SILK_BODY_OFFSET;
    b.silk_line((x_pin1_outer, y), (x, y));
    b.silk_line((-x, -y), (x, -y));
    if sides {
        b.silk_line((-x, y), (-x, -y));
        b.silk_line((x, y), (x, -y));
    }
}

/// Positions along a row of `n` pins at `pitch`, first one at the top/left.
fn row_positions(n: u32, pitch: f64) -> impl DoubleEndedIterator<Item = f64> {
    let half = (n.saturating_sub(1)) as f64 / 2.0;
    (0..n).map(move |i| snap((half - i as f64) * pitch))
}

// ---------- Family generators ----------

fn chip(s: &ChipSpec, name: String, goals: LandGoals) -> Result<Footprint, CoreError> {
    validate_prefix(&s.prefix)?;
    let d = s.size.dims();
    let j = goals;
    let land = calculate_land(Tol::pm(d.l.0, d.l.1), Tol::pm(d.t.0, d.t.1), Tol::pm(d.w.0, d.w.1), j);
    check_land(&land)?;
    let desc = format!("{} chip {} ({} metric), {:.2}x{:.2} mm body, PCBPlanner Studio editable land-pattern estimate", s.prefix, d.code, d.metric, d.l.0, d.w.0);
    let mut b = FpBuilder::new(name, desc);
    two_terminal(&mut b, &land, d.l.0, d.w.0, s.polarity);
    Ok(b.build())
}

fn sod(s: &SodSpec, name: String, goals: LandGoals) -> Result<Footprint, CoreError> {
    validate_prefix(&s.prefix)?;
    // (body length, body width, span L, foot T, width W, fillet)
    let (bl, bw, l, t, w, j) = match s.package {
        SodPackage::Sod123 => (2.7, 1.6, Tol::new(3.55, 3.85), Tol::new(0.25, 0.40), Tol::new(0.50, 0.70), goals),
        SodPackage::Sod323 => (1.7, 1.25, Tol::new(2.30, 2.70), Tol::new(0.25, 0.40), Tol::new(0.25, 0.40), goals),
        SodPackage::Sod523 => (1.2, 0.8, Tol::new(1.50, 1.70), Tol::new(0.15, 0.35), Tol::new(0.25, 0.35), goals),
        SodPackage::Sma => (4.3, 2.6, Tol::new(4.80, 5.28), Tol::new(0.76, 1.52), Tol::new(1.25, 1.65), goals),
        SodPackage::Smb => (4.3, 3.6, Tol::new(5.21, 5.59), Tol::new(0.76, 1.52), Tol::new(1.96, 2.21), goals),
        SodPackage::Smc => (6.9, 5.9, Tol::new(7.75, 8.13), Tol::new(0.76, 1.52), Tol::new(2.90, 3.20), goals),
    };
    let land = calculate_land(l, t, w, j);
    check_land(&land)?;
    let desc = format!("{} diode package, pad 1 = cathode, PCBPlanner Studio editable land-pattern estimate", s.package.label());
    let mut b = FpBuilder::new(name, desc);
    two_terminal(&mut b, &land, bl, bw, true);
    Ok(b.build())
}

#[derive(Clone, Copy)]
struct Lead {
    number: &'static str,
    right: bool,
    y: f64,
    width: Tol,
}

fn sot(s: &SotSpec, name: String, goals: LandGoals) -> Result<Footprint, CoreError> {
    let pkg = s.package;
    if pkg == SotPackage::Sot89 {
        return Ok(sot89(name));
    }
    let lead = |number, right, y, width| Lead { number, right, y, width };
    let w23 = Tol::new(0.30, 0.50);
    // (body width across leads, body length, span, foot, leads, pitch)
    let (bw, bl, span, foot, leads, _pitch): (f64, f64, Tol, Tol, Vec<Lead>, f64) = match pkg {
        SotPackage::Sot23 => (1.3, 2.9, Tol::new(2.10, 2.64), Tol::new(0.30, 0.50), vec![lead("1", false, 0.95, w23), lead("2", false, -0.95, w23), lead("3", true, 0.0, w23)], 0.95),
        SotPackage::Sot23_5 => (
            1.6,
            2.9,
            Tol::new(2.60, 3.00),
            Tol::new(0.30, 0.60),
            vec![lead("1", false, 0.95, w23), lead("2", false, 0.0, w23), lead("3", false, -0.95, w23), lead("4", true, -0.95, w23), lead("5", true, 0.95, w23)],
            0.95,
        ),
        SotPackage::Sot23_6 => (
            1.6,
            2.9,
            Tol::new(2.60, 3.00),
            Tol::new(0.30, 0.60),
            vec![
                lead("1", false, 0.95, w23),
                lead("2", false, 0.0, w23),
                lead("3", false, -0.95, w23),
                lead("4", true, -0.95, w23),
                lead("5", true, 0.0, w23),
                lead("6", true, 0.95, w23),
            ],
            0.95,
        ),
        SotPackage::Sot223 => {
            let w = Tol::new(0.66, 0.84);
            (3.5, 6.5, Tol::new(6.70, 7.30), Tol::new(0.70, 1.10), vec![lead("1", false, 2.3, w), lead("2", false, 0.0, w), lead("3", false, -2.3, w), lead("2", true, 0.0, Tol::new(2.90, 3.15))], 2.3)
        }
        SotPackage::Sot89 => unreachable!(),
    };
    let j = goals;
    let mut b = FpBuilder::new(name, format!("{} package, PCBPlanner Studio editable land-pattern estimate", pkg.label()));
    let mut z = 0.0_f64;
    let mut pad_top = f64::MIN;
    for l in &leads {
        let land = calculate_land(span, foot, l.width, j);
        check_land(&land)?;
        let x = if l.right { land.offset() } else { -land.offset() };
        b.smd(l.number, x, l.y, land.len(), land.x);
        z = land.z;
        pad_top = pad_top.max(l.y + land.x / 2.0);
    }
    b.body(0.0, 0.0, bw, bl, Pin1Corner::TopLeft);
    side_lead_silk(&mut b, bw, bl, pad_top, -z / 2.0, true);
    Ok(b.build())
}


/// SOT-89-3 (TO-243): leads 1-3 on the left at 1.5 mm pitch, pin 2 also
/// continues as a tab to the right. Fixed example geometry (toe 0.35 mm beyond the
/// lead tips, tab land 0.1 mm over the tab on each side).
fn sot89(name: String) -> Footprint {
    let mut b = FpBuilder::new(name, "SOT-89-3 package, tab connected to pin 2");
    let (bw, bl) = (2.45, 4.5);
    // Lead lands: x from -2.6 to -1.1; pin 2 extends further to join the tab land.
    b.smd("1", -1.85, 1.5, 1.5, 0.7);
    b.smd("2", -1.7, 0.0, 1.8, 0.8);
    b.smd("3", -1.85, -1.5, 1.5, 0.7);
    // Tab land: x from -0.9 to 2.225.
    b.smd("2", 0.6625, 0.0, 3.125, 1.8);
    b.body(0.0, 0.0, bw, bl, Pin1Corner::TopLeft);
    side_lead_silk(&mut b, bw, bl, 1.85, -2.6, true);
    b.build()
}

fn dual_row(d: &DualRow, fam: DualFamily, name: String, goals: LandGoals) -> Result<Footprint, CoreError> {
    ensure(d.pins >= 4 && d.pins.is_multiple_of(2) && d.pins <= 128, "pin count must be even, 4 to 128")?;
    let pitch = positive(d.pitch, "pitch must be positive")?;
    let bw = positive(d.body_width, "body width must be positive")?;
    let bl = positive(d.body_length, "body length must be positive")?;
    let span = tol(d.lead_span_min, d.lead_span_max, "lead span min/max must be positive and ordered")?;
    let foot = tol(d.lead_length_min, d.lead_length_max, "lead length min/max must be positive and ordered")?;
    let width = tol(d.lead_width_min, d.lead_width_max, "lead width min/max must be positive and ordered")?;
    ensure(width.max < pitch, "lead width must be smaller than the pitch")?;
    ensure(span.min > bw, "lead span must exceed the body width")?;
    let n = d.pins / 2;
    ensure(bl >= (n - 1) as f64 * pitch + width.max, "body is too short for the pin count and pitch")?;

    let mut land = calculate_land(span, foot, width, goals);
    land.x = land.x.min(snap(pitch - MIN_COPPER_GAP));
    check_land(&land)?;
    let desc = format!(
        "{}-{} {}x{} mm body, {} mm pitch, PCBPlanner Studio editable land-pattern estimate",
        fam.label(),
        d.pins,
        num(bw),
        num(bl),
        num(pitch)
    );
    let mut b = FpBuilder::new(name, desc);
    let (c, len) = (land.offset(), land.len());
    let ys: Vec<f64> = row_positions(n, pitch).collect();
    for (i, &y) in ys.iter().enumerate() {
        b.smd(&(i + 1).to_string(), -c, y, len, land.x);
    }
    for (i, &y) in ys.iter().rev().enumerate() {
        b.smd(&(n as usize + i + 1).to_string(), c, y, len, land.x);
    }
    b.body(0.0, 0.0, bw, bl, Pin1Corner::TopLeft);
    side_lead_silk(&mut b, bw, bl, ys[0] + land.x / 2.0, -land.z / 2.0, false);
    Ok(b.build())
}

/// Places `n` pads per side counter-clockwise from the top of the left side.
fn quad_pads(b: &mut FpBuilder, n: u32, pitch: f64, land: &Land) {
    let (c, len, x) = (land.offset(), land.len(), land.x);
    let pos: Vec<f64> = row_positions(n, pitch).collect();
    let mut k = 1;
    let mut pad = |b: &mut FpBuilder, px: f64, py: f64, w: f64, h: f64| {
        b.smd(&k.to_string(), px, py, w, h);
        k += 1;
    };
    for &p in &pos {
        pad(b, -c, p, len, x); // left, top → bottom
    }
    for &p in pos.iter().rev() {
        pad(b, p, -c, x, len); // bottom, left → right
    }
    for &p in pos.iter().rev() {
        pad(b, c, p, len, x); // right, bottom → top
    }
    for &p in &pos {
        pad(b, p, c, x, len); // top, right → left
    }
}

fn quad_corner_check(n: u32, pitch: f64, land: &Land) -> Result<(), CoreError> {
    let a = (n - 1) as f64 / 2.0 * pitch + land.x / 2.0;
    ensure(land.g / 2.0 - a >= 0.1 - 1e-9, "corner pads of adjacent sides collide; use a larger body or fewer pins")
}

fn quad_silk(b: &mut FpBuilder, body: f64, land: &Land, n: u32, pitch: f64) {
    let h = body / 2.0 + SILK_BODY_OFFSET;
    b.silk_rect(-h, -h, h, h);
    let y1 = (n - 1) as f64 / 2.0 * pitch;
    b.silk_dot(-land.z / 2.0 - dot_offset(), y1);
}

fn qfn(s: &QfnSpec, name: String, goals: LandGoals) -> Result<Footprint, CoreError> {
    let n = s.pins_per_side;
    ensure((1..=64).contains(&n), "pins per side must be 1 to 64")?;
    let pitch = positive(s.pitch, "pitch must be positive")?;
    let body = positive(s.body_size, "body size must be positive")?;
    let foot = tol(s.lead_length_min, s.lead_length_max, "lead length min/max must be positive and ordered")?;
    let width = tol(s.lead_width_min, s.lead_width_max, "lead width min/max must be positive and ordered")?;
    ensure(width.max < pitch, "lead width must be smaller than the pitch")?;
    ensure(2.0 * foot.max < body, "terminals longer than half the body")?;
    let mut land = calculate_land(Tol::pm(body, 0.05), foot, width, goals);
    land.x = land.x.min(snap(pitch - MIN_COPPER_GAP));
    let ep = s.ep();
    if s.exposed_pad {
        positive(ep, "exposed pad size must be positive")?;
        // Keep ≥ 0.2 mm between the exposed pad and the signal lands.
        land.g = land.g.max(snap(ep + 2.0 * MIN_COPPER_GAP));
    }
    check_land(&land)?;
    quad_corner_check(n, pitch, &land)?;
    ensure(s.paste_grid <= 8, "paste grid must be 0 to 8")?;

    let desc = format!("QFN-{} {b}x{b} mm body, {} mm pitch, PCBPlanner Studio editable land-pattern estimate", n * 4, num(pitch), b = num(body));
    let mut b = FpBuilder::new(name, desc);
    quad_pads(&mut b, n, pitch, &land);
    if s.exposed_pad {
        b.smd(&(n * 4 + 1).to_string(), 0.0, 0.0, ep, ep);
        if s.paste_grid >= 2 {
            let k = s.paste_grid;
            let cell = ep / k as f64;
            let ap = (cell - 0.2).max(0.2);
            for i in 0..k {
                for jj in 0..k {
                    let x = -ep / 2.0 + cell * (i as f64 + 0.5);
                    let y = -ep / 2.0 + cell * (jj as f64 + 0.5);
                    b.paste_rect(snap(x), snap(y), snap(ap), snap(ap));
                }
            }
        }
    }
    b.body(0.0, 0.0, body, body, Pin1Corner::TopLeft);
    quad_silk(&mut b, body, &land, n, pitch);
    Ok(b.build())
}

fn qfp(s: &QfpSpec, name: String, goals: LandGoals) -> Result<Footprint, CoreError> {
    ensure(s.pins >= 8 && s.pins.is_multiple_of(4) && s.pins <= 256, "pin count must be a multiple of 4, 8 to 256")?;
    let n = s.pins / 4;
    let pitch = positive(s.pitch, "pitch must be positive")?;
    let body = positive(s.body_size, "body size must be positive")?;
    let span = s.span();
    let span = tol(span.min, span.max, "lead span min/max must be positive and ordered")?;
    let foot = tol(s.lead_length_min, s.lead_length_max, "lead length min/max must be positive and ordered")?;
    let width = s.width();
    let width = tol(width.min, width.max, "lead width min/max must be positive and ordered")?;
    ensure(width.max < pitch, "lead width must be smaller than the pitch")?;
    ensure(span.min > body, "lead span must exceed the body size")?;
    let mut land = calculate_land(span, foot, width, goals);
    land.x = land.x.min(snap(pitch - MIN_COPPER_GAP));
    check_land(&land)?;
    quad_corner_check(n, pitch, &land)?;
    let desc = format!("{}-{} {b}x{b} mm body, {} mm pitch, PCBPlanner Studio editable land-pattern estimate", s.style.label(), s.pins, num(pitch), b = num(body));
    let mut b = FpBuilder::new(name, desc);
    quad_pads(&mut b, n, pitch, &land);
    b.body(0.0, 0.0, body, body, Pin1Corner::TopLeft);
    quad_silk(&mut b, body, &land, n, pitch);
    Ok(b.build())
}

fn tht_checks(pad: f64, drill: f64, pitch: f64) -> Result<(), CoreError> {
    positive(pad, "pad diameter must be positive")?;
    positive(drill, "drill must be positive")?;
    positive(pitch, "pitch must be positive")?;
    ensure(drill + 0.2 <= pad + 1e-9, "pad must be at least 0.2 mm larger than the drill")?;
    ensure(pad + MIN_COPPER_GAP <= pitch + 1e-9, "pads would touch at this pitch")
}

fn dip(s: &DipSpec, name: String) -> Result<Footprint, CoreError> {
    ensure(s.pins >= 4 && s.pins.is_multiple_of(2) && s.pins <= 128, "pin count must be even, 4 to 128")?;
    tht_checks(s.pad_diameter, s.drill, s.pitch)?;
    let row = positive(s.row_spacing, "row spacing must be positive")?;
    ensure(row >= s.pad_diameter + 1.0, "row spacing too small for the pads")?;
    let (bw, bl) = s.body();
    positive(bw, "body width must be positive")?;
    positive(bl, "body length must be positive")?;
    let n = s.pins / 2;
    let pad = s.pad_diameter;
    let desc = format!("{}-lead through-hole DIP, {} mm row spacing, {} mm pitch", s.pins, num(row), num(s.pitch));
    let mut b = FpBuilder::new(name, desc);
    let ys: Vec<f64> = row_positions(n, s.pitch).collect();
    let shape = |i: usize| if i == 0 { PadShape::Rect } else { PadShape::Circle };
    for (i, &y) in ys.iter().enumerate() {
        b.tht(&(i + 1).to_string(), -row / 2.0, y, pad, pad, shape(i), s.drill);
    }
    for (i, &y) in ys.iter().rev().enumerate() {
        b.tht(&(n as usize + i + 1).to_string(), row / 2.0, y, pad, pad, PadShape::Circle, s.drill);
    }
    b.body(0.0, 0.0, bw, bl, Pin1Corner::TopLeft);
    // Silk between the rows (or on the body edge if that is clear of the pads).
    let sx = (bw / 2.0 + SILK_BODY_OFFSET).min(row / 2.0 - pad / 2.0 - silk_keepout());
    let sy = bl / 2.0 + SILK_BODY_OFFSET;
    let notch = 1.0_f64.min(sx * 0.4);
    b.silk_line((-sx, sy), (-notch, sy));
    b.silk_line((notch, sy), (sx, sy));
    b.silk_line((sx, sy), (sx, -sy));
    b.silk_line((sx, -sy), (-sx, -sy));
    b.silk_line((-sx, -sy), (-sx, sy));
    b.silk_arc(0.0, sy, notch, 180.0, 180.0);
    Ok(b.build())
}

fn pin_header(s: &PinHeaderSpec, name: String) -> Result<Footprint, CoreError> {
    ensure((1..=3).contains(&s.rows), "rows must be 1 to 3")?;
    ensure((1..=60).contains(&s.pins_per_row), "pins per row must be 1 to 60")?;
    let (pad, drill) = s.pad_drill();
    tht_checks(pad, drill, s.pitch)?;
    let p = s.pitch;
    let desc = format!("Through-hole pin header, {}x{:02}, {:.2} mm pitch{}", s.rows, s.pins_per_row, p, if s.shrouded { ", shrouded" } else { "" });
    let mut b = FpBuilder::new(name, desc);
    let xs: Vec<f64> = row_positions(s.rows, p).rev().collect(); // left → right
    let ys: Vec<f64> = row_positions(s.pins_per_row, p).collect();
    for (r, &y) in ys.iter().enumerate() {
        for (c, &x) in xs.iter().enumerate() {
            let k = r * s.rows as usize + c + 1;
            let shape = if k == 1 { PadShape::Rect } else { PadShape::Circle };
            b.tht(&k.to_string(), x, y, pad, pad, shape, drill);
        }
    }
    let scale = p / 2.54;
    let (bw, bl) = if s.shrouded {
        ((s.rows - 1) as f64 * p + 6.4 * scale, (s.pins_per_row - 1) as f64 * p + 10.16 * scale)
    } else {
        (s.rows as f64 * p, s.pins_per_row as f64 * p)
    };
    b.body(0.0, 0.0, bw, bl, Pin1Corner::TopLeft);
    let (hx, hy) = (bw / 2.0 + SILK_BODY_OFFSET, bl / 2.0 + SILK_BODY_OFFSET);
    b.silk_rect(-hx, -hy, hx, hy);
    if s.shrouded {
        // Key notch on the left wall.
        let (k, d) = (2.25 * scale, 0.9 * scale);
        b.silk_path(&[(-hx, k), (-hx + d, k), (-hx + d, -k), (-hx, -k)], false);
    }
    Ok(b.build())
}

fn axial(s: &AxialSpec, name: String) -> Result<Footprint, CoreError> {
    validate_prefix(&s.prefix)?;
    tht_checks(s.pad_diameter, s.drill, s.pitch)?;
    let l = positive(s.body_length, "body length must be positive")?;
    let d = positive(s.body_diameter, "body diameter must be positive")?;
    let hp = s.pitch / 2.0;
    let (bx, by) = (l / 2.0 + SILK_BODY_OFFSET, d / 2.0 + SILK_BODY_OFFSET);
    ensure(hp - s.pad_diameter / 2.0 - silk_keepout() >= bx, "pitch too short for the body length (use a standing mount)")?;
    let desc = format!("Axial through-hole {}, body {}x{} mm, {} mm pitch, horizontal", s.prefix, num(l), num(d), num(s.pitch));
    let mut b = FpBuilder::new(name, desc);
    b.tht("1", -hp, 0.0, s.pad_diameter, s.pad_diameter, PadShape::Rect, s.drill);
    b.tht("2", hp, 0.0, s.pad_diameter, s.pad_diameter, PadShape::Circle, s.drill);
    b.body(0.0, 0.0, l, d, Pin1Corner::TopLeft);
    b.assembly_line((-hp, 0.0), (-l / 2.0, 0.0));
    b.assembly_line((l / 2.0, 0.0), (hp, 0.0));
    b.silk_rect(-bx, -by, bx, by);
    b.silk_line((-hp, 0.0), (-bx, 0.0));
    b.silk_line((bx, 0.0), (hp, 0.0));
    if s.polarity {
        let xb = -l / 2.0 + 0.15 * l;
        b.silk_line((xb, by), (xb, -by));
        b.silk_line((xb + SILK_BODY_OFFSET, by), (xb + SILK_BODY_OFFSET, -by));
        b.assembly_line((xb, d / 2.0), (xb, -d / 2.0));
    }
    Ok(b.build())
}

fn radial(s: &RadialSpec, name: String) -> Result<Footprint, CoreError> {
    tht_checks(s.pad_diameter, s.drill, s.pitch)?;
    let dia = positive(s.diameter, "diameter must be positive")?;
    let r = dia / 2.0;
    let hp = s.pitch / 2.0;
    ensure(hp + s.pad_diameter / 2.0 < r, "pads must lie inside the can outline; increase the diameter or reduce the pitch")?;
    let desc = format!("Radial through-hole capacitor, D{:.1} mm, {:.2} mm pitch{}", dia, s.pitch, if s.polarized { ", pad 1 = +" } else { "" });
    let mut b = FpBuilder::new(name, desc);
    b.tht("1", -hp, 0.0, s.pad_diameter, s.pad_diameter, PadShape::Rect, s.drill);
    b.tht("2", hp, 0.0, s.pad_diameter, s.pad_diameter, PadShape::Circle, s.drill);
    // Assembly: can outline with a flat ("bevel") at the pin-1 side.
    b.assembly_circle(0.0, 0.0, r);
    let at = |deg: f64| {
        let a = deg.to_radians();
        (snap(r * a.cos()), snap(r * a.sin()))
    };
    b.assembly_line(at(112.5), at(157.5));
    b.body_extent(super::builder::Box2::centered(0.0, 0.0, dia, dia));
    ensure(b.silk_circle(0.0, 0.0, r + SILK_BODY_OFFSET), "pads do not fit inside the can outline; increase the diameter or reduce the pitch")?;
    if s.polarized {
        // "+" above pad 1, inside the can.
        let x = -hp;
        let lo = s.pad_diameter / 2.0 + silk_keepout();
        let hi = ((r * r - x * x).max(0.0)).sqrt() - 0.3;
        let arm = (0.5_f64).min((hi - lo) / 2.0);
        if arm >= 0.2 {
            let cy = snap((lo + hi) / 2.0);
            let arm = snap(arm);
            b.silk_line((x - arm, cy), (x + arm, cy));
            b.silk_line((x, cy - arm), (x, cy + arm));
        }
    }
    Ok(b.build())
}

fn crystal(s: &CrystalSpec, name: String) -> Result<Footprint, CoreError> {
    match s.package {
        CrystalPackage::Hc49 => {
            let mut b = FpBuilder::new(name, "HC-49/U(S) through-hole crystal, 4.88 mm pitch");
            b.tht("1", -2.44, 0.0, 1.5, 1.5, PadShape::Rect, 0.8);
            b.tht("2", 2.44, 0.0, 1.5, 1.5, PadShape::Circle, 0.8);
            b.body(0.0, 0.0, 11.05, 4.65, Pin1Corner::TopLeft);
            let (hy, hx) = (4.65 / 2.0 + SILK_BODY_OFFSET, 11.05 / 2.0 + SILK_BODY_OFFSET);
            let ac = snap(hx - hy);
            b.silk_line((-ac, hy), (ac, hy));
            b.silk_line((-ac, -hy), (ac, -hy));
            b.silk_arc(ac, 0.0, hy, -90.0, 180.0);
            b.silk_arc(-ac, 0.0, hy, 90.0, 180.0);
            Ok(b.build())
        }
        CrystalPackage::Smd3225 | CrystalPackage::Smd2016 => {
            // (body w, body h, pad center x, pad center y, pad w, pad h)
            let (bw, bh, px, py, pw, ph) =
                if s.package == CrystalPackage::Smd3225 { (3.2, 2.5, 1.1, 0.8, 1.4, 1.15) } else { (2.0, 1.6, 0.7, 0.55, 0.95, 0.8) };
            let desc = format!("SMD ceramic crystal {}x{} mm, 4 pads (1/3 crystal, 2/4 case)", num(bw), num(bh));
            let mut b = FpBuilder::new(name, desc);
            b.smd("1", -px, -py, pw, ph);
            b.smd("2", px, -py, pw, ph);
            b.smd("3", px, py, pw, ph);
            b.smd("4", -px, py, pw, ph);
            b.body(0.0, 0.0, bw, bh, Pin1Corner::BottomLeft);
            let (hx, hy) = (bw / 2.0 + SILK_BODY_OFFSET, bh / 2.0 + SILK_BODY_OFFSET);
            b.silk_rect(-hx, -hy, hx, hy);
            b.silk_dot(-px - pw / 2.0 - dot_offset(), -py);
            Ok(b.build())
        }
    }
}

// ---------- Family descriptions for the UI ----------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ParamKind {
    Number,
    Integer,
    Bool,
    /// One of `choices`.
    Choice,
    /// Free text; `choices` are suggestions.
    Text,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParamInfo {
    /// JSON field name in the family's spec object.
    pub key: String,
    pub label: String,
    pub kind: ParamKind,
    /// "mm" for lengths.
    pub unit: Option<String>,
    /// Value used when the field is omitted (for optional fields: the value
    /// derived for the family defaults).
    pub default: Value,
    pub choices: Vec<Value>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    /// `null` is accepted and means "derive automatically".
    pub optional: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FamilyInfo {
    /// The `family` tag value.
    pub id: String,
    pub name: String,
    pub description: String,
    pub params: Vec<ParamInfo>,
    /// Complete default spec (as JSON it includes the `family` tag).
    pub default_spec: FootprintSpec,
}

struct P(ParamInfo);

impl P {
    fn new(key: &str, label: &str, kind: ParamKind) -> P {
        P(ParamInfo {
            key: key.into(),
            label: label.into(),
            kind,
            unit: None,
            default: Value::Null,
            choices: Vec::new(),
            min: None,
            max: None,
            optional: false,
        })
    }
    fn mm(key: &str, label: &str) -> P {
        let mut p = P::new(key, label, ParamKind::Number);
        p.0.unit = Some("mm".into());
        p.0.min = Some(0.0);
        p
    }
    fn int(key: &str, label: &str, min: f64, max: f64) -> P {
        let mut p = P::new(key, label, ParamKind::Integer);
        p.0.min = Some(min);
        p.0.max = Some(max);
        p
    }
    fn choice(key: &str, label: &str, choices: &[&str]) -> P {
        let mut p = P::new(key, label, ParamKind::Choice);
        p.0.choices = choices.iter().map(|c| Value::String((*c).into())).collect();
        p
    }
    fn text(key: &str, label: &str, suggestions: &[&str]) -> P {
        let mut p = P::new(key, label, ParamKind::Text);
        p.0.choices = suggestions.iter().map(|c| Value::String((*c).into())).collect();
        p
    }
    fn num_choices(mut self, v: &[f64]) -> P {
        self.0.choices = v.iter().map(|&x| Value::from(x)).collect();
        self
    }
    fn optional(mut self) -> P {
        self.0.optional = true;
        self
    }
}

fn family(id: &str, name: &str, description: &str, spec: FootprintSpec, resolved: Value, params: Vec<P>) -> FamilyInfo {
    let params = params
        .into_iter()
        .map(|P(mut p)| {
            p.default = resolved.get(&p.key).cloned().unwrap_or(Value::Null);
            p
        })
        .collect();
    FamilyInfo { id: id.into(), name: name.into(), description: description.into(), params, default_spec: spec }
}

fn to_value<T: Serialize>(v: &T) -> Value {
    serde_json::to_value(v).unwrap_or(Value::Null)
}

fn labels<T: Serialize>(all: &[T]) -> Vec<String> {
    all.iter().filter_map(|v| to_value(v).as_str().map(String::from)).collect()
}

/// Parameter descriptions of every family, for building wizard forms.
pub fn families() -> Vec<FamilyInfo> {
    let simple = |spec: FootprintSpec| (to_value(&spec), spec);
    let chip_sizes = labels(&ChipSize::ALL);
    let sods = labels(&SodPackage::ALL);
    let sots = labels(&SotPackage::ALL);
    let crystals = labels(&CrystalPackage::ALL);

    let mut out = Vec::new();

    let (v, s) = simple(FootprintSpec::Chip(ChipSpec::default()));
    out.push(family(
        "chip",
        "Chip (2-terminal)",
        "Rectangular end-cap chip resistors, capacitors, inductors, LEDs, fuses",
        s,
        v,
        vec![
            P::choice("size", "Size code (imperial)", &strs(&chip_sizes)),
            P::text("prefix", "Name prefix", &["R", "C", "L", "D", "LED", "CP", "Fuse"]),
            P::new("polarity", "Polarity mark", ParamKind::Bool),
        ],
    ));

    let (v, s) = simple(FootprintSpec::Sod(SodSpec::default()));
    out.push(family(
        "sod",
        "SOD / SMx diode",
        "Molded two-terminal diode packages; pad 1 = cathode",
        s,
        v,
        vec![P::choice("package", "Package", &strs(&sods)), P::text("prefix", "Name prefix", &["D", "LED"])],
    ));

    let (v, s) = simple(FootprintSpec::Sot(SotSpec::default()));
    out.push(family("sot", "SOT", "Small-outline transistor packages", s, v, vec![P::choice("package", "Package", &strs(&sots))]));

    for (fam, id, name, desc, pitches) in [
        (DualFamily::Soic, "soic", "SOIC", "Small-outline IC, gull-wing leads", &[1.27][..]),
        (DualFamily::Ssop, "ssop", "SSOP", "Shrink small-outline package", &[0.65, 0.5][..]),
        (DualFamily::Tssop, "tssop", "TSSOP", "Thin shrink small-outline package", &[0.65, 0.5][..]),
    ] {
        let spec = DualRowSpec::default();
        let resolved = to_value(&spec.resolve(fam));
        let fs = match fam {
            DualFamily::Soic => FootprintSpec::Soic(spec),
            DualFamily::Ssop => FootprintSpec::Ssop(spec),
            DualFamily::Tssop => FootprintSpec::Tssop(spec),
        };
        out.push(family(
            id,
            name,
            desc,
            fs,
            resolved,
            vec![
                P::int("pins", "Pins", 4.0, 128.0).optional(),
                P::mm("pitch", "Pitch").num_choices(pitches).optional(),
                P::mm("bodyWidth", "Body width (E1)").optional(),
                P::mm("bodyLength", "Body length (D)").optional(),
                P::mm("leadSpanMin", "Lead span min (E)").optional(),
                P::mm("leadSpanMax", "Lead span max (E)").optional(),
                P::mm("leadLengthMin", "Foot length min (L)").optional(),
                P::mm("leadLengthMax", "Foot length max (L)").optional(),
                P::mm("leadWidthMin", "Lead width min (b)").optional(),
                P::mm("leadWidthMax", "Lead width max (b)").optional(),
            ],
        ));
    }

    let qfn_spec = QfnSpec::default();
    let mut v = to_value(&qfn_spec);
    v["epSize"] = Value::from(qfn_spec.ep());
    out.push(family(
        "qfn",
        "QFN",
        "Quad flat no-lead with optional exposed pad",
        FootprintSpec::Qfn(qfn_spec),
        v,
        vec![
            P::int("pinsPerSide", "Pins per side", 1.0, 64.0),
            P::mm("pitch", "Pitch").num_choices(&[0.4, 0.5, 0.65]),
            P::mm("bodySize", "Body size"),
            P::mm("leadLengthMin", "Terminal length min (L)"),
            P::mm("leadLengthMax", "Terminal length max (L)"),
            P::mm("leadWidthMin", "Terminal width min (b)"),
            P::mm("leadWidthMax", "Terminal width max (b)"),
            P::new("exposedPad", "Exposed pad", ParamKind::Bool),
            P::mm("epSize", "Exposed pad size").optional(),
            P::int("pasteGrid", "EP paste grid (N×N, 0 = none)", 0.0, 8.0),
        ],
    ));

    let qfp_spec = QfpSpec::default();
    let mut v = to_value(&qfp_spec);
    let (span, width) = (qfp_spec.span(), qfp_spec.width());
    v["leadSpanMin"] = Value::from(span.min);
    v["leadSpanMax"] = Value::from(span.max);
    v["leadWidthMin"] = Value::from(width.min);
    v["leadWidthMax"] = Value::from(width.max);
    out.push(family(
        "qfp",
        "LQFP / TQFP",
        "Quad flat package with gull-wing leads",
        FootprintSpec::Qfp(qfp_spec),
        v,
        vec![
            P::choice("style", "Style", &["LQFP", "TQFP"]),
            P::int("pins", "Pins (total)", 8.0, 256.0),
            P::mm("pitch", "Pitch").num_choices(&[0.4, 0.5, 0.65, 0.8]),
            P::mm("bodySize", "Body size"),
            P::mm("leadSpanMin", "Lead span min").optional(),
            P::mm("leadSpanMax", "Lead span max").optional(),
            P::mm("leadLengthMin", "Foot length min"),
            P::mm("leadLengthMax", "Foot length max"),
            P::mm("leadWidthMin", "Lead width min").optional(),
            P::mm("leadWidthMax", "Lead width max").optional(),
        ],
    ));

    let dip_spec = DipSpec::default();
    let mut v = to_value(&dip_spec);
    let (bw, bl) = dip_spec.body();
    v["bodyWidth"] = Value::from(bw);
    v["bodyLength"] = Value::from(bl);
    out.push(family(
        "dip",
        "DIP",
        "Through-hole dual in-line package; pin 1 square pad",
        FootprintSpec::Dip(dip_spec),
        v,
        vec![
            P::int("pins", "Pins", 4.0, 128.0),
            P::mm("rowSpacing", "Row spacing").num_choices(&[7.62, 10.16, 15.24]),
            P::mm("pitch", "Pitch").num_choices(&[2.54, 1.778]),
            P::mm("padDiameter", "Pad diameter"),
            P::mm("drill", "Drill"),
            P::mm("bodyWidth", "Body width").optional(),
            P::mm("bodyLength", "Body length").optional(),
        ],
    ));

    let ph = PinHeaderSpec::default();
    let mut v = to_value(&ph);
    let (pad, drill) = ph.pad_drill();
    v["padDiameter"] = Value::from(pad);
    v["drill"] = Value::from(drill);
    out.push(family(
        "pinHeader",
        "Pin header",
        "Through-hole pin header / box header; pin 1 square pad",
        FootprintSpec::PinHeader(ph),
        v,
        vec![
            P::int("rows", "Rows (side by side)", 1.0, 3.0),
            P::int("pinsPerRow", "Pins per row", 1.0, 60.0),
            P::mm("pitch", "Pitch").num_choices(&[2.54, 2.0, 1.27]),
            P::mm("padDiameter", "Pad diameter").optional(),
            P::mm("drill", "Drill").optional(),
            P::new("shrouded", "Shrouded (box header)", ParamKind::Bool),
        ],
    ));

    let (v, s) = simple(FootprintSpec::Axial(AxialSpec::default()));
    out.push(family(
        "axial",
        "Axial THT",
        "Horizontal axial two-lead part; pad 1 square (cathode when polarized)",
        s,
        v,
        vec![
            P::text("prefix", "Name prefix", &["R", "D", "C", "L"]),
            P::mm("pitch", "Pitch").num_choices(&[7.62, 10.16, 12.7, 15.24]),
            P::mm("bodyLength", "Body length"),
            P::mm("bodyDiameter", "Body diameter"),
            P::mm("padDiameter", "Pad diameter"),
            P::mm("drill", "Drill"),
            P::new("polarity", "Polarity band", ParamKind::Bool),
        ],
    ));

    let (v, s) = simple(FootprintSpec::Radial(RadialSpec::default()));
    out.push(family(
        "radial",
        "Radial THT",
        "Radial can (electrolytic capacitor); pad 1 = +",
        s,
        v,
        vec![
            P::mm("diameter", "Can diameter").num_choices(&[4.0, 5.0, 6.3, 8.0, 10.0]),
            P::mm("pitch", "Pitch").num_choices(&[1.5, 2.0, 2.5, 3.5, 5.0]),
            P::mm("padDiameter", "Pad diameter"),
            P::mm("drill", "Drill"),
            P::new("polarized", "Polarized", ParamKind::Bool),
        ],
    ));

    let (v, s) = simple(FootprintSpec::Crystal(CrystalSpec::default()));
    out.push(family("crystal", "Crystal", "HC-49 through-hole or 4-pad SMD ceramic crystal", s, v, vec![P::choice("package", "Package", &strs(&crystals))]));

    out
}

fn strs(v: &[String]) -> Vec<&str> {
    v.iter().map(String::as_str).collect()
}
