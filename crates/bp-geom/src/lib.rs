//! Shape outlines and hit-testing, shared by the canvas, the scene builder
//! and the exporters so all of them agree on what a shape looks like.

use bp_model::{Document, Element, ElementId, PageId, ShapeKind};
use kurbo::{BezPath, Ellipse, PathEl, Point, Rect, RoundedRect, Shape};

/// Corner radius of `ShapeKind::RoundedRectangle`, in page units.
pub const CORNER_RADIUS: f64 = 10.0;

/// Accuracy when converting curves to Béziers, in page units.
const CURVE_ACCURACY: f64 = 0.05;

/// The outline of a shape of `kind` filling `rect`. Always a closed path.
pub fn outline(kind: ShapeKind, rect: Rect) -> BezPath {
    let rect = rect.abs();
    let mut path = match kind {
        ShapeKind::Rectangle | ShapeKind::Text => rect.to_path(CURVE_ACCURACY),
        ShapeKind::RoundedRectangle => {
            let radius = CORNER_RADIUS
                .min(rect.width() / 2.0)
                .min(rect.height() / 2.0);
            RoundedRect::from_rect(rect, radius).to_path(CURVE_ACCURACY)
        }
        ShapeKind::Ellipse => Ellipse::from_rect(rect).to_path(CURVE_ACCURACY),
        ShapeKind::Diamond => {
            let c = rect.center();
            let mut path = BezPath::new();
            path.move_to((c.x, rect.y0));
            path.line_to((rect.x1, c.y));
            path.line_to((c.x, rect.y1));
            path.line_to((rect.x0, c.y));
            path
        }
    };
    // kurbo's ellipse path ends without a ClosePath; renderers need one to
    // fill the shape and to join the stroke cleanly.
    if !matches!(path.elements().last(), Some(PathEl::ClosePath)) {
        path.close_path();
    }
    path
}

/// Whether `point` is on `element`, allowing `tolerance` page units of slack.
pub fn hit_test(element: &Element, point: Point, tolerance: f64) -> bool {
    let grown = element.bounds.abs().inflate(tolerance, tolerance);
    if !grown.contains(point) {
        return false;
    }
    match element.kind {
        ShapeKind::Rectangle | ShapeKind::RoundedRectangle | ShapeKind::Text => true,
        ShapeKind::Ellipse | ShapeKind::Diamond => outline(element.kind, grown).contains(point),
    }
}

/// The top-most element under `point` on unlocked, visible layers of `page`.
pub fn topmost_at(doc: &Document, page: PageId, point: Point, tolerance: f64) -> Option<ElementId> {
    doc.elements_on_page(page)
        .into_iter()
        .rev()
        .filter(|e| doc.layers.get(&e.layer).is_some_and(|l| !l.locked))
        .find(|e| hit_test(e, point, tolerance))
        .map(|e| e.id)
}

/// The union of the bounds of `elements`, or `None` if there are none.
pub fn union_bounds<'a>(elements: impl IntoIterator<Item = &'a Element>) -> Option<Rect> {
    elements
        .into_iter()
        .map(|e| e.bounds.abs())
        .reduce(|a, b| a.union(b))
}

#[cfg(test)]
mod tests {
    use super::*;
    use bp_model::OrderKey;

    fn element(kind: ShapeKind) -> Element {
        let layer = bp_model::LayerId::new();
        Element::new(
            kind,
            layer,
            OrderKey::first(),
            Rect::new(0.0, 0.0, 100.0, 50.0),
        )
    }

    #[test]
    fn every_outline_is_closed() {
        for kind in ShapeKind::ALL {
            let path = outline(kind, Rect::new(0.0, 0.0, 40.0, 20.0));
            assert_eq!(path.elements().last(), Some(&PathEl::ClosePath), "{kind:?}");
        }
    }

    #[test]
    fn rectangle_hits_its_corners() {
        let e = element(ShapeKind::Rectangle);
        assert!(hit_test(&e, Point::new(1.0, 1.0), 0.0));
        assert!(!hit_test(&e, Point::new(-5.0, 1.0), 0.0));
        assert!(hit_test(&e, Point::new(-2.0, 1.0), 3.0));
    }

    #[test]
    fn ellipse_and_diamond_miss_their_corners() {
        for kind in [ShapeKind::Ellipse, ShapeKind::Diamond] {
            let e = element(kind);
            assert!(hit_test(&e, Point::new(50.0, 25.0), 0.0), "{kind:?} centre");
            assert!(!hit_test(&e, Point::new(2.0, 2.0), 0.0), "{kind:?} corner");
        }
    }

    #[test]
    fn topmost_prefers_the_last_drawn() {
        let mut doc = Document::new();
        let page = doc.first_page().unwrap();
        let layer = doc.layers_of(page)[0].id;
        let mut ids = vec![];
        for _ in 0..2 {
            let order = doc.next_order_key(layer);
            let e = Element::new(
                ShapeKind::Rectangle,
                layer,
                order,
                Rect::new(0.0, 0.0, 10.0, 10.0),
            );
            ids.push(e.id);
            doc.elements.insert(e.id, e);
        }
        assert_eq!(
            topmost_at(&doc, page, Point::new(5.0, 5.0), 0.0),
            Some(ids[1])
        );
        assert_eq!(topmost_at(&doc, page, Point::new(50.0, 5.0), 0.0), None);
    }
}