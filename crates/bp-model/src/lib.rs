//! The Blueprint document model.
//!
//! The model is deliberately flat and CRDT-friendly (see the project plan):
//! - every page, layer and element has a permanent UUIDv7 id;
//! - nothing refers to anything by array position;
//! - the element tree is stored as parent pointers, and stacking order uses
//!   fractional [`OrderKey`]s instead of indices;
//! - only values the user set are stored (styles are sparse overrides);
//!   connector routes, text layout and group bounds are recomputed.

mod document;
mod element;
mod erd;
mod ids;
mod order;
mod style;

pub use document::{DiagramKind, Document, Layer, ModelError, Page, SCHEMA_VERSION, Tree};
pub use element::{
    Connector, Element, ElementKind, Endpoint, Marker, Parent, PortId, Routing, Shape, ShapeRef,
};
pub use erd::{ErdColumn, ErdTable, SqlDialect, TableDisplay};
pub use ids::{ColumnId, ElementId, IdHasher, IdMap, LayerId, PageId};
pub use kurbo;
pub use order::OrderKey;
pub use style::{Color, Dash, Paint, Style, StyleValues, TextAlign, VerticalAlign};
