use crate::Configuration;
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const CONFIG: &str = "main.tf.json";
const PLAN: &str = "blueprint.tfplan";
const OWNER: &str = ".blueprint-workspace";

pub struct Terraform {
    pub executable: PathBuf,
}

impl Default for Terraform {
    fn default() -> Self {
        Self {
            executable: "terraform".into(),
        }
    }
}

/// Only produced by a successful plan. Applying verifies the saved bytes and
/// configuration so the UI cannot accidentally apply a different plan.
pub struct Plan {
    pub configuration: Configuration,
    pub directory: PathBuf,
    pub review: String,
    pub changes: Vec<String>,
    pub destroys_resources: bool,
    bytes: Vec<u8>,
    packages: BTreeMap<String, Vec<u8>>,
}

impl Terraform {
    pub fn plan(
        &self,
        configuration: Configuration,
        root: &Path,
        progress: impl Fn(&str),
    ) -> Result<Plan, String> {
        let directory = prepare_workspace(root, &configuration)?;
        let _lock = WorkspaceLock::acquire(&directory)?;
        check_workspace(&directory, &configuration)?;
        crate::write_configuration(&configuration, &directory.join(CONFIG), None)?;
        // Never reuse a plan from a failed or interrupted planning attempt.
        if directory.join(PLAN).exists() {
            fs::remove_file(directory.join(PLAN)).map_err(|e| e.to_string())?;
        }
        for (stage, args) in [
            (
                "Initializing Terraform…",
                vec!["init", "-input=false", "-no-color"],
            ),
            (
                "Validating resource configuration…",
                vec!["validate", "-no-color"],
            ),
            (
                "Previewing AWS changes…",
                vec![
                    "plan",
                    "-input=false",
                    "-no-color",
                    "-lock-timeout=60s",
                    "-out=blueprint.tfplan",
                ],
            ),
        ] {
            progress(stage);
            self.run(&directory, &args)?;
        }
        if self.run(&directory, &["workspace", "show"])?.trim() != "default" {
            return Err("This page must use the default Terraform workspace".into());
        }
        let review = self.run(&directory, &["show", "-no-color", PLAN])?;
        let json = self.run(&directory, &["show", "-json", PLAN])?;
        let value: Value = serde_json::from_str(&json)
            .map_err(|e| format!("Invalid Terraform plan response: {e}"))?;
        let account = value
            .pointer("/planned_values/outputs/aws_account_id/value")
            .and_then(Value::as_str);
        if account != Some(configuration.account_id.as_str()) {
            return Err(
                "The plan did not verify the requested AWS account ID; no deployment is available"
                    .into(),
            );
        }
        let mut changes = Vec::new();
        let mut destroys_resources = false;
        for resource in value["resource_changes"].as_array().into_iter().flatten() {
            let actions: Vec<_> = resource
                .pointer("/change/actions")
                .and_then(Value::as_array)
                .ok_or("Terraform returned a change without actions")?
                .iter()
                .map(|a| a.as_str().ok_or("Terraform returned an invalid action"))
                .collect::<Result<_, _>>()?;
            if actions
                .iter()
                .any(|a| !matches!(*a, "no-op" | "read" | "create" | "update" | "delete"))
            {
                return Err("Terraform returned an unsupported change action".into());
            }
            if actions.iter().all(|a| matches!(*a, "no-op" | "read")) {
                continue;
            }
            destroys_resources |= actions.contains(&"delete");
            changes.push(format!(
                "{}: {}",
                resource["address"]
                    .as_str()
                    .ok_or("Terraform returned a change without an address")?,
                actions.join(" → ")
            ));
        }
        let bytes = fs::read(directory.join(PLAN)).map_err(|e| e.to_string())?;
        if bytes.is_empty() {
            return Err("Terraform did not produce a saved plan".into());
        }
        crate::packages::verify_packages(&directory, &configuration.packages)?;
        Ok(Plan {
            packages: configuration.packages.clone(),
            configuration,
            directory,
            review,
            changes,
            destroys_resources,
            bytes,
        })
    }

    /// The caller presents the plan to the user and invokes this only when the
    /// user chooses Deploy. A saved plan applies without a terminal prompt.
    pub fn apply(&self, plan: Plan, progress: impl Fn(&str)) -> Result<String, String> {
        check_workspace(&plan.directory, &plan.configuration)?;
        let _lock = WorkspaceLock::acquire(&plan.directory)?;
        check_workspace(&plan.directory, &plan.configuration)?;
        if fs::read(plan.directory.join(CONFIG)).map_err(|e| e.to_string())?
            != plan.configuration.json.as_bytes()
            || fs::read(plan.directory.join(PLAN)).map_err(|e| e.to_string())? != plan.bytes
        {
            return Err("The configuration or saved plan changed. Preview the changes again before deploying.".into());
        }
        crate::packages::verify_packages(&plan.directory, &plan.packages)?;
        progress("Deploying the reviewed plan to AWS…");
        let result = self.run(
            &plan.directory,
            &[
                "apply",
                "-input=false",
                "-no-color",
                "-lock-timeout=60s",
                PLAN,
            ],
        );
        // Failed applies can partially succeed; consume the plan in either case.
        let _ = fs::remove_file(plan.directory.join(PLAN));
        result
    }

    fn run(&self, directory: &Path, args: &[&str]) -> Result<String, String> {
        let output = Command::new(&self.executable)
            .args(args)
            .current_dir(directory)
            .env("TF_IN_AUTOMATION", "1")
            // Ignore inherited CLI flags which could change the requested operation.
            .env_remove("TF_CLI_ARGS")
            .env_remove(format!("TF_CLI_ARGS_{}", args[0]))
            .env_remove("TF_WORKSPACE")
            .env_remove("TF_DATA_DIR")
            .stdin(Stdio::null())
            .output()
            .map_err(|e| {
                format!(
                    "Could not run Terraform: {e}. Install Terraform and make it available on PATH."
                )
            })?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !output.status.success() {
            return Err(format!(
                "Terraform {} failed ({}):\n{stdout}\n{stderr}",
                args[0], output.status
            ));
        }
        Ok(stdout.into_owned())
    }
}

fn prepare_workspace(root: &Path, configuration: &Configuration) -> Result<PathBuf, String> {
    let root = root
        .canonicalize()
        .map_err(|e| format!("Choose an existing deployment folder: {e}"))?;
    let directory = root.join(format!("blueprint-{}", configuration.page));
    if !directory.exists() {
        fs::create_dir(&directory).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))
                .map_err(|e| e.to_string())?;
        }
        bp_io::atomic_write(
            &directory.join(OWNER),
            configuration.page.to_string().as_bytes(),
        )
        .map_err(|e| e.to_string())?;
    }
    check_workspace(&directory, configuration)?;
    Ok(directory)
}

fn check_workspace(directory: &Path, configuration: &Configuration) -> Result<(), String> {
    if fs::symlink_metadata(directory)
        .map_err(|e| e.to_string())?
        .file_type()
        .is_symlink()
    {
        return Err(
            "The deployment workspace must be a real directory, not a symbolic link".into(),
        );
    }
    let owner = fs::read_to_string(directory.join(OWNER)).map_err(
        |_| "This folder is not a Blueprint deployment workspace. Choose another parent folder.",
    )?;
    if owner != configuration.page.to_string() {
        return Err("The deployment workspace belongs to a different page".into());
    }
    for entry in fs::read_dir(directory).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if entry.file_type().map_err(|e| e.to_string())?.is_symlink() {
            return Err(format!(
                "Deployment workspace contains a symbolic link: {name}"
            ));
        }
        if (name.ends_with(".tf") || name.ends_with(".tf.json")) && name != CONFIG {
            return Err(format!(
                "Remove unrelated Terraform configuration {name} from this dedicated workspace"
            ));
        }
        if name == "terraform.tfstate.d" {
            return Err("Use a dedicated workspace without named Terraform workspaces".into());
        }
    }
    Ok(())
}

struct WorkspaceLock(PathBuf);
impl WorkspaceLock {
    fn acquire(directory: &Path) -> Result<Self, String> {
        let path = directory.join(".blueprint-operation");
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|e| format!("Deployment workspace is busy or unavailable: {e}"))?;
        Ok(Self(path))
    }
}
impl Drop for WorkspaceLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
