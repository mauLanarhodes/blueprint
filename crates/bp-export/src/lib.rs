//! Exports a display list as SVG. Every later format (PNG, JPEG, WebP via
//! resvg; PDF via krilla) is produced from this one SVG writer.

use bp_model::{Color, Document, PageId, TextAlign};
use bp_scene::{DisplayList, Primitive, Stroke, TextRun};
use bp_text::Face;
use kurbo::{BezPath, PathEl, Point, Rect};
use std::collections::BTreeSet;
use std::fmt::Write;

/// Font stack for exported text: the bundled font first, then lookalikes
/// for viewers that have neither it nor the embedded copy.
pub const FONT_FAMILY: &str = "Inter, 'Segoe UI', 'Helvetica Neue', Arial, sans-serif";

#[derive(Clone, Debug, PartialEq)]
pub struct SvgOptions {
    /// Margin around the diagram, in page units.
    pub padding: f64,
    /// Overrides the page background; `Some(None)` exports transparent.
    pub background: Option<Option<Color>>,
    /// Embed the font faces the text uses, so the file looks the same on
    /// machines without Inter (adds roughly 550 KB per face).
    pub embed_fonts: bool,
}

impl Default for SvgOptions {
    fn default() -> Self {
        Self {
            padding: 20.0,
            background: None,
            embed_fonts: false,
        }
    }
}

/// Exports one page of `doc`.
pub fn page_to_svg(doc: &Document, page: PageId, options: &SvgOptions) -> String {
    to_svg(&bp_scene::build_page(doc, page).list, options)
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
    if options.embed_fonts {
        write_font_faces(&mut svg, list);
    }
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

    for item in list.items() {
        match &item.primitive {
            Primitive::Path { path, fill, stroke } => {
                let _ = writeln!(
                    svg,
                    r#"  <path d="{}"{}{}/>"#,
                    path_data(path),
                    paint("fill", *fill),
                    stroke_attrs(stroke.as_ref()),
                );
            }
            Primitive::Text(run) => write_text(&mut svg, run),
            Primitive::Icon {
                svg: source,
                bounds,
                opacity,
            } => {
                let _ = writeln!(
                    svg,
                    r#"  <image x="{}" y="{}" width="{}" height="{}" opacity="{}" preserveAspectRatio="xMidYMid meet" href="data:image/svg+xml;base64,{}"/>"#,
                    num(bounds.x0),
                    num(bounds.y0),
                    num(bounds.width()),
                    num(bounds.height()),
                    num(opacity.clamp(0.0, 1.0)),
                    base64(source.as_bytes()),
                );
            }
        }
    }
    svg.push_str("</svg>\n");
    svg
}

fn stroke_attrs(stroke: Option<&Stroke>) -> String {
    let Some(s) = stroke else {
        return String::new();
    };
    let mut out = format!(
        r#"{} stroke-width="{}" stroke-linejoin="round""#,
        paint("stroke", Some(s.color)),
        num(s.width)
    );
    if let Some([on, off]) = s.dash {
        let _ = write!(out, r#" stroke-dasharray="{} {}""#, num(on), num(off));
    }
    out
}

fn write_text(svg: &mut String, run: &TextRun) {
    let anchor = match run.align {
        TextAlign::Left => "start",
        TextAlign::Center => "middle",
        TextAlign::Right => "end",
    };
    let mut font = format!(
        r#"font-family="{FONT_FAMILY}" font-size="{}""#,
        num(run.size)
    );
    if run.face.is_bold() {
        font.push_str(r#" font-weight="bold""#);
    }
    if run.face.is_italic() {
        font.push_str(r#" font-style="italic""#);
    }
    for line in &run.lines {
        if line.text.trim().is_empty() {
            continue;
        }
        let _ = writeln!(
            svg,
            r#"  <text x="{}" y="{}" {font}{} text-anchor="{anchor}" xml:space="preserve">{}</text>"#,
            num(line.x),
            num(line.baseline),
            paint("fill", Some(run.color)),
            escape(&line.text),
        );
    }
}

/// `@font-face` rules carrying every face the text uses.
fn write_font_faces(svg: &mut String, list: &DisplayList) {
    let faces: BTreeSet<usize> = list
        .items()
        .filter_map(|i| match &i.primitive {
            Primitive::Text(run) => Some(Face::ALL.iter().position(|f| *f == run.face)?),
            _ => None,
        })
        .collect();
    if faces.is_empty() {
        return;
    }
    svg.push_str("  <defs><style>\n");
    for index in faces {
        let face = Face::ALL[index];
        let _ = writeln!(
            svg,
            "    @font-face {{ font-family: Inter; font-weight: {}; font-style: {}; src: url(data:font/ttf;base64,{}) format('truetype'); }}",
            if face.is_bold() { "bold" } else { "normal" },
            if face.is_italic() { "italic" } else { "normal" },
            base64(face.data()),
        );
    }
    svg.push_str("  </style></defs>\n");
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

fn base64(data: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for (i, shift) in [18, 12, 6, 0].into_iter().enumerate() {
            if i <= chunk.len() {
                out.push(ALPHABET[((n >> shift) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use bp_model::{Element, ElementKind, Endpoint, Paint, Parent, ShapeRef};

    fn doc_with(shape: &str, bounds: Rect, text: &str) -> (Document, PageId, Parent) {
        let mut doc = Document::new();
        let page = doc.first_page().unwrap();
        let layer = Parent::Layer(doc.layers_of(page)[0].id);
        let (library, name) = shape.split_once('/').unwrap();
        let mut el = Element::shape(
            ShapeRef::new(library, name),
            layer,
            doc.next_order_key(layer),
            bounds,
        );
        if let ElementKind::Shape(s) = &mut el.kind {
            s.text = text.into();
        }
        doc.elements.insert(el.id, el);
        (doc, page, layer)
    }

    #[test]
    fn exports_shapes_and_escaped_text() {
        let (doc, page, _) = doc_with(
            "basic/rectangle",
            Rect::new(0.0, 0.0, 120.0, 60.0),
            "A & <B>",
        );
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
        assert!(svg.contains(r#"text-anchor="middle""#));
        assert!(svg.trim_end().ends_with("</svg>"));
        assert!(!svg.contains("@font-face"));
    }

    #[test]
    fn transparent_background_and_translucent_fill() {
        let (mut doc, page, _) = doc_with("basic/ellipse", Rect::new(0.0, 0.0, 10.0, 10.0), "");
        for el in doc.elements.values_mut() {
            if let ElementKind::Shape(s) = &mut el.kind {
                s.style.fill = Some(Paint::Color(Color::rgba(255, 0, 0, 128)));
            }
        }
        let options = SvgOptions {
            background: Some(None),
            ..SvgOptions::default()
        };
        let svg = page_to_svg(&doc, page, &options);
        assert!(!svg.contains("<rect"));
        assert!(svg.contains(r##"fill="#ff0000" fill-opacity="0.502""##));
    }

    #[test]
    fn dashes_bold_text_and_connectors() {
        let (mut doc, page, layer) = doc_with(
            "basic/sticky-note",
            Rect::new(0.0, 0.0, 160.0, 120.0),
            "Bold note",
        );
        let id = *doc.elements.keys().next().unwrap();
        if let ElementKind::Shape(s) = &mut doc.elements.get_mut(&id).unwrap().kind {
            s.style.bold = Some(true);
            s.style.dash = Some(bp_model::Dash::Dashed);
        }
        let c = Element::connector(
            Endpoint::glued(id, Some("e")),
            Endpoint::Free(Point::new(300.0, 60.0)),
            layer,
            doc.next_order_key(layer),
        );
        doc.elements.insert(c.id, c);
        let svg = page_to_svg(&doc, page, &SvgOptions::default());
        assert!(svg.contains(r#"stroke-dasharray="4 3""#), "{svg}");
        assert!(svg.contains(r#"font-weight="bold""#));
        assert!(
            svg.contains(r#"text-anchor="start""#),
            "sticky notes align left"
        );
        assert!(
            svg.contains(r#"<path d="M160,60 L"#),
            "connector leaves the east port"
        );
    }

    #[test]
    fn fonts_embed_on_request() {
        let (doc, page, _) = doc_with("basic/text", Rect::new(0.0, 0.0, 100.0, 30.0), "Hi");
        let options = SvgOptions {
            embed_fonts: true,
            ..SvgOptions::default()
        };
        let svg = page_to_svg(&doc, page, &options);
        assert_eq!(svg.matches("@font-face").count(), 1, "only the face in use");
        assert!(
            svg.contains("data:font/ttf;base64,AAEAAA"),
            "a TrueType header"
        );
    }

    #[test]
    fn number_formatting_and_base64() {
        assert_eq!(num(1.5), "1.5");
        assert_eq!(num(2.0), "2");
        assert_eq!(num(-0.0001), "0");
        assert_eq!(num(1.23456), "1.235");
        assert_eq!(base64(b"Man"), "TWFu");
        assert_eq!(base64(b"Ma"), "TWE=");
        assert_eq!(base64(b"M"), "TQ==");
        assert_eq!(base64(b""), "");
    }

    #[test]
    fn icons_embed_exact_original_svg_bytes_with_contain_fit() {
        // Authored artwork; it intentionally includes whitespace and two colours.
        let artwork = "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 20 10\">\n <path fill=\"#ff8000\" d=\"M0 0h10v10H0z\"/>\n <path fill=\"#0080ff\" d=\"M10 0h10v10H10z\"/>\n</svg>";
        let list = DisplayList {
            groups: vec![
                vec![bp_scene::DisplayItem {
                    element: bp_model::ElementId::new(),
                    bbox: Rect::new(10.0, 20.0, 106.0, 116.0),
                    primitive: Primitive::Icon {
                        svg: artwork.into(),
                        bounds: Rect::new(10.0, 20.0, 106.0, 116.0),
                        opacity: 0.5,
                    },
                }]
                .into(),
            ],
            background: None,
        };
        let svg = to_svg(&list, &SvgOptions::default());
        assert!(svg.contains(r#"<image x="10" y="20" width="96" height="96" opacity="0.5" preserveAspectRatio="xMidYMid meet""#), "{svg}");
        assert!(svg.contains(&format!(
            "href=\"data:image/svg+xml;base64,{}\"",
            base64(artwork.as_bytes())
        )));
        assert!(
            !svg.contains("<path"),
            "provider paths remain isolated within the original SVG"
        );
    }
}
