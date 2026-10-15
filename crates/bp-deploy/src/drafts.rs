//! Deployment readiness diagnostics for portable drafts. These do not provision
//! resources; unresolved references are valid saved work, but block planning.

use crate::{catalog::Catalog, schemas::SchemaSnapshot};
use bp_model::{
    AwsResource, DiagramKind, Document, ElementId, PageId, ResourceTarget, TerraformProvider,
};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DraftDiagnostic {
    pub element: ElementId,
    pub resource: String,
    pub message: String,
}

pub fn validate_page(
    doc: &Document,
    page: PageId,
    catalog: &Catalog,
    schemas: &[SchemaSnapshot],
) -> Vec<DraftDiagnostic> {
    let mut diagnostics = Vec::new();
    let mut pins: BTreeMap<TerraformProvider, String> = BTreeMap::new();
    let mut graph: BTreeMap<ResourceTarget, BTreeSet<ResourceTarget>> = BTreeMap::new();
    for element in doc.page_elements(page) {
        let Some(infrastructure) = element
            .as_shape()
            .and_then(|shape| shape.infrastructure.as_ref())
        else {
            continue;
        };
        let mut report = |key: &str, message: String| {
            diagnostics.push(DraftDiagnostic {
                element: element.id,
                resource: key.into(),
                message,
            })
        };
        if doc.pages[&page].diagram_kind != Some(DiagramKind::Cloud) {
            report(
                "",
                "Infrastructure requires a Cloud architecture page".into(),
            );
        }
        if let Err(message) = infrastructure.validate() {
            report("", message);
            continue;
        }
        let AwsResource::Resources { resources } = infrastructure else {
            graph
                .entry(ResourceTarget {
                    element: Some(element.id),
                    resource: "primary".into(),
                })
                .or_default();
            continue;
        };
        for resource in resources {
            let key = ResourceTarget {
                element: Some(element.id),
                resource: resource.key.clone(),
            };
            let edges = graph.entry(key).or_default();
            if let Some(version) = pins.insert(
                resource.provider.provider,
                resource.provider.version.clone(),
            ) && version != resource.provider.version
            {
                report(
                    &resource.key,
                    "Conflicting provider versions on one deployment page".into(),
                );
            }
            let choice = crate::catalog::ResourceChoice {
                provider: resource.provider.clone(),
                resource_type: resource.resource_type.clone(),
            };
            if let Err(message) = catalog.validate_choice(&choice) {
                report(&resource.key, message);
            }
            if let Some(schema) = schemas
                .iter()
                .find(|schema| schema.pin == resource.provider)
            {
                match schema.validate_resource(resource) {
                    Ok(errors) => {
                        for message in errors {
                            report(&resource.key, message);
                        }
                    }
                    Err(message) => report(&resource.key, message),
                }
            } else {
                report(
                    &resource.key,
                    format!(
                        "Missing verified {} {} schema snapshot",
                        resource.provider.provider.name(),
                        resource.provider.version
                    ),
                );
            }
            for target in resource.targets() {
                let Some(id) = target.element else {
                    report(
                        &resource.key,
                        format!("Choose a target for dependency {}", target.resource),
                    );
                    continue;
                };
                let infrastructure = doc
                    .elements
                    .get(&id)
                    .and_then(|element| element.as_shape())
                    .and_then(|shape| shape.infrastructure.as_ref());
                let exists = match infrastructure {
                    Some(AwsResource::Resources { resources }) => resources
                        .iter()
                        .any(|resource| resource.key == target.resource),
                    Some(_) => target.resource == "primary",
                    None => false,
                };
                if doc.page_of(id) != Some(page) || !exists {
                    report(
                        &resource.key,
                        format!(
                            "Dependency {} must identify a configured resource on this page",
                            target.resource
                        ),
                    );
                } else {
                    edges.insert(target.clone());
                }
            }
            for reference in resource
                .properties
                .values()
                .flat_map(|value| value.references())
            {
                let Some(owner) = reference.target.element else {
                    continue;
                };
                let Some(AwsResource::Resources { resources }) = doc
                    .elements
                    .get(&owner)
                    .and_then(|element| element.as_shape())
                    .and_then(|shape| shape.infrastructure.as_ref())
                else {
                    continue;
                };
                let Some(target) = resources
                    .iter()
                    .find(|target| target.key == reference.target.resource)
                else {
                    continue;
                };
                if let Some(schema) = schemas.iter().find(|schema| schema.pin == target.provider)
                    && let Err(message) =
                        schema.output_type(&target.resource_type, &reference.output)
                {
                    report(&resource.key, message);
                }
            }
        }
    }
    // Repeatedly remove nodes whose dependencies have been resolved. Remaining
    // nodes contain a cycle or depend on one; both must block a future plan.
    loop {
        let ready: BTreeSet<_> = graph
            .iter()
            .filter(|(_, edges)| edges.is_empty())
            .map(|(key, _)| key.clone())
            .collect();
        if ready.is_empty() {
            break;
        }
        graph.retain(|key, _| !ready.contains(key));
        for edges in graph.values_mut() {
            edges.retain(|edge| !ready.contains(edge));
        }
    }
    for key in graph.keys() {
        diagnostics.push(DraftDiagnostic {
            element: key.element.expect("graph nodes have owners"),
            resource: key.resource.clone(),
            message: "Resource has a dependency cycle or depends on a cycle".into(),
        });
    }
    diagnostics
}
