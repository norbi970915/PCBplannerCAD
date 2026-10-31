use crate::{
    Point,
    pcb::{
        Track,
        connectivity::{
            Index,
            geo::{Geo, TOUCH},
            pt,
        },
    },
};
use std::collections::HashSet;

pub(super) fn contacts(idx: &Index, i: usize, removed: &HashSet<crate::ItemId>) -> Vec<usize> {
    let source = &idx.objs[i];
    idx.tree
        .query(idx.geos[i].bb.grow(TOUCH))
        .filter(|j| {
            *j != i
                && idx.objs[*j].item != source.item
                && !removed.contains(&idx.objs[*j].item)
                && idx.objs[*j].net == source.net
                && idx.objs[*j].shares_layer(source)
                && idx.touches(i, *j)
        })
        .collect()
}
pub(super) fn touches_end(idx: &Index, contacts: &[usize], t: &Track, p: Point) -> bool {
    let q = pt(p);
    let cap = Geo::cap(q, q, t.width as f64 / 2.);
    contacts
        .iter()
        .any(|j| crate::pcb::connectivity::geo::gap(&cap, &idx.geos[*j], TOUCH).is_some())
}
/// Pieces already joined at a single support must not keep an open stub alive.
pub(super) fn one_support(idx: &Index, contacts: &[usize]) -> bool {
    let contacts_set: HashSet<_> = contacts.iter().copied().collect();
    let mut reached = HashSet::new();
    let mut pending = Vec::new();
    if let Some(first) = contacts.first() {
        reached.insert(*first);
        pending.push(*first);
    }
    while let Some(at) = pending.pop() {
        for j in idx.tree.query(idx.geos[at].bb.grow(TOUCH)) {
            if contacts_set.contains(&j) && !reached.contains(&j) && idx.touches(at, j) {
                reached.insert(j);
                pending.push(j);
            }
        }
    }
    reached.len() == contacts.len()
}
/// Retain all physical supports while cutting only exact integer-lattice tails.
pub(super) fn trim(
    idx: &Index,
    contacts: &[usize],
    t: &Track,
    keep_a: bool,
    keep_b: bool,
) -> Option<Track> {
    let dx = t.b.x as i128 - t.a.x as i128;
    let dy = t.b.y as i128 - t.a.y as i128;
    let gcd = |mut a: u128, mut b: u128| {
        while b != 0 {
            let r = a % b;
            a = b;
            b = r;
        }
        a
    };
    let steps = gcd(dx.unsigned_abs(), dy.unsigned_abs());
    if steps < 2 || t.width <= 0 {
        return None;
    }
    let source = Geo::cap(pt(t.a), pt(t.b), t.width as f64 / 2.);
    let len2 = (dx as f64).powi(2) + (dy as f64).powi(2);
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for j in contacts {
        let (_, at) = crate::pcb::connectivity::geo::gap(&source, &idx.geos[*j], TOUCH)?;
        let k = (((at[0] - t.a.x as f64) * dx as f64 + (at[1] - t.a.y as f64) * dy as f64) / len2
            * (steps as f64))
            .clamp(0., steps as f64);
        let k = if (k - k.round()).abs() < 1e-6 {
            k.round()
        } else {
            k
        };
        lo = lo.min(k.floor());
        hi = hi.max(k.ceil());
    }
    if !lo.is_finite() || !hi.is_finite() {
        return None;
    }
    let mut lo = if keep_a { 0 } else { lo as u128 };
    let mut hi = if keep_b { steps } else { hi as u128 };
    if lo == hi {
        lo = lo.saturating_sub(1);
        hi = (hi + 1).min(steps);
    }
    if lo >= hi || (lo == 0 && hi == steps) {
        return None;
    }
    let at = |k: u128| {
        Point::new(
            (t.a.x as i128 + dx / (steps as i128) * k as i128) as i64,
            (t.a.y as i128 + dy / (steps as i128) * k as i128) as i64,
        )
    };
    let next = Track {
        a: at(lo),
        b: at(hi),
        ..t.clone()
    };
    let geometry = Geo::cap(pt(next.a), pt(next.b), next.width as f64 / 2.);
    contacts
        .iter()
        .all(|j| crate::pcb::connectivity::geo::gap(&geometry, &idx.geos[*j], TOUCH).is_some())
        .then_some(next)
}
