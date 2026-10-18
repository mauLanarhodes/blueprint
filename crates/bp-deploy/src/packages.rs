use crate::Configuration;
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::Read;
use std::path::Path;

/// Export the Terraform JSON and its code archives to one existing directory.
/// The optional input document is protected against every generated output.
pub fn write_configuration(
    configuration: &Configuration,
    destination: &Path,
    protected: Option<&Path>,
) -> Result<(), String> {
    let directory = destination
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
        .canonicalize()
        .map_err(|e| format!("Choose an existing export folder: {e}"))?;
    let destination = directory.join(
        destination
            .file_name()
            .ok_or("Choose a Terraform configuration filename")?,
    );
    let protected = protected
        .map(Path::canonicalize)
        .transpose()
        .map_err(|e| e.to_string())?;
    let mut outputs = Vec::new();
    for (name, bytes) in &configuration.packages {
        validate_name(name)?;
        let path = directory.join(name);
        if path == destination {
            return Err("A code package conflicts with the configuration filename".into());
        }
        outputs.push((path, bytes.as_slice()));
    }
    outputs.push((destination, configuration.json.as_bytes()));
    // Check the entire set before replacing any file.
    for (path, _) in &outputs {
        if let Ok(metadata) = fs::symlink_metadata(path)
            && !metadata.is_file()
        {
            return Err(format!(
                "Export requires a regular file: {}",
                path.display()
            ));
        }
        if protected.as_ref().is_some_and(|input| {
            path == input || path.canonicalize().is_ok_and(|output| output == *input)
        }) {
            return Err("Export must not overwrite the input project or its source code".into());
        }
    }
    for (path, bytes) in outputs {
        bp_io::atomic_write(&path, bytes).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn validate_name(name: &str) -> Result<(), String> {
    if !name.starts_with("blueprint-lambda-")
        || !name.ends_with(".zip")
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
        || name.contains("..")
    {
        return Err("Invalid generated Lambda package filename".into());
    }
    Ok(())
}

pub(crate) fn verify_packages(
    directory: &Path,
    packages: &BTreeMap<String, Vec<u8>>,
) -> Result<(), String> {
    for (name, expected) in packages {
        validate_name(name)?;
        let path = directory.join(name);
        let changed = || {
            format!(
                "Lambda code package {name} changed or is missing. Preview again before deploying."
            )
        };
        let metadata = fs::symlink_metadata(&path).map_err(|_| changed())?;
        if !metadata.is_file() || metadata.len() != expected.len() as u64 {
            return Err(changed());
        }
        let mut actual = Vec::new();
        File::open(&path)
            .map_err(|_| changed())?
            .take(expected.len() as u64 + 1)
            .read_to_end(&mut actual)
            .map_err(|_| changed())?;
        if actual != *expected {
            return Err(changed());
        }
    }
    Ok(())
}
