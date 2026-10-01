//! Draws a [`DisplayList`] with egui's painter, plus the view transform
//! (pan and zoom) between page units and screen points.
//!
//! Phase 0 shapes are all convex, so egui's convex-polygon fill is enough.
//! Concave paths (Phase 1 connectors and stencils) will need tessellation.

use bp_model::Color;
use bp_scene::{DisplayList, Primitive, line_centers};
use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Shape, Stroke, Vec2};
use kurbo::{PathEl, Point};

pub const MIN_ZOOM: f32 = 0.05;
pub const MAX_ZOOM: f32 = 32.0;

/// Maps page units to screen points: `screen = origin + pan + page * zoom`,
/// where `origin` is the top-left corner of the canvas widget.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Viewport {
    pub pan: Vec2,
    pub zoom: f32,
}

impl Default for Viewport {
    fn default() -> Self {
        Self {
            pan: Vec2::new(40.0, 40.0),
            zoom: 1.0,
        }
    }
}

impl Viewport {
    pub fn to_screen(&self, origin: Pos2, p: Point) -> Pos2 {
        origin + self.pan + Vec2::new(p.x as f32, p.y as f32) * self.zoom
    }

    pub fn to_page(&self, origin: Pos2, p: Pos2) -> Point {
        let v = (p - origin - self.pan) / self.zoom;
        Point::new(f64::from(v.x), f64::from(v.y))
    }

    pub fn rect_to_screen(&self, origin: Pos2, r: kurbo::Rect) -> Rect {
        Rect::from_two_pos(
            self.to_screen(origin, Point::new(r.x0, r.y0)),
            self.to_screen(origin, Point::new(r.x1, r.y1)),
        )
    }

    pub fn screen_to_page_rect(&self, origin: Pos2, r: Rect) -> kurbo::Rect {
        kurbo::Rect::from_points(self.to_page(origin, r.min), self.to_page(origin, r.max))
    }

    /// Zooms by `factor`, keeping the page point under `anchor` still.
    pub fn zoom_around(&mut self, origin: Pos2, anchor: Pos2, factor: f32) {
        let fixed = self.to_page(origin, anchor);
        self.zoom = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        self.pan += anchor - self.to_screen(origin, fixed);
    }

    /// Fits `content` (page units) into `canvas` (screen) with a margin,
    /// never zooming in past 100%.
    pub fn fit(&mut self, canvas: Rect, content: kurbo::Rect, margin: f32) {
        let avail = (canvas.size() - Vec2::splat(margin * 2.0)).max(Vec2::splat(1.0));
        let w = (content.width() as f32).max(1.0);
        let h = (content.height() as f32).max(1.0);
        self.zoom = (avail.x / w).min(avail.y / h).clamp(MIN_ZOOM, 1.0);
        let center = content.center();
        let scaled = Vec2::new(center.x as f32, center.y as f32) * self.zoom;
        self.pan = canvas.size() / 2.0 - scaled;
    }
}

pub fn color32(c: Color) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a)
}

/// Paints `list`, skipping items outside the painter's clip rect.
/// `hide_text_of` hides the text of the element being edited in place.
pub fn paint(
    painter: &Painter,
    origin: Pos2,
    view: &Viewport,
    list: &DisplayList,
    hide_text_of: Option<bp_model::ElementId>,
) {
    let clip = painter.clip_rect();
    let tolerance = 0.25 / f64::from(view.zoom);
    for item in &list.items {
        if !clip.intersects(view.rect_to_screen(origin, item.bbox)) {
            continue;
        }
        match &item.primitive {
            Primitive::Path { path, fill, stroke } => {
                let stroke = stroke.map_or(Stroke::NONE, |s| {
                    Stroke::new((s.width as f32 * view.zoom).max(0.5), color32(s.color))
                });
                let fill = fill.map_or(Color32::TRANSPARENT, color32);
                for (points, closed) in flatten(path, tolerance, view, origin) {
                    if closed {
                        painter.add(Shape::convex_polygon(points, fill, stroke));
                    } else {
                        painter.add(Shape::line(points, stroke));
                    }
                }
            }
            Primitive::Text {
                center,
                lines,
                font_size,
                color,
            } => {
                if hide_text_of == Some(item.element) {
                    continue;
                }
                let size = *font_size as f32 * view.zoom;
                if size < 2.0 {
                    continue; // unreadable at this zoom; skip the work
                }
                for (line, at) in lines
                    .iter()
                    .zip(line_centers(*center, lines.len(), *font_size))
                {
                    painter.text(
                        view.to_screen(origin, at),
                        Align2::CENTER_CENTER,
                        line,
                        FontId::proportional(size),
                        color32(*color),
                    );
                }
            }
        }
    }
}

/// Flattens a Bézier path into screen-space polylines: `(points, closed)`.
fn flatten(
    path: &kurbo::BezPath,
    tolerance: f64,
    view: &Viewport,
    origin: Pos2,
) -> Vec<(Vec<Pos2>, bool)> {
    let mut out = Vec::new();
    let mut current: Vec<Pos2> = Vec::new();
    kurbo::flatten(path.iter(), tolerance, |el| match el {
        PathEl::MoveTo(p) => {
            if current.len() > 1 {
                out.push((std::mem::take(&mut current), false));
            }
            current.clear();
            current.push(view.to_screen(origin, p));
        }
        PathEl::LineTo(p) => current.push(view.to_screen(origin, p)),
        PathEl::ClosePath => {
            if current.len() > 1 && current.first() == current.last() {
                current.pop();
            }
            if current.len() > 2 {
                out.push((std::mem::take(&mut current), true));
            }
            current.clear();
        }
        // `flatten` only emits the three variants above.
        PathEl::QuadTo(..) | PathEl::CurveTo(..) => {}
    });
    if current.len() > 1 {
        out.push((current, false));
    }
    out
}

/// Draws grid lines every `spacing` page units, doubling the spacing until
/// lines are at least 8 points apart on screen.
pub fn paint_grid(painter: &Painter, origin: Pos2, view: &Viewport, spacing: f64, color: Color32) {
    let mut step = spacing;
    while step * f64::from(view.zoom) < 8.0 {
        step *= 2.0;
    }
    let area = view.screen_to_page_rect(origin, painter.clip_rect());
    let stroke = Stroke::new(1.0, color);
    let mut x = (area.x0 / step).floor() * step;
    while x <= area.x1 {
        let a = view.to_screen(origin, Point::new(x, area.y0));
        let b = view.to_screen(origin, Point::new(x, area.y1));
        painter.line_segment([a, b], stroke);
        x += step;
    }
    let mut y = (area.y0 / step).floor() * step;
    while y <= area.y1 {
        let a = view.to_screen(origin, Point::new(area.x0, y));
        let b = view.to_screen(origin, Point::new(area.x1, y));
        painter.line_segment([a, b], stroke);
        y += step;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screen_and_page_round_trip() {
        let view = Viewport {
            pan: Vec2::new(10.0, 20.0),
            zoom: 2.0,
        };
        let origin = Pos2::new(100.0, 50.0);
        let p = Point::new(5.0, 7.0);
        let s = view.to_screen(origin, p);
        assert_eq!(s, Pos2::new(120.0, 84.0));
        assert_eq!(view.to_page(origin, s), p);
    }

    #[test]
    fn zoom_keeps_anchor_fixed() {
        let mut view = Viewport::default();
        let origin = Pos2::ZERO;
        let anchor = Pos2::new(300.0, 200.0);
        let before = view.to_page(origin, anchor);
        view.zoom_around(origin, anchor, 1.5);
        let after = view.to_page(origin, anchor);
        assert!((before - after).hypot() < 1e-3);
    }

    #[test]
    fn fit_centres_content() {
        let mut view = Viewport::default();
        let canvas = Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0));
        view.fit(canvas, kurbo::Rect::new(0.0, 0.0, 200.0, 100.0), 20.0);
        assert_eq!(view.zoom, 1.0);
        assert_eq!(
            view.to_screen(Pos2::ZERO, Point::new(100.0, 50.0)),
            Pos2::new(400.0, 300.0)
        );
    }
}