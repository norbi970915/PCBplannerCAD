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
}

impl Command {
    pub fn label(&self) -> &'static str {
        match self {
            Command::Add { .. } => "Place",
            Command::Delete { .. } => "Delete",
            Command::Move { .. } => "Move",
            Command::Rotate { .. } => "Rotate",
            Command::Mirror { .. } => "Mirror",
            Command::Replace { .. } | Command::ReplaceMany { .. } => "Modify",
            Command::Change { .. } => "Edit",
            Command::EditPolygon { .. } => "Edit polygon",
        }
    }

    pub(crate) fn resolve(self, doc: &mut Document) -> Result<Vec<Edit>, CoreError> {
        let existing = |doc: &Document, id: ItemId| doc.get(id).cloned().ok_or(CoreError::UnknownItem(id));
        let transform = |doc: &Document, ids: Vec<ItemId>, f: &dyn Fn(&Item) -> Item| -> Result<Vec<Edit>, CoreError> {
            ids.into_iter()
                .map(|id| {
                    let before = existing(doc, id)?;
                    let after = f(&before);
                    Ok(Edit { id, before: Some(before), after: Some(after) })
                })
                .collect()
        };
        match self {
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
            Command::Rotate { ids, center, angle } => transform(doc, ids, &|i| i.rotated(center, angle)),
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
