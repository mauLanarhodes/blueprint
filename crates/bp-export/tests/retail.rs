//! A dense, real-sized ERD exercises layout, routing and exported geometry together.

use bp_commands::History;
use bp_model::kurbo::{Point, Rect};
use bp_model::{DiagramKind, Document, Parent, SqlDialect};

fn enters(a: Point, b: Point, bounds: Rect) -> bool {
    // Touching the perimeter at a column port is allowed, entering a row is not.
    let bounds = bounds.inset(-0.01);
    if (a.x - b.x).abs() < 1e-8 {
        a.x > bounds.x0 && a.x < bounds.x1 && a.y.min(b.y) < bounds.y1 && a.y.max(b.y) > bounds.y0
    } else {
        assert!(
            (a.y - b.y).abs() < 1e-8,
            "non-orthogonal route: {a:?} -> {b:?}"
        );
        a.y > bounds.y0 && a.y < bounds.y1 && a.x.min(b.x) < bounds.x1 && a.x.max(b.x) > bounds.x0
    }
}

#[test]
fn dense_retail_import_routes_clear_of_tables_and_exports_the_shared_scene() {
    let preview = bp_sql::parse(
        include_str!("../../../examples/retail.sql"),
        SqlDialect::PostgreSql,
    )
    .unwrap();
    assert!(preview.warnings.is_empty(), "{:?}", preview.warnings);
    assert_eq!(preview.schema.tables.len(), 30);
    assert_eq!(preview.schema.foreign_keys.len(), 44);
    let mut doc = Document::new();
    let page = doc.first_page().unwrap();
    doc.pages.get_mut(&page).unwrap().diagram_kind = Some(DiagramKind::Erd);
    let parent = Parent::Layer(doc.layers_of(page)[0].id);
    let (_, commands) = bp_sql::import_commands(&doc, parent, &preview).unwrap();
    let mut history = History::new();
    history
        .apply(&mut doc, "Import retail SQL", commands)
        .unwrap();
    let scene = bp_scene::build_page(&doc, page);
    let tables: Vec<_> = doc
        .elements
        .values()
        .filter_map(|element| {
            let table = element.as_shape()?.erd.as_ref()?;
            let shape = scene.shape(element.id).unwrap();
            assert_eq!(shape.erd.as_ref().unwrap().rows.len(), table.columns.len());
            Some((element.id, shape.bounds))
        })
        .collect();
    for (i, (id, bounds)) in tables.iter().enumerate() {
        for (other, other_bounds) in &tables[i + 1..] {
            assert_eq!(
                bounds.intersect(*other_bounds).area(),
                0.0,
                "overlap: {id} and {other}"
            );
        }
    }
    for element in doc
        .elements
        .values()
        .filter(|element| element.is_connector())
    {
        let route = scene.connector(element.id).unwrap();
        assert!(route.points.len() >= 2);
        for segment in route.points.windows(2) {
            for (table, bounds) in &tables {
                assert!(
                    !enters(segment[0], segment[1], *bounds),
                    "route {} enters table {table}: {:?}",
                    element.id,
                    route.points
                );
            }
        }
    }
    let options = bp_export::SvgOptions::default();
    assert_eq!(
        bp_export::page_to_svg(&doc, page, &options),
        bp_export::to_svg(&scene.list, &options)
    );
    let ddl = bp_sql::export_page(&doc, page, SqlDialect::PostgreSql).unwrap();
    assert!(ddl.warnings.is_empty(), "{:?}", ddl.warnings);
    assert_eq!(
        bp_sql::parse(&ddl.sql, SqlDialect::PostgreSql)
            .unwrap()
            .schema,
        preview.schema
    );
    let imported = doc.clone();
    assert!(history.undo(&mut doc));
    assert!(doc.elements.is_empty());
    assert!(history.redo(&mut doc));
    assert_eq!(doc, imported);
}
