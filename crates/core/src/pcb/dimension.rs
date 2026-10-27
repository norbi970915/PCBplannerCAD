//! Dimensions: a measured distance drawn as extension lines, a dimension line
//! with arrows and the value. The object stays live (moving its points updates
//! the value); outputs draw it through [`Dimension::primitives`]. The frontend
//! mirrors the geometry in `app/src/editor/dimension.ts`.

use serde::{Deserialize, Serialize};

use crate::document::LayerId;
use crate::item::{HAlign, Item, Line, Text, VAlign};
use crate::units::{Coord, Point};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DimUnits {
    #[default]
    Mm,
    Mil,
    Inch,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Dimension {
    /// Measured points.
    pub a: Point,
    pub b: Point,
    /// Distance of the dimension line from `a`, along the left normal of the measuring direction.
    pub offset: Coord,
    /// Measures along `angle` (horizontal = 0°, vertical = 90°) instead of along a→b.
    #[serde(default)]
    pub orthogonal: bool,
    #[serde(default)]
    pub angle: f64,
    pub layer: LayerId,
    pub width: Coord,
    pub text_height: Coord,
    #[serde(default)]
    pub units: DimUnits,
    #[serde(default = "two")]
    pub precision: u8,
}

fn two() -> u8 {
    2
}

type V = (f64, f64);

impl Dimension {
    /// Measuring direction (unit vector).
    pub fn direction(&self) -> V {
        if !self.orthogonal {
            let (dx, dy) = ((self.b.x - self.a.x) as f64, (self.b.y - self.a.y) as f64);
            let l = dx.hypot(dy);
            if l > 0.0 {
                return (dx / l, dy / l);
            }
        }
        let r = self.angle.to_radians();
        (r.cos(), r.sin())
    }

    /// The measured distance, nm.
    pub fn value(&self) -> f64 {
        let u = self.direction();
        (((self.b.x - self.a.x) as f64) * u.0 + ((self.b.y - self.a.y) as f64) * u.1).abs()
    }

    pub fn label(&self) -> String {
        let v = self.value();
        let p = self.precision.min(6) as usize;
        match self.units {
            DimUnits::Mm => format!("{:.*} mm", p, v / 1e6),
            DimUnits::Mil => format!("{:.*} mil", p, v / 25_400.0),
            DimUnits::Inch => format!("{:.*} in", p, v / 25_400_000.0),
        }
    }

    /// Lines and the value text, in board coordinates.
    pub fn primitives(&self) -> Vec<Item> {
        let u = self.direction();
        let n = (-u.1, u.0);
        let (a, b) = ((self.a.x as f64, self.a.y as f64), (self.b.x as f64, self.b.y as f64));
        let along = (b.0 - a.0) * u.0 + (b.1 - a.1) * u.1;
        let off = self.offset as f64;
        let p1 = (a.0 + n.0 * off, a.1 + n.1 * off);
        let p2 = (p1.0 + u.0 * along, p1.1 + u.1 * along);
        let h = self.text_height as f64;
        let side = if off >= 0.0 { 1.0 } else { -1.0 };
        let over = h * 0.4;
        let pt = |p: V| Point::new(p.0.round() as Coord, p.1.round() as Coord);
        let line = |p: V, q: V| Item::Line(Line { a: pt(p), b: pt(q), width: self.width, layer: self.layer });
        let mut out = Vec::new();
        // Extension lines: from each measured point past the dimension line.
        for (from, foot) in [(a, p1), (b, p2)] {
            let (dx, dy) = (foot.0 - from.0, foot.1 - from.1);
            let l = dx.hypot(dy);
            if l > 0.0 {
                let e = (foot.0 + dx / l * over, foot.1 + dy / l * over);
                out.push(line(from, e));
            }
        }
        if along.abs() > 0.0 {
            out.push(line(p1, p2));
            // Arrow heads pointing outwards at each foot.
            let s = along.signum();
            let arrow = (h * 0.8).min(along.abs() / 3.0);
            for (tip, dir) in [(p1, s), (p2, -s)] {
                for k in [-1.0, 1.0] {
                    let (c, si) = (25f64.to_radians().cos(), 25f64.to_radians().sin() * k);
                    let d = (u.0 * dir * c - u.1 * dir * si, u.1 * dir * c + u.0 * dir * si);
                    out.push(line(tip, (tip.0 + d.0 * arrow, tip.1 + d.1 * arrow)));
                }
            }
        }
        // Value text centred above the line (on the side away from the measured points), kept readable.
        let mid = ((p1.0 + p2.0) / 2.0 + n.0 * side * h * 0.8, (p1.1 + p2.1) / 2.0 + n.1 * side * h * 0.8);
        let mut rot = u.1.atan2(u.0).to_degrees();
        if rot > 90.0 + 1e-6 {
            rot -= 180.0;
        } else if rot <= -90.0 + 1e-6 {
            rot += 180.0;
        }
        out.push(Item::Text(Text {
            pos: pt(mid),
            text: self.label(),
            height: self.text_height,
            rotation: rot,
            h_align: HAlign::Center,
            v_align: VAlign::Middle,
            layer: self.layer,
            mirrored: false,
        }));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layer;
    use crate::units::mm;

    fn dim(a: (f64, f64), b: (f64, f64), offset: f64) -> Dimension {
        Dimension { a: Point::new(mm(a.0), mm(a.1)), b: Point::new(mm(b.0), mm(b.1)), offset: mm(offset), orthogonal: false, angle: 0.0, layer: layer::MECHANICAL_1, width: mm(0.1), text_height: mm(1.0), units: DimUnits::Mm, precision: 2 }
    }

    #[test]
    fn aligned_and_orthogonal_values_and_labels() {
        let d = dim((0.0, 0.0), (30.0, 40.0), 5.0);
        assert_eq!(d.label(), "50.00 mm");
        let h = Dimension { orthogonal: true, angle: 0.0, ..d.clone() };
        assert_eq!(h.label(), "30.00 mm");
        let v = Dimension { orthogonal: true, angle: 90.0, units: DimUnits::Mil, precision: 0, ..d };
        assert_eq!(v.label(), "1575 mil");
    }

    #[test]
    fn primitives_put_the_line_at_the_offset_with_the_value_beside_it() {
        let d = Dimension { orthogonal: true, ..dim((0.0, 0.0), (20.0, 3.0), 5.0) };
        let p = d.primitives();
        // Two extension lines, the dimension line, four arrow strokes, the text.
        assert_eq!(p.len(), 8);
        let Item::Line(l) = &p[2] else { panic!() };
        assert_eq!((l.a, l.b), (Point::new(0, mm(5.0)), Point::new(mm(20.0), mm(5.0))));
        let Item::Text(t) = p.last().unwrap() else { panic!() };
        assert_eq!((t.text.as_str(), t.pos, t.rotation), ("20.00 mm", Point::new(mm(10.0), mm(5.8)), 0.0));
        // Measured right to left: still readable, not upside down.
        let Item::Text(t) = dim((20.0, 0.0), (0.0, 0.0), -5.0).primitives().pop().unwrap() else { panic!() };
        assert!(t.rotation.abs() < 1e-9, "{}", t.rotation);
    }
}
