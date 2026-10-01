//! The Blueprint document model.
//!
//! The model is deliberately flat and CRDT-friendly (see the project plan):
//! - every page, layer and element has a permanent UUIDv7 id;
//! - nothing refers to anything by array position;
//! - stacking order uses fractional [`OrderKey`]s instead of indices;
//! - only values the user set are stored; everything derived is recomputed.

mod document;
mod ids;
mod order;
mod style;

pub use document::{Document, Element, Layer, ModelError, Page, SCHEMA_VERSION, ShapeKind};
pub use ids::{ElementId, LayerId, PageId};
pub use kurbo;
pub use order::OrderKey;
pub use style::{Color, Style};