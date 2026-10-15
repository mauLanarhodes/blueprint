use bp_deploy::{catalog::*, schemas::*};
use bp_model::*;
use serde_json::json;
use std::collections::BTreeMap;

const SCHEMA: &[u8] = include_bytes!("fixtures/schema.json");
const VERSION: &[u8] = include_bytes!("fixtures/version.json");

fn pin() -> ProviderPin {
    ProviderPin {
        provider: TerraformProvider::Aws,
        version: "6.68.0".into(),
    }
}
fn snapshot() -> SchemaSnapshot {
    SchemaSnapshot::from_outputs(&pin(), VERSION, SCHEMA).unwrap()
}
fn draft(resource_type: &str) -> TerraformResource {
    TerraformResource {
        key: "primary".into(),
        provider: pin(),
        resource_type: resource_type.into(),
        properties: BTreeMap::new(),
        depends_on: Vec::new(),
    }
}

#[test]
fn inventory_has_all_supplied_entries_without_claiming_distinct_aws_resources() {
    let catalog = Catalog::builtin();
    assert_eq!(catalog.entries().count(), 3388);
    for (provider, count, groups) in [
        (TerraformProvider::Aws, 1724, 255),
        (TerraformProvider::Awscc, 1664, 292),
    ] {
        let inventory = catalog
            .manifest
            .providers
            .iter()
            .find(|entry| entry.provider == provider)
            .unwrap();
        assert_eq!(
            (inventory.resource_count, inventory.group_count),
            (count, groups)
        );
    }
    for entry in catalog.entries().chain(catalog.aliases()) {
        let mut resource = draft(&entry.terraform_resource_type);
        resource.provider = entry.choice().provider;
        resource.validate().unwrap();
        catalog.validate_choice(&entry.choice()).unwrap();
    }
    assert!(
        !catalog
            .search(Some(TerraformProvider::Awscc), "s3 bucket")
            .is_empty()
    );
}

#[test]
fn schemas_verify_selected_release_keep_versions_separate_and_report_coverage_gaps() {
    let snapshot = snapshot();
    assert_eq!(snapshot.resource("aws_instance").unwrap().version, 3);
    assert_eq!(
        snapshot
            .output_type("aws_s3_bucket", &[OutputStep::Attribute("arn".into())])
            .unwrap(),
        Some(json!("string"))
    );
    assert!(
        snapshot
            .output_type(
                "aws_s3_bucket",
                &[OutputStep::Attribute("not_an_output".into())]
            )
            .is_err()
    );
    assert_eq!(
        snapshot
            .output_type(
                "aws_instance",
                &[
                    OutputStep::Attribute("root_block_device".into()),
                    OutputStep::Index(0),
                    OutputStep::Attribute("volume_size".into())
                ]
            )
            .unwrap(),
        Some(json!("number"))
    );
    assert_eq!(snapshot.pin.version, "6.68.0");
    assert_eq!(
        snapshot.provider_schema["future_minor_field"]["retained"],
        true
    );
    let coverage = snapshot.coverage(Catalog::builtin()).unwrap();
    assert_eq!(
        (
            coverage.expected,
            coverage.available,
            coverage.missing.len()
        ),
        (1730, 2, 1728)
    );
    let wrong = ProviderPin {
        version: "6.67.0".into(),
        ..pin()
    };
    assert!(SchemaSnapshot::from_outputs(&wrong, VERSION, SCHEMA).is_err());
    assert!(snapshot.validate(&wrong).is_err());
    let mut unsupported = serde_json::from_slice::<serde_json::Value>(SCHEMA).unwrap();
    unsupported["format_version"] = "2.0".into();
    assert!(
        SchemaSnapshot::from_outputs(&pin(), VERSION, &serde_json::to_vec(&unsupported).unwrap())
            .is_err()
    );
}

#[test]
fn structural_validation_covers_required_computed_sensitive_nested_and_mixed_values() {
    let snapshot = snapshot();
    let mut resource = draft("aws_instance");
    assert_eq!(snapshot.validate_resource(&resource).unwrap().len(), 2);
    resource.properties = BTreeMap::from([
        ("ami".into(), ResourceValue::Literal(json!("ami-example"))),
        (
            "instance_type".into(),
            ResourceValue::Literal(json!("t3.micro")),
        ),
        (
            "root_block_device".into(),
            ResourceValue::Literal(json!([{"volume_size":30}])),
        ),
        (
            "options".into(),
            ResourceValue::Literal(json!([{"enabled":true}])),
        ),
        (
            "password".into(),
            ResourceValue::Input(InputReference {
                name: "password".into(),
                sensitive: true,
            }),
        ),
    ]);
    assert!(snapshot.validate_resource(&resource).unwrap().is_empty());
    resource.properties.insert(
        "password".into(),
        ResourceValue::Literal(json!("do-not-store-this")),
    );
    resource.properties.insert(
        "options".into(),
        ResourceValue::Literal(json!([{"enabled":"wrong type"}])),
    );
    resource.properties.insert(
        "root_block_device".into(),
        ResourceValue::Literal(json!([{}, {}])),
    );
    assert!(snapshot.validate_resource(&resource).unwrap().len() >= 4);
    let mut bucket = draft("aws_s3_bucket");
    bucket
        .properties
        .insert("bucket".into(), ResourceValue::Literal(json!("example")));
    bucket.properties.insert(
        "arn".into(),
        ResourceValue::Literal(json!("cannot configure output")),
    );
    let errors = snapshot.validate_resource(&bucket).unwrap();
    assert!(errors.iter().any(|error| error.contains("computed output")));
    bucket.properties.remove("arn");
    bucket.properties.insert(
        "tags".into(),
        ResourceValue::Object(BTreeMap::from([
            (
                "Dynamic".into(),
                ResourceValue::Input(InputReference {
                    name: "owner".into(),
                    sensitive: false,
                }),
            ),
            ("Invalid".into(), ResourceValue::Literal(json!(42))),
        ])),
    );
    assert!(
        snapshot
            .validate_resource(&bucket)
            .unwrap()
            .iter()
            .any(|error| error.contains("tags.Invalid"))
    );
}

fn icon(name: &str, kind: IconKind) -> CloudIcon {
    CloudIcon {
        reference: ShapeRef::new("aws", "sample@v1"),
        name: name.into(),
        provider: CloudProvider::Aws,
        category: "Compute".into(),
        kind,
        pack_version: "v1".into(),
        source_path: "sample.svg".into(),
        svg: std::sync::Arc::from("<svg/>"),
    }
}

#[test]
fn every_aws_icon_gets_a_defined_resolution_and_overrides_are_pack_specific() {
    let catalog = Catalog::builtin();
    let bucket = icon("Amazon S3", IconKind::Service);
    assert!(matches!(
        catalog.resolve_icon(&bucket, None).unwrap(),
        IconAction::Configure { .. }
    ));
    assert!(matches!(
        catalog
            .resolve_icon(&icon("Region", IconKind::Group), None)
            .unwrap(),
        IconAction::Context {
            context: DeploymentContext::Region
        }
    ));
    assert!(matches!(
        catalog
            .resolve_icon(
                &icon("Unrecognized new AWS artwork", IconKind::Resource),
                None
            )
            .unwrap(),
        IconAction::SelectResource { .. }
    ));
    let action = IconAction::Configure {
        choices: vec![
            catalog
                .find(TerraformProvider::Aws, "aws_sqs_queue")
                .unwrap()
                .choice(),
        ],
    };
    let mappings = IconMappings {
        format_version: 1,
        bindings: BTreeMap::from([(bucket.reference.clone(), action.clone())]),
    };
    assert_eq!(
        catalog.resolve_icon(&bucket, Some(&mappings)).unwrap(),
        action
    );
    let mut newer = bucket;
    newer.reference = ShapeRef::new("aws", "sample@v2");
    newer.pack_version = "v2".into();
    assert_ne!(
        catalog.resolve_icon(&newer, Some(&mappings)).unwrap(),
        action
    );
}

struct Scratch(std::path::PathBuf);
impl Scratch {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!("blueprint-schema-{}", ElementId::new()));
        std::fs::create_dir(&p).unwrap();
        Self(p)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn draft_diagnostics_detect_cycles_unresolved_cross_page_and_missing_schema_targets() {
    let mut doc = Document::new();
    let page = doc.first_page().unwrap();
    doc.pages.get_mut(&page).unwrap().diagram_kind = Some(DiagramKind::Cloud);
    let parent = Parent::Layer(doc.layers_of(page)[0].id);
    let make = || {
        Element::shape(
            ShapeRef::new("basic", "rectangle"),
            parent,
            OrderKey::first(),
            kurbo::Rect::new(0., 0., 100., 100.),
        )
    };
    let mut first = make();
    let mut second = make();
    let mut a = draft("aws_s3_bucket");
    let mut b = a.clone();
    for resource in [&mut a, &mut b] {
        resource
            .properties
            .insert("bucket".into(), ResourceValue::Literal(json!("example")));
    }
    a.depends_on.push(ResourceTarget {
        element: Some(second.id),
        resource: "primary".into(),
    });
    b.depends_on.push(ResourceTarget {
        element: Some(first.id),
        resource: "primary".into(),
    });
    first.as_shape_mut().unwrap().infrastructure =
        Some(AwsResource::Resources { resources: vec![a] });
    second.as_shape_mut().unwrap().infrastructure =
        Some(AwsResource::Resources { resources: vec![b] });
    let first_id = first.id;
    let second_id = second.id;
    doc.elements.insert(first_id, first);
    doc.elements.insert(second_id, second);
    let check = |doc: &Document, schemas: &[SchemaSnapshot]| {
        bp_deploy::drafts::validate_page(doc, page, Catalog::builtin(), schemas)
    };
    assert_eq!(
        check(&doc, &[snapshot()])
            .iter()
            .filter(|error| error.message.contains("cycle"))
            .count(),
        2
    );
    assert!(
        check(&doc, &[])
            .iter()
            .any(|error| error.message.contains("Missing verified"))
    );
    let AwsResource::Resources { resources } = doc
        .elements
        .get_mut(&first_id)
        .unwrap()
        .as_shape_mut()
        .unwrap()
        .infrastructure
        .as_mut()
        .unwrap()
    else {
        unreachable!()
    };
    resources[0].depends_on.clear();
    assert!(check(&doc, &[snapshot()]).is_empty());
    let target_before = doc.elements.remove(&first_id).unwrap();
    assert!(
        check(&doc, &[snapshot()])
            .iter()
            .any(|error| error.message.contains("configured resource on this page"))
    );
    doc.elements.insert(first_id, target_before);
    let AwsResource::Resources { resources } = doc
        .elements
        .get_mut(&second_id)
        .unwrap()
        .as_shape_mut()
        .unwrap()
        .infrastructure
        .as_mut()
        .unwrap()
    else {
        unreachable!()
    };
    resources[0].depends_on[0].element = None;
    assert!(
        check(&doc, &[snapshot()])
            .iter()
            .any(|error| error.message.contains("Choose a target"))
    );
    assert!(
        bp_deploy::configuration(&doc, page)
            .unwrap_err()
            .contains("target first")
    );
    doc.pages.get_mut(&page).unwrap().deployment = Some(AwsDeployment {
        region: "us-east-1".into(),
        profile: String::new(),
        account_id: "123456789012".into(),
    });
    assert!(
        bp_deploy::configuration(&doc, page)
            .unwrap_err()
            .contains("Foundation checkpoint")
    );
}

#[test]
fn offline_cache_rejects_missing_corrupt_and_wrong_release_snapshots() {
    let scratch = Scratch::new();
    let cache = SchemaCache {
        root: scratch.0.clone(),
    };
    assert!(cache.load(&pin()).is_err());
    let directory = scratch.0.join("aws/6.68.0");
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("schema.json");
    std::fs::write(&path, serde_json::to_vec(&snapshot()).unwrap()).unwrap();
    assert_eq!(cache.load(&pin()).unwrap(), snapshot());
    assert!(
        cache
            .load(&ProviderPin {
                version: "6.67.0".into(),
                ..pin()
            })
            .is_err()
    );
    std::fs::write(path, b"broken JSON").unwrap();
    assert!(cache.load(&pin()).is_err());
}

#[cfg(unix)]
#[test]
fn discovery_only_initializes_and_reads_schemas_then_reuses_the_verified_cache() {
    use std::os::unix::fs::PermissionsExt;
    let scratch = Scratch::new();
    let version = scratch.0.join("version.json");
    let schema = scratch.0.join("schema.json");
    std::fs::write(&version, VERSION).unwrap();
    std::fs::write(&schema, SCHEMA).unwrap();
    let executable = scratch.0.join("terraform-mock");
    let log = scratch.0.join("commands");
    let quote = |p: &std::path::Path| format!("'{}'", p.to_string_lossy().replace('\'', "'\\''"));
    std::fs::write(&executable,format!("#!/bin/sh\necho \"$*\" >> {}\ncase \"$1 $2 $3\" in\n 'init -backend=false -input=false') echo initialized;;\n 'version -json ') cat {};;\n 'providers schema -json') cat {};;\n *) exit 83;;\nesac\n",quote(&log),quote(&version),quote(&schema))).unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
    let cache = SchemaCache {
        root: scratch.0.join("cache"),
    };
    let first = cache.discover(&executable, &pin()).unwrap();
    assert_eq!(first, snapshot());
    assert_eq!(cache.discover(&executable, &pin()).unwrap(), first);
    assert_eq!(
        std::fs::read_to_string(log)
            .unwrap()
            .lines()
            .collect::<Vec<_>>(),
        [
            "init -backend=false -input=false -no-color",
            "version -json",
            "providers schema -json"
        ]
    );
    let link = scratch.0.join("linked");
    std::os::unix::fs::symlink(&cache.root, &link).unwrap();
    assert!(SchemaCache { root: link }.load(&pin()).is_err());
}

#[test]
fn provider_aliases_are_separate_searchable_and_do_not_hide_unexpected_resource_types() {
    let catalog = Catalog::builtin();
    assert_eq!(catalog.entries().count(), 3388);
    assert_eq!(catalog.aliases().count(), 6);
    let alias = catalog.find(TerraformProvider::Aws, "aws_alb").unwrap();
    assert_eq!(alias.terraform_resource_type, "aws_alb");
    assert_eq!(
        catalog
            .search(Some(TerraformProvider::Aws), "aws_alb")
            .len(),
        6
    );
    let mut snapshot = snapshot();
    let sample = snapshot.provider_schema["resource_schemas"]["aws_s3_bucket"].clone();
    for entry in catalog.aliases() {
        snapshot.provider_schema["resource_schemas"][&entry.terraform_resource_type] =
            sample.clone();
    }
    snapshot.provider_schema["resource_schemas"]["aws_surprise_resource"] = sample;
    let report = snapshot.coverage(catalog).unwrap();
    assert_eq!(report.recognized_aliases.len(), 6);
    assert_eq!(report.additional, ["aws_surprise_resource"]);
    assert_eq!(report.listed, 1724);
}

#[test]
fn schema_accepts_omitted_empty_nested_attributes_without_rewriting_the_payload() {
    let mut schema: serde_json::Value = serde_json::from_slice(SCHEMA).unwrap();
    let attribute = json!({
        "computed": true,
        "nested_type": { "nesting_mode": "single" }
    });
    let provider = &mut schema["provider_schemas"][pin().provider.address()];
    let attributes = &mut provider["resource_schemas"]["aws_instance"]["block"]["attributes"];
    attributes["empty_nested"] = attribute.clone();
    let snapshot =
        SchemaSnapshot::from_outputs(&pin(), VERSION, &serde_json::to_vec(&schema).unwrap()).unwrap();
    let resource = snapshot.resource("aws_instance").unwrap();
    let attribute_schema = &resource.block.attributes["empty_nested"];
    let nested = attribute_schema.nested_type.as_ref().unwrap();
    assert!(nested.attributes.is_empty());
    let raw = &snapshot.provider_schema["resource_schemas"]["aws_instance"]["block"]["attributes"];
    assert_eq!(raw["empty_nested"], attribute);
    assert_eq!(
        snapshot
            .output_type("aws_instance", &[OutputStep::Attribute("empty_nested".into())])
            .unwrap(),
        Some(json!(["object", {}]))
    );
}
