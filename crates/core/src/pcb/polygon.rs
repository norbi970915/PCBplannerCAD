//! Atomic polygon edits: outline changes, slices and dependent copper fills
//! share a transaction. Planning on a copy leaves failed edits untouched.

use super::{fill, geometry::signed_area};
use crate::document::Edit;
use crate::error::CoreError;
use crate::{DocKind, Document, Item, ItemId, Point};

fn cross(a: Point, b: Point, c: Point) -> f64 {
    (b.x as f64 - a.x as f64) * (c.y as f64 - a.y as f64)
        - (b.y as f64 - a.y as f64) * (c.x as f64 - a.x as f64)
}

fn on_segment(a: Point, b: Point, p: Point) -> bool {
    cross(a, b, p) == 0.0
        && p.x >= a.x.min(b.x)
        && p.x <= a.x.max(b.x)
        && p.y >= a.y.min(b.y)
        && p.y <= a.y.max(b.y)
}

fn touches(a: Point, b: Point, c: Point, d: Point) -> bool {
    let (ab_c, ab_d, cd_a, cd_b) = (
        cross(a, b, c),
        cross(a, b, d),
        cross(c, d, a),
        cross(c, d, b),
    );
    (ab_c * ab_d < 0.0 && cd_a * cd_b < 0.0)
        || on_segment(a, b, c)
        || on_segment(a, b, d)
        || on_segment(c, d, a)
        || on_segment(c, d, b)
}

fn valid(ring: &[Point]) -> bool {
    let n = ring.len();
    if n < 3 || signed_area(ring).abs() <= 1e6 {
        return false;
    }
    for i in 0..n {
        let (a, b, c) = (ring[i], ring[(i + 1) % n], ring[(i + 2) % n]);
        if a == b || (cross(a, b, c) == 0.0 && (on_segment(a, b, c) || on_segment(b, c, a))) {
            return false;
        }
        for j in i + 2..n {
            if i == 0 && j == n - 1 {
                continue;
            }
            if touches(a, b, ring[j], ring[(j + 1) % n]) {
                return false;
            }
        }
    }
    true
}

pub(crate) fn resolve(
    doc: &mut Document,
    id: ItemId,
    original: Vec<Point>,
    outlines: Vec<Vec<Point>>,
) -> Result<Vec<Edit>, CoreError> {
    let source = doc.get(id).ok_or(CoreError::UnknownItem(id))?.clone();
    let ring = match &source {
        Item::Zone(z) => &z.outline,
        Item::Polyline(p) if p.closed => &p.points,
        _ => {
            return Err(CoreError::InvalidGeometry(
                "select a pour or closed polygon",
            ));
        }
    };
    if ring != &original {
        return Err(CoreError::InvalidGeometry(
            "polygon changed; start the cut again",
        ));
    }
    if outlines.is_empty() || outlines.len() > 2 || outlines.iter().any(|r| !valid(r)) {
        return Err(CoreError::InvalidGeometry(
            "cut must produce one or two simple polygons",
        ));
    }
    let count = outlines.len();
    let mut next = doc.clone();
    for (i, mut outline) in outlines.into_iter().enumerate() {
        if signed_area(&outline).signum() != signed_area(ring).signum() {
            outline.reverse();
        }
        let mut item = source.clone();
        match &mut item {
            Item::Zone(z) => {
                z.outline = outline;
                z.fill.clear();
            }
            Item::Polyline(p) => p.points = outline,
            _ => unreachable!(),
        }
        item.validate()?;
        let target = if i == 0 { id } else { next.alloc_id() };
        let before = next.get(target).cloned();
        next.apply(&[Edit {
            id: target,
            before,
            after: Some(item),
        }]);
    }
    // Refill all zones: priorities, keep-outs and board outlines can affect
    // neighboring pours too. Only differences become history entries.
    if doc.kind() == DocKind::Pcb {
        for (zid, polygons) in fill::fill_all(&next) {
            if let Some(Item::Zone(z)) = next.get(zid) {
                let mut z = z.clone();
                z.fill = polygons;
                let before = next.get(zid).cloned();
                next.apply(&[Edit {
                    id: zid,
                    before,
                    after: Some(Item::Zone(z)),
                }]);
            }
        }
    }
    let edits = next
        .items()
        .filter_map(|(id, item)| {
            let before = doc.get(id).cloned();
            (before.as_ref() != Some(item)).then(|| Edit {
                id,
                before,
                after: Some(item.clone()),
            })
        })
        .collect();
    // Reserve new IDs only after all planning/validation has succeeded.
    for _ in 1..count {
        doc.alloc_id();
    }
    Ok(edits)
}

#[cfg(test)]
mod tests;
