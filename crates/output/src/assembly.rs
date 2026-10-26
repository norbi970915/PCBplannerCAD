//! Assembly outputs: pick-and-place (centroid) and bill of materials CSV.
//!
//! Pick-and-place coordinate conventions:
//! - X/Y are the footprint origin in mm relative to `BoardSettings::origin`,
//!   Y up, seen from the top (bottom-side parts are *not* X-negated).
//! - Rotation is `PlacedFootprint::rotation` normalized to [0, 360): degrees
//!   counter-clockwise as seen from the top. For bottom parts it is the
//!   rotation applied after the footprint was mirrored onto the bottom side
//!   (mirror across the footprint's local Y axis, then rotate).
//! - Bottom parts are listed with Side = Bottom (`bottom` in coordinate CSV).
//!
//! Footprints whose parameters mark them "do not populate" (`DNP`,
//! `Do Not Populate` or `Exclude From BOM` set to 1/true/yes/y/x/dnp) are
//! left out of both files. Rows are sorted by designator in natural order
//! (R2 before R10).

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fmt::Write as _;

use pcbplannercad_core::document::Document;
use pcbplannercad_core::geom::normalize_deg;
use pcbplannercad_core::item::Item;
use pcbplannercad_core::pcb::geometry::board_settings;
use pcbplannercad_core::pcb::{PlacedFootprint, Side};
use pcbplannercad_core::units::{Coord, Point};
use serde::{Deserialize, Serialize};

use crate::gerber::{div_round, fmt_fixed};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PnpFormat {
    /// `Designator,Value,Footprint,X (mm),Y (mm),Rotation,Side`
    Generic,
    /// JLCPCB CPL: `Designator,Mid X,Mid Y,Layer,Rotation`
    Jlc,
    /// Coordinate `.pos` CSV: `Ref,Val,Package,PosX,PosY,Rot,Side`
    #[serde(alias = "kicad")]
    Coordinates,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BomFormat {
    /// `Item,Quantity,Designator,Value,Footprint,Manufacturer,MPN,LCSC`
    Generic,
    /// JLCPCB: `Comment,Designator,Footprint,LCSC Part #`
    Jlc,
}

/// Pick-and-place CSV. `bottom`: `Some(false)` top only, `Some(true)` bottom
/// only, `None` both sides.
pub fn pick_and_place_csv(doc: &Document, format: PnpFormat, bottom: Option<bool>) -> String {
    let origin = board_settings(doc).map_or(Point::default(), |(_, b)| b.origin);
    let mut parts: Vec<&PlacedFootprint> = placed(doc).into_iter().filter(|f| bottom.is_none_or(|b| (f.side == Side::Bottom) == b)).collect();
    parts.sort_by(|a, b| natural_cmp(&a.designator, &b.designator));
    let mut s = String::new();
    s.push_str(match format {
        PnpFormat::Generic => "Designator,Value,Footprint,X (mm),Y (mm),Rotation,Side\n",
        PnpFormat::Jlc => "Designator,Mid X,Mid Y,Layer,Rotation\n",
        PnpFormat::Coordinates => "Ref,Val,Package,PosX,PosY,Rot,Side\n",
    });
    for f in parts {
        let (x, y) = (mm4(f.pos.x - origin.x), mm4(f.pos.y - origin.y));
        let rot = normalize_deg(f.rotation);
        let bottom = f.side == Side::Bottom;
        let side = if bottom { "Bottom" } else { "Top" };
        let _ = match format {
            PnpFormat::Generic => writeln!(s, "{},{},{},{x},{y},{},{side}", csv(&f.designator), csv(&f.value), csv(footprint_name(&f.footprint)), fmt_rot(rot)),
            PnpFormat::Jlc => writeln!(s, "{},{x}mm,{y}mm,{side},{}", csv(&f.designator), fmt_rot(rot)),
            PnpFormat::Coordinates => writeln!(
                s,
                "{},{},{},{x},{y},{rot:.4},{}",
                quoted(&f.designator),
                quoted(&f.value),
                quoted(footprint_name(&f.footprint)),
                if bottom { "bottom" } else { "top" }
            ),
        };
    }
    s
}

/// Bill of materials grouped by value + footprint + MPN.
pub fn bom_csv(doc: &Document, format: BomFormat) -> String {
    struct Group<'a> {
        value: &'a str,
        footprint: &'a str,
        mpn: &'a str,
        manufacturer: &'a str,
        lcsc: &'a str,
        designators: Vec<&'a str>,
    }
    let mut groups: BTreeMap<(&str, &str, &str), Group> = BTreeMap::new();
    for f in placed(doc) {
        let (value, footprint, mpn) = (f.value.as_str(), footprint_name(&f.footprint), param(f, MPN_KEYS));
        let g = groups.entry((value, footprint, mpn)).or_insert_with(|| Group { value, footprint, mpn, manufacturer: "", lcsc: "", designators: Vec::new() });
        if g.manufacturer.is_empty() {
            g.manufacturer = param(f, MANUFACTURER_KEYS);
        }
        if g.lcsc.is_empty() {
            g.lcsc = param(f, LCSC_KEYS);
        }
        g.designators.push(&f.designator);
    }
    let mut rows: Vec<Group> = groups.into_values().collect();
    for g in &mut rows {
        g.designators.sort_by(|a, b| natural_cmp(a, b));
    }
    rows.sort_by(|a, b| natural_cmp(a.designators[0], b.designators[0]));
    let mut s = String::new();
    match format {
        BomFormat::Generic => {
            s.push_str("Item,Quantity,Designator,Value,Footprint,Manufacturer,MPN,LCSC\n");
            for (i, g) in rows.iter().enumerate() {
                let _ = writeln!(
                    s,
                    "{},{},{},{},{},{},{},{}",
                    i + 1,
                    g.designators.len(),
                    csv(&g.designators.join(", ")),
                    csv(g.value),
                    csv(g.footprint),
                    csv(g.manufacturer),
                    csv(g.mpn),
                    csv(g.lcsc)
                );
            }
        }
        BomFormat::Jlc => {
            s.push_str("Comment,Designator,Footprint,LCSC Part #\n");
            for g in &rows {
                let _ = writeln!(s, "{},{},{},{}", csv(g.value), csv(&g.designators.join(",")), csv(g.footprint), csv(g.lcsc));
            }
        }
    }
    s
}

const MPN_KEYS: &[&str] = &["mpn", "manufacturerpartnumber", "mfrpartnumber", "mfrpn", "partnumber"];
const MANUFACTURER_KEYS: &[&str] = &["manufacturer", "mfr", "manufacturername"];
const LCSC_KEYS: &[&str] = &["lcsc", "lcscpart", "lcscpartnumber", "lcscpn", "jlcpcbpart", "jlcpcbpartnumber", "jlc"];
const DNP_KEYS: &[&str] = &["dnp", "donotpopulate", "excludefrombom"];

fn norm_key(k: &str) -> String {
    k.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

/// First parameter whose normalized name is in `keys` (in `keys` order).
fn param<'a>(f: &'a PlacedFootprint, keys: &[&str]) -> &'a str {
    keys.iter()
        .find_map(|k| f.parameters.iter().find(|(name, v)| norm_key(name) == *k && !v.trim().is_empty()).map(|(_, v)| v.trim()))
        .unwrap_or("")
}

fn is_dnp(f: &PlacedFootprint) -> bool {
    f.parameters.iter().any(|(k, v)| DNP_KEYS.contains(&norm_key(k).as_str()) && matches!(v.trim().to_lowercase().as_str(), "1" | "true" | "yes" | "y" | "x" | "dnp"))
}

fn placed(doc: &Document) -> Vec<&PlacedFootprint> {
    doc.items().filter_map(|(_, i)| if let Item::Footprint(f) = i { Some(f) } else { None }).filter(|f| !is_dnp(f)).collect()
}

/// "Library/Name" → "Name".
fn footprint_name(key: &str) -> &str {
    key.rsplit('/').next().unwrap_or(key)
}

fn mm4(c: Coord) -> String {
    fmt_fixed(div_round(c, 100), 4)
}

fn fmt_rot(r: f64) -> String {
    let s = format!("{r:.3}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() { "0".into() } else { s.to_string() }
}

/// CSV field, quoted only when needed.
fn csv(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) || s.starts_with(' ') || s.ends_with(' ') { quoted(s) } else { s.to_string() }
}

fn quoted(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

/// Natural ordering: digit runs compare numerically ("R2" < "R10").
pub(crate) fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (mut x, mut y) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (x.peek().copied(), y.peek().copied()) {
            (None, None) => return a.cmp(b),
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(c), Some(d)) if c.is_ascii_digit() && d.is_ascii_digit() => {
                let take = |it: &mut std::iter::Peekable<std::str::Chars>| {
                    let mut s = String::new();
                    while let Some(c) = it.peek().copied().filter(char::is_ascii_digit) {
                        s.push(c);
                        it.next();
                    }
                    s
                };
                let (n, m) = (take(&mut x), take(&mut y));
                let (n2, m2) = (n.trim_start_matches('0'), m.trim_start_matches('0'));
                let o = n2.len().cmp(&m2.len()).then_with(|| n2.cmp(m2));
                if o != Ordering::Equal {
                    return o;
                }
            }
            (Some(c), Some(d)) => {
                let o = c.to_ascii_lowercase().cmp(&d.to_ascii_lowercase());
                if o != Ordering::Equal {
                    return o;
                }
                x.next();
                y.next();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gerber::test_board::{board_doc, place_full};
    use pcbplannercad_core::pcb::BoardSettings;
    use pcbplannercad_core::units::mm;

    fn doc() -> Document {
        let board = BoardSettings { origin: Point::new(mm(10.0), mm(5.0)), ..Default::default() };
        let items = vec![
            Item::Board(board),
            place_full("R10", "10k", "Lib/R_0603", Point::new(mm(15.0), mm(7.0)), 90.0, Side::Top, &[("MPN", "RC0603-10K")]),
            place_full("R2", "10k", "Lib/R_0603", Point::new(mm(12.5), mm(4.0)), -90.0, Side::Bottom, &[("MPN", "RC0603-10K"), ("LCSC", "C25804")]),
            place_full("R1", "10k", "Lib/R_0603", Point::new(mm(11.0), mm(5.0)), 0.0, Side::Top, &[("MPN", "OTHER-10K")]),
            place_full("C1", "100n, 16V", "Lib/C_0402", Point::new(mm(20.0), mm(5.0)), 450.0, Side::Top, &[("Manufacturer", "Murata"), ("Mfr Part Number", "GRM155")]),
            place_full("C2", "100n, 16V", "Lib/C_0402", Point::new(mm(21.0), mm(5.0)), 0.0, Side::Top, &[("Manufacturer", "Murata"), ("Mfr Part Number", "GRM155")]),
            place_full("TP1", "", "Lib/TP", Point::new(mm(1.0), mm(1.0)), 0.0, Side::Top, &[("DNP", "yes")]),
        ];
        // board_doc adds its own default settings first; drop it so ours is found.
        let d = board_doc(items, &[]);
        let items: Vec<Item> = d.item_list().into_iter().skip(1).collect();
        Document::from_item_list(d.kind(), items)
    }

    #[test]
    fn pnp_generic_with_origin_offset_and_bottom_side() {
        let s = pick_and_place_csv(&doc(), PnpFormat::Generic, None);
        let lines: Vec<&str> = s.lines().collect();
        assert_eq!(lines[0], "Designator,Value,Footprint,X (mm),Y (mm),Rotation,Side");
        assert_eq!(
            &lines[1..],
            &[
                "C1,\"100n, 16V\",C_0402,10.0000,0.0000,90,Top",
                "C2,\"100n, 16V\",C_0402,11.0000,0.0000,0,Top",
                "R1,10k,R_0603,1.0000,0.0000,0,Top",
                "R2,10k,R_0603,2.5000,-1.0000,270,Bottom",
                "R10,10k,R_0603,5.0000,2.0000,90,Top",
            ]
        );
        let bottom = pick_and_place_csv(&doc(), PnpFormat::Generic, Some(true));
        assert_eq!(bottom.lines().count(), 2);
        assert!(bottom.contains("R2,"));
        let top = pick_and_place_csv(&doc(), PnpFormat::Generic, Some(false));
        assert_eq!(top.lines().count(), 5);
    }

    #[test]
    fn pnp_jlc_and_coordinate_formats() {
        let s = pick_and_place_csv(&doc(), PnpFormat::Jlc, None);
        assert!(s.starts_with("Designator,Mid X,Mid Y,Layer,Rotation\n"));
        assert!(s.contains("\nR2,2.5000mm,-1.0000mm,Bottom,270\n"));
        let k = pick_and_place_csv(&doc(), PnpFormat::Coordinates, None);
        assert!(k.starts_with("Ref,Val,Package,PosX,PosY,Rot,Side\n"));
        assert!(k.contains("\n\"R2\",\"10k\",\"R_0603\",2.5000,-1.0000,270.0000,bottom\n"));
    }

    #[test]
    fn legacy_coordinate_format_option_remains_readable() {
        let old: PnpFormat = serde_json::from_str(r#""kicad""#).unwrap();
        assert_eq!(old, PnpFormat::Coordinates);
        assert_eq!(serde_json::to_string(&old).unwrap(), r#""coordinates""#);
        assert_eq!(pick_and_place_csv(&doc(), old, None), pick_and_place_csv(&doc(), PnpFormat::Coordinates, None));
    }

    #[test]
    fn bom_groups_by_value_footprint_and_mpn() {
        let s = bom_csv(&doc(), BomFormat::Generic);
        let lines: Vec<&str> = s.lines().collect();
        assert_eq!(lines[0], "Item,Quantity,Designator,Value,Footprint,Manufacturer,MPN,LCSC");
        assert_eq!(
            &lines[1..],
            &["1,2,\"C1, C2\",\"100n, 16V\",C_0402,Murata,GRM155,", "2,1,R1,10k,R_0603,,OTHER-10K,", "3,2,\"R2, R10\",10k,R_0603,,RC0603-10K,C25804",]
        );
        let j = bom_csv(&doc(), BomFormat::Jlc);
        assert!(j.starts_with("Comment,Designator,Footprint,LCSC Part #\n"));
        assert!(j.contains("\n10k,\"R2,R10\",R_0603,C25804\n"));
        assert!(!j.contains("TP1"));
    }

    #[test]
    fn natural_order() {
        let mut v = vec!["R10", "R2", "C1", "R1", "U1A", "r3"];
        v.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(v, vec!["C1", "R1", "R2", "r3", "R10", "U1A"]);
    }
}
