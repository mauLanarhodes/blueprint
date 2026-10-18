//! Deployment settings contain resource configuration, never credentials or state.

use crate::ElementId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AwsDeployment {
    pub region: String,
    /// An optional local AWS shared-config profile name.
    pub profile: String,
    /// Required when planning: the provider refuses a different account.
    pub account_id: String,
}

impl Default for AwsDeployment {
    fn default() -> Self {
        Self {
            region: "us-east-1".into(),
            profile: String::new(),
            account_id: String::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AwsResource {
    Vpc {
        cidr: String,
    },
    Subnet {
        vpc: Option<ElementId>,
        cidr: String,
        /// Empty lets AWS select an availability zone.
        availability_zone: String,
    },
    S3Bucket {
        name: String,
        versioning: bool,
    },
    DynamoDbTable {
        name: String,
        partition_key: String,
        /// The trial uses string keys; an empty sort key disables it.
        sort_key: String,
    },
    SnsTopic {
        name: String,
    },
    SqsQueue {
        name: String,
        visibility_timeout: u32,
        retention_seconds: u32,
        /// A same-page standard SNS topic, with an explicit queue policy.
        topic: Option<ElementId>,
    },
}

impl AwsResource {
    pub fn label(&self) -> &'static str {
        self.kind().label()
    }

    pub fn kind(&self) -> AwsResourceKind {
        match self {
            Self::Vpc { .. } => AwsResourceKind::Vpc,
            Self::Subnet { .. } => AwsResourceKind::Subnet,
            Self::S3Bucket { .. } => AwsResourceKind::S3Bucket,
            Self::DynamoDbTable { .. } => AwsResourceKind::DynamoDbTable,
            Self::SnsTopic { .. } => AwsResourceKind::SnsTopic,
            Self::SqsQueue { .. } => AwsResourceKind::SqsQueue,
        }
    }

    /// Dependencies outside a copied selection are cleared rather than silently
    /// pointing at another page or the original network.
    pub fn remap(&mut self, ids: &HashMap<ElementId, ElementId>) {
        match self {
            Self::Subnet { vpc, .. } => *vpc = vpc.and_then(|id| ids.get(&id).copied()),
            Self::SqsQueue { topic, .. } => *topic = topic.and_then(|id| ids.get(&id).copied()),
            _ => {}
        }
    }
}

/// The trial's supported services. Adding a service means adding its saved
/// settings, inspector fields, generator and provider validation coverage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AwsResourceKind {
    Vpc,
    Subnet,
    S3Bucket,
    DynamoDbTable,
    SnsTopic,
    SqsQueue,
}

impl AwsResourceKind {
    pub const ALL: [Self; 6] = [
        Self::Vpc,
        Self::Subnet,
        Self::S3Bucket,
        Self::DynamoDbTable,
        Self::SnsTopic,
        Self::SqsQueue,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Vpc => "AWS VPC",
            Self::Subnet => "AWS private subnet",
            Self::S3Bucket => "Amazon S3 bucket",
            Self::DynamoDbTable => "Amazon DynamoDB table",
            Self::SnsTopic => "Amazon SNS topic",
            Self::SqsQueue => "Amazon SQS queue",
        }
    }

    pub fn create(self, owner: ElementId) -> AwsResource {
        // Random UUID suffixes remain unique even when several trial stacks
        // are created in the same account, unlike a page-wide default name.
        let suffix = owner.to_string().replace('-', "");
        let name = format!("blueprint-{}", &suffix[16..]);
        match self {
            Self::Vpc => AwsResource::Vpc {
                cidr: "10.0.0.0/16".into(),
            },
            Self::Subnet => AwsResource::Subnet {
                vpc: None,
                cidr: "10.0.1.0/24".into(),
                availability_zone: String::new(),
            },
            Self::S3Bucket => AwsResource::S3Bucket {
                name,
                versioning: true,
            },
            Self::DynamoDbTable => AwsResource::DynamoDbTable {
                name,
                partition_key: "id".into(),
                sort_key: String::new(),
            },
            Self::SnsTopic => AwsResource::SnsTopic { name },
            Self::SqsQueue => AwsResource::SqsQueue {
                name,
                visibility_timeout: 30,
                retention_seconds: 345_600,
                topic: None,
            },
        }
    }

    /// Suggestions are explicit choices, never automatic deployments. Artwork
    /// for other services remains selectable as diagram-only in this trial.
    pub fn suggest(icon_name: &str) -> Option<Self> {
        let name: String = icon_name
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .flat_map(char::to_lowercase)
            .collect();
        match name.as_str() {
            "vpc" | "amazonvpc" | "virtualprivatecloud" => Some(Self::Vpc),
            "subnet" | "privatesubnet" => Some(Self::Subnet),
            "s3" | "amazons3" | "s3bucket" | "amazonsimplestorageservice" => Some(Self::S3Bucket),
            "dynamodb" | "amazondynamodb" | "dynamodbtable" => Some(Self::DynamoDbTable),
            "sns"
            | "amazonsns"
            | "simplenotificationservice"
            | "amazonsimplenotificationservice" => Some(Self::SnsTopic),
            "sqs" | "amazonsqs" | "simplequeueservice" | "amazonsimplequeueservice" => {
                Some(Self::SqsQueue)
            }
            _ => None,
        }
    }
}
