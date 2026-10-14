use bp_deploy::{Configuration, Terraform, configuration};
use bp_model::kurbo::Rect;
use bp_model::{
    AwsDeployment, AwsResource, DiagramKind, Document, Element, ElementId, OrderKey, Parent,
    ShapeRef,
};
use serde_json::{Value, json};
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

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("blueprint-deploy-test-{}", ElementId::new()));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
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
