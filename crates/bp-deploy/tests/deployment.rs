#[cfg(unix)]
use bp_deploy::Terraform;
use bp_deploy::{Configuration, configuration};
use bp_model::kurbo::Rect;
use bp_model::{
    AwsDeployment, AwsResource, DiagramKind, Document, Element, ElementId, OrderKey, Parent,
    ShapeRef,
};
use serde_json::{Value, json};
#[cfg(unix)]
use std::path::PathBuf;

fn network() -> (Document, ElementId, ElementId) {
    let mut doc = Document::new();
    let page = doc.first_page().unwrap();
    let data = doc.pages.get_mut(&page).unwrap();
    data.diagram_kind = Some(DiagramKind::Cloud);
    data.deployment = Some(AwsDeployment {
        account_id: "123456789012".into(),
        ..Default::default()
    });
    let parent = Parent::Layer(doc.layers_of(page)[0].id);
    let mut vpc = Element::shape(
        ShapeRef::new("basic", "rectangle"),
        parent,
        OrderKey::first(),
        Rect::new(0.0, 0.0, 100.0, 100.0),
    );
    let vpc_id = vpc.id;
    vpc.as_shape_mut().unwrap().text = "VPC".into();
    vpc.as_shape_mut().unwrap().infrastructure = Some(AwsResource::Vpc {
        cidr: "10.0.0.0/16".into(),
    });
    let mut subnet = Element::shape(
        ShapeRef::new("basic", "rectangle"),
        parent,
        OrderKey::after(&vpc.order),
        Rect::new(200.0, 0.0, 300.0, 100.0),
    );
    let subnet_id = subnet.id;
    subnet.as_shape_mut().unwrap().infrastructure = Some(AwsResource::Subnet {
        vpc: Some(vpc_id),
        cidr: "10.0.1.0/24".into(),
        availability_zone: String::new(),
    });
    doc.elements.insert(vpc.id, vpc);
    doc.elements.insert(subnet.id, subnet);
    (doc, vpc_id, subnet_id)
}

fn config(doc: &Document) -> Result<Configuration, String> {
    configuration(doc, doc.first_page().unwrap())
}

#[test]
fn generated_network_has_stable_dependencies_private_subnets_and_account_guard() {
    let (mut doc, vpc, _) = network();
    let original = config(&doc).unwrap();
    let value: Value = serde_json::from_str(&original.json).unwrap();
    assert_eq!(
        value["provider"]["aws"]["allowed_account_ids"],
        json!(["123456789012"])
    );
    let subnets = value["resource"]["aws_subnet"].as_object().unwrap();
    let subnet = subnets.values().next().unwrap();
    assert_eq!(subnet["map_public_ip_on_launch"], false);
    assert!(
        subnet["vpc_id"]
            .as_str()
            .unwrap()
            .contains(&vpc.to_string().replace('-', ""))
    );
    assert!(value["resource"].get("aws_internet_gateway").is_none());
    doc.elements
        .get_mut(&vpc)
        .unwrap()
        .as_shape_mut()
        .unwrap()
        .bounds = Rect::new(5.0, 5.0, 80.0, 80.0);
    assert_eq!(
        config(&doc).unwrap().json,
        original.json,
        "moving shapes must not recreate infrastructure"
    );
}

#[test]
fn rejects_unconfigured_targets_missing_dependencies_bad_cidrs_and_cross_page_refs() {
    let (doc, _, subnet) = network();
    let page = doc.first_page().unwrap();
    let mut changed = doc.clone();
    changed
        .pages
        .get_mut(&page)
        .unwrap()
        .deployment
        .as_mut()
        .unwrap()
        .account_id = "unknown".into();
    assert!(config(&changed).unwrap_err().contains("12-digit"));
    for (vpc, cidr, expected) in [
        (None, "10.0.1.0/24", "choose its VPC"),
        (Some(ElementId::new()), "10.0.1.0/24", "on this page"),
        (
            Some(
                doc.elements
                    .keys()
                    .copied()
                    .find(|id| *id != subnet)
                    .unwrap(),
            ),
            "10.0.1.9/24",
            "aligned",
        ),
        (
            Some(
                doc.elements
                    .keys()
                    .copied()
                    .find(|id| *id != subnet)
                    .unwrap(),
            ),
            "192.168.1.0/24",
            "outside VPC",
        ),
    ] {
        let mut changed = doc.clone();
        changed
            .elements
            .get_mut(&subnet)
            .unwrap()
            .as_shape_mut()
            .unwrap()
            .infrastructure = Some(AwsResource::Subnet {
            vpc,
            cidr: cidr.into(),
            availability_zone: String::new(),
        });
        assert!(config(&changed).unwrap_err().contains(expected));
    }
    let mut changed = doc.clone();
    let mut duplicate = changed.elements[&subnet].clone();
    duplicate.id = ElementId::new();
    changed.elements.insert(duplicate.id, duplicate);
    assert!(config(&changed).unwrap_err().contains("overlaps"));
    let mut changed = doc;
    changed.pages.get_mut(&page).unwrap().diagram_kind = Some(DiagramKind::Flowchart);
    assert!(config(&changed).unwrap_err().contains("Cloud architecture"));
}

#[test]
fn labels_and_profiles_are_literals_and_configuration_survives_both_formats() {
    let (mut doc, vpc, _) = network();
    doc.elements
        .get_mut(&vpc)
        .unwrap()
        .as_shape_mut()
        .unwrap()
        .text = "${file(\"secret\")} %{ if true }".into();
    let page = doc.first_page().unwrap();
    doc.pages
        .get_mut(&page)
        .unwrap()
        .deployment
        .as_mut()
        .unwrap()
        .profile = "${danger}".into();
    let generated = config(&doc).unwrap();
    let value: Value = serde_json::from_str(&generated.json).unwrap();
    assert_eq!(value["provider"]["aws"]["profile"], "$${danger}");
    let vpc = value["resource"]["aws_vpc"]
        .as_object()
        .unwrap()
        .values()
        .next()
        .unwrap();
    assert_eq!(vpc["tags"]["Name"], "$${file(\"secret\")} %%{ if true }");
    for bytes in [
        bp_io::to_json_bytes(&doc).unwrap(),
        bp_io::to_zip_bytes(&doc).unwrap(),
    ] {
        let loaded = bp_io::from_bytes(&bytes).unwrap();
        assert_eq!(loaded, doc);
        assert_eq!(config(&loaded).unwrap().json, generated.json);
    }
}

fn trial() -> Document {
    bp_deploy::trial::document(AwsDeployment {
        account_id: "123456789012".into(),
        ..Default::default()
    })
}

#[test]
fn trial_generates_private_bucket_and_a_topic_scoped_queue_subscription() {
    let generated = config(&trial()).unwrap();
    let value: Value = serde_json::from_str(&generated.json).unwrap();
    assert_eq!(generated.resources.len(), 4);
    assert!(generated.warnings.is_empty());
    let resources = &value["resource"];
    assert_eq!(
        resources
            .as_object()
            .unwrap()
            .values()
            .map(|v| v.as_object().unwrap().len())
            .sum::<usize>(),
        8
    );
    let (queue, _) = resources["aws_sqs_queue"]
        .as_object()
        .unwrap()
        .iter()
        .next()
        .unwrap();
    let (topic, _) = resources["aws_sns_topic"]
        .as_object()
        .unwrap()
        .iter()
        .next()
        .unwrap();
    let queue_arn = format!("${{aws_sqs_queue.{queue}.arn}}");
    let topic_arn = format!("${{aws_sns_topic.{topic}.arn}}");
    let subscription = &resources["aws_sns_topic_subscription"][queue];
    assert_eq!(subscription["endpoint"], queue_arn);
    assert_eq!(subscription["topic_arn"], topic_arn);
    assert_eq!(
        subscription["depends_on"],
        json!([format!("aws_sqs_queue_policy.{queue}")])
    );
    let policy: Value = serde_json::from_str(
        resources["aws_sqs_queue_policy"][queue]["policy"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(policy["Version"], "2012-10-17");
    assert_eq!(policy["Statement"][0]["Resource"], queue_arn);
    assert_eq!(
        policy["Statement"][0]["Condition"]["ArnEquals"]["aws:SourceArn"],
        topic_arn
    );
    assert_eq!(
        policy["Statement"][0]["Principal"]["Service"],
        "sns.amazonaws.com"
    );
    let bucket = resources["aws_s3_bucket"]
        .as_object()
        .unwrap()
        .values()
        .next()
        .unwrap();
    assert_eq!(bucket["force_destroy"], false);
    let block = resources["aws_s3_bucket_public_access_block"]
        .as_object()
        .unwrap()
        .values()
        .next()
        .unwrap();
    for field in [
        "block_public_acls",
        "block_public_policy",
        "ignore_public_acls",
        "restrict_public_buckets",
    ] {
        assert_eq!(block[field], true);
    }
}

#[test]
fn trial_settings_round_trip_and_legacy_network_migration_keeps_resource_identities() {
    let doc = trial();
    for bytes in [
        bp_io::to_json_bytes(&doc).unwrap(),
        bp_io::to_zip_bytes(&doc).unwrap(),
    ] {
        let loaded = bp_io::from_bytes(&bytes).unwrap();
        assert_eq!(loaded, doc);
        assert_eq!(config(&loaded).unwrap().json, config(&doc).unwrap().json);
    }
    let doc = network().0;
    let expected = config(&doc).unwrap();
    let mut legacy = serde_json::to_value(&doc).unwrap();
    legacy["schema_version"] = 6.into();
    let loaded = bp_io::from_bytes(&serde_json::to_vec(&legacy).unwrap()).unwrap();
    assert_eq!(loaded.schema_version, bp_model::SCHEMA_VERSION);
    assert_eq!(loaded.elements, doc.elements);
    assert_eq!(config(&loaded).unwrap().json, expected.json);
}

#[test]
fn trial_rejects_invalid_service_names_and_settings_before_terraform() {
    use bp_model::AwsResourceKind;
    for (kind, invalid, message) in [
        (
            AwsResourceKind::S3Bucket,
            AwsResource::S3Bucket {
                name: "Invalid Bucket".into(),
                versioning: false,
            },
            "Bucket name",
        ),
        (
            AwsResourceKind::DynamoDbTable,
            AwsResource::DynamoDbTable {
                name: "table".into(),
                partition_key: "id".into(),
                sort_key: "id".into(),
            },
            "different names",
        ),
        (
            AwsResourceKind::SqsQueue,
            AwsResource::SqsQueue {
                name: "queue".into(),
                visibility_timeout: 43_201,
                retention_seconds: 60,
                topic: None,
            },
            "Visibility",
        ),
        (
            AwsResourceKind::SqsQueue,
            AwsResource::SqsQueue {
                name: "queue".into(),
                visibility_timeout: 30,
                retention_seconds: 59,
                topic: None,
            },
            "retention",
        ),
        (
            AwsResourceKind::SqsQueue,
            AwsResource::SqsQueue {
                name: "queue".into(),
                visibility_timeout: 30,
                retention_seconds: 60,
                topic: Some(ElementId::new()),
            },
            "on this page",
        ),
        (
            AwsResourceKind::SnsTopic,
            AwsResource::SnsTopic {
                name: "topic.fifo".into(),
            },
            "Name must",
        ),
    ] {
        let mut doc = trial();
        let element = doc
            .elements
            .values_mut()
            .find(|e| {
                e.as_shape()
                    .and_then(|s| s.infrastructure.as_ref())
                    .is_some_and(|r| r.kind() == kind)
            })
            .unwrap();
        element.as_shape_mut().unwrap().infrastructure = Some(invalid);
        assert!(config(&doc).unwrap_err().contains(message));
    }
    let mut doc = trial();
    let mut copy = doc
        .elements
        .values()
        .find(|e| {
            matches!(
                e.as_shape().and_then(|s| s.infrastructure.as_ref()),
                Some(AwsResource::S3Bucket { .. })
            )
        })
        .unwrap()
        .clone();
    copy.id = ElementId::new();
    copy.order = OrderKey::after(&copy.order);
    doc.elements.insert(copy.id, copy);
    assert!(config(&doc).unwrap_err().contains("unique name"));
}

#[test]
fn unlinking_a_queue_removes_only_its_subscription_and_keys_remain_literal() {
    let mut doc = trial();
    let before: Value = serde_json::from_str(&config(&doc).unwrap().json).unwrap();
    for element in doc.elements.values_mut() {
        match element
            .as_shape_mut()
            .and_then(|s| s.infrastructure.as_mut())
        {
            Some(AwsResource::SqsQueue { topic, .. }) => *topic = None,
            Some(AwsResource::DynamoDbTable {
                partition_key,
                sort_key,
                ..
            }) => {
                *partition_key = "${file(\"literal\")}".into();
                *sort_key = "%{ literal }".into();
            }
            _ => {}
        }
    }
    let value: Value = serde_json::from_str(&config(&doc).unwrap().json).unwrap();
    assert_eq!(
        value["resource"]["aws_sqs_queue"],
        before["resource"]["aws_sqs_queue"]
    );
    assert!(value["resource"].get("aws_sqs_queue_policy").is_none());
    assert!(
        value["resource"]
            .get("aws_sns_topic_subscription")
            .is_none()
    );
    let table = value["resource"]["aws_dynamodb_table"]
        .as_object()
        .unwrap()
        .values()
        .next()
        .unwrap();
    assert_eq!(table["hash_key"], "$${file(\"literal\")}");
    assert_eq!(table["range_key"], "%%{ literal }");
}

#[cfg(unix)]
#[test]
fn trial_uses_the_reviewed_plan_runner_for_all_supporting_resources() {
    let scratch = Scratch::new();
    let generated = config(&trial()).unwrap();
    let value: Value = serde_json::from_str(&generated.json).unwrap();
    let (runner, workspace) = mock(&scratch, &generated);
    let changes: Vec<_> = value["resource"].as_object().unwrap().iter().flat_map(|(kind, instances)| {
        instances.as_object().unwrap().keys().map(move |name| json!({"address": format!("{kind}.{name}"), "change": {"actions": ["create"]}}))
    }).collect();
    std::fs::write(
        workspace.join("mock-plan.json"),
        json!({
            "planned_values": {"outputs": {"aws_account_id": {"value": generated.account_id}}},
            "resource_changes": changes
        })
        .to_string(),
    )
    .unwrap();
    let plan = runner.plan(generated, &scratch.0, |_| {}).unwrap();
    assert_eq!(plan.changes.len(), 8);
    assert!(!workspace.join("applied").exists());
    runner.apply(plan, |_| {}).unwrap();
    assert!(workspace.join("applied").exists());
}

#[cfg(unix)]
struct Scratch(PathBuf);
#[cfg(unix)]
impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("blueprint-deploy-test-{}", ElementId::new()));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
#[cfg(unix)]
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(unix)]
fn mock(scratch: &Scratch, config: &Configuration) -> (Terraform, PathBuf) {
    use std::os::unix::fs::PermissionsExt;
    let binary = scratch.0.join("terraform-test");
    std::fs::write(&binary, r#"#!/bin/sh
printf '%s\n' "$*" >> commands.log
if [ -f "fail-$1" ]; then printf 'deliberate failure' >&2; exit 1; fi
case "$1" in
  init|validate) : ;;
  workspace) printf 'default\n' ;;
  plan) printf 'reviewed-plan-bytes' > blueprint.tfplan ;;
  show) if [ "$2" = '-json' ]; then cat mock-plan.json; else printf 'Plan: 1 to add, 0 to change, 0 to destroy.\n'; fi ;;
  apply) touch applied; printf 'Apply complete! Resources: 1 added.\n' ;;
  *) exit 1 ;;
esac
"#).unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
    let workspace = scratch.0.join(format!("blueprint-{}", config.page));
    std::fs::create_dir(&workspace).unwrap();
    std::fs::write(
        workspace.join(".blueprint-workspace"),
        config.page.to_string(),
    )
    .unwrap();
    std::fs::write(
        workspace.join("mock-plan.json"),
        json!({
            "planned_values": { "outputs": { "aws_account_id": {"value": config.account_id} } },
            "resource_changes": [{"address": "aws_vpc.network", "change": {"actions": ["create"]}}]
        })
        .to_string(),
    )
    .unwrap();
    (Terraform { executable: binary }, workspace)
}

#[cfg(unix)]
#[test]
fn planning_never_applies_and_deploy_uses_the_saved_reviewed_plan() {
    let scratch = Scratch::new();
    let configuration = config(&network().0).unwrap();
    let (runner, workspace) = mock(&scratch, &configuration);
    let plan = runner.plan(configuration, &scratch.0, |_| {}).unwrap();
    assert_eq!(plan.changes, ["aws_vpc.network: create"]);
    assert!(!workspace.join("applied").exists());
    runner.apply(plan, |_| {}).unwrap();
    let calls = std::fs::read_to_string(workspace.join("commands.log")).unwrap();
    assert!(calls.contains("plan -input=false -no-color -lock-timeout=60s -out=blueprint.tfplan"));
    assert!(calls.contains("apply -input=false -no-color -lock-timeout=60s blueprint.tfplan"));
    assert!(workspace.join("applied").exists());
    assert!(!workspace.join("blueprint.tfplan").exists());
    assert!(!workspace.join(".blueprint-operation").exists());
}

#[cfg(unix)]
#[test]
fn changed_plan_configuration_and_extra_terraform_sources_prevent_apply() {
    for filename in ["main.tf.json", "blueprint.tfplan", "unexpected.tf"] {
        let scratch = Scratch::new();
        let configuration = config(&network().0).unwrap();
        let (runner, workspace) = mock(&scratch, &configuration);
        let plan = runner.plan(configuration, &scratch.0, |_| {}).unwrap();
        std::fs::write(workspace.join(filename), "changed").unwrap();
        assert!(runner.apply(plan, |_| {}).is_err());
        assert!(!workspace.join("applied").exists());
    }
}

#[cfg(unix)]
#[test]
fn failures_account_mismatch_and_workspace_locks_never_offer_an_apply() {
    for failure in ["init", "validate", "plan", "account", "lock"] {
        let scratch = Scratch::new();
        let configuration = config(&network().0).unwrap();
        let (runner, workspace) = mock(&scratch, &configuration);
        match failure {
            "account" => {
                std::fs::write(workspace.join("mock-plan.json"), "{}").unwrap();
            }
            "lock" => {
                std::fs::write(workspace.join(".blueprint-operation"), "busy").unwrap();
            }
            stage => {
                std::fs::write(workspace.join(format!("fail-{stage}")), "").unwrap();
            }
        }
        assert!(runner.plan(configuration, &scratch.0, |_| {}).is_err());
        assert!(!workspace.join("applied").exists());
    }
}

#[cfg(unix)]
#[test]
fn failed_apply_consumes_plan_and_preserves_state_for_the_next_preview() {
    let scratch = Scratch::new();
    let configuration = config(&network().0).unwrap();
    let (runner, workspace) = mock(&scratch, &configuration);
    let plan = runner.plan(configuration, &scratch.0, |_| {}).unwrap();
    std::fs::write(workspace.join("terraform.tfstate"), "existing state").unwrap();
    std::fs::write(workspace.join("fail-apply"), "").unwrap();
    assert!(
        runner
            .apply(plan, |_| {})
            .unwrap_err()
            .contains("deliberate failure")
    );
    assert!(!workspace.join("blueprint.tfplan").exists());
    assert_eq!(
        std::fs::read_to_string(workspace.join("terraform.tfstate")).unwrap(),
        "existing state"
    );
    assert!(!workspace.join(".blueprint-operation").exists());
}

#[cfg(unix)]
#[test]
fn workspace_symlinks_are_rejected_without_writing_the_target() {
    let scratch = Scratch::new();
    let target = Scratch::new();
    let configuration = config(&network().0).unwrap();
    let workspace = scratch.0.join(format!("blueprint-{}", configuration.page));
    std::os::unix::fs::symlink(&target.0, workspace).unwrap();
    assert!(
        Terraform::default()
            .plan(configuration, &scratch.0, |_| {})
            .is_err()
    );
    assert_eq!(std::fs::read_dir(&target.0).unwrap().count(), 0);
}
