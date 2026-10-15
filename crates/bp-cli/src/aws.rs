//! Read-only Foundation diagnostics and explicit provider-schema installation.

use bp_deploy::{catalog::Catalog, schemas::SchemaCache};
use bp_model::{AwsResource, ProviderPin, TerraformProvider};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn provider(name: &str) -> Result<TerraformProvider, String> {
    match name {
        "aws" => Ok(TerraformProvider::Aws),
        "awscc" => Ok(TerraformProvider::Awscc),
        _ => Err("Provider must be aws or awscc".into()),
    }
}

pub(super) fn run(args: &[String]) -> Result<(), String> {
    let catalog = Catalog::builtin();
    match args[0].as_str() {
        "aws-catalog" => {
            let mut selected = None;
            let mut query = String::new();
            let mut remaining = args[1..].iter();
            while let Some(flag) = remaining.next() {
                let value = remaining.next().ok_or("Catalog option needs a value")?;
                match flag.as_str() {
                    "--provider" => selected = Some(provider(value)?),
                    "--search" => query = value.clone(),
                    _ => return Err(format!("Unknown catalog option {flag}")),
                }
            }
            let entries = catalog.search(selected, &query);
            println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                "manifest": catalog.manifest, "matches": entries,
                "notice": "Inventory entries are not claims of implemented or tested deployment support"
            })).map_err(|e| e.to_string())?);
        }
        "aws-icons" if args.len() == 2 => {
            let doc = bp_io::load(Path::new(&args[1])).map_err(|e| e.to_string())?;
            let mut mappings = Vec::new();
            for icon in doc.icons.values() {
                mappings.push(serde_json::json!({"reference": icon.reference, "name": icon.name, "resolution": catalog.resolve_icon(icon, None)?}));
            }
            println!(
                "{}",
                serde_json::to_string_pretty(&mappings).map_err(|e| e.to_string())?
            );
        }
        "aws-schema" if args.len() == 4 || args.len() == 6 => {
            let selected = provider(&args[2])?;
            let pin = catalog
                .manifest
                .providers
                .iter()
                .find(|entry| entry.provider == selected)
                .ok_or("Provider is not cataloged")?
                .pin();
            let cache = SchemaCache {
                root: PathBuf::from(&args[3]),
            };
            let executable = if args.len() == 6 && args[4] == "--terraform" && args[1] == "fetch" {
                PathBuf::from(&args[5])
            } else if args.len() == 4 {
                PathBuf::from("terraform")
            } else {
                return Err("Only schema fetch accepts --terraform <path>".into());
            };
            let snapshot = match args[1].as_str() {
                "fetch" => cache.discover(&executable, &pin)?,
                "check" => cache.load(&pin)?,
                _ => return Err("Schema action must be fetch or check".into()),
            };
            let coverage = snapshot.coverage(catalog)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&coverage).map_err(|e| e.to_string())?
            );
            if !coverage.missing.is_empty() || !coverage.additional.is_empty() {
                return Err(
                    "Provider schema and catalog differ; review the reported resource types".into(),
                );
            }
        }
        "aws-check" if args.len() == 3 => {
            let doc = bp_io::load(Path::new(&args[1])).map_err(|e| e.to_string())?;
            let cache = SchemaCache {
                root: args[2].clone().into(),
            };
            let pins: BTreeSet<ProviderPin> = doc
                .elements
                .values()
                .filter_map(|element| element.as_shape())
                .filter_map(|shape| shape.infrastructure.as_ref())
                .flat_map(|infrastructure| match infrastructure {
                    AwsResource::Resources { resources } => resources
                        .iter()
                        .map(|resource| resource.provider.clone())
                        .collect(),
                    _ => Vec::new(),
                })
                .collect();
            let mut schemas = Vec::new();
            for pin in pins {
                // A missing/corrupt cache becomes a resource-linked diagnostic;
                // this offline command must never initialize providers.
                match cache.load(&pin) {
                    Ok(snapshot) => schemas.push(snapshot),
                    Err(error) => eprintln!(
                        "Schema unavailable for {} {}: {error}",
                        pin.provider.name(),
                        pin.version
                    ),
                }
            }
            let diagnostics: Vec<_> = doc
                .pages
                .keys()
                .flat_map(|page| bp_deploy::drafts::validate_page(&doc, *page, catalog, &schemas))
                .collect();
            println!(
                "{}",
                serde_json::to_string_pretty(&diagnostics).map_err(|e| e.to_string())?
            );
            if !diagnostics.is_empty() {
                return Err(
                    "Resource drafts require configuration; no AWS operations were performed"
                        .into(),
                );
            }
        }
        _ => return Err("Invalid AWS Foundation command arguments; run --help for usage".into()),
    }
    Ok(())
}
