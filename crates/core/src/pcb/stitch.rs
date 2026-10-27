//! Via stitching: a grid of through vias inside a filled pour, tying it to the
//! same net's copper on the other layers. Candidates must sit fully inside the
//! pour's fill, keep off pads and existing vias, and pass the online DRC; the
//! frontend commits the result as one `Route` command (vias plus refill).

use serde::{Deserialize, Serialize};

use crate::document::{Document, Edit, ItemId, LayerId};
use crate::item::Item;
use crate::units::{Coord, Point};

use super::connectivity::geo::{point_seg, Bb};
use super::connectivity::{copper_objects, pt, settings_or_default, Index};
use super::items::{Via, ZonePolygon};
use super::query::ObjKind;
use super::router::via_between;

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StitchRequest {
    pub zone: ItemId,
    pub pitch: Coord,
    /// Offset every other row by half a pitch.
    #[serde(default)]
    pub stagger: bool,
    /// Only where the same net is also poured on another layer the via reaches.
    #[serde(default)]
    pub both_sides: bool,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StitchResult {
    pub vias: Vec<Via>,
    /// Grid points inside the pour's outline.
    pub candidates: usize,
    /// Candidates dropped by the online DRC.
    pub rejected: usize,
}

/// `p` is inside a fill polygon with at least `r` of copper around it.
fn inside(polys: &[ZonePolygon], p: Point, r: f64) -> bool {
    let q = pt(p);
    polys.iter().any(|z| {
        let mut bb = Bb::EMPTY;
        z.outer.iter().for_each(|v| bb.add(pt(*v)));
        if !bb.grow(-r).intersects(&Bb::of(q, q)) {
            return false;
        }
        let rings: Vec<&Vec<Point>> = std::iter::once(&z.outer).chain(z.holes.iter()).collect();
        let mut crossings = 0;
        for ring in &rings {
            for k in 0..ring.len() {
                let (a, b) = (pt(ring[k]), pt(ring[(k + 1) % ring.len()]));
                if point_seg(q, a, b).0 < r {
                    return false;
                }
                if (a[1] > q[1]) != (b[1] > q[1]) && q[0] < a[0] + (q[1] - a[1]) * (b[0] - a[0]) / (b[1] - a[1]) {
                    crossings += 1;
                }
            }
        }
        crossings % 2 == 1
    })
}

pub fn stitch(doc: &Document, req: &StitchRequest) -> Result<StitchResult, String> {
    let Some(Item::Zone(zone)) = doc.get(req.zone) else { return Err("not a polygon pour".into()) };
    if zone.keepout || zone.net.is_empty() {
        return Err("via stitching needs a pour with a net".into());
    }
    if zone.fill.is_empty() {
        return Err("the pour is not filled; repour it first (T G)".into());
    }
    if req.pitch <= 0 {
        return Err("the pitch must be positive".into());
    }
    let board = settings_or_default(doc);
    let copper = board.stackup.copper_layers();
    let (Some(&top), Some(&bottom)) = (copper.first(), copper.last()) else { return Err("the board has no copper layers".into()) };
    let probe = via_between(doc, &zone.net, Point::default(), top, bottom);
    let r = probe.diameter as f64 / 2.0;
    let span: Vec<LayerId> = board.stackup.span(probe.from_layer, probe.to_layer);
    let others: Vec<ZonePolygon> = doc
        .items()
        .filter_map(|(id, i)| match i {
            Item::Zone(z) if id != req.zone && !z.keepout && !z.teardrop && z.net == zone.net && z.layer != zone.layer && span.contains(&z.layer) => Some(z.fill.clone()),
            _ => None,
        })
        .flatten()
        .collect();

    let mut bb = Bb::EMPTY;
    zone.fill.iter().flat_map(|z| z.outer.iter()).for_each(|p| bb.add(pt(*p)));
    let idx = Index::new(copper_objects(doc, &board));
    let mut out = StitchResult::default();
    let mut points = Vec::new();
    let pitch = req.pitch as f64;
    let mut y = bb.min[1] + pitch / 2.0;
    let mut row = 0;
    while y <= bb.max[1] {
        let mut x = bb.min[0] + pitch / 2.0 + if req.stagger && row % 2 == 1 { pitch / 2.0 } else { 0.0 };
        while x <= bb.max[0] {
            let p = Point::new(x.round() as Coord, y.round() as Coord);
            if inside(&zone.fill, p, r) {
                out.candidates += 1;
                // Pads (via-in-pad) and existing vias are left alone; other nets are the DRC's job.
                let q = pt(p);
                let near = idx.tree.query(Bb::of(q, q).grow(r + pitch / 2.0)).any(|j| {
                    let o = &idx.objs[j];
                    match o.kind {
                        ObjKind::Via => ((o.pos.x - p.x) as f64).hypot((o.pos.y - p.y) as f64) < pitch / 2.0,
                        ObjKind::Pad => idx.geos[j].bb.grow(r).intersects(&Bb::of(q, q)),
                        _ => false,
                    }
                });
                if !near && (!req.both_sides || inside(&others, p, r)) {
                    points.push(p);
                }
            }
            x += pitch;
        }
        y += pitch;
        row += 1;
    }

    // Online DRC over all candidates at once; clashes with pour fills don't count since the commit refills them.
    let mut work = doc.clone();
    let mut placed = Vec::new();
    let mut edits = Vec::new();
    for p in points {
        let id = work.alloc_id();
        let via = Via { pos: p, ..probe.clone() };
        edits.push(Edit { id, before: None, after: Some(Item::Via(via.clone())) });
        placed.push((id, via));
    }
    work.apply(&edits);
    let ids: Vec<ItemId> = placed.iter().map(|p| p.0).collect();
    let mut bad = std::collections::HashSet::new();
    for v in super::drc::run_near(&work, &ids) {
        if v.code == "unrouted" || v.items.iter().any(|i| matches!(work.get(*i), Some(Item::Zone(_)))) {
            continue;
        }
        bad.extend(v.items.iter().copied().filter(|i| ids.contains(i)));
    }
    out.rejected = bad.len();
    out.vias = placed.into_iter().filter(|(id, _)| !bad.contains(id)).map(|(_, v)| v).collect();
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layer::{BOTTOM_COPPER, TOP_COPPER};
    use crate::pcb::connectivity::fixtures::{doc, fp, id, track, zone};
    use crate::units::mm;

    fn req(zone: u64, pitch: f64) -> StitchRequest {
        StitchRequest { zone: id(zone), pitch: mm(pitch), stagger: false, both_sides: false }
    }

    #[test]
    fn a_grid_of_vias_fills_the_pour_inside_its_edges() {
        let d = doc(vec![zone(0.0, 0.0, 10.0, 10.0, TOP_COPPER, "GND")]);
        let r = stitch(&d, &req(0, 2.0)).unwrap();
        assert_eq!(r.vias.len(), 25);
        assert!(r.vias.iter().all(|v| v.net == "GND" && v.pos.x > 0 && v.pos.x < mm(10.0)));
        assert_eq!(stitch(&d, &StitchRequest { stagger: true, ..req(0, 2.0) }).unwrap().vias.len(), 23);
    }

    #[test]
    fn vias_keep_off_pads_existing_vias_and_other_nets() {
        let d = doc(vec![
            zone(0.0, 0.0, 10.0, 10.0, TOP_COPPER, "GND"),
            fp("R1", "L/R", 5.0, 5.0, &[("1", "GND"), ("2", "SIG")]),
            track((0.0, 3.0), (10.0, 3.0), 0.25, BOTTOM_COPPER, "SIG"),
        ]);
        let r = stitch(&d, &req(0, 2.0)).unwrap();
        assert!(r.rejected >= 5, "{r:?}");
        for v in &r.vias {
            assert!((v.pos.y - mm(3.0)).abs() > mm(0.4), "via on the SIG track: {:?}", v.pos);
            assert!((v.pos.x - mm(5.0)).abs() > mm(1.2) || (v.pos.y - mm(5.0)).abs() > mm(1.0), "via in R1: {:?}", v.pos);
        }
    }

    #[test]
    fn both_sides_needs_the_net_poured_on_another_layer() {
        let d = doc(vec![zone(0.0, 0.0, 10.0, 10.0, TOP_COPPER, "GND"), zone(0.0, 0.0, 5.0, 10.0, BOTTOM_COPPER, "GND")]);
        let r = stitch(&d, &StitchRequest { both_sides: true, ..req(0, 2.0) }).unwrap();
        assert!(!r.vias.is_empty() && r.vias.iter().all(|v| v.pos.x < mm(5.0)));
        assert!(stitch(&d, &StitchRequest { pitch: 0, ..req(0, 2.0) }).is_err());
    }
}
