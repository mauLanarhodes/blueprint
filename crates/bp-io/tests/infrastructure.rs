use bp_model::*;
use std::collections::BTreeMap;

fn network() -> (Document, ElementId, ElementId) {
    let mut doc = Document::new();
    let page = doc.first_page().unwrap();
    doc.pages.get_mut(&page).unwrap().diagram_kind = Some(DiagramKind::Cloud);
    doc.pages.get_mut(&page).unwrap().deployment = Some(AwsDeployment {
        region: "us-east-1".into(),
        profile: "test".into(),
        account_id: "123456789012".into(),
    });
    let parent = Parent::Layer(doc.layers_of(page)[0].id);
    let mut vpc = Element::shape(
        ShapeRef::new("basic", "rectangle"),
        parent,
        OrderKey::first(),
        kurbo::Rect::new(0., 0., 100., 100.),
    );
    vpc.as_shape_mut().unwrap().infrastructure = Some(AwsResource::Vpc {
        cidr: "10.0.0.0/16".into(),
    });
    let mut subnet = Element::shape(
        ShapeRef::new("basic", "rectangle"),
        parent,
        OrderKey::after(&vpc.order),
        kurbo::Rect::new(150., 0., 250., 100.),
    );
    subnet.as_shape_mut().unwrap().infrastructure = Some(AwsResource::Subnet {
        vpc: Some(vpc.id),
        cidr: "10.0.1.0/24".into(),
        availability_zone: "us-east-1a".into(),
    });
    let ids = (vpc.id, subnet.id);
    doc.elements.insert(vpc.id, vpc);
    doc.elements.insert(subnet.id, subnet);
    (doc, ids.0, ids.1)
}

#[test]
fn schema6_migration_preserves_legacy_settings_ids_and_both_file_formats() {
    let (doc, vpc, subnet) = network();
    let mut old = serde_json::to_value(&doc).unwrap();
    old["schema_version"] = 6.into();
    let migrated = bp_io::from_bytes(&serde_json::to_vec(&old).unwrap()).unwrap();
    assert_eq!(migrated, doc);
    assert_eq!(migrated.elements[&vpc], doc.elements[&vpc]);
    assert_eq!(migrated.elements[&subnet], doc.elements[&subnet]);
    for bytes in [
        bp_io::to_json_bytes(&migrated).unwrap(),
        bp_io::to_zip_bytes(&migrated).unwrap(),
    ] {
        assert_eq!(bp_io::from_bytes(&bytes).unwrap(), doc);
    }
}

#[test]
fn schema7_keeps_multi_resource_drafts_and_unresolved_inputs_without_installed_schemas() {
    let (mut doc, vpc, _) = network();
    let draft = TerraformResource {
        key: "primary".into(),
        provider: ProviderPin {
            provider: TerraformProvider::Awscc,
            version: "1.105.0".into(),
        },
        resource_type: "awscc_s3_bucket".into(),
        properties: BTreeMap::from([(
            "bucket_name".into(),
            ResourceValue::Input(InputReference {
                name: "bucket_name".into(),
                sensitive: false,
            }),
        )]),
        depends_on: vec![ResourceTarget {
            element: None,
            resource: "primary".into(),
        }],
    };
    doc.elements
        .get_mut(&vpc)
        .unwrap()
        .as_shape_mut()
        .unwrap()
        .infrastructure = Some(AwsResource::Resources {
        resources: vec![
            draft.clone(),
            TerraformResource {
                key: "logs".into(),
                ..draft
            },
        ],
    });
    for bytes in [
        bp_io::to_json_bytes(&doc).unwrap(),
        bp_io::to_zip_bytes(&doc).unwrap(),
    ] {
        assert_eq!(bp_io::from_bytes(&bytes).unwrap(), doc);
    }
    let mut too_new = serde_json::to_value(&doc).unwrap();
    too_new["schema_version"] = (SCHEMA_VERSION + 1).into();
    assert!(matches!(
        bp_io::from_bytes(&serde_json::to_vec(&too_new).unwrap()),
        Err(bp_io::IoError::TooNew { .. })
    ));
}
