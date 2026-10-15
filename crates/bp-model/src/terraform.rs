//! Serializable deployment drafts. Provider schemas and deployment readiness live
//! in bp-deploy; the document preserves incomplete references without guessing.

use crate::ElementId;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TerraformProvider {
    Aws,
    Awscc,
}

impl TerraformProvider {
    pub const ALL: [Self; 2] = [Self::Aws, Self::Awscc];

    pub fn name(self) -> &'static str {
        match self {
            Self::Aws => "aws",
            Self::Awscc => "awscc",
        }
    }

    pub fn source(self) -> &'static str {
        match self {
            Self::Aws => "hashicorp/aws",
            Self::Awscc => "hashicorp/awscc",
        }
    }

    pub fn address(self) -> &'static str {
        match self {
            Self::Aws => "registry.terraform.io/hashicorp/aws",
            Self::Awscc => "registry.terraform.io/hashicorp/awscc",
        }
    }

    pub fn owns(self, resource_type: &str) -> bool {
        resource_type.starts_with(&format!("{}_", self.name()))
            && terraform_identifier(resource_type)
    }
}

/// An exact provider version, not a constraint that can silently upgrade.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ProviderPin {
    pub provider: TerraformProvider,
    pub version: String,
}

impl ProviderPin {
    pub fn validate(&self) -> Result<(), String> {
        let parts: Vec<_> = self.version.split('.').collect();
        if parts.len() != 3
            || parts.iter().any(|part| {
                part.is_empty()
                    || !part.bytes().all(|b| b.is_ascii_digit())
                    || (part.len() > 1 && part.starts_with('0'))
                    || part.parse::<u64>().is_err()
            })
        {
            return Err("Provider version must be an exact major.minor.patch release".into());
        }
        Ok(())
    }
}

pub fn terraform_identifier(value: &str) -> bool {
    let mut bytes = value.bytes();
    bytes
        .next()
        .is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
        && bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// A resource key is local to its owning diagram element. `None` is an
/// explicitly unresolved dependency, including a reference outside a copy.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ResourceTarget {
    pub element: Option<ElementId>,
    pub resource: String,
}

impl ResourceTarget {
    pub fn remap(&mut self, ids: &HashMap<ElementId, ElementId>) {
        self.element = self.element.and_then(|id| ids.get(&id).copied());
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum OutputStep {
    Attribute(String),
    Index(usize),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceReference {
    pub target: ResourceTarget,
    /// Explicit output selection; resources need not expose an `id` output.
    pub output: Vec<OutputStep>,
}

/// Only input identity is portable. Secret values are supplied at deployment,
/// never stored in this structure or its default values.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputReference {
    pub name: String,
    #[serde(default)]
    pub sensitive: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum ResourceValue {
    Literal(Value),
    Reference(ResourceReference),
    Input(InputReference),
    List(Vec<ResourceValue>),
    Object(BTreeMap<String, ResourceValue>),
}

impl ResourceValue {
    fn validate(&self, depth: usize) -> Result<(), String> {
        if depth > 64 {
            return Err("Resource properties exceed 64 levels of nesting".into());
        }
        match self {
            Self::Literal(_) => Ok(()),
            Self::Reference(reference) => {
                if !terraform_identifier(&reference.target.resource)
                    || !matches!(reference.output.first(), Some(OutputStep::Attribute(_)))
                    || reference.output.iter().any(|step| {
                        matches!(step, OutputStep::Attribute(name) if !terraform_identifier(name))
                    })
                {
                    return Err("Resource reference needs a resource key and output path".into());
                }
                Ok(())
            }
            Self::Input(input) if !terraform_identifier(&input.name) => {
                Err("Deployment input needs a valid name".into())
            }
            Self::Input(_) => Ok(()),
            Self::List(values) => values
                .iter()
                .try_for_each(|value| value.validate(depth + 1)),
            Self::Object(values) => values
                .values()
                .try_for_each(|value| value.validate(depth + 1)),
        }
    }

    pub fn references(&self) -> Vec<&ResourceReference> {
        match self {
            Self::Reference(reference) => vec![reference],
            Self::List(values) => values.iter().flat_map(Self::references).collect(),
            Self::Object(values) => values.values().flat_map(Self::references).collect(),
            _ => Vec::new(),
        }
    }

    pub fn remap(&mut self, ids: &HashMap<ElementId, ElementId>) {
        match self {
            Self::Reference(reference) => reference.target.remap(ids),
            Self::List(values) => values.iter_mut().for_each(|value| value.remap(ids)),
            Self::Object(values) => values.values_mut().for_each(|value| value.remap(ids)),
            _ => {}
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TerraformResource {
    /// Stable local identity; changing a display label must never change this.
    pub key: String,
    pub provider: ProviderPin,
    pub resource_type: String,
    #[serde(default)]
    pub properties: BTreeMap<String, ResourceValue>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub depends_on: Vec<ResourceTarget>,
}

impl TerraformResource {
    pub fn validate(&self) -> Result<(), String> {
        self.provider.validate()?;
        if !terraform_identifier(&self.key) || !self.provider.provider.owns(&self.resource_type) {
            return Err("Resource identity or provider/type combination is invalid".into());
        }
        if self.properties.keys().any(|key| !terraform_identifier(key))
            || self
                .depends_on
                .iter()
                .any(|target| !terraform_identifier(&target.resource))
        {
            return Err("Resource property and dependency names must be valid identifiers".into());
        }
        self.properties
            .values()
            .try_for_each(|value| value.validate(0))
    }

    pub fn address(&self, owner: ElementId) -> String {
        format!(
            "{}.r_{}_{}",
            self.resource_type,
            owner.to_string().replace('-', ""),
            self.key
        )
    }

    pub fn targets(&self) -> Vec<&ResourceTarget> {
        self.depends_on
            .iter()
            .chain(self.properties.values().flat_map(|value| {
                value
                    .references()
                    .into_iter()
                    .map(|reference| &reference.target)
            }))
            .collect()
    }

    pub fn remap(&mut self, ids: &HashMap<ElementId, ElementId>) {
        self.depends_on
            .iter_mut()
            .for_each(|target| target.remap(ids));
        self.properties
            .values_mut()
            .for_each(|value| value.remap(ids));
    }
}

pub(crate) fn validate_resources(resources: &[TerraformResource]) -> Result<(), String> {
    let mut keys = HashSet::new();
    for resource in resources {
        if !keys.insert(&resource.key) {
            return Err(format!("Duplicate resource key {:?}", resource.key));
        }
        resource.validate()?;
    }
    Ok(())
}
