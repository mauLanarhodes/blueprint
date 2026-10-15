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
    /// General deployment drafts. Execution is enabled in a later checkpoint;
    /// retaining legacy variants preserves their existing Terraform addresses.
    Resources {
        resources: Vec<crate::TerraformResource>,
    },
    Vpc {
        cidr: String,
    },
    Subnet {
        vpc: Option<ElementId>,
        cidr: String,
        /// Empty lets AWS select an availability zone.
        availability_zone: String,
    },
}

impl AwsResource {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Resources { .. } => "AWS resource configuration",
            Self::Vpc { .. } => "AWS VPC",
            Self::Subnet { .. } => "AWS private subnet",
        }
    }

    /// Dependencies outside a copied selection are cleared rather than silently
    /// pointing at another page or the original network.
    pub fn remap(&mut self, ids: &HashMap<ElementId, ElementId>) {
        if let Self::Resources { resources } = self {
            for resource in resources {
                resource.remap(ids);
            }
        }
        if let Self::Subnet { vpc, .. } = self {
            *vpc = vpc.and_then(|id| ids.get(&id).copied());
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::Resources { resources } => crate::terraform::validate_resources(resources),
            _ => Ok(()),
        }
    }
}
