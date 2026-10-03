//! Import official cloud icon ZIPs locally, without downloading or changing their SVGs.
//!
//! Catalogs are immutable by provider and pack version. A document can therefore
//! keep referring to its original artwork after a newer pack is installed.

mod catalog;
mod import;
mod search;
mod svg;

pub use catalog::{install_pack, load_packs};
pub use import::import_zip;
pub use search::{aliases, search};
pub use svg::{offline_svg_options, validate_svg};

use bp_model::{CloudIcon, CloudProvider};
use serde::{Deserialize, Serialize};

/// Deliberately generous enough for current official packs, but bounded before
/// decompression and parsing. PNG/PDF resources are never decompressed.
pub const MAX_ARCHIVE_ENTRIES: usize = 30_000;
pub const MAX_ZIP_BYTES: usize = 512 * 1024 * 1024;
pub const MAX_SVG_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_TOTAL_SVG_BYTES: usize = 128 * 1024 * 1024;
pub(crate) const MAX_CATALOG_BYTES: usize = 3 * MAX_TOTAL_SVG_BYTES;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IconPack {
    pub provider: CloudProvider,
    pub version: String,
    pub icons: Vec<CloudIcon>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum IconError {
    #[error("could not read or write the icon pack: {0}")]
    Io(#[from] std::io::Error),
    #[error("the icon ZIP is damaged or unsupported: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("the installed icon catalog is invalid: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid pack version {0:?}; use 1–64 letters, digits, dots, underscores or hyphens")]
    InvalidVersion(String),
    #[error("unsafe archive entry {0:?}; paths must be relative and cannot be symlinks")]
    UnsafePath(String),
    #[error("icon import limit exceeded: {0}")]
    Limit(String),
    #[error("invalid SVG {path:?}: {reason}")]
    Svg { path: String, reason: String },
    #[error(
        "ambiguous icon ID {id:?}: {first:?} and {second:?}; choose an archive with one variant per size"
    )]
    Collision {
        id: String,
        first: String,
        second: String,
    },
    #[error("no usable SVG icons found; select the official {0} architecture icon ZIP")]
    Empty(String),
    #[error(
        "{provider} pack version {version:?} is already installed with different contents; use the release's actual version"
    )]
    AlreadyInstalled { provider: String, version: String },
    #[error("invalid icon catalog: {0}")]
    Catalog(String),
}

pub(crate) fn validate_version(version: &str) -> Result<(), IconError> {
    if version.is_empty()
        || version.len() > 64
        || matches!(version, "." | "..")
        || !version
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
    {
        return Err(IconError::InvalidVersion(version.into()));
    }
    Ok(())
}
