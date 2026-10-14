//! Derived smart-table layout, shared by canvas interaction and exporters.

use crate::{DisplayItem, Primitive, ShapeGeometry, TextStyle, path_item, stroke_of, text};
use bp_geom::Dir;
use bp_model::kurbo::{BezPath, Point, Rect, RoundedRect, Shape as _};
use bp_model::{
    ColumnId, ElementId, ErdColumn, ErdTable, PortId, Shape, StyleValues, TableDisplay, TextAlign,
    VerticalAlign,
};
use bp_shapes::Port;
use bp_text::Face;

/// A visible column row. Permanent column IDs stay independent of row order.
#[derive(Clone, Debug, PartialEq)]
pub struct ErdRowGeometry {
    pub column: ColumnId,
    pub bounds: Rect,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ErdGeometry {
    pub header: Rect,
    pub rows: Vec<ErdRowGeometry>,
}

/// Height of a table's header at the resolved font size.
pub fn erd_header_height(font_size: f64) -> f64 {
    (font_size * bp_text::LINE_HEIGHT + 14.0).max(34.0)
}

/// Height of each table row at the resolved font size.
pub fn erd_row_height(font_size: f64) -> f64 {
    (font_size * bp_text::LINE_HEIGHT + 12.0).max(28.0)
}

fn visible(table: &ErdTable, column: &ErdColumn) -> bool {
    match table.display {
        TableDisplay::All => true,
        TableDisplay::KeysOnly => column.primary_key || column.foreign_key || column.unique,
        TableDisplay::Collapsed => false,
    }
}

fn badges(column: &ErdColumn) -> String {
    let mut badges = Vec::new();
    if column.primary_key {
        badges.push("PK");
    }
    if column.foreign_key {
        badges.push("FK");
    }
    if column.unique {
        badges.push("UK");
    }
    if !column.nullable {
        badges.push("NOT NULL");
    }
    badges.join(" ")
}

fn data_type(column: &ErdColumn) -> String {
    let mut label = column.data_type.clone();
    if let Some(default) = &column.default_value {
        label.push_str(" = ");
        label.push_str(default);
    }
    label
}

/// Shared slots keep every row aligned, even when its key badges differ.
#[derive(Default)]
struct ColumnSlots {
    badge: f64,
    name: f64,
    data_type: f64,
}

fn column_slots(columns: &[&ErdColumn], style: &StyleValues) -> ColumnSlots {
    let mut slots = ColumnSlots::default();
    for column in columns {
        let badge = badges(column);
        if !badge.is_empty() {
            slots.badge = slots
                .badge
                .max(bp_text::measure(&badge, Face::Bold, style.font_size * 0.8) + 12.0);
        }
        slots.name = slots.name.max(
            bp_text::measure(
                &column.name,
                Face::new(column.primary_key || style.bold, style.italic),
                style.font_size,
            ) + 12.0,
        );
        slots.data_type = slots.data_type.max(
            bp_text::measure(
                &data_type(column),
                Face::new(style.bold, style.italic),
                style.font_size,
            ) + 12.0,
        );
    }
    slots
}

/// Enough room for the full typed rows, while preserving larger requested boxes.
pub(crate) fn geometry(shape: &Shape, table: &ErdTable, style: StyleValues) -> ShapeGeometry {
    let requested = shape.bounds.abs();
    let columns: Vec<_> = table
        .columns_sorted()
        .into_iter()
        .filter(|c| visible(table, c))
        .collect();
    let height = erd_header_height(style.font_size);
    let row_height = erd_row_height(style.font_size);
    let slots = column_slots(&columns, &style);
    let width = requested
        .width()
        .max(220.0)
        .max(bp_text::measure(&shape.text, Face::new(true, style.italic), style.font_size) + 24.0)
        .max(slots.badge + slots.name + slots.data_type + 32.0);
    let minimum_height = height + columns.len() as f64 * row_height;
    let actual_height = if table.display == TableDisplay::Collapsed {
        height
    } else {
        requested.height().max(minimum_height)
    };
    let bounds = Rect::new(
        requested.x0,
        requested.y0,
        requested.x0 + width,
        requested.y0 + actual_height,
    );
    let header = Rect::new(bounds.x0, bounds.y0, bounds.x1, bounds.y0 + height);
    let rows: Vec<_> = columns
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let top = header.y1 + i as f64 * row_height;
            ErdRowGeometry {
                column: c.id,
                bounds: Rect::new(bounds.x0, top, bounds.x1, top + row_height),
            }
        })
        .collect();
    let mut ports: Vec<_> = Dir::ALL
        .iter()
        .map(|&dir| Port {
            id: PortId::new(dir.name()),
            at: match dir {
                Dir::N => Point::new(bounds.center().x, bounds.y0),
                Dir::E => Point::new(bounds.x1, bounds.center().y),
                Dir::S => Point::new(bounds.center().x, bounds.y1),
                Dir::W => Point::new(bounds.x0, bounds.center().y),
            },
            dir,
        })
        .collect();
    for column in &table.columns {
        // Filtering never changes a connector's stored endpoint. Hidden
        // columns resolve to the header until the rows become visible again.
        let y = rows
            .iter()
            .find(|r| r.column == column.id)
            .map_or(header.center().y, |r| r.bounds.center().y);
        ports.extend([
            Port {
                id: PortId::column(column.id, true),
                at: Point::new(bounds.x0, y),
                dir: Dir::W,
            },
            Port {
                id: PortId::column(column.id, false),
                at: Point::new(bounds.x1, y),
                dir: Dir::E,
            },
        ]);
    }
    ShapeGeometry {
        label: None,
        bounds,
        outline: bounds.to_path(0.05),
        back: Vec::new(),
        closed: true,
        ports,
        text_box: header,
        erd: Some(ErdGeometry { header, rows }),
        style,
    }
}

fn label(id: ElementId, value: &str, area: Rect, style: &TextStyle, out: &mut Vec<DisplayItem>) {
    if let Some((run, bbox)) = text::place(value, area, style, false) {
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
    table: &ErdTable,
    g: &ShapeGeometry,
    layout: &ErdGeometry,
    out: &mut Vec<DisplayItem>,
) {
    let s = &g.style;
    let text_style = TextStyle {
        face: Face::new(s.bold, s.italic),
        size: s.font_size,
        color: s.text_color.faded(s.opacity),
        align: TextAlign::Left,
        vertical_align: VerticalAlign::Middle,
    };
    label(
        id,
        &shape.text,
        layout.header,
        &TextStyle {
            face: Face::new(true, s.italic),
            ..text_style
        },
        out,
    );
    if let Some(stroke) = stroke_of(s) {
        let line = |y: f64| {
            let mut path = BezPath::new();
            path.move_to((g.bounds.x0, y));
            path.line_to((g.bounds.x1, y));
            path
        };
        if table.display != TableDisplay::Collapsed {
            out.push(path_item(id, line(layout.header.y1), None, Some(stroke)));
        }
        for pair in layout.rows.windows(2) {
            let primary = |row: &ErdRowGeometry| {
                table
                    .columns
                    .iter()
                    .find(|c| c.id == row.column)
                    .is_some_and(|c| c.primary_key)
            };
            if primary(&pair[0]) && !primary(&pair[1]) {
                out.push(path_item(id, line(pair[0].bounds.y1), None, Some(stroke)));
            }
        }
    }
    let columns: Vec<_> = table
        .columns_sorted()
        .into_iter()
        .filter(|column| visible(table, column))
        .collect();
    let slots = column_slots(&columns, s);
    for row in &layout.rows {
        let Some(column) = table.columns.iter().find(|c| c.id == row.column) else {
            continue;
        };
        let badge = badges(column);
        let badge_style = TextStyle {
            face: Face::Bold,
            size: s.font_size * 0.8,
            ..text_style
        };
        let badge_width = bp_text::measure(&badge, badge_style.face, badge_style.size) + 12.0;
        let badge_end = row.bounds.x0 + 8.0 + slots.badge;
        if !badge.is_empty() {
            let pill = Rect::from_center_size(
                Point::new(
                    row.bounds.x0 + 8.0 + badge_width / 2.0,
                    row.bounds.center().y,
                ),
                (
                    badge_width,
                    (s.font_size * 1.3 + 4.0).min(row.bounds.height() - 6.0),
                ),
            );
            out.push(path_item(
                id,
                RoundedRect::from_rect(pill, 3.0).to_path(0.1),
                Some(s.text_color.faded(s.opacity * 0.08)),
                None,
            ));
        }
        label(
            id,
            &badge,
            Rect::new(row.bounds.x0 + 8.0, row.bounds.y0, badge_end, row.bounds.y1),
            &badge_style,
            out,
        );
        let type_label = data_type(column);
        let type_start = row.bounds.x1 - slots.data_type - 8.0;
        label(
            id,
            &column.name,
            Rect::new(
                badge_end + 8.0,
                row.bounds.y0,
                type_start - 8.0,
                row.bounds.y1,
            ),
            &TextStyle {
                face: Face::new(column.primary_key || s.bold, s.italic),
                ..text_style
            },
            out,
        );
        label(
            id,
            &type_label,
            Rect::new(
                type_start,
                row.bounds.y0,
                row.bounds.x1 - 8.0,
                row.bounds.y1,
            ),
            &text_style,
            out,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bp_model::{Element, LayerId, OrderKey, Parent, ShapeRef};

    #[test]
    fn table_slots_align_names_and_types_with_different_badges_and_long_values() {
        let mut element = Element::shape(
            ShapeRef::new("erd", "table"),
            Parent::Layer(LayerId::new()),
            OrderKey::first(),
            Rect::new(10.0, 20.0, 230.0, 100.0),
        );
        let id = element.id;
        let shape = element.as_shape_mut().unwrap();
        let table = shape.erd.as_mut().unwrap();
        table.columns[0].name = "identifier_with_a_long_name".into();
        table.columns[0].data_type = "UUID".into();
        table.columns.push(ErdColumn::new(
            "note",
            "VARCHAR(200) COLLATE customer_locale",
            OrderKey::after(&table.columns[0].order),
        ));
        let style = StyleValues::default();
        let geometry = geometry(shape, shape.erd.as_ref().unwrap(), style);
        let mut display = Vec::new();
        items(
            id,
            shape,
            shape.erd.as_ref().unwrap(),
            &geometry,
            geometry.erd.as_ref().unwrap(),
            &mut display,
        );
        let texts: Vec<_> = display
            .iter()
            .filter_map(|item| {
                if let Primitive::Text(run) = &item.primitive {
                    Some((&run.lines[0], item.bbox))
                } else {
                    None
                }
            })
            .collect();
        let line = |text: &str| texts.iter().find(|(line, _)| line.text == text).unwrap().0;
        assert_eq!(line("identifier_with_a_long_name").x, line("note").x);
        assert_eq!(
            line("UUID").x,
            line("VARCHAR(200) COLLATE customer_locale").x
        );
        for (_, bounds) in texts {
            assert!(
                bounds.x0 >= geometry.bounds.x0 && bounds.x1 <= geometry.bounds.x1,
                "full text must fit the measured table width"
            );
        }
        assert_eq!(
            geometry.erd.as_ref().unwrap().rows[0].column,
            shape.erd.as_ref().unwrap().columns[0].id
        );
        assert_eq!(
            geometry.erd.as_ref().unwrap().rows[1].column,
            shape.erd.as_ref().unwrap().columns[1].id
        );
    }
}
