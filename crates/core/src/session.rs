use serde::Serialize;

use crate::command::Command;
use crate::document::{DocKind, Document, Edit, IdItem, ItemId};
use crate::error::CoreError;
use crate::library::CachedPart;

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
        let edits = cmd.resolve(&mut self.doc)?;
        if edits.is_empty() {
            return Ok(Delta::default());
        }
        self.doc.apply(&edits);
        let delta = Delta::from_edits(&edits);
        self.undo.push(Transaction { serial: self.next_serial, edits });
        self.next_serial += 1;
        self.redo.clear();
        Ok(delta)
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
