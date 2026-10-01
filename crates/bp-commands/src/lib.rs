//! Every change to a document is a [`Command`]. Applying a command returns
//! its inverse, and [`History`] groups commands into undoable transactions.
//!
//! Commands are small and property-level ("set the bounds of X"), which is
//! what keeps the model CRDT-friendly.

use bp_model::{Document, Element, ElementId, LayerId, OrderKey, Style};
use kurbo::Rect;
use std::time::{Duration, Instant};

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    Insert(Box<Element>),
    Remove(ElementId),
    SetBounds { id: ElementId, bounds: Rect },
    SetText { id: ElementId, text: String },
    SetStyle { id: ElementId, style: Style },
    SetOrder { id: ElementId, order: OrderKey },
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum CommandError {
    #[error("element {0} does not exist")]
    MissingElement(ElementId),
    #[error("element {0} already exists")]
    DuplicateElement(ElementId),
    #[error("layer {0} does not exist")]
    MissingLayer(LayerId),
}

/// Which property a command sets, so repeated sets can be coalesced.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Property {
    Bounds,
    Text,
    Style,
    Order,
}

impl Command {
    /// Applies the command and returns the command that undoes it.
    pub fn apply(self, doc: &mut Document) -> Result<Command, CommandError> {
        match self {
            Command::Insert(element) => {
                if doc.elements.contains_key(&element.id) {
                    return Err(CommandError::DuplicateElement(element.id));
                }
                if !doc.layers.contains_key(&element.layer) {
                    return Err(CommandError::MissingLayer(element.layer));
                }
                let id = element.id;
                doc.elements.insert(id, *element);
                Ok(Command::Remove(id))
            }
            Command::Remove(id) => {
                let element = doc
                    .elements
                    .remove(&id)
                    .ok_or(CommandError::MissingElement(id))?;
                Ok(Command::Insert(Box::new(element)))
            }
            Command::SetBounds { id, bounds } => {
                let old = std::mem::replace(&mut element_mut(doc, id)?.bounds, bounds.abs());
                Ok(Command::SetBounds { id, bounds: old })
            }
            Command::SetText { id, text } => {
                let old = std::mem::replace(&mut element_mut(doc, id)?.text, text);
                Ok(Command::SetText { id, text: old })
            }
            Command::SetStyle { id, style } => {
                let old = std::mem::replace(&mut element_mut(doc, id)?.style, style);
                Ok(Command::SetStyle { id, style: old })
            }
            Command::SetOrder { id, order } => {
                let old = std::mem::replace(&mut element_mut(doc, id)?.order, order);
                Ok(Command::SetOrder { id, order: old })
            }
        }
    }

    fn coalesce_key(&self) -> Option<(ElementId, Property)> {
        match self {
            Command::SetBounds { id, .. } => Some((*id, Property::Bounds)),
            Command::SetText { id, .. } => Some((*id, Property::Text)),
            Command::SetStyle { id, .. } => Some((*id, Property::Style)),
            Command::SetOrder { id, .. } => Some((*id, Property::Order)),
            Command::Insert(_) | Command::Remove(_) => None,
        }
    }
}

fn element_mut(doc: &mut Document, id: ElementId) -> Result<&mut Element, CommandError> {
    doc.elements
        .get_mut(&id)
        .ok_or(CommandError::MissingElement(id))
}

/// One undo step.
#[derive(Debug)]
struct Transaction {
    label: String,
    serial: u64,
    forward: Vec<Command>,
    inverse: Vec<Command>,
    merge_key: Option<String>,
    touched: Instant,
}

impl Transaction {
    fn new(label: String, serial: u64) -> Self {
        Self {
            label,
            serial,
            forward: Vec::new(),
            inverse: Vec::new(),
            merge_key: None,
            touched: Instant::now(),
        }
    }

    /// Applies `command`. A repeated set of the same property keeps only the
    /// first inverse and the last forward value, so a 200-frame drag is one
    /// small undo step.
    fn push(&mut self, doc: &mut Document, command: Command) -> Result<(), CommandError> {
        let key = command.coalesce_key();
        let inverse = command.clone().apply(doc)?;
        self.touched = Instant::now();
        if let Some(key) = key
            && let Some(slot) = self
                .forward
                .iter_mut()
                .find(|c| c.coalesce_key() == Some(key))
        {
            *slot = command;
            return Ok(());
        }
        self.forward.push(command);
        self.inverse.push(inverse);
        Ok(())
    }

    fn rollback(self, doc: &mut Document) {
        for command in self.inverse.into_iter().rev() {
            let result = command.apply(doc);
            debug_assert!(result.is_ok(), "rollback failed: {result:?}");
        }
    }
}

/// Undo/redo stacks. Changes made between [`History::begin`] and
/// [`History::commit`] form one undo step.
#[derive(Debug)]
pub struct History {
    undo: Vec<Transaction>,
    redo: Vec<Transaction>,
    open: Option<Transaction>,
    next_serial: u64,
    limit: usize,
}

/// Edits with the same merge key closer together than this become one step.
pub const MERGE_WINDOW: Duration = Duration::from_millis(1000);

impl Default for History {
    fn default() -> Self {
        Self {
            undo: Vec::new(),
            redo: Vec::new(),
            open: None,
            next_serial: 1,
            limit: 500,
        }
    }
}

impl History {
    pub fn new() -> Self {
        Self::default()
    }

    fn serial(&mut self) -> u64 {
        let s = self.next_serial;
        self.next_serial += 1;
        s
    }

    /// Applies `commands` as one undo step (or adds them to the open step).
    /// If any command fails, everything already applied is rolled back.
    pub fn apply(
        &mut self,
        doc: &mut Document,
        label: impl Into<String>,
        commands: impl IntoIterator<Item = Command>,
    ) -> Result<(), CommandError> {
        if let Some(open) = self.open.as_mut() {
            for command in commands {
                open.push(doc, command)?;
            }
            return Ok(());
        }
        let serial = self.serial();
        let mut txn = Transaction::new(label.into(), serial);
        for command in commands {
            if let Err(e) = txn.push(doc, command) {
                txn.rollback(doc);
                return Err(e);
            }
        }
        self.finish(txn);
        Ok(())
    }

    /// Like [`History::apply`], but merges into the previous step when it has
    /// the same `merge_key` and was touched within [`MERGE_WINDOW`]. Used for
    /// sliders and colour pickers that fire every frame.
    pub fn apply_merging(
        &mut self,
        doc: &mut Document,
        label: impl Into<String>,
        merge_key: impl Into<String>,
        commands: impl IntoIterator<Item = Command>,
    ) -> Result<(), CommandError> {
        let merge_key = merge_key.into();
        let mergeable = self.open.is_none()
            && self.redo.is_empty()
            && self.undo.last().is_some_and(|t| {
                t.merge_key.as_deref() == Some(merge_key.as_str())
                    && t.touched.elapsed() < MERGE_WINDOW
            });
        if mergeable {
            let serial = self.serial();
            let top = self.undo.last_mut().expect("checked above");
            for command in commands {
                top.push(doc, command)?;
            }
            top.serial = serial;
            return Ok(());
        }
        self.apply(doc, label, commands)?;
        if let Some(top) = self.undo.last_mut() {
            top.merge_key = Some(merge_key);
        }
        Ok(())
    }

    /// Opens a step that collects every change until [`History::commit`].
    pub fn begin(&mut self, label: impl Into<String>) {
        if self.open.is_none() {
            let serial = self.serial();
            self.open = Some(Transaction::new(label.into(), serial));
        }
    }

    /// Closes the open step. An empty step is dropped.
    pub fn commit(&mut self) {
        if let Some(txn) = self.open.take() {
            self.finish(txn);
        }
    }

    /// Undoes everything in the open step and discards it.
    pub fn cancel(&mut self, doc: &mut Document) {
        if let Some(txn) = self.open.take() {
            txn.rollback(doc);
        }
    }

    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }

    fn finish(&mut self, txn: Transaction) {
        if txn.forward.is_empty() {
            return;
        }
        self.redo.clear();
        self.undo.push(txn);
        if self.undo.len() > self.limit {
            self.undo.remove(0);
        }
    }

    pub fn undo(&mut self, doc: &mut Document) -> bool {
        self.commit();
        let Some(txn) = self.undo.pop() else {
            return false;
        };
        for command in txn.inverse.iter().rev() {
            let result = command.clone().apply(doc);
            debug_assert!(result.is_ok(), "undo failed: {result:?}");
        }
        self.redo.push(txn);
        true
    }

    pub fn redo(&mut self, doc: &mut Document) -> bool {
        self.commit();
        let Some(txn) = self.redo.pop() else {
            return false;
        };
        for command in &txn.forward {
            let result = command.clone().apply(doc);
            debug_assert!(result.is_ok(), "redo failed: {result:?}");
        }
        self.undo.push(txn);
        true
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty() || self.open.as_ref().is_some_and(|t| !t.forward.is_empty())
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo_label(&self) -> Option<&str> {
        self.undo.last().map(|t| t.label.as_str())
    }

    pub fn redo_label(&self) -> Option<&str> {
        self.redo.last().map(|t| t.label.as_str())
    }

    /// Identifies the current document state. Compare it with the value at
    /// the last save to know whether there are unsaved changes; undoing back
    /// to the saved state makes them equal again.
    pub fn state_id(&self) -> u64 {
        self.undo.last().map_or(0, |t| t.serial)
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bp_model::ShapeKind;

    fn setup() -> (Document, Element) {
        let doc = Document::new();
        let page = doc.first_page().unwrap();
        let layer = doc.layers_of(page)[0].id;
        let el = Element::new(
            ShapeKind::Rectangle,
            layer,
            doc.next_order_key(layer),
            Rect::new(0.0, 0.0, 10.0, 10.0),
        );
        (doc, el)
    }

    #[test]
    fn insert_undo_redo() {
        let (mut doc, el) = setup();
        let mut h = History::new();
        h.apply(&mut doc, "Add", [Command::Insert(Box::new(el.clone()))])
            .unwrap();
        assert!(doc.elements.contains_key(&el.id));
        assert!(h.undo(&mut doc));
        assert!(doc.elements.is_empty());
        assert!(h.redo(&mut doc));
        assert_eq!(doc.elements[&el.id], el);
        assert!(!h.redo(&mut doc));
    }

    #[test]
    fn drag_is_one_step() {
        let (mut doc, el) = setup();
        let id = el.id;
        let mut h = History::new();
        h.apply(&mut doc, "Add", [Command::Insert(Box::new(el))])
            .unwrap();
        h.begin("Move");
        for i in 1..=100 {
            let x = f64::from(i);
            h.apply(
                &mut doc,
                "",
                [Command::SetBounds {
                    id,
                    bounds: Rect::new(x, 0.0, x + 10.0, 10.0),
                }],
            )
            .unwrap();
        }
        h.commit();
        assert_eq!(doc.elements[&id].bounds.x0, 100.0);
        assert_eq!(h.undo_label(), Some("Move"));
        h.undo(&mut doc);
        assert_eq!(doc.elements[&id].bounds.x0, 0.0);
        h.redo(&mut doc);
        assert_eq!(doc.elements[&id].bounds.x0, 100.0);
    }

    #[test]
    fn failed_step_rolls_back() {
        let (mut doc, el) = setup();
        let id = el.id;
        let mut h = History::new();
        let result = h.apply(
            &mut doc,
            "Broken",
            [
                Command::Insert(Box::new(el)),
                Command::Remove(ElementId::new()),
            ],
        );
        assert!(result.is_err());
        assert!(!doc.elements.contains_key(&id));
        assert!(!h.can_undo());
    }

    #[test]
    fn merging_edits_become_one_step() {
        let (mut doc, el) = setup();
        let id = el.id;
        let mut h = History::new();
        h.apply(&mut doc, "Add", [Command::Insert(Box::new(el))])
            .unwrap();
        for w in [2.0, 3.0, 4.0] {
            let style = Style {
                stroke_width: w,
                ..Style::default()
            };
            h.apply_merging(
                &mut doc,
                "Stroke",
                format!("style:{id}"),
                [Command::SetStyle { id, style }],
            )
            .unwrap();
        }
        assert_eq!(doc.elements[&id].style.stroke_width, 4.0);
        h.undo(&mut doc);
        assert_eq!(doc.elements[&id].style.stroke_width, 1.5);
        assert_eq!(h.undo_label(), Some("Add"));
    }

    #[test]
    fn state_id_tracks_saved_state() {
        let (mut doc, el) = setup();
        let mut h = History::new();
        let saved = h.state_id();
        h.apply(&mut doc, "Add", [Command::Insert(Box::new(el))])
            .unwrap();
        assert_ne!(h.state_id(), saved);
        h.undo(&mut doc);
        assert_eq!(h.state_id(), saved);
    }
}