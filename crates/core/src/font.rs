//! The single-stroke font shared with the UI renderer (glyph data lives in
//! `app/src/editor/glyphs.json`), so Gerber, PDF and 3D text match the screen.
//! Glyph units: cap height 6, x-height 4, descender −2.

use std::collections::HashMap;
use std::sync::OnceLock;

use crate::item::{HAlign, Text, VAlign};
use crate::units::{Coord, Point};

const GLYPH_JSON: &str = include_str!("../../../app/src/editor/glyphs.json");
const SPACING: f64 = 1.2;
const CAP: f64 = 6.0;
const LINE_PITCH: f64 = 10.0;
/// Stroke width as a fraction of text height.
pub const STROKE_RATIO: f64 = 0.13;

struct Glyph {
    width: f64,
    strokes: Vec<Vec<(f64, f64)>>,
}

fn table() -> &'static HashMap<char, Glyph> {
    static TABLE: OnceLock<HashMap<char, Glyph>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let raw: HashMap<String, String> = serde_json::from_str(GLYPH_JSON).expect("glyphs.json is valid");
        raw.into_iter().filter_map(|(k, v)| Some((k.chars().next()?, parse(&v)))).collect()
    })
}

fn parse(raw: &str) -> Glyph {
    let (w, body) = raw.split_once(':').unwrap_or(("4", ""));
    let strokes = body
        .split('|')
        .filter(|s| !s.trim().is_empty())
        .map(|s| {
            s.split_whitespace()
                .filter_map(|p| {
                    let (x, y) = p.split_once(',')?;
                    Some((x.parse().ok()?, y.parse().ok()?))
                })
                .collect()
        })
        .collect();
    Glyph { width: w.parse().unwrap_or(4.0), strokes }
}

fn glyph(c: char) -> &'static Glyph {
    static BOX: OnceLock<Glyph> = OnceLock::new();
    table().get(&c).unwrap_or_else(|| BOX.get_or_init(|| parse("4:0,0 4,0 4,6 0,6 0,0")))
}

/// Splits `~{overbar}` markup into (text, overbar) runs.
fn runs(line: &str) -> Vec<(String, bool)> {
    let mut out = Vec::new();
    let mut rest = line;
    while let Some(i) = rest.find("~{") {
        if let Some(j) = rest[i + 2..].find('}') {
            if i > 0 {
                out.push((rest[..i].to_string(), false));
            }
            out.push((rest[i + 2..i + 2 + j].to_string(), true));
            rest = &rest[i + 3 + j..];
        } else {
            break;
        }
    }
    if !rest.is_empty() {
        out.push((rest.to_string(), false));
    }
    out
}

fn line_width(line: &str) -> f64 {
    let w: f64 = runs(line).iter().flat_map(|(t, _)| t.chars()).map(|c| glyph(c).width + SPACING).sum();
    (w - SPACING).max(0.0)
}

#[derive(Clone, Copy, Debug)]
pub struct TextStyle {
    pub height: Coord,
    pub rotation: f64,
    pub h_align: HAlign,
    pub v_align: VAlign,
    pub mirrored: bool,
    /// Keep text readable (never upside down).
    pub upright: bool,
}

impl TextStyle {
    pub fn of(t: &Text) -> TextStyle {
        TextStyle { height: t.height, rotation: t.rotation, h_align: t.h_align, v_align: t.v_align, mirrored: t.mirrored, upright: true }
    }
}

/// Stroke width used for text of `height`.
pub fn stroke_width(height: Coord) -> Coord {
    ((height as f64) * STROKE_RATIO).round() as Coord
}

/// Width of one line of text at `height`.
pub fn text_width(text: &str, height: Coord) -> Coord {
    (line_width(text) * height as f64 / CAP).round() as Coord
}

/// Stroke segments of `text` anchored at `pos`, in world nanometers.
pub fn text_segments(text: &str, pos: Point, style: TextStyle) -> Vec<(Point, Point)> {
    let mut rot = style.rotation.rem_euclid(360.0);
    let (mut h, mut v) = (style.h_align, style.v_align);
    if style.upright && rot > 90.5 && rot <= 270.5 {
        rot -= 180.0;
        h = h.flipped();
        v = match v {
            VAlign::Bottom => VAlign::Top,
            VAlign::Top => VAlign::Bottom,
            VAlign::Middle => VAlign::Middle,
        };
    }
    let unit = style.height as f64 / CAP;
    let lines: Vec<&str> = text.split('\n').collect();
    let block_bottom = -((lines.len() as f64) - 1.0) * LINE_PITCH;
    let dy = match v {
        VAlign::Bottom => -block_bottom,
        VAlign::Top => -CAP,
        VAlign::Middle => -(CAP + block_bottom) / 2.0,
    };
    let (sin, cos) = rot.to_radians().sin_cos();
    let mx = if style.mirrored { -1.0 } else { 1.0 };
    let tf = |x: f64, y: f64| {
        let (lx, ly) = (x * unit * mx, y * unit);
        Point::new(pos.x + (lx * cos - ly * sin).round() as Coord, pos.y + (lx * sin + ly * cos).round() as Coord)
    };
    let mut out = Vec::new();
    for (li, line) in lines.iter().enumerate() {
        let w = line_width(line);
        let dx = match h {
            HAlign::Left => 0.0,
            HAlign::Right => -w,
            HAlign::Center => -w / 2.0,
        };
        let base_y = -(li as f64) * LINE_PITCH + dy;
        let mut cx = dx;
        for (run, overbar) in runs(line) {
            let start = cx;
            for c in run.chars() {
                let g = glyph(c);
                for s in &g.strokes {
                    if s.len() == 1 {
                        let (x, y) = s[0];
                        out.push((tf(cx + x, base_y + y), tf(cx + x + 0.01, base_y + y)));
                    }
                    for pair in s.windows(2) {
                        out.push((tf(cx + pair[0].0, base_y + pair[0].1), tf(cx + pair[1].0, base_y + pair[1].1)));
                    }
                }
                cx += g.width + SPACING;
            }
            if overbar && cx > start {
                out.push((tf(start, base_y + CAP + 1.0), tf(cx - SPACING, base_y + CAP + 1.0)));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::mm;

    #[test]
    fn glyph_table_loads_and_layout_aligns() {
        assert!(table().len() > 90);
        let style = TextStyle { height: mm(1.0), rotation: 0.0, h_align: HAlign::Left, v_align: VAlign::Bottom, mirrored: false, upright: true };
        let segs = text_segments("AB", Point::default(), style);
        assert!(!segs.is_empty());
        let min_x = segs.iter().flat_map(|(a, b)| [a.x, b.x]).min().unwrap();
        let max_y = segs.iter().flat_map(|(a, b)| [a.y, b.y]).max().unwrap();
        assert_eq!(min_x, 0);
        assert_eq!(max_y, mm(1.0)); // cap height == text height
        let centered = text_segments("AB", Point::default(), TextStyle { h_align: HAlign::Center, ..style });
        let cmin = centered.iter().flat_map(|(a, b)| [a.x, b.x]).min().unwrap();
        assert!((cmin + text_width("AB", mm(1.0)) / 2).abs() <= 1);
    }

    #[test]
    fn overbar_markup_adds_a_bar() {
        let style = TextStyle { height: mm(1.0), rotation: 0.0, h_align: HAlign::Left, v_align: VAlign::Bottom, mirrored: false, upright: true };
        let plain = text_segments("RESET", Point::default(), style).len();
        assert_eq!(text_segments("~{RESET}", Point::default(), style).len(), plain + 1);
    }
}
