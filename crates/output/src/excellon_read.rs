//! Excellon drill reader (part of [`super`]): holes become dark circles,
//! G85 and routed (M15/M16) slots become strokes of the tool diameter.

use std::collections::HashMap;

use pcbplannercad_core::{Coord, Point};

use super::{GerberFile, GerberShape, MAX_SHAPES, MAX_WARNINGS, clamp_nm};
use crate::OutputError;

struct Excellon {
    unit_nm: f64,
    int_digits: u32,
    dec_digits: u32,
    /// Leading zeros kept (trailing omitted) — `LZ`.
    leading_kept: bool,
    format_explicit: bool,
    /// Integers are plain decimal numbers (structured format "FORMAT=... decimal").
    decimal: bool,
    incremental: bool,
    tools: HashMap<u32, f64>,
    tool: Option<u32>,
    pos: (f64, f64),
    rout: bool,
    plunged: bool,
    shapes: Vec<GerberShape>,
    warnings: Vec<String>,
    function: Option<String>,
}

/// Parses an Excellon (XNC) drill file. Supports METRIC/INCH (and M71/M72),
/// LZ/TZ zero suppression, `FILE_FORMAT=i:d` / `METRIC,000.000` digit counts,
/// decimal coordinates, tool tables (`T01C0.8`), G85 slots, routed slots
/// (G00 + M15, G01/G02/G03 with A or I/J, M16/M17), R repeats, G90/G91.
pub fn parse_excellon(name: &str, text: &str) -> Result<GerberFile, OutputError> {
    let looks = text.lines().any(|l| {
        let l = l.trim();
        l == "M48" || l.starts_with("METRIC") || l.starts_with("INCH") || l.starts_with('T') || l.starts_with('X') || l.starts_with('Y')
    });
    if !looks {
        return Err(OutputError::Invalid(format!("{name}: not an Excellon drill file")));
    }
    let mut x = Excellon {
        unit_nm: 25.4e6,
        int_digits: 2,
        dec_digits: 4,
        leading_kept: false,
        format_explicit: false,
        decimal: false,
        incremental: false,
        tools: HashMap::new(),
        tool: None,
        pos: (0.0, 0.0),
        rout: false,
        plunged: false,
        shapes: Vec::new(),
        warnings: Vec::new(),
        function: None,
    };
    let mut units_seen = false;
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(c) = line.strip_prefix(';') {
            let c = c.trim();
            if let Some(v) = c.strip_prefix("#@!").map(str::trim).and_then(|a| a.strip_prefix("TF.FileFunction,")) {
                x.function = Some(v.trim().to_string());
            } else if c.starts_with("FORMAT=") && c.contains("decimal") {
                x.decimal = true;
            } else if let Some((i, d)) = c.strip_prefix("FILE_FORMAT=").and_then(|f| f.split_once(':'))
                && let (Ok(i), Ok(d)) = (i.trim().parse::<u32>(), d.trim().parse::<u32>())
            {
                x.int_digits = i.min(9);
                x.dec_digits = d.min(9);
                x.format_explicit = true;
            }
            continue;
        }
        let upper = line.to_ascii_uppercase();
        if upper.starts_with("METRIC") || upper.starts_with("INCH") || upper == "M71" || upper == "M72" {
            x.units(&upper);
            units_seen = true;
            continue;
        }
        match upper.as_str() {
            "M48" | "%" | "M95" => continue,
            "G05" | "G81" => {
                x.rout = false;
                continue;
            }
            "M16" | "M17" => {
                x.plunged = false;
                continue;
            }
            "M15" => {
                x.plunged = true;
                continue;
            }
            "M30" | "M00" | "M02" => break,
            "G90" => {
                x.incremental = false;
                continue;
            }
            "G91" => {
                x.incremental = true;
                continue;
            }
            _ => {}
        }
        if upper.starts_with("FMAT") || upper.starts_with("ICI") || upper.starts_with("VER") || upper.starts_with("DETECT") || upper.starts_with("ATC") {
            continue;
        }
        x.command(&upper);
        if x.shapes.len() >= MAX_SHAPES {
            x.warn("too many holes; output truncated");
            break;
        }
    }
    if !units_seen {
        x.warn("no METRIC/INCH header; assuming inch");
    }
    let mut bounds = None;
    x.shapes.iter().for_each(|s| s.extend_bounds(&mut bounds));
    Ok(GerberFile { name: name.to_string(), function: x.function, shapes: x.shapes, warnings: x.warnings, bounds })
}

impl Excellon {
    fn warn(&mut self, msg: impl Into<String>) {
        let msg = msg.into();
        if self.warnings.len() < MAX_WARNINGS && !self.warnings.contains(&msg) {
            self.warnings.push(msg);
        }
    }

    fn units(&mut self, s: &str) {
        let metric = s.starts_with("METRIC") || s == "M71";
        self.unit_nm = if metric { 1e6 } else { 25.4e6 };
        if !self.format_explicit {
            (self.int_digits, self.dec_digits) = if metric { (3, 3) } else { (2, 4) };
        }
        for part in s.split(',').skip(1) {
            match part.trim() {
                "LZ" => self.leading_kept = true,
                "TZ" => self.leading_kept = false,
                p if p.contains('.') && p.bytes().all(|c| c == b'0' || c == b'.') => {
                    let (i, d) = p.split_once('.').unwrap_or((p, ""));
                    self.int_digits = i.len() as u32;
                    self.dec_digits = d.len() as u32;
                    self.format_explicit = true;
                }
                _ => {}
            }
        }
    }

    fn coord(&mut self, s: &str) -> Option<f64> {
        let v = if s.contains('.') || self.decimal {
            s.parse::<f64>().ok()
        } else {
            let (neg, digits) = match s.as_bytes().first() {
                Some(b'-') => (true, &s[1..]),
                Some(b'+') => (false, &s[1..]),
                _ => (false, s),
            };
            if digits.is_empty() || !digits.bytes().all(|c| c.is_ascii_digit()) {
                None
            } else {
                let total = (self.int_digits + self.dec_digits) as usize;
                let mut d = digits.to_string();
                if self.leading_kept && d.len() < total {
                    d.extend(std::iter::repeat_n('0', total - d.len()));
                }
                d.parse::<f64>().ok().map(|v| {
                    let v = v / 10f64.powi(self.dec_digits as i32);
                    if neg { -v } else { v }
                })
            }
        };
        match v {
            Some(v) if v.is_finite() => Some(clamp_nm(v * self.unit_nm)),
            _ => {
                self.warn(format!("invalid coordinate '{s}'"));
                None
            }
        }
    }

    /// Splits `X1.0Y2.0G85X3Y4` into (letter, value) pairs.
    fn fields(s: &str) -> Vec<(u8, String)> {
        let b = s.as_bytes();
        let mut out = Vec::new();
        let mut i = 0;
        while i < b.len() {
            let letter = b[i];
            i += 1;
            let st = i;
            while i < b.len() && (b[i].is_ascii_digit() || matches!(b[i], b'+' | b'-' | b'.')) {
                i += 1;
            }
            out.push((letter, std::str::from_utf8(&b[st..i]).unwrap_or("").to_string()));
        }
        out
    }

    fn diameter(&mut self) -> f64 {
        match self.tool {
            Some(t) => match self.tools.get(&t) {
                Some(d) => *d,
                None => {
                    self.warn(format!("tool T{t} has no diameter"));
                    0.0
                }
            },
            None => {
                self.warn("hole without a selected tool");
                0.0
            }
        }
    }

    fn target(&mut self, x: Option<&str>, y: Option<&str>) -> (f64, f64) {
        let fx = x.and_then(|s| self.coord(s));
        let fy = y.and_then(|s| self.coord(s));
        if self.incremental {
            (self.pos.0 + fx.unwrap_or(0.0), self.pos.1 + fy.unwrap_or(0.0))
        } else {
            (fx.unwrap_or(self.pos.0), fy.unwrap_or(self.pos.1))
        }
    }

    fn command(&mut self, line: &str) {
        let fields = Self::fields(line);
        let Some(&(first, ref first_val)) = fields.first() else { return };
        match first {
            b'T' => {
                let num: u32 = first_val.split('.').next().unwrap_or("").parse().unwrap_or(0);
                let mut dia = None;
                for (l, v) in &fields[1..] {
                    if *l == b'C' {
                        dia = v.parse::<f64>().ok().filter(|d| d.is_finite() && *d >= 0.0);
                    }
                }
                if let Some(d) = dia {
                    self.tools.insert(num, clamp_nm(d * self.unit_nm));
                }
                if num == 0 {
                    self.tool = None;
                } else {
                    self.tool = Some(num);
                }
                return;
            }
            b'R' => {
                let n: u32 = first_val.parse().unwrap_or(0);
                let get = |c: u8| fields.iter().find(|(l, _)| *l == c).map(|(_, v)| v.clone());
                let dx = get(b'X').and_then(|v| self.coord(&v)).unwrap_or(0.0);
                let dy = get(b'Y').and_then(|v| self.coord(&v)).unwrap_or(0.0);
                let d = self.diameter();
                for _ in 0..n.min(100_000) {
                    self.pos = (self.pos.0 + dx, self.pos.1 + dy);
                    self.hole(self.pos, d);
                }
                return;
            }
            _ => {}
        }
        // Coordinate block, optionally with G codes (G00/G01/G02/G03/G85).
        let mut g: Option<u32> = None;
        let mut first_xy: (Option<String>, Option<String>) = (None, None);
        let mut second_xy: (Option<String>, Option<String>) = (None, None);
        let (mut a, mut i, mut j) = (None, None, None);
        let mut slot = false;
        for (l, v) in &fields {
            match l {
                b'G' => {
                    let code = v.parse::<u32>().unwrap_or(u32::MAX);
                    if code == 85 {
                        slot = true;
                    } else {
                        g = Some(code);
                    }
                }
                b'X' if slot => second_xy.0 = Some(v.clone()),
                b'Y' if slot => second_xy.1 = Some(v.clone()),
                b'X' => first_xy.0 = Some(v.clone()),
                b'Y' => first_xy.1 = Some(v.clone()),
                b'A' => a = Some(v.clone()),
                b'I' => i = Some(v.clone()),
                b'J' => j = Some(v.clone()),
                b'M' | b'F' | b'S' | b'Z' => {}
                other => {
                    self.warn(format!("unknown Excellon command '{line}' ({})", *other as char));
                    return;
                }
            }
        }
        match g {
            Some(0) => {
                self.rout = true;
                self.plunged = false;
            }
            Some(1..=3) => self.rout = true,
            Some(5) | Some(81) => self.rout = false,
            Some(90) => self.incremental = false,
            Some(91) => self.incremental = true,
            Some(4) | None => {}
            Some(_) => self.warn(format!("unsupported Excellon G code in '{line}'")),
        }
        if first_xy.0.is_none() && first_xy.1.is_none() {
            return;
        }
        let start = self.pos;
        let p = self.target(first_xy.0.as_deref(), first_xy.1.as_deref());
        let d = self.diameter();
        if slot {
            self.pos = p;
            let e = self.target(second_xy.0.as_deref(), second_xy.1.as_deref());
            self.pos = e;
            self.shapes.push(GerberShape::Stroke { a: pt(p), b: pt(e), width: d.round() as Coord });
            return;
        }
        if self.rout {
            match g {
                Some(1) | Some(2) | Some(3) if self.plunged => {
                    if g == Some(1) {
                        self.shapes.push(GerberShape::Stroke { a: pt(start), b: pt(p), width: d.round() as Coord });
                    } else {
                        let cw = g == Some(2);
                        let center = match (a, i, j) {
                            (Some(r), _, _) => self.coord(&r).and_then(|r| arc_center(start, p, r, cw)),
                            (None, i, j) if i.is_some() || j.is_some() => {
                                let ci = i.and_then(|v| self.coord(&v)).unwrap_or(0.0);
                                let cj = j.and_then(|v| self.coord(&v)).unwrap_or(0.0);
                                Some((start.0 + ci, start.1 + cj))
                            }
                            _ => None,
                        };
                        match center {
                            Some(c) => {
                                let a0 = (start.1 - c.1).atan2(start.0 - c.0).to_degrees();
                                let a1 = (p.1 - c.1).atan2(p.0 - c.0).to_degrees();
                                let mut sweep = if cw { -(a0 - a1).rem_euclid(360.0) } else { (a1 - a0).rem_euclid(360.0) };
                                if sweep == 0.0 && start == p {
                                    sweep = if cw { -360.0 } else { 360.0 };
                                }
                                let r = ((start.0 - c.0).powi(2) + (start.1 - c.1).powi(2)).sqrt();
                                self.shapes.push(GerberShape::ArcStroke {
                                    center: pt(c),
                                    radius: r.round() as Coord,
                                    start_angle: a0,
                                    sweep,
                                    width: d.round() as Coord,
                                });
                            }
                            None => {
                                self.warn("routed arc without radius or center; drawn straight");
                                self.shapes.push(GerberShape::Stroke { a: pt(start), b: pt(p), width: d.round() as Coord });
                            }
                        }
                    }
                }
                Some(1) | Some(2) | Some(3) => {
                    // Rout move without M15: structured format-style "G00 → G01" slots
                    // sometimes omit the plunge; treat G01 as a cut anyway.
                    self.shapes.push(GerberShape::Stroke { a: pt(start), b: pt(p), width: d.round() as Coord });
                }
                _ => {}
            }
            self.pos = p;
            return;
        }
        self.pos = p;
        self.hole(p, d);
    }

    fn hole(&mut self, p: (f64, f64), d: f64) {
        self.shapes.push(GerberShape::Circle { center: pt(p), radius: (d / 2.0).round() as Coord, dark: true });
    }
}

fn pt(p: (f64, f64)) -> Point {
    Point::new(clamp_nm(p.0).round() as Coord, clamp_nm(p.1).round() as Coord)
}

/// Center of the arc of radius `r` from `s` to `e` (shorter arc for r > 0).
fn arc_center(s: (f64, f64), e: (f64, f64), r: f64, cw: bool) -> Option<(f64, f64)> {
    let (dx, dy) = (e.0 - s.0, e.1 - s.1);
    let d = (dx * dx + dy * dy).sqrt();
    if d == 0.0 || r.abs() < d / 2.0 - 1.0 {
        return None;
    }
    let h = (r * r - d * d / 4.0).max(0.0).sqrt();
    let m = ((s.0 + e.0) / 2.0, (s.1 + e.1) / 2.0);
    let (ux, uy) = (-dy / d, dx / d);
    // CCW short arc: center on the left of s→e.
    let left = !cw ^ (r < 0.0);
    let sign = if left { 1.0 } else { -1.0 };
    Some((m.0 + sign * h * ux, m.1 + sign * h * uy))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metric_decimal_holes_and_slots() {
        let src = "M48\n; FORMAT={-:-/ absolute / metric / decimal}\n; #@! TF.FileFunction,Plated,1,2,PTH\nFMAT,2\nMETRIC\nT1C0.300\nT2C1.000\n%\nG90\nG05\nT1\nX4.9Y33.08\nX5.0\nT2\nX1Y1G85X3Y1\n\
                   G00X10Y10\nM15\nG01X12Y10\nG03X14Y12A2\nM16\nG05\nM30\n";
        let f = parse_excellon("a.drl", src).unwrap();
        assert_eq!(f.function.as_deref(), Some("Plated,1,2,PTH"));
        assert!(f.warnings.is_empty(), "{:?}", f.warnings);
        assert_eq!(f.shapes[0], GerberShape::Circle { center: Point::new(4_900_000, 33_080_000), radius: 150_000, dark: true });
        assert_eq!(f.shapes[1], GerberShape::Circle { center: Point::new(5_000_000, 33_080_000), radius: 150_000, dark: true });
        assert_eq!(f.shapes[2], GerberShape::Stroke { a: Point::new(1_000_000, 1_000_000), b: Point::new(3_000_000, 1_000_000), width: 1_000_000 });
        assert_eq!(f.shapes[3], GerberShape::Stroke { a: Point::new(10_000_000, 10_000_000), b: Point::new(12_000_000, 10_000_000), width: 1_000_000 });
        match &f.shapes[4] {
            GerberShape::ArcStroke { center, radius, sweep, .. } => {
                assert_eq!(*center, Point::new(12_000_000, 12_000_000));
                assert_eq!(*radius, 2_000_000);
                assert!((sweep - 90.0).abs() < 1e-6);
            }
            s => panic!("{s:?}"),
        }
    }

    #[test]
    fn inch_zero_suppression_formats() {
        // LZ: leading zeros kept, 2.4 format → "0254" = 02.54 inch? No: pad right to 6 digits → 025400 → 2.54".
        let lz = parse_excellon("a.drl", "M48\nINCH,LZ\nT1C0.0394\n%\nT1\nX0254Y01\nM30\n").unwrap();
        assert_eq!(lz.shapes[0], GerberShape::Circle { center: Point::new(64_516_000, 25_400_000), radius: 500_380, dark: true });
        // TZ: trailing zeros kept → divide by 10^4.
        let tz = parse_excellon("a.drl", "M48\nINCH,TZ\nT1C0.0394\n%\nT1\nX0254Y10000\nM30\n").unwrap();
        assert_eq!(tz.shapes[0], GerberShape::Circle { center: Point::new(645_160, 25_400_000), radius: 500_380, dark: true });
        // METRIC,000.000 with LZ.
        let m = parse_excellon("a.drl", "M48\nMETRIC,LZ,000.000\nT01C0.8\n%\nT01\nX001Y-0015\nM30\n").unwrap();
        assert_eq!(m.shapes[0], GerberShape::Circle { center: Point::new(1_000_000, -1_500_000), radius: 400_000, dark: true });
    }

    #[test]
    fn malformed_drills_never_panic() {
        for src in ["M48\nT\nTxC\nX\nY-\nG85\nX1G85\nR99999999X1\nG02X1Y1A0\nG03X1Y1\nM15\nG01X1é\nM30", "T1C-1\nT1\nXééY\nX1Y1G85"] {
            let _ = parse_excellon("bad.drl", src);
        }
        assert!(parse_excellon("x", "hello\nworld").is_err());
    }
}
