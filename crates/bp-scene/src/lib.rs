//! Turns a page into a [`Scene`]: resolved geometry for every element
//! (outlines, ports, connector routes, text layout) and a render-agnostic
//! display list. The canvas, hit-testing and every exporter use the same
//! scene, so an export always matches what is on screen.
//!
//! [`SceneCache`] keeps each element's display items between builds and
//! rebuilds only elements whose data changed (and connectors whose shapes
//! changed), so dragging one shape in a large diagram stays cheap.

mod cloud;
mod connector;
mod erd;
mod text;

pub use connector::ROUTE_MARGIN;
pub use erd::{ErdGeometry, ErdRowGeometry, erd_header_height, erd_row_height};
pub use text::{PADDING_X, PADDING_Y, TextStyle, place as place_text};

use bp_geom::{SpatialIndex, distance_to_path, distance_to_polyline};
use bp_model::kurbo::{Affine, BezPath, Point, Rect, Shape as _};
use bp_model::{
    Color, Connector, Document, Element, ElementId, ElementKind, IdMap, PageId, Parent, Shape,
    StyleValues, TextAlign, VerticalAlign,
};
use bp_shapes::{Libraries, Port};
use bp_text::Face;
use connector::End;
use std::sync::Arc;

/// Offset and colour of the hard drop shadow drawn under shapes.
pub const SHADOW_OFFSET: f64 = 3.0;
const SHADOW: Color = Color::rgba(15, 23, 42, 46);
/// Labels on connectors wrap at this width.
pub const LABEL_WIDTH: f64 = 200.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stroke {
    pub color: Color,
    pub width: f64,
    /// On/off lengths, or `None` for a solid line.
    pub dash: Option<[f64; 2]>,
}

/// One line of placed text.
#[derive(Clone, Debug, PartialEq)]
pub struct PlacedLine {
    pub text: String,
    /// The line's left edge, centre or right edge, per the run's alignment.
    pub x: f64,
    pub baseline: f64,
    pub width: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TextRun {
    pub lines: Vec<PlacedLine>,
    pub face: Face,
    pub size: f64,
    pub color: Color,
    pub align: TextAlign,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Primitive {
    Path {
        path: BezPath,
        fill: Option<Color>,
        stroke: Option<Stroke>,
    },
    Text(TextRun),
    /// Original provider SVG, fitted into `bounds` without changing its colours.
    Icon {
        svg: Arc<str>,
        bounds: Rect,
        opacity: f64,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct DisplayItem {
    pub element: ElementId,
    /// Area the item covers, including half the stroke width. Used to skip
    /// items outside the viewport.
    pub bbox: Rect,
    pub primitive: Primitive,
}

/// Everything to draw for a page, bottom to top, grouped by element so
/// unchanged elements share their items between builds.
#[derive(Clone, Debug, Default)]
pub struct DisplayList {
    pub groups: Vec<Arc<[DisplayItem]>>,
    pub background: Option<Color>,
}

impl DisplayList {
    pub fn items(&self) -> impl Iterator<Item = &DisplayItem> {
        self.groups.iter().flat_map(|g| g.iter())
    }

    /// Union of every item's `bbox`, or `None` for an empty page.
    pub fn bounds(&self) -> Option<Rect> {
        self.items().map(|i| i.bbox).reduce(|a, b| a.union(b))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ShapeGeometry {
    pub bounds: Rect,
    pub outline: BezPath,
    pub back: Vec<BezPath>,
    /// Whether the outline encloses an area.
    pub closed: bool,
    pub ports: Vec<Port>,
    pub text_box: Rect,
    /// A cloud service's clickable label below its resize bounds.
    pub label: Option<Rect>,
    /// Header and visible column rows of a smart ERD table.
    pub erd: Option<ErdGeometry>,
    pub style: StyleValues,
}

impl ShapeGeometry {
    /// Ports offered when drawing a new connector. Hidden ERD rows retain
    /// their ports for existing relationships, but cannot be picked anew.
    pub fn visible_ports(&self) -> impl Iterator<Item = &Port> {
        self.ports.iter().filter(|port| {
            port.id.column_id().is_none_or(|column| {
                self.erd
                    .as_ref()
                    .is_none_or(|table| table.rows.iter().any(|row| row.column == column))
            })
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ConnectorGeometry {
    /// A polyline following the drawn path (before markers trim it).
    pub points: Vec<Point>,
    pub path: BezPath,
    /// Where the label text is drawn, if there is a label.
    pub label: Option<Rect>,
    /// Where a label goes: `label_position` along the route.
    pub label_anchor: Point,
    pub width: f64,
    pub bounds: Rect,
    pub style: StyleValues,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Geometry {
    Shape(ShapeGeometry),
    Connector(ConnectorGeometry),
    Group { bounds: Rect },
}

impl Geometry {
    pub fn bounds(&self) -> Rect {
        match self {
            Geometry::Shape(s) => s.bounds,
            Geometry::Connector(c) => c.bounds,
            Geometry::Group { bounds } => *bounds,
        }
    }
}

/// The resolved style defaults of connectors.
pub fn connector_defaults() -> StyleValues {
    StyleValues {
        fill: None,
        font_size: 12.0,
        ..StyleValues::default()
    }
}

/// A shape's resolved geometry, from its data alone.
pub fn shape_geometry(libraries: &Libraries, shape: &Shape) -> ShapeGeometry {
    if shape.shape.is_cloud() {
        return cloud::geometry(shape);
    }
    let def = libraries.resolve(&shape.shape);
    let style = shape.style.resolve(&def.default_style());
    if let Some(table) = &shape.erd {
        return erd::geometry(shape, table, style);
    }
    let bounds = shape.bounds.abs();
    ShapeGeometry {
        outline: def.outline(bounds, style.corner_radius),
        back: def.back(bounds),
        closed: def.is_closed(),
        ports: def.ports(bounds, style.corner_radius),
        text_box: def.text_box(bounds),
        label: None,
        erd: None,
        bounds,
        style,
    }
}

fn stroke_of(style: &StyleValues) -> Option<Stroke> {
    style
        .stroke
        .filter(|_| style.stroke_width > 0.0)
        .map(|color| Stroke {
            color: color.faded(style.opacity),
            width: style.stroke_width,
            dash: style.dash.pattern(style.stroke_width),
        })
}

fn path_item(
    element: ElementId,
    path: BezPath,
    fill: Option<Color>,
    stroke: Option<Stroke>,
) -> DisplayItem {
    let half = stroke.map_or(0.0, |s| s.width / 2.0);
    DisplayItem {
        element,
        bbox: path.bounding_box().inflate(half, half),
        primitive: Primitive::Path { path, fill, stroke },
    }
}

fn shape_items(
    id: ElementId,
    doc: &Document,
    libraries: &Libraries,
    shape: &Shape,
    g: &ShapeGeometry,
) -> Vec<DisplayItem> {
    if shape.shape.is_cloud() {
        return cloud::items(id, doc.icons.get(&shape.shape), shape, g);
    }
    let def = libraries.resolve(&shape.shape);
    let s = &g.style;
    let fill = s.fill.map(|c| c.faded(s.opacity)).filter(|_| g.closed);
    let stroke = stroke_of(s);
    let mut items = Vec::new();
    if s.shadow && fill.is_some() {
        let shift = Affine::translate((SHADOW_OFFSET, SHADOW_OFFSET));
        let shadow = Some(SHADOW.faded(s.opacity));
        for path in g.back.iter().chain([&g.outline]) {
            items.push(path_item(id, shift * path.clone(), shadow, None));
        }
    }
    for back in &g.back {
        items.push(path_item(id, back.clone(), fill, stroke));
    }
    if fill.is_some() || stroke.is_some() {
        items.push(path_item(id, g.outline.clone(), fill, stroke));
    }
    if let (Some(table), Some(geometry)) = (&shape.erd, &g.erd) {
        erd::items(id, shape, table, g, geometry, &mut items);
        return items;
    }
    if let (Some(details), Some(stroke)) = (def.details(g.bounds), stroke) {
        items.push(path_item(id, details, None, Some(stroke)));
    }
    let style = TextStyle {
        face: Face::new(s.bold, s.italic),
        size: s.font_size,
        color: s.text_color.faded(s.opacity),
        align: s.text_align,
        vertical_align: s.vertical_align,
    };
    if let Some((run, covered)) = text::place(&shape.text, g.text_box, &style, true) {
        items.push(DisplayItem {
            element: id,
            bbox: covered,
            primitive: Primitive::Text(run),
        });
    }
    items
}

fn connector_items(
    id: ElementId,
    c: &Connector,
    source: &End,
    target: &End,
    background: Color,
) -> (Vec<DisplayItem>, ConnectorGeometry) {
    let style = c.style.resolve(&connector_defaults());
    let route = connector::route(c, source, target);
    let stroke = stroke_of(&style);
    let mut items = Vec::new();
    let mut trims = [0.0, 0.0];
    let mut markers = Vec::new();
    if let Some(stroke) = stroke {
        for (k, (kind, at_end)) in [(c.start_marker, false), (c.end_marker, true)]
            .into_iter()
            .enumerate()
        {
            let tip = if at_end {
                route.points.last()
            } else {
                route.points.first()
            };
            let (Some(&tip), Some(dir)) = (tip, connector::end_direction(&route.path, at_end))
            else {
                continue;
            };
            let (paths, trim) = connector::marker(kind, tip, dir, &stroke, background);
            trims[k] = trim;
            markers.extend(paths);
        }
        let line = connector::trim(&route.path, trims[0], trims[1]);
        if line.segments().next().is_some() {
            items.push(path_item(id, line, None, Some(stroke)));
        }
        for m in markers {
            items.push(path_item(id, m.path, m.fill, m.stroke));
        }
    }
    let anchor = connector::label_point(&route.points, c.label_position);
    let mut label = None;
    let text_style = TextStyle {
        face: Face::new(style.bold, style.italic),
        size: style.font_size,
        color: style.text_color.faded(style.opacity),
        align: TextAlign::Center,
        vertical_align: VerticalAlign::Middle,
    };
    let area = Rect::from_center_size(anchor, (LABEL_WIDTH + 2.0 * PADDING_X, 0.0));
    if let Some((run, covered)) = text::place(&c.text, area, &text_style, true) {
        let back = covered.inflate(3.0, 1.0);
        items.push(path_item(id, back.to_path(0.1), Some(background), None));
        items.push(DisplayItem {
            element: id,
            bbox: covered,
            primitive: Primitive::Text(run),
        });
        label = Some(back);
    }
    let mut bounds = route.path.bounding_box();
    for item in &items {
        bounds = bounds.union(item.bbox);
    }
    let geometry = ConnectorGeometry {
        points: route.points,
        path: route.path,
        label,
        label_anchor: anchor,
        width: style.stroke_width,
        bounds,
        style,
    };
    (items, geometry)
}

/// The resolved page: what to draw, and the geometry to hit-test against.
#[derive(Default)]
pub struct Scene {
    pub list: DisplayList,
    /// Every element on visible layers, bottom to top.
    pub order: Vec<ElementId>,
    /// Each element's paint position (its index in `order`) and geometry.
    geometry: IdMap<ElementId, (usize, Arc<Geometry>)>,
    /// Bounds of every shape and connector. Shared with the cache, which
    /// updates it in place when only a few elements changed.
    index: Arc<SpatialIndex<ElementId>>,
}

/// How far around a point the index is searched: covers stroke widths and
/// marker sizes that stick out of an element's recorded bounds.
const HIT_SLACK: f64 = 12.0;

impl Scene {
    pub fn geometry(&self, id: ElementId) -> Option<&Geometry> {
        self.geometry.get(&id).map(|(_, g)| g.as_ref())
    }

    pub fn shape(&self, id: ElementId) -> Option<&ShapeGeometry> {
        match self.geometry(id)? {
            Geometry::Shape(s) => Some(s),
            _ => None,
        }
    }

    pub fn connector(&self, id: ElementId) -> Option<&ConnectorGeometry> {
        match self.geometry(id)? {
            Geometry::Connector(c) => Some(c),
            _ => None,
        }
    }

    pub fn bounds_of(&self, id: ElementId) -> Option<Rect> {
        self.geometry(id).map(Geometry::bounds)
    }

    /// The bounds of everything drawn, or `None` for an empty page.
    pub fn bounds(&self) -> Option<Rect> {
        self.list.bounds()
    }

    /// Elements whose bounds come near `rect`, topmost first.
    fn candidates(&self, rect: Rect) -> Vec<ElementId> {
        let mut found: Vec<(usize, ElementId)> = self
            .index
            .query_rect(rect)
            .filter_map(|(_, id)| Some((self.geometry.get(id)?.0, *id)))
            .collect();
        found.sort_unstable_by_key(|(rank, _)| std::cmp::Reverse(*rank));
        found.into_iter().map(|(_, id)| id).collect()
    }

    /// The topmost shape or connector under `p` that `accept` allows.
    pub fn hit(
        &self,
        p: Point,
        tolerance: f64,
        accept: impl Fn(ElementId) -> bool,
    ) -> Option<ElementId> {
        let area = Rect::from_center_size(
            p,
            (2.0 * (tolerance + HIT_SLACK), 2.0 * (tolerance + HIT_SLACK)),
        );
        self.candidates(area)
            .into_iter()
            .filter(|id| accept(*id))
            .find(|id| self.hits(*id, p, tolerance))
    }

    /// Whether `p` is on element `id`, within `tolerance`.
    pub fn hits(&self, id: ElementId, p: Point, tolerance: f64) -> bool {
        match self.geometry(id) {
            Some(Geometry::Shape(g)) => {
                let edge = tolerance + g.style.stroke_width / 2.0;
                if g.closed {
                    g.outline.contains(p)
                        || g.label.is_some_and(|label| label.contains(p))
                        || g.back.iter().any(|b| b.contains(p))
                        || distance_to_path(&g.outline, p) <= edge
                } else {
                    g.bounds.inflate(tolerance, tolerance).contains(p)
                }
            }
            Some(Geometry::Connector(c)) => {
                distance_to_polyline(&c.points, p) <= tolerance + c.width / 2.0
                    || c.label.is_some_and(|l| l.contains(p))
            }
            Some(Geometry::Group { .. }) | None => false,
        }
    }

    /// Shapes and connectors whose bounds touch `rect`, in paint order.
    pub fn query(&self, rect: Rect) -> Vec<ElementId> {
        let mut ids = self.candidates(rect);
        ids.reverse();
        ids
    }

    /// Shapes and connectors lying entirely inside `rect`.
    pub fn enclosed(&self, rect: Rect) -> Vec<ElementId> {
        self.query(rect)
            .into_iter()
            .filter(|id| match self.geometry(*id) {
                Some(Geometry::Shape(g)) => rect.contains_rect(g.bounds),
                Some(Geometry::Connector(c)) => rect.contains_rect(c.bounds),
                _ => false,
            })
            .collect()
    }

    /// The port nearest to `p` within `tolerance`, on shapes `accept` allows.
    pub fn port_near(
        &self,
        p: Point,
        tolerance: f64,
        accept: impl Fn(ElementId) -> bool,
    ) -> Option<(ElementId, Port)> {
        let area = Rect::from_center_size(p, (2.0 * tolerance, 2.0 * tolerance));
        let mut best: Option<(f64, ElementId, Port)> = None;
        for id in self.candidates(area) {
            let Some(g) = self.shape(id).filter(|_| accept(id)) else {
                continue;
            };
            for port in g.visible_ports() {
                let d = (port.at - p).hypot();
                if d <= tolerance && best.as_ref().is_none_or(|(bd, ..)| d < *bd) {
                    best = Some((d, id, port.clone()));
                }
            }
        }
        best.map(|(_, id, port)| (id, port))
    }
}

/// The display items of `connector` as it would be drawn on `page`, without
/// adding it to the document: the live preview while drawing a connector.
pub fn connector_preview(
    doc: &Document,
    page: PageId,
    libraries: &Libraries,
    connector: &Connector,
) -> Vec<DisplayItem> {
    let background = doc.pages.get(&page).map_or(Color::WHITE, |p| p.background);
    let shapes: IdMap<ElementId, ShapeGeometry> = connector
        .endpoints()
        .iter()
        .filter_map(|end| {
            let id = end.element()?;
            let shape = doc.elements.get(&id)?.as_shape()?;
            Some((id, shape_geometry(libraries, shape)))
        })
        .collect();
    let shape_of = |id: ElementId| shapes.get(&id);
    let source = End::resolve(&connector.source, shape_of);
    let target = End::resolve(&connector.target, shape_of);
    connector_items(ElementId::new(), connector, &source, &target, background).0
}

/// Builds the scene for `page` from scratch with the built-in libraries.
pub fn build_page(doc: &Document, page: PageId) -> Scene {
    SceneCache::default().build(doc, page, Libraries::builtin())
}

/// What a cached element was built from.
enum Key {
    Shape(Box<Shape>, Option<Arc<str>>),
    Connector(Box<ConnectorKey>),
}

struct ConnectorKey {
    connector: Connector,
    ends: [EndKey; 2],
    background: Color,
}

/// What a connector end depended on when it was built.
#[derive(PartialEq)]
enum EndKey {
    Free,
    /// A shape built in the same scene, at this version of its entry.
    Shape(ElementId, u64),
    /// A shape outside the scene (hidden layer, other page), by its data.
    Elsewhere(ElementId, Option<Box<Shape>>),
}

struct Entry {
    key: Key,
    items: Arc<[DisplayItem]>,
    geometry: Arc<Geometry>,
    /// Bumped every time the entry is rebuilt, so connectors can tell
    /// whether their shapes changed without comparing them.
    version: u64,
    generation: u64,
}

/// Display items and geometry kept between builds.
#[derive(Default)]
pub struct SceneCache {
    entries: IdMap<ElementId, Entry>,
    generation: u64,
    next_version: u64,
    /// The spatial index of the last build, and what it holds, so the next
    /// build can update just the elements that moved.
    index: Arc<SpatialIndex<ElementId>>,
    indexed: IdMap<ElementId, Rect>,
    indexed_page: Option<PageId>,
    /// How many elements the last build had to rebuild.
    pub rebuilt: usize,
}

impl SceneCache {
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// The key of a connector end as of this build.
    fn end_key(&self, doc: &Document, generation: u64, end: &bp_model::Endpoint) -> EndKey {
        match end.element() {
            None => EndKey::Free,
            Some(id) => match self.entries.get(&id) {
                Some(e) if e.generation == generation && matches!(e.key, Key::Shape(..)) => {
                    EndKey::Shape(id, e.version)
                }
                _ => EndKey::Elsewhere(
                    id,
                    doc.elements
                        .get(&id)
                        .and_then(Element::as_shape)
                        .cloned()
                        .map(Box::new),
                ),
            },
        }
    }

    /// Builds the scene for `page`, reusing everything that did not change
    /// since the previous build. Drop the previous [`Scene`] first: it
    /// shares the spatial index, which can then be updated in place.
    pub fn build(&mut self, doc: &Document, page: PageId, libraries: &Libraries) -> Scene {
        self.generation += 1;
        self.rebuilt = 0;
        let generation = self.generation;
        let background = doc.pages.get(&page).map_or(Color::WHITE, |p| p.background);
        let order: Vec<&Element> = doc.paint_order(page);
        let mut slots: Vec<Option<Arc<[DisplayItem]>>> = vec![None; order.len()];
        let mut geometry: IdMap<ElementId, (usize, Arc<Geometry>)> = IdMap::default();
        geometry.reserve(order.len());
        let mut changed: Vec<ElementId> = Vec::new();

        // Pass 1: shapes. Connectors wait until every shape they may be
        // glued to is built.
        let mut connectors: Vec<(usize, &Element)> = Vec::new();
        for (rank, element) in order.iter().enumerate() {
            let id = element.id;
            let ElementKind::Shape(shape) = &element.kind else {
                if element.is_connector() {
                    connectors.push((rank, element));
                }
                continue;
            };
            let svg = doc.icons.get(&shape.shape).map(|icon| icon.svg.clone());
            if let Some(entry) = self.entries.get_mut(&id)
                && matches!(&entry.key, Key::Shape(old, old_svg) if old.as_ref() == shape && *old_svg == svg)
            {
                entry.generation = generation;
                slots[rank] = Some(entry.items.clone());
                geometry.insert(id, (rank, entry.geometry.clone()));
                continue;
            }
            let g = shape_geometry(libraries, shape);
            let items: Arc<[DisplayItem]> = shape_items(id, doc, libraries, shape, &g).into();
            let g = Arc::new(Geometry::Shape(g));
            self.store(
                id,
                Key::Shape(Box::new(shape.clone()), svg),
                items.clone(),
                g.clone(),
                generation,
            );
            slots[rank] = Some(items);
            geometry.insert(id, (rank, g));
            changed.push(id);
        }

        // Pass 2: connectors, reusing the shapes' geometry from pass 1.
        let mut built = Vec::new();
        {
            // Shapes a connector is glued to that are not in this scene.
            let mut elsewhere: IdMap<ElementId, ShapeGeometry> = IdMap::default();
            let mut stale = Vec::new();
            for &(rank, element) in &connectors {
                let c = element.as_connector().expect("a connector");
                let ends = [
                    self.end_key(doc, generation, &c.source),
                    self.end_key(doc, generation, &c.target),
                ];
                let fresh = self.entries.get(&element.id).is_some_and(|e| {
                    matches!(&e.key, Key::Connector(k)
                        if k.connector == *c && k.background == background && k.ends == ends)
                });
                if fresh {
                    let entry = self.entries.get_mut(&element.id).expect("checked");
                    entry.generation = generation;
                    slots[rank] = Some(entry.items.clone());
                    geometry.insert(element.id, (rank, entry.geometry.clone()));
                } else {
                    for key in &ends {
                        if let EndKey::Elsewhere(id, Some(shape)) = key {
                            elsewhere
                                .entry(*id)
                                .or_insert_with(|| shape_geometry(libraries, shape));
                        }
                    }
                    stale.push((rank, element, ends));
                }
            }
            let shape_of = |id: ElementId| -> Option<&ShapeGeometry> {
                match geometry.get(&id).map(|(_, g)| g.as_ref()) {
                    Some(Geometry::Shape(s)) => Some(s),
                    _ => elsewhere.get(&id),
                }
            };
            for (rank, element, ends) in stale {
                let c = element.as_connector().expect("a connector");
                let source = End::resolve(&c.source, shape_of);
                let target = End::resolve(&c.target, shape_of);
                let (items, g) = connector_items(element.id, c, &source, &target, background);
                built.push((rank, element.id, c.clone(), ends, items, g));
            }
        }
        for (rank, id, connector, ends, items, g) in built {
            let items: Arc<[DisplayItem]> = items.into();
            let g = Arc::new(Geometry::Connector(g));
            let key = Key::Connector(Box::new(ConnectorKey {
                connector,
                ends,
                background,
            }));
            self.store(id, key, items.clone(), g.clone(), generation);
            slots[rank] = Some(items);
            geometry.insert(id, (rank, g));
            changed.push(id);
        }
        self.rebuilt = changed.len();

        // Groups cover their descendants: children come after parents in
        // paint order, so a reverse pass sees every child first.
        let mut covered: IdMap<ElementId, Rect> = IdMap::default();
        for (rank, element) in order.iter().enumerate().rev() {
            let own = if element.is_group() {
                covered.get(&element.id).copied()
            } else {
                geometry.get(&element.id).map(|(_, g)| g.bounds())
            };
            if let (Some(b), Parent::Element(parent)) = (own, element.parent) {
                covered
                    .entry(parent)
                    .and_modify(|r| *r = r.union(b))
                    .or_insert(b);
            }
            if element.is_group()
                && let Some(bounds) = own
            {
                geometry.insert(element.id, (rank, Arc::new(Geometry::Group { bounds })));
            }
        }

        let mut removed = Vec::new();
        self.entries.retain(|id, e| {
            let keep = e.generation == generation;
            if !keep {
                removed.push(*id);
            }
            keep
        });
        self.update_index(page, &order, &geometry, &changed, &removed);
        Scene {
            list: DisplayList {
                groups: slots.into_iter().flatten().collect(),
                background: Some(background),
            },
            order: order.iter().map(|e| e.id).collect(),
            geometry,
            index: self.index.clone(),
        }
    }

    fn store(
        &mut self,
        id: ElementId,
        key: Key,
        items: Arc<[DisplayItem]>,
        geometry: Arc<Geometry>,
        generation: u64,
    ) {
        self.next_version += 1;
        self.entries.insert(
            id,
            Entry {
                key,
                items,
                geometry,
                version: self.next_version,
                generation,
            },
        );
    }

    /// Brings the spatial index in line with this build: in place for the
    /// elements that were rebuilt or removed, by bulk loading when many
    /// changed or the page is different.
    fn update_index(
        &mut self,
        page: PageId,
        order: &[&Element],
        geometry: &IdMap<ElementId, (usize, Arc<Geometry>)>,
        changed: &[ElementId],
        removed: &[ElementId],
    ) {
        let bounds_of = |id: &ElementId| match geometry.get(id) {
            Some((_, g)) if matches!(**g, Geometry::Shape(_)) => {
                let Geometry::Shape(shape) = g.as_ref() else {
                    unreachable!()
                };
                let half = shape.style.stroke_width / 2.0;
                let bounds = shape.bounds.inflate(half, half);
                Some(shape.label.map_or(bounds, |label| bounds.union(label)))
            }
            Some((_, g)) if !matches!(**g, Geometry::Group { .. }) => Some(g.bounds()),
            _ => None,
        };
        let few = changed.len() + removed.len() <= (order.len() / 4).max(64);
        if self.indexed_page == Some(page) && few {
            let index = Arc::make_mut(&mut self.index);
            for id in changed.iter().chain(removed) {
                let new = bounds_of(id);
                let old = self.indexed.get(id).copied();
                if new == old {
                    continue;
                }
                if let Some(old) = old {
                    index.remove(old, *id);
                    self.indexed.remove(id);
                }
                if let Some(new) = new {
                    index.insert(new, *id);
                    self.indexed.insert(*id, new);
                }
            }
        } else {
            self.indexed = order
                .iter()
                .filter_map(|e| Some((e.id, bounds_of(&e.id)?)))
                .collect();
            self.index = Arc::new(SpatialIndex::new(
                self.indexed.iter().map(|(id, r)| (*r, *id)),
            ));
            self.indexed_page = Some(page);
        }
    }
}

#[cfg(test)]
mod tests;
