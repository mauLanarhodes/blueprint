use crate::{
    IconError, IconPack, MAX_ARCHIVE_ENTRIES, MAX_SVG_BYTES, MAX_TOTAL_SVG_BYTES, validate_svg,
    validate_version,
};
use bp_model::{CloudIcon, CloudProvider, IconKind, ShapeRef};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Read};
use std::sync::Arc;
use zip::ZipArchive;

struct Candidate {
    icon: CloudIcon,
    size: u32,
}

/// Imports SVGs from an official provider ZIP, preserving their exact UTF-8
/// contents. Repeated size variants resolve to the largest variant, independently
/// of ZIP entry order. No archive path is ever extracted to disk.
pub fn import_zip(
    bytes: &[u8],
    provider: CloudProvider,
    version: &str,
) -> Result<IconPack, IconError> {
    validate_version(version)?;
    let mut archive = ZipArchive::new(Cursor::new(bytes))?;
    if archive.len() > MAX_ARCHIVE_ENTRIES {
        return Err(IconError::Limit(format!(
            "archive has more than {MAX_ARCHIVE_ENTRIES} entries"
        )));
    }
    let mut seen_paths = BTreeSet::new();
    let mut candidates: BTreeMap<String, Candidate> = BTreeMap::new();
    let mut total = 0usize;
    let mut warnings = Vec::new();
    for index in 0..archive.len() {
        let mut file = archive.by_index(index)?;
        let source_path = file.name().to_owned();
        safe_path(&source_path)?;
        if file
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            return Err(IconError::UnsafePath(source_path));
        }
        if file.is_dir() || is_metadata(&source_path) {
            continue;
        }
        if !source_path.to_ascii_lowercase().ends_with(".svg") {
            continue;
        }
        if !seen_paths.insert(source_path.clone()) {
            return Err(IconError::Collision {
                id: source_path.clone(),
                first: source_path.clone(),
                second: source_path,
            });
        }
        if file.size() > MAX_SVG_BYTES as u64 {
            return Err(IconError::Limit(format!(
                "{source_path} exceeds {MAX_SVG_BYTES} bytes"
            )));
        }
        if file.size() > (MAX_TOTAL_SVG_BYTES - total) as u64 {
            return Err(IconError::Limit(format!(
                "SVGs exceed {MAX_TOTAL_SVG_BYTES} total unpacked bytes"
            )));
        }
        let mut svg_bytes = Vec::new();
        file.by_ref()
            .take((MAX_SVG_BYTES + 1) as u64)
            .read_to_end(&mut svg_bytes)?;
        if svg_bytes.len() > MAX_SVG_BYTES {
            return Err(IconError::Limit(format!(
                "{source_path} exceeds {MAX_SVG_BYTES} bytes"
            )));
        }
        total = total
            .checked_add(svg_bytes.len())
            .ok_or_else(|| IconError::Limit("unpacked SVG byte count overflowed".into()))?;
        if total > MAX_TOTAL_SVG_BYTES {
            return Err(IconError::Limit(format!(
                "SVGs exceed {MAX_TOTAL_SVG_BYTES} total unpacked bytes"
            )));
        }
        let svg = String::from_utf8(svg_bytes).map_err(|e| IconError::Svg {
            path: source_path.clone(),
            reason: format!("SVG must be UTF-8: {e}"),
        })?;
        validate_svg(&svg).map_err(|reason| IconError::Svg {
            path: source_path.clone(),
            reason,
        })?;
        let (category, name, kind, size) = metadata(&source_path, provider)?;
        let base_id = format!(
            "{}-{}-{}@{version}",
            slug(&category),
            kind_slug(kind),
            slug(&name)
        );
        // Azure assigns numeric asset IDs to distinct artwork with the same
        // service name. Keep those candidates separate before resolving sizes.
        let id = if provider == CloudProvider::Azure {
            azure_asset_id(&base_id, &source_path).unwrap_or_else(|| base_id.clone())
        } else {
            base_id.clone()
        };
        let icon = CloudIcon {
            reference: ShapeRef::new(provider.id(), &base_id),
            name,
            provider,
            category,
            kind,
            pack_version: version.into(),
            source_path: source_path.clone(),
            svg: Arc::from(svg),
        };
        if let Some(previous) = candidates.get_mut(&id) {
            if previous.icon.name != icon.name
                || previous.icon.category != icon.category
                || (previous.size == size && previous.icon.svg != icon.svg)
            {
                return Err(IconError::Collision {
                    id,
                    first: previous.icon.source_path.clone(),
                    second: source_path,
                });
            }
            warnings.push(format!("{}: kept the largest SVG variant", icon.name));
            if size > previous.size
                || (size == previous.size && source_path < previous.icon.source_path)
            {
                *previous = Candidate { icon, size };
            }
        } else {
            candidates.insert(id, Candidate { icon, size });
        }
    }
    if candidates.is_empty() {
        return Err(IconError::Empty(provider.label().into()));
    }
    let mut name_counts = BTreeMap::new();
    for candidate in candidates.values() {
        *name_counts
            .entry(candidate.icon.reference.clone())
            .or_insert(0) += 1;
    }
    let mut icons: Vec<_> = candidates
        .into_iter()
        .map(|(id, mut candidate)| {
            // Preserve existing IDs for unambiguous names. Every member of an
            // ambiguous name gets its vendor ID, independently of ZIP order.
            if name_counts[&candidate.icon.reference] > 1 {
                candidate.icon.reference = ShapeRef::new(provider.id(), &id);
            }
            candidate.icon
        })
        .collect();
    icons.sort_by(|a, b| a.reference.cmp(&b.reference));
    warnings.sort();
    warnings.dedup();
    Ok(IconPack {
        provider,
        version: version.into(),
        icons,
        warnings,
    })
}

/// Disambiguate official Azure filenames using their numeric asset identity.
/// A double hyphen cannot occur in a name slug, avoiding collisions with names
/// that happen to end in the same number. Existing unambiguous IDs stay valid.
pub(crate) fn azure_asset_id(base_id: &str, source_path: &str) -> Option<String> {
    let filename = source_path.rsplit('/').next()?;
    let (number, _) = filename
        .split_once("-icon-service-")
        .or_else(|| filename.split_once("-icon-resource-"))?;
    if number.is_empty() || !number.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let (name, version) = base_id.rsplit_once('@')?;
    Some(format!("{name}--{number}@{version}"))
}

pub(crate) fn safe_path(path: &str) -> Result<(), IconError> {
    if path.is_empty()
        || path.starts_with('/')
        || path.contains('\\')
        || path.contains('\0')
        || path
            .split('/')
            .any(|part| matches!(part, "." | "..") || part.contains(':'))
    {
        return Err(IconError::UnsafePath(path.into()));
    }
    Ok(())
}

fn is_metadata(path: &str) -> bool {
    path.split('/')
        .any(|part| part == "__MACOSX" || part.starts_with("._") || part == ".DS_Store")
}

fn metadata(
    path: &str,
    provider: CloudProvider,
) -> Result<(String, String, IconKind, u32), IconError> {
    let parts: Vec<_> = path.split('/').collect();
    let filename = parts.last().copied().unwrap_or_default();
    let stem = &filename[..filename.len() - 4];
    let lower_path = path.to_ascii_lowercase();
    let folders = &parts[..parts.len() - 1];
    let kind = if folders.iter().any(|folder| {
        folder
            .to_ascii_lowercase()
            .starts_with("architecture-group")
    }) {
        IconKind::Group
    } else if stem.starts_with("Res_")
        || stem.contains("-icon-resource-")
        || folders.iter().any(|folder| {
            folder.starts_with("Res_") || folder.to_ascii_lowercase().starts_with("resource-icons")
        })
    {
        IconKind::Resource
    } else {
        IconKind::Service
    };
    let mut size = 0;
    let stem = if provider == CloudProvider::Aws {
        let mut stem = stem
            .strip_prefix("Arch_")
            .or_else(|| stem.strip_prefix("Res_"))
            .or_else(|| stem.strip_prefix("Arch-Category_"))
            .unwrap_or(stem)
            .to_owned();
        // General resource artwork has `_48_Light` / `_48_Dark` suffixes.
        // Retain named themes as distinct original artwork, removing only size.
        let theme = stem.rsplit_once('_').and_then(|(name, suffix)| {
            matches!(suffix, "Light" | "Dark").then(|| (name.to_owned(), suffix.to_owned()))
        });
        if let Some((name, _)) = &theme {
            stem = name.clone();
        }
        if let Some((name, suffix)) = stem.rsplit_once('_')
            && suffix.parse::<u32>().is_ok()
        {
            size = suffix.parse().unwrap_or_default();
            stem = name.to_owned();
        }
        if let Some((_, theme)) = theme {
            stem.push('_');
            stem.push_str(&theme);
        }
        stem
    } else {
        stem.split_once("-icon-service-")
            .or_else(|| stem.split_once("-icon-resource-"))
            .map_or(stem, |(_, name)| name)
            .to_owned()
    };
    let category = if lower_path.contains("category-icons") {
        "Categories".to_owned()
    } else {
        parts[..parts.len() - 1]
            .iter()
            .rev()
            .find_map(|part| {
                let folder = part.strip_prefix("Res_").unwrap_or(part);
                let size_part = folder
                    .strip_suffix("_Light")
                    .or_else(|| folder.strip_suffix("_Dark"))
                    .unwrap_or(folder);
                if let Ok(folder_size) = size_part.parse::<u32>() {
                    size = size.max(folder_size);
                    return None;
                }
                let lower = part.to_ascii_lowercase();
                if lower.is_empty()
                    || matches!(lower.as_str(), "svg" | "icons" | "icon" | "light" | "dark")
                    || lower.contains("architecture")
                    || lower.starts_with("aws-architecture")
                {
                    return None;
                }
                Some(pretty(
                    part.strip_prefix("Arch_")
                        .or_else(|| part.strip_prefix("Res_"))
                        .unwrap_or(part),
                ))
            })
            .unwrap_or_else(|| {
                match kind {
                    IconKind::Group => "Groups",
                    _ => "General",
                }
                .into()
            })
    };
    let name = pretty(&stem);
    if slug(&name).is_empty() || slug(&category).is_empty() {
        return Err(IconError::Catalog(format!(
            "cannot derive an icon name/category from {path:?}"
        )));
    }
    Ok((category, name, kind, size))
}

pub(crate) fn slug(value: &str) -> String {
    let mut result = String::new();
    let mut separator = false;
    for c in value.chars() {
        if c.is_ascii_alphanumeric() {
            if separator && !result.is_empty() {
                result.push('-');
            }
            result.push(c.to_ascii_lowercase());
            separator = false;
        } else {
            separator = true;
        }
    }
    result
}

fn pretty(value: &str) -> String {
    value
        .split(['-', '_', ' '])
        .filter(|word| !word.is_empty())
        .map(|word| {
            let mut chars = word.chars();
            let first = chars.next().unwrap();
            format!("{}{}", first.to_uppercase(), chars.as_str())
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) fn kind_slug(kind: IconKind) -> &'static str {
    match kind {
        IconKind::Service => "service",
        IconKind::Resource => "resource",
        IconKind::Group => "group",
    }
}
