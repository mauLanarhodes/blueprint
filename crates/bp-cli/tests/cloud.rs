//! Portable cloud documents exported by the real CLI without installed packs.
use bp_model::kurbo::Rect;
use bp_model::{
    CloudIcon, CloudProvider, DiagramKind, Document, Element, ElementId, Endpoint, IconKind,
    Parent, ShapeRef,
};
use std::process::Command;

#[test]
fn embedded_cloud_artwork_reopens_and_exports_through_the_cli() {
    let mut doc = Document::new();
    let page = doc.first_page().unwrap();
    doc.pages.get_mut(&page).unwrap().diagram_kind = Some(DiagramKind::Cloud);
    let parent = Parent::Layer(doc.layers_of(page)[0].id);
    let asset = CloudIcon {
        reference: ShapeRef::new("azure", "compute-service-demo@v1"),
        name: "Demo service".into(),
        provider: CloudProvider::Azure,
        category: "Compute".into(),
        kind: IconKind::Service,
        pack_version: "v1".into(),
        source_path: "Compute/Demo.svg".into(),
        svg: std::sync::Arc::from(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"32\" height=\"16\"><rect width=\"32\" height=\"16\" fill=\"#0078d4\"/></svg>\n",
        ),
    };
    doc.icons.insert(asset.reference.clone(), asset.clone());
    let mut ids = Vec::new();
    for x in [20.0, 240.0] {
        let mut shape = Element::shape(
            asset.reference.clone(),
            parent,
            doc.next_order_key(parent),
            Rect::new(x, 20.0, x + 64.0, 84.0),
        );
        shape.as_shape_mut().unwrap().text = asset.name.clone();
        ids.push(shape.id);
        doc.elements.insert(shape.id, shape);
    }
    let connector = Element::connector(
        Endpoint::glued(ids[0], Some("e")),
        Endpoint::glued(ids[1], Some("w")),
        parent,
        doc.next_order_key(parent),
    );
    let connector_id = connector.id;
    doc.elements.insert(connector_id, connector);
    assert_eq!(doc.validate(), Ok(()));
    let directory = std::env::temp_dir().join(format!("bp-cloud-cli-{}", ElementId::new()));
    std::fs::create_dir_all(&directory).unwrap();
    for extension in ["blueprint", "blueprint.json"] {
        let input = directory.join(format!("cloud.{extension}"));
        let output = directory.join(format!("{extension}.svg"));
        bp_io::save(&doc, &input).unwrap();
        let reopened = bp_io::load(&input).unwrap();
        assert_eq!(reopened, doc);
        let result = Command::new(env!("CARGO_BIN_EXE_blueprint-cli"))
            .arg("export")
            .arg(&input)
            .arg(&output)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let svg = std::fs::read_to_string(output).unwrap();
        assert_eq!(svg.matches("data:image/svg+xml;base64,").count(), 2);
        assert_eq!(
            svg.matches("preserveAspectRatio=\"xMidYMid meet\"").count(),
            2
        );
        assert!(svg.contains("Demo service"));
        assert!(!svg.contains("/home/") && !svg.contains("file://"));
        assert_eq!(
            svg,
            bp_export::page_to_svg(&doc, page, &bp_export::SvgOptions::default())
        );
        let scene = bp_scene::build_page(&reopened, page);
        assert_eq!(
            scene
                .list
                .items()
                .filter(|item| matches!(item.primitive, bp_scene::Primitive::Icon { .. }))
                .count(),
            2
        );
        assert_eq!(
            scene.connector(connector_id).unwrap().points[0],
            scene
                .shape(ids[0])
                .unwrap()
                .ports
                .iter()
                .find(|port| port.id.as_str() == "e")
                .unwrap()
                .at
        );
    }
    std::fs::remove_dir_all(directory).unwrap();
}
