//! Cloud icons keep provider artwork separate from editable diagram styling.

use crate::{DisplayItem, Primitive, ShapeGeometry, Stroke, TextStyle, path_item, text};
use bp_geom::Dir;
use bp_model::kurbo::{Point, Rect, Shape as _};
use bp_model::{CloudIcon, Color, ElementId, PortId, Shape, StyleValues, VerticalAlign};
use bp_shapes::Port;
use bp_text::Face;

pub(crate) fn geometry(shape: &Shape) -> ShapeGeometry {
    let style = shape.style.resolve(&StyleValues {
        fill: None,
        stroke: None,
        stroke_width: 0.0,
        ..StyleValues::default()
    });
    let bounds = shape.bounds.abs();
    let center = bounds.center();
    let width = bounds.width().max(120.0);
    let text_height = bp_text::layout(
        &shape.text,
        Face::new(style.bold, style.italic),
        style.font_size,
        Some((width - 2.0 * text::PADDING_X).max(1.0)),
    )
    .height;
    let text_box = Rect::new(
        center.x - width / 2.0,
        bounds.y1 + 6.0,
        center.x + width / 2.0,
        bounds.y1 + 6.0 + text_height + 2.0 * text::PADDING_Y,
    );
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
        outline: bounds.to_path(0.1),
        back: Vec::new(),
        closed: true,
        ports,
        text_box,
        label: (!shape.text.is_empty()).then_some(text_box),
        erd: None,
        style,
    }
}

pub(crate) fn items(
    id: ElementId,
    icon: Option<&CloudIcon>,
    shape: &Shape,
    geometry: &ShapeGeometry,
) -> Vec<DisplayItem> {
    let mut items = Vec::new();
    if let Some(icon) = icon {
        items.push(DisplayItem {
            element: id,
            bbox: geometry.bounds,
            primitive: Primitive::Icon {
                svg: icon.svg.clone(),
                bounds: geometry.bounds,
                opacity: geometry.style.opacity,
            },
        });
    } else {
        // Keep the reference visible if an asset is missing from an older file.
        items.push(path_item(
            id,
            geometry.outline.clone(),
            Some(Color::rgba(248, 250, 252, 255)),
            Some(Stroke {
                color: Color::rgba(148, 163, 184, 255),
                width: 1.0,
                dash: Some([4.0, 3.0]),
            }),
        ));
    }
    let style = &geometry.style;
    let text_style = TextStyle {
        face: Face::new(style.bold, style.italic),
        size: style.font_size,
        color: style.text_color.faded(style.opacity),
        align: style.text_align,
        vertical_align: VerticalAlign::Top,
    };
    if let Some((run, bbox)) = text::place(&shape.text, geometry.text_box, &text_style, true) {
        items.push(DisplayItem {
            element: id,
            bbox,
            primitive: Primitive::Text(run),
        });
    }
    items
}
