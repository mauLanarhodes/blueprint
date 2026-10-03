//! Every change to a document is a [`Command`]. Applying a command returns
//! its inverse, and [`History`] groups commands into undoable transactions.
//!
//! Commands are small and property-level ("set the fill of X"), which is
//! what keeps the model CRDT-friendly. They also keep the document valid:
//! a command that would leave a dangling reference (a connector glued to a
//! removed shape, a child without its group) fails instead, and the whole
//! transaction is rolled back.

pub mod edit;

use bp_model::kurbo::{Point, Rect};
use bp_model::{
    Color, ColumnId, Dash, Document, Element, ElementId, ElementKind, Endpoint, ErdColumn,
    ErdTable, Layer, LayerId, Marker, ModelError, OrderKey, Page, PageId, Paint, Parent, Routing,
    ShapeRef, SqlDialect, Style, TableDisplay, TextAlign, VerticalAlign,
};
use std::mem::{Discriminant, discriminant, replace};
use std::time::{Duration, Instant};

/// One settable property of an element. Style properties are overrides:
/// `None` returns the property to the shape's default.
#[derive(Clone, Debug, PartialEq)]
pub enum Prop {
    Locked(bool),
    Order(OrderKey),
    Parent(Parent),
    Bounds(Rect),
    Shape(ShapeRef),
    Text(String),
    Fill(Option<Paint>),
    Stroke(Option<Paint>),
    StrokeWidth(Option<f64>),
    Dash(Option<Dash>),
    Opacity(Option<f64>),
    Shadow(Option<bool>),
    CornerRadius(Option<f64>),
    TextColor(Option<Color>),
    FontSize(Option<f64>),
    Bold(Option<bool>),
    Italic(Option<bool>),
    TextAlign(Option<TextAlign>),
    VerticalAlign(Option<VerticalAlign>),
    Source(Endpoint),
    Target(Endpoint),
    Waypoints(Vec<Point>),
    Routing(Routing),
    StartMarker(Marker),
    EndMarker(Marker),
    LabelPosition(f64),
    TableDisplay(TableDisplay),
    SqlDialect(SqlDialect),
}

impl Prop {
    /// Properties that point at other elements. Sets of these are never
    /// coalesced, because reordering them could break a reference.
    fn is_reference(&self) -> bool {
        matches!(self, Prop::Parent(_) | Prop::Source(_) | Prop::Target(_))
    }

    pub fn name(&self) -> &'static str {
        match self {
            Prop::Locked(_) => "locked",
            Prop::Order(_) => "order",
            Prop::Parent(_) => "parent",
            Prop::Bounds(_) => "bounds",
            Prop::Shape(_) => "shape",
            Prop::Text(_) => "text",
            Prop::Fill(_) => "fill",
            Prop::Stroke(_) => "stroke",
            Prop::StrokeWidth(_) => "stroke width",
            Prop::Dash(_) => "dash",
            Prop::Opacity(_) => "opacity",
            Prop::Shadow(_) => "shadow",
            Prop::CornerRadius(_) => "corner radius",
            Prop::TextColor(_) => "text colour",
            Prop::FontSize(_) => "font size",
            Prop::Bold(_) => "bold",
            Prop::Italic(_) => "italic",
            Prop::TextAlign(_) => "text alignment",
            Prop::VerticalAlign(_) => "vertical alignment",
            Prop::Source(_) => "source",
            Prop::Target(_) => "target",
            Prop::Waypoints(_) => "waypoints",
            Prop::Routing(_) => "routing",
            Prop::StartMarker(_) => "start marker",
            Prop::EndMarker(_) => "end marker",
            Prop::LabelPosition(_) => "label position",
            Prop::TableDisplay(_) => "table display",
            Prop::SqlDialect(_) => "SQL dialect",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum PageProp {
    Name(String),
    Order(OrderKey),
    Background(Color),
}

#[derive(Clone, Debug, PartialEq)]
pub enum LayerProp {
    Name(String),
    Order(OrderKey),
    Visible(bool),
    Locked(bool),
}

/// One independently editable column property; column ids remain stable.
#[derive(Clone, Debug, PartialEq)]
pub enum ColumnProp {
    Name(String),
    DataType(String),
    Order(OrderKey),
    PrimaryKey(bool),
    ForeignKey(bool),
    Unique(bool),
    Nullable(bool),
    DefaultValue(Option<String>),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    Insert(Box<Element>),
    Remove(ElementId),
    Set {
        id: ElementId,
        prop: Prop,
    },
    InsertPage(Box<Page>),
    RemovePage(PageId),
    SetPage {
        id: PageId,
        prop: PageProp,
    },
    InsertLayer(Box<Layer>),
    RemoveLayer(LayerId),
    SetLayer {
        id: LayerId,
        prop: LayerProp,
    },
    InsertColumn {
        id: ElementId,
        column: Box<ErdColumn>,
    },
    /// An inverse insertion that preserves the table's exact stored row sequence.
    #[doc(hidden)]
    RestoreColumn {
        id: ElementId,
        column: Box<ErdColumn>,
        index: usize,
    },
    RemoveColumn {
        id: ElementId,
        column: ColumnId,
    },
    SetColumn {
        id: ElementId,
        column: ColumnId,
        prop: ColumnProp,
    },
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum CommandError {
    #[error("element {0} does not exist")]
    MissingElement(ElementId),
    #[error("element {0} already exists")]
    DuplicateElement(ElementId),
    #[error("page {0} does not exist")]
    MissingPage(PageId),
    #[error("page {0} already exists")]
    DuplicatePage(PageId),
    #[error("layer {0} does not exist")]
    MissingLayer(LayerId),
    #[error("layer {0} already exists")]
    DuplicateLayer(LayerId),
    #[error("a document needs at least one page")]
    LastPage,
    #[error("{0} still has contents; remove them first")]
    NotEmpty(String),
    #[error("element {0} still has connectors attached")]
    InUse(ElementId),
    #[error("element {id} has no {prop} property")]
    WrongKind { id: ElementId, prop: &'static str },
    #[error("element {0} cannot be moved inside itself")]
    Cycle(ElementId),
    #[error("table {id} has no column {column}")]
    MissingColumn { id: ElementId, column: ColumnId },
    #[error("table {id} already has column {column}")]
    DuplicateColumn { id: ElementId, column: ColumnId },
    #[error("column {column} in table {id} still has relationships attached")]
    InUseColumn { id: ElementId, column: ColumnId },
    #[error(transparent)]
    Invalid(#[from] ModelError),
}

/// What a command changes, for coalescing repeated sets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Target {
    Element(ElementId),
    Page(PageId),
    Layer(LayerId),
    Column(ElementId, ColumnId),
}

/// A target and which of its properties a set changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CoalesceKey {
    Element(ElementId, Discriminant<Prop>),
    Page(PageId, Discriminant<PageProp>),
    Layer(LayerId, Discriminant<LayerProp>),
    Column(ElementId, ColumnId, Discriminant<ColumnProp>),
}

impl CoalesceKey {
    fn target(self) -> Target {
        match self {
            CoalesceKey::Element(id, _) => Target::Element(id),
            CoalesceKey::Page(id, _) => Target::Page(id),
            CoalesceKey::Layer(id, _) => Target::Layer(id),
            CoalesceKey::Column(id, column, _) => Target::Column(id, column),
        }
    }
}

impl Command {
    /// Applies the command and returns the command that undoes it.
    pub fn apply(self, doc: &mut Document) -> Result<Command, CommandError> {
        match self {
            Command::Insert(element) => {
                if doc.elements.contains_key(&element.id) {
                    return Err(CommandError::DuplicateElement(element.id));
                }
                doc.check_parent(element.id, element.parent)?;
                doc.check_kind(&element)?;
                let id = element.id;
                doc.elements.insert(id, *element);
                Ok(Command::Remove(id))
            }
            Command::Remove(id) => {
                if !doc.elements.contains_key(&id) {
                    return Err(CommandError::MissingElement(id));
                }
                if doc
                    .elements
                    .values()
                    .any(|e| e.parent == Parent::Element(id))
                {
                    return Err(CommandError::NotEmpty(format!("element {id}")));
                }
                if !doc.connectors_attached_to(id).is_empty() {
                    return Err(CommandError::InUse(id));
                }
                let element = doc.elements.remove(&id).expect("checked above");
                Ok(Command::Insert(Box::new(element)))
            }
            Command::Set { id, prop } => {
                check_prop(doc, id, &prop)?;
                let element = doc
                    .elements
                    .get_mut(&id)
                    .ok_or(CommandError::MissingElement(id))?;
                let old = set_prop(element, prop)?;
                Ok(Command::Set { id, prop: old })
            }
            Command::InsertPage(page) => {
                if doc.pages.contains_key(&page.id) {
                    return Err(CommandError::DuplicatePage(page.id));
                }
                let id = page.id;
                doc.pages.insert(id, *page);
                Ok(Command::RemovePage(id))
            }
            Command::RemovePage(id) => {
                if !doc.pages.contains_key(&id) {
                    return Err(CommandError::MissingPage(id));
                }
                if doc.pages.len() == 1 {
                    return Err(CommandError::LastPage);
                }
                if doc.layers.values().any(|l| l.page == id) {
                    return Err(CommandError::NotEmpty(format!("page {id}")));
                }
                let page = doc.pages.remove(&id).expect("checked above");
                Ok(Command::InsertPage(Box::new(page)))
            }
            Command::SetPage { id, prop } => {
                let page = doc
                    .pages
                    .get_mut(&id)
                    .ok_or(CommandError::MissingPage(id))?;
                let old = match prop {
                    PageProp::Name(v) => PageProp::Name(replace(&mut page.name, v)),
                    PageProp::Order(v) => PageProp::Order(replace(&mut page.order, v)),
                    PageProp::Background(v) => {
                        PageProp::Background(replace(&mut page.background, v))
                    }
                };
                Ok(Command::SetPage { id, prop: old })
            }
            Command::InsertLayer(layer) => {
                if doc.layers.contains_key(&layer.id) {
                    return Err(CommandError::DuplicateLayer(layer.id));
                }
                if !doc.pages.contains_key(&layer.page) {
                    return Err(CommandError::MissingPage(layer.page));
                }
                let id = layer.id;
                doc.layers.insert(id, *layer);
                Ok(Command::RemoveLayer(id))
            }
            Command::RemoveLayer(id) => {
                if !doc.layers.contains_key(&id) {
                    return Err(CommandError::MissingLayer(id));
                }
                if doc.elements.values().any(|e| e.parent == Parent::Layer(id)) {
                    return Err(CommandError::NotEmpty(format!("layer {id}")));
                }
                let layer = doc.layers.remove(&id).expect("checked above");
                Ok(Command::InsertLayer(Box::new(layer)))
            }
            Command::SetLayer { id, prop } => {
                let layer = doc
                    .layers
                    .get_mut(&id)
                    .ok_or(CommandError::MissingLayer(id))?;
                let old = match prop {
                    LayerProp::Name(v) => LayerProp::Name(replace(&mut layer.name, v)),
                    LayerProp::Order(v) => LayerProp::Order(replace(&mut layer.order, v)),
                    LayerProp::Visible(v) => LayerProp::Visible(replace(&mut layer.visible, v)),
                    LayerProp::Locked(v) => LayerProp::Locked(replace(&mut layer.locked, v)),
                };
                Ok(Command::SetLayer { id, prop: old })
            }
            Command::InsertColumn { id, column } => {
                if !column.order.is_valid() {
                    return Err(
                        ModelError::InvalidOrderKey(column.order.as_str().to_owned()).into(),
                    );
                }
                let table = table_mut(doc, id)?;
                if table.column(column.id).is_some() {
                    return Err(CommandError::DuplicateColumn {
                        id,
                        column: column.id,
                    });
                }
                let column_id = column.id;
                table.columns.push(*column);
                Ok(Command::RemoveColumn {
                    id,
                    column: column_id,
                })
            }
            Command::RestoreColumn { id, column, index } => {
                if !column.order.is_valid() {
                    return Err(
                        ModelError::InvalidOrderKey(column.order.as_str().to_owned()).into(),
                    );
                }
                let table = table_mut(doc, id)?;
                if table.column(column.id).is_some() {
                    return Err(CommandError::DuplicateColumn {
                        id,
                        column: column.id,
                    });
                }
                let column_id = column.id;
                table
                    .columns
                    .insert(index.min(table.columns.len()), *column);
                Ok(Command::RemoveColumn {
                    id,
                    column: column_id,
                })
            }
            Command::RemoveColumn { id, column } => {
                if !doc.connectors_attached_to_column(id, column).is_empty() {
                    return Err(CommandError::InUseColumn { id, column });
                }
                let table = table_mut(doc, id)?;
                let index = table
                    .columns
                    .iter()
                    .position(|row| row.id == column)
                    .ok_or(CommandError::MissingColumn { id, column })?;
                Ok(Command::RestoreColumn {
                    id,
                    column: Box::new(table.columns.remove(index)),
                    index,
                })
            }
            Command::SetColumn { id, column, prop } => {
                if let ColumnProp::Order(order) = &prop
                    && !order.is_valid()
                {
                    return Err(ModelError::InvalidOrderKey(order.as_str().to_owned()).into());
                }
                let row = table_mut(doc, id)?
                    .columns
                    .iter_mut()
                    .find(|row| row.id == column)
                    .ok_or(CommandError::MissingColumn { id, column })?;
                let old = match prop {
                    ColumnProp::Name(value) => ColumnProp::Name(replace(&mut row.name, value)),
                    ColumnProp::DataType(value) => {
                        ColumnProp::DataType(replace(&mut row.data_type, value))
                    }
                    ColumnProp::Order(value) => ColumnProp::Order(replace(&mut row.order, value)),
                    ColumnProp::PrimaryKey(value) => {
                        ColumnProp::PrimaryKey(replace(&mut row.primary_key, value))
                    }
                    ColumnProp::ForeignKey(value) => {
                        ColumnProp::ForeignKey(replace(&mut row.foreign_key, value))
                    }
                    ColumnProp::Unique(value) => {
                        ColumnProp::Unique(replace(&mut row.unique, value))
                    }
                    ColumnProp::Nullable(value) => {
                        ColumnProp::Nullable(replace(&mut row.nullable, value))
                    }
                    ColumnProp::DefaultValue(value) => {
                        ColumnProp::DefaultValue(replace(&mut row.default_value, value))
                    }
                };
                Ok(Command::SetColumn {
                    id,
                    column,
                    prop: old,
                })
            }
        }
    }

    /// The target and property of a set that may be coalesced with an
    /// earlier set of the same property in the same transaction.
    fn coalesce_key(&self) -> Option<CoalesceKey> {
        match self {
            Command::Set { id, prop } if !prop.is_reference() => {
                Some(CoalesceKey::Element(*id, discriminant(prop)))
            }
            Command::SetPage { id, prop } => Some(CoalesceKey::Page(*id, discriminant(prop))),
            Command::SetLayer { id, prop } => Some(CoalesceKey::Layer(*id, discriminant(prop))),
            Command::SetColumn { id, column, prop } => {
                Some(CoalesceKey::Column(*id, *column, discriminant(prop)))
            }
            _ => None,
        }
    }

    /// Whether this command creates or destroys `target`, which ends the
    /// range in which sets of `target` may be coalesced.
    fn creates_or_destroys(&self, target: Target) -> bool {
        match (self, target) {
            (Command::Insert(e), Target::Element(id)) => e.id == id,
            (Command::Remove(r), Target::Element(id)) => *r == id,
            (Command::InsertPage(p), Target::Page(id)) => p.id == id,
            (Command::RemovePage(r), Target::Page(id)) => *r == id,
            (Command::InsertLayer(l), Target::Layer(id)) => l.id == id,
            (Command::RemoveLayer(r), Target::Layer(id)) => *r == id,
            (Command::Insert(e), Target::Column(id, _)) => e.id == id,
            (Command::Remove(r), Target::Column(id, _)) => *r == id,
            (Command::InsertColumn { id, column }, Target::Column(target, row)) => {
                *id == target && column.id == row
            }
            (Command::RestoreColumn { id, column, .. }, Target::Column(target, row)) => {
                *id == target && column.id == row
            }
            (Command::RemoveColumn { id, column }, Target::Column(target, row)) => {
                *id == target && *column == row
            }
            _ => false,
        }
    }
}

/// Rejects sets that would break the document before anything changes.
fn check_prop(doc: &Document, id: ElementId, prop: &Prop) -> Result<(), CommandError> {
    let finite = |p: &Point| p.x.is_finite() && p.y.is_finite();
    match prop {
        Prop::Shape(shape) => {
            if let Some(element) = doc.elements.get(&id)
                && let Some(data) = element.as_shape()
            {
                match (shape.as_str() == "erd/table", data.erd.is_some()) {
                    (true, false) => return Err(ModelError::MissingErdData(id).into()),
                    (false, true) => return Err(ModelError::UnexpectedErdData(id).into()),
                    _ => {}
                }
            }
        }
        Prop::Parent(parent) => {
            doc.check_parent(id, *parent)?;
            if let Parent::Element(p) = *parent
                && (p == id || doc.ancestors(p).contains(&id))
            {
                return Err(CommandError::Cycle(id));
            }
        }
        Prop::Source(end) | Prop::Target(end) => match end {
            Endpoint::Glued { .. } => doc.check_endpoint(id, end)?,
            Endpoint::Free(p) if !finite(p) => return Err(ModelError::NotFinite(id).into()),
            Endpoint::Free(_) => {}
        },
        Prop::Bounds(r) if ![r.x0, r.y0, r.x1, r.y1].iter().all(|v| v.is_finite()) => {
            return Err(ModelError::NotFinite(id).into());
        }
        Prop::Waypoints(points) if !points.iter().all(finite) => {
            return Err(ModelError::NotFinite(id).into());
        }
        _ => {}
    }
    Ok(())
}

fn style_mut(element: &mut Element) -> Option<&mut Style> {
    match &mut element.kind {
        ElementKind::Shape(s) => Some(&mut s.style),
        ElementKind::Connector(c) => Some(&mut c.style),
        ElementKind::Group => None,
    }
}

fn table_mut(doc: &mut Document, id: ElementId) -> Result<&mut ErdTable, CommandError> {
    doc.elements
        .get_mut(&id)
        .ok_or(CommandError::MissingElement(id))?
        .as_shape_mut()
        .and_then(|shape| shape.erd.as_mut())
        .ok_or(CommandError::WrongKind {
            id,
            prop: "ERD columns",
        })
}

/// Stores `prop` on `element` and returns the previous value.
fn set_prop(element: &mut Element, prop: Prop) -> Result<Prop, CommandError> {
    let id = element.id;
    let wrong = |prop: Prop| CommandError::WrongKind {
        id,
        prop: prop.name(),
    };
    macro_rules! style_field {
        ($variant:ident, $field:ident, $value:expr) => {{
            let value = $value;
            match style_mut(element) {
                Some(style) => Prop::$variant(replace(&mut style.$field, value)),
                None => return Err(wrong(Prop::$variant(value))),
            }
        }};
    }
    Ok(match prop {
        Prop::Locked(v) => Prop::Locked(replace(&mut element.locked, v)),
        Prop::Order(v) => Prop::Order(replace(&mut element.order, v)),
        Prop::Parent(v) => Prop::Parent(replace(&mut element.parent, v)),
        Prop::Bounds(v) => match element.as_shape_mut() {
            Some(s) => Prop::Bounds(replace(&mut s.bounds, v.abs())),
            None => return Err(wrong(Prop::Bounds(v))),
        },
        Prop::Shape(v) => match element.as_shape_mut() {
            Some(s) => Prop::Shape(replace(&mut s.shape, v)),
            None => return Err(wrong(Prop::Shape(v))),
        },
        Prop::Text(v) => match &mut element.kind {
            ElementKind::Shape(s) => Prop::Text(replace(&mut s.text, v)),
            ElementKind::Connector(c) => Prop::Text(replace(&mut c.text, v)),
            ElementKind::Group => return Err(wrong(Prop::Text(v))),
        },
        Prop::TableDisplay(v) => {
            match element.as_shape_mut().and_then(|shape| shape.erd.as_mut()) {
                Some(table) => Prop::TableDisplay(replace(&mut table.display, v)),
                None => return Err(wrong(Prop::TableDisplay(v))),
            }
        }
        Prop::SqlDialect(v) => match element.as_shape_mut().and_then(|shape| shape.erd.as_mut()) {
            Some(table) => Prop::SqlDialect(replace(&mut table.dialect, v)),
            None => return Err(wrong(Prop::SqlDialect(v))),
        },
        Prop::Fill(v) => style_field!(Fill, fill, v),
        Prop::Stroke(v) => style_field!(Stroke, stroke, v),
        Prop::StrokeWidth(v) => style_field!(StrokeWidth, stroke_width, v),
        Prop::Dash(v) => style_field!(Dash, dash, v),
        Prop::Opacity(v) => style_field!(Opacity, opacity, v),
        Prop::Shadow(v) => style_field!(Shadow, shadow, v),
        Prop::CornerRadius(v) => style_field!(CornerRadius, corner_radius, v),
        Prop::TextColor(v) => style_field!(TextColor, text_color, v),
        Prop::FontSize(v) => style_field!(FontSize, font_size, v),
        Prop::Bold(v) => style_field!(Bold, bold, v),
        Prop::Italic(v) => style_field!(Italic, italic, v),
        Prop::TextAlign(v) => style_field!(TextAlign, text_align, v),
        Prop::VerticalAlign(v) => style_field!(VerticalAlign, vertical_align, v),
        prop => {
            let Some(c) = element.as_connector_mut() else {
                return Err(wrong(prop));
            };
            match prop {
                Prop::Source(v) => Prop::Source(replace(&mut c.source, v)),
                Prop::Target(v) => Prop::Target(replace(&mut c.target, v)),
                Prop::Waypoints(v) => Prop::Waypoints(replace(&mut c.waypoints, v)),
                Prop::Routing(v) => Prop::Routing(replace(&mut c.routing, v)),
                Prop::StartMarker(v) => Prop::StartMarker(replace(&mut c.start_marker, v)),
                Prop::EndMarker(v) => Prop::EndMarker(replace(&mut c.end_marker, v)),
                Prop::LabelPosition(v) => {
                    Prop::LabelPosition(replace(&mut c.label_position, v.clamp(0.0, 1.0)))
                }
                other => unreachable!("{} is handled above", other.name()),
            }
        }
    })
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

    /// Applies `command`. A repeated set of the same plain property keeps
    /// only the first inverse and the last forward value, so a 200-frame
    /// drag is one small undo step.
    fn push(&mut self, doc: &mut Document, command: Command) -> Result<(), CommandError> {
        let inverse = command.clone().apply(doc)?;
        self.touched = Instant::now();
        if let Some(slot) = self.coalescable_slot(&command) {
            self.forward[slot] = command;
            return Ok(());
        }
        self.forward.push(command);
        self.inverse.push(inverse);
        Ok(())
    }

    /// The index of an earlier set this one can replace. The search stops
    /// at any command that creates or destroys the same target, because
    /// replacing across it would reorder their effects.
    fn coalescable_slot(&self, command: &Command) -> Option<usize> {
        let key = command.coalesce_key()?;
        for (i, earlier) in self.forward.iter().enumerate().rev() {
            if earlier.coalesce_key() == Some(key) {
                return Some(i);
            }
            if earlier.creates_or_destroys(key.target()) {
                return None;
            }
        }
        None
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
    base_serial: u64,
    limit: usize,
    revision: u64,
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
            base_serial: 0,
            limit: 500,
            revision: 0,
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
    /// If any command fails, everything this call applied is rolled back.
    pub fn apply(
        &mut self,
        doc: &mut Document,
        label: impl Into<String>,
        commands: impl IntoIterator<Item = Command>,
    ) -> Result<(), CommandError> {
        if self.open.is_some() {
            // Apply into a scratch step first, so a failure leaves the open
            // step exactly as it was.
            let mut scratch = Transaction::new(String::new(), 0);
            for command in commands {
                if let Err(e) = scratch.push(doc, command) {
                    scratch.rollback(doc);
                    return Err(e);
                }
            }
            if scratch.forward.is_empty() {
                return Ok(());
            }
            let serial = self.serial();
            let open = self.open.as_mut().expect("checked above");
            for command in scratch.forward.into_iter().zip(scratch.inverse) {
                let (forward, inverse) = command;
                if let Some(slot) = open.coalescable_slot(&forward) {
                    open.forward[slot] = forward;
                } else {
                    open.forward.push(forward);
                    open.inverse.push(inverse);
                }
            }
            open.serial = serial;
            open.touched = Instant::now();
            self.revision += 1;
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
        self.revision += 1;
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
            let top = self.undo.pop().expect("checked above");
            self.open = Some(top);
            let result = self.apply(doc, "", commands);
            let top = self.open.take().expect("still open");
            self.undo.push(top);
            return result;
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
            self.revision += 1;
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
            self.base_serial = self.undo.remove(0).serial;
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
        self.revision += 1;
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
        self.revision += 1;
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
        self.open
            .as_ref()
            .filter(|t| !t.forward.is_empty())
            .or_else(|| self.undo.last())
            .map_or(self.base_serial, |t| t.serial)
    }

    /// Counts every change to the document made through this history,
    /// including undo, redo and changes inside an open step. Caches keyed
    /// on it (the scene) know exactly when to rebuild.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Forgets all steps (for a newly opened document). The revision keeps
    /// counting, so caches still see the change.
    pub fn clear(&mut self) {
        let revision = self.revision + 1;
        *self = Self::default();
        self.revision = revision;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (Document, Parent, Element) {
        let doc = Document::new();
        let page = doc.first_page().unwrap();
        let layer = Parent::Layer(doc.layers_of(page)[0].id);
        let el = Element::shape(
            ShapeRef::new("basic", "rectangle"),
            layer,
            doc.next_order_key(layer),
            Rect::new(0.0, 0.0, 10.0, 10.0),
        );
        (doc, layer, el)
    }

    fn bounds_x(doc: &Document, id: ElementId) -> f64 {
        doc.elements[&id].as_shape().unwrap().bounds.x0
    }

    #[test]
    fn insert_undo_redo() {
        let (mut doc, _, el) = setup();
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
        let (mut doc, _, el) = setup();
        let id = el.id;
        let mut h = History::new();
        h.apply(&mut doc, "Add", [Command::Insert(Box::new(el))])
            .unwrap();
        h.begin("Move");
        for i in 1..=100 {
            let x = f64::from(i);
            let bounds = Rect::new(x, 0.0, x + 10.0, 10.0);
            h.apply(
                &mut doc,
                "",
                [Command::Set {
                    id,
                    prop: Prop::Bounds(bounds),
                }],
            )
            .unwrap();
        }
        h.commit();
        assert_eq!(bounds_x(&doc, id), 100.0);
        assert_eq!(h.undo_label(), Some("Move"));
        assert_eq!(h.undo.last().unwrap().forward.len(), 1, "coalesced");
        h.undo(&mut doc);
        assert_eq!(bounds_x(&doc, id), 0.0);
        h.redo(&mut doc);
        assert_eq!(bounds_x(&doc, id), 100.0);
    }

    #[test]
    fn failed_step_rolls_back() {
        let (mut doc, _, el) = setup();
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
    fn failure_inside_an_open_step_keeps_earlier_changes() {
        let (mut doc, _, el) = setup();
        let id = el.id;
        let mut h = History::new();
        h.begin("Gesture");
        h.apply(&mut doc, "", [Command::Insert(Box::new(el))])
            .unwrap();
        let bad = h.apply(
            &mut doc,
            "",
            [
                Command::Set {
                    id,
                    prop: Prop::Text("x".into()),
                },
                Command::Remove(ElementId::new()),
            ],
        );
        assert!(bad.is_err());
        assert_eq!(doc.elements[&id].text(), Some(""), "partial call undone");
        h.commit();
        h.undo(&mut doc);
        assert!(doc.elements.is_empty(), "earlier change still undoable");
    }

    #[test]
    fn merging_edits_become_one_step() {
        let (mut doc, _, el) = setup();
        let id = el.id;
        let mut h = History::new();
        h.apply(&mut doc, "Add", [Command::Insert(Box::new(el))])
            .unwrap();
        for w in [2.0, 3.0, 4.0] {
            h.apply_merging(
                &mut doc,
                "Stroke",
                format!("stroke:{id}"),
                [Command::Set {
                    id,
                    prop: Prop::StrokeWidth(Some(w)),
                }],
            )
            .unwrap();
        }
        let width = |doc: &Document| doc.elements[&id].style().unwrap().stroke_width;
        assert_eq!(width(&doc), Some(4.0));
        h.undo(&mut doc);
        assert_eq!(width(&doc), None);
        assert_eq!(h.undo_label(), Some("Add"));
    }

    #[test]
    fn revision_counts_every_change() {
        let (mut doc, _, el) = setup();
        let id = el.id;
        let mut h = History::new();
        let r0 = h.revision();
        h.apply(&mut doc, "Add", [Command::Insert(Box::new(el))])
            .unwrap();
        h.begin("Drag");
        h.apply(
            &mut doc,
            "",
            [Command::Set {
                id,
                prop: Prop::Text("a".into()),
            }],
        )
        .unwrap();
        let mid = h.revision();
        h.apply(
            &mut doc,
            "",
            [Command::Set {
                id,
                prop: Prop::Text("b".into()),
            }],
        )
        .unwrap();
        assert!(h.revision() > mid, "changes inside an open step count");
        h.cancel(&mut doc);
        let after_cancel = h.revision();
        h.undo(&mut doc);
        assert!(h.revision() > after_cancel && after_cancel > r0);
        let before_clear = h.revision();
        h.clear();
        assert!(h.revision() > before_clear);
        assert!(
            h.apply(&mut doc, "", [Command::Remove(ElementId::new())])
                .is_err()
        );
        assert_eq!(
            h.revision(),
            before_clear + 1,
            "failed steps change nothing"
        );
    }

    #[test]
    fn state_id_tracks_saved_state() {
        let (mut doc, _, el) = setup();
        let mut h = History::new();
        let saved = h.state_id();
        h.apply(&mut doc, "Add", [Command::Insert(Box::new(el))])
            .unwrap();
        assert_ne!(h.state_id(), saved);
        h.undo(&mut doc);
        assert_eq!(h.state_id(), saved);
    }

    #[test]
    fn state_id_tracks_each_change_in_an_open_step() {
        let (mut doc, _, el) = setup();
        let id = el.id;
        doc.elements.insert(id, el);
        let mut h = History::new();
        let saved = h.state_id();
        h.begin("Drag");
        assert_eq!(h.state_id(), saved, "an empty gesture changes nothing");
        for text in ["first", "second"] {
            let before = h.state_id();
            h.apply(
                &mut doc,
                "",
                [Command::Set {
                    id,
                    prop: Prop::Text(text.into()),
                }],
            )
            .unwrap();
            assert_ne!(h.state_id(), before, "saving mid-gesture must be safe");
        }
        let open = h.state_id();
        h.apply(&mut doc, "", []).unwrap();
        assert_eq!(h.state_id(), open, "an empty apply changes nothing");
        assert!(
            h.apply(&mut doc, "", [Command::Remove(ElementId::new())])
                .is_err()
        );
        assert_eq!(h.state_id(), open, "a failed apply changes nothing");
        h.commit();
        assert_eq!(h.state_id(), open, "commit keeps the document state");
        h.undo(&mut doc);
        assert_eq!(h.state_id(), saved);
        h.redo(&mut doc);
        assert_eq!(h.state_id(), open);
        h.begin("Cancel");
        h.apply(
            &mut doc,
            "",
            [Command::Set {
                id,
                prop: Prop::Text("third".into()),
            }],
        )
        .unwrap();
        assert_ne!(h.state_id(), open);
        h.cancel(&mut doc);
        assert_eq!(h.state_id(), open, "cancel returns to the prior state");
    }

    #[test]
    fn state_id_keeps_the_baseline_when_old_steps_are_discarded() {
        let (mut doc, _, el) = setup();
        let id = el.id;
        doc.elements.insert(id, el);
        let mut h = History::new();
        h.limit = 2;
        let initial = h.state_id();
        let mut first = 0;
        for text in ["one", "two", "three"] {
            h.apply(
                &mut doc,
                "Edit",
                [Command::Set {
                    id,
                    prop: Prop::Text(text.into()),
                }],
            )
            .unwrap();
            if text == "one" {
                first = h.state_id();
            }
        }
        assert!(h.undo(&mut doc));
        assert!(h.undo(&mut doc));
        assert!(!h.undo(&mut doc));
        assert_eq!(doc.elements[&id].text(), Some("one"));
        assert_eq!(h.state_id(), first);
        assert_ne!(
            h.state_id(),
            initial,
            "discarded edits remain in the document"
        );
    }

    #[test]
    fn style_props_reject_groups_and_connector_props_reject_shapes() {
        let (mut doc, layer, el) = setup();
        let shape = el.id;
        let group = Element::group(layer, doc.next_order_key(layer));
        let g = group.id;
        let mut h = History::new();
        h.apply(
            &mut doc,
            "Add",
            [
                Command::Insert(Box::new(el)),
                Command::Insert(Box::new(group)),
            ],
        )
        .unwrap();
        let fill = Prop::Fill(Some(Paint::None));
        assert_eq!(
            h.apply(
                &mut doc,
                "",
                [Command::Set {
                    id: g,
                    prop: fill.clone()
                }]
            ),
            Err(CommandError::WrongKind {
                id: g,
                prop: "fill"
            })
        );
        assert!(
            h.apply(
                &mut doc,
                "",
                [Command::Set {
                    id: shape,
                    prop: fill
                }]
            )
            .is_ok()
        );
        assert!(matches!(
            h.apply(
                &mut doc,
                "",
                [Command::Set {
                    id: shape,
                    prop: Prop::Routing(Routing::Curved)
                }]
            ),
            Err(CommandError::WrongKind { .. })
        ));
    }

    #[test]
    fn references_are_enforced() {
        let (mut doc, layer, el) = setup();
        let shape = el.id;
        let mut h = History::new();
        let connector = Element::connector(
            Endpoint::glued(shape, Some("e")),
            Endpoint::Free(Point::new(50.0, 50.0)),
            layer,
            OrderKey::first(),
        );
        let c = connector.id;
        // The connector can't be added before its shape.
        assert!(
            h.apply(&mut doc, "", [Command::Insert(Box::new(connector.clone()))])
                .is_err()
        );
        h.apply(
            &mut doc,
            "Add",
            [
                Command::Insert(Box::new(el)),
                Command::Insert(Box::new(connector)),
            ],
        )
        .unwrap();
        // The shape can't be removed while the connector holds it...
        assert_eq!(
            h.apply(&mut doc, "", [Command::Remove(shape)]),
            Err(CommandError::InUse(shape))
        );
        // ...but it can after the connector goes, and undo restores both.
        h.apply(
            &mut doc,
            "Delete",
            [Command::Remove(c), Command::Remove(shape)],
        )
        .unwrap();
        assert!(doc.elements.is_empty());
        h.undo(&mut doc);
        assert_eq!(doc.elements.len(), 2);
        assert_eq!(doc.validate(), Ok(()));
        // Connectors can only glue to shapes.
        assert!(matches!(
            h.apply(
                &mut doc,
                "",
                [Command::Set {
                    id: c,
                    prop: Prop::Target(Endpoint::glued(c, None))
                }]
            ),
            Err(CommandError::Invalid(ModelError::BadEndpoint { .. }))
        ));
    }

    #[test]
    fn reparenting_rejects_cycles() {
        let (mut doc, layer, _) = setup();
        let a = Element::group(layer, OrderKey::first());
        let b = Element::group(Parent::Element(a.id), OrderKey::first());
        let (a_id, b_id) = (a.id, b.id);
        let mut h = History::new();
        h.apply(
            &mut doc,
            "",
            [Command::Insert(Box::new(a)), Command::Insert(Box::new(b))],
        )
        .unwrap();
        for target in [a_id, b_id] {
            assert_eq!(
                h.apply(
                    &mut doc,
                    "",
                    [Command::Set {
                        id: a_id,
                        prop: Prop::Parent(Parent::Element(target))
                    }]
                ),
                Err(CommandError::Cycle(a_id))
            );
        }
        assert_eq!(
            h.apply(&mut doc, "", [Command::Remove(a_id)]),
            Err(CommandError::NotEmpty(format!("element {a_id}")))
        );
    }

    #[test]
    fn pages_and_layers() {
        let (mut doc, _, _) = setup();
        let first = doc.first_page().unwrap();
        let mut h = History::new();
        assert_eq!(
            h.apply(&mut doc, "", [Command::RemovePage(first)]),
            Err(CommandError::LastPage)
        );
        let page = Page::new("Page 2", OrderKey::after(&doc.pages[&first].order));
        let layer = Layer::new(page.id, "Layer 1", OrderKey::first());
        let (p, l) = (page.id, layer.id);
        h.apply(
            &mut doc,
            "Add page",
            [
                Command::InsertPage(Box::new(page)),
                Command::InsertLayer(Box::new(layer)),
            ],
        )
        .unwrap();
        h.apply(
            &mut doc,
            "Hide",
            [Command::SetLayer {
                id: l,
                prop: LayerProp::Visible(false),
            }],
        )
        .unwrap();
        h.apply(
            &mut doc,
            "Rename",
            [Command::SetPage {
                id: p,
                prop: PageProp::Name("Flows".into()),
            }],
        )
        .unwrap();
        assert_eq!(doc.pages[&p].name, "Flows");
        assert!(!doc.layers[&l].visible);
        assert_eq!(
            h.apply(&mut doc, "", [Command::RemovePage(p)]),
            Err(CommandError::NotEmpty(format!("page {p}")))
        );
        h.apply(
            &mut doc,
            "Delete page",
            [Command::RemoveLayer(l), Command::RemovePage(p)],
        )
        .unwrap();
        assert_eq!(doc.pages.len(), 1);
        h.undo(&mut doc);
        assert_eq!(doc.pages[&p].name, "Flows");
        assert_eq!(doc.validate(), Ok(()));
    }
}
