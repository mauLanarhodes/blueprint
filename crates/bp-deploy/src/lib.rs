//! Cloud diagrams become deployable only through explicit, validated settings.

mod services;
mod terraform;
pub mod trial;
pub use terraform::{Plan, Terraform};

use bp_model::{AwsDeployment, AwsResource, DiagramKind, Document, ElementId, PageId};
use serde_json::{Map, json};
use std::collections::{BTreeMap, BTreeSet};
use std::net::Ipv4Addr;

#[derive(Clone, Debug)]
pub struct Configuration {
    pub page: PageId,
    pub account_id: String,
    pub json: String,
    pub resources: Vec<String>,
    pub warnings: Vec<String>,
}

pub fn configuration(doc: &Document, page: PageId) -> Result<Configuration, String> {
    let page_data = doc
        .pages
        .get(&page)
        .ok_or("The cloud page no longer exists")?;
    if page_data.diagram_kind != Some(DiagramKind::Cloud) {
        return Err("Deployment requires a Cloud architecture page".into());
    }
    let target = page_data
        .deployment
        .as_ref()
        .ok_or("Configure the AWS deployment target first")?;
    validate_target(target)?;
    let elements = doc.page_elements(page);
    let configured: BTreeMap<_, _> = elements
        .iter()
        .filter_map(|el| {
            el.as_shape()?
                .infrastructure
                .as_ref()
                .map(|resource| (el.id, (*el, resource)))
        })
        .collect();
    if configured.is_empty() {
        return Err(
            "Choose a resource in the Infrastructure inspector, or add an AWS trial stack".into(),
        );
    }
    let mut resources = Map::new();
    let mut outputs = Map::new();
    let mut names = Vec::new();
    let mut subnets: Vec<(ElementId, ElementId, Cidr)> = Vec::new();
    let topics: BTreeSet<_> = configured
        .iter()
        .filter_map(|(id, (_, resource))| {
            matches!(resource, AwsResource::SnsTopic { .. }).then_some(*id)
        })
        .collect();
    let mut service_names = BTreeSet::new();
    for (id, (element, resource)) in &configured {
        let shape = element.as_shape().expect("configured shapes");
        if shape.shape.library() == "azure" || shape.erd.is_some() {
            return Err(format!(
                "{} cannot be deployed as an AWS resource",
                element_name(element)
            ));
        }
        let name = element_name(element);
        let (kind, mut body) = match resource {
            AwsResource::Vpc { cidr } => {
                Cidr::parse(cidr).map_err(|e| format!("{name}: {e}"))?;
                (
                    "aws_vpc",
                    json!({"cidr_block": cidr, "enable_dns_support": true, "enable_dns_hostnames": true}),
                )
            }
            AwsResource::Subnet {
                vpc,
                cidr,
                availability_zone,
            } => {
                let vpc = vpc.ok_or_else(|| format!("{name}: choose its VPC"))?;
                let Some((_, AwsResource::Vpc { cidr: parent_cidr })) = configured.get(&vpc) else {
                    return Err(format!(
                        "{name}: its VPC must be a configured VPC on this page"
                    ));
                };
                let child = Cidr::parse(cidr).map_err(|e| format!("{name}: {e}"))?;
                let parent = Cidr::parse(parent_cidr)?;
                if !parent.contains(child) {
                    return Err(format!(
                        "{name}: subnet {cidr} is outside VPC {parent_cidr}"
                    ));
                }
                if subnets
                    .iter()
                    .any(|(_, parent, previous)| *parent == vpc && previous.overlaps(child))
                {
                    return Err(format!(
                        "{name}: subnet CIDR overlaps another subnet in this VPC"
                    ));
                }
                subnets.push((*id, vpc, child));
                let mut body = json!({"vpc_id": reference("aws_vpc", vpc), "cidr_block": cidr, "map_public_ip_on_launch": false});
                if !availability_zone.is_empty() {
                    if !availability_zone.starts_with(&target.region)
                        || !availability_zone
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
                    {
                        return Err(format!(
                            "{name}: availability zone must belong to {}",
                            target.region
                        ));
                    }
                    body["availability_zone"] = availability_zone.clone().into();
                }
                ("aws_subnet", body)
            }
            other => services::generate(*id, other, &topics, &mut resources)
                .map_err(|error| format!("{name}: {error}"))?,
        };
        if let Some(aws_name) = body
            .get("name")
            .or_else(|| body.get("bucket"))
            .and_then(serde_json::Value::as_str)
            && !service_names.insert((kind, aws_name.to_owned()))
        {
            return Err(format!(
                "{name}: another {kind} on this page uses the name {aws_name:?}; choose a unique name"
            ));
        }
        body["tags"] = json!({"Name": literal(&name), "ManagedBy": "Blueprint", "BlueprintElement": id.to_string()});
        let logical = logical_name(*id);
        resources.entry(kind).or_insert_with(|| json!({}))[&logical] = body;
        outputs.insert(
            format!("{logical}_id"),
            json!({"value": reference(kind, *id)}),
        );
        if matches!(
            resource,
            AwsResource::S3Bucket { .. }
                | AwsResource::DynamoDbTable { .. }
                | AwsResource::SnsTopic { .. }
                | AwsResource::SqsQueue { .. }
        ) {
            outputs.insert(
                format!("{logical}_arn"),
                json!({"value": reference_attribute(kind, *id, "arn")}),
            );
        }
        names.push(format!("{} — {}", name, resource.label()));
    }
    let mut provider = json!({"region": target.region, "allowed_account_ids": [target.account_id]});
    if !target.profile.is_empty() {
        provider["profile"] = literal(&target.profile).into();
    }
    outputs.insert(
        "aws_account_id".into(),
        json!({"value": "${data.aws_caller_identity.current.account_id}"}),
    );
    let value = json!({
        "terraform": {"required_version": ">= 1.5, < 2.0", "required_providers": {"aws": {"source": "hashicorp/aws", "version": "~> 6.0"}}},
        "provider": {"aws": provider},
        "data": {"aws_caller_identity": {"current": {}}},
        "resource": resources,
        "output": outputs
    });
    let ignored = elements
        .iter()
        .filter(|el| {
            el.as_shape()
                .is_some_and(|s| s.shape.is_cloud() && s.infrastructure.is_none())
        })
        .count();
    let warnings = if ignored > 0 {
        vec![format!(
            "{ignored} cloud icon(s) have no resource configuration and will remain diagram-only."
        )]
    } else {
        Vec::new()
    };
    Ok(Configuration {
        page,
        account_id: target.account_id.clone(),
        json: format!(
            "{}\n",
            serde_json::to_string_pretty(&value).map_err(|e| e.to_string())?
        ),
        resources: names,
        warnings,
    })
}

fn validate_target(target: &AwsDeployment) -> Result<(), String> {
    if target.account_id.len() != 12 || !target.account_id.bytes().all(|b| b.is_ascii_digit()) {
        return Err("Enter the 12-digit AWS account ID you intend to deploy into".into());
    }
    if target.region.is_empty()
        || !target
            .region
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        || !target.region.ends_with(|c: char| c.is_ascii_digit())
    {
        return Err("Enter an AWS region such as us-east-1".into());
    }
    if target.profile.chars().any(char::is_control) {
        return Err("AWS profile names cannot contain control characters".into());
    }
    Ok(())
}

fn element_name(element: &bp_model::Element) -> String {
    element
        .text()
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| element.id.to_string())
}

fn literal(value: &str) -> String {
    value.replace("${", "$${").replace("%{", "%%{")
}

fn logical_name(id: ElementId) -> String {
    format!("r_{}", id.to_string().replace('-', ""))
}

fn reference(kind: &str, id: ElementId) -> String {
    reference_attribute(kind, id, "id")
}

fn reference_attribute(kind: &str, id: ElementId, attribute: &str) -> String {
    format!("${{{kind}.{}.{attribute}}}", logical_name(id))
}

#[derive(Clone, Copy)]
struct Cidr {
    start: u32,
    end: u32,
}

impl Cidr {
    fn parse(value: &str) -> Result<Self, String> {
        let invalid = || format!("{value:?} must be an aligned IPv4 CIDR with a /16 to /28 prefix");
        let (ip, prefix) = value.split_once('/').ok_or_else(invalid)?;
        let ip = ip.parse::<Ipv4Addr>().map_err(|_| invalid())?;
        let prefix = prefix.parse::<u32>().map_err(|_| invalid())?;
        if !(16..=28).contains(&prefix) {
            return Err(invalid());
        }
        let start = u32::from(ip);
        let mask = u32::MAX << (32 - prefix);
        if start & mask != start {
            return Err(invalid());
        }
        Ok(Self {
            start,
            end: start | !mask,
        })
    }
    fn contains(self, other: Self) -> bool {
        self.start <= other.start && self.end >= other.end
    }
    fn overlaps(self, other: Self) -> bool {
        self.start <= other.end && other.start <= self.end
    }
}
