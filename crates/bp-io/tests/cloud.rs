use bp_model::kurbo::Rect;
use bp_model::{
    CloudIcon, CloudProvider, DiagramKind, Document, Element, IconKind, OrderKey, Parent, ShapeRef,
};
use std::io::{Cursor, Read, Write};
use std::sync::Arc;
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

const SVG: &str = "<?xml version=\"1.0\"?>\n<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 64 64\"><path d=\"M0 0h64v64H0Z\" fill=\"#765432\"/></svg>\n";

fn document() -> Document {
    let mut doc = Document::new();
    let page = doc.first_page().unwrap();
    doc.pages.get_mut(&page).unwrap().diagram_kind = Some(DiagramKind::Cloud);
    let icon = CloudIcon {
        reference: ShapeRef::new("aws", "sample@2026-10"),
        name: "Sample service".into(),
        provider: CloudProvider::Aws,
        category: "Compute".into(),
        kind: IconKind::Service,
        pack_version: "2026-10".into(),
        source_path: "Compute/Arch_Sample_64.svg".into(),
        svg: Arc::from(SVG),
    };
    let element = Element::shape(
        icon.reference.clone(),
        Parent::Layer(doc.layers_of(page)[0].id),
        OrderKey::first(),
        Rect::new(10.0, 20.0, 74.0, 84.0),
    );
    doc.elements.insert(element.id, element);
    doc.icons.insert(icon.reference.clone(), icon);
    doc
}

fn archive(value: &serde_json::Value, assets: &[(&str, &[u8])]) -> Vec<u8> {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file("document.json", SimpleFileOptions::default())
        .unwrap();
    zip.write_all(&serde_json::to_vec(value).unwrap()).unwrap();
    for (path, bytes) in assets {
        zip.start_file(*path, SimpleFileOptions::default()).unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

fn metadata() -> serde_json::Value {
    let mut value = serde_json::to_value(document()).unwrap();
    let icon = value["icons"]["aws/sample@2026-10"]
        .as_object_mut()
        .unwrap();
    icon.remove("svg");
    icon.insert("asset_path".into(), "icons/aws/sample@2026-10.svg".into());
    value
}

#[test]
fn native_and_json_files_keep_exact_svg_without_an_installed_pack() {
    let doc = document();
    let zipped = bp_io::to_zip_bytes(&doc).unwrap();
    let mut zip = ZipArchive::new(Cursor::new(&zipped)).unwrap();
    assert_eq!(zip.len(), 2);
    let mut raw = Vec::new();
    zip.by_name("icons/aws/sample@2026-10.svg")
        .unwrap()
        .read_to_end(&mut raw)
        .unwrap();
    assert_eq!(raw, SVG.as_bytes());
    let metadata: serde_json::Value =
        serde_json::from_reader(zip.by_name("document.json").unwrap()).unwrap();
    assert!(metadata["icons"]["aws/sample@2026-10"].get("svg").is_none());
    assert_eq!(
        metadata["icons"]["aws/sample@2026-10"]["asset_path"],
        "icons/aws/sample@2026-10.svg"
    );
    for bytes in [zipped, bp_io::to_json_bytes(&doc).unwrap()] {
        let loaded = bp_io::from_bytes(&bytes).unwrap();
        assert_eq!(loaded, doc);
        assert_eq!(
            loaded.icons[&ShapeRef::new("aws", "sample@2026-10")]
                .svg
                .as_bytes(),
            SVG.as_bytes()
        );
    }
}

#[test]
fn missing_mismatched_and_ambiguous_icon_entries_are_rejected() {
    assert!(bp_io::from_bytes(&archive(&metadata(), &[])).is_err());
    for path in [
        "../../outside.svg",
        "icons/aws/wrong@2026-10.svg",
        "/icons/aws/sample@2026-10.svg",
    ] {
        let mut value = metadata();
        value["icons"]["aws/sample@2026-10"]["asset_path"] = path.into();
        assert!(matches!(
            bp_io::from_bytes(&archive(&value, &[(path, SVG.as_bytes())])),
            Err(bp_io::IoError::IconAsset(_))
        ));
    }
    let mut value = metadata();
    value["icons"]["aws/sample@2026-10"]["svg"] = SVG.into();
    assert!(matches!(
        bp_io::from_bytes(&archive(
            &value,
            &[("icons/aws/sample@2026-10.svg", SVG.as_bytes())]
        )),
        Err(bp_io::IoError::IconAsset(_))
    ));
}

#[test]
fn oversized_and_non_utf8_archive_icons_are_rejected() {
    let path = "icons/aws/sample@2026-10.svg";
    assert!(matches!(
        bp_io::from_bytes(&archive(&metadata(), &[(path, &[255])])),
        Err(bp_io::IoError::IconAsset(_))
    ));
    let too_large = vec![b' '; bp_icons::MAX_SVG_BYTES + 1];
    assert!(matches!(
        bp_io::from_bytes(&archive(&metadata(), &[(path, &too_large)])),
        Err(bp_io::IoError::IconAsset(_))
    ));
}

#[test]
fn unsupported_svg_text_is_rejected_before_save_and_after_native_or_json_load() {
    let mixed = SVG.replace(
        "</svg>",
        "<text x=\"0\" y=\"16\">Unsupported text</text></svg>",
    );
    let mut doc = document();
    doc.icons.values_mut().next().unwrap().svg = Arc::from(mixed.as_str());
    assert!(matches!(
        bp_io::to_zip_bytes(&doc),
        Err(bp_io::IoError::IconAsset(_))
    ));
    assert!(matches!(
        bp_io::to_json_bytes(&doc),
        Err(bp_io::IoError::IconAsset(_))
    ));
    assert!(matches!(
        bp_io::from_bytes(&serde_json::to_vec(&doc).unwrap()),
        Err(bp_io::IoError::IconAsset(_))
    ));
    let native = archive(
        &metadata(),
        &[("icons/aws/sample@2026-10.svg", mixed.as_bytes())],
    );
    assert!(matches!(
        bp_io::from_bytes(&native),
        Err(bp_io::IoError::IconAsset(_))
    ));
}

#[test]
fn zip_writer_rejects_more_entries_than_the_reader_accepts() {
    let mut doc = document();
    let template = doc.icons.values().next().unwrap().clone();
    // SVG bytes are shared, so this tests the entry boundary without huge assets.
    for index in 0..10_000 {
        let mut icon = template.clone();
        icon.reference = ShapeRef::new("aws", &format!("sample-{index}@2026-10"));
        doc.icons.insert(icon.reference.clone(), icon);
    }
    assert!(
        matches!(bp_io::to_zip_bytes(&doc), Err(bp_io::IoError::IconAsset(message)) if message == "too many archive entries")
    );
}
