//! Reading and writing `.blueprint` project files.
//!
//! A `.blueprint` file is a zip archive holding `document.json` and the
//! cloud SVGs used by the document. A file whose name ends in `.json` is saved
//! as plain, pretty-printed JSON for readable Git diffs. When opening, the
//! format is detected from the content, not the extension, and files from
//! older versions are migrated to the current schema.

mod migrate;

use bp_model::{Document, ModelError, SCHEMA_VERSION, ShapeRef};
use serde_json::Value;
use std::collections::HashSet;
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

pub const FILE_EXTENSION: &str = "blueprint";
const DOCUMENT_ENTRY: &str = "document.json";
const ZIP_MAGIC: &[u8] = b"PK\x03\x04";
const MAX_DOCUMENT_BYTES: u64 = 64 * 1024 * 1024;
const MAX_ICON_BYTES: u64 = bp_icons::MAX_SVG_BYTES as u64;
const MAX_ASSET_BYTES: usize = 128 * 1024 * 1024;
const MAX_ARCHIVE_ENTRIES: usize = 10_001;

#[derive(Debug, thiserror::Error)]
pub enum IoError {
    #[error("could not read or write the file: {0}")]
    Io(#[from] std::io::Error),
    #[error("the file is not a valid Blueprint project: {0}")]
    Json(#[from] serde_json::Error),
    #[error("the project archive is damaged: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("the project is inconsistent: {0}")]
    Model(#[from] ModelError),
    #[error(
        "the file was made by a newer Blueprint (format {found}, this build reads up to {SCHEMA_VERSION})"
    )]
    TooNew { found: u64 },
    #[error("the file has no schema_version field")]
    NoVersion,
    #[error("the file could not be upgraded: {0}")]
    Migration(String),
    #[error("the embedded icon assets are invalid: {0}")]
    IconAsset(String),
}

/// Saves `doc` to `path` atomically: the old file stays intact until the new
/// one is completely written.
pub fn save(doc: &Document, path: &Path) -> Result<(), IoError> {
    let bytes = if is_json_path(path) {
        to_json_bytes(doc)?
    } else {
        to_zip_bytes(doc)?
    };
    atomic_write(path, &bytes)
}

pub fn load(path: &Path) -> Result<Document, IoError> {
    from_bytes(&fs::read(path)?)
}

/// Parses either format.
pub fn from_bytes(bytes: &[u8]) -> Result<Document, IoError> {
    if bytes.starts_with(ZIP_MAGIC) {
        let mut archive = ZipArchive::new(Cursor::new(bytes))?;
        if archive.len() > MAX_ARCHIVE_ENTRIES {
            return Err(IoError::IconAsset("too many archive entries".into()));
        }
        let mut names = HashSet::new();
        for name in archive.file_names() {
            if !names.insert(name) {
                return Err(IoError::IconAsset(format!(
                    "duplicate archive entry {name:?}"
                )));
            }
        }
        let json = read_entry(&mut archive, DOCUMENT_ENTRY, MAX_DOCUMENT_BYTES)?;
        let mut value: Value = serde_json::from_slice(&json)?;
        hydrate_icons(&mut value, &mut archive)?;
        from_value(value)
    } else {
        from_json_bytes(bytes)
    }
}

pub fn to_zip_bytes(doc: &Document) -> Result<Vec<u8>, IoError> {
    if doc.icons.len() >= MAX_ARCHIVE_ENTRIES {
        return Err(IoError::IconAsset("too many archive entries".into()));
    }
    doc.validate()?;
    validate_icons(doc)?;
    let mut value = serde_json::to_value(doc)?;
    if let Some(icons) = value.get_mut("icons").and_then(Value::as_object_mut) {
        for reference in doc.icons.keys() {
            let metadata = icons
                .get_mut(reference.as_str())
                .and_then(Value::as_object_mut)
                .expect("serialized CloudIcon is an object");
            metadata.remove("svg");
            metadata.insert("asset_path".into(), icon_asset_path(reference)?.into());
        }
    }
    let json = serde_json::to_vec_pretty(&value)?;
    // The stored metadata also includes the trailing newline.
    if json.len() as u64 >= MAX_DOCUMENT_BYTES {
        return Err(IoError::IconAsset("document metadata is too large".into()));
    }
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    zip.start_file(DOCUMENT_ENTRY, options)?;
    zip.write_all(&json)?;
    zip.write_all(b"\n")?;
    for (reference, icon) in &doc.icons {
        zip.start_file(icon_asset_path(reference)?, options)?;
        zip.write_all(icon.svg.as_bytes())?;
    }
    Ok(zip.finish()?.into_inner())
}

pub fn to_json_bytes(doc: &Document) -> Result<Vec<u8>, IoError> {
    doc.validate()?;
    validate_icons(doc)?;
    let mut bytes = serde_json::to_vec_pretty(doc)?;
    bytes.push(b'\n');
    if bytes.len() as u64 > MAX_DOCUMENT_BYTES {
        return Err(IoError::IconAsset("document JSON is too large".into()));
    }
    Ok(bytes)
}

fn from_json_bytes(bytes: &[u8]) -> Result<Document, IoError> {
    if bytes.len() as u64 > MAX_DOCUMENT_BYTES {
        return Err(IoError::IconAsset("document JSON is too large".into()));
    }
    let value: Value = serde_json::from_slice(bytes)?;
    from_value(value)
}

fn from_value(value: Value) -> Result<Document, IoError> {
    let doc: Document = serde_json::from_value(migrate::migrate(value)?)?;
    doc.validate()?;
    validate_icons(&doc)?;
    Ok(doc)
}

fn validate_icons(doc: &Document) -> Result<(), IoError> {
    let mut total = 0;
    for icon in doc.icons.values() {
        check_asset_size(icon.svg.len(), &mut total)?;
        bp_icons::validate_svg(&icon.svg)
            .map_err(|error| IoError::IconAsset(format!("{}: {error}", icon.reference)))?;
    }
    Ok(())
}

fn icon_asset_path(reference: &ShapeRef) -> Result<String, IoError> {
    if ShapeRef::parse(reference.as_str()).is_none()
        || !reference.is_cloud()
        || matches!(reference.shape(), "." | "..")
    {
        return Err(IoError::IconAsset("unsafe icon reference".into()));
    }
    Ok(format!("icons/{}.svg", reference.as_str()))
}

fn check_asset_size(size: usize, total: &mut usize) -> Result<(), IoError> {
    if size as u64 > MAX_ICON_BYTES {
        return Err(IoError::IconAsset(format!(
            "an icon exceeds the {} byte limit",
            bp_icons::MAX_SVG_BYTES
        )));
    }
    *total = total
        .checked_add(size)
        .ok_or_else(|| IoError::IconAsset("asset size overflow".into()))?;
    if *total > MAX_ASSET_BYTES {
        return Err(IoError::IconAsset(
            "embedded icons exceed the 128 MiB limit".into(),
        ));
    }
    Ok(())
}

fn read_entry(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    path: &str,
    limit: u64,
) -> Result<Vec<u8>, IoError> {
    let entry = archive.by_name(path)?;
    if entry.size() > limit {
        return Err(IoError::IconAsset(format!(
            "archive entry {path:?} is too large"
        )));
    }
    let mut bytes = Vec::new();
    entry.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(IoError::IconAsset(format!(
            "archive entry {path:?} is too large"
        )));
    }
    Ok(bytes)
}

fn hydrate_icons(
    value: &mut Value,
    archive: &mut ZipArchive<Cursor<&[u8]>>,
) -> Result<(), IoError> {
    let Some(icons) = value.get_mut("icons").and_then(Value::as_object_mut) else {
        return Ok(());
    };
    let mut total = 0;
    for (reference, metadata) in icons {
        let metadata = metadata
            .as_object_mut()
            .ok_or_else(|| IoError::IconAsset("icon metadata is not an object".into()))?;
        let Some(path) = metadata.remove("asset_path") else {
            continue;
        };
        let reference = ShapeRef::parse(reference)
            .ok_or_else(|| IoError::IconAsset("unsafe icon reference".into()))?;
        let expected = icon_asset_path(&reference)?;
        if path.as_str() != Some(expected.as_str()) || metadata.contains_key("svg") {
            return Err(IoError::IconAsset(
                "mismatched or ambiguous icon asset path".into(),
            ));
        }
        let bytes = read_entry(archive, &expected, MAX_ICON_BYTES)?;
        check_asset_size(bytes.len(), &mut total)?;
        let svg = String::from_utf8(bytes)
            .map_err(|_| IoError::IconAsset("icon SVG is not UTF-8".into()))?;
        metadata.insert("svg".into(), svg.into());
    }
    Ok(())
}

fn is_json_path(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("json"))
}

/// Writes bytes to an exclusively created temporary file next to `path`,
/// flushes it to disk, then renames it over `path`.
///
/// Concurrent writes each use their own file, and a failed write leaves the
/// previous destination intact. This also supports non-project output such
/// as an SVG export.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), IoError> {
    let (tmp, mut file) = create_temporary(path)?;
    let result = (|| {
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    Ok(result?)
}

fn create_temporary(path: &Path) -> Result<(PathBuf, File), std::io::Error> {
    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
    loop {
        let mut name = OsString::from(".");
        name.push(path.file_name().unwrap_or_else(|| "document".as_ref()));
        name.push(format!(
            ".{}.{}.tmp",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        let tmp = path.with_file_name(name);
        match OpenOptions::new().write(true).create_new(true).open(&tmp) {
            Ok(file) => return Ok((tmp, file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
}

/// Adds `.blueprint` unless the path already ends in `.blueprint` or `.json`.
pub fn with_default_extension(path: PathBuf) -> PathBuf {
    match path.extension().and_then(|e| e.to_str()) {
        Some(e) if e.eq_ignore_ascii_case(FILE_EXTENSION) || e.eq_ignore_ascii_case("json") => path,
        _ => {
            let mut s = path.into_os_string();
            s.push(".");
            s.push(FILE_EXTENSION);
            PathBuf::from(s)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bp_model::{Element, ElementKind, Parent, ShapeRef};
    use kurbo::Rect;

    fn sample() -> Document {
        let mut doc = Document::new();
        let page = doc.first_page().unwrap();
        let layer = Parent::Layer(doc.layers_of(page)[0].id);
        let mut el = Element::shape(
            ShapeRef::new("basic", "ellipse"),
            layer,
            doc.next_order_key(layer),
            Rect::new(10.0, 20.0, 110.0, 80.0),
        );
        if let ElementKind::Shape(s) = &mut el.kind {
            s.text = "Hello".into();
        }
        doc.elements.insert(el.id, el);
        doc
    }

    fn scratch_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("bp-io-test-{name}-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn zip_round_trip_on_disk() {
        let dir = scratch_dir("zip");
        let path = dir.join("plan.blueprint");
        let doc = sample();
        save(&doc, &path).unwrap();
        assert!(fs::read(&path).unwrap().starts_with(ZIP_MAGIC));
        assert_eq!(load(&path).unwrap(), doc);
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn json_round_trip_on_disk() {
        let dir = scratch_dir("json");
        let path = dir.join("plan.blueprint.json");
        let doc = sample();
        save(&doc, &path).unwrap();
        assert!(
            fs::read_to_string(&path)
                .unwrap()
                .contains(&format!("\"schema_version\": {SCHEMA_VERSION}"))
        );
        assert_eq!(load(&path).unwrap(), doc);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn rejects_newer_schema() {
        let mut value = serde_json::to_value(sample()).unwrap();
        value["schema_version"] = 999.into();
        let err = from_bytes(&serde_json::to_vec(&value).unwrap()).unwrap_err();
        assert!(matches!(err, IoError::TooNew { found: 999 }));
    }

    #[test]
    fn rejects_inconsistent_documents() {
        let mut doc = sample();
        doc.layers.clear();
        let err = from_bytes(&serde_json::to_vec(&doc).unwrap()).unwrap_err();
        assert!(matches!(err, IoError::Model(_)));
    }

    #[test]
    fn invalid_save_preserves_the_previous_project() {
        let dir = scratch_dir("invalid-save");
        let path = dir.join("plan.blueprint");
        let doc = sample();
        save(&doc, &path).unwrap();
        let before = fs::read(&path).unwrap();

        let mut invalid = doc.clone();
        invalid.layers.clear();
        assert!(matches!(save(&invalid, &path), Err(IoError::Model(_))));
        assert_eq!(fs::read(&path).unwrap(), before);
        assert_eq!(load(&path).unwrap(), doc);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn save_does_not_overwrite_an_existing_temporary_file() {
        let dir = scratch_dir("existing-temp");
        let path = dir.join("plan.blueprint");
        let existing = dir.join(".plan.blueprint.tmp");
        fs::write(&existing, b"unrelated data").unwrap();

        let doc = sample();
        save(&doc, &path).unwrap();
        assert_eq!(load(&path).unwrap(), doc);
        assert_eq!(fs::read(existing).unwrap(), b"unrelated data");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn concurrent_saves_each_write_a_complete_project() {
        use std::sync::{Arc, Barrier};

        let dir = scratch_dir("concurrent");
        let path = dir.join("plan.blueprint");
        let documents: Vec<_> = (0..8)
            .map(|i| {
                let mut doc = sample();
                doc.pages.values_mut().next().unwrap().name = format!("Writer {i}");
                doc
            })
            .collect();
        let barrier = Arc::new(Barrier::new(documents.len()));
        std::thread::scope(|scope| {
            let writers: Vec<_> = documents
                .iter()
                .map(|doc| {
                    let barrier = Arc::clone(&barrier);
                    let path = &path;
                    scope.spawn(move || {
                        barrier.wait();
                        for _ in 0..4 {
                            save(doc, path).unwrap();
                        }
                    })
                })
                .collect();
            for writer in writers {
                writer.join().unwrap();
            }
        });
        assert!(documents.contains(&load(&path).unwrap()));
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn default_extension() {
        let p = |s: &str| with_default_extension(PathBuf::from(s));
        assert_eq!(p("a"), PathBuf::from("a.blueprint"));
        assert_eq!(p("a.blueprint"), PathBuf::from("a.blueprint"));
        assert_eq!(p("a.JSON"), PathBuf::from("a.JSON"));
    }
}
