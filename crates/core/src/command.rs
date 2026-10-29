use serde::Deserialize;

use crate::document::{Document, Edit, IdItem, ItemId};
use crate::error::CoreError;
use crate::item::Item;
use crate::units::{Coord, Point};

/// A user-level intent. Commands are resolved against the current document
/// into concrete [`Edit`]s, which is what gets applied and recorded.
#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase")]
pub enum Command {
    Add { items: Vec<Item> },
    /// Ordinary pour properties and affected overlapping fills, one undo step.
    EditPours { items: Vec<IdItem>, #[serde(default)] refill: Option<Vec<ItemId>> },
    Delete { ids: Vec<ItemId> },
    Move { ids: Vec<ItemId>, dx: Coord, dy: Coord },
    /// Counter-clockwise rotation around `center`.
    Rotate { ids: Vec<ItemId>, center: Point, angle: f64 },
    /// Mirror across the vertical line through `center`.
    Mirror { ids: Vec<ItemId>, center: Point },
    Replace { id: ItemId, item: Item },
    /// Several replacements as one undo step (e.g. annotation).
    ReplaceMany { items: Vec<IdItem> },
    /// Removes `delete` and adds `add` in one undo step (e.g. wire splitting).
    Change { delete: Vec<ItemId>, add: Vec<Item> },
    /// Changes/slices an outline and recomputes affected pours in one undo step.
    EditPolygon { id: ItemId, original: Vec<Point>, outlines: Vec<Vec<Point>> },
    /// Commits routed copper and dependent zone clearances as one undo step.
    /// `replace` keeps item IDs (e.g. a footprint dragged with its tracks).
    Route {
        delete: Vec<ItemId>,
        add: Vec<Item>,
        #[serde(default)]
        replace: Vec<IdItem>,
    },
}

impl Command {
    pub fn label(&self) -> &'static str {
        match self {
            Command::Add { .. } => "Place",
            Command::EditPours { .. } => "Edit pours",
            Command::Delete { .. } => "Delete",
            Command::Move { .. } => "Move",
            Command::Rotate { .. } => "Rotate",
            Command::Mirror { .. } => "Mirror",
            Command::Replace { .. } | Command::ReplaceMany { .. } => "Modify",
            Command::Change { .. } => "Edit",
            Command::EditPolygon { .. } => "Edit polygon",
            Command::Route { .. } => "Route",
        }
    }

    pub(crate) fn resolve(self, doc: &mut Document) -> Result<Vec<Edit>, CoreError> {
        self.check_targets()?;
        self.check_locks(doc)?;
        let existing = |doc: &Document, id: ItemId| doc.get(id).cloned().ok_or(CoreError::UnknownItem(id));
        let transform = |doc: &Document, ids: Vec<ItemId>, f: &dyn Fn(&Item) -> Item| -> Result<Vec<Edit>, CoreError> {
            ids.into_iter()
                .map(|id| {
                    let before = existing(doc, id)?;
                    let after = f(&before);
                    after.validate()?;
                    Ok(Edit { id, before: Some(before), after: Some(after) })
                })
                .collect()
        };
        match self {
            Command::EditPours { items, refill } => resolve_pours(doc, items, refill),
            Command::Route { delete, add, replace } => resolve_route(doc, delete, add, replace),
            Command::EditPolygon { id, original, outlines } => crate::pcb::polygon::resolve(doc, id, original, outlines),
            Command::Add { items } => {
                items.iter().try_for_each(Item::validate)?;
                Ok(items.into_iter().map(|item| Edit { id: doc.alloc_id(), before: None, after: Some(item) }).collect())
            }
            Command::Delete { ids } => ids
                .into_iter()
                .map(|id| Ok(Edit { id, before: Some(existing(doc, id)?), after: None }))
                .collect(),
            Command::Move { ids, dx, dy } => transform(doc, ids, &|i| i.translated(dx, dy)),
            Command::Rotate { ids, center, angle } => {
                if !angle.is_finite() { return Err(CoreError::InvalidGeometry("rotation angle must be finite")); }
                transform(doc, ids, &|i| i.rotated(center, angle))
            },
            Command::Mirror { ids, center } => transform(doc, ids, &|i| i.mirrored(center)),
            Command::Replace { id, item } => Command::ReplaceMany { items: vec![IdItem { id, item }] }.resolve(doc),
            Command::ReplaceMany { items } => items
                .into_iter()
                .map(|IdItem { id, item }| {
                    item.validate()?;
                    Ok(Edit { id, before: Some(existing(doc, id)?), after: Some(item) })
                })
                .collect(),
            Command::Change { delete, add } => {
                let mut edits = Command::Delete { ids: delete }.resolve(doc)?;
                edits.extend(Command::Add { items: add }.resolve(doc)?);
                Ok(edits)
            }
        }
    }
}

impl Command {
    /// Every source item has one intent. In particular, routing may not both
    /// remove and replace an item; its delta and Undo must describe one result.
    fn check_targets(&self) -> Result<(), CoreError> {
        let mut seen = std::collections::HashSet::new();
        let mut check = |id: ItemId| if seen.insert(id) { Ok(()) } else { Err(CoreError::DuplicateItem(id)) };
        match self {
            Self::Delete { ids } | Self::Move { ids, .. } | Self::Rotate { ids, .. } | Self::Mirror { ids, .. } |
            Self::Change { delete: ids, .. } => { for id in ids { check(*id)?; } }
            Self::ReplaceMany { items } | Self::EditPours { items, .. } => { for item in items { check(item.id)?; } }
            Self::Route { delete, replace, .. } => {
                for id in delete { check(*id)?; }
                for item in replace { check(item.id)?; }
            }
            _ => {}
        }
        Ok(())
    }

    fn check_locks(&self, doc: &Document) -> Result<(), CoreError> {
        let check = |id: ItemId, after: Option<&Item>| -> Result<(), CoreError> {
            let Some(before) = doc.get(id) else { return Ok(()) };
            if !is_locked(before) { return Ok(()) }
            if let Some(after) = after {
                let normalize = |item: &Item| {
                    let mut item = item.clone();
                    match &mut item {
                        Item::Footprint(v) => v.locked = false,
                        Item::Track(v) => v.locked = false,
                        Item::ArcTrack(v) => v.locked = false,
                        Item::Via(v) => v.locked = false,
                        Item::Zone(v) => { v.locked = false; v.fill.clear(); },
                        _ => {}
                    }
                    item
                };
                if normalize(before) == normalize(after) { return Ok(()) }
            }
            Err(CoreError::InvalidGeometry("locked item: unlock it before editing"))
        };
        match self {
            Self::Delete { ids } | Self::Move { ids, .. } | Self::Rotate { ids, .. } | Self::Mirror { ids, .. } => {
                for id in ids { check(*id, None)?; }
            }
            Self::EditPolygon { id, .. } => check(*id, None)?,
            Self::Replace { id, item } => check(*id, Some(item))?,
            Self::ReplaceMany { items } | Self::EditPours { items, .. } => for item in items { check(item.id, Some(&item.item))?; },
            Self::Route { delete, replace, .. } => {
                for id in delete { check(*id, None)?; }
                for item in replace { check(item.id, Some(&item.item))?; }
            }
            Self::Change { delete, .. } => for id in delete { check(*id, None)?; },
            Self::Add { .. } => {}
        }
        Ok(())
    }
}

pub fn is_locked(item: &Item) -> bool {
    match item {
        Item::Footprint(v) => v.locked,
        Item::Track(v) => v.locked,
        Item::ArcTrack(v) => v.locked,
        Item::Via(v) => v.locked,
        Item::Zone(v) => v.locked,
        _ => false,
    }
}

/// Plan on a copy so failed validation cannot allocate IDs or alter copper.
fn resolve_route(doc: &mut Document, delete: Vec<ItemId>, add: Vec<Item>, replace: Vec<IdItem>) -> Result<Vec<Edit>, CoreError> {
    if doc.kind() != crate::DocKind::Pcb || add.iter().any(|item| !matches!(item, Item::Track(_) | Item::Via(_) | Item::ArcTrack(_)) && !matches!(item, Item::Zone(z) if z.teardrop)) {
        return Err(CoreError::InvalidGeometry("routing requires PCB tracks or vias"));
    }
    let count = add.len();
    let mut next = doc.clone();
    let mut edits = Command::Change { delete, add }.resolve(&mut next)?;
    if !replace.is_empty() {
        edits.extend(Command::ReplaceMany { items: replace }.resolve(&mut next)?);
    }
    next.apply(&edits);
    // Stale teardrops go before the refill, so pours flow back into their place.
    let pruned = crate::pcb::teardrop::prune(&mut next, &edits);
    edits.extend(pruned);
    for (id, polygons) in crate::pcb::fill::fill_all(&next) {
        if let Some(Item::Zone(zone)) = next.get(id) {
            if zone.fill == polygons { continue; }
            let mut zone = zone.clone();
            zone.fill = polygons;
            edits.push(Edit { id, before: doc.get(id).cloned(), after: Some(Item::Zone(zone)) });
        }
    }
    for _ in 0..count { doc.alloc_id(); }
    Ok(edits)
}

fn resolve_pours(doc: &mut Document, items: Vec<IdItem>, refill: Option<Vec<ItemId>>) -> Result<Vec<Edit>, CoreError> {
    if doc.kind() != crate::DocKind::Pcb || items.iter().any(|r| !matches!(r.item, Item::Zone(ref z) if !z.keepout && !z.teardrop) || !matches!(doc.get(r.id),Some(Item::Zone(z)) if !z.keepout && !z.teardrop)) {
        return Err(CoreError::InvalidGeometry("pour manager requires ordinary board pours"));
    }
    let mut next=doc.clone();
    let mut edits=Command::ReplaceMany { items }.resolve(&mut next)?;next.apply(&edits);
    let fallback=crate::pcb::board::BoardSettings::default();
    let board=crate::pcb::geometry::board_settings(&next).map_or(&fallback,|(_,b)|b);
    let clearance=crate::pcb::query::RuleSet::new(board).max_clearance();
    let bbox=|z:&crate::pcb::Zone| {
        let margin=z.clearance.max(z.thermal_gap).max(clearance);
        (z.layer, z.outline.iter().map(|p|p.x).min().unwrap_or(0)-margin,z.outline.iter().map(|p|p.y).min().unwrap_or(0)-margin,z.outline.iter().map(|p|p.x).max().unwrap_or(0)+margin,z.outline.iter().map(|p|p.y).max().unwrap_or(0)+margin)
    };
    let affected=if let Some(ids)=refill {
        let mut set:std::collections::HashSet<_>=ids.into_iter().chain(edits.iter().map(|e|e.id)).collect();
        if set.iter().any(|id|!matches!(next.get(*id),Some(Item::Zone(z))if !z.keepout)){return Err(CoreError::InvalidGeometry("select a pour to refill"));}
        let mut areas:Vec<_>=edits.iter().flat_map(|e|[&e.before,&e.after]).filter_map(|i|if let Some(Item::Zone(z))=i{Some(bbox(z))}else{None}).collect();
        areas.extend(set.iter().filter_map(|id|if let Some(Item::Zone(z))=next.get(*id){Some(bbox(z))}else{None}));
        loop {
            let mut added=false;
            for (id,item) in next.items() {
                let Item::Zone(z)=item else{continue};if z.keepout||set.contains(&id){continue;}
                let b=bbox(z);
                if areas.iter().any(|a|a.0==b.0&&a.1<=b.3&&a.3>=b.1&&a.2<=b.4&&a.4>=b.2){set.insert(id);areas.push(b);added=true;}
            }
            if !added{break;}
        }
        Some(set)
    } else {None};
    for (id,fill) in crate::pcb::fill::fill_subset(&next,affected.as_ref()) {
        if let Some(Item::Zone(zone))=next.get(id){if zone.fill==fill{continue;}let mut updated=zone.clone();updated.fill=fill;edits.push(Edit{id,before:next.get(id).cloned(),after:Some(Item::Zone(updated))});}
    }
    Ok(edits)
}
