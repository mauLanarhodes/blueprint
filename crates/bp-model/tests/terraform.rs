use bp_model::*;
use serde_json::json;
use std::collections::{BTreeMap, HashMap};

fn resource() -> TerraformResource {
    TerraformResource {
        key: "primary".into(),
        provider: ProviderPin {
            provider: TerraformProvider::Aws,
            version: "6.68.0".into(),
        },
        resource_type: "aws_s3_bucket".into(),
        properties: BTreeMap::new(),
        depends_on: Vec::new(),
    }
}

#[test]
fn exact_versions_resource_identity_and_nested_values_round_trip() {
    let owner = ElementId::new();
    let mut bucket = resource();
    bucket.properties.insert(
        "tags".into(),
        ResourceValue::Object(BTreeMap::from([
            (
                "Name".into(),
                ResourceValue::Literal(json!("${literal-text}")),
            ),
            (
                "Owner".into(),
                ResourceValue::Input(InputReference {
                    name: "owner".into(),
                    sensitive: false,
                }),
            ),
        ])),
    );
    let address = bucket.address(owner);
    let configuration = AwsResource::Resources {
        resources: vec![bucket],
    };
    assert!(configuration.validate().is_ok());
    let value = serde_json::to_vec(&configuration).unwrap();
    assert_eq!(
        serde_json::from_slice::<AwsResource>(&value).unwrap(),
        configuration
    );
    let AwsResource::Resources { resources } = configuration else {
        unreachable!()
    };
    assert_eq!(resources[0].address(owner), address);
    assert_ne!(resources[0].address(ElementId::new()), address);
    let mut invalid = resources[0].clone();
    for version in ["~> 6.0", "6.68", "../6.68.0", "06.68.0", "6.68.0-beta"] {
        invalid.provider.version = version.into();
        assert!(invalid.validate().is_err());
    }
    invalid = resource();
    invalid.provider.provider = TerraformProvider::Awscc;
    assert!(invalid.validate().is_err());
    assert!(
        AwsResource::Resources {
            resources: vec![resource(), resource()]
        }
        .validate()
        .is_err()
    );
}

#[test]
fn copies_remap_nested_references_and_keep_external_dependencies_explicitly_unresolved() {
    let original = ElementId::new();
    let copied = ElementId::new();
    let outside = ElementId::new();
    let target = |element| ResourceTarget {
        element: Some(element),
        resource: "primary".into(),
    };
    let mut value = ResourceValue::Object(BTreeMap::from([(
        "rules".into(),
        ResourceValue::List(vec![
            ResourceValue::Reference(ResourceReference {
                target: target(original),
                output: vec![OutputStep::Attribute("arn".into())],
            }),
            ResourceValue::Reference(ResourceReference {
                target: target(outside),
                output: vec![
                    OutputStep::Attribute("network".into()),
                    OutputStep::Index(0),
                    OutputStep::Attribute("id".into()),
                ],
            }),
        ]),
    )]));
    value.remap(&HashMap::from([(original, copied)]));
    let references = value.references();
    assert_eq!(references[0].target.element, Some(copied));
    assert_eq!(references[1].target.element, None);
    assert_eq!(references[1].output.len(), 3);
}

#[test]
fn documents_reject_malformed_resource_metadata_but_preserve_incomplete_drafts() {
    let mut doc = Document::new();
    let page = doc.first_page().unwrap();
    let mut element = Element::shape(
        ShapeRef::new("basic", "rectangle"),
        Parent::Layer(doc.layers_of(page)[0].id),
        OrderKey::first(),
        kurbo::Rect::new(0., 0., 100., 100.),
    );
    let mut draft = resource();
    draft.depends_on.push(ResourceTarget {
        element: None,
        resource: "primary".into(),
    });
    element.as_shape_mut().unwrap().infrastructure = Some(AwsResource::Resources {
        resources: vec![draft.clone()],
    });
    doc.elements.insert(element.id, element.clone());
    assert!(doc.validate().is_ok());
    draft.key = "not a valid identity".into();
    element.as_shape_mut().unwrap().infrastructure = Some(AwsResource::Resources {
        resources: vec![draft],
    });
    doc.elements.insert(element.id, element);
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidInfrastructure { .. })
    ));
}
