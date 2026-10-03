//! Files saved by older versions of Blueprint must keep opening.

use bp_model::{Color, DiagramKind, ElementKind, OrderKey, Page, Paint, Parent, SCHEMA_VERSION};
use std::path::Path;

fn fixture(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// The Phase 0 gate drawing, saved by the Phase 0 app (schema 1).
fn check_phase0(doc: &bp_model::Document) {
    assert_eq!(doc.schema_version, SCHEMA_VERSION);
    assert_eq!(doc.validate(), Ok(()));
    assert!(doc.pages.values().all(|page| page.diagram_kind.is_none()));
    let page = doc.first_page().unwrap();
    let layer = doc.layers_of(page)[0].id;
    let shapes: Vec<_> = doc
        .paint_order(page)
        .into_iter()
        .map(|e| {
            assert_eq!(e.parent, Parent::Layer(layer));
            match &e.kind {
                ElementKind::Shape(s) => s.clone(),
                other => panic!("expected a shape, got {other:?}"),
            }
        })
        .collect();
    let summary: Vec<_> = shapes
        .iter()
        .map(|s| (s.shape.as_str(), s.text.as_str()))
        .collect();
    assert_eq!(
        summary,
        [
            ("basic/rounded-rectangle", "Web app"),
            ("basic/diamond", "Signed in?"),
            ("basic/rectangle", "Orders API"),
            ("basic/ellipse", "Postgres"),
            ("basic/text", "Phase 0 gate"),
        ]
    );
    // Only the changed fill survives as an override.
    assert_eq!(
        shapes[3].style.fill,
        Some(Paint::Color(Color::rgb(0xdb, 0xea, 0xfe)))
    );
    assert!(
        shapes
            .iter()
            .filter(|s| s.shape.shape() != "ellipse")
            .all(|s| s.style.is_empty())
    );
    assert_eq!(shapes[1].bounds.y0, -10.0);
}

#[test]
fn opens_phase0_zip() {
    check_phase0(&bp_io::load(&fixture("phase0.blueprint")).unwrap());
}

#[test]
fn opens_phase0_json() {
    check_phase0(&bp_io::load(&fixture("phase0.blueprint.json")).unwrap());
}

#[test]
fn migrated_files_save_as_the_current_schema() {
    let doc = bp_io::load(&fixture("phase0.blueprint.json")).unwrap();
    let json = String::from_utf8(bp_io::to_json_bytes(&doc).unwrap()).unwrap();
    assert!(json.contains(&format!("\"schema_version\": {SCHEMA_VERSION}")));
    assert!(!json.contains("\"kind\""));
    let again = bp_io::from_bytes(json.as_bytes()).unwrap();
    assert_eq!(again, doc);
}

#[test]
fn phase1_shapes_migrate_without_changing_their_data() {
    let mut doc = bp_model::Document::new();
    let parent = Parent::Layer(doc.layers_of(doc.first_page().unwrap())[0].id);
    let mut shape = bp_model::Element::shape(
        bp_model::ShapeRef::new("basic", "rectangle"),
        parent,
        bp_model::OrderKey::first(),
        bp_model::kurbo::Rect::new(10.0, 20.0, 120.0, 80.0),
    );
    shape.as_shape_mut().unwrap().text = "Phase 1".into();
    doc.elements.insert(shape.id, shape);
    let mut json = serde_json::to_value(&doc).unwrap();
    json["schema_version"] = 2.into();
    let loaded = bp_io::from_bytes(&serde_json::to_vec(&json).unwrap()).unwrap();
    assert_eq!(loaded, doc);
    assert!(
        loaded
            .elements
            .values()
            .all(|element| element.as_shape().unwrap().erd.is_none())
    );
}

#[test]
fn phase1_table_placeholders_gain_structured_data_on_migration() {
    let mut doc = bp_model::Document::new();
    let parent = Parent::Layer(doc.layers_of(doc.first_page().unwrap())[0].id);
    let mut shape = bp_model::Element::shape(
        bp_model::ShapeRef::new("erd", "table"),
        parent,
        bp_model::OrderKey::first(),
        bp_model::kurbo::Rect::new(10.0, 20.0, 250.0, 140.0),
    );
    shape.as_shape_mut().unwrap().text = "accounts".into();
    let id = shape.id;
    doc.elements.insert(id, shape);
    let mut json = serde_json::to_value(&doc).unwrap();
    json["schema_version"] = 2.into();
    json["elements"][id.to_string()]
        .as_object_mut()
        .unwrap()
        .remove("erd");
    let loaded = bp_io::from_bytes(&serde_json::to_vec(&json).unwrap()).unwrap();
    assert_eq!(loaded.schema_version, SCHEMA_VERSION);
    assert_eq!(loaded.elements[&id].text(), Some("accounts"));
    let table = loaded.elements[&id]
        .as_shape()
        .unwrap()
        .erd
        .as_ref()
        .unwrap();
    assert_eq!(table.columns[0].name, "id");
    assert!(table.columns[0].primary_key);
    assert!(!table.columns[0].nullable);
    assert_eq!(
        bp_io::from_bytes(&bp_io::to_zip_bytes(&loaded).unwrap()).unwrap(),
        loaded
    );
}

#[test]
fn structured_table_and_relationship_round_trip_in_both_formats() {
    let mut doc = bp_model::Document::new();
    let parent = Parent::Layer(doc.layers_of(doc.first_page().unwrap())[0].id);
    let mut table = bp_model::Element::shape(
        bp_model::ShapeRef::new("erd", "table"),
        parent,
        bp_model::OrderKey::first(),
        bp_model::kurbo::Rect::new(0.0, 0.0, 240.0, 120.0),
    );
    let table_id = table.id;
    let data = table.as_shape_mut().unwrap().erd.as_mut().unwrap();
    data.display = bp_model::TableDisplay::KeysOnly;
    data.dialect = bp_model::SqlDialect::Oracle;
    let mut row = bp_model::ErdColumn::new(
        "owner_id",
        "RAW(16)",
        bp_model::OrderKey::after(&data.columns[0].order),
    );
    row.foreign_key = true;
    row.unique = true;
    row.default_value = Some("SYS_GUID()".into());
    let column = row.id;
    data.columns.push(row);
    doc.elements.insert(table_id, table);
    let mut relationship = bp_model::Element::connector(
        bp_model::Endpoint::Glued {
            element: table_id,
            port: Some(bp_model::PortId::column(column, false)),
        },
        bp_model::Endpoint::Free(bp_model::kurbo::Point::new(400.0, 90.0)),
        parent,
        doc.next_order_key(parent),
    );
    let connector = relationship.as_connector_mut().unwrap();
    connector.start_marker = bp_model::Marker::ExactlyOne;
    connector.end_marker = bp_model::Marker::ZeroOrMany;
    doc.elements.insert(relationship.id, relationship);
    for bytes in [
        bp_io::to_json_bytes(&doc).unwrap(),
        bp_io::to_zip_bytes(&doc).unwrap(),
    ] {
        assert_eq!(bp_io::from_bytes(&bytes).unwrap(), doc);
    }
}

#[test]
fn page_kinds_round_trip_in_json_and_zip_without_classifying_legacy_pages() {
    let mut doc = bp_io::load(&fixture("phase0.blueprint.json")).unwrap();
    let first = doc.first_page().unwrap();
    let mut order = doc.pages[&first].order.clone();
    for kind in [DiagramKind::Erd, DiagramKind::Flowchart] {
        order = OrderKey::after(&order);
        let mut page = Page::new(kind.label(), order.clone());
        page.diagram_kind = Some(kind);
        doc.pages.insert(page.id, page);
    }

    for bytes in [
        bp_io::to_json_bytes(&doc).unwrap(),
        bp_io::to_zip_bytes(&doc).unwrap(),
    ] {
        let loaded = bp_io::from_bytes(&bytes).unwrap();
        assert_eq!(loaded, doc);
        assert_eq!(loaded.pages[&first].diagram_kind, None);
        assert_eq!(
            loaded
                .pages_sorted()
                .iter()
                .map(|page| page.diagram_kind)
                .collect::<Vec<_>>(),
            [None, Some(DiagramKind::Erd), Some(DiagramKind::Flowchart)]
        );
    }
}

#[test]
fn current_schema_rejects_table_metadata_mismatches() {
    let mut doc = bp_model::Document::new();
    let parent = Parent::Layer(doc.layers_of(doc.first_page().unwrap())[0].id);
    let table = bp_model::Element::shape(
        bp_model::ShapeRef::new("erd", "table"),
        parent,
        bp_model::OrderKey::first(),
        bp_model::kurbo::Rect::new(0.0, 0.0, 240.0, 120.0),
    );
    let id = table.id;
    doc.elements.insert(id, table);
    let mut json = serde_json::to_value(&doc).unwrap();
    json["elements"][id.to_string()]
        .as_object_mut()
        .unwrap()
        .remove("erd");
    assert!(
        matches!(bp_io::from_bytes(&serde_json::to_vec(&json).unwrap()),Err(bp_io::IoError::Model(bp_model::ModelError::MissingErdData(target))) if target == id)
    );
    let mut json = serde_json::to_value(&doc).unwrap();
    json["elements"][id.to_string()]["shape"] = "basic/rectangle".into();
    assert!(
        matches!(bp_io::from_bytes(&serde_json::to_vec(&json).unwrap()),Err(bp_io::IoError::Model(bp_model::ModelError::UnexpectedErdData(target))) if target == id)
    );
}
