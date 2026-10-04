use crate::MAX_SVG_BYTES;

/// SVG parsing options that never read external image files or system fonts.
/// Embedded vector definitions such as `url(#gradient)` remain supported.
pub fn offline_svg_options() -> usvg::Options<'static> {
    usvg::Options {
        image_href_resolver: usvg::ImageHrefResolver {
            resolve_data: Box::new(|_, _, _| None),
            resolve_string: Box::new(|_, _| None),
        },
        ..Default::default()
    }
}

/// Checks artwork before it is embedded in a project or rendered. The original
/// SVG remains untouched: validation does not serialize a normalized SVG.
pub fn validate_svg(svg: &str) -> Result<(), String> {
    if svg.len() > MAX_SVG_BYTES {
        return Err(format!("SVG exceeds the {} byte limit", MAX_SVG_BYTES));
    }
    let xml = roxmltree::Document::parse(svg).map_err(|e| e.to_string())?;
    let root = xml.root_element();
    if root.tag_name().name() != "svg"
        || root.tag_name().namespace() != Some("http://www.w3.org/2000/svg")
    {
        return Err("root must be an SVG element in the SVG namespace".into());
    }
    for node in xml.descendants() {
        if let Some(pi) = node.pi()
            && pi.target != "xml"
        {
            return Err("external stylesheet processing instructions are not supported".into());
        }
        if !node.is_element() {
            continue;
        }
        let tag = node.tag_name().name().to_ascii_lowercase();
        if matches!(tag.as_str(), "text" | "tspan" | "textpath") {
            return Err(
                "SVG text rendering is not supported; choose original artwork with vector outlines"
                    .into(),
            );
        }
        if matches!(
            tag.as_str(),
            "script"
                | "foreignobject"
                | "iframe"
                | "object"
                | "embed"
                | "image"
                | "animate"
                | "animatemotion"
                | "animatetransform"
                | "set"
        ) {
            return Err(format!("{tag} is not supported in static vector icons"));
        }
        for attr in node.attributes() {
            let name = attr.name().to_ascii_lowercase();
            if name.starts_with("on") {
                return Err(format!("event handler {name} is not allowed"));
            }
            if matches!(name.as_str(), "href" | "src" | "base")
                && !attr.value().trim().starts_with('#')
            {
                return Err("external file and network references are not allowed".into());
            }
            if name == "style" && attr.value().contains('\\') {
                return Err("escaped CSS is not supported in static icons".into());
            }
            validate_css_references(attr.value())?;
        }
        if tag == "style" {
            for child in node.children().filter(|n| n.is_text()) {
                if child.text().unwrap_or_default().contains('\\') {
                    return Err("escaped CSS is not supported in static icons".into());
                }
                validate_css_references(child.text().unwrap_or_default())?;
            }
        }
    }
    let tree = usvg::Tree::from_str(svg, &offline_svg_options()).map_err(|e| e.to_string())?;
    if tree.root().children().is_empty() {
        return Err("SVG contains no renderable vector artwork".into());
    }
    Ok(())
}

fn validate_css_references(value: &str) -> Result<(), String> {
    let lower = value.to_ascii_lowercase();
    if lower.contains("@import") || lower.contains("@font-face") {
        return Err("external stylesheets and fonts are not allowed".into());
    }
    let mut tail = lower.as_str();
    while let Some(start) = tail.find("url(") {
        tail = &tail[start + 4..];
        let end = tail.find(')').ok_or("unterminated SVG URL reference")?;
        let target = tail[..end].trim().trim_matches(['\'', '"']).trim();
        if !target.starts_with('#') || target.contains('\\') {
            return Err("only local fragment URL references are allowed".into());
        }
        tail = &tail[end + 1..];
    }
    Ok(())
}
