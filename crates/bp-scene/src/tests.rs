use super::*;
use bp_model::kurbo::Vec2;
use bp_model::{
    Endpoint, ErdColumn, Marker, OrderKey, Paint, PortId, Routing, ShapeRef, TableDisplay,
};

struct Page {
    doc: Document,
    page: PageId,
    layer: Parent,
}

impl Page {
    fn new() -> Self {
        let doc = Document::new();
        let page = doc.first_page().unwrap();
        let layer = Parent::Layer(doc.layers_of(page)[0].id);
        Self { doc, page, layer }
    }

    fn add(&mut self, el: Element) -> ElementId {
        let id = el.id;
        self.doc.elements.insert(id, el);
        id
    }

    fn shape(&mut self, name: &str, bounds: Rect, text: &str) -> ElementId {
        let order = self.doc.next_order_key(self.layer);
        let (library, shape) = name.split_once('/').unwrap();
        let mut el = Element::shape(ShapeRef::new(library, shape), self.layer, order, bounds);
        if let ElementKind::Shape(s) = &mut el.kind {
            s.text = text.into();
        }
        self.add(el)
    }

    fn connect(&mut self, source: Endpoint, target: Endpoint) -> ElementId {
        let order = self.doc.next_order_key(self.layer);
        self.add(Element::connector(source, target, self.layer, order))
    }

    fn connector_mut(&mut self, id: ElementId) -> &mut Connector {
        self.doc
            .elements
            .get_mut(&id)
            .unwrap()
            .as_connector_mut()
            .unwrap()
    }

    fn build(&self) -> Scene {
        build_page(&self.doc, self.page)
    }
}

fn texts(scene: &Scene) -> Vec<String> {
    scene
        .list
        .items()
        .filter_map(|i| match &i.primitive {
            Primitive::Text(run) => Some(
                run.lines
                    .iter()
                    .map(|l| l.text.as_str())
                    .collect::<Vec<_>>()
                    .join("|"),
            ),
            _ => None,
        })
        .collect()
}

#[test]
fn smart_tables_draw_typed_rows_and_expand_to_fit() {
    let mut p = Page::new();
    let id = p.shape("erd/table", Rect::new(10.0, 20.0, 90.0, 40.0), "customers");
    let shape = p.doc.elements.get_mut(&id).unwrap().as_shape_mut().unwrap();
    shape.style.font_size = Some(24.0);
    let table = shape.erd.as_mut().unwrap();
    let mut email = ErdColumn::new(
        "email",
        "VARCHAR(255)",
        OrderKey::before(&table.columns[0].order),
    );
    email.foreign_key = true;
    email.unique = true;
    email.nullable = false;
    email.default_value = Some("'unknown@example.com'".into());
    let email_id = email.id;
    table.columns.push(email);

    let scene = p.build();
    let g = scene.shape(id).unwrap();
    let erd = g.erd.as_ref().unwrap();
    assert_eq!(g.text_box, erd.header);
    assert_eq!(erd.rows.len(), 2);
    assert_eq!(
        erd.rows[1].column, email_id,
        "PK rows remain above other rows"
    );
    assert_eq!(
        g.bounds.height(),
        erd_header_height(24.0) + 2.0 * erd_row_height(24.0)
    );
    assert!(
        g.bounds.width() > 280.0,
        "full row metadata fits the derived width"
    );
    let content = texts(&scene);
    for value in [
        "customers",
        "id",
        "BIGINT",
        "PK NOT NULL",
        "email",
        "FK UK NOT NULL",
        "VARCHAR(255) = 'unknown@example.com'",
    ] {
        assert!(
            content.iter().any(|text| text == value),
            "missing {value}: {content:?}"
        );
    }
    for item in scene
        .list
        .items()
        .filter(|i| matches!(i.primitive, Primitive::Text(_)))
    {
        assert!(g.bounds.contains_rect(item.bbox), "{item:?}");
    }
    assert!(
        scene.list.items().any(|item| match &item.primitive {
            Primitive::Path {
                path, fill: None, ..
            } => {
                let b = path.bounding_box();
                b.y0 == erd.rows[0].bounds.y1 && b.y1 == b.y0 && b.width() == g.bounds.width()
            }
            _ => false,
        }),
        "primary key divider is drawn"
    );
}

#[test]
fn column_ports_follow_identity_through_reorder_move_and_collapse() {
    let mut p = Page::new();
    let id = p.shape("erd/table", Rect::new(0.0, 0.0, 280.0, 80.0), "orders");
    let column = {
        let table = p
            .doc
            .elements
            .get_mut(&id)
            .unwrap()
            .as_shape_mut()
            .unwrap()
            .erd
            .as_mut()
            .unwrap();
        let first = ErdColumn::new(
            "customer_id",
            "BIGINT",
            OrderKey::after(&table.columns[0].order),
        );
        let second = ErdColumn::new("created_at", "TIMESTAMP", OrderKey::after(&first.order));
        let column = second.id;
        table.columns.extend([first, second]);
        column
    };
    let endpoint = Endpoint::Glued {
        element: id,
        port: Some(PortId::column(column, false)),
    };
    let connector = p.connect(endpoint.clone(), Endpoint::Free(Point::new(800.0, 100.0)));
    let mut cache = SceneCache::default();
    let first = cache.build(&p.doc, p.page, Libraries::builtin());
    let initial = first.connector(connector).unwrap().points[0];
    let row = first
        .shape(id)
        .unwrap()
        .erd
        .as_ref()
        .unwrap()
        .rows
        .iter()
        .find(|r| r.column == column)
        .unwrap();
    assert_eq!(initial, Point::new(row.bounds.x1, row.bounds.center().y));
    assert_eq!(
        first.port_near(initial, 1.0, |_| true).unwrap().1.id,
        PortId::column(column, false)
    );
    drop(first);

    let table = p
        .doc
        .elements
        .get_mut(&id)
        .unwrap()
        .as_shape_mut()
        .unwrap()
        .erd
        .as_mut()
        .unwrap();
    table.columns[2].order = OrderKey::before(&table.columns[1].order);
    let reordered = cache.build(&p.doc, p.page, Libraries::builtin());
    assert_eq!(
        cache.rebuilt, 2,
        "table data changes invalidate the attached route"
    );
    assert_eq!(
        reordered.connector(connector).unwrap().points[0].y,
        initial.y - erd_row_height(13.0)
    );
    drop(reordered);

    let shape = p.doc.elements.get_mut(&id).unwrap().as_shape_mut().unwrap();
    shape.bounds = shape.bounds + Vec2::new(30.0, 40.0);
    shape.erd.as_mut().unwrap().display = TableDisplay::KeysOnly;
    let keys = cache.build(&p.doc, p.page, Libraries::builtin());
    let header = keys.shape(id).unwrap().erd.as_ref().unwrap().header;
    assert_eq!(keys.shape(id).unwrap().erd.as_ref().unwrap().rows.len(), 1);
    assert_eq!(
        keys.connector(connector).unwrap().points[0],
        Point::new(header.x1, header.center().y)
    );
    assert!(
        keys.port_near(Point::new(header.x1, header.center().y), 1.0, |_| true)
            .is_none(),
        "hidden columns cannot be picked for new relationships"
    );
    assert!(!texts(&keys).iter().any(|t| t == "created_at"));
    drop(keys);

    p.doc
        .elements
        .get_mut(&id)
        .unwrap()
        .as_shape_mut()
        .unwrap()
        .erd
        .as_mut()
        .unwrap()
        .display = TableDisplay::Collapsed;
    let collapsed = cache.build(&p.doc, p.page, Libraries::builtin());
    let g = collapsed.shape(id).unwrap();
    assert_eq!(g.bounds, g.text_box);
    assert!(g.erd.as_ref().unwrap().rows.is_empty());
    assert_eq!(
        g.visible_ports().count(),
        4,
        "collapsed tables expose outline ports only"
    );
    assert_eq!(texts(&collapsed), ["orders"]);
    assert_eq!(
        collapsed.connector(connector).unwrap().points[0],
        Point::new(g.bounds.x1, g.bounds.center().y)
    );
    assert_eq!(
        p.connector_mut(connector).source,
        endpoint,
        "filtering never rewrites the permanent port"
    );
}

#[test]
fn shapes_and_text_become_items() {
    let mut p = Page::new();
    p.shape(
        "basic/rectangle",
        Rect::new(0.0, 0.0, 100.0, 40.0),
        "Orders\nv2",
    );
    p.shape("basic/text", Rect::new(0.0, 60.0, 50.0, 80.0), "Note");
    let scene = p.build();
    let kinds: Vec<_> = scene
        .list
        .items()
        .map(|i| matches!(i.primitive, Primitive::Path { .. }))
        .collect();
    // Rectangle outline + its text, then the text shape (no outline).
    assert_eq!(kinds, [true, false, false]);
    assert_eq!(texts(&scene), ["Orders|v2", "Note"]);
    assert_eq!(scene.order.len(), 2);
}

#[test]
fn text_wraps_inside_the_text_area_and_centres() {
    let mut p = Page::new();
    let id = p.shape(
        "flowchart/decision",
        Rect::new(0.0, 0.0, 160.0, 100.0),
        "Is the customer signed in?",
    );
    let scene = p.build();
    let area = scene.shape(id).unwrap().text_box;
    let run = scene
        .list
        .items()
        .find_map(|i| match &i.primitive {
            Primitive::Text(r) => Some(r.clone()),
            _ => None,
        })
        .unwrap();
    assert!(run.lines.len() >= 2, "wrapped: {:?}", run.lines);
    for line in &run.lines {
        assert!(line.width <= area.width(), "{line:?} wider than {area:?}");
        assert_eq!(line.x, area.center().x, "centred");
    }
    // The block is centred vertically on the shape.
    let top = run.lines[0].baseline;
    let bottom = run.lines.last().unwrap().baseline;
    assert!(
        ((top + bottom) / 2.0 - 50.0).abs() < run.size,
        "{top} {bottom}"
    );
}

#[test]
fn glued_connectors_end_at_ports_and_follow_shapes() {
    let mut p = Page::new();
    let a = p.shape("flowchart/process", Rect::new(0.0, 0.0, 120.0, 60.0), "A");
    let b = p.shape("flowchart/process", Rect::new(300.0, 0.0, 420.0, 60.0), "B");
    let c = p.connect(Endpoint::glued(a, Some("e")), Endpoint::glued(b, Some("w")));
    let scene = p.build();
    let g = scene.connector(c).unwrap();
    assert_eq!(
        g.points,
        vec![Point::new(120.0, 30.0), Point::new(300.0, 30.0)]
    );

    // Move B down: the route bends and still ends at B's west port.
    if let ElementKind::Shape(s) = &mut p.doc.elements.get_mut(&b).unwrap().kind {
        s.bounds = s.bounds + Vec2::new(0.0, 200.0);
    }
    let g = p.build().connector(c).unwrap().clone();
    assert_eq!(g.points.first(), Some(&Point::new(120.0, 30.0)));
    assert_eq!(g.points.last(), Some(&Point::new(300.0, 230.0)));
    assert_eq!(g.points.len(), 4, "a Z between the shapes: {:?}", g.points);
}

#[test]
fn floating_connectors_choose_facing_sides() {
    let mut p = Page::new();
    let a = p.shape("basic/rectangle", Rect::new(0.0, 0.0, 100.0, 50.0), "");
    let b = p.shape("basic/rectangle", Rect::new(0.0, 200.0, 100.0, 250.0), "");
    let c = p.connect(Endpoint::glued(a, None), Endpoint::glued(b, None));
    for routing in Routing::ALL {
        p.connector_mut(c).routing = routing;
        let g = p.build().connector(c).unwrap().clone();
        let (first, last) = (g.points[0], *g.points.last().unwrap());
        assert!(
            (first - Point::new(50.0, 50.0)).hypot() < 1e-6,
            "{routing:?} {first:?}"
        );
        assert!(
            (last - Point::new(50.0, 200.0)).hypot() < 1e-6,
            "{routing:?} {last:?}"
        );
    }
}

#[test]
fn straight_connectors_to_floating_ends_meet_the_outline() {
    let mut p = Page::new();
    let a = p.shape("basic/ellipse", Rect::new(0.0, 0.0, 100.0, 100.0), "");
    let c = p.connect(
        Endpoint::glued(a, None),
        Endpoint::Free(Point::new(300.0, 300.0)),
    );
    p.connector_mut(c).routing = Routing::Straight;
    let g = p.build().connector(c).unwrap().clone();
    // On the circle, along the diagonal towards the free end.
    let start = g.points[0];
    assert!(
        ((start - Point::new(50.0, 50.0)).hypot() - 50.0).abs() < 0.1,
        "{start:?}"
    );
    assert!((start.x - start.y).abs() < 0.1);
}

#[test]
fn markers_shorten_the_line() {
    let mut p = Page::new();
    let c = p.connect(
        Endpoint::Free(Point::new(0.0, 0.0)),
        Endpoint::Free(Point::new(200.0, 0.0)),
    );
    p.connector_mut(c).start_marker = Marker::Circle;
    let scene = p.build();
    let paths: Vec<&BezPath> = scene
        .list
        .items()
        .filter_map(|i| match &i.primitive {
            Primitive::Path { path, .. } => Some(path),
            _ => None,
        })
        .collect();
    assert_eq!(paths.len(), 3, "line, start circle, end arrow");
    let line = paths[0].bounding_box();
    assert!(
        line.x0 > 0.0 && line.x1 < 200.0,
        "trimmed at both ends: {line:?}"
    );
    let arrow = paths[2].bounding_box();
    assert!((arrow.x1 - 200.0).abs() < 1e-6, "arrow tip at the end");
}

#[test]
fn thick_shape_strokes_can_be_selected_at_their_outer_edge() {
    let mut p = Page::new();
    let id = p.shape("basic/rectangle", Rect::new(0.0, 0.0, 100.0, 50.0), "");
    p.doc
        .elements
        .get_mut(&id)
        .unwrap()
        .as_shape_mut()
        .unwrap()
        .style
        .stroke_width = Some(40.0);
    let scene = p.build();
    let point = Point::new(-18.0, 25.0);
    assert!(scene.hits(id, point, 0.0));
    assert_eq!(scene.hit(point, 0.0, |_| true), Some(id));
}

#[test]
fn labels_sit_on_the_route_with_a_background() {
    let mut p = Page::new();
    let c = p.connect(
        Endpoint::Free(Point::new(0.0, 0.0)),
        Endpoint::Free(Point::new(0.0, 100.0)),
    );
    p.connector_mut(c).text = "Yes".into();
    p.connector_mut(c).label_position = 0.25;
    let scene = p.build();
    let g = scene.connector(c).unwrap();
    let label = g.label.unwrap();
    assert!(label.contains(Point::new(0.0, 25.0)), "{label:?}");
    assert_eq!(texts(&scene), ["Yes"]);
    // Clicking the label hits the connector.
    assert_eq!(
        scene.hit(Point::new(label.x0 + 1.0, 25.0), 0.0, |_| true),
        Some(c)
    );
}

#[test]
fn hit_testing_prefers_the_topmost_and_respects_outlines() {
    let mut p = Page::new();
    let below = p.shape("basic/rectangle", Rect::new(0.0, 0.0, 100.0, 100.0), "");
    let above = p.shape("basic/ellipse", Rect::new(50.0, 50.0, 150.0, 150.0), "");
    let scene = p.build();
    assert_eq!(
        scene.hit(Point::new(100.0, 100.0), 0.0, |_| true),
        Some(above)
    );
    // Inside the ellipse's box but outside the ellipse: the rectangle.
    assert_eq!(
        scene.hit(Point::new(55.0, 55.0), 0.0, |_| true),
        Some(below)
    );
    assert_eq!(
        scene.hit(Point::new(100.0, 100.0), 0.0, |id| id != above),
        Some(below)
    );
    assert_eq!(scene.hit(Point::new(500.0, 500.0), 0.0, |_| true), None);
    assert_eq!(
        scene.enclosed(Rect::new(-1.0, -1.0, 101.0, 101.0)),
        vec![below]
    );
}

#[test]
fn connectors_are_hit_along_their_route() {
    let mut p = Page::new();
    let c = p.connect(
        Endpoint::Free(Point::new(0.0, 0.0)),
        Endpoint::Free(Point::new(100.0, 100.0)),
    );
    let scene = p.build();
    let g = scene.connector(c).unwrap();
    let corner = g.points[1];
    assert_eq!(
        scene.hit(corner + Vec2::new(0.5, 0.5), 2.0, |_| true),
        Some(c)
    );
    // Inside the elbow, away from both legs.
    assert_eq!(scene.hit(Point::new(80.0, 20.0), 2.0, |_| true), None);
}

#[test]
fn groups_cover_their_children_and_hidden_layers_vanish() {
    let mut p = Page::new();
    let layer = p.layer;
    let group = Element::group(layer, OrderKey::first());
    let g = p.add(group);
    let saved = p.layer;
    p.layer = Parent::Element(g);
    p.shape("basic/rectangle", Rect::new(0.0, 0.0, 10.0, 10.0), "");
    p.shape("basic/rectangle", Rect::new(50.0, 50.0, 60.0, 70.0), "");
    p.layer = saved;
    let scene = p.build();
    assert_eq!(scene.bounds_of(g), Some(Rect::new(0.0, 0.0, 60.0, 70.0)));
    // Groups are never hit themselves; their children are.
    assert_ne!(scene.hit(Point::new(5.0, 5.0), 0.0, |_| true), Some(g));

    let Parent::Layer(layer_id) = layer else {
        unreachable!()
    };
    p.doc.layers.get_mut(&layer_id).unwrap().visible = false;
    let scene = p.build();
    assert!(scene.order.is_empty() && scene.bounds().is_none());
}

#[test]
fn styles_resolve_against_shape_defaults() {
    let mut p = Page::new();
    let note = p.shape(
        "basic/sticky-note",
        Rect::new(0.0, 0.0, 160.0, 120.0),
        "Remember",
    );
    let plain = p.shape("basic/rectangle", Rect::new(200.0, 0.0, 300.0, 50.0), "");
    if let ElementKind::Shape(s) = &mut p.doc.elements.get_mut(&plain).unwrap().kind {
        s.style.fill = Some(Paint::None);
        s.style.opacity = Some(0.5);
    }
    let scene = p.build();
    let note_style = &scene.shape(note).unwrap().style;
    assert!(note_style.shadow && note_style.text_align == TextAlign::Left);
    let paths: Vec<_> = scene
        .list
        .items()
        .filter(|i| i.element == plain)
        .filter_map(|i| match &i.primitive {
            Primitive::Path { fill, stroke, .. } => Some((*fill, *stroke)),
            _ => None,
        })
        .collect();
    assert_eq!(paths.len(), 1);
    assert_eq!(paths[0].0, None);
    assert_eq!(paths[0].1.unwrap().color.a, 128, "opacity fades the stroke");
    // Sticky notes get a shadow item under the outline.
    let note_paths = scene.list.items().filter(|i| i.element == note).count();
    assert_eq!(note_paths, 4, "shadow, outline, fold, text");
}

#[test]
fn unknown_shapes_still_draw() {
    let mut p = Page::new();
    p.shape("future/widget", Rect::new(0.0, 0.0, 50.0, 50.0), "?");
    let scene = p.build();
    let dashed = scene.list.items().any(
        |i| matches!(&i.primitive, Primitive::Path { stroke: Some(s), .. } if s.dash.is_some()),
    );
    assert!(dashed);
}

#[test]
fn cache_rebuilds_only_what_changed() {
    let mut p = Page::new();
    let a = p.shape("basic/rectangle", Rect::new(0.0, 0.0, 100.0, 50.0), "A");
    let b = p.shape("basic/rectangle", Rect::new(300.0, 0.0, 400.0, 50.0), "B");
    let lone = p.shape("basic/ellipse", Rect::new(0.0, 300.0, 100.0, 350.0), "C");
    let c = p.connect(Endpoint::glued(a, None), Endpoint::glued(b, None));
    let mut cache = SceneCache::default();
    let libs = Libraries::builtin();
    let first = cache.build(&p.doc, p.page, libs);
    assert_eq!(cache.rebuilt, 4);
    let again = cache.build(&p.doc, p.page, libs);
    assert_eq!(cache.rebuilt, 0);
    assert!(Arc::ptr_eq(&first.list.groups[0], &again.list.groups[0]));

    // Moving A rebuilds A and the connector glued to it, nothing else.
    if let ElementKind::Shape(s) = &mut p.doc.elements.get_mut(&a).unwrap().kind {
        s.bounds = s.bounds + Vec2::new(0.0, 100.0);
    }
    let moved = cache.build(&p.doc, p.page, libs);
    assert_eq!(cache.rebuilt, 2);
    assert_ne!(first.connector(c), moved.connector(c));
    assert_eq!(first.shape(lone), moved.shape(lone));
    // The cached build matches a fresh one exactly.
    let fresh = build_page(&p.doc, p.page);
    let items = |s: &Scene| s.list.items().cloned().collect::<Vec<_>>();
    assert_eq!(items(&moved), items(&fresh));
}

fn cloud_icon() -> bp_model::CloudIcon {
    bp_model::CloudIcon {
        reference: ShapeRef::new("aws", "sample@v1"),
        name: "Sample service".into(),
        provider: bp_model::CloudProvider::Aws,
        category: "Compute".into(),
        kind: bp_model::IconKind::Service,
        pack_version: "v1".into(),
        source_path: "Compute/Sample.svg".into(),
        svg: Arc::from(
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 10"><path fill="#ff8000" d="M0 0h20v10H0z"/></svg>"##,
        ),
    }
}

#[test]
fn cloud_icons_keep_artwork_and_have_labels_and_four_ports() {
    let mut p = Page::new();
    let asset = cloud_icon();
    p.doc.icons.insert(asset.reference.clone(), asset.clone());
    let id = p.shape(
        "aws/sample@v1",
        Rect::new(20.0, 30.0, 116.0, 126.0),
        &asset.name,
    );
    let shape = p.doc.elements.get_mut(&id).unwrap().as_shape_mut().unwrap();
    // Diagram colour controls cannot recolour provider SVGs.
    shape.style.fill = Some(Paint::Color(Color::BLACK));
    shape.style.stroke = Some(Paint::Color(Color::BLACK));
    shape.style.opacity = Some(0.5);
    let connector = p.connect(
        Endpoint::glued(id, Some("e")),
        Endpoint::Free(Point::new(250.0, 78.0)),
    );
    let scene = p.build();
    let geometry = scene.shape(id).unwrap();
    assert_eq!(geometry.bounds, Rect::new(20.0, 30.0, 116.0, 126.0));
    assert_eq!(geometry.ports.len(), 4);
    assert_eq!(
        geometry
            .ports
            .iter()
            .find(|port| port.id.as_str() == "e")
            .unwrap()
            .at,
        Point::new(116.0, 78.0)
    );
    assert_eq!(
        scene.connector(connector).unwrap().points[0],
        Point::new(116.0, 78.0)
    );
    assert!(geometry.text_box.y0 > geometry.bounds.y1);
    assert_eq!(
        scene.hit(geometry.text_box.center(), 1.0, |_| true),
        Some(id)
    );
    assert!(scene.bounds().unwrap().y1 > geometry.bounds.y1);
    assert!(scene.list.items().any(|item| matches!(&item.primitive,
        Primitive::Icon { svg, bounds, opacity } if svg == &asset.svg && *bounds == geometry.bounds && *opacity == 0.5)));
    assert_eq!(texts(&scene), vec![asset.name]);
    assert!(
        !scene
            .list
            .items()
            .any(|item| item.element == id && matches!(&item.primitive, Primitive::Path { .. }))
    );
}

#[test]
fn cloud_asset_changes_invalidate_scene_cache_without_shape_changes() {
    let mut p = Page::new();
    let id = p.shape(
        "aws/sample@v1",
        Rect::new(0.0, 0.0, 96.0, 96.0),
        "Sample service",
    );
    let mut cache = SceneCache::default();
    let libraries = Libraries::builtin();
    let missing = cache.build(&p.doc, p.page, libraries);
    assert!(
        !missing
            .list
            .items()
            .any(|item| matches!(item.primitive, Primitive::Icon { .. }))
    );
    let mut asset = cloud_icon();
    p.doc.icons.insert(asset.reference.clone(), asset.clone());
    let installed = cache.build(&p.doc, p.page, libraries);
    assert_eq!(cache.rebuilt, 1);
    assert!(
        installed
            .list
            .items()
            .any(|item| matches!(item.primitive, Primitive::Icon { .. }))
    );
    let unchanged = cache.build(&p.doc, p.page, libraries);
    assert_eq!(cache.rebuilt, 0);
    assert!(Arc::ptr_eq(
        &installed.list.groups[0],
        &unchanged.list.groups[0]
    ));
    asset.svg = Arc::from(asset.svg.replace("#ff8000", "#0080ff"));
    p.doc.icons.insert(asset.reference.clone(), asset.clone());
    let replaced = cache.build(&p.doc, p.page, libraries);
    assert_eq!(cache.rebuilt, 1);
    assert_ne!(installed.list.groups[0], replaced.list.groups[0]);
    assert!(replaced.list.items().any(|item| item.element == id
        && matches!(&item.primitive,
        Primitive::Icon { svg, .. } if svg == &asset.svg)));
    p.doc.icons.clear();
    let removed = cache.build(&p.doc, p.page, libraries);
    assert_eq!(cache.rebuilt, 1);
    assert_eq!(
        missing.list.items().collect::<Vec<_>>(),
        removed.list.items().collect::<Vec<_>>()
    );
}
