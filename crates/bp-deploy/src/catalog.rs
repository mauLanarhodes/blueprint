//! Provider coverage inventory, independent of provisioning support in Blueprint.

use bp_model::{CloudIcon, CloudProvider, ProviderPin, ShapeRef, TerraformProvider};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CatalogEntry {
    pub provider: TerraformProvider,
    pub provider_version: String,
    pub service_group: String,
    pub terraform_resource_type: String,
}

impl CatalogEntry {
    pub fn choice(&self) -> ResourceChoice {
        ResourceChoice {
            provider: ProviderPin {
                provider: self.provider,
                version: self.provider_version.clone(),
            },
            resource_type: self.terraform_resource_type.clone(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProviderInventory {
    pub provider: TerraformProvider,
    pub version: String,
    pub resource_count: usize,
    pub group_count: usize,
    pub source_url: String,
}

impl ProviderInventory {
    pub fn pin(&self) -> ProviderPin {
        ProviderPin {
            provider: self.provider,
            version: self.version.clone(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CatalogManifest {
    pub format_version: u32,
    pub snapshot: String,
    pub provenance: String,
    pub csv_sha256: String,
    pub providers: Vec<ProviderInventory>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceChoice {
    pub provider: ProviderPin,
    pub resource_type: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeploymentContext {
    Account,
    Region,
    AvailabilityZone,
}

/// A versioned mapping describes choices; it never implies that a service's
/// complete architecture or deployment has been implemented or tested.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum IconAction {
    Configure { choices: Vec<ResourceChoice> },
    Context { context: DeploymentContext },
    SelectResource { reason: String },
    Unsupported { reason: String },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IconMappings {
    pub format_version: u32,
    /// Exact artwork references, including pack versions; overrides do not leak
    /// from a retired icon to unrelated artwork with the same display name.
    pub bindings: BTreeMap<ShapeRef, IconAction>,
}

#[derive(Clone, Debug, Deserialize)]
struct IconRule {
    names: Vec<String>,
    resource_types: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct AliasCatalog {
    format_version: u32,
    provider: TerraformProvider,
    provider_version: String,
    source_url: String,
    aliases: BTreeMap<String, String>,
}

pub struct Catalog {
    pub manifest: CatalogManifest,
    entries: BTreeMap<(TerraformProvider, String), CatalogEntry>,
    aliases: BTreeMap<(TerraformProvider, String), CatalogEntry>,
    rules: Vec<IconRule>,
}

impl Catalog {
    pub fn builtin() -> &'static Self {
        static CATALOG: OnceLock<Catalog> = OnceLock::new();
        CATALOG.get_or_init(|| {
            Self::from_json(
                include_str!("../catalogs/manifest.json"),
                include_str!("../catalogs/resources.json"),
                include_str!("../catalogs/icon-rules.json"),
            )
            .and_then(|catalog| catalog.with_aliases(include_str!("../catalogs/aliases.json")))
            .expect("bundled AWS catalog is validated by tests")
        })
    }

    pub fn from_json(manifest: &str, entries: &str, rules: &str) -> Result<Self, String> {
        let manifest: CatalogManifest =
            serde_json::from_str(manifest).map_err(|e| e.to_string())?;
        if manifest.format_version != 1 {
            return Err("Unsupported AWS catalog format".into());
        }
        let mut providers = BTreeSet::new();
        for provider in &manifest.providers {
            provider.pin().validate()?;
            if !providers.insert(provider.provider) {
                return Err("Duplicate provider inventory".into());
            }
        }
        let mut indexed = BTreeMap::new();
        for entry in
            serde_json::from_str::<Vec<CatalogEntry>>(entries).map_err(|e| e.to_string())?
        {
            let pin = entry.choice().provider;
            pin.validate()?;
            if entry.service_group.trim().is_empty()
                || !entry.provider.owns(&entry.terraform_resource_type)
                || !manifest
                    .providers
                    .iter()
                    .any(|provider| provider.pin() == pin)
                || indexed
                    .insert(
                        (entry.provider, entry.terraform_resource_type.clone()),
                        entry,
                    )
                    .is_some()
            {
                return Err("Invalid, duplicate, or unpinned resource catalog entry".into());
            }
        }
        for provider in &manifest.providers {
            let entries: Vec<_> = indexed
                .values()
                .filter(|entry| entry.provider == provider.provider)
                .collect();
            let groups: BTreeSet<_> = entries.iter().map(|entry| &entry.service_group).collect();
            if entries.len() != provider.resource_count || groups.len() != provider.group_count {
                return Err(format!(
                    "{} catalog counts do not match the manifest",
                    provider.provider.name()
                ));
            }
        }
        let catalog = Self {
            manifest,
            entries: indexed,
            aliases: BTreeMap::new(),
            rules: serde_json::from_str(rules).map_err(|e| e.to_string())?,
        };
        for rule in &catalog.rules {
            if rule.names.is_empty()
                || rule.resource_types.is_empty()
                || rule
                    .resource_types
                    .iter()
                    .any(|name| catalog.find(TerraformProvider::Aws, name).is_none())
            {
                return Err("Icon rule refers to an unknown resource".into());
            }
        }
        Ok(catalog)
    }

    /// Provider aliases keep their literal Terraform identity while remaining
    /// separate from the supplied documentation inventory.
    pub fn with_aliases(mut self, json: &str) -> Result<Self, String> {
        let aliases: AliasCatalog = serde_json::from_str(json).map_err(|e| e.to_string())?;
        if aliases.format_version != 1
            || aliases.source_url.trim().is_empty()
            || !self.manifest.providers.iter().any(|provider| {
                provider.provider == aliases.provider
                    && provider.version == aliases.provider_version
            })
        {
            return Err("Invalid or unpinned provider alias catalog".into());
        }
        for (alias, canonical) in aliases.aliases {
            let mut entry = self
                .entries
                .get(&(aliases.provider, canonical))
                .ok_or("Provider alias refers to an unknown canonical resource")?
                .clone();
            if !aliases.provider.owns(&alias)
                || self
                    .entries
                    .contains_key(&(aliases.provider, alias.clone()))
            {
                return Err("Invalid or conflicting provider alias".into());
            }
            entry.terraform_resource_type = alias.clone();
            if self
                .aliases
                .insert((aliases.provider, alias), entry)
                .is_some()
            {
                return Err("Duplicate provider alias".into());
            }
        }
        Ok(self)
    }

    pub fn aliases(&self) -> impl Iterator<Item = &CatalogEntry> {
        self.aliases.values()
    }

    pub fn entries(&self) -> impl Iterator<Item = &CatalogEntry> {
        self.entries.values()
    }

    pub fn find(&self, provider: TerraformProvider, resource_type: &str) -> Option<&CatalogEntry> {
        self.entries
            .get(&(provider, resource_type.into()))
            .or_else(|| self.aliases.get(&(provider, resource_type.into())))
    }

    pub fn validate_choice(&self, choice: &ResourceChoice) -> Result<(), String> {
        if self
            .find(choice.provider.provider, &choice.resource_type)
            .is_some_and(|entry| entry.provider_version == choice.provider.version)
        {
            Ok(())
        } else {
            Err(format!(
                "{} is absent from the selected provider catalog/version",
                choice.resource_type
            ))
        }
    }

    pub fn search(&self, provider: Option<TerraformProvider>, query: &str) -> Vec<&CatalogEntry> {
        let words: Vec<_> = query.split_whitespace().map(str::to_lowercase).collect();
        self.entries()
            .chain(self.aliases())
            .filter(|entry| {
                provider.is_none_or(|provider| entry.provider == provider)
                    && words.iter().all(|word| {
                        format!("{} {}", entry.service_group, entry.terraform_resource_type)
                            .to_lowercase()
                            .contains(word)
                    })
            })
            .collect()
    }

    pub fn resolve_icon(
        &self,
        icon: &CloudIcon,
        mappings: Option<&IconMappings>,
    ) -> Result<IconAction, String> {
        icon.validate().map_err(|e| e.to_string())?;
        if icon.provider != CloudProvider::Aws {
            return Ok(IconAction::Unsupported {
                reason: "This catalog covers AWS; Azure provisioning is a separate integration"
                    .into(),
            });
        }
        if let Some(mappings) = mappings {
            if mappings.format_version != 1 {
                return Err("Unsupported icon mapping format".into());
            }
            if let Some(action) = mappings.bindings.get(&icon.reference) {
                if let IconAction::Configure { choices } = action {
                    if choices.is_empty() {
                        return Err("An icon mapping needs at least one resource choice".into());
                    }
                    for choice in choices {
                        self.validate_choice(choice)?;
                    }
                }
                return Ok(action.clone());
            }
        }
        let name = normalize(&icon.name);
        // Context matching is deliberately exact, not inferred from arbitrary
        // filenames containing words like 'region' or 'account'.
        if icon.kind == bp_model::IconKind::Group {
            let context = match name.as_str() {
                "awsaccount" | "account" | "awscloud" => Some(DeploymentContext::Account),
                "awsregion" | "region" => Some(DeploymentContext::Region),
                "availabilityzone" | "awsavailabilityzone" => {
                    Some(DeploymentContext::AvailabilityZone)
                }
                _ => None,
            };
            if let Some(context) = context {
                return Ok(IconAction::Context { context });
            }
        }
        if let Some(rule) = self
            .rules
            .iter()
            .find(|rule| rule.names.iter().any(|alias| normalize(alias) == name))
        {
            return Ok(IconAction::Configure {
                choices: rule
                    .resource_types
                    .iter()
                    .map(|name| {
                        self.find(TerraformProvider::Aws, name)
                            .expect("validated rule")
                            .choice()
                    })
                    .collect(),
            });
        }
        Ok(IconAction::SelectResource {
            reason: format!(
                "Choose a resource from the AWS catalog for {}; artwork alone does not identify its configuration",
                icon.name
            ),
        })
    }
}

fn normalize(value: &str) -> String {
    value
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}
