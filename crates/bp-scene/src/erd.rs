//! Derived smart-table layout, shared by canvas interaction and exporters.

use crate::{DisplayItem, Primitive, ShapeGeometry, TextStyle, path_item, stroke_of, text};
use bp_geom::Dir;
use bp_model::kurbo::{BezPath, Point, Rect, Shape as _};
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
    let width = columns.iter().fold(
        requested.width().max(220.0).max(
            bp_text::measure(&shape.text, Face::new(true, style.italic), style.font_size) + 24.0,
        ),
        |width, c| {
            let badge_width = bp_text::measure(&badges(c), Face::Bold, style.font_size * 0.8);
            let name_width = bp_text::measure(
                &c.name,
                Face::new(c.primary_key || style.bold, style.italic),
                style.font_size,
            );
            let type_width = bp_text::measure(
                &data_type(c),
                Face::new(style.bold, style.italic),
                style.font_size,
            );
            width.max(badge_width + name_width + type_width + 48.0)
        },
    );
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
        let badge_width = bp_text::measure(&badge, badge_style.face, badge_style.size);
        let badge_end = row.bounds.x0 + badge_width + 12.0;
        label(
            id,
            &badge,
            Rect::new(row.bounds.x0, row.bounds.y0, badge_end, row.bounds.y1),
            &badge_style,
            out,
        );
        let type_label = data_type(column);
        let type_width = bp_text::measure(&type_label, text_style.face, text_style.size);
        let type_start = row.bounds.x1 - type_width - 12.0;
        label(
            id,
            &column.name,
            Rect::new(
                badge_end + 6.0,
                row.bounds.y0,
                type_start - 6.0,
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
            Rect::new(type_start, row.bounds.y0, row.bounds.x1, row.bounds.y1),
            &TextStyle {
                align: TextAlign::Right,
                ..text_style
            },
            out,
        );
    }
}
