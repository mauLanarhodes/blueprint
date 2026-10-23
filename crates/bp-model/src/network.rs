//! Network boundaries use ordinary parent pointers and absolute page coordinates.

use kurbo::{Rect, Size};
use serde::{Deserialize, Serialize};

pub const NETWORK_CONTAINER_HEADER_HEIGHT: f64 = 56.0;
pub const NETWORK_CONTAINER_PADDING: f64 = 16.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkContainer {
    Vpc,
    Subnet,
}

impl NetworkContainer {
    pub fn label(self) -> &'static str {
        match self {
            Self::Vpc => "VPC",
            Self::Subnet => "Subnet",
        }
    }

    pub fn min_size(self) -> Size {
        match self {
            Self::Vpc => Size::new(360.0, 240.0),
            Self::Subnet => Size::new(240.0, 180.0),
        }
    }

    pub fn default_size(self) -> Size {
        match self {
            Self::Vpc => Size::new(960.0, 520.0),
            Self::Subnet => Size::new(440.0, 360.0),
        }
    }

    pub fn header_rect(self, bounds: Rect) -> Rect {
        Rect::new(
            bounds.x0,
            bounds.y0,
            bounds.x1,
            bounds.y0 + NETWORK_CONTAINER_HEADER_HEIGHT,
        )
    }

    pub fn content_rect(self, bounds: Rect) -> Rect {
        Rect::new(
            bounds.x0 + NETWORK_CONTAINER_PADDING,
            bounds.y0 + NETWORK_CONTAINER_HEADER_HEIGHT + NETWORK_CONTAINER_PADDING,
            bounds.x1 - NETWORK_CONTAINER_PADDING,
            bounds.y1 - NETWORK_CONTAINER_PADDING,
        )
    }
}
