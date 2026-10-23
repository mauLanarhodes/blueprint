use bp_deploy::{Configuration, configuration, write_configuration};
use bp_model::{AwsDeployment, AwsResource, AwsResourceKind, Document, ElementId};
use serde_json::{Value, json};
use std::io::{Cursor, Read};
use std::path::PathBuf;

fn trial() -> Document {
    bp_deploy::trial::document_with_lambda(AwsDeployment {
        account_id: "123456789012".into(),
        ..Default::default()
    })
}

fn config(doc: &Document) -> Result<Configuration, String> {
    configuration(doc, doc.first_page().unwrap())
}

fn lambda(doc: &mut Document) -> &mut AwsResource {
    doc.elements
        .values_mut()
        .filter_map(|e| e.as_shape_mut()?.infrastructure.as_mut())
        .find(|r| matches!(r, AwsResource::LambdaFunction { .. }))
        .unwrap()
}

fn module(bytes: &[u8]) -> String {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    assert_eq!(archive.len(), 1);
    let mut source = String::new();
    archive
        .by_name("lambda_function.py")
        .unwrap()
        .read_to_string(&mut source)
        .unwrap();
    source
}

#[test]
fn lambda_packages_exact_source_and_scopes_permissions_to_its_queue_and_logs() {
    let mut doc = trial();
    let expected = match lambda(&mut doc) {
        AwsResource::LambdaFunction { source_code, .. } => source_code.clone(),
        _ => unreachable!(),
    };
    let generated = config(&doc).unwrap();
    assert_eq!(generated.resources.len(), 5);
    assert_eq!(generated.packages.len(), 1);
    assert_eq!(
        module(generated.packages.values().next().unwrap()),
        expected
    );
    assert_eq!(generated.packages, config(&doc).unwrap().packages);
    let value: Value = serde_json::from_str(&generated.json).unwrap();
    let resources = &value["resource"];
    assert_eq!(
        resources
            .as_object()
            .unwrap()
            .values()
            .map(|r| r.as_object().unwrap().len())
            .sum::<usize>(),
        13
    );
    let (name, function) = resources["aws_lambda_function"]
        .as_object()
        .unwrap()
        .iter()
        .next()
        .unwrap();
    let queue = resources["aws_sqs_queue"]
        .as_object()
        .unwrap()
        .keys()
        .next()
        .unwrap();
    let queue_arn = format!("${{aws_sqs_queue.{queue}.arn}}");
    assert_eq!(function["runtime"], "python3.13");
    assert_eq!(function["handler"], "lambda_function.lambda_handler");
    assert_eq!(function["timeout"], 5);
    assert_eq!(function["memory_size"], 128);
    assert_eq!(function["role"], format!("${{aws_iam_role.{name}.arn}}"));
    assert_eq!(
        function["depends_on"],
        json!([
            format!("aws_iam_role_policy.{name}"),
            format!("aws_cloudwatch_log_group.{name}")
        ])
    );
    let mapping = &resources["aws_lambda_event_source_mapping"][name];
    assert_eq!(mapping["event_source_arn"], queue_arn);
    assert_eq!(
        mapping["function_name"],
        format!("${{aws_lambda_function.{name}.arn}}")
    );
    assert_eq!(mapping["batch_size"], 10);
    assert_eq!(mapping["enabled"], true);
    let trust: Value = serde_json::from_str(
        resources["aws_iam_role"][name]["assume_role_policy"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        trust["Statement"][0]["Principal"],
        json!({"Service": "lambda.amazonaws.com"})
    );
    let policy: Value = serde_json::from_str(
        resources["aws_iam_role_policy"][name]["policy"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(policy["Statement"].as_array().unwrap().len(), 2);
    assert_eq!(
        policy["Statement"][0]["Resource"],
        format!("${{aws_cloudwatch_log_group.{name}.arn}}:*")
    );
    assert_eq!(
        policy["Statement"][0]["Action"],
        json!(["logs:CreateLogStream", "logs:PutLogEvents"])
    );
    assert_eq!(policy["Statement"][1]["Resource"], queue_arn);
    assert_eq!(
        policy["Statement"][1]["Action"],
        json!([
            "sqs:ReceiveMessage",
            "sqs:DeleteMessage",
            "sqs:GetQueueAttributes"
        ])
    );
    assert_eq!(
        resources["aws_cloudwatch_log_group"][name]["retention_in_days"],
        14
    );
}

#[test]
fn source_changes_invalidate_review_and_survive_native_and_json_round_trips() {
    let mut doc = trial();
    let before = config(&doc).unwrap();
    let source = "def lambda_handler(event, context):\n    return '${literal} %{ literal }'\n";
    if let AwsResource::LambdaFunction { source_code, .. } = lambda(&mut doc) {
        *source_code = source.into();
    }
    let after = config(&doc).unwrap();
    assert_eq!(
        before.json, after.json,
        "the file hash expression is stable"
    );
    assert!(
        !before.matches(&after),
        "review must include the archive bytes"
    );
    assert_eq!(module(after.packages.values().next().unwrap()), source);
    for bytes in [
        bp_io::to_zip_bytes(&doc).unwrap(),
        bp_io::to_json_bytes(&doc).unwrap(),
    ] {
        let loaded = bp_io::from_bytes(&bytes).unwrap();
        assert_eq!(loaded, doc);
        assert!(config(&loaded).unwrap().matches(&after));
    }
    let old = bp_deploy::trial::document(AwsDeployment {
        account_id: "123456789012".into(),
        ..Default::default()
    });
    let expected = config(&old).unwrap();
    let mut legacy = serde_json::to_value(&old).unwrap();
    legacy["schema_version"] = 7.into();
    let migrated = bp_io::from_bytes(&serde_json::to_vec(&legacy).unwrap()).unwrap();
    assert_eq!(migrated.schema_version, bp_model::SCHEMA_VERSION);
    assert!(config(&migrated).unwrap().matches(&expected));
}

#[test]
fn lambda_rejects_invalid_settings_and_queue_references_before_terraform() {
    for case in 0..10 {
        let mut doc = trial();
        let wrong_kind = doc
            .elements
            .values()
            .find(|e| {
                matches!(
                    e.as_shape().and_then(|s| s.infrastructure.as_ref()),
                    Some(AwsResource::S3Bucket { .. })
                )
            })
            .unwrap()
            .id;
        if let AwsResource::LambdaFunction {
            name,
            source_code,
            timeout_seconds,
            memory_mb,
            queue,
        } = lambda(&mut doc)
        {
            match case {
                0 => *name = "bad.name".into(),
                1 => source_code.clear(),
                2 => *source_code = "x\0".into(),
                3 => *source_code = "x".repeat(65_537),
                4 => *timeout_seconds = 0,
                5 => *timeout_seconds = 901,
                6 => *memory_mb = 127,
                7 => *memory_mb = 10_241,
                8 => *queue = Some(ElementId::new()),
                _ => *queue = Some(wrong_kind),
            }
        }
        assert!(config(&doc).is_err(), "invalid case {case}");
    }
    let mut doc = trial();
    if let AwsResource::LambdaFunction {
        timeout_seconds, ..
    } = lambda(&mut doc)
    {
        *timeout_seconds = 6;
    }
    assert!(config(&doc).unwrap_err().contains("36 seconds"));
    if let AwsResource::LambdaFunction { queue, .. } = lambda(&mut doc) {
        *queue = None;
    }
    let value: Value = serde_json::from_str(&config(&doc).unwrap().json).unwrap();
    assert!(
        value["resource"]
            .get("aws_lambda_event_source_mapping")
            .is_none()
    );
    let policy = value["resource"]["aws_iam_role_policy"]
        .as_object()
        .unwrap()
        .values()
        .next()
        .unwrap();
    let policy: Value = serde_json::from_str(policy["policy"].as_str().unwrap()).unwrap();
    assert_eq!(policy["Statement"].as_array().unwrap().len(), 1);
    assert_eq!(
        AwsResourceKind::suggest("AWS Lambda"),
        Some(AwsResourceKind::LambdaFunction)
    );
}

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("blueprint-lambda-test-{}", ElementId::new()));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn export_writes_archives_and_checks_all_paths_before_replacing_files() {
    let scratch = Scratch::new();
    let mut generated = config(&trial()).unwrap();
    let output = scratch.0.join("main.tf.json");
    write_configuration(&generated, &output, None).unwrap();
    assert_eq!(std::fs::read_to_string(&output).unwrap(), generated.json);
    let name = generated.packages.keys().next().unwrap().clone();
    assert_eq!(
        std::fs::read(scratch.0.join(&name)).unwrap(),
        generated.packages[&name]
    );
    std::fs::write(&output, "preserve config").unwrap();
    std::fs::write(scratch.0.join(&name), "preserve input").unwrap();
    assert!(
        write_configuration(&generated, &output, Some(&scratch.0.join(&name)))
            .unwrap_err()
            .contains("input project")
    );
    assert_eq!(std::fs::read_to_string(&output).unwrap(), "preserve config");
    assert_eq!(
        std::fs::read_to_string(scratch.0.join(&name)).unwrap(),
        "preserve input"
    );
    for invalid in [
        "../outside.zip",
        "blueprint-lambda-../outside.zip",
        "blueprint-lambda-C:\\outside.zip",
    ] {
        generated.packages = [(invalid.into(), vec![1])].into();
        assert!(write_configuration(&generated, &output, None).is_err());
        assert_eq!(std::fs::read_to_string(&output).unwrap(), "preserve config");
    }
}

#[test]
fn lambda_queue_must_be_on_the_same_page_and_function_names_must_be_unique() {
    let mut doc = trial();
    let queue = match lambda(&mut doc) {
        AwsResource::LambdaFunction { queue, .. } => queue.unwrap(),
        _ => unreachable!(),
    };
    let page = bp_model::Page::new(
        "Other page",
        bp_model::OrderKey::after(&doc.pages[&doc.first_page().unwrap()].order),
    );
    let layer = bp_model::Layer::new(page.id, "Other layer", bp_model::OrderKey::first());
    let parent = bp_model::Parent::Layer(layer.id);
    doc.pages.insert(page.id, page);
    doc.layers.insert(layer.id, layer);
    doc.elements.get_mut(&queue).unwrap().parent = parent;
    assert!(config(&doc).unwrap_err().contains("on this page"));

    let mut doc = trial();
    let mut copy = doc
        .elements
        .values()
        .find(|e| {
            matches!(
                e.as_shape().and_then(|s| s.infrastructure.as_ref()),
                Some(AwsResource::LambdaFunction { .. })
            )
        })
        .unwrap()
        .clone();
    copy.id = ElementId::new();
    copy.order = bp_model::OrderKey::after(&copy.order);
    doc.elements.insert(copy.id, copy);
    assert!(config(&doc).unwrap_err().contains("unique name"));
}
