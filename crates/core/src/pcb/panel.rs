//! Panelization: an N×M array of a board with break-away rails, either
//! separated by V-score lines or routed apart (the board outlines become
//! routing paths).

use serde::{Deserialize, Serialize};

use crate::document::{DocKind, Document};
use crate::item::{FillStyle, HAlign, Item, Line, Polyline, Text, VAlign};
use crate::layer;
use crate::units::{Coord, Point, mm};

use super::geometry::{board_settings, outline_loops};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PanelSpec {
    pub rows: u32,
    pub cols: u32,
    /// Gap between boards (0 for V-scored panels).
    pub spacing: Coord,
    /// Rail width on all four sides (0 = no rails).
    pub rail: Coord,
    /// Draw V-score lines on Mechanical 1 instead of routing between boards.
    pub vscore: bool,
    /// Add three fiducial marks (copper dots) on the rails.
    #[serde(default = "yes")]
    pub fiducials: bool,
}

fn yes() -> bool {
    true
}

/// Builds the panel document. The board must have a closed outline.
pub fn panelize(board: &Document, spec: &PanelSpec) -> Result<Document, String> {
    let loops = outline_loops(board, 10_000);
    let outer = loops.first().ok_or("the board has no closed outline")?;
    let (min, max) = outer.iter().fold((Point::new(Coord::MAX, Coord::MAX), Point::new(Coord::MIN, Coord::MIN)), |(lo, hi), p| {
        (Point::new(lo.x.min(p.x), lo.y.min(p.y)), Point::new(hi.x.max(p.x), hi.y.max(p.y)))
    });
    let (w, h) = (max.x - min.x, max.y - min.y);
    let (rows, cols) = (spec.rows.clamp(1, 50) as i64, spec.cols.clamp(1, 50) as i64);
    let gap = if spec.vscore { 0 } else { spec.spacing.max(0) };
    let pitch = Point::new(w + gap, h + gap);

    let mut items: Vec<Item> = Vec::new();
    if let Some((_, settings)) = board_settings(board) {
        items.push(Item::Board(settings.clone()));
    }
    for r in 0..rows {
        for c in 0..cols {
            let (dx, dy) = (c * pitch.x - min.x, r * pitch.y - min.y);
            for (_, item) in board.items() {
                if matches!(item, Item::Board(_)) {
                    continue;
                }
                let on_outline = matches!(item,
                    Item::Line(l) if l.layer == layer::BOARD_OUTLINE) || matches!(item, Item::Arc(a) if a.layer == layer::BOARD_OUTLINE)
                    || matches!(item, Item::Polyline(p) if p.layer == layer::BOARD_OUTLINE)
                    || matches!(item, Item::Rect(x) if x.layer == layer::BOARD_OUTLINE)
                    || matches!(item, Item::Circle(x) if x.layer == layer::BOARD_OUTLINE);
                // V-scored panels keep only inner cutouts of each board; the
                // outer edges become score lines.
                if on_outline && spec.vscore && is_outer_edge(item, min, max) {
                    continue;
                }
                items.push(item.translated(dx, dy));
            }
        }
    }

    let array = Point::new(cols * pitch.x - gap, rows * pitch.y - gap);
    let rail = spec.rail.max(0);
    let frame_min = Point::new(-rail, -rail);
    let frame_max = Point::new(array.x + rail, array.y + rail);
    if rail > 0 || spec.vscore {
        let rect = [frame_min, Point::new(frame_max.x, frame_min.y), frame_max, Point::new(frame_min.x, frame_max.y)];
        items.push(Item::Polyline(Polyline { points: rect.to_vec(), closed: true, width: mm(0.1), layer: layer::BOARD_OUTLINE, fill: FillStyle::None }));
    }
    if spec.vscore {
        let score = |a: Point, b: Point| Item::Line(Line { a, b, width: mm(0.1), layer: layer::MECHANICAL_1 });
        for c in 0..=cols {
            let x = c * pitch.x;
            items.push(score(Point::new(x, frame_min.y), Point::new(x, frame_max.y)));
        }
        for r in 0..=rows {
            let y = r * pitch.y;
            items.push(score(Point::new(frame_min.x, y), Point::new(frame_max.x, y)));
        }
        items.push(Item::Text(Text {
            pos: Point::new(frame_min.x + mm(1.0), frame_min.y - mm(1.5)),
            text: "V-SCORE: Mechanical 1".into(),
            height: mm(1.0),
            rotation: 0.0,
            h_align: HAlign::Left,
            v_align: VAlign::Top,
            layer: layer::MECHANICAL_1,
            mirrored: false,
        }));
    }
    if spec.fiducials && rail >= mm(3.0) {
        let r = rail / 2;
        for p in [Point::new(frame_min.x + r * 2, frame_min.y + r), Point::new(frame_max.x - r * 2, frame_min.y + r), Point::new(frame_min.x + r * 2, frame_max.y - r)] {
            items.push(Item::Circle(crate::item::Circle { center: p, radius: mm(0.5), width: 0, layer: layer::TOP_COPPER, fill: FillStyle::Solid }));
            items.push(Item::Circle(crate::item::Circle { center: p, radius: mm(1.0), width: 0, layer: layer::TOP_MASK, fill: FillStyle::Solid }));
        }
    }
    let mut doc = Document::from_item_list(DocKind::Pcb, items);
    for (k, f) in board.footprint_cache() {
        doc.put_cached_footprint(k.clone(), f.clone());
    }
    Ok(doc)
}

/// Does an outline item lie on the board's bounding box (an outer edge)?
fn is_outer_edge(item: &Item, min: Point, max: Point) -> bool {
    let on_edge = |p: &Point| p.x == min.x || p.x == max.x || p.y == min.y || p.y == max.y;
    match item {
        Item::Line(l) => on_edge(&l.a) && on_edge(&l.b),
        Item::Rect(r) => on_edge(&r.a) && on_edge(&r.b),
        Item::Polyline(p) => p.points.iter().filter(|q| on_edge(q)).count() * 2 >= p.points.len(),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::Rect;

    fn board() -> Document {
        Document::from_item_list(
            DocKind::Pcb,
            [
                Item::Board(Default::default()),
                Item::Rect(Rect { a: Point::new(0, 0), b: Point::new(mm(20.0), mm(10.0)), width: mm(0.1), layer: layer::BOARD_OUTLINE, fill: FillStyle::None }),
            ],
        )
    }

    #[test]
    fn routed_panel_copies_outlines_and_adds_rails() {
        let p = panelize(&board(), &PanelSpec { rows: 2, cols: 3, spacing: mm(2.0), rail: mm(5.0), vscore: false, fiducials: true }).unwrap();
        let outlines = outline_loops(&p, 1000);
        assert_eq!(outlines.len(), 7); // 6 boards + frame
        assert_eq!(p.items().filter(|(_, i)| matches!(i, Item::Board(_))).count(), 1);
    }

    #[test]
    fn vscored_panel_replaces_outer_edges_with_score_lines() {
        let p = panelize(&board(), &PanelSpec { rows: 2, cols: 2, spacing: mm(2.0), rail: mm(5.0), vscore: true, fiducials: false }).unwrap();
        assert_eq!(outline_loops(&p, 1000).len(), 1);
        let scores = p.items().filter(|(_, i)| matches!(i, Item::Line(l) if l.layer == layer::MECHANICAL_1)).count();
        assert_eq!(scores, 6);
    }
}
