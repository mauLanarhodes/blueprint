//! The drawing canvas: pointer input, painting and in-place text editing.

use crate::app::{BlueprintApp, Tool};
use bp_commands::Command;
use bp_model::kurbo::{Point, Rect};
use bp_model::{Color, ElementId, ShapeKind};
use bp_render_egui::{Viewport, color32, paint, paint_grid};
use bp_scene::{DisplayItem, DisplayList, Primitive, Stroke as SceneStroke};
use egui::{
    Color32, CursorIcon, Key, PointerButton, Pos2, Response, Sense, Stroke, StrokeKind, Vec2,
};

pub const ACCENT: Color32 = Color32::from_rgb(37, 99, 235);
/// Side length of a resize handle, in screen points.
const HANDLE: f32 = 9.0;
/// Grid spacing in page units.
const GRID: f64 = 20.0;

/// What the pointer is doing on the canvas.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Drag {
    #[default]
    None,
    Pan,
    Move {
        id: ElementId,
        last: Point,
    },
    /// `anchor` is the corner that stays put.
    Resize {
        id: ElementId,
        anchor: Point,
    },
    Create {
        kind: ShapeKind,
        start: Point,
        current: Point,
    },
}

impl Drag {
    /// Whether this drag changes the document (and holds an open undo step).
    pub fn is_edit(&self) -> bool {
        matches!(self, Drag::Move { .. } | Drag::Resize { .. })
    }
}

impl BlueprintApp {
    pub fn canvas(&mut self, ui: &mut egui::Ui) {
        let (response, painter) = ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
        let ctx = ui.ctx().clone();
        let origin = response.rect.min;
        self.canvas_rect = response.rect;

        if self.fit_requested {
            self.fit_requested = false;
            match bp_scene::build_page(&self.doc, self.page).bounds() {
                Some(bounds) => self.view.fit(response.rect, bounds, 48.0),
                None => self.view = Viewport::default(),
            }
        }

        // Scrolling pans; Ctrl/Cmd + scroll or a pinch zooms.
        if response.hovered() {
            let (zoom, scroll, hover) = ctx.input(|i| {
                (
                    i.zoom_delta(),
                    i.smooth_scroll_delta(),
                    i.pointer.hover_pos(),
                )
            });
            if zoom != 1.0
                && let Some(anchor) = hover
            {
                self.view.zoom_around(origin, anchor, zoom);
            }
            self.view.pan += scroll;
        }

        self.pointer_input(&ctx, &response, origin);

        let background = self
            .doc
            .pages
            .get(&self.page)
            .map_or(Color32::WHITE, |p| color32(p.background));
        painter.rect_filled(response.rect, 0.0, background);
        if self.show_grid {
            paint_grid(&painter, origin, &self.view, GRID, Color32::from_gray(234));
        }
        // Rebuilt every frame for now; per-element caching comes with Phase 1.
        let list = bp_scene::build_page(&self.doc, self.page);
        paint(
            &painter,
            origin,
            &self.view,
            &list,
            self.editing.as_ref().map(|e| e.id),
        );
        self.paint_overlays(&painter, origin);
        self.update_cursor(&ctx, &response, origin);
    }

    fn pointer_input(&mut self, ctx: &egui::Context, response: &Response, origin: Pos2) {
        let view = self.view;
        let to_page = |p: Pos2| view.to_page(origin, p);
        let space = ctx.input(|i| i.key_down(Key::Space));

        if response.drag_started() {
            let start = ctx
                .input(|i| i.pointer.press_origin())
                .or(response.interact_pointer_pos());
            let primary = response.dragged_by(PointerButton::Primary);
            self.drag = match start {
                _ if !primary || space || self.tool == Tool::Pan => Drag::Pan,
                Some(start) => self.begin_drag(start, origin),
                None => Drag::None,
            };
        }

        if response.dragged()
            && let Some(pos) = response.interact_pointer_pos()
        {
            let p = to_page(pos);
            match self.drag {
                Drag::Pan => self.view.pan += response.drag_delta(),
                Drag::Move { id, last } => {
                    let delta = p - last;
                    self.drag = Drag::Move { id, last: p };
                    if let Some(bounds) = self.doc.elements.get(&id).map(|e| e.bounds) {
                        self.apply(
                            "Move",
                            [Command::SetBounds {
                                id,
                                bounds: bounds + delta,
                            }],
                        );
                    }
                }
                Drag::Resize { id, anchor } => {
                    let bounds = Rect::from_points(anchor, p);
                    self.apply("Resize", [Command::SetBounds { id, bounds }]);
                }
                Drag::Create { kind, start, .. } => {
                    self.drag = Drag::Create {
                        kind,
                        start,
                        current: p,
                    };
                }
                Drag::None => {}
            }
        }

        if response.drag_stopped() {
            match std::mem::take(&mut self.drag) {
                Drag::Move { .. } | Drag::Resize { .. } => self.history.commit(),
                Drag::Create {
                    kind,
                    start,
                    current,
                } => {
                    let bounds = Rect::from_points(start, current);
                    if bounds.width() < 4.0 && bounds.height() < 4.0 {
                        self.create_shape_at(kind, start);
                    } else {
                        self.create_shape(kind, bounds);
                    }
                }
                Drag::Pan | Drag::None => {}
            }
        }

        if response.clicked()
            && let Some(pos) = response.interact_pointer_pos()
        {
            let p = to_page(pos);
            match self.tool {
                Tool::Shape(kind) => self.create_shape_at(kind, p),
                Tool::Select => self.selection = self.hit(p),
                Tool::Pan => {}
            }
        }

        if response.double_clicked()
            && self.tool == Tool::Select
            && let Some(pos) = response.interact_pointer_pos()
        {
            let p = to_page(pos);
            match self.hit(p) {
                Some(id) => self.start_text_edit(id),
                None => self.create_shape_at(ShapeKind::Text, p),
            }
        }
    }

    /// Starts a primary-button drag at screen position `start`.
    fn begin_drag(&mut self, start: Pos2, origin: Pos2) -> Drag {
        let p = self.view.to_page(origin, start);
        match self.tool {
            Tool::Pan => Drag::Pan,
            Tool::Shape(kind) => Drag::Create {
                kind,
                start: p,
                current: p,
            },
            Tool::Select => {
                if let Some((id, anchor)) = self.handle_at(start, origin) {
                    self.history.begin("Resize");
                    return Drag::Resize { id, anchor };
                }
                match self.hit(p) {
                    Some(id) => {
                        self.selection = Some(id);
                        self.history.begin("Move");
                        Drag::Move { id, last: p }
                    }
                    None => {
                        // Marquee selection arrives in Phase 1.
                        self.selection = None;
                        Drag::None
                    }
                }
            }
        }
    }

    fn hit(&self, p: Point) -> Option<ElementId> {
        let tolerance = 3.0 / f64::from(self.view.zoom);
        bp_geom::topmost_at(&self.doc, self.page, p, tolerance)
    }

    /// Screen positions of the selected element's corners, each paired with
    /// the opposite corner in page units.
    fn handles(&self, origin: Pos2) -> Vec<(ElementId, Pos2, Point)> {
        let Some(id) = self.selection else {
            return vec![];
        };
        let Some(b) = self.doc.elements.get(&id).map(|e| e.bounds) else {
            return vec![];
        };
        [
            (Point::new(b.x0, b.y0), Point::new(b.x1, b.y1)),
            (Point::new(b.x1, b.y0), Point::new(b.x0, b.y1)),
            (Point::new(b.x1, b.y1), Point::new(b.x0, b.y0)),
            (Point::new(b.x0, b.y1), Point::new(b.x1, b.y0)),
        ]
        .into_iter()
        .map(|(corner, opposite)| (id, self.view.to_screen(origin, corner), opposite))
        .collect()
    }

    fn handle_at(&self, pos: Pos2, origin: Pos2) -> Option<(ElementId, Point)> {
        self.handles(origin)
            .into_iter()
            .find(|(_, at, _)| (*at - pos).abs().max_elem() <= HANDLE)
            .map(|(id, _, anchor)| (id, anchor))
    }

    fn paint_overlays(&self, painter: &egui::Painter, origin: Pos2) {
        if let Drag::Create {
            kind,
            start,
            current,
        } = self.drag
        {
            let accent = Color::rgb(ACCENT.r(), ACCENT.g(), ACCENT.b());
            let path = bp_geom::outline(kind, Rect::from_points(start, current));
            let preview = DisplayList {
                items: vec![DisplayItem {
                    element: ElementId::new(),
                    bbox: Rect::from_points(start, current),
                    primitive: Primitive::Path {
                        path,
                        fill: Some(Color::rgba(accent.r, accent.g, accent.b, 30)),
                        stroke: Some(SceneStroke {
                            color: accent,
                            width: 1.5 / f64::from(self.view.zoom),
                        }),
                    },
                }],
                background: None,
            };
            paint(painter, origin, &self.view, &preview, None);
        }

        if let Some(bounds) = self
            .selection
            .and_then(|id| self.doc.elements.get(&id))
            .map(|e| e.bounds)
        {
            let rect = self.view.rect_to_screen(origin, bounds);
            painter.rect_stroke(rect, 0.0, Stroke::new(1.5, ACCENT), StrokeKind::Outside);
            for (_, at, _) in self.handles(origin) {
                let handle = egui::Rect::from_center_size(at, Vec2::splat(HANDLE));
                painter.rect_filled(handle, 2.0, Color32::WHITE);
                painter.rect_stroke(handle, 2.0, Stroke::new(1.5, ACCENT), StrokeKind::Inside);
            }
        }
    }

    fn update_cursor(&self, ctx: &egui::Context, response: &Response, origin: Pos2) {
        let space = ctx.input(|i| i.key_down(Key::Space));
        let icon = match self.drag {
            Drag::Pan => CursorIcon::Grabbing,
            Drag::Move { .. } => CursorIcon::Move,
            Drag::Resize { .. } | Drag::Create { .. } => CursorIcon::Crosshair,
            Drag::None if !response.hovered() => return,
            Drag::None if space || self.tool == Tool::Pan => CursorIcon::Grab,
            Drag::None => match self.tool {
                Tool::Shape(_) => CursorIcon::Crosshair,
                _ => {
                    let hover = response.hover_pos();
                    if hover.is_some_and(|p| self.handle_at(p, origin).is_some()) {
                        CursorIcon::Crosshair
                    } else if hover
                        .is_some_and(|p| self.hit(self.view.to_page(origin, p)).is_some())
                    {
                        CursorIcon::Move
                    } else {
                        CursorIcon::Default
                    }
                }
            },
        };
        ctx.set_cursor_icon(icon);
    }

    /// Draws the in-place text editor over the element being edited.
    pub fn text_editor(&mut self, ctx: &egui::Context) {
        let Some(id) = self.editing.as_ref().map(|e| e.id) else {
            return;
        };
        let Some(element) = self.doc.elements.get(&id) else {
            self.editing = None;
            return;
        };
        let screen = self
            .view
            .rect_to_screen(self.canvas_rect.min, element.bounds);
        let font_size = (element.style.font_size as f32 * self.view.zoom).clamp(10.0, 48.0);
        let width = screen.width().max(140.0);
        let at = Pos2::new(
            screen.center().x - width / 2.0,
            screen.center().y - font_size,
        );

        let Some(edit) = self.editing.as_mut() else {
            return;
        };
        let mut finish = None;
        egui::Area::new(egui::Id::new("bp-text-editor"))
            .order(egui::Order::Foreground)
            .fixed_pos(at)
            .show(ctx, |ui| {
                let output = egui::TextEdit::multiline(&mut edit.text)
                    .font(egui::FontId::proportional(font_size))
                    .horizontal_align(egui::Align::Center)
                    .desired_width(width)
                    .desired_rows(1)
                    .show(ui);
                if !edit.focused_once {
                    output.response.request_focus();
                    edit.focused_once = true;
                }
                let (escape, ctrl_enter) = ui.ctx().input(|i| {
                    (
                        i.key_pressed(Key::Escape),
                        i.key_pressed(Key::Enter) && i.modifiers.command,
                    )
                });
                if escape {
                    finish = Some(false);
                } else if ctrl_enter || output.response.lost_focus() {
                    finish = Some(true);
                }
            });
        if let Some(keep) = finish {
            self.finish_text_edit(keep);
        }
    }
}