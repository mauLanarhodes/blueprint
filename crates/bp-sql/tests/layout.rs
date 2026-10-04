use bp_commands::{Command, History, Prop};
use bp_model::kurbo::{Point, Rect};
use bp_model::{Document, ElementId, Layer, OrderKey, Parent, Routing, SqlDialect, TableDisplay};
use bp_scene::shape_geometry;
use bp_shapes::Libraries;
use bp_sql::{arrange_page, export_page, import_commands, parse};
use std::collections::BTreeMap;

const SCHEMA: &str = "
    CREATE TABLE child (id integer PRIMARY KEY, parent_id integer NOT NULL);
    CREATE TABLE parent (id integer PRIMARY KEY, title text DEFAULT 'a long default value');
    CREATE TABLE sibling (id integer PRIMARY KEY, parent_id integer);
    CREATE TABLE unrelated (id integer PRIMARY KEY);
    ALTER TABLE child ADD FOREIGN KEY (parent_id) REFERENCES parent(id);
    ALTER TABLE sibling ADD FOREIGN KEY (parent_id) REFERENCES parent(id);
";

fn imported(sql: &str) -> (Document, History, Parent) {
    let mut doc = Document::new();
    let parent = Parent::Layer(doc.layers_of(doc.first_page().unwrap())[0].id);
    let preview = parse(sql, SqlDialect::PostgreSql).unwrap();
    assert!(preview.warnings.is_empty(), "{:?}", preview.warnings);
    let (_, commands) = import_commands(&doc, parent, &preview).unwrap();
    let mut history = History::new();
    history.apply(&mut doc, "Import SQL", commands).unwrap();
    (doc, history, parent)
}

fn id(doc: &Document, name: &str) -> ElementId {
    doc.elements
        .values()
        .find(|element| element.as_shape().is_some_and(|shape| shape.text == name))
        .unwrap()
        .id
}

fn bounds(doc: &Document) -> BTreeMap<String, Rect> {
    doc.elements
        .values()
        .filter_map(|element| {
            let shape = element.as_shape()?;
            shape.erd.as_ref()?;
            Some((
                shape.text.clone(),
                shape_geometry(Libraries::builtin(), shape).bounds,
            ))
        })
        .collect()
}

fn assert_non_overlapping(doc: &Document) {
    let boxes = bounds(doc);
    for (i, (left_name, left)) in boxes.iter().enumerate() {
        for (right_name, right) in boxes.iter().skip(i + 1) {
            assert!(
                left.x1 <= right.x0
                    || right.x1 <= left.x0
                    || left.y1 <= right.y0
                    || right.y1 <= left.y0,
                "{left_name} overlaps {right_name}: {left:?}, {right:?}"
            );
        }
    }
}

#[test]
fn imports_use_dependency_layers_and_all_rendered_rows_without_overlap() {
    let (doc, _, _) = imported(SCHEMA);
    let positions = bounds(&doc);
    assert!(positions["parent"].x1 + 159.0 <= positions["child"].x0);
    assert!(positions["parent"].x1 + 159.0 <= positions["sibling"].x0);
    assert_non_overlapping(&doc);
    for element in doc.elements.values() {
        if let Some(shape) = element.as_shape() {
            let table = shape.erd.as_ref().unwrap();
            let geometry = shape_geometry(Libraries::builtin(), shape);
            assert_eq!(table.display, TableDisplay::All);
            assert_eq!(geometry.erd.unwrap().rows.len(), table.columns.len());
            assert_eq!(shape.bounds, geometry.bounds);
        }
    }
}

#[test]
fn placement_is_deterministic_across_document_ids_and_statement_order() {
    let (left, _, _) = imported(SCHEMA);
    let (right, _, _) = imported(
        "CREATE TABLE unrelated(id integer PRIMARY KEY);
         CREATE TABLE sibling(id integer PRIMARY KEY,parent_id integer);
         CREATE TABLE parent(id integer PRIMARY KEY,title text DEFAULT 'a long default value');
         CREATE TABLE child(id integer PRIMARY KEY,parent_id integer NOT NULL);
         ALTER TABLE sibling ADD FOREIGN KEY(parent_id) REFERENCES parent(id);
         ALTER TABLE child ADD FOREIGN KEY(parent_id) REFERENCES parent(id);",
    );
    assert_eq!(bounds(&left), bounds(&right));
    assert!(
        arrange_page(&left, left.first_page().unwrap())
            .unwrap()
            .is_empty()
    );
}

#[test]
fn crossing_reduction_orders_children_by_their_parents() {
    let (doc, _, _) = imported(
        "CREATE TABLE alpha(id integer PRIMARY KEY);
         CREATE TABLE beta(id integer PRIMARY KEY);
         CREATE TABLE xray(id integer PRIMARY KEY,parent_id integer REFERENCES beta(id));
         CREATE TABLE yankee(id integer PRIMARY KEY,parent_id integer REFERENCES alpha(id));
         CREATE TABLE zulu(id integer PRIMARY KEY,a integer REFERENCES alpha(id),b integer REFERENCES beta(id));",
    );
    let positions = bounds(&doc);
    assert!(positions["alpha"].center().y < positions["beta"].center().y);
    assert!(positions["yankee"].center().y < positions["xray"].center().y);
    assert_non_overlapping(&doc);
}

#[test]
fn cycles_self_references_and_disconnected_components_are_packed_safely() {
    let (doc, _, _) = imported(
        "CREATE TABLE alpha(id integer PRIMARY KEY,beta_id integer);
         CREATE TABLE beta(id integer PRIMARY KEY,alpha_id integer);
         CREATE TABLE descendant(id integer PRIMARY KEY,beta_id integer REFERENCES beta(id));
         CREATE TABLE employee(id integer PRIMARY KEY,manager integer REFERENCES employee(id));
         CREATE TABLE isolated(id integer PRIMARY KEY);
         ALTER TABLE alpha ADD FOREIGN KEY(beta_id) REFERENCES beta(id);
         ALTER TABLE beta ADD FOREIGN KEY(alpha_id) REFERENCES alpha(id);",
    );
    let positions = bounds(&doc);
    assert_eq!(positions["alpha"].x0, positions["beta"].x0);
    assert!(positions["beta"].x1 < positions["descendant"].x0);
    assert_non_overlapping(&doc);
    assert_eq!(
        doc.elements
            .values()
            .filter(|element| element.as_connector().is_some())
            .count(),
        4
    );
}

#[test]
fn legacy_relationship_direction_is_inferred_from_columns_in_either_visual_direction() {
    let (mut forward, _, _) = imported(SCHEMA);
    for element in forward.elements.values_mut() {
        if let Some(connector) = element.as_connector_mut() {
            connector.foreign_key = None;
        }
    }
    let mut reverse = forward.clone();
    for element in reverse.elements.values_mut() {
        if let Some(connector) = element.as_connector_mut() {
            std::mem::swap(&mut connector.source, &mut connector.target);
            std::mem::swap(&mut connector.start_marker, &mut connector.end_marker);
        }
    }
    for doc in [&mut forward, &mut reverse] {
        let commands = arrange_page(doc, doc.first_page().unwrap()).unwrap();
        History::new().apply(doc, "Arrange", commands).unwrap();
        let positions = bounds(doc);
        assert!(positions["parent"].x1 < positions["child"].x0);
        assert!(positions["parent"].x1 < positions["sibling"].x0);
    }
    assert_eq!(bounds(&forward), bounds(&reverse));
}

#[test]
fn arrangement_is_one_undo_step_and_preserves_composite_fk_semantics() {
    let (mut doc, _, _) = imported(
        "CREATE TABLE parent(a integer,b integer,PRIMARY KEY(a,b));
         CREATE TABLE child(a integer,b integer,PRIMARY KEY(a,b),FOREIGN KEY(a,b) REFERENCES parent(a,b) ON DELETE CASCADE);",
    );
    for element in doc.elements.values_mut() {
        if let Some(shape) = element.as_shape_mut() {
            shape.bounds = Rect::new(500.0, 500.0, 520.0, 520.0);
            shape.style.font_size = Some(23.0);
        }
        if let Some(connector) = element.as_connector_mut() {
            // Visual direction can be swapped independently of SQL ownership.
            std::mem::swap(&mut connector.source, &mut connector.target);
            std::mem::swap(&mut connector.start_marker, &mut connector.end_marker);
            connector.foreign_key.as_mut().unwrap().owner_at_target = true;
            connector.waypoints = vec![Point::new(900.0, 900.0)];
            connector.routing = Routing::Curved;
        }
    }
    doc.validate().unwrap();
    let before = doc.clone();
    let sql_before = export_page(&doc, doc.first_page().unwrap(), SqlDialect::PostgreSql).unwrap();
    let original_connector = before
        .elements
        .values()
        .find_map(|element| element.as_connector())
        .unwrap();
    let mut history = History::new();
    let commands = arrange_page(&doc, doc.first_page().unwrap()).unwrap();
    history
        .apply(&mut doc, "Auto-arrange ERD", commands)
        .unwrap();
    assert_non_overlapping(&doc);
    let connector = doc
        .elements
        .values()
        .find_map(|element| element.as_connector())
        .unwrap();
    assert_eq!(connector.foreign_key, original_connector.foreign_key);
    assert_eq!(connector.start_marker, original_connector.start_marker);
    assert_eq!(connector.end_marker, original_connector.end_marker);
    assert_eq!(connector.style, original_connector.style);
    assert!(connector.waypoints.is_empty());
    assert_eq!(connector.routing, Routing::Orthogonal);
    for (old, new) in original_connector
        .endpoints()
        .iter()
        .zip(connector.endpoints())
    {
        let column = |endpoint: &bp_model::Endpoint| match endpoint {
            bp_model::Endpoint::Glued { port, .. } => port.as_ref().unwrap().column_id(),
            _ => None,
        };
        assert_eq!(old.element(), new.element());
        assert_eq!(column(old), column(new));
    }
    assert_eq!(
        export_page(&doc, doc.first_page().unwrap(), SqlDialect::PostgreSql)
            .unwrap()
            .sql,
        sql_before.sql
    );
    let arranged = doc.clone();
    assert!(history.undo(&mut doc));
    assert_eq!(doc, before);
    assert!(!history.can_undo());
    assert!(history.redo(&mut doc));
    assert_eq!(doc, arranged);
}

#[test]
fn locked_elements_and_hidden_layers_remain_fixed() {
    let (mut doc, _, _) = imported(SCHEMA);
    let page = doc.first_page().unwrap();
    let parent = id(&doc, "parent");
    let unrelated = id(&doc, "unrelated");
    doc.elements.get_mut(&parent).unwrap().locked = true;
    doc.elements
        .get_mut(&parent)
        .unwrap()
        .as_shape_mut()
        .unwrap()
        .bounds = Rect::new(40.0, 40.0, 700.0, 400.0);
    let mut hidden = Layer::new(page, "Hidden", OrderKey::first());
    hidden.visible = false;
    let hidden_id = hidden.id;
    doc.layers.insert(hidden.id, hidden);
    doc.elements.get_mut(&unrelated).unwrap().parent = Parent::Layer(hidden_id);
    let fixed_table = doc.elements[&parent].clone();
    let hidden_table = doc.elements[&unrelated].clone();
    let locked_connector = doc
        .elements
        .values()
        .find(|element| element.as_connector().is_some())
        .unwrap()
        .id;
    doc.elements.get_mut(&locked_connector).unwrap().locked = true;
    let fixed_connector = doc.elements[&locked_connector].clone();
    let commands = arrange_page(&doc, page).unwrap();
    History::new().apply(&mut doc, "Arrange", commands).unwrap();
    assert_eq!(doc.elements[&parent], fixed_table);
    assert_eq!(doc.elements[&unrelated], hidden_table);
    assert_eq!(doc.elements[&locked_connector], fixed_connector);
    assert!(!doc.layers[&hidden_id].visible);
    let fixed_bounds = shape_geometry(Libraries::builtin(), fixed_table.as_shape().unwrap()).bounds;
    for name in ["child", "sibling"] {
        let positioned = doc.elements[&id(&doc, name)].as_shape().unwrap().bounds;
        assert!(
            positioned.x0 >= fixed_bounds.x1
                || positioned.x1 <= fixed_bounds.x0
                || positioned.y0 >= fixed_bounds.y1
                || positioned.y1 <= fixed_bounds.y0
        );
    }
}

#[test]
fn import_keeps_existing_positions_and_avoids_actual_rendered_obstacles() {
    let (mut doc, _, parent) = imported("CREATE TABLE existing(id integer PRIMARY KEY);");
    let existing = id(&doc, "existing");
    let mut history = History::new();
    history
        .apply(
            &mut doc,
            "Move existing",
            [Command::Set {
                id: existing,
                prop: Prop::Bounds(Rect::new(50.0, 50.0, 60.0, 60.0)),
            }],
        )
        .unwrap();
    let original = doc.clone();
    let actual = shape_geometry(
        Libraries::builtin(),
        doc.elements[&existing].as_shape().unwrap(),
    )
    .bounds;
    let preview = parse(SCHEMA, SqlDialect::PostgreSql).unwrap();
    let (_, commands) = import_commands(&doc, parent, &preview).unwrap();
    history.apply(&mut doc, "Import SQL", commands).unwrap();
    assert_eq!(doc.elements[&existing], original.elements[&existing]);
    for name in ["parent", "child", "sibling", "unrelated"] {
        assert!(doc.elements[&id(&doc, name)].as_shape().unwrap().bounds.x0 > actual.x1);
    }
    assert!(history.undo(&mut doc));
    assert_eq!(doc, original);
}

#[test]
fn imports_into_hidden_layers_are_arranged_without_revealing_the_layer() {
    let mut doc = Document::new();
    let page = doc.first_page().unwrap();
    let layer = doc.layers_of(page)[0].id;
    doc.layers.get_mut(&layer).unwrap().visible = false;
    let preview = parse(SCHEMA, SqlDialect::PostgreSql).unwrap();
    let (_, commands) = import_commands(&doc, Parent::Layer(layer), &preview).unwrap();
    History::new()
        .apply(&mut doc, "Import SQL", commands)
        .unwrap();
    let positions = bounds(&doc);
    assert!(positions["parent"].x1 < positions["child"].x0);
    assert_non_overlapping(&doc);
    assert!(!doc.layers[&layer].visible);
    assert!(arrange_page(&doc, page).unwrap().is_empty());
}
