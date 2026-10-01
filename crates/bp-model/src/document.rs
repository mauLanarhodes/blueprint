use crate::{Color, ElementId, LayerId, OrderKey, PageId, Style};
use kurbo::Rect;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Bumped whenever the file format changes; `bp-io` migrates older files.
pub const SCHEMA_VERSION: u32 = 1;

/// The shapes available in Phase 0. ERD, cloud and flowchart shapes come
/// from shape libraries in later phases.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShapeKind {
    Rectangle,
    RoundedRectangle,
    Ellipse,
    Diamond,
    Text,
}

impl ShapeKind {
    pub const ALL: [ShapeKind; 5] = [
        ShapeKind::Rectangle,
        ShapeKind::RoundedRectangle,
        ShapeKind::Ellipse,
        ShapeKind::Diamond,
        ShapeKind::Text,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ShapeKind::Rectangle => "Rectangle",
            ShapeKind::RoundedRectangle => "Rounded rectangle",
            ShapeKind::Ellipse => "Ellipse",
            ShapeKind::Diamond => "Diamond",
            ShapeKind::Text => "Text",
        }
    }

    pub fn default_style(self) -> Style {
        match self {
            ShapeKind::Text => Style {
                fill: None,
                stroke: None,
                ..Style::default()
            },
            _ => Style::default(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Page {
    pub id: PageId,
    pub name: String,
    pub order: OrderKey,
    #[serde(default = "default_background")]
    pub background: Color,
}

fn default_background() -> Color {
    Color::WHITE
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Layer {
    pub id: LayerId,
    pub page: PageId,
    pub name: String,
    pub order: OrderKey,
    #[serde(default = "yes")]
    pub visible: bool,
    #[serde(default)]
    pub locked: bool,
}

fn yes() -> bool {
    true
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Element {
    pub id: ElementId,
    pub layer: LayerId,
    pub order: OrderKey,
    pub kind: ShapeKind,
    /// Axis-aligned bounds in page units (1 unit = 1 CSS pixel at 100% zoom).
    pub bounds: Rect,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub text: String,
    #[serde(default)]
    pub style: Style,
}

impl Element {
    /// A new element with the kind's default style. `bounds` is normalised.
    pub fn new(kind: ShapeKind, layer: LayerId, order: OrderKey, bounds: Rect) -> Self {
        Self {
            id: ElementId::new(),
            layer,
            order,
            kind,
            bounds: bounds.abs(),
            text: String::new(),
            style: kind.default_style(),
        }
    }
}

/// A whole Blueprint document: flat maps of pages, layers and elements that
/// point at their parents by id.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Document {
    pub schema_version: u32,
    pub pages: BTreeMap<PageId, Page>,
    pub layers: BTreeMap<LayerId, Layer>,
    pub elements: BTreeMap<ElementId, Element>,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ModelError {
    #[error("the document has no pages")]
    NoPages,
    #[error("layer {layer} points at missing page {page}")]
    MissingPage { layer: LayerId, page: PageId },
    #[error("element {element} points at missing layer {layer}")]
    MissingLayer { element: ElementId, layer: LayerId },
    #[error("map key does not match the stored id {0}")]
    IdMismatch(String),
    #[error("invalid order key {0:?}")]
    InvalidOrderKey(String),
}

impl Default for Document {
    fn default() -> Self {
        Self::new()
    }
}

impl Document {
    /// A new document with one page ("Page 1") holding one layer ("Layer 1").
    pub fn new() -> Self {
        let page = Page {
            id: PageId::new(),
            name: "Page 1".into(),
            order: OrderKey::first(),
            background: Color::WHITE,
        };
        let layer = Layer {
            id: LayerId::new(),
            page: page.id,
            name: "Layer 1".into(),
            order: OrderKey::first(),
            visible: true,
            locked: false,
        };
        Self {
            schema_version: SCHEMA_VERSION,
            pages: BTreeMap::from([(page.id, page)]),
            layers: BTreeMap::from([(layer.id, layer)]),
            elements: BTreeMap::new(),
        }
    }

    /// Pages in display order (ties broken by id so every client agrees).
    pub fn pages_sorted(&self) -> Vec<&Page> {
        let mut pages: Vec<_> = self.pages.values().collect();
        pages.sort_by(|a, b| (&a.order, a.id).cmp(&(&b.order, b.id)));
        pages
    }

    pub fn first_page(&self) -> Option<PageId> {
        self.pages_sorted().first().map(|p| p.id)
    }

    /// Layers of `page`, bottom to top.
    pub fn layers_of(&self, page: PageId) -> Vec<&Layer> {
        let mut layers: Vec<_> = self.layers.values().filter(|l| l.page == page).collect();
        layers.sort_by(|a, b| (&a.order, a.id).cmp(&(&b.order, b.id)));
        layers
    }

    /// Elements on visible layers of `page`, bottom to top.
    pub fn elements_on_page(&self, page: PageId) -> Vec<&Element> {
        let layers = self.layers_of(page);
        let rank = |id: LayerId| layers.iter().position(|l| l.id == id);
        let mut elements: Vec<_> = self
            .elements
            .values()
            .filter(|e| {
                rank(e.layer).is_some() && self.layers.get(&e.layer).is_some_and(|l| l.visible)
            })
            .collect();
        elements
            .sort_by(|a, b| (rank(a.layer), &a.order, a.id).cmp(&(rank(b.layer), &b.order, b.id)));
        elements
    }

    /// An order key that puts a new element on top of everything in `layer`.
    pub fn next_order_key(&self, layer: LayerId) -> OrderKey {
        self.elements
            .values()
            .filter(|e| e.layer == layer)
            .map(|e| &e.order)
            .max()
            .map_or_else(OrderKey::first, OrderKey::after)
    }

    pub fn page_of(&self, element: ElementId) -> Option<PageId> {
        let layer = self.elements.get(&element)?.layer;
        Some(self.layers.get(&layer)?.page)
    }

    /// Checks references and keys after loading a file.
    pub fn validate(&self) -> Result<(), ModelError> {
        if self.pages.is_empty() {
            return Err(ModelError::NoPages);
        }
        let bad_key = |k: &OrderKey| ModelError::InvalidOrderKey(k.as_str().to_owned());
        for (id, page) in &self.pages {
            if *id != page.id {
                return Err(ModelError::IdMismatch(page.id.to_string()));
            }
            if !page.order.is_valid() {
                return Err(bad_key(&page.order));
            }
        }
        for (id, layer) in &self.layers {
            if *id != layer.id {
                return Err(ModelError::IdMismatch(layer.id.to_string()));
            }
            if !self.pages.contains_key(&layer.page) {
                return Err(ModelError::MissingPage {
                    layer: layer.id,
                    page: layer.page,
                });
            }
            if !layer.order.is_valid() {
                return Err(bad_key(&layer.order));
            }
        }
        for (id, element) in &self.elements {
            if *id != element.id {
                return Err(ModelError::IdMismatch(element.id.to_string()));
            }
            if !self.layers.contains_key(&element.layer) {
                return Err(ModelError::MissingLayer {
                    element: element.id,
                    layer: element.layer,
                });
            }
            if !element.order.is_valid() {
                return Err(bad_key(&element.order));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc_with_three() -> (Document, LayerId, Vec<ElementId>) {
        let mut doc = Document::new();
        let page = doc.first_page().unwrap();
        let layer = doc.layers_of(page)[0].id;
        let mut ids = vec![];
        for i in 0..3 {
            let order = doc.next_order_key(layer);
            let r = Rect::new(i as f64 * 10.0, 0.0, i as f64 * 10.0 + 5.0, 5.0);
            let el = Element::new(ShapeKind::Rectangle, layer, order, r);
            ids.push(el.id);
            doc.elements.insert(el.id, el);
        }
        (doc, layer, ids)
    }

    #[test]
    fn new_document_is_valid() {
        let doc = Document::new();
        assert_eq!(doc.pages.len(), 1);
        assert_eq!(doc.layers.len(), 1);
        assert_eq!(doc.validate(), Ok(()));
    }

    #[test]
    fn new_elements_stack_on_top() {
        let (doc, _, ids) = doc_with_three();
        let page = doc.first_page().unwrap();
        let order: Vec<_> = doc.elements_on_page(page).iter().map(|e| e.id).collect();
        assert_eq!(order, ids);
    }

    #[test]
    fn hidden_layers_are_skipped() {
        let (mut doc, layer, _) = doc_with_three();
        doc.layers.get_mut(&layer).unwrap().visible = false;
        assert!(doc.elements_on_page(doc.first_page().unwrap()).is_empty());
    }

    #[test]
    fn json_round_trip() {
        let (doc, _, _) = doc_with_three();
        let json = serde_json::to_string(&doc).unwrap();
        let back: Document = serde_json::from_str(&json).unwrap();
        assert_eq!(doc, back);
    }

    #[test]
    fn validate_catches_dangling_layer() {
        let (mut doc, _, ids) = doc_with_three();
        doc.elements.get_mut(&ids[0]).unwrap().layer = LayerId::new();
        assert!(matches!(
            doc.validate(),
            Err(ModelError::MissingLayer { .. })
        ));
    }
}