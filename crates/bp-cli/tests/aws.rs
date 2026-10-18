use std::process::Command;

#[test]
fn trial_project_exports_complete_terraform_without_running_aws() {
    let root = std::env::temp_dir().join(format!(
        "blueprint-trial-cli-{}",
        bp_model::ElementId::new()
    ));
    std::fs::create_dir(&root).unwrap();
    let project = root.join("trial.blueprint");
    let output = root.join("main.tf.json");
    let cli = env!("CARGO_BIN_EXE_blueprint-cli");
    let created = Command::new(cli)
        .args([
            "aws-trial",
            project.to_str().unwrap(),
            "--account",
            "123456789012",
        ])
        .output()
        .unwrap();
    assert!(
        created.status.success(),
        "{}",
        String::from_utf8_lossy(&created.stderr)
    );
    let generated = Command::new(cli)
        .args([
            "aws-export",
            project.to_str().unwrap(),
            output.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        generated.status.success(),
        "{}",
        String::from_utf8_lossy(&generated.stderr)
    );
    let doc = bp_io::load(&project).unwrap();
    let expected = bp_deploy::configuration(&doc, doc.first_page().unwrap()).unwrap();
    assert_eq!(std::fs::read_to_string(&output).unwrap(), expected.json);
    assert_eq!(expected.resources.len(), 4);
    let original = std::fs::read(&project).unwrap();
    let again = Command::new(cli)
        .args(["aws-trial", project.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!again.status.success());
    assert_eq!(std::fs::read(&project).unwrap(), original);
    assert!(!root.join(".terraform").exists());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn draft_trial_requires_an_account_before_export_and_invalid_flags_fail() {
    let root = std::env::temp_dir().join(format!(
        "blueprint-trial-cli-{}",
        bp_model::ElementId::new()
    ));
    std::fs::create_dir(&root).unwrap();
    let project = root.join("draft.blueprint.json");
    let output = root.join("main.tf.json");
    let cli = env!("CARGO_BIN_EXE_blueprint-cli");
    assert!(
        Command::new(cli)
            .args(["aws-trial", project.to_str().unwrap()])
            .status()
            .unwrap()
            .success()
    );
    let result = Command::new(cli)
        .args([
            "aws-export",
            project.to_str().unwrap(),
            output.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("12-digit"));
    assert!(!output.exists());
    let result = Command::new(cli)
        .args([
            "aws-trial",
            root.join("invalid.blueprint").to_str().unwrap(),
            "--account",
        ])
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("needs a value"));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn lambda_trial_exports_the_code_package_beside_terraform_json() {
    let root = std::env::temp_dir().join(format!(
        "blueprint-lambda-cli-{}",
        bp_model::ElementId::new()
    ));
    std::fs::create_dir(&root).unwrap();
    let project = root.join("worker.blueprint.json");
    let output = root.join("main.tf.json");
    let cli = env!("CARGO_BIN_EXE_blueprint-cli");
    let created = Command::new(cli)
        .args([
            "aws-trial",
            project.to_str().unwrap(),
            "--lambda",
            "--account",
            "123456789012",
        ])
        .output()
        .unwrap();
    assert!(
        created.status.success(),
        "{}",
        String::from_utf8_lossy(&created.stderr)
    );
    let exported = Command::new(cli)
        .args([
            "aws-export",
            project.to_str().unwrap(),
            output.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        exported.status.success(),
        "{}",
        String::from_utf8_lossy(&exported.stderr)
    );
    let doc = bp_io::load(&project).unwrap();
    let expected = bp_deploy::configuration(&doc, doc.first_page().unwrap()).unwrap();
    assert_eq!(expected.resources.len(), 5);
    assert_eq!(expected.packages.len(), 1);
    assert_eq!(std::fs::read_to_string(&output).unwrap(), expected.json);
    for (name, bytes) in expected.packages {
        assert_eq!(std::fs::read(root.join(name)).unwrap(), bytes);
    }
    assert!(!root.join(".terraform").exists());
    std::fs::remove_dir_all(root).unwrap();
}
