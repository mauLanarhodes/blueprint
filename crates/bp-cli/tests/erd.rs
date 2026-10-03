//! First Phase 2 milestone: build and edit a smart-table ERD through the
//! app's commands, then round-trip native files and export with the CLI.

use bp_commands::{ColumnProp, Command, History, Prop, edit};
use bp_model::kurbo::{Point, Rect};
use bp_model::{
    ColumnId, Dash, Document, Element, ElementId, Endpoint, ErdColumn, Marker, OrderKey, PageId,
    Parent, PortId, ShapeRef, SqlDialect, TableDisplay,
};
use bp_scene::{Primitive, Scene, SceneCache};
use std::path::Path;
use std::process::Command as Process;

struct Diagram {
    doc: Document,
    history: History,
    page: PageId,
    customers: ElementId,
    orders: ElementId,
    customer_id: ColumnId,
    order_customer_id: ColumnId,
    order_status: ColumnId,
    relationship: ElementId,
}

impl Diagram {
    fn new() -> Self {
        let mut doc = Document::new();
        let page = doc.first_page().unwrap();
        let parent = Parent::Layer(doc.layers_of(page)[0].id);
        let mut history = History::new();
        let mut table = |name: &str, bounds: Rect| {
            let mut element = Element::shape(
                ShapeRef::new("erd", "table"),
                parent,
                doc.next_order_key(parent),
                bounds,
            );
            element.as_shape_mut().unwrap().text = name.into();
            let id = element.id;
            let column = element.as_shape().unwrap().erd.as_ref().unwrap().columns[0].id;
            history
                .apply(
                    &mut doc,
                    "Add table",
                    vec![Command::Insert(Box::new(element))],
                )
                .unwrap();
            (id, column)
        };
        let (customers, customer_id) = table("customers", Rect::new(20.0, 20.0, 420.0, 200.0));
        let (orders, _) = table("orders", Rect::new(620.0, 20.0, 1020.0, 220.0));

        let mut email =
            ErdColumn::new("email", "VARCHAR(255)", OrderKey::after(&OrderKey::first()));
        email.unique = true;
        email.nullable = false;
        let status_order = OrderKey::after(&OrderKey::first());
        let mut status = ErdColumn::new("status", "TEXT", status_order.clone());
        status.default_value = Some("'pending'".into());
        let order_status = status.id;
        let mut foreign_key =
            ErdColumn::new("customer_id", "BIGINT", OrderKey::after(&status_order));
        foreign_key.foreign_key = true;
        foreign_key.nullable = false;
        let order_customer_id = foreign_key.id;
        history
            .apply(
                &mut doc,
                "Add columns",
                vec![
                    Command::InsertColumn {
                        id: customers,
                        column: Box::new(email),
                    },
                    Command::InsertColumn {
                        id: orders,
                        column: Box::new(status),
                    },
                    Command::InsertColumn {
                        id: orders,
                        column: Box::new(foreign_key),
                    },
                    Command::Set {
                        id: orders,
                        prop: Prop::SqlDialect(SqlDialect::MySql),
                    },
                ],
            )
            .unwrap();

        let mut relationship = Element::connector(
            column_endpoint(customers, customer_id, false),
            column_endpoint(orders, order_customer_id, true),
            parent,
            doc.next_order_key(parent),
        );
        let connector = relationship.as_connector_mut().unwrap();
        connector.start_marker = Marker::ExactlyOne;
        connector.end_marker = Marker::ZeroOrMany;
        connector.text = "places".into();
        connector.style.dash = Some(Dash::Dashed);
        let relationship_id = relationship.id;
        history
            .apply(
                &mut doc,
                "Add relationship",
                vec![Command::Insert(Box::new(relationship))],
            )
            .unwrap();
        assert_eq!(doc.validate(), Ok(()));
        Self {
            doc,
            history,
            page,
            customers,
            orders,
            customer_id,
            order_customer_id,
            order_status,
            relationship: relationship_id,
        }
    }

    fn run(&mut self, label: &str, commands: Vec<Command>) {
        self.history.apply(&mut self.doc, label, commands).unwrap();
        assert_eq!(self.doc.validate(), Ok(()), "after {label}");
    }
}

fn column_endpoint(table: ElementId, column: ColumnId, left: bool) -> Endpoint {
    Endpoint::Glued {
        element: table,
        port: Some(PortId::column(column, left)),
    }
}

fn port(scene: &Scene, table: ElementId, column: ColumnId, left: bool) -> Point {
    scene
        .shape(table)
        .unwrap()
        .ports
        .iter()
        .find(|port| port.id == PortId::column(column, left))
        .unwrap()
        .at
}

fn texts(scene: &Scene) -> Vec<String> {
    scene
        .list
        .items()
        .filter_map(|item| match &item.primitive {
            Primitive::Text(run) => Some(run.lines.iter().map(|line| line.text.clone())),
            _ => None,
        })
        .flatten()
        .collect()
}

fn cli(args: &[&str]) -> String {
    let output = Process::new(env!("CARGO_BIN_EXE_blueprint-cli"))
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn path_str(path: &Path) -> &str {
    path.to_str().unwrap()
}

#[test]
fn table_edits_refresh_column_routes_and_undo_together() {
    let mut diagram = Diagram::new();
    let mut cache = SceneCache::default();
    let libraries = bp_shapes::Libraries::builtin();
    let scene = cache.build(&diagram.doc, diagram.page, libraries);
    let before = diagram.doc.clone();
    let before_port = port(&scene, diagram.orders, diagram.order_customer_id, true);
    assert_eq!(
        scene.connector(diagram.relationship).unwrap().points.last(),
        Some(&before_port)
    );

    let status_order = diagram.doc.elements[&diagram.orders]
        .as_shape()
        .unwrap()
        .erd
        .as_ref()
        .unwrap()
        .columns
        .iter()
        .find(|column| column.id == diagram.order_status)
        .unwrap()
        .order
        .clone();
    diagram.run(
        "Rename and reorder foreign key",
        vec![
            Command::SetColumn {
                id: diagram.orders,
                column: diagram.order_customer_id,
                prop: ColumnProp::Name("buyer_id".into()),
            },
            Command::SetColumn {
                id: diagram.orders,
                column: diagram.order_customer_id,
                prop: ColumnProp::Order(OrderKey::before(&status_order)),
            },
        ],
    );
    let scene = cache.build(&diagram.doc, diagram.page, libraries);
    let reordered_port = port(&scene, diagram.orders, diagram.order_customer_id, true);
    assert!(reordered_port.y < before_port.y, "row moved above status");
    assert_eq!(
        scene.connector(diagram.relationship).unwrap().points.last(),
        Some(&reordered_port),
        "cached relationship follows the same column id"
    );
    let lines = texts(&scene);
    assert!(lines.iter().any(|line| line == "buyer_id"));
    assert!(!lines.iter().any(|line| line == "customer_id"));
    assert!(diagram.history.undo(&mut diagram.doc));
    assert_eq!(diagram.doc, before);
    let scene = cache.build(&diagram.doc, diagram.page, libraries);
    assert_eq!(
        scene.connector(diagram.relationship).unwrap().points.last(),
        Some(&before_port)
    );

    diagram.run(
        "Collapse orders",
        vec![Command::Set {
            id: diagram.orders,
            prop: Prop::TableDisplay(TableDisplay::Collapsed),
        }],
    );
    let scene = cache.build(&diagram.doc, diagram.page, libraries);
    let collapsed_port = port(&scene, diagram.orders, diagram.order_customer_id, true);
    assert!(collapsed_port.y < before_port.y);
    assert_eq!(
        scene.connector(diagram.relationship).unwrap().points.last(),
        Some(&collapsed_port)
    );
    let lines = texts(&scene);
    assert!(lines.iter().any(|line| line == "orders"));
    assert!(!lines.iter().any(|line| line == "customer_id"));
    assert!(diagram.history.undo(&mut diagram.doc));
    assert_eq!(diagram.doc, before);

    diagram.run(
        "Show order keys",
        vec![Command::Set {
            id: diagram.orders,
            prop: Prop::TableDisplay(TableDisplay::KeysOnly),
        }],
    );
    let scene = cache.build(&diagram.doc, diagram.page, libraries);
    let lines = texts(&scene);
    assert!(lines.iter().any(|line| line == "customer_id"));
    assert!(!lines.iter().any(|line| line == "status"));
    assert_eq!(
        scene.connector(diagram.relationship).unwrap().points.last(),
        Some(&port(
            &scene,
            diagram.orders,
            diagram.order_customer_id,
            true
        ))
    );
    assert!(diagram.history.undo(&mut diagram.doc));
    assert_eq!(diagram.doc, before);

    let commands = edit::remove_column(&diagram.doc, diagram.orders, diagram.order_customer_id);
    diagram.run("Delete foreign key column", commands);
    assert!(!diagram.doc.elements.contains_key(&diagram.relationship));
    let scene = cache.build(&diagram.doc, diagram.page, libraries);
    assert!(scene.connector(diagram.relationship).is_none());
    assert!(diagram.history.undo(&mut diagram.doc));
    assert_eq!(
        diagram.doc, before,
        "undo restores the column and relationship"
    );
    let scene = cache.build(&diagram.doc, diagram.page, libraries);
    assert_eq!(
        scene.connector(diagram.relationship).unwrap().points.last(),
        Some(&before_port)
    );
}

#[test]
fn erd_save_reopen_and_cli_svg_preserve_columns_and_cardinalities() {
    let mut diagram = Diagram::new();
    let parent = diagram.doc.elements[&diagram.customers].parent;
    // Include every Crow's Foot end in an export so the headless path
    // cannot silently lose a cardinality implemented by the canvas.
    let mut markers = Vec::new();
    for (index, marker) in [
        Marker::ExactlyOne,
        Marker::ZeroOrOne,
        Marker::OneOrMany,
        Marker::ZeroOrMany,
        Marker::Many,
    ]
    .into_iter()
    .enumerate()
    {
        let y = 300.0 + index as f64 * 45.0;
        let mut element = Element::connector(
            Endpoint::Free(Point::new(50.0, y)),
            Endpoint::Free(Point::new(380.0, y)),
            parent,
            diagram.doc.next_order_key(parent),
        );
        let connector = element.as_connector_mut().unwrap();
        connector.end_marker = marker;
        connector.text = marker.label().into();
        markers.push(element.id);
        diagram.run(
            "Add cardinality example",
            vec![Command::Insert(Box::new(element))],
        );
    }

    let directory = std::env::temp_dir().join(format!("bp-erd-gate-{}", ElementId::new()));
    std::fs::create_dir_all(&directory).unwrap();
    let native = directory.join("shop.blueprint");
    let json = directory.join("shop.blueprint.json");
    bp_io::save(&diagram.doc, &native).unwrap();
    bp_io::save(&diagram.doc, &json).unwrap();
    for path in [&native, &json] {
        let reopened = bp_io::load(path).unwrap();
        assert_eq!(
            reopened,
            diagram.doc,
            "native round trip of {}",
            path.display()
        );
        assert_eq!(reopened.validate(), Ok(()));
        assert_eq!(
            reopened.elements[&diagram.orders]
                .as_shape()
                .unwrap()
                .erd
                .as_ref()
                .unwrap()
                .dialect,
            SqlDialect::MySql,
            "the selected dialect survives the native round trip"
        );
        assert_eq!(
            reopened.elements[&diagram.relationship]
                .as_connector()
                .unwrap()
                .source,
            column_endpoint(diagram.customers, diagram.customer_id, false)
        );
    }
    let saved_json = std::fs::read_to_string(&json).unwrap();
    assert!(saved_json.contains("\"schema_version\": 3"));
    assert!(saved_json.contains(&diagram.order_customer_id.to_string()));
    assert!(!saved_json.contains("\"points\"") && !saved_json.contains("\"lines\""));

    let output = directory.join("shop.svg");
    cli(&["export", path_str(&native), path_str(&output)]);
    let svg = std::fs::read_to_string(&output).unwrap();
    let scene = bp_scene::build_page(&diagram.doc, diagram.page);
    let lines = texts(&scene);
    for expected in ["customers", "orders", "email", "customer_id", "places"] {
        assert!(lines.iter().any(|line| line == expected), "{lines:?}");
        assert!(
            svg.contains(&format!(">{expected}</text>")),
            "missing {expected}"
        );
    }
    for badge in ["PK", "FK", "UK", "NOT NULL"] {
        assert!(
            lines.iter().any(|line| line.contains(badge)),
            "missing {badge}"
        );
        assert!(svg.contains(badge), "missing exported {badge}");
    }
    assert!(svg.contains("VARCHAR(255)") && svg.contains("BIGINT"));
    assert!(lines.iter().any(|line| line.contains("pending")));
    assert!(svg.contains("pending"));
    assert!(
        svg.contains("stroke-dasharray"),
        "non-identifying relationship"
    );
    assert_eq!(
        scene.connector(diagram.relationship).unwrap().points[0],
        port(&scene, diagram.customers, diagram.customer_id, false)
    );
    assert_eq!(
        scene.connector(diagram.relationship).unwrap().points.last(),
        Some(&port(
            &scene,
            diagram.orders,
            diagram.order_customer_id,
            true
        ))
    );

    assert_eq!(
        svg,
        bp_export::page_to_svg(
            &diagram.doc,
            diagram.page,
            &bp_export::SvgOptions::default()
        ),
        "the real CLI exports the complete scene, including cardinality paths"
    );
    // Each cardinality adds visible geometry beyond the route, and all
    // five must remain distinct. Compare paths rather than marker labels.
    let mut cardinality_paths = Vec::new();
    for id in markers {
        let paths: Vec<_> = scene
            .list
            .items()
            .filter(|item| item.element == id)
            .filter_map(|item| match &item.primitive {
                Primitive::Path { path, .. } => Some(path.clone()),
                _ => None,
            })
            .collect();
        assert!(paths.len() >= 2, "a route and a visible cardinality marker");
        cardinality_paths.push(paths);
    }
    // Normalize the examples to the same row before comparing shapes.
    for (index, paths) in cardinality_paths.iter_mut().enumerate() {
        for path in paths {
            *path =
                bp_model::kurbo::Affine::translate((0.0, -(index as f64) * 45.0)) * path.clone();
        }
    }
    for (index, paths) in cardinality_paths.iter().enumerate() {
        assert!(
            cardinality_paths[..index]
                .iter()
                .all(|earlier| earlier != paths)
        );
    }
    let info = cli(&["info", path_str(&native)]);
    assert!(info.contains("2 shapes, 6 connectors"), "{info}");

    if std::env::var_os("BP_KEEP_GATE_FILES").is_some() {
        println!("ERD gate files kept in {}", directory.display());
    } else {
        std::fs::remove_dir_all(directory).unwrap();
    }
}
