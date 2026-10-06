//! Upgrades older project files to the current schema, one version at a
//! time, working on the raw JSON so old field names never need Rust types.

use crate::IoError;
use bp_model::SCHEMA_VERSION;
use serde_json::{Map, Value, json};

pub fn migrate(mut value: Value) -> Result<Value, IoError> {
    let version = value
        .get("schema_version")
        .and_then(Value::as_u64)
        .ok_or(IoError::NoVersion)?;
    if version > u64::from(SCHEMA_VERSION) {
        return Err(IoError::TooNew { found: version });
    }
    if version < 2 {
        value = v1_to_v2(value)?;
    }
    if version < 3 {
        value = v2_to_v3(value)?;
    }
    if version < 4 {
        // Cloud assets and Cloud page kinds need a new format identifier:
        // earlier readers must refuse to overwrite artwork they cannot keep.
        // Both fields have defaults, so existing diagrams retain their data.
        value["schema_version"] = 4.into();
    }
    if version < 5 {
        // Named/composite SQL constraints and indexes use defaulted fields.
        // Earlier readers must refuse to resave and discard that metadata.
        value["schema_version"] = 5.into();
    }
    Ok(value)
}

/// Schema 2 → 3: structured table data and stable column relationships.
/// Ordinary shapes retain their existing data and sparse serialization.
fn v2_to_v3(mut value: Value) -> Result<Value, IoError> {
    if let Some(elements) = value.get_mut("elements").and_then(Value::as_object_mut) {
        for element in elements.values_mut() {
            let element = element.as_object_mut().ok_or_else(|| {
                IoError::Migration("schema 2 file: element is not an object".into())
            })?;
            if element.get("shape").and_then(Value::as_str) == Some("erd/table")
                && element.get("erd").is_none_or(Value::is_null)
            {
                element.insert(
                    "erd".into(),
                    serde_json::to_value(bp_model::ErdTable::default())?,
                );
            }
        }
    }
    value["schema_version"] = 3.into();
    Ok(value)
}

/// Schema 1 (Phase 0) → 2 (Phase 1):
/// - `layer` becomes `parent: {"layer": …}`, since elements can now nest;
/// - `kind` becomes a shape-library reference (`basic/…`);
/// - styles become sparse overrides: values equal to the old defaults are
///   dropped, so the shape's own defaults apply.
fn v1_to_v2(mut value: Value) -> Result<Value, IoError> {
    let bad = |what: &str| IoError::Migration(format!("schema 1 file: {what}"));
    if let Some(elements) = value.get_mut("elements").and_then(Value::as_object_mut) {
        for element in elements.values_mut() {
            let element = element
                .as_object_mut()
                .ok_or_else(|| bad("element is not an object"))?;
            let layer = element
                .remove("layer")
                .ok_or_else(|| bad("element has no layer"))?;
            element.insert("parent".into(), json!({ "layer": layer }));

            let kind = element
                .remove("kind")
                .and_then(|k| k.as_str().map(str::to_owned))
                .ok_or_else(|| bad("element has no kind"))?;
            let shape = match kind.as_str() {
                "rectangle" => "basic/rectangle",
                "rounded_rectangle" => "basic/rounded-rectangle",
                "ellipse" => "basic/ellipse",
                "diamond" => "basic/diamond",
                "text" => "basic/text",
                other => return Err(bad(&format!("unknown shape kind {other:?}"))),
            };
            element.insert("type".into(), "shape".into());
            element.insert("shape".into(), shape.into());

            if let Some(Value::Object(style)) = element.remove("style") {
                let sparse = v1_style_overrides(style, kind == "text");
                if !sparse.is_empty() {
                    element.insert("style".into(), Value::Object(sparse));
                }
            }
        }
    }
    value["schema_version"] = 2.into();
    Ok(value)
}

/// Keeps only the schema-1 style values that differ from that version's
/// defaults, which match the Phase 1 defaults of the same `basic/` shapes.
fn v1_style_overrides(style: Map<String, Value>, is_text: bool) -> Map<String, Value> {
    let defaults = if is_text {
        json!({ "fill": null, "stroke": null, "stroke_width": 1.5, "text_color": "#111827", "font_size": 14.0 })
    } else {
        json!({ "fill": "#ffffff", "stroke": "#1f2937", "stroke_width": 1.5, "text_color": "#111827", "font_size": 14.0 })
    };
    let mut out = Map::new();
    for (key, value) in style {
        let default = &defaults[key.as_str()];
        let same = match (value.as_f64(), default.as_f64()) {
            (Some(a), Some(b)) => a == b,
            _ => value == *default,
        };
        if same {
            continue;
        }
        // Schema 1 wrote "no fill" as null; schema 2 writes "none".
        let value = match (key.as_str(), value) {
            ("fill" | "stroke", Value::Null) => "none".into(),
            (_, value) => value,
        };
        out.insert(key, value);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sparse_styles_keep_only_changes() {
        let style = json!({
            "fill": "#dbeafe", "stroke": "#1f2937", "stroke_width": 1.5,
            "text_color": "#111827", "font_size": 14.0
        });
        let out = v1_style_overrides(style.as_object().unwrap().clone(), false);
        assert_eq!(Value::Object(out), json!({ "fill": "#dbeafe" }));

        let text = json!({
            "fill": null, "stroke": "#ff0000", "stroke_width": 1.5,
            "text_color": "#111827", "font_size": 20
        });
        let out = v1_style_overrides(text.as_object().unwrap().clone(), true);
        assert_eq!(
            Value::Object(out),
            json!({ "stroke": "#ff0000", "font_size": 20 })
        );

        let unfilled = json!({ "fill": null });
        let out = v1_style_overrides(unfilled.as_object().unwrap().clone(), false);
        assert_eq!(Value::Object(out), json!({ "fill": "none" }));
    }

    #[test]
    fn unknown_kinds_are_reported() {
        let v1 = json!({
            "schema_version": 1, "pages": {}, "layers": {},
            "elements": { "x": { "id": "x", "layer": "l", "order": "V", "kind": "hexagon" } }
        });
        assert!(matches!(migrate(v1), Err(IoError::Migration(_))));
    }
}
