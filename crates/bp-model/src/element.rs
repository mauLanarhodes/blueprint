use crate::{ColumnId, ElementId, ErdTable, LayerId, OrderKey, Style};
use kurbo::{Point, Rect};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Where an element sits in the tree: directly on a layer, or inside a
/// group or container. Siblings are stacked by their [`OrderKey`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Parent {
    Layer(LayerId),
    Element(ElementId),
}

/// Names a shape definition in a shape library, as `library/shape`
/// (for example `flowchart/decision`).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(into = "String", try_from = "String")]
pub struct ShapeRef(String);

impl ShapeRef {
    pub fn new(library: &str, shape: &str) -> Self {
        Self(format!("{library}/{shape}"))
    }

    /// Accepts `library/shape` where both parts are non-empty.
    pub fn parse(s: &str) -> Option<Self> {
        let (library, shape) = s.split_once('/')?;
        let ok = |part: &str| {
            !part.is_empty()
                && part
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '@'))
        };
        (ok(library) && ok(shape)).then(|| Self(s.to_owned()))
    }

    pub fn library(&self) -> &str {
        self.0.split_once('/').map_or("", |(l, _)| l)
    }

    pub fn shape(&self) -> &str {
        self.0.split_once('/').map_or("", |(_, s)| s)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn is_cloud(&self) -> bool {
        matches!(self.library(), "aws" | "azure")
    }
}

impl fmt::Display for ShapeRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<ShapeRef> for String {
    fn from(r: ShapeRef) -> String {
        r.0
    }
}

impl TryFrom<String> for ShapeRef {
    type Error = String;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        ShapeRef::parse(&s).ok_or_else(|| format!("invalid shape reference {s:?}"))
    }
}

/// The stable name of a connection point within its shape definition
/// (`n`, `e`, `s`, `w`, or a name the definition declares).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PortId(pub String);

impl PortId {
    pub fn new(name: &str) -> Self {
        Self(name.to_owned())
    }

    /// A stable port on the left or right edge of a column row.
    pub fn column(id: ColumnId, left: bool) -> Self {
        Self(format!("column:{id}:{}", if left { "w" } else { "e" }))
    }

    pub fn column_id(&self) -> Option<ColumnId> {
        let (id, side) = self.0.strip_prefix("column:")?.split_once(':')?;
        if !matches!(side, "w" | "e") {
            return None;
        }
        uuid::Uuid::parse_str(id).ok().map(ColumnId)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// One end of a connector.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Endpoint {
    /// Attached to a shape: at a named port, or (with no port) floating on
    /// the outline wherever the route meets it.
    Glued {
        element: ElementId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        port: Option<PortId>,
    },
    /// Not attached to anything, at a point in page units.
    Free(Point),
}

impl Endpoint {
    pub fn glued(element: ElementId, port: Option<&str>) -> Self {
        Endpoint::Glued {
            element,
            port: port.map(PortId::new),
        }
    }

    /// The shape this end is attached to, if any.
    pub fn element(&self) -> Option<ElementId> {
        match self {
            Endpoint::Glued { element, .. } => Some(*element),
            Endpoint::Free(_) => None,
        }
    }
}

/// How a connector's path is laid out between its ends.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Routing {
    Straight,
    #[default]
    Orthogonal,
    Curved,
}

impl Routing {
    pub const ALL: [Routing; 3] = [Routing::Straight, Routing::Orthogonal, Routing::Curved];

    pub fn label(self) -> &'static str {
        match self {
            Routing::Straight => "Straight",
            Routing::Orthogonal => "Orthogonal",
            Routing::Curved => "Curved",
        }
    }
}

/// The decoration drawn at either end of a connector.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Marker {
    #[default]
    None,
    Arrow,
    OpenArrow,
    Triangle,
    Diamond,
    OpenDiamond,
    Circle,
    OpenCircle,
    ExactlyOne,
    ZeroOrOne,
    OneOrMany,
    ZeroOrMany,
    Many,
}

impl Marker {
    pub const ALL: [Marker; 13] = [
        Marker::None,
        Marker::Arrow,
        Marker::OpenArrow,
        Marker::Triangle,
        Marker::Diamond,
        Marker::OpenDiamond,
        Marker::Circle,
        Marker::OpenCircle,
        Marker::ExactlyOne,
        Marker::ZeroOrOne,
        Marker::OneOrMany,
        Marker::ZeroOrMany,
        Marker::Many,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Marker::None => "None",
            Marker::Arrow => "Arrow",
            Marker::OpenArrow => "Open arrow",
            Marker::Triangle => "Triangle",
            Marker::Diamond => "Diamond",
            Marker::OpenDiamond => "Open diamond",
            Marker::Circle => "Circle",
            Marker::OpenCircle => "Open circle",
            Marker::ExactlyOne => "Exactly one",
            Marker::ZeroOrOne => "Zero or one",
            Marker::OneOrMany => "One or many",
            Marker::ZeroOrMany => "Zero or many",
            Marker::Many => "Many",
        }
    }

    fn is_none(&self) -> bool {
        *self == Marker::None
    }

    fn is_arrow(&self) -> bool {
        *self == Marker::Arrow
    }
}

fn arrow() -> Marker {
    Marker::Arrow
}

fn half() -> f64 {
    0.5
}

fn is_half(v: &f64) -> bool {
    *v == 0.5
}

fn is_default<T: Default + PartialEq>(v: &T) -> bool {
    *v == T::default()
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// A shape from a shape library, drawn in an axis-aligned box.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Shape {
    pub shape: ShapeRef,
    /// The box in page units (1 unit = 1 CSS pixel at 100% zoom).
    pub bounds: Rect,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub text: String,
    #[serde(default, skip_serializing_if = "Style::is_empty")]
    pub style: Style,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub erd: Option<ErdTable>,
}

/// A line between two endpoints. Its route is derived on every load, so
/// only the endpoints, waypoints and options are stored.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Connector {
    pub source: Endpoint,
    pub target: Endpoint,
    /// Points the route must pass through, in page units.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub waypoints: Vec<Point>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub routing: Routing,
    #[serde(default, skip_serializing_if = "Marker::is_none")]
    pub start_marker: Marker,
    #[serde(default = "arrow", skip_serializing_if = "Marker::is_arrow")]
    pub end_marker: Marker,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub text: String,
    /// Where the label sits along the route, from 0 (source) to 1 (target).
    #[serde(default = "half", skip_serializing_if = "is_half")]
    pub label_position: f64,
    #[serde(default, skip_serializing_if = "Style::is_empty")]
    pub style: Style,
}

impl Connector {
    pub fn new(source: Endpoint, target: Endpoint) -> Self {
        Self {
            source,
            target,
            waypoints: Vec::new(),
            routing: Routing::default(),
            start_marker: Marker::None,
            end_marker: Marker::Arrow,
            text: String::new(),
            label_position: 0.5,
            style: Style::default(),
        }
    }

    pub fn endpoints(&self) -> [&Endpoint; 2] {
        [&self.source, &self.target]
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ElementKind {
    Shape(Shape),
    Connector(Connector),
    /// Holds child elements so they select and move together. Its bounds
    /// are derived from the children.
    Group,
}

/// Anything on a page.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Element {
    pub id: ElementId,
    pub parent: Parent,
    pub order: OrderKey,
    /// Locked elements can be selected but not moved, resized or deleted.
    #[serde(default, skip_serializing_if = "is_false")]
    pub locked: bool,
    #[serde(flatten)]
    pub kind: ElementKind,
}

impl Element {
    pub fn new(parent: Parent, order: OrderKey, kind: ElementKind) -> Self {
        Self {
            id: ElementId::new(),
            parent,
            order,
            locked: false,
            kind,
        }
    }

    /// A shape with no style overrides. `bounds` is normalised.
    pub fn shape(shape: ShapeRef, parent: Parent, order: OrderKey, bounds: Rect) -> Self {
        let erd = (shape.as_str() == "erd/table").then(ErdTable::default);
        let text = if erd.is_some() {
            "Table".to_owned()
        } else {
            String::new()
        };
        Self::new(
            parent,
            order,
            ElementKind::Shape(Shape {
                shape,
                bounds: bounds.abs(),
                text,
                style: Style::default(),
                erd,
            }),
        )
    }

    pub fn connector(source: Endpoint, target: Endpoint, parent: Parent, order: OrderKey) -> Self {
        Self::new(
            parent,
            order,
            ElementKind::Connector(Connector::new(source, target)),
        )
    }

    pub fn group(parent: Parent, order: OrderKey) -> Self {
        Self::new(parent, order, ElementKind::Group)
    }

    pub fn as_shape(&self) -> Option<&Shape> {
        match &self.kind {
            ElementKind::Shape(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_shape_mut(&mut self) -> Option<&mut Shape> {
        match &mut self.kind {
            ElementKind::Shape(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_connector(&self) -> Option<&Connector> {
        match &self.kind {
            ElementKind::Connector(c) => Some(c),
            _ => None,
        }
    }

    pub fn as_connector_mut(&mut self) -> Option<&mut Connector> {
        match &mut self.kind {
            ElementKind::Connector(c) => Some(c),
            _ => None,
        }
    }

    pub fn is_shape(&self) -> bool {
        matches!(self.kind, ElementKind::Shape(_))
    }

    pub fn is_connector(&self) -> bool {
        matches!(self.kind, ElementKind::Connector(_))
    }

    pub fn is_group(&self) -> bool {
        matches!(self.kind, ElementKind::Group)
    }

    /// Whether other elements may sit inside this one.
    pub fn can_have_children(&self) -> bool {
        !self.is_connector()
    }

    pub fn text(&self) -> Option<&str> {
        match &self.kind {
            ElementKind::Shape(s) => Some(&s.text),
            ElementKind::Connector(c) => Some(&c.text),
            ElementKind::Group => None,
        }
    }

    pub fn style(&self) -> Option<&Style> {
        match &self.kind {
            ElementKind::Shape(s) => Some(&s.style),
            ElementKind::Connector(c) => Some(&c.style),
            ElementKind::Group => None,
        }
    }

    /// A short name for menus and the inspector.
    pub fn kind_label(&self) -> &'static str {
        match self.kind {
            ElementKind::Shape(_) => "Shape",
            ElementKind::Connector(_) => "Connector",
            ElementKind::Group => "Group",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Color, Paint};

    #[test]
    fn shape_refs() {
        let r = ShapeRef::new("flowchart", "decision");
        assert_eq!(r.as_str(), "flowchart/decision");
        assert_eq!((r.library(), r.shape()), ("flowchart", "decision"));
        assert!(ShapeRef::parse("basic/rounded-rectangle").is_some());
        for bad in ["", "basic", "/x", "x/", "a/b c", "a\\b"] {
            assert!(ShapeRef::parse(bad).is_none(), "{bad:?}");
        }
    }

    #[test]
    fn column_ports_keep_the_same_stable_row_on_both_sides() {
        let id = ColumnId::new();
        let left = PortId::column(id, true);
        let right = PortId::column(id, false);
        assert_eq!(left.as_str(), format!("column:{id}:w"));
        assert_eq!(right.as_str(), format!("column:{id}:e"));
        assert_eq!(left.column_id(), Some(id));
        assert_eq!(right.column_id(), Some(id));
        assert_eq!(PortId::new("n").column_id(), None);
        assert_eq!(PortId::new(&format!("column:{id}:n")).column_id(), None);
    }

    #[test]
    fn shape_json_is_flat_and_sparse() {
        let layer = LayerId::new();
        let mut el = Element::shape(
            ShapeRef::new("basic", "rectangle"),
            Parent::Layer(layer),
            OrderKey::first(),
            Rect::new(10.0, 20.0, 0.0, 0.0),
        );
        if let ElementKind::Shape(s) = &mut el.kind {
            s.style.fill = Some(Paint::Color(Color::WHITE));
        }
        let json = serde_json::to_value(&el).unwrap();
        assert_eq!(json["type"], "shape");
        assert_eq!(json["shape"], "basic/rectangle");
        assert_eq!(json["parent"]["layer"], layer.to_string());
        assert_eq!(json["bounds"]["x1"], 10.0, "bounds are normalised");
        assert_eq!(json["style"], serde_json::json!({ "fill": "#ffffff" }));
        assert!(json.get("text").is_none() && json.get("locked").is_none());
        let back: Element = serde_json::from_value(json).unwrap();
        assert_eq!(back, el);
    }

    #[test]
    fn connector_endpoints_round_trip() {
        let a = ElementId::new();
        let el = Element::connector(
            Endpoint::glued(a, Some("e")),
            Endpoint::Free(Point::new(5.0, 6.0)),
            Parent::Element(ElementId::new()),
            OrderKey::first(),
        );
        let json = serde_json::to_value(&el).unwrap();
        assert_eq!(json["type"], "connector");
        assert_eq!(
            json["source"],
            serde_json::json!({ "element": a.to_string(), "port": "e" })
        );
        assert_eq!(json["target"], serde_json::json!({ "x": 5.0, "y": 6.0 }));
        assert!(json.get("end_marker").is_none(), "arrow is the default");
        assert!(json.get("routing").is_none(), "orthogonal is the default");
        let back: Element = serde_json::from_value(json).unwrap();
        assert_eq!(back, el);

        let floating: Endpoint =
            serde_json::from_value(serde_json::json!({ "element": a.to_string() })).unwrap();
        assert_eq!(floating, Endpoint::glued(a, None));
    }

    #[test]
    fn groups_serialise_as_a_bare_type() {
        let el = Element::group(Parent::Layer(LayerId::new()), OrderKey::first());
        let json = serde_json::to_value(&el).unwrap();
        assert_eq!(json["type"], "group");
        let back: Element = serde_json::from_value(json).unwrap();
        assert_eq!(back, el);
    }
}
