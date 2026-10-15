//! Exact-version Terraform schema snapshots. Schema discovery initializes only
//! providers in an isolated directory; it never runs plan/apply or contacts AWS.

use crate::catalog::Catalog;
use bp_model::{OutputStep, ProviderPin, ResourceValue, TerraformResource};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const MAX_SCHEMA_BYTES: u64 = 128 * 1024 * 1024;
const CACHE_FILE: &str = "schema.json";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AttributeSchema {
    #[serde(rename = "type", default)]
    pub value_type: Option<Value>,
    #[serde(default)]
    pub nested_type: Option<NestedAttributes>,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub optional: bool,
    #[serde(default)]
    pub computed: bool,
    #[serde(default)]
    pub sensitive: bool,
    #[serde(default)]
    pub write_only: bool,
    #[serde(default)]
    pub description: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NestedAttributes {
    pub nesting_mode: String,
    pub attributes: BTreeMap<String, AttributeSchema>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct BlockSchema {
    #[serde(default)]
    pub attributes: BTreeMap<String, AttributeSchema>,
    #[serde(default)]
    pub block_types: BTreeMap<String, NestedBlock>,
    #[serde(default)]
    pub description: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NestedBlock {
    pub nesting_mode: String,
    pub block: Box<BlockSchema>,
    #[serde(default)]
    pub min_items: Option<usize>,
    #[serde(default)]
    pub max_items: Option<usize>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ResourceSchema {
    pub version: u64,
    pub block: BlockSchema,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// Keep the raw provider payload, including future minor-version fields. The
/// resource schema version is deliberately separate from the provider release.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SchemaSnapshot {
    pub cache_format_version: u32,
    pub pin: ProviderPin,
    pub terraform_version: String,
    pub terraform_schema_format: String,
    pub provider_schema: Value,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SchemaCoverage {
    pub listed: usize,
    pub available: usize,
    pub missing: Vec<String>,
    pub additional: Vec<String>,
}

impl SchemaSnapshot {
    /// Provider schema JSON has no release number. Verify the actual selected
    /// provider from `terraform version -json` instead of trusting a filename.
    pub fn from_outputs(
        pin: &ProviderPin,
        version_json: &[u8],
        schema_json: &[u8],
    ) -> Result<Self, String> {
        pin.validate()?;
        let version: Value = serde_json::from_slice(version_json).map_err(|e| e.to_string())?;
        if version
            .get("provider_selections")
            .and_then(|providers| providers.get(pin.provider.address()))
            .and_then(Value::as_str)
            != Some(pin.version.as_str())
        {
            return Err("Terraform selected a different or unverified provider version".into());
        }
        let schemas: Value = serde_json::from_slice(schema_json).map_err(|e| e.to_string())?;
        let snapshot = Self {
            cache_format_version: 1,
            pin: pin.clone(),
            terraform_version: version["terraform_version"]
                .as_str()
                .ok_or("Missing Terraform version")?
                .into(),
            terraform_schema_format: schemas["format_version"]
                .as_str()
                .ok_or("Missing Terraform schema format")?
                .into(),
            provider_schema: schemas
                .get("provider_schemas")
                .and_then(|providers| providers.get(pin.provider.address()))
                .ok_or("Provider is missing from schema output")?
                .clone(),
        };
        snapshot.validate(pin)?;
        Ok(snapshot)
    }

    pub fn validate(&self, expected: &ProviderPin) -> Result<(), String> {
        expected.validate()?;
        self.pin.validate()?;
        let parts: Vec<_> = self.terraform_schema_format.split('.').collect();
        if self.cache_format_version != 1
            || &self.pin != expected
            || self.terraform_version.is_empty()
            || parts.len() != 2
            || parts[0] != "1"
            || parts[1].parse::<u64>().is_err()
        {
            return Err("Incompatible or mismatched Terraform schema cache".into());
        }
        let resources = self
            .provider_schema
            .get("resource_schemas")
            .and_then(Value::as_object)
            .ok_or("Provider schema has no resource_schemas object")?;
        if resources.is_empty() {
            return Err("Provider schema contains no resources".into());
        }
        for (name, schema) in resources {
            if !expected.provider.owns(name) {
                return Err(format!("Wrong provider resource {name}"));
            }
            serde_json::from_value::<ResourceSchema>(schema.clone())
                .map_err(|e| format!("{name}: {e}"))?;
        }
        Ok(())
    }

    pub fn resource(&self, resource_type: &str) -> Result<ResourceSchema, String> {
        let schema = self
            .provider_schema
            .get("resource_schemas")
            .and_then(|resources| resources.get(resource_type))
            .ok_or_else(|| format!("{resource_type} is absent from the selected schema"))?;
        serde_json::from_value(schema.clone()).map_err(|e| e.to_string())
    }

    /// Check output traversal against readable provider fields. Dynamic values
    /// can only be resolved by Terraform; write-only attributes are never outputs.
    pub fn output_type(
        &self,
        resource_type: &str,
        path: &[OutputStep],
    ) -> Result<Option<Value>, String> {
        let schema = self.resource(resource_type)?;
        let mut ty = block_type(&schema.block);
        for step in path {
            if ty.as_str() == Some("dynamic") {
                return Ok(None);
            }
            let parts = ty
                .as_array()
                .filter(|parts| parts.len() == 2)
                .ok_or("Output path traverses a scalar value")?;
            ty =
                match (parts[0].as_str(), step) {
                    (Some("object"), OutputStep::Attribute(name)) => parts[1]
                        .get(name)
                        .ok_or_else(|| format!("Output field {name} is missing or write-only"))?
                        .clone(),
                    (Some("map"), OutputStep::Attribute(_)) => parts[1].clone(),
                    (Some("list"), OutputStep::Index(_)) => parts[1].clone(),
                    (Some("tuple"), OutputStep::Index(index)) => parts[1]
                        .get(*index)
                        .ok_or("Output tuple index is outside its schema")?
                        .clone(),
                    _ => return Err(
                        "Output path does not match its provider schema (sets cannot be indexed)"
                            .into(),
                    ),
                };
        }
        Ok(Some(ty))
    }

    pub fn coverage(&self, catalog: &Catalog) -> Result<SchemaCoverage, String> {
        self.validate(&self.pin)?;
        if !catalog
            .manifest
            .providers
            .iter()
            .any(|provider| provider.pin() == self.pin)
        {
            return Err("Schema release does not match this catalog snapshot".into());
        }
        let listed: BTreeSet<_> = catalog
            .entries()
            .filter(|entry| entry.choice().provider == self.pin)
            .map(|entry| entry.terraform_resource_type.clone())
            .collect();
        let available: BTreeSet<_> = self.provider_schema["resource_schemas"]
            .as_object()
            .expect("validated")
            .keys()
            .cloned()
            .collect();
        Ok(SchemaCoverage {
            listed: listed.len(),
            available: available.len(),
            missing: listed.difference(&available).cloned().collect(),
            additional: available.difference(&listed).cloned().collect(),
        })
    }

    /// Structural checks only. Provider-specific conflicts, AWS quotas,
    /// permissions and service semantics still require Terraform validation.
    pub fn validate_resource(&self, resource: &TerraformResource) -> Result<Vec<String>, String> {
        resource.validate()?;
        if self.pin != resource.provider || self.cache_format_version != 1 {
            return Err("Resource and schema provider releases differ".into());
        }
        let schema = self.resource(&resource.resource_type)?;
        let mut errors = Vec::new();
        validate_block(
            &schema.block,
            &resource.properties,
            &resource.key,
            &mut errors,
        );
        Ok(errors)
    }
}

fn attribute_type(attribute: &AttributeSchema) -> Value {
    if let Some(nested) = &attribute.nested_type {
        let fields: serde_json::Map<_, _> = nested
            .attributes
            .iter()
            .filter(|(_, attribute)| !attribute.write_only)
            .map(|(name, attribute)| (name.clone(), attribute_type(attribute)))
            .collect();
        let object = json!(["object", fields]);
        match nested.nesting_mode.as_str() {
            "single" | "group" => object,
            mode => json!([mode, object]),
        }
    } else {
        attribute
            .value_type
            .clone()
            .unwrap_or_else(|| json!("dynamic"))
    }
}

fn block_type(block: &BlockSchema) -> Value {
    let mut fields: serde_json::Map<_, _> = block
        .attributes
        .iter()
        .filter(|(_, attribute)| !attribute.write_only)
        .map(|(name, attribute)| (name.clone(), attribute_type(attribute)))
        .collect();
    for (name, nested) in &block.block_types {
        let object = block_type(&nested.block);
        fields.insert(
            name.clone(),
            match nested.nesting_mode.as_str() {
                "single" | "group" => object,
                mode => json!([mode, object]),
            },
        );
    }
    json!(["object", fields])
}

fn validate_block(
    schema: &BlockSchema,
    values: &BTreeMap<String, ResourceValue>,
    path: &str,
    errors: &mut Vec<String>,
) {
    for (name, attribute) in &schema.attributes {
        let value = values
            .get(name)
            .filter(|value| !matches!(value, ResourceValue::Literal(Value::Null)));
        if attribute.required && value.is_none() {
            errors.push(format!("{path}.{name}: required property is missing"));
        }
        if let Some(value) = value {
            if attribute.computed && !attribute.optional && !attribute.required {
                errors.push(format!(
                    "{path}.{name}: computed output cannot be configured"
                ));
            } else if (attribute.sensitive || attribute.write_only)
                && !matches!(value, ResourceValue::Input(input) if input.sensitive)
                && !matches!(value, ResourceValue::Reference(_))
            {
                errors.push(format!("{path}.{name}: use a sensitive deployment input"));
            } else {
                validate_attribute(attribute, value, &format!("{path}.{name}"), errors);
            }
        }
    }
    for (name, block) in &schema.block_types {
        let value = values.get(name);
        let count = value.map_or(0, |value| {
            if matches!(block.nesting_mode.as_str(), "single" | "group") {
                usize::from(!matches!(value, ResourceValue::Literal(Value::Null)))
            } else {
                collection_len(value)
            }
        });
        if block.min_items.is_some_and(|min| count < min)
            || block.max_items.is_some_and(|max| count > max)
        {
            errors.push(format!(
                "{path}.{name}: nested block count is outside schema limits"
            ));
        }
        if let Some(value) = value {
            validate_nested(
                &block.block,
                &block.nesting_mode,
                value,
                &format!("{path}.{name}"),
                errors,
            );
        }
    }
    for name in values.keys() {
        if !schema.attributes.contains_key(name) && !schema.block_types.contains_key(name) {
            errors.push(format!("{path}.{name}: unknown property"));
        }
    }
}

fn validate_attribute(
    schema: &AttributeSchema,
    value: &ResourceValue,
    path: &str,
    errors: &mut Vec<String>,
) {
    if matches!(value, ResourceValue::Reference(_) | ResourceValue::Input(_)) {
        return;
    }
    if let Some(nested) = &schema.nested_type {
        let block = BlockSchema {
            attributes: nested.attributes.clone(),
            ..Default::default()
        };
        validate_nested(&block, &nested.nesting_mode, value, path, errors);
    } else if let Some(ty) = &schema.value_type {
        match literal_value(value) {
            Some(value) if matches_type(&value, ty) => {}
            Some(_) => errors.push(format!(
                "{path}: value does not match its provider schema type"
            )),
            // A nested reference is resolved later. Validate each known literal
            // by recursing through its collection type below.
            None => validate_collection(ty, value, path, errors),
        }
    }
}

fn literal_value(value: &ResourceValue) -> Option<Value> {
    match value {
        ResourceValue::Literal(value) => Some(value.clone()),
        ResourceValue::List(values) => Some(Value::Array(
            values.iter().map(literal_value).collect::<Option<_>>()?,
        )),
        ResourceValue::Object(values) => Some(Value::Object(
            values
                .iter()
                .map(|(k, v)| Some((k.clone(), literal_value(v)?)))
                .collect::<Option<_>>()?,
        )),
        _ => None,
    }
}

fn matches_type(value: &Value, ty: &Value) -> bool {
    if value.is_null() {
        return true;
    }
    match ty.as_str() {
        Some("string") => value.is_string(),
        Some("number") => value.is_number(),
        Some("bool") => value.is_boolean(),
        Some("dynamic") => true,
        Some(_) => false,
        None => {
            let Some(parts) = ty.as_array().filter(|parts| parts.len() == 2) else {
                return false;
            };
            match parts[0].as_str() {
                Some("list" | "set") => value.as_array().is_some_and(|values| {
                    values.iter().all(|value| matches_type(value, &parts[1]))
                }),
                Some("map") => value.as_object().is_some_and(|values| {
                    values.values().all(|value| matches_type(value, &parts[1]))
                }),
                Some("object") => {
                    value
                        .as_object()
                        .zip(parts[1].as_object())
                        .is_some_and(|(values, fields)| {
                            values.keys().all(|key| fields.contains_key(key))
                                && fields.iter().all(|(key, ty)| {
                                    values.get(key).is_some_and(|value| matches_type(value, ty))
                                })
                        })
                }
                Some("tuple") => {
                    value
                        .as_array()
                        .zip(parts[1].as_array())
                        .is_some_and(|(values, types)| {
                            values.len() == types.len()
                                && values
                                    .iter()
                                    .zip(types)
                                    .all(|(value, ty)| matches_type(value, ty))
                        })
                }
                _ => false,
            }
        }
    }
}

fn validate_collection(ty: &Value, value: &ResourceValue, path: &str, errors: &mut Vec<String>) {
    if matches!(value, ResourceValue::Reference(_) | ResourceValue::Input(_)) {
        return;
    }
    let Some(parts) = ty.as_array().filter(|parts| parts.len() == 2) else {
        errors.push(format!("{path}: expected a literal of the declared type"));
        return;
    };
    let check = |value: &ResourceValue, ty: &Value, path: String, errors: &mut Vec<String>| {
        let attribute = AttributeSchema {
            value_type: Some(ty.clone()),
            nested_type: None,
            required: false,
            optional: true,
            computed: false,
            sensitive: false,
            write_only: false,
            description: String::new(),
            extra: BTreeMap::new(),
        };
        validate_attribute(&attribute, value, &path, errors);
    };
    match (parts[0].as_str(), value) {
        (Some("list" | "set"), ResourceValue::List(values)) => {
            for (index, value) in values.iter().enumerate() {
                check(value, &parts[1], format!("{path}[{index}]"), errors);
            }
        }
        (Some("map"), ResourceValue::Object(values)) => {
            for (key, value) in values {
                check(value, &parts[1], format!("{path}.{key}"), errors);
            }
        }
        (Some("object"), ResourceValue::Object(values)) if parts[1].is_object() => {
            let fields = parts[1].as_object().expect("checked");
            for (key, ty) in fields {
                if let Some(value) = values.get(key) {
                    check(value, ty, format!("{path}.{key}"), errors);
                } else {
                    errors.push(format!("{path}.{key}: missing object field"));
                }
            }
            for key in values.keys().filter(|key| !fields.contains_key(*key)) {
                errors.push(format!("{path}.{key}: unknown object field"));
            }
        }
        (Some("tuple"), ResourceValue::List(values))
            if parts[1].is_array()
                && values.len() == parts[1].as_array().expect("checked").len() =>
        {
            for (index, (value, ty)) in values
                .iter()
                .zip(parts[1].as_array().expect("checked"))
                .enumerate()
            {
                check(value, ty, format!("{path}[{index}]"), errors);
            }
        }
        _ => errors.push(format!(
            "{path}: collection does not match its provider schema"
        )),
    }
}

fn collection_len(value: &ResourceValue) -> usize {
    match value {
        ResourceValue::List(values) => values.len(),
        ResourceValue::Object(values) => values.len(),
        ResourceValue::Literal(Value::Array(values)) => values.len(),
        ResourceValue::Literal(Value::Object(values)) => values.len(),
        ResourceValue::Literal(Value::Null) => 0,
        _ => 1,
    }
}

fn validate_nested(
    schema: &BlockSchema,
    mode: &str,
    value: &ResourceValue,
    path: &str,
    errors: &mut Vec<String>,
) {
    if matches!(value, ResourceValue::Reference(_) | ResourceValue::Input(_)) {
        return;
    }
    if let ResourceValue::Literal(value) = value {
        let converted = match value {
            Value::Object(values) => ResourceValue::Object(
                values
                    .iter()
                    .map(|(k, v)| (k.clone(), ResourceValue::Literal(v.clone())))
                    .collect(),
            ),
            Value::Array(values) => {
                ResourceValue::List(values.iter().cloned().map(ResourceValue::Literal).collect())
            }
            Value::Null => return,
            _ => {
                errors.push(format!("{path}: expected a nested block"));
                return;
            }
        };
        validate_nested(schema, mode, &converted, path, errors);
        return;
    }
    match (mode, value) {
        ("single" | "group", ResourceValue::Object(values)) => {
            validate_block(schema, values, path, errors)
        }
        ("list" | "set", ResourceValue::List(values)) => {
            for (index, value) in values.iter().enumerate() {
                validate_nested(schema, "single", value, &format!("{path}[{index}]"), errors);
            }
        }
        ("map", ResourceValue::Object(values)) => {
            for (key, value) in values {
                validate_nested(schema, "single", value, &format!("{path}.{key}"), errors);
            }
        }
        _ => errors.push(format!("{path}: expected {mode} nesting")),
    }
}

pub struct SchemaCache {
    pub root: PathBuf,
}

impl SchemaCache {
    fn directory(&self, pin: &ProviderPin) -> Result<PathBuf, String> {
        pin.validate()?;
        Ok(self.root.join(pin.provider.name()).join(&pin.version))
    }

    pub fn load(&self, pin: &ProviderPin) -> Result<SchemaSnapshot, String> {
        let directory = self.directory(pin)?;
        require_directory(&self.root)?;
        require_directory(&self.root.join(pin.provider.name()))?;
        require_directory(&directory)?;
        let bytes = read_json(&directory.join(CACHE_FILE))?;
        let snapshot: SchemaSnapshot = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        snapshot.validate(pin)?;
        Ok(snapshot)
    }

    /// Cache misses are explicit; an offline read never installs/upgrades a
    /// provider or substitutes a schema from another release.
    pub fn discover(&self, executable: &Path, pin: &ProviderPin) -> Result<SchemaSnapshot, String> {
        let directory = self.directory(pin)?;
        ensure_directory(&self.root)?;
        ensure_directory(&self.root.join(pin.provider.name()))?;
        ensure_directory(&directory)?;
        let _lock = CacheLock::acquire(&directory)?;
        if directory.join(CACHE_FILE).exists() {
            return self.load(pin);
        }
        let configuration = serde_json::to_vec_pretty(&json!({ "terraform": {
            "required_providers": { pin.provider.name(): { "source": pin.provider.source(), "version": format!("= {}", pin.version) } }
        }})).map_err(|e| e.to_string())?;
        for entry in fs::read_dir(&directory).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if entry.file_type().map_err(|e| e.to_string())?.is_symlink()
                || !matches!(
                    name.as_str(),
                    ".schema-operation" | "provider.tf.json" | ".terraform.lock.hcl" | ".terraform"
                )
            {
                return Err("Schema workspace contains unrelated or symbolic-link files".into());
            }
        }
        let config_path = directory.join("provider.tf.json");
        if config_path.exists() && read_json(&config_path)? != configuration {
            return Err("Schema workspace belongs to a different provider configuration".into());
        }
        bp_io::atomic_write(&config_path, &configuration).map_err(|e| e.to_string())?;
        run(
            executable,
            &directory,
            &["init", "-backend=false", "-input=false", "-no-color"],
            "init-output.json",
        )?;
        let version = run(
            executable,
            &directory,
            &["version", "-json"],
            "version-output.json",
        )?;
        let schema = run(
            executable,
            &directory,
            &["providers", "schema", "-json"],
            "schema-output.json",
        )?;
        let snapshot = SchemaSnapshot::from_outputs(pin, &version, &schema)?;
        let bytes = serde_json::to_vec(&snapshot).map_err(|e| e.to_string())?;
        if bytes.len() as u64 > MAX_SCHEMA_BYTES {
            return Err("Schema cache is too large".into());
        }
        bp_io::atomic_write(&directory.join(CACHE_FILE), &bytes).map_err(|e| e.to_string())?;
        Ok(snapshot)
    }
}

fn require_directory(path: &Path) -> Result<(), String> {
    let meta = fs::symlink_metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if !meta.is_dir() || meta.file_type().is_symlink() {
        return Err("Schema cache directories cannot be symbolic links".into());
    }
    Ok(())
}

fn ensure_directory(path: &Path) -> Result<(), String> {
    match fs::create_dir(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => require_directory(path),
        Err(error) => Err(error.to_string()),
    }
}

fn read_json(path: &Path) -> Result<Vec<u8>, String> {
    let metadata = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > MAX_SCHEMA_BYTES
    {
        return Err("Schema input is not a bounded regular file".into());
    }
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|e| e.to_string())?
        .take(MAX_SCHEMA_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_SCHEMA_BYTES {
        return Err("Schema input exceeds the size limit".into());
    }
    Ok(bytes)
}

fn run(
    executable: &Path,
    directory: &Path,
    args: &[&str],
    filename: &str,
) -> Result<Vec<u8>, String> {
    let path = directory.join(filename);
    let file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&path)
        .map_err(|e| e.to_string())?;
    let result = Command::new(executable)
        .current_dir(directory)
        .args(args)
        .env("TF_IN_AUTOMATION", "1")
        .env_remove("TF_CLI_ARGS")
        .env_remove("TF_CLI_ARGS_init")
        .env_remove("TF_CLI_ARGS_version")
        .env_remove("TF_CLI_ARGS_providers")
        .env_remove("TF_DATA_DIR")
        .stdout(Stdio::from(file))
        .stderr(Stdio::piped())
        .output();
    let read = match result {
        Ok(output) if output.status.success() => read_json(&path),
        Ok(_) => Err(format!(
            "Terraform {} failed; inspect the isolated schema workspace",
            args[0]
        )),
        Err(error) => Err(format!("Could not run Terraform: {error}")),
    };
    let _ = fs::remove_file(path);
    read
}

struct CacheLock(PathBuf);
impl CacheLock {
    fn acquire(directory: &Path) -> Result<Self, String> {
        let path = directory.join(".schema-operation");
        OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)
            .map_err(|e| format!("Schema cache is busy or requires recovery: {e}"))?;
        Ok(Self(path))
    }
}
impl Drop for CacheLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
