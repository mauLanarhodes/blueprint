//! Turns a page of a document into a [`DisplayList`]: plain paths and text
//! runs that both the on-screen renderer and every exporter draw. Because
//! they share this list, an export always matches what is on screen.

use bp_model::{Color, Document, ElementId, PageId};
use kurbo::{BezPath, Point, Rect, Shape};

/// Line height as a multiple of the font size.
pub const LINE_HEIGHT: f64 = 1.25;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stroke {
    pub color: Color,
    pub width: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Primitive {
    Path {
        path: BezPath,
        fill: Option<Color>,
        stroke: Option<Stroke>,
    },
    /// Lines of text centred on `center`, stacked with [`LINE_HEIGHT`].
    Text {
        center: Point,
        lines: Vec<String>,
        font_size: f64,
        color: Color,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct DisplayItem {
    pub element: ElementId,
    /// Area the item covers, including half the stroke width. Used to skip
    /// items outside the viewport.
    pub bbox: Rect,
    pub primitive: Primitive,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DisplayList {
    /// Bottom to top.
    pub items: Vec<DisplayItem>,
    pub background: Option<Color>,
}

impl DisplayList {
    /// Union of every item's `bbox`, or `None` for an empty page.
    pub fn bounds(&self) -> Option<Rect> {
        self.items.iter().map(|i| i.bbox).reduce(|a, b| a.union(b))
    }
}

/// Builds the display list for `page`, bottom to top.
pub fn build_page(doc: &Document, page: PageId) -> DisplayList {
    let mut items = Vec::new();
    for element in doc.elements_on_page(page) {
        let style = &element.style;
        let stroke = style
            .stroke
            .filter(|_| style.stroke_width > 0.0)
            .map(|color| Stroke {
                color,
                width: style.stroke_width,
            });

        if style.fill.is_some() || stroke.is_some() {
            let path = bp_geom::outline(element.kind, element.bounds);
            let half = stroke.map_or(0.0, |s| s.width / 2.0);
            items.push(DisplayItem {
                element: element.id,
                bbox: path.bounding_box().inflate(half, half),
                primitive: Primitive::Path {
                    path,
                    fill: style.fill,
                    stroke,
                },
            });
        }

        if !element.text.trim().is_empty() {
            let lines: Vec<String> = element.text.split('\n').map(str::to_owned).collect();
            let center = element.bounds.center();
            let height = lines.len() as f64 * style.font_size * LINE_HEIGHT;
            // Rough width estimate; exact text layout arrives with bp-text.
            let longest = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0);
            let width = longest as f64 * style.font_size * 0.6;
            items.push(DisplayItem {
                element: element.id,
                bbox: Rect::from_center_size(center, (width, height)).union(element.bounds),
                primitive: Primitive::Text {
                    center,
                    lines,
                    font_size: style.font_size,
                    color: style.text_color,
                },
            });
        }
    }
    DisplayList {
        items,
        background: doc.pages.get(&page).map(|p| p.background),
    }
}

/// Baseline-centre positions for each line of a text primitive.
pub fn line_centers(
    center: Point,
    line_count: usize,
    font_size: f64,
) -> impl Iterator<Item = Point> {
    let step = font_size * LINE_HEIGHT;
    let first = center.y - step * (line_count.saturating_sub(1) as f64) / 2.0;
    (0..line_count).map(move |i| Point::new(center.x, first + step * i as f64))
}

#[cfg(test)]
mod tests {
    use super::*;
    use bp_model::{Element, ShapeKind};

    #[test]
    fn shapes_and_text_become_items() {
        let mut doc = Document::new();
        let page = doc.first_page().unwrap();
        let layer = doc.layers_of(page)[0].id;
        let mut a = Element::new(
            ShapeKind::Rectangle,
            layer,
            doc.next_order_key(layer),
            Rect::new(0.0, 0.0, 100.0, 40.0),
        );
        a.text = "Orders\nv2".into();
        doc.elements.insert(a.id, a.clone());
        let mut t = Element::new(
            ShapeKind::Text,
            layer,
            doc.next_order_key(layer),
            Rect::new(0.0, 60.0, 50.0, 80.0),
        );
        t.text = "Note".into();
        doc.elements.insert(t.id, t);

        let list = build_page(&doc, page);
        // Rectangle path + its text, then the text element (no outline).
        assert_eq!(list.items.len(), 3);
        assert!(matches!(list.items[0].primitive, Primitive::Path { .. }));
        assert!(
            matches!(&list.items[1].primitive, Primitive::Text { lines, .. } if lines.len() == 2)
        );
        assert!(matches!(list.items[2].primitive, Primitive::Text { .. }));
        let b = list.bounds().unwrap();
        assert!(b.x0 < 0.0 && b.y1 >= 80.0);
    }

    #[test]
    fn line_centers_are_stacked_around_the_middle() {
        let ys: Vec<f64> = line_centers(Point::new(0.0, 100.0), 3, 10.0)
            .map(|p| p.y)
            .collect();
        assert_eq!(ys, vec![87.5, 100.0, 112.5]);
    }
}