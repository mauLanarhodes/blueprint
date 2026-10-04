use crate::import::{azure_asset_id, kind_slug, safe_path, slug};
use crate::{
    IconError, IconPack, MAX_CATALOG_BYTES, MAX_TOTAL_SVG_BYTES, validate_svg, validate_version,
};
use bp_model::{CloudProvider, ShapeRef};
use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const CATALOG_FILE: &str = "catalog.json";

/// Atomically publishes an immutable catalog at `root/provider/version/`.
/// Artwork is stored verbatim in JSON rather than extracted archive paths.
/// Reinstalling identical artwork is harmless; differing same-version artwork
/// is rejected so existing documents cannot silently change appearance.
pub fn install_pack(root: &Path, pack: &IconPack) -> Result<PathBuf, IconError> {
    validate_pack(pack)?;
    ensure_directory(root)?;
    let provider_dir = root.join(pack.provider.id());
    ensure_directory(&provider_dir)?;
    let destination = provider_dir.join(&pack.version);
    if destination.try_exists()? {
        return compare_existing(&destination, pack);
    }
    let (temporary, mut file) = create_temporary(&provider_dir)?;
    let result = (|| {
        serde_json::to_writer(&mut file, pack)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        drop(file);
        match fs::rename(&temporary, &destination) {
            Ok(()) => {
                #[cfg(unix)]
                File::open(&provider_dir)?.sync_all()?;
                Ok(destination.clone())
            }
            Err(_) if destination.try_exists()? => compare_existing(&destination, pack),
            Err(error) => Err(IconError::Io(error)),
        }
    })();
    let _ = fs::remove_dir_all(&temporary);
    result
}

/// Loads all installed versions in stable provider/version order. Hidden
/// temporary directories left by interrupted imports are ignored.
pub fn load_packs(root: &Path) -> Result<Vec<IconPack>, IconError> {
    if !root.try_exists()? {
        return Ok(Vec::new());
    }
    require_directory(root)?;
    let mut packs = Vec::new();
    for provider in [CloudProvider::Aws, CloudProvider::Azure] {
        let provider_dir = root.join(provider.id());
        if !provider_dir.try_exists()? {
            continue;
        }
        require_directory(&provider_dir)?;
        let mut versions = fs::read_dir(&provider_dir)?.collect::<Result<Vec<_>, _>>()?;
        versions.sort_by_key(|entry| entry.file_name());
        for entry in versions {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            validate_version(&name)?;
            let pack = read_pack(&entry.path())?;
            if pack.provider != provider || pack.version != name {
                return Err(IconError::Catalog(format!(
                    "catalog metadata does not match {}",
                    entry.path().display()
                )));
            }
            packs.push(pack);
        }
    }
    Ok(packs)
}

fn validate_pack(pack: &IconPack) -> Result<(), IconError> {
    validate_version(&pack.version)?;
    if pack.icons.is_empty() {
        return Err(IconError::Empty(pack.provider.label().into()));
    }
    let mut ids = BTreeSet::new();
    let mut total = 0usize;
    for icon in &pack.icons {
        let base_id = format!(
            "{}-{}-{}@{}",
            slug(&icon.category),
            kind_slug(icon.kind),
            slug(&icon.name),
            pack.version
        );
        let expected = ShapeRef::new(pack.provider.id(), &base_id);
        let azure_expected = (pack.provider == CloudProvider::Azure)
            .then(|| azure_asset_id(&base_id, &icon.source_path))
            .flatten()
            .map(|id| ShapeRef::new(pack.provider.id(), &id));
        if icon.provider != pack.provider
            || icon.pack_version != pack.version
            || (icon.reference != expected && Some(&icon.reference) != azure_expected.as_ref())
            || icon.name.trim().is_empty()
            || icon.category.trim().is_empty()
        {
            return Err(IconError::Catalog(format!(
                "metadata for {} does not match its pack",
                icon.reference
            )));
        }
        if !ids.insert(&icon.reference) {
            return Err(IconError::Catalog(format!(
                "duplicate icon ID {}",
                icon.reference
            )));
        }
        safe_path(&icon.source_path)?;
        total = total
            .checked_add(icon.svg.len())
            .ok_or_else(|| IconError::Limit("catalog byte count overflowed".into()))?;
        if total > MAX_TOTAL_SVG_BYTES {
            return Err(IconError::Limit(
                "installed catalog SVGs exceed the unpacked size limit".into(),
            ));
        }
        validate_svg(&icon.svg).map_err(|reason| IconError::Svg {
            path: icon.source_path.clone(),
            reason,
        })?;
    }
    Ok(())
}

fn read_pack(directory: &Path) -> Result<IconPack, IconError> {
    require_directory(directory)?;
    let path = directory.join(CATALOG_FILE);
    let metadata = fs::symlink_metadata(&path)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(IconError::UnsafePath(path.display().to_string()));
    }
    if metadata.len() > MAX_CATALOG_BYTES as u64 {
        return Err(IconError::Limit("installed catalog is too large".into()));
    }
    let mut bytes = Vec::new();
    File::open(path)?
        .take((MAX_CATALOG_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_CATALOG_BYTES {
        return Err(IconError::Limit("installed catalog is too large".into()));
    }
    let pack: IconPack = serde_json::from_slice(&bytes)?;
    validate_pack(&pack)?;
    Ok(pack)
}

fn compare_existing(directory: &Path, pack: &IconPack) -> Result<PathBuf, IconError> {
    let previous = read_pack(directory)?;
    // Warning wording is incidental; immutable contents are the actual icons.
    if previous.provider == pack.provider
        && previous.version == pack.version
        && previous.icons == pack.icons
    {
        Ok(directory.into())
    } else {
        Err(IconError::AlreadyInstalled {
            provider: pack.provider.label().into(),
            version: pack.version.clone(),
        })
    }
}

fn require_directory(path: &Path) -> Result<(), IconError> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(IconError::UnsafePath(path.display().to_string()));
    }
    Ok(())
}

fn ensure_directory(path: &Path) -> Result<(), IconError> {
    match fs::symlink_metadata(path) {
        Ok(_) => require_directory(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir_all(path)?;
            require_directory(path)
        }
        Err(error) => Err(error.into()),
    }
}

fn create_temporary(provider_dir: &Path) -> Result<(PathBuf, File), IconError> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    loop {
        let path = provider_dir.join(format!(
            ".import-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        match fs::create_dir(&path) {
            Ok(()) => {
                match OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(path.join(CATALOG_FILE))
                {
                    Ok(file) => return Ok((path, file)),
                    Err(error) => {
                        let _ = fs::remove_dir(&path);
                        return Err(error.into());
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }
}
