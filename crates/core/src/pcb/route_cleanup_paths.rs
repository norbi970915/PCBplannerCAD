//! Alternatives that retain existing obstacle geometry during bend cleanup.
use super::{CornerStyle, Point, candidates, elastic, geom, length};

const MAX_RETAINED_PATHS: usize = 24;

/// Retain an exact point inside an edge, never a rounded approximation of its
/// heading. Coprime endpoint differences have no interior lattice point.
fn midpoint(a: Point, b: Point) -> Option<Point> {
    let (a, b) = if (a.x, a.y) <= (b.x, b.y) {
        (a, b)
    } else {
        (b, a)
    };
    let dx = b.x as i128 - a.x as i128;
    let dy = b.y as i128 - a.y as i128;
    let (mut x, mut y) = (dx.abs(), dy.abs());
    while y != 0 {
        (x, y) = (y, x % y);
    }
    if x < 2 {
        return None;
    }
    let steps = x / 2;
    Some(Point::new(
        (a.x as i128 + dx / x * steps) as i64,
        (a.y as i128 + dy / x * steps) as i64,
    ))
}

fn through(old: &[Point], pivots: &[Point], style: CornerStyle) -> Vec<Vec<Point>> {
    let a = old[0];
    let b = old[old.len() - 1];
    let direct = candidates(a, b, style);
    let mut out = Vec::new();
    for &pivot in pivots {
        let first = candidates(a, pivot, style);
        let second = candidates(pivot, b, style);
        for left in &first {
            for right in &second {
                let path = elastic::simplify(
                    &left
                        .iter()
                        .copied()
                        .chain(right.iter().copied().skip(1))
                        .collect::<Vec<_>>(),
                );
                if path.len() < 2
                    || path.len() > old.len()
                    || !geom::improves(old, &path)
                    || !elastic::simple_path(&path)
                    || direct.contains(&path)
                {
                    continue;
                }
                out.push(path);
            }
        }
    }
    out.sort_by(|a, b| {
        length(a).total_cmp(&length(b)).then_with(|| {
            a.iter()
                .map(|p| (p.x, p.y))
                .cmp(b.iter().map(|p| (p.x, p.y)))
        })
    });
    out.dedup();
    out.truncate(MAX_RETAINED_PATHS);
    out
}

/// Prefer retained vertices, then exact edge points. Every alternative remains
/// subject to the caller's full contact, corridor and clearance checks.
pub(super) fn retained(old: &[Point], style: CornerStyle) -> Vec<Vec<Point>> {
    if old.len() < 4 {
        return Vec::new();
    }
    let mut out = through(old, &old[1..old.len() - 1], style);
    if out.len() < MAX_RETAINED_PATHS {
        let pivots: Vec<_> = old
            .windows(2)
            .filter_map(|w| midpoint(w[0], w[1]))
            .collect();
        for path in through(old, &pivots, style) {
            if !out.contains(&path) {
                out.push(path);
            }
            if out.len() == MAX_RETAINED_PATHS {
                break;
            }
        }
    }
    out
}
