//! Exports a display list as SVG. Every later format (PNG, JPEG, WebP via
//! resvg; PDF via krilla) is produced from this one SVG writer.

use bp_model::{Color, Document, PageId};
use bp_scene::{DisplayList, Primitive, line_centers};
use kurbo::{BezPath, PathEl, Point, Rect};
use std::fmt::Write;

/// Font stack used for exported text until fonts are bundled (bp-text).
pub const FONT_FAMILY: &str = "Inter, 'Segoe UI', 'Helvetica Neue', Arial, sans-serif";

#[derive(Clone, Debug, PartialEq)]
pub struct SvgOptions {
    /// Margin around the diagram, in page units.
    pub padding: f64,
    /// Overrides the page background; `Some(None)` exports transparent.
    pub background: Option<Option<Color>>,
}

impl Default for SvgOptions {
    fn default() -> Self {
        Self {
            padding: 20.0,
            background: None,
        }
    }
}

/// Exports one page of `doc`.
pub fn page_to_svg(doc: &Document, page: PageId, options: &SvgOptions) -> String {
    to_svg(&bp_scene::build_page(doc, page), options)
}

/// Writes `list` as a standalone SVG document cropped to its content.
pub fn to_svg(list: &DisplayList, options: &SvgOptions) -> String {
    let content = list
        .bounds()
        .unwrap_or_else(|| Rect::new(0.0, 0.0, 100.0, 100.0));
    let view = content.inflate(options.padding, options.padding);
    let background = options.background.unwrap_or(list.background);

    let mut svg = String::new();
    let _ = writeln!(svg, r#"<?xml version="1.0" encoding="UTF-8"?>"#);
    let _ = writeln!(
        svg,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="{x} {y} {w} {h}">"#,
        x = num(view.x0),
        y = num(view.y0),
        w = num(view.width()),
        h = num(view.height()),
    );
    if let Some(bg) = background {
        let _ = writeln!(
            svg,
            r#"  <rect x="{}" y="{}" width="{}" height="{}"{}/>"#,
            num(view.x0),
            num(view.y0),
            num(view.width()),
            num(view.height()),
            paint("fill", Some(bg)),
        );
    }

    for item in &list.items {
        match &item.primitive {
            Primitive::Path { path, fill, stroke } => {
                let stroke_attrs = match stroke {
                    Some(s) => format!(
                        r#"{} stroke-width="{}" stroke-linejoin="round""#,
                        paint("stroke", Some(s.color)),
                        num(s.width)
                    ),
                    None => String::new(),
                };
                let _ = writeln!(
                    svg,
                    r#"  <path d="{}"{}{}/>"#,
                    path_data(path),
                    paint("fill", *fill),
                    stroke_attrs
                );
            }
            Primitive::Text {
                center,
                lines,
                font_size,
                color,
            } => {
                for (line, at) in lines
                    .iter()
                    .zip(line_centers(*center, lines.len(), *font_size))
                {
                    if line.is_empty() {
                        continue;
                    }
                    let _ = writeln!(
                        svg,
                        r#"  <text x="{}" y="{}" font-family="{FONT_FAMILY}" font-size="{}"{} text-anchor="middle" dominant-baseline="central">{}</text>"#,
                        num(at.x),
                        num(at.y),
                        num(*font_size),
                        paint("fill", Some(*color)),
                        escape(line),
                    );
                }
            }
        }
    }
    svg.push_str("</svg>\n");
    svg
}

/// ` fill="#rrggbb"` plus an opacity attribute when needed; `none` if unset.
fn paint(attr: &str, color: Option<Color>) -> String {
    match color {
        None => format!(r#" {attr}="none""#),
        Some(c) if c.is_opaque() => format!(r#" {attr}="{}""#, c.to_hex_rgb()),
        Some(c) => format!(
            r#" {attr}="{}" {attr}-opacity="{}""#,
            c.to_hex_rgb(),
            num(c.opacity())
        ),
    }
}

/// SVG path data with compact numbers (kurbo's `to_svg` prints full
/// precision, e.g. `0.0000000000000017763568394002505`).
fn path_data(path: &BezPath) -> String {
    let pt = |p: Point| format!("{},{}", num(p.x), num(p.y));
    let mut d = Vec::new();
    for el in path.elements() {
        d.push(match *el {
            PathEl::MoveTo(p) => format!("M{}", pt(p)),
            PathEl::LineTo(p) => format!("L{}", pt(p)),
            PathEl::QuadTo(a, p) => format!("Q{} {}", pt(a), pt(p)),
            PathEl::CurveTo(a, b, p) => format!("C{} {} {}", pt(a), pt(b), pt(p)),
            PathEl::ClosePath => "Z".to_owned(),
        });
    }
    d.join(" ")
}

/// Up to three decimals, without trailing zeros.
fn num(v: f64) -> String {
    let s = format!("{v:.3}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.into() }
}

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use bp_model::{Element, ShapeKind};

    #[test]
    fn exports_shapes_and_escaped_text() {
        let mut doc = Document::new();
        let page = doc.first_page().unwrap();
        let layer = doc.layers_of(page)[0].id;
        let mut el = Element::new(
            ShapeKind::Rectangle,
            layer,
            doc.next_order_key(layer),
            Rect::new(0.0, 0.0, 120.0, 60.0),
        );
        el.text = "A & <B>".into();
        doc.elements.insert(el.id, el);

        let svg = page_to_svg(&doc, page, &SvgOptions::default());
        assert!(svg.starts_with("<?xml"));
        assert!(
            svg.contains(r#"viewBox="-20.75 -20.75 161.5 101.5""#),
            "{svg}"
        );
        assert!(svg.contains(
            r##"<rect x="-20.75" y="-20.75" width="161.5" height="101.5" fill="#ffffff"/>"##
        ));
        assert!(svg.contains(r##"fill="#ffffff" stroke="#1f2937" stroke-width="1.5""##));
        assert!(
            svg.contains(r#"<path d="M0,0 L120,0 L120,60 L0,60 Z""#),
            "{svg}"
        );
        assert!(svg.contains(">A &amp; &lt;B&gt;</text>"));
        assert!(svg.trim_end().ends_with("</svg>"));
    }

    #[test]
    fn transparent_background_and_translucent_fill() {
        let mut doc = Document::new();
        let page = doc.first_page().unwrap();
        let layer = doc.layers_of(page)[0].id;
        let mut el = Element::new(
            ShapeKind::Ellipse,
            layer,
            doc.next_order_key(layer),
            Rect::new(0.0, 0.0, 10.0, 10.0),
        );
        el.style.fill = Some(Color::rgba(255, 0, 0, 128));
        doc.elements.insert(el.id, el);
        let options = SvgOptions {
            background: Some(None),
            ..SvgOptions::default()
        };
        let svg = page_to_svg(&doc, page, &options);
        assert!(!svg.contains("<rect"));
        assert!(svg.contains(r##"fill="#ff0000" fill-opacity="0.502""##));
    }

    #[test]
    fn number_formatting() {
        assert_eq!(num(1.5), "1.5");
        assert_eq!(num(2.0), "2");
        assert_eq!(num(-0.0001), "0");
        assert_eq!(num(1.23456), "1.235");
    }
}