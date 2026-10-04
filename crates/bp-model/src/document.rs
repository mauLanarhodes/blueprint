use crate::{
    CloudIcon, Color, ColumnId, Element, ElementId, ElementKind, Endpoint, LayerId, OrderKey,
    PageId, Parent, ShapeRef,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};

/// Bumped for incompatible format changes; `bp-io` migrates older files.
pub const SCHEMA_VERSION: u32 = 4;

/// The tools and shape libraries a page presents in the editor.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagramKind {
    Flowchart,
    Erd,
    Cloud,
}

impl DiagramKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Flowchart => "Flowchart",
            Self::Erd => "ERD",
            Self::Cloud => "Cloud architecture",
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
    /// `None` keeps legacy pages unclassified until their kind is chosen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagram_kind: Option<DiagramKind>,
}

impl Page {
    pub fn new(name: impl Into<String>, order: OrderKey) -> Self {
        Self {
            id: PageId::new(),
            name: name.into(),
            order,
            background: Color::WHITE,
            diagram_kind: None,
        }
    }
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

impl Layer {
    pub fn new(page: PageId, name: impl Into<String>, order: OrderKey) -> Self {
        Self {
            id: LayerId::new(),
            page,
            name: name.into(),
            order,
            visible: true,
            locked: false,
        }
    }
}

fn yes() -> bool {
    true
}

/// A whole Blueprint document: flat maps of pages, layers and elements that
/// point at their parents by id.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Document {
    pub schema_version: u32,
    pub pages: BTreeMap<PageId, Page>,
    pub layers: BTreeMap<LayerId, Layer>,
    pub elements: BTreeMap<ElementId, Element>,
    /// Only icons used by this document are embedded; installed packs stay
    /// outside the document. JSON stores SVG inline and ZIP stores it separately.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub icons: BTreeMap<ShapeRef, CloudIcon>,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ModelError {
    #[error("the document has no pages")]
    NoPages,
    #[error("layer {layer} points at missing page {page}")]
    MissingPage { layer: LayerId, page: PageId },
    #[error("element {element} points at missing parent {parent:?}")]
    MissingParent { element: ElementId, parent: Parent },
    #[error("element {element} cannot contain other elements")]
    InvalidParent { element: ElementId },
    #[error("element {0} is its own ancestor")]
    Cycle(ElementId),
    #[error("connector {connector} is attached to {target}, which is not a shape")]
    BadEndpoint {
        connector: ElementId,
        target: ElementId,
    },
    #[error("element {0} has coordinates that are not finite numbers")]
    NotFinite(ElementId),
    #[error("map key does not match the stored id {0}")]
    IdMismatch(String),
    #[error("invalid order key {0:?}")]
    InvalidOrderKey(String),
    #[error("table {element} contains duplicate column {column}")]
    DuplicateColumn {
        element: ElementId,
        column: ColumnId,
    },
    #[error("table {0} has no structured ERD data")]
    MissingErdData(ElementId),
    #[error("shape {0} has ERD data but is not an ERD table")]
    UnexpectedErdData(ElementId),
    #[error("connector {connector} refers to missing column {column} in table {target}")]
    MissingColumnEndpoint {
        connector: ElementId,
        target: ElementId,
        column: ColumnId,
    },
    #[error("connector {connector} has invalid column port {port:?} on {target}")]
    InvalidColumnPort {
        connector: ElementId,
        target: ElementId,
        port: String,
    },
    #[error("cloud shape {element} needs missing icon {reference}")]
    MissingIcon {
        element: ElementId,
        reference: ShapeRef,
    },
    #[error("icon {reference} is invalid: {message}")]
    InvalidIcon {
        reference: ShapeRef,
        message: String,
    },
}

impl Default for Document {
    fn default() -> Self {
        Self::new()
    }
}

/// Children of every parent, bottom to top. Building it is O(n log n), so
/// build it once and run many queries against it.
pub struct Tree<'a> {
    children: HashMap<Parent, Vec<&'a Element>>,
}

impl<'a> Tree<'a> {
    pub fn children(&self, parent: Parent) -> &[&'a Element] {
        self.children.get(&parent).map_or(&[], Vec::as_slice)
    }

    /// `parent`'s descendants in paint order (each parent before its
    /// children).
    pub fn subtree(&self, parent: Parent, out: &mut Vec<&'a Element>) {
        for &child in self.children(parent) {
            out.push(child);
            self.subtree(Parent::Element(child.id), out);
        }
    }
}

impl Document {
    /// A new document with one page ("Page 1") holding one layer ("Layer 1").
    pub fn new() -> Self {
        let page = Page::new("Page 1", OrderKey::first());
        let layer = Layer::new(page.id, "Layer 1", OrderKey::first());
        Self {
            schema_version: SCHEMA_VERSION,
            pages: BTreeMap::from([(page.id, page)]),
            layers: BTreeMap::from([(layer.id, layer)]),
            elements: BTreeMap::new(),
            icons: BTreeMap::new(),
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

    pub fn tree(&self) -> Tree<'_> {
        let mut children: HashMap<Parent, Vec<&Element>> = HashMap::new();
        for element in self.elements.values() {
            children.entry(element.parent).or_default().push(element);
        }
        for list in children.values_mut() {
            list.sort_by(|a, b| (&a.order, a.id).cmp(&(&b.order, b.id)));
        }
        Tree { children }
    }

    /// Children of `parent`, bottom to top.
    pub fn children(&self, parent: Parent) -> Vec<&Element> {
        let mut list: Vec<_> = self
            .elements
            .values()
            .filter(|e| e.parent == parent)
            .collect();
        list.sort_by(|a, b| (&a.order, a.id).cmp(&(&b.order, b.id)));
        list
    }

    /// Every element on visible layers of `page`, in paint order: layers
    /// bottom to top, and each group or container before its children.
    pub fn paint_order(&self, page: PageId) -> Vec<&Element> {
        let tree = self.tree();
        let mut out = Vec::new();
        for layer in self.layers_of(page) {
            if layer.visible {
                tree.subtree(Parent::Layer(layer.id), &mut out);
            }
        }
        out
    }

    /// Every element on `page`, including hidden layers, in paint order.
    pub fn page_elements(&self, page: PageId) -> Vec<&Element> {
        let tree = self.tree();
        let mut out = Vec::new();
        for layer in self.layers_of(page) {
            tree.subtree(Parent::Layer(layer.id), &mut out);
        }
        out
    }

    /// Ids of `id`'s ancestors, nearest first. Stops early on a broken chain.
    pub fn ancestors(&self, id: ElementId) -> Vec<ElementId> {
        let mut out = Vec::new();
        let mut current = self.elements.get(&id).map(|e| e.parent);
        while let Some(Parent::Element(parent)) = current {
            if out.contains(&parent) || parent == id {
                break; // a cycle; validation reports it
            }
            out.push(parent);
            current = self.elements.get(&parent).map(|e| e.parent);
        }
        out
    }

    /// Every descendant of `id`, parents before children.
    pub fn descendants(&self, id: ElementId) -> Vec<ElementId> {
        let tree = self.tree();
        let mut out = Vec::new();
        tree.subtree(Parent::Element(id), &mut out);
        out.into_iter().map(|e| e.id).collect()
    }

    /// The layer `id` belongs to, through its ancestors.
    pub fn layer_of(&self, id: ElementId) -> Option<LayerId> {
        let top = self.ancestors(id).last().copied().unwrap_or(id);
        match self.elements.get(&top)?.parent {
            Parent::Layer(layer) => Some(layer),
            Parent::Element(_) => None,
        }
    }

    pub fn page_of(&self, id: ElementId) -> Option<PageId> {
        Some(self.layers.get(&self.layer_of(id)?)?.page)
    }

    /// Connectors with an end glued to `id`.
    pub fn connectors_attached_to(&self, id: ElementId) -> Vec<ElementId> {
        self.elements
            .values()
            .filter(|e| {
                e.as_connector()
                    .is_some_and(|c| c.endpoints().iter().any(|end| end.element() == Some(id)))
            })
            .map(|e| e.id)
            .collect()
    }

    /// Relationships attached to either side of a particular column row.
    pub fn connectors_attached_to_column(&self, id: ElementId, column: ColumnId) -> Vec<ElementId> {
        self.elements
            .values()
            .filter(|element| {
                element.as_connector().is_some_and(|connector| {
                    connector.endpoints().iter().any(|endpoint| {
                        matches!(endpoint, Endpoint::Glued { element, port: Some(port) }
                        if *element == id && port.column_id() == Some(column))
                    })
                })
            })
            .map(|element| element.id)
            .collect()
    }

    /// An order key that puts a new child on top of `parent`'s children.
    pub fn next_order_key(&self, parent: Parent) -> OrderKey {
        self.elements
            .values()
            .filter(|e| e.parent == parent)
            .map(|e| &e.order)
            .max()
            .map_or_else(OrderKey::first, OrderKey::after)
    }

    /// An order key that puts a new child below all of `parent`'s children.
    pub fn first_order_key(&self, parent: Parent) -> OrderKey {
        self.elements
            .values()
            .filter(|e| e.parent == parent)
            .map(|e| &e.order)
            .min()
            .map_or_else(OrderKey::first, OrderKey::before)
    }

    /// Whether `id` can be edited: neither it, an ancestor nor its layer is
    /// locked.
    pub fn is_locked(&self, id: ElementId) -> bool {
        let chain = std::iter::once(id).chain(self.ancestors(id));
        let element_locked = chain
            .into_iter()
            .any(|e| self.elements.get(&e).is_some_and(|e| e.locked));
        let layer_locked = self
            .layer_of(id)
            .and_then(|l| self.layers.get(&l))
            .is_some_and(|l| l.locked);
        element_locked || layer_locked
    }

    /// Checks references and keys after loading a file.
    pub fn validate(&self) -> Result<(), ModelError> {
        if self.pages.is_empty() {
            return Err(ModelError::NoPages);
        }
        let bad_key = |k: &OrderKey| ModelError::InvalidOrderKey(k.as_str().to_owned());
        for (reference, icon) in &self.icons {
            if reference != &icon.reference {
                return Err(ModelError::InvalidIcon {
                    reference: reference.clone(),
                    message: "the map key does not match the stored reference".into(),
                });
            }
            icon.validate()?;
        }
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
            if !element.order.is_valid() {
                return Err(bad_key(&element.order));
            }
            self.check_parent(element.id, element.parent)?;
            self.check_kind(element)?;
        }
        self.check_cycles()
    }

    /// Whether `parent` exists and can hold children.
    pub fn check_parent(&self, element: ElementId, parent: Parent) -> Result<(), ModelError> {
        let missing = || ModelError::MissingParent { element, parent };
        match parent {
            Parent::Layer(layer) if self.layers.contains_key(&layer) => Ok(()),
            Parent::Layer(_) => Err(missing()),
            Parent::Element(p) => {
                let p = self.elements.get(&p).ok_or_else(missing)?;
                if p.can_have_children() {
                    Ok(())
                } else {
                    Err(ModelError::InvalidParent { element: p.id })
                }
            }
        }
    }

    /// Whether `element`'s own data is usable: finite coordinates, and
    /// connector ends attached only to shapes that exist.
    pub fn check_kind(&self, element: &Element) -> Result<(), ModelError> {
        let finite = |p: kurbo::Point| p.x.is_finite() && p.y.is_finite();
        match &element.kind {
            ElementKind::Shape(s) => {
                if s.shape.is_cloud() && !self.icons.contains_key(&s.shape) {
                    return Err(ModelError::MissingIcon {
                        element: element.id,
                        reference: s.shape.clone(),
                    });
                }
                let b = s.bounds;
                if ![b.x0, b.y0, b.x1, b.y1].iter().all(|v| v.is_finite()) {
                    return Err(ModelError::NotFinite(element.id));
                }
                match (s.shape.as_str() == "erd/table", s.erd.is_some()) {
                    (true, false) => return Err(ModelError::MissingErdData(element.id)),
                    (false, true) => return Err(ModelError::UnexpectedErdData(element.id)),
                    _ => {}
                }
                if let Some(table) = &s.erd {
                    let mut ids = HashSet::new();
                    for column in &table.columns {
                        if !ids.insert(column.id) {
                            return Err(ModelError::DuplicateColumn {
                                element: element.id,
                                column: column.id,
                            });
                        }
                        if !column.order.is_valid() {
                            return Err(ModelError::InvalidOrderKey(
                                column.order.as_str().to_owned(),
                            ));
                        }
                    }
                }
            }
            ElementKind::Connector(c) => {
                for end in c.endpoints() {
                    match end {
                        crate::Endpoint::Free(p) if !finite(*p) => {
                            return Err(ModelError::NotFinite(element.id));
                        }
                        crate::Endpoint::Free(_) => {}
                        crate::Endpoint::Glued { .. } => self.check_endpoint(element.id, end)?,
                    }
                }
                if !c.waypoints.iter().copied().all(finite) {
                    return Err(ModelError::NotFinite(element.id));
                }
            }
            ElementKind::Group => {}
        }
        Ok(())
    }

    /// Checks glued element and stable column-port references.
    pub fn check_endpoint(
        &self,
        connector: ElementId,
        endpoint: &Endpoint,
    ) -> Result<(), ModelError> {
        if let Endpoint::Glued {
            element: target,
            port,
        } = endpoint
        {
            let shape = self
                .elements
                .get(target)
                .and_then(Element::as_shape)
                .ok_or(ModelError::BadEndpoint {
                    connector,
                    target: *target,
                })?;
            if let Some(port) = port
                .as_ref()
                .filter(|port| port.as_str().starts_with("column:"))
            {
                let column = port
                    .column_id()
                    .ok_or_else(|| ModelError::InvalidColumnPort {
                        connector,
                        target: *target,
                        port: port.as_str().to_owned(),
                    })?;
                if !shape
                    .erd
                    .as_ref()
                    .is_some_and(|table| table.column(column).is_some())
                {
                    return Err(ModelError::MissingColumnEndpoint {
                        connector,
                        target: *target,
                        column,
                    });
                }
            }
        }
        Ok(())
    }

    fn check_cycles(&self) -> Result<(), ModelError> {
        let mut known_good: HashSet<ElementId> = HashSet::new();
        for &start in self.elements.keys() {
            let mut chain = Vec::new();
            let mut current = start;
            loop {
                if known_good.contains(&current) {
                    break;
                }
                if chain.contains(&current) {
                    return Err(ModelError::Cycle(current));
                }
                chain.push(current);
                match self.elements.get(&current).map(|e| e.parent) {
                    Some(Parent::Element(parent)) => current = parent,
                    _ => break,
                }
            }
            known_good.extend(chain);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Endpoint, ShapeRef};
    use kurbo::{Point, Rect};

    fn rect_ref() -> ShapeRef {
        ShapeRef::new("basic", "rectangle")
    }

    fn add_shape(doc: &mut Document, parent: Parent, x: f64) -> ElementId {
        let order = doc.next_order_key(parent);
        let el = Element::shape(rect_ref(), parent, order, Rect::new(x, 0.0, x + 5.0, 5.0));
        let id = el.id;
        doc.elements.insert(id, el);
        id
    }

    fn doc_with_three() -> (Document, LayerId, Vec<ElementId>) {
        let mut doc = Document::new();
        let page = doc.first_page().unwrap();
        let layer = doc.layers_of(page)[0].id;
        let ids = (0..3)
            .map(|i| add_shape(&mut doc, Parent::Layer(layer), f64::from(i) * 10.0))
            .collect();
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
    fn page_kind_is_optional_and_serializes_with_stable_names() {
        let mut page = Page::new("Page 1", OrderKey::first());
        let legacy = serde_json::to_value(&page).unwrap();
        assert!(legacy.get("diagram_kind").is_none());
        assert_eq!(serde_json::from_value::<Page>(legacy).unwrap(), page);

        for (kind, value, label) in [
            (DiagramKind::Flowchart, "flowchart", "Flowchart"),
            (DiagramKind::Erd, "erd", "ERD"),
        ] {
            page.diagram_kind = Some(kind);
            let json = serde_json::to_value(&page).unwrap();
            assert_eq!(json["diagram_kind"], value);
            assert_eq!(serde_json::from_value::<Page>(json).unwrap(), page);
            assert_eq!(kind.label(), label);
        }
    }

    #[test]
    fn new_elements_stack_on_top() {
        let (doc, _, ids) = doc_with_three();
        let page = doc.first_page().unwrap();
        let order: Vec<_> = doc.paint_order(page).iter().map(|e| e.id).collect();
        assert_eq!(order, ids);
    }

    #[test]
    fn hidden_layers_are_skipped_when_painting() {
        let (mut doc, layer, _) = doc_with_three();
        doc.layers.get_mut(&layer).unwrap().visible = false;
        let page = doc.first_page().unwrap();
        assert!(doc.paint_order(page).is_empty());
        assert_eq!(doc.page_elements(page).len(), 3);
    }

    #[test]
    fn groups_paint_their_children_in_place() {
        let (mut doc, layer, ids) = doc_with_three();
        // A group between the first and second shape, holding two shapes.
        let order = OrderKey::between(
            Some(&doc.elements[&ids[0]].order),
            Some(&doc.elements[&ids[1]].order),
        );
        let group = Element::group(Parent::Layer(layer), order);
        let g = group.id;
        doc.elements.insert(g, group);
        let a = add_shape(&mut doc, Parent::Element(g), 100.0);
        let b = add_shape(&mut doc, Parent::Element(g), 110.0);

        let page = doc.first_page().unwrap();
        let order: Vec<_> = doc.paint_order(page).iter().map(|e| e.id).collect();
        assert_eq!(order, vec![ids[0], g, a, b, ids[1], ids[2]]);
        assert_eq!(doc.ancestors(b), vec![g]);
        assert_eq!(doc.descendants(g), vec![a, b]);
        assert_eq!(doc.layer_of(b), Some(layer));
        assert_eq!(doc.page_of(b), Some(page));
        assert_eq!(doc.validate(), Ok(()));
    }

    #[test]
    fn locks_are_inherited() {
        let (mut doc, layer, ids) = doc_with_three();
        let group = Element::group(
            Parent::Layer(layer),
            doc.next_order_key(Parent::Layer(layer)),
        );
        let g = group.id;
        doc.elements.insert(g, group);
        let child = add_shape(&mut doc, Parent::Element(g), 50.0);
        assert!(!doc.is_locked(child));
        doc.elements.get_mut(&g).unwrap().locked = true;
        assert!(doc.is_locked(child));
        assert!(!doc.is_locked(ids[0]));
        doc.layers.get_mut(&layer).unwrap().locked = true;
        assert!(doc.is_locked(ids[0]));
    }

    #[test]
    fn json_round_trip() {
        let (mut doc, layer, ids) = doc_with_three();
        let c = Element::connector(
            Endpoint::glued(ids[0], Some("e")),
            Endpoint::Free(Point::new(1.0, 2.0)),
            Parent::Layer(layer),
            doc.next_order_key(Parent::Layer(layer)),
        );
        doc.elements.insert(c.id, c);
        let json = serde_json::to_string(&doc).unwrap();
        let back: Document = serde_json::from_str(&json).unwrap();
        assert_eq!(doc, back);
        assert_eq!(back.validate(), Ok(()));
    }

    #[test]
    fn validate_catches_dangling_parent() {
        let (mut doc, _, ids) = doc_with_three();
        doc.elements.get_mut(&ids[0]).unwrap().parent = Parent::Layer(LayerId::new());
        assert!(matches!(
            doc.validate(),
            Err(ModelError::MissingParent { .. })
        ));
    }

    #[test]
    fn validate_catches_cycles() {
        let (mut doc, layer, _) = doc_with_three();
        let a = Element::group(Parent::Layer(layer), OrderKey::first());
        let b = Element::group(Parent::Element(a.id), OrderKey::first());
        let (a_id, b_id) = (a.id, b.id);
        doc.elements.insert(a_id, a);
        doc.elements.insert(b_id, b);
        assert_eq!(doc.validate(), Ok(()));
        doc.elements.get_mut(&a_id).unwrap().parent = Parent::Element(b_id);
        assert!(matches!(doc.validate(), Err(ModelError::Cycle(_))));
        assert!(doc.ancestors(a_id).len() <= 2, "ancestors stops on a cycle");
    }

    #[test]
    fn validate_catches_bad_connectors() {
        let (mut doc, layer, ids) = doc_with_three();
        let missing = ElementId::new();
        let c = Element::connector(
            Endpoint::glued(ids[0], None),
            Endpoint::glued(missing, None),
            Parent::Layer(layer),
            OrderKey::first(),
        );
        doc.elements.insert(c.id, c);
        assert!(matches!(
            doc.validate(),
            Err(ModelError::BadEndpoint { target, .. }) if target == missing
        ));
    }

    #[test]
    fn connectors_cannot_hold_children() {
        let (mut doc, layer, ids) = doc_with_three();
        let c = Element::connector(
            Endpoint::glued(ids[0], None),
            Endpoint::glued(ids[1], None),
            Parent::Layer(layer),
            OrderKey::first(),
        );
        let c_id = c.id;
        doc.elements.insert(c_id, c);
        assert_eq!(doc.connectors_attached_to(ids[1]), vec![c_id]);
        doc.elements.get_mut(&ids[2]).unwrap().parent = Parent::Element(c_id);
        assert!(matches!(
            doc.validate(),
            Err(ModelError::InvalidParent { .. })
        ));
    }

    #[test]
    fn validate_catches_nan() {
        let (mut doc, _, ids) = doc_with_three();
        if let ElementKind::Shape(s) = &mut doc.elements.get_mut(&ids[0]).unwrap().kind {
            s.bounds.x1 = f64::NAN;
        }
        assert!(matches!(doc.validate(), Err(ModelError::NotFinite(_))));
    }

    #[test]
    fn column_ids_are_unique_within_each_table() {
        let mut doc = Document::new();
        let parent = Parent::Layer(doc.layers_of(doc.first_page().unwrap())[0].id);
        let table = Element::shape(
            ShapeRef::new("erd", "table"),
            parent,
            OrderKey::first(),
            Rect::new(0.0, 0.0, 240.0, 120.0),
        );
        let id = table.id;
        let column = table.as_shape().unwrap().erd.as_ref().unwrap().columns[0].clone();
        let mut copy = table.clone();
        copy.id = ElementId::new();
        doc.elements.insert(copy.id, copy);
        doc.elements.insert(id, table);
        assert_eq!(
            doc.validate(),
            Ok(()),
            "ids may repeat across copied tables"
        );
        doc.elements
            .get_mut(&id)
            .unwrap()
            .as_shape_mut()
            .unwrap()
            .erd
            .as_mut()
            .unwrap()
            .columns
            .push(column.clone());
        assert_eq!(
            doc.validate(),
            Err(ModelError::DuplicateColumn {
                element: id,
                column: column.id
            })
        );
    }

    #[test]
    fn column_ports_reject_missing_rows_and_malformed_ids() {
        let mut doc = Document::new();
        let parent = Parent::Layer(doc.layers_of(doc.first_page().unwrap())[0].id);
        let table = Element::shape(
            ShapeRef::new("erd", "table"),
            parent,
            OrderKey::first(),
            Rect::new(0.0, 0.0, 240.0, 120.0),
        );
        let id = table.id;
        let column = table.as_shape().unwrap().erd.as_ref().unwrap().columns[0].id;
        doc.elements.insert(id, table);
        let mut connector = Element::connector(
            Endpoint::Glued {
                element: id,
                port: Some(crate::PortId::column(column, true)),
            },
            Endpoint::Free(Point::ZERO),
            parent,
            doc.next_order_key(parent),
        );
        let connector_id = connector.id;
        doc.elements.insert(connector_id, connector.clone());
        assert_eq!(doc.validate(), Ok(()));
        let missing = ColumnId::new();
        connector.as_connector_mut().unwrap().source = Endpoint::Glued {
            element: id,
            port: Some(crate::PortId::column(missing, false)),
        };
        doc.elements.insert(connector_id, connector.clone());
        assert_eq!(
            doc.validate(),
            Err(ModelError::MissingColumnEndpoint {
                connector: connector_id,
                target: id,
                column: missing
            })
        );
        connector.as_connector_mut().unwrap().source =
            Endpoint::glued(id, Some("column:invalid:e"));
        doc.elements.insert(connector_id, connector);
        assert!(matches!(
            doc.validate(),
            Err(ModelError::InvalidColumnPort { .. })
        ));
    }
}
