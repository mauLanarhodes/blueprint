//! Files saved by older versions of Blueprint must keep opening.

use bp_model::{Color, ElementKind, Paint, Parent, SCHEMA_VERSION};
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
