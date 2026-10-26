use super::*;
use crate::layer;
use crate::pcb::{BoardSettings, Zone, ZoneConnect};
use crate::units::mm;
use crate::{Command, Session, io};

fn p(x: f64, y: f64) -> Point {
    Point::new(mm(x), mm(y))
}
fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<Point> {
    vec![p(x0, y0), p(x1, y0), p(x1, y1), p(x0, y1)]
}
fn zone(outline: Vec<Point>, priority: i32) -> Item {
    Item::Zone(Zone {
        outline,
        layer: layer::TOP_COPPER,
        net: "GND".into(),
        name: "Ground plane".into(),
        priority,
        clearance: mm(0.2),
        min_width: 0,
        connect: ZoneConnect::Thermal,
        thermal_gap: mm(0.3),
        spoke_width: mm(0.3),
        keepout: false,
        remove_islands: false,
        fill: vec![],
    })
}
fn board() -> Session {
    Session::with_document(Document::from_item_list(
        DocKind::Pcb,
        [
            Item::Board(BoardSettings::default()),
            zone(rect(0., 0., 10., 10.), 2),
            zone(rect(-1., -1., 11., 11.), 0),
        ],
    ))
}
fn edit(outlines: Vec<Vec<Point>>) -> Command {
    Command::EditPolygon {
        id: ItemId(1),
        original: rect(0., 0., 10., 10.),
        outlines,
    }
}

#[test]
fn cut_refills_neighbors_and_is_one_undo_step() {
    let mut s = board();
    let before = s.document().clone();
    let delta = s.execute(edit(vec![rect(0., 0., 5., 10.)])).unwrap();
    assert!(
        delta.upserted.iter().any(|x| x.id == ItemId(2)),
        "dependent pour must refill"
    );
    let Item::Zone(z) = s.document().get(ItemId(1)).unwrap() else {
        panic!()
    };
    assert_eq!(z.outline, rect(0., 0., 5., 10.));
    assert!(!z.fill.is_empty());
    assert_eq!(z.name, "Ground plane");
    assert_eq!(z.net, "GND");
    let after = s.document().clone();
    s.undo().unwrap();
    assert_eq!(s.document(), &before);
    assert!(!s.can_undo());
    s.redo().unwrap();
    assert_eq!(s.document(), &after);
}

#[test]
fn slice_preserves_settings_refills_and_survives_save_reload() {
    let mut s = board();
    let before = s.document().item_list();
    s.execute(edit(vec![rect(0., 0., 5., 10.), rect(5., 0., 10., 10.)]))
        .unwrap();
    let Item::Zone(a) = s.document().get(ItemId(1)).unwrap() else {
        panic!()
    };
    let Item::Zone(b) = s.document().get(ItemId(3)).unwrap() else {
        panic!()
    };
    let mut expected = a.clone();
    expected.outline = b.outline.clone();
    expected.fill = b.fill.clone();
    assert_eq!(&expected, b);
    assert!(!a.fill.is_empty() && !b.fill.is_empty());
    let json = io::document_to_string(s.document()).unwrap();
    assert_eq!(
        io::document_from_str(DocKind::Pcb, &json).unwrap(),
        *s.document()
    );
    s.undo().unwrap();
    assert_eq!(s.document().item_list(), before);
    assert!(s.document().get(ItemId(3)).is_none());
    assert!(!s.can_undo());
    s.redo().unwrap();
    assert!(s.document().get(ItemId(3)).is_some());
    let d = s
        .execute(Command::Add {
            items: vec![zone(rect(20., 0., 30., 10.), 0)],
        })
        .unwrap();
    assert_eq!(d.upserted[0].id, ItemId(4), "split must reserve IDs");
}

#[test]
fn bad_and_stale_cuts_leave_document_and_history_untouched() {
    let mut s = board();
    let before = s.document().clone();
    // Crossing, repeated vertex, T junction, retraced edge and a missing side.
    for ring in [
        vec![p(0., 0.), p(10., 10.), p(0., 10.), p(10., 0.)],
        vec![p(0., 0.), p(10., 0.), p(10., 10.), p(0., 0.), p(0., 10.)],
        vec![p(0., 0.), p(10., 0.), p(10., 10.), p(5., 0.), p(0., 10.)],
        vec![p(0., 0.), p(10., 0.), p(5., 0.), p(5., 10.), p(0., 10.)],
        vec![p(0., 0.), p(10., 0.)],
    ] {
        assert!(s.execute(edit(vec![ring])).is_err());
        assert_eq!(s.document(), &before);
        assert!(!s.can_undo());
    }
    assert!(
        s.execute(Command::EditPolygon {
            id: ItemId(1),
            original: rect(0., 0., 9., 10.),
            outlines: vec![rect(0., 0., 5., 10.)]
        })
        .is_err()
    );
    assert_eq!(s.document(), &before);
    assert!(!s.can_undo());
}

#[test]
fn reverse_winding_is_normalized_and_keepouts_stay_unfilled() {
    let mut item = zone(rect(0., 0., 10., 10.), 0);
    if let Item::Zone(z) = &mut item {
        z.keepout = true;
    }
    let mut s = Session::with_document(Document::from_item_list(
        DocKind::Pcb,
        [Item::Board(BoardSettings::default()), item],
    ));
    let mut reversed = rect(0., 0., 5., 10.);
    reversed.reverse();
    s.execute(edit(vec![reversed])).unwrap();
    let Item::Zone(z) = s.document().get(ItemId(1)).unwrap() else {
        panic!()
    };
    assert!(signed_area(&z.outline) > 0.);
    assert!(z.keepout && z.fill.is_empty());
}
