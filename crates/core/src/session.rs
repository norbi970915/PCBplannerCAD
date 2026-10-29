use serde::Serialize;

use crate::command::Command;
use crate::document::{DocKind, Document, Edit, IdItem, ItemId};
use crate::error::CoreError;
use crate::library::CachedPart;
use crate::item::Item;

#[derive(Clone, Debug)]
struct Transaction {
    serial: u64,
    edits: Vec<Edit>,
}

/// What changed, in a form the UI can patch its view state with.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Delta {
    pub upserted: Vec<IdItem>,
    pub removed: Vec<ItemId>,
}

impl Delta {
    fn from_edits<'a>(edits: impl IntoIterator<Item = &'a Edit>) -> Self {
        let mut delta = Delta::default();
        for edit in edits {
            match &edit.after {
                Some(item) => delta.upserted.push(IdItem { id: edit.id, item: item.clone() }),
                None => delta.removed.push(edit.id),
            }
        }
        delta
    }
}

/// An open document together with its undo history and save state.
#[derive(Debug)]
pub struct Session {
    doc: Document,
    undo: Vec<Transaction>,
    redo: Vec<Transaction>,
    next_serial: u64,
    /// Serial of the transaction on top of the undo stack when last saved
    /// (0 = the empty history).
    saved_serial: u64,
    /// Set by changes outside the undo history (e.g. cache updates).
    extra_dirty: bool,
}

impl Session {
    pub fn new(kind: DocKind) -> Self {
        Self::with_document(Document::new(kind))
    }

    pub fn with_document(doc: Document) -> Self {
        Self { doc, undo: Vec::new(), redo: Vec::new(), next_serial: 1, saved_serial: 0, extra_dirty: false }
    }

    pub fn document(&self) -> &Document {
        &self.doc
    }

    /// Stores a library snapshot for placed parts (not part of undo history).
    pub fn put_cached_part(&mut self, part: CachedPart) -> String {
        if self.doc.cached_part(&part.key()) != Some(&part) {
            self.extra_dirty = true;
        }
        self.doc.put_cached_part(part)
    }

    /// Stores footprint geometry for placed footprints (not part of undo history).
    pub fn put_cached_footprint(&mut self, key: impl Into<String>, footprint: crate::library::Footprint) {
        let key = key.into();
        if self.doc.cached_footprint(&key) != Some(&footprint) {
            self.extra_dirty = true;
        }
        self.doc.put_cached_footprint(key, footprint);
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    fn current_serial(&self) -> u64 {
        self.undo.last().map_or(0, |t| t.serial)
    }

    pub fn is_dirty(&self) -> bool {
        self.extra_dirty || self.current_serial() != self.saved_serial
    }

    pub fn mark_saved(&mut self) {
        self.saved_serial = self.current_serial();
        self.extra_dirty = false;
    }

    /// Marks the document as modified without an undo step.
    pub fn mark_dirty(&mut self) {
        self.extra_dirty = true;
    }

    pub fn execute(&mut self, cmd: Command) -> Result<Delta, CoreError> {
        let mut edits = cmd.resolve(&mut self.doc)?;
        if edits.is_empty() {
            return Ok(Delta::default());
        }
        self.doc.apply(&edits);
        // Teardrops left without their track or pad go in the same undo step.
        if self.doc.kind() == DocKind::Pcb {
            let prune = crate::pcb::teardrop::prune(&mut self.doc, &edits);
            edits.extend(prune);
            edits.extend(crate::pcb::teardrop::maintain(&mut self.doc, &edits));
        }
        Ok(self.record(edits))
    }

    /// Validate component placements and board vias on their actual copper span before
    /// publishing an edit. Includes paste/arrays, properties and transforms.
    /// Rejection preserves geometry, allocated IDs, dirty state and both histories.
    pub fn execute_with_via_checks(&mut self, cmd: Command, enforce: bool) -> Result<Delta, CoreError> {
        let contains_via = |items: &[Item]| items.iter().any(|i| matches!(i, Item::Via(_)));
        let replaces_via = |items: &[IdItem]| items.iter().any(|i| matches!(i.item, Item::Via(_)));
        let moves_via = |ids: &[ItemId]| ids.iter().any(|id| matches!(self.doc.get(*id), Some(Item::Via(_))));
        let touches_via = match &cmd {
            Command::Add { items } => contains_via(items),
            Command::Change { delete, add } => contains_via(add) || moves_via(delete),
            Command::Delete { ids } => moves_via(ids),
            Command::Replace { id, item } => matches!(item, Item::Via(_)) || moves_via(&[*id]),
            Command::ReplaceMany { items } => replaces_via(items) || items.iter().any(|i| matches!(self.doc.get(i.id), Some(Item::Via(_)))),
            Command::Move { ids, .. } | Command::Rotate { ids, .. } | Command::Mirror { ids, .. } => moves_via(ids),
            Command::Route { add, replace, .. } => contains_via(add) || replaces_via(replace),
            _ => false,
        };
        let part_ids=|ids:&[ItemId]|ids.iter().any(|id|matches!(self.doc.get(*id),Some(Item::Footprint(_))));
        let part_items=|items:&[Item]|items.iter().any(|i|matches!(i,Item::Footprint(_)));
        let part_replacements=|items:&[IdItem]|items.iter().any(|i|matches!(i.item,Item::Footprint(_)))||part_ids(&items.iter().map(|i|i.id).collect::<Vec<_>>());
        let touches_part=match &cmd {
            Command::Add{items}=>part_items(items),
            Command::Change{delete,add}=>part_items(add)||part_ids(delete),
            Command::Delete{ids}=>part_ids(ids),
            Command::Replace{id,item}=>matches!(item,Item::Footprint(_))||part_ids(&[*id]),
            Command::ReplaceMany{items}=>part_replacements(items),
            Command::Move{ids,..}|Command::Rotate{ids,..}|Command::Mirror{ids,..}=>part_ids(ids),
            Command::Route{add,replace,delete}=>part_items(add)||part_replacements(replace)||part_ids(delete),
            _=>false,
        };
        if self.doc.kind() != DocKind::Pcb || (!touches_via&&!touches_part) { return self.execute(cmd); }
        let routed = matches!(&cmd, Command::Route { .. });
        let mut planned = Self::with_document(self.doc.clone());
        let delta = planned.execute(cmd)?;
        let vias: Vec<_> = delta.upserted.iter().filter_map(|entry| {
            let Item::Via(via) = &entry.item else { return None; };
            if let Some(Item::Via(before)) = self.doc.get(entry.id) {
                // Existing violations can still be locked/unlocked for repair.
                let mut unchanged = before.clone();
                unchanged.locked = via.locked;
                if unchanged == *via { return None; }
            }
            Some(entry.id)
        }).collect();
        let components:Vec<_>=delta.upserted.iter().filter_map(|entry|{
            let Item::Footprint(after)=&entry.item else{return None;};
            if let Some(Item::Footprint(before))=self.doc.get(entry.id) {
                if before.pos==after.pos&&before.rotation==after.rotation&&before.side==after.side&&before.footprint==after.footprint&&before.pad_nets==after.pad_nets{return None;}
            }
            Some(entry.id)
        }).collect();
        let removed_part=delta.removed.iter().any(|id|matches!(self.doc.get(*id),Some(Item::Footprint(_))))
            ||delta.upserted.iter().any(|i|matches!(self.doc.get(i.id),Some(Item::Footprint(_)))&&!matches!(i.item,Item::Footprint(_)));
        let removed_via = delta.removed.iter().any(|id| matches!(self.doc.get(*id), Some(Item::Via(_))))
            || delta.upserted.iter().any(|i| matches!(self.doc.get(i.id), Some(Item::Via(_))) && !matches!(i.item, Item::Via(_)));
        if (!vias.is_empty() || removed_via || !components.is_empty() || removed_part) && !routed {
            // Generic edits need the same final poured copper as routing. Refill
            // first so a legal via can clear a foreign-net pour.
            let mut refill = Vec::new();
            for (id, fill) in crate::pcb::fill::fill_all(&planned.doc) {
                if let Some(Item::Zone(zone)) = planned.doc.get(id) {
                    if zone.fill == fill { continue; }
                    let mut next = zone.clone();
                    next.fill = fill;
                    refill.push(Edit { id, before: Some(Item::Zone(zone.clone())), after: Some(Item::Zone(next)) });
                }
            }
            planned.doc.apply(&refill);
            if let Some(transaction) = planned.undo.last_mut() { transaction.edits.extend(refill); }
        }
        if enforce&&!components.is_empty() {
            if let Some(violation)=crate::pcb::drc::run_near(&planned.doc,&components).into_iter().find(|v|v.severity==crate::pcb::drc::Severity::Error){
                return Err(CoreError::PlacementBlocked(violation.message));
            }
        }
        if enforce && !vias.is_empty() {
            // A paired transition can add fanout necks as well as annuli. Validate
            // every changed route conductor, including the far ends of those necks.
            let checked = if routed {delta.upserted.iter().filter(|entry|matches!(entry.item,Item::Track(_)|Item::ArcTrack(_)|Item::Via(_))).map(|entry|entry.id).collect()} else {vias};
            if let Some(violation) = crate::pcb::drc::run_near(&planned.doc, &checked).into_iter()
                .find(|v| v.severity == crate::pcb::drc::Severity::Error) {
                return Err(CoreError::ViaBlocked(violation.message));
            }
        }
        if let Some(transaction) = planned.undo.pop() {
            self.doc = planned.doc;
            return Ok(self.record(transaction.edits));
        }
        Ok(delta)
    }
    fn record(&mut self, edits: Vec<Edit>) -> Delta {
        let delta = Delta::from_edits(&edits);
        self.undo.push(Transaction { serial: self.next_serial, edits });
        self.next_serial += 1;
        self.redo.clear();
        delta
    }

    pub fn undo(&mut self) -> Option<Delta> {
        let tx = self.undo.pop()?;
        let inverse: Vec<Edit> = tx.edits.iter().rev().map(Edit::inverted).collect();
        self.doc.apply(&inverse);
        self.redo.push(tx);
        Some(Delta::from_edits(&inverse))
    }

    pub fn redo(&mut self) -> Option<Delta> {
        let tx = self.redo.pop()?;
        self.doc.apply(&tx.edits);
        let delta = Delta::from_edits(&tx.edits);
        self.undo.push(tx);
        Some(delta)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::LayerId;
    use crate::item::{Item, Line, Wire};
    use crate::units::{Point, mm};

    fn line(x0: f64, y0: f64, x1: f64, y1: f64) -> Item {
        Item::Line(Line {
            a: Point::new(mm(x0), mm(y0)),
            b: Point::new(mm(x1), mm(y1)),
            width: mm(0.25),
            layer: LayerId(0),
        })
    }

    #[test]
    fn add_undo_redo() {
        let mut s = Session::new(DocKind::Pcb);
        let delta = s.execute(Command::Add { items: vec![line(0.0, 0.0, 10.0, 0.0)] }).unwrap();
        let id = delta.upserted[0].id;
        assert_eq!(s.document().len(), 1);
        assert!(s.is_dirty());

        let undo = s.undo().unwrap();
        assert_eq!(undo.removed, vec![id]);
        assert!(s.document().is_empty());
        assert!(!s.is_dirty());

        s.redo().unwrap();
        assert_eq!(s.document().get(id), Some(&line(0.0, 0.0, 10.0, 0.0)));
    }

    #[test]
    fn move_rotate_mirror_and_delete_are_reversible() {
        let mut s = Session::new(DocKind::Pcb);
        let id = s.execute(Command::Add { items: vec![line(0.0, 0.0, 1.0, 1.0)] }).unwrap().upserted[0].id;
        s.execute(Command::Move { ids: vec![id], dx: mm(2.0), dy: mm(-1.0) }).unwrap();
        assert_eq!(s.document().get(id), Some(&line(2.0, -1.0, 3.0, 0.0)));
        s.execute(Command::Rotate { ids: vec![id], center: Point::new(mm(2.0), mm(-1.0)), angle: 90.0 }).unwrap();
        assert_eq!(s.document().get(id), Some(&line(2.0, -1.0, 1.0, 0.0)));
        s.execute(Command::Mirror { ids: vec![id], center: Point::new(mm(2.0), 0) }).unwrap();
        assert_eq!(s.document().get(id), Some(&line(2.0, -1.0, 3.0, 0.0)));
        s.execute(Command::Delete { ids: vec![id] }).unwrap();
        assert!(s.document().is_empty());

        for _ in 0..4 {
            s.undo();
        }
        assert_eq!(s.document().get(id), Some(&line(0.0, 0.0, 1.0, 1.0)));
    }

    #[test]
    fn change_is_one_undo_step() {
        let mut s = Session::new(DocKind::Schematic);
        let w = |x0: f64, x1: f64| Item::Wire(Wire { a: Point::new(mm(x0), 0), b: Point::new(mm(x1), 0) });
        let id = s.execute(Command::Add { items: vec![w(0.0, 10.0)] }).unwrap().upserted[0].id;
        s.execute(Command::Change { delete: vec![id], add: vec![w(0.0, 5.0), w(5.0, 10.0)] }).unwrap();
        assert_eq!(s.document().len(), 2);
        s.undo();
        assert_eq!(s.document().len(), 1);
        assert_eq!(s.document().get(id), Some(&w(0.0, 10.0)));
    }

    #[test]
    fn new_command_clears_redo_and_dirty_tracks_save_point() {
        let mut s = Session::new(DocKind::Pcb);
        s.execute(Command::Add { items: vec![line(0.0, 0.0, 1.0, 0.0)] }).unwrap();
        s.mark_saved();
        assert!(!s.is_dirty());
        s.undo();
        assert!(s.is_dirty());
        s.execute(Command::Add { items: vec![line(0.0, 0.0, 0.0, 1.0)] }).unwrap();
        assert!(!s.can_redo());
        assert!(s.is_dirty());
    }

    #[test]
    fn invalid_commands_leave_document_untouched() {
        let mut s = Session::new(DocKind::Pcb);
        assert!(s.execute(Command::Delete { ids: vec![ItemId(42)] }).is_err());
        assert!(s.execute(Command::Add { items: vec![line(1.0, 1.0, 1.0, 1.0)] }).is_err());
        assert!(s.document().is_empty());
        assert!(!s.can_undo());
    }
}
