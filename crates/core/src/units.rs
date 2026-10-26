use serde::{Deserialize, Serialize};

/// A length or coordinate in nanometers.
pub type Coord = i64;

pub const NM_PER_MM: Coord = 1_000_000;
pub const NM_PER_MIL: Coord = 25_400;

pub fn mm(v: f64) -> Coord {
    (v * NM_PER_MM as f64).round() as Coord
}

pub fn mil(v: f64) -> Coord {
    (v * NM_PER_MIL as f64).round() as Coord
}

pub fn to_mm(c: Coord) -> f64 {
    c as f64 / NM_PER_MM as f64
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Point {
    pub x: Coord,
    pub y: Coord,
}

impl Point {
    pub const fn new(x: Coord, y: Coord) -> Self {
        Self { x, y }
    }

    pub const fn translated(self, dx: Coord, dy: Coord) -> Self {
        Self { x: self.x + dx, y: self.y + dy }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_conversions_round_trip() {
        assert_eq!(mm(1.0), 1_000_000);
        assert_eq!(mil(10.0), 254_000);
        assert_eq!(mm(2.54), mil(100.0));
        assert_eq!(to_mm(mm(0.127)), 0.127);
    }
}
