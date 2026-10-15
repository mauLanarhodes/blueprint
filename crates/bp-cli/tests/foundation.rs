use std::process::{Command, Output};

fn cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_blueprint-cli"))
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn catalog_cli_reports_pinned_inventory_and_filters_without_deployment_claims() {
    let output = cli(&[
        "aws-catalog",
        "--provider",
        "awscc",
        "--search",
        "s3 bucket",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let entries = value["matches"].as_array().unwrap();
    assert!(!entries.is_empty());
    assert!(
        entries
            .iter()
            .all(|entry| entry["provider"] == "awscc" && entry["provider_version"] == "1.105.0")
    );
    assert!(value["notice"].as_str().unwrap().contains("not claims"));
    assert!(
        !cli(&["aws-catalog", "--provider", "unknown"])
            .status
            .success()
    );
}

#[test]
fn offline_schema_check_never_creates_a_cache_or_installs_a_provider() {
    let path =
        std::env::temp_dir().join(format!("blueprint-offline-{}", bp_model::ElementId::new()));
    let output = cli(&["aws-schema", "check", "aws", path.to_str().unwrap()]);
    assert!(!output.status.success());
    assert!(!path.exists());
    let output = cli(&[
        "aws-schema",
        "check",
        "aws",
        path.to_str().unwrap(),
        "--terraform",
        "must-not-run",
    ]);
    assert!(!output.status.success());
    assert!(!path.exists());
}
