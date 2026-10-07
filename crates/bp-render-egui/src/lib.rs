//! Draws a [`DisplayList`] with egui's painter, plus the view transform
//! (pan and zoom) between page units and screen points.
//!
//! Convex fills use egui's anti-aliased polygons; concave ones (stars,
//! clouds, documents) are tessellated with lyon. Dashes are cut in page
//! units, as the SVG export does, so both show the same pattern.

mod icons;

use bp_model::{Color, ElementId, TextAlign};
use bp_scene::{DisplayList, Primitive, Stroke as SceneStroke, TextRun};
use bp_text::Face;
use egui::epaint::{Mesh, Vertex, WHITE_UV};
use egui::{
    Align2, Color32, FontData, FontDefinitions, FontFamily, FontId, Painter, Pos2, Rect, Shape,
    Stroke, Vec2,
};
use kurbo::{BezPath, PathEl, Point};
use lyon_tessellation::math::point as lyon_point;
use lyon_tessellation::path::Path as LyonPath;
use lyon_tessellation::{
    BuffersBuilder, FillOptions, FillRule, FillTessellator, FillVertex, VertexBuffers,
};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

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

    /// Page units per screen point.
    pub fn page_per_point(&self) -> f64 {
        1.0 / f64::from(self.zoom)
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

/// Temporary canvas emphasis. It never changes the shared display list or exports.
#[derive(Clone, Debug, PartialEq)]
pub struct ElementPresentation {
    pub opacity: f64,
    pub accent: Option<Color>,
    /// Backgrounds painted after the element's fills and before its first text run.
    pub text_backgrounds: Vec<(kurbo::Rect, Color)>,
}

impl Default for ElementPresentation {
    fn default() -> Self {
        Self {
            opacity: 1.0,
            accent: None,
            text_backgrounds: Vec::new(),
        }
    }
}

impl ElementPresentation {
    fn color(&self, color: Color, accented: bool) -> Color32 {
        color32(
            if accented {
                self.accent.map_or(color, |accent| {
                    Color {
                        r: accent.r,
                        g: accent.g,
                        b: accent.b,
                        a: color.a,
                    }
                    .faded(accent.opacity())
                })
            } else {
                color
            }
            .faded(self.opacity),
        )
    }
}

/// The egui font family that draws `face`.
pub fn font_family(face: Face) -> FontFamily {
    FontFamily::Name(face.name().into())
}

/// Whether the bundled faces are loaded in `ctx` yet. Fonts set with
/// [`egui::Context::set_fonts`] arrive on the next frame, and asking egui
/// for a family it doesn't know panics.
pub fn bundled_fonts_ready(ctx: &egui::Context) -> bool {
    ctx.fonts(|f| f.families().contains(&font_family(Face::Regular)))
}

/// A font for `face` at `size` points, or egui's default font until the
/// bundled faces are loaded.
pub fn text_font(ctx: &egui::Context, face: Face, size: f32) -> FontId {
    if bundled_fonts_ready(ctx) {
        FontId::new(size, font_family(face))
    } else {
        FontId::proportional(size)
    }
}

/// Font definitions with the bundled Inter faces: one named family per
/// face for the canvas, and Inter as the UI's proportional font (egui's
/// own fonts stay as fallbacks for symbols and emoji). Add more fonts if
/// needed, then pass the result to [`egui::Context::set_fonts`].
pub fn font_definitions() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    for face in Face::ALL {
        let name = face.name().to_owned();
        fonts
            .font_data
            .insert(name.clone(), Arc::new(FontData::from_static(face.data())));
        let mut chain = vec![name];
        chain.extend(fonts.families[&FontFamily::Proportional].iter().cloned());
        fonts.families.insert(font_family(face), chain);
    }
    if let Some(ui) = fonts.families.get_mut(&FontFamily::Proportional) {
        ui.insert(0, Face::Regular.name().to_owned());
    }
    fonts
}

/// Paints `list`, skipping items outside the painter's clip rect.
/// `hide_text_of` hides the text of the element being edited in place.
pub fn paint(
    painter: &Painter,
    origin: Pos2,
    view: &Viewport,
    list: &DisplayList,
    hide_text_of: Option<ElementId>,
) {
    paint_with_presentation(painter, origin, view, list, hide_text_of, &HashMap::new());
}

/// Paints with editor-only emphasis while retaining the scene's geometry and styles.
pub fn paint_with_presentation(
    painter: &Painter,
    origin: Pos2,
    view: &Viewport,
    list: &DisplayList,
    hide_text_of: Option<ElementId>,
    presentation: &HashMap<ElementId, ElementPresentation>,
) {
    let clip = painter.clip_rect();
    let tolerance = 0.25 / f64::from(view.zoom);
    let bundled = bundled_fonts_ready(painter.ctx());
    let normal = ElementPresentation::default();
    let mut backgrounds_painted = HashSet::new();
    for item in list.items() {
        if !clip.intersects(view.rect_to_screen(origin, item.bbox)) {
            continue;
        }
        let emphasis = presentation.get(&item.element).unwrap_or(&normal);
        match &item.primitive {
            Primitive::Path { path, fill, stroke } => {
                if let Some(fill) = fill {
                    let accented = stroke.as_ref().is_some_and(|stroke| stroke.color == *fill);
                    paint_fill(
                        painter,
                        origin,
                        view,
                        path,
                        emphasis.color(*fill, accented),
                        tolerance,
                    );
                }
                if let Some(stroke) = stroke {
                    paint_stroke(
                        painter,
                        origin,
                        view,
                        path,
                        stroke,
                        emphasis.color(stroke.color, true),
                        tolerance,
                    );
                }
            }
            Primitive::Text(run) => {
                if !emphasis.text_backgrounds.is_empty() && backgrounds_painted.insert(item.element)
                {
                    for (bounds, color) in &emphasis.text_backgrounds {
                        painter.rect_filled(
                            view.rect_to_screen(origin, *bounds),
                            0.0,
                            color32(*color),
                        );
                    }
                }
                if hide_text_of != Some(item.element) {
                    paint_text(
                        painter,
                        origin,
                        view,
                        run,
                        bundled,
                        emphasis.color(run.color, true),
                    );
                }
            }
            Primitive::Icon {
                svg,
                bounds,
                opacity,
            } => {
                icons::paint(
                    painter,
                    view.rect_to_screen(origin, *bounds),
                    svg,
                    *opacity * emphasis.opacity,
                );
            }
        }
    }
}

fn paint_text(
    painter: &Painter,
    origin: Pos2,
    view: &Viewport,
    run: &TextRun,
    bundled: bool,
    color: Color32,
) {
    let size = run.size as f32 * view.zoom;
    if size < 2.0 {
        return; // unreadable at this zoom; skip the work
    }
    let ascent = bp_text::metrics(run.face, run.size).ascent;
    let align = match run.align {
        TextAlign::Left => Align2::LEFT_TOP,
        TextAlign::Center => Align2::CENTER_TOP,
        TextAlign::Right => Align2::RIGHT_TOP,
    };
    let family = if bundled {
        font_family(run.face)
    } else {
        FontFamily::Proportional
    };
    let font = FontId::new(size, family);
    for line in &run.lines {
        if line.text.is_empty() {
            continue;
        }
        let top = view.to_screen(origin, Point::new(line.x, line.baseline - ascent));
        painter.text(top, align, &line.text, font.clone(), color);
    }
}

/// Flattens a path into screen-space polylines: `(points, closed)`.
fn flatten(
    path: &BezPath,
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

/// Whether a closed polygon turns the same way at every corner.
fn is_convex(points: &[Pos2]) -> bool {
    let n = points.len();
    if n < 3 {
        return false;
    }
    let mut sign = 0.0f32;
    for i in 0..n {
        let (a, b, c) = (points[i], points[(i + 1) % n], points[(i + 2) % n]);
        let cross = (b - a).x * (c - b).y - (b - a).y * (c - b).x;
        if cross.abs() < 1e-3 {
            continue;
        }
        if sign == 0.0 {
            sign = cross.signum();
        } else if cross.signum() != sign {
            return false;
        }
    }
    sign != 0.0
}

fn paint_fill(
    painter: &Painter,
    origin: Pos2,
    view: &Viewport,
    path: &BezPath,
    fill: Color32,
    tolerance: f64,
) {
    let polygons: Vec<Vec<Pos2>> = flatten(path, tolerance, view, origin)
        .into_iter()
        .filter(|(_, closed)| *closed)
        .map(|(points, _)| points)
        .collect();
    match polygons.as_slice() {
        [] => {}
        [single] if is_convex(single) => {
            painter.add(Shape::convex_polygon(single.clone(), fill, Stroke::NONE));
        }
        _ => {
            if let Some(mesh) = tessellate(&polygons, fill) {
                painter.add(Shape::mesh(mesh));
            }
            // A hairline in the fill colour anti-aliases the mesh's edges.
            for polygon in &polygons {
                painter.add(Shape::closed_line(polygon.clone(), Stroke::new(0.75, fill)));
            }
        }
    }
}

/// Fills any polygons (concave, self-intersecting, with holes) with the
/// non-zero rule, as SVG does by default.
fn tessellate(polygons: &[Vec<Pos2>], fill: Color32) -> Option<Mesh> {
    let mut builder = LyonPath::builder();
    for polygon in polygons {
        let (first, rest) = polygon.split_first()?;
        builder.begin(lyon_point(first.x, first.y));
        for p in rest {
            builder.line_to(lyon_point(p.x, p.y));
        }
        builder.end(true);
    }
    let path = builder.build();
    let mut buffers: VertexBuffers<Pos2, u32> = VertexBuffers::new();
    let options = FillOptions::default().with_fill_rule(FillRule::NonZero);
    FillTessellator::new()
        .tessellate_path(
            &path,
            &options,
            &mut BuffersBuilder::new(&mut buffers, |v: FillVertex| {
                let p = v.position();
                Pos2::new(p.x, p.y)
            }),
        )
        .ok()?;
    Some(Mesh {
        indices: buffers.indices,
        vertices: buffers
            .vertices
            .into_iter()
            .map(|pos| Vertex {
                pos,
                uv: WHITE_UV,
                color: fill,
            })
            .collect(),
        ..Mesh::default()
    })
}

fn paint_stroke(
    painter: &Painter,
    origin: Pos2,
    view: &Viewport,
    path: &BezPath,
    stroke: &SceneStroke,
    color: Color32,
    tolerance: f64,
) {
    let egui_stroke = Stroke::new((stroke.width as f32 * view.zoom).max(0.5), color);
    let dashed;
    let path = match stroke.dash {
        Some([on, off]) => {
            dashed = BezPath::from_iter(kurbo::dash(path.iter(), 0.0, &[on, off]));
            &dashed
        }
        None => path,
    };
    for (points, closed) in flatten(path, tolerance, view, origin) {
        if closed {
            painter.add(Shape::closed_line(points, egui_stroke));
        } else {
            painter.add(Shape::line(points, egui_stroke));
        }
    }
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
    use bp_scene::{DisplayItem, PlacedLine};

    fn emphasis_fixture() -> (ElementId, DisplayList) {
        let id = ElementId::new();
        let mut path = BezPath::new();
        path.move_to((10.0, 10.0));
        path.line_to((100.0, 10.0));
        let bounds = kurbo::Rect::new(0.0, 0.0, 120.0, 50.0);
        (
            id,
            DisplayList {
                groups: vec![Arc::from(vec![
                    DisplayItem {
                        element: id,
                        bbox: bounds,
                        primitive: Primitive::Path {
                            path,
                            fill: None,
                            stroke: Some(SceneStroke {
                                color: Color::BLACK,
                                width: 1.0,
                                dash: None,
                            }),
                        },
                    },
                    DisplayItem {
                        element: id,
                        bbox: bounds,
                        primitive: Primitive::Text(TextRun {
                            lines: vec![PlacedLine {
                                text: "parent_id".into(),
                                x: 10.0,
                                baseline: 35.0,
                                width: 80.0,
                            }],
                            face: Face::Regular,
                            size: 13.0,
                            color: Color::BLACK,
                            align: TextAlign::Left,
                        }),
                    },
                ])],
                background: None,
            },
        )
    }

    fn painted(
        list: &DisplayList,
        presentation: &HashMap<ElementId, ElementPresentation>,
    ) -> Vec<egui::epaint::ClippedShape> {
        let ctx = egui::Context::default();
        ctx.begin_pass(egui::RawInput::default());
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Middle,
            egui::Id::new("erd-emphasis-test"),
        ));
        paint_with_presentation(
            &painter,
            Pos2::ZERO,
            &Viewport::default(),
            list,
            None,
            presentation,
        );
        let mut output = ctx.end_pass();
        output.textures_delta.clear();
        output.shapes
    }

    #[test]
    fn temporary_emphasis_changes_stroke_and_label_colors_without_changing_scene_items() {
        let (id, list) = emphasis_fixture();
        let items: Vec<_> = list.items().cloned().collect();
        let normal = painted(&list, &HashMap::new());
        let faded = painted(
            &list,
            &HashMap::from([(
                id,
                ElementPresentation {
                    opacity: 0.25,
                    ..ElementPresentation::default()
                },
            )]),
        );
        let accent = Color::rgb(37, 99, 235);
        let focused = painted(
            &list,
            &HashMap::from([(
                id,
                ElementPresentation {
                    accent: Some(accent),
                    ..ElementPresentation::default()
                },
            )]),
        );
        for (shapes, expected) in [
            (&normal, Color32::BLACK),
            (&faded, color32(Color::BLACK.faded(0.25))),
            (&focused, color32(accent)),
        ] {
            let path = shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    Shape::Path(path) => Some(path),
                    _ => None,
                })
                .unwrap();
            assert_eq!(path.stroke.color, egui::epaint::ColorMode::Solid(expected));
            let text = shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    Shape::Text(text) => Some(text),
                    _ => None,
                })
                .unwrap();
            assert_eq!(text.galley.job.sections[0].format.color, expected);
            assert_eq!(
                path.points.len(),
                2,
                "emphasis does not add or reroute line segments"
            );
        }
        assert_eq!(list.items().cloned().collect::<Vec<_>>(), items);
    }

    #[test]
    fn row_background_is_painted_before_unchanged_text() {
        let (id, list) = emphasis_fixture();
        let shapes = painted(
            &list,
            &HashMap::from([(
                id,
                ElementPresentation {
                    text_backgrounds: vec![(
                        kurbo::Rect::new(0.0, 16.0, 120.0, 45.0),
                        Color::rgba(37, 99, 235, 33),
                    )],
                    ..ElementPresentation::default()
                },
            )]),
        );
        let background = shapes
            .iter()
            .position(|shape| matches!(&shape.shape, Shape::Rect(_)))
            .unwrap();
        let text = shapes
            .iter()
            .position(|shape| matches!(&shape.shape, Shape::Text(_)))
            .unwrap();
        assert!(background < text);
        let Shape::Text(text) = &shapes[text].shape else {
            unreachable!()
        };
        assert_eq!(text.galley.job.sections[0].format.color, Color32::BLACK);
    }

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

    #[test]
    fn convexity() {
        let square = [
            Pos2::new(0.0, 0.0),
            Pos2::new(1.0, 0.0),
            Pos2::new(1.0, 1.0),
            Pos2::new(0.0, 1.0),
        ];
        assert!(is_convex(&square));
        let arrow = [
            Pos2::new(0.0, 0.0),
            Pos2::new(2.0, 1.0),
            Pos2::new(0.0, 2.0),
            Pos2::new(0.5, 1.0),
        ];
        assert!(!is_convex(&arrow));
    }

    #[test]
    fn concave_shapes_tessellate() {
        let star: Vec<Pos2> = (0..10)
            .map(|i| {
                let a = std::f32::consts::TAU * i as f32 / 10.0;
                let r = if i % 2 == 0 { 10.0 } else { 4.0 };
                Pos2::new(r * a.cos(), r * a.sin())
            })
            .collect();
        let mesh = tessellate(&[star], Color32::RED).unwrap();
        assert!(
            mesh.indices.len() >= 8 * 3,
            "{} indices",
            mesh.indices.len()
        );
        assert!(mesh.vertices.iter().all(|v| v.color == Color32::RED));
    }
}
