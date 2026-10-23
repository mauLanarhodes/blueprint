//! Network boundaries use the same geometry and labels in the editor and exports.

use crate::{DisplayItem, Primitive, ShapeGeometry, Stroke, TextStyle, path_item, text};
use bp_geom::Dir;
use bp_model::kurbo::{BezPath, Point, Rect, RoundedRect, Shape as _};
use bp_model::{
    AwsResource, Color, ElementId, NetworkContainer, PortId, Shape, StyleValues, TextAlign,
    VerticalAlign,
};
use bp_shapes::Port;
use bp_text::Face;

#[derive(Clone, Debug, PartialEq)]
pub struct NetworkContainerGeometry {
    pub header: Rect,
    pub content: Rect,
}

fn dark(background: Color) -> bool {
    0.2126 * f64::from(background.r)
        + 0.7152 * f64::from(background.g)
        + 0.0722 * f64::from(background.b)
        < 128.0
}

fn defaults(container: NetworkContainer, background: Color) -> StyleValues {
    let (fill, stroke, text) = match (container, dark(background)) {
        (NetworkContainer::Vpc, false) => (
            Color::rgba(139, 92, 181, 15),
            Color::rgb(134, 95, 174),
            Color::rgb(84, 53, 115),
        ),
        (NetworkContainer::Subnet, false) => (
            Color::rgba(21, 150, 139, 15),
            Color::rgb(34, 139, 129),
            Color::rgb(22, 94, 89),
        ),
        (NetworkContainer::Vpc, true) => (
            Color::rgba(179, 139, 224, 20),
            Color::rgb(179, 145, 218),
            Color::rgb(226, 211, 245),
        ),
        (NetworkContainer::Subnet, true) => (
            Color::rgba(68, 192, 174, 18),
            Color::rgb(85, 189, 174),
            Color::rgb(183, 231, 222),
        ),
    };
    StyleValues {
        fill: Some(fill),
        stroke: Some(stroke),
        stroke_width: 1.5,
        corner_radius: 8.0,
        text_color: text,
        font_size: 14.0,
        bold: true,
        text_align: TextAlign::Left,
        vertical_align: VerticalAlign::Middle,
        ..StyleValues::default()
    }
}

pub(crate) fn geometry(
    shape: &Shape,
    container: NetworkContainer,
    background: Color,
) -> ShapeGeometry {
    // Container presentation belongs to its explicit kind and page palette.
    // Keep a converted resource's legacy shape overrides in the document for
    // undo, but do not let those hidden overrides obscure its boundary/header.
    let style = StyleValues {
        opacity: shape.style.opacity.unwrap_or(1.0).clamp(0.0, 1.0),
        ..defaults(container, background)
    };
    let bounds = shape.bounds.abs();
    let header = container.header_rect(bounds);
    let center = bounds.center();
    let ports = Dir::ALL
        .iter()
        .map(|&dir| Port {
            id: PortId::new(dir.name()),
            at: match dir {
                Dir::N => Point::new(center.x, bounds.y0),
                Dir::E => Point::new(bounds.x1, center.y),
                Dir::S => Point::new(center.x, bounds.y1),
                Dir::W => Point::new(bounds.x0, center.y),
            },
            dir,
        })
        .collect();
    ShapeGeometry {
        bounds,
        outline: RoundedRect::from_rect(bounds, style.corner_radius).to_path(0.1),
        back: Vec::new(),
        closed: true,
        ports,
        text_box: Rect::new(
            header.x0 + 42.0,
            header.y0 + 4.0,
            header.x1 - 10.0,
            header.y0 + 30.0,
        ),
        label: None,
        erd: None,
        network_container: Some(NetworkContainerGeometry {
            header,
            content: container.content_rect(bounds),
        }),
        style,
    }
}

fn metadata(shape: &Shape) -> String {
    match &shape.infrastructure {
        Some(AwsResource::Vpc { cidr }) => cidr.clone(),
        Some(AwsResource::Subnet {
            cidr,
            availability_zone,
            ..
        }) if !availability_zone.is_empty() => format!("{cidr}  ·  {availability_zone}"),
        Some(AwsResource::Subnet { cidr, .. }) => cidr.clone(),
        _ => "CIDR not configured".into(),
    }
}

/// Headers have a fixed height: ellipsize long values rather than allowing
/// text to flow over service content or beyond the boundary.
fn fit_label(value: &str, face: Face, size: f64, width: f64) -> String {
    if bp_text::measure(value, face, size) <= width {
        return value.into();
    }
    let mut label = value.to_owned();
    while !label.is_empty() {
        label.pop();
        if bp_text::measure(&format!("{label}…"), face, size) <= width {
            break;
        }
    }
    format!("{label}…")
}

fn label(id: ElementId, value: &str, area: Rect, style: &TextStyle, out: &mut Vec<DisplayItem>) {
    let value = fit_label(
        value,
        style.face,
        style.size,
        (area.width() - 12.0).max(1.0),
    );
    if let Some((run, bbox)) = text::place(&value, area, style, false) {
        out.push(DisplayItem {
            element: id,
            bbox,
            primitive: Primitive::Text(run),
        });
    }
}

pub(crate) fn items(
    id: ElementId,
    shape: &Shape,
    container: NetworkContainer,
    geometry: &ShapeGeometry,
) -> Vec<DisplayItem> {
    let mut items = Vec::new();
    let style = &geometry.style;
    let layout = geometry
        .network_container
        .as_ref()
        .expect("network geometry");
    let stroke = crate::stroke_of(style);
    items.push(path_item(
        id,
        geometry.outline.clone(),
        style.fill.map(|fill| fill.faded(style.opacity)),
        stroke,
    ));
    let header = layout.header;
    let radius = style.corner_radius.min(header.height() / 2.0);
    let mut header_path = BezPath::new();
    header_path.move_to((header.x0, header.y1));
    header_path.line_to((header.x0, header.y0 + radius));
    header_path.quad_to((header.x0, header.y0), (header.x0 + radius, header.y0));
    header_path.line_to((header.x1 - radius, header.y0));
    header_path.quad_to((header.x1, header.y0), (header.x1, header.y0 + radius));
    header_path.line_to((header.x1, header.y1));
    header_path.close_path();
    items.push(path_item(
        id,
        header_path,
        style.stroke.map(|color| color.faded(0.10 * style.opacity)),
        None,
    ));
    if let Some(stroke) = stroke {
        let mut divider = BezPath::new();
        divider.move_to((header.x0, header.y1));
        divider.line_to((header.x1, header.y1));
        items.push(path_item(id, divider, None, Some(stroke)));
    }

    // A boundary glyph is deliberately separate from provider artwork: its
    // dimensions stay fixed when the user resizes the container.
    let icon = Rect::new(
        header.x0 + 16.0,
        header.y0 + 17.0,
        header.x0 + 36.0,
        header.y0 + 37.0,
    );
    let icon_stroke = Some(Stroke {
        color: style.text_color.faded(style.opacity),
        width: 1.5,
        dash: (container == NetworkContainer::Vpc).then_some([3.0, 2.0]),
    });
    items.push(path_item(id, icon.to_path(0.1), None, icon_stroke));
    let inset = if container == NetworkContainer::Vpc {
        5.0
    } else {
        6.0
    };
    items.push(path_item(
        id,
        icon.inflate(-inset, -inset).to_path(0.1),
        None,
        icon_stroke.map(|s| Stroke { dash: None, ..s }),
    ));

    let name = if shape.text.trim().is_empty() {
        container.label()
    } else {
        &shape.text
    };
    let title_style = TextStyle {
        face: Face::Bold,
        size: style.font_size.min(16.0),
        color: style.text_color.faded(style.opacity),
        align: TextAlign::Left,
        vertical_align: VerticalAlign::Middle,
    };
    label(id, name, geometry.text_box, &title_style, &mut items);
    label(
        id,
        &metadata(shape),
        Rect::new(
            header.x0 + 42.0,
            header.y0 + 27.0,
            header.x1 - 10.0,
            header.y1 - 3.0,
        ),
        &TextStyle {
            face: Face::Regular,
            size: 11.0,
            ..title_style
        },
        &mut items,
    );
    items
}
