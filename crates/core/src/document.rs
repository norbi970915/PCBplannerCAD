use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::item::Item;
use crate::library::{CachedPart, Footprint};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ItemId(pub u64);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct LayerId(pub u16);

/// What a document is; decides which editor opens it and which items fit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DocKind {
    Pcb,
    Schematic,
    /// One unit of a library symbol.
    Symbol,
    Footprint,
}

/// An item together with its id, as exchanged with the UI and stored on disk.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IdItem {
    pub id: ItemId,
    pub item: Item,
}

/// One concrete change to the document. `before == None` is an insertion,
/// `after == None` a removal, both `Some` a modification.
#[derive(Clone, Debug, PartialEq)]
pub struct Edit {
    pub id: ItemId,
    pub before: Option<Item>,
    pub after: Option<Item>,
}

impl Edit {
    pub fn inverted(&self) -> Edit {
        Edit { id: self.id, before: self.after.clone(), after: self.before.clone() }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Document {
    kind: DocKind,
    next_id: u64,
    items: BTreeMap<ItemId, Item>,
    /// Schematics only: library snapshots for placed parts, keyed by
    /// [`CachedPart::key`]. Append-only and outside undo history.
    cache: BTreeMap<String, CachedPart>,
    /// PCBs only: footprint geometry for placed footprints, keyed "Library/Name".
    footprints: BTreeMap<String, Footprint>,
    bus_aliases: BTreeMap<String, Vec<String>>,
}

impl Document {
    pub fn new(kind: DocKind) -> Self {
        Self { kind, next_id: 0, items: BTreeMap::new(), cache: BTreeMap::new(), footprints: BTreeMap::new(), bus_aliases: BTreeMap::new() }
    }

    /// Builds a document from plain items, assigning fresh ids in order.
    pub fn from_item_list(kind: DocKind, items: impl IntoIterator<Item = Item>) -> Self {
        let mut doc = Document::new(kind);
        for item in items {
            let id = doc.alloc_id();
            doc.items.insert(id, item);
        }
        doc
    }

    pub fn from_parts(kind: DocKind, items: BTreeMap<ItemId, Item>, cache: BTreeMap<String, CachedPart>) -> Self {
        let next_id = items.keys().next_back().map_or(0, |id| id.0 + 1);
        Self { kind, next_id, items, cache, footprints: BTreeMap::new(), bus_aliases: BTreeMap::new() }
    }

    /// Group-bus aliases are schematic metadata, preserved alongside library snapshots.
    pub fn bus_aliases(&self) -> &BTreeMap<String, Vec<String>> { &self.bus_aliases }

    pub fn with_bus_aliases(mut self, aliases: BTreeMap<String, Vec<String>>) -> Self {
        self.bus_aliases = aliases; self
    }

    pub fn kind(&self) -> DocKind {
        self.kind
    }

    pub fn items(&self) -> impl Iterator<Item = (ItemId, &Item)> {
        self.items.iter().map(|(id, item)| (*id, item))
    }

    /// Items in id order without ids (for writing back into a library entry).
    pub fn item_list(&self) -> Vec<Item> {
        self.items.values().cloned().collect()
    }

    pub fn get(&self, id: ItemId) -> Option<&Item> {
        self.items.get(&id)
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn cache(&self) -> &BTreeMap<String, CachedPart> {
        &self.cache
    }

    pub fn cached_part(&self, key: &str) -> Option<&CachedPart> {
        self.cache.get(key)
    }

    /// Stores (or refreshes) a library snapshot; returns its key.
    pub fn footprint_cache(&self) -> &BTreeMap<String, Footprint> {
        &self.footprints
    }

    pub fn cached_footprint(&self, key: &str) -> Option<&Footprint> {
        self.footprints.get(key)
    }

    /// Stores footprint geometry used by placed footprints (PCB documents).
    pub fn put_cached_footprint(&mut self, key: impl Into<String>, footprint: Footprint) {
        self.footprints.insert(key.into(), footprint);
    }

    /// Replaces the whole footprint cache (used when loading files).
    pub fn with_footprints(mut self, footprints: BTreeMap<String, Footprint>) -> Self {
        self.footprints = footprints;
        self
    }

    pub fn put_cached_part(&mut self, part: CachedPart) -> String {
        let key = part.key();
        self.cache.insert(key.clone(), part);
        key
    }

    pub(crate) fn alloc_id(&mut self) -> ItemId {
        let id = ItemId(self.next_id);
        self.next_id += 1;
        id
    }

    /// Applies edits in order. Edits are produced by [`crate::Command`]
    /// resolution or by history inversion, so they are always consistent.
    pub(crate) fn apply(&mut self, edits: &[Edit]) {
        for edit in edits {
            match &edit.after {
                Some(item) => {
                    self.items.insert(edit.id, item.clone());
                }
                None => {
                    self.items.remove(&edit.id);
                }
            }
        }
    }
}
