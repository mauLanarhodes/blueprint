//! The drawing canvas: pointer input, painting, overlays and in-place text
//! editing.

use crate::app::{BlueprintApp, Tool};
use crate::theme::{ACCENT, GRID, GUIDE, PORT};
use bp_commands::edit::{self, rect_to_rect};
use bp_commands::{Command, Prop};
use bp_geom::{Axis, Dir, Features, polyline_length, snap_point, snap_rect};
use bp_model::kurbo::{Affine, Point, Rect};
use bp_model::{
    Connector, Document, ElementId, Endpoint, Paint, PortId, Routing, ShapeRef, TextAlign,
};
use bp_render_egui::{color32, paint, paint_grid, paint_with_presentation, text_font};
use bp_scene::{DisplayItem, DisplayList, Geometry};
use egui::{
    Color32, CursorIcon, Key, PointerButton, Pos2, Response, Sense, Shape, Stroke, StrokeKind,
    Vec2 as ScreenVec,
};
use std::sync::Arc;

/// Side length of a resize handle, in screen points.
const HANDLE: f32 = 8.0;
/// How close (in screen points) the pointer must be to hit something.
const HIT: f32 = 5.0;
/// How close (in screen points) a dragged edge must be to snap.
const SNAP: f32 = 6.0;
/// How close (in screen points) a connector end must be to glue to a port.
const PORT_SNAP: f32 = 12.0;
/// Distance of the connect arrows from a selected shape's sides.
const ARROW_GAP: f32 = 22.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handle {
    N,
    NE,
    E,
    SE,
    S,
    SW,
    W,
    NW,
}

impl Handle {
    const ALL: [Handle; 8] = [
        Handle::N,
        Handle::NE,
        Handle::E,
        Handle::SE,
        Handle::S,
        Handle::SW,
        Handle::W,
        Handle::NW,
    ];

    /// Where on a box this handle sits, as fractions.
    fn anchor(self) -> (f64, f64) {
        match self {
            Handle::N => (0.5, 0.0),
            Handle::NE => (1.0, 0.0),
            Handle::E => (1.0, 0.5),
            Handle::SE => (1.0, 1.0),
            Handle::S => (0.5, 1.0),
            Handle::SW => (0.0, 1.0),
            Handle::W => (0.0, 0.5),
            Handle::NW => (0.0, 0.0),
        }
    }

    fn at(self, r: Rect) -> Point {
        let (fx, fy) = self.anchor();
        Point::new(r.x0 + fx * r.width(), r.y0 + fy * r.height())
    }

    fn cursor(self) -> CursorIcon {
        match self {
            Handle::N | Handle::S => CursorIcon::ResizeVertical,
            Handle::E | Handle::W => CursorIcon::ResizeHorizontal,
            Handle::NE | Handle::SW => CursorIcon::ResizeNeSw,
            Handle::NW | Handle::SE => CursorIcon::ResizeNwSe,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum End {
    Source,
    Target,
}

/// Something on the canvas the pointer can grab.
#[derive(Clone, Debug, PartialEq)]
enum Grab {
    Resize(Handle),
    ConnectorEnd(ElementId, End),
    Waypoint(ElementId, usize),
    /// A virtual handle halfway along a segment of a straight or curved
    /// connector: dragging it adds a waypoint at this index.
    NewWaypoint(ElementId, usize),
    /// An inner segment of an orthogonal route, between route points
    /// `index` and `index + 1`.
    Segment(ElementId, usize),
    Label(ElementId),
    /// A connect arrow beside a selected shape, or a port of a hovered
    /// shape: dragging it draws a new connector.
    Port(ElementId, PortId),
}

/// What the pointer is doing on the canvas.
#[derive(Default)]
pub enum Drag {
    #[default]
    None,
    Pan,
    Marquee {
        start: Point,
        current: Point,
        additive: bool,
    },
    Move {
        ids: Vec<ElementId>,
        start: Point,
        bounds: Option<Rect>,
        snapshot: Box<Document>,
    },
    Resize {
        ids: Vec<ElementId>,
        handle: Handle,
        start_bounds: Rect,
        snapshot: Box<Document>,
    },
    Create {
        shape: ShapeRef,
        start: Point,
        current: Point,
    },
    Connect {
        source: Endpoint,
        target: Endpoint,
    },
    MoveEnd {
        id: ElementId,
        end: End,
    },
    MoveWaypoint {
        id: ElementId,
        index: usize,
    },
    MoveSegment {
        id: ElementId,
        /// Waypoint indices of the segment's two corners.
        corners: [usize; 2],
        horizontal: bool,
        start: Point,
        original: Vec<Point>,
    },
    MoveLabel {
        id: ElementId,
    },
}

impl Drag {
    pub fn is_active(&self) -> bool {
        !matches!(self, Drag::None)
    }

    /// Whether this drag edits the document inside an open undo step.
    pub fn edits(&self) -> bool {
        matches!(
            self,
            Drag::Move { .. }
                | Drag::Resize { .. }
                | Drag::MoveEnd { .. }
                | Drag::MoveWaypoint { .. }
                | Drag::MoveSegment { .. }
                | Drag::MoveLabel { .. }
        )
    }
}

impl BlueprintApp {
    pub fn canvas(&mut self, ui: &mut egui::Ui) {
        let (response, painter) = ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
        let ctx = ui.ctx().clone();
        let origin = response.rect.min;
        self.canvas_rect = response.rect;
        self.floating_toolbar(&ctx);

        if self.fit_requested {
            self.fit_requested = false;
            self.refresh_scene();
            match self.scene.bounds() {
                Some(bounds) => self.view.fit(response.rect, bounds, 48.0),
                None => self.view = bp_render_egui::Viewport::default(),
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
        self.pointer = response
            .hover_pos()
            .or(response.interact_pointer_pos())
            .map(|p| self.view.to_page(origin, p));

        self.pointer_input(&ctx, &response, origin);
        self.palette_drop(&ctx, &response, origin);
        self.refresh_scene();

        let background = self
            .doc
            .pages
            .get(&self.page)
            .map_or(Color32::WHITE, |p| color32(p.background));
        painter.rect_filled(response.rect, 0.0, background);
        if self.settings.show_grid {
            paint_grid(&painter, origin, &self.view, self.settings.grid * 2.0, GRID);
        }
        let erd_focus = self.erd_focus();
        paint_with_presentation(
            &painter,
            origin,
            &self.view,
            &self.scene.list,
            self.editing.as_ref().map(|e| e.id),
            &self.erd_presentation(&erd_focus),
        );
        for id in &erd_focus.tables {
            if !self.is_selected(*id)
                && let Some(shape) = self.scene.shape(*id)
            {
                painter.rect_stroke(
                    self.view.rect_to_screen(origin, shape.bounds).expand(2.0),
                    0.0,
                    Stroke::new(1.5, ACCENT.gamma_multiply(0.75)),
                    StrokeKind::Outside,
                );
            }
        }
        self.paint_overlays(&painter, origin, &response);
        self.update_cursor(&ctx, &response, origin);
    }

    /// The page point under a screen position.
    fn page_at(&self, origin: Pos2, p: Pos2) -> Point {
        self.view.to_page(origin, p)
    }

    /// Screen points to page units at the current zoom.
    fn units(&self, points: f32) -> f64 {
        f64::from(points) * self.view.page_per_point()
    }

    fn snap_disabled(ctx: &egui::Context) -> bool {
        ctx.input(|i| i.modifiers.alt || i.modifiers.command)
    }

    fn grid_step(&self) -> Option<f64> {
        self.settings.snap_to_grid.then_some(self.settings.grid)
    }

    fn snap_to_grid(&self, p: Point) -> Point {
        match self.grid_step() {
            Some(g) => Point::new((p.x / g).round() * g, (p.y / g).round() * g),
            None => p,
        }
    }

    /// The topmost element under `p` that clicks can land on.
    fn hit(&self, p: Point) -> Option<ElementId> {
        let tolerance = self.units(HIT);
        self.scene.hit(p, tolerance, |id| self.is_hittable(id))
    }

    /// Boxes of shapes near `area` to snap against, leaving out `moving`
    /// and everything inside it.
    fn snap_targets(&self, moving: &[ElementId], area: Rect) -> Vec<Rect> {
        if !self.settings.snap_to_shapes {
            return Vec::new();
        }
        let mut skip: std::collections::HashSet<ElementId> = moving.iter().copied().collect();
        for id in moving {
            skip.extend(self.doc.descendants(*id));
        }
        let near = area.inflate(400.0, 400.0).union(self.visible_page_rect());
        self.scene
            .query(near)
            .into_iter()
            .filter(|id| !skip.contains(id))
            .filter_map(|id| self.scene.shape(id).map(|g| g.bounds))
            .collect()
    }

    // ----- Handles --------------------------------------------------------

    /// The selection's box, for resize handles: shapes and groups only.
    fn resize_bounds(&self) -> Option<Rect> {
        if self.editing.is_some() || self.selection.is_empty() {
            return None;
        }
        let editable = self.editable_selection();
        if editable.len() != self.selection.len() {
            return None;
        }
        let boxes: Vec<Rect> = editable
            .iter()
            .filter(|id| !self.doc.elements.get(id).is_some_and(|e| e.is_connector()))
            .filter_map(|id| self.scene.bounds_of(*id))
            .collect();
        boxes.into_iter().reduce(|a, b| a.union(b))
    }

    /// The single selected connector, if any.
    fn selected_connector(&self) -> Option<(ElementId, &Connector)> {
        match self.selection[..] {
            [id] => Some((id, self.doc.elements.get(&id)?.as_connector()?)),
            _ => None,
        }
    }

    /// The single selected shape, if any.
    fn selected_shape(&self) -> Option<ElementId> {
        match self.selection[..] {
            [id] if self.doc.elements.get(&id).is_some_and(|e| e.is_shape()) => Some(id),
            _ => None,
        }
    }

    /// The points a straight or curved connector passes through:
    /// its ends and waypoints.
    fn control_points(&self, id: ElementId, c: &Connector) -> Option<Vec<Point>> {
        let g = self.scene.connector(id)?;
        let mut pts = vec![*g.points.first()?];
        pts.extend_from_slice(&c.waypoints);
        pts.push(*g.points.last()?);
        Some(pts)
    }

    /// The connect arrows beside a selected shape: (port, arrow position).
    fn connect_arrows(&self, origin: Pos2) -> Vec<(ElementId, PortId, Pos2, Dir)> {
        let Some(id) = self.selected_shape() else {
            return Vec::new();
        };
        if self.doc.is_locked(id) || self.editing.is_some() || self.drag.is_active() {
            return Vec::new();
        }
        let Some(g) = self.scene.shape(id) else {
            return Vec::new();
        };
        let rect = self.view.rect_to_screen(origin, g.bounds);
        g.visible_ports()
            .filter(|p| Dir::ALL.iter().any(|d| d.name() == p.id.as_str()))
            .map(|p| {
                let at = match p.dir {
                    Dir::N => Pos2::new(rect.center().x, rect.min.y - ARROW_GAP),
                    Dir::E => Pos2::new(rect.max.x + ARROW_GAP, rect.center().y),
                    Dir::S => Pos2::new(rect.center().x, rect.max.y + ARROW_GAP),
                    Dir::W => Pos2::new(rect.min.x - ARROW_GAP, rect.center().y),
                };
                (id, p.id.clone(), at, p.dir)
            })
            .collect()
    }

    /// The shape whose ports show under the pointer.
    fn hovered_shape(&self) -> Option<ElementId> {
        if self.drag.is_active() {
            return None;
        }
        self.port_shape_at(self.pointer?)
    }

    /// The shape whose ports are within reach of `p`, if ports can be
    /// used with the current tool.
    fn port_shape_at(&self, p: Point) -> Option<ElementId> {
        if !matches!(self.tool, Tool::Select | Tool::Connector) {
            return None;
        }
        let tolerance = self.units(PORT_SNAP);
        // A port just outside a shape still counts as hovering it.
        self.scene
            .port_near(p, tolerance, |id| self.is_hittable(id))
            .map(|(id, _)| id)
            .or_else(|| {
                self.hit(p)
                    .filter(|id| self.doc.elements.get(id).is_some_and(|e| e.is_shape()))
            })
            .filter(|id| !self.doc.is_locked(*id))
    }

    /// What the pointer at screen position `pos` would grab.
    fn grab_at(&self, pos: Pos2, origin: Pos2) -> Option<Grab> {
        let near = |a: Pos2| (a - pos).length() <= HANDLE;
        let at = |p: Point| self.view.to_screen(origin, p);

        if let Some((id, c)) = self.selected_connector()
            && !self.doc.is_locked(id)
            && let Some(g) = self.scene.connector(id)
        {
            if g.points.first().is_some_and(|p| near(at(*p))) {
                return Some(Grab::ConnectorEnd(id, End::Source));
            }
            if g.points.last().is_some_and(|p| near(at(*p))) {
                return Some(Grab::ConnectorEnd(id, End::Target));
            }
            if c.routing == Routing::Orthogonal {
                let n = g.points.len();
                for k in 1..n.saturating_sub(2) {
                    if near(at(g.points[k].midpoint(g.points[k + 1]))) {
                        return Some(Grab::Segment(id, k));
                    }
                }
            } else if let Some(pts) = self.control_points(id, c) {
                for (k, w) in c.waypoints.iter().enumerate() {
                    if near(at(*w)) {
                        return Some(Grab::Waypoint(id, k));
                    }
                }
                for k in 0..pts.len() - 1 {
                    if near(at(pts[k].midpoint(pts[k + 1]))) {
                        return Some(Grab::NewWaypoint(id, k));
                    }
                }
            }
            if let Some(label) = g.label
                && self.view.rect_to_screen(origin, label).contains(pos)
            {
                return Some(Grab::Label(id));
            }
        }
        // Named table ports are usable while the table is selected. They
        // take precedence over a coincident edge resize handle.
        if let Some(shape) = self.port_shape_at(self.view.to_page(origin, pos))
            && let Some(g) = self.scene.shape(shape)
            && let Some(port) = g
                .visible_ports()
                .filter(|p| p.id.column_id().is_some())
                .map(|port| ((at(port.at) - pos).length(), port))
                .filter(|(distance, _)| *distance <= PORT_SNAP * 0.6)
                .min_by(|a, b| a.0.total_cmp(&b.0))
                .map(|(_, port)| port)
        {
            return Some(Grab::Port(shape, port.id.clone()));
        }
        if let Some(bounds) = self.resize_bounds() {
            for handle in Handle::ALL {
                if near(at(handle.at(bounds))) {
                    return Some(Grab::Resize(handle));
                }
            }
        }
        for (id, port, arrow, _) in self.connect_arrows(origin) {
            if (arrow - pos).length() <= HANDLE + 2.0 {
                return Some(Grab::Port(id, port));
            }
        }
        // Judge by where the press began, not where the pointer is now:
        // a drag only starts after the pointer has moved a little.
        if let Some(shape) = self.port_shape_at(self.view.to_page(origin, pos))
            && !self.is_selected(shape)
            && let Some(g) = self.scene.shape(shape)
        {
            for port in g.visible_ports() {
                if (at(port.at) - pos).length() <= PORT_SNAP * 0.6 {
                    return Some(Grab::Port(shape, port.id.clone()));
                }
            }
        }
        None
    }

    // ----- Pointer --------------------------------------------------------

    fn pointer_input(&mut self, ctx: &egui::Context, response: &Response, origin: Pos2) {
        let space = ctx.input(|i| i.key_down(Key::Space));
        let shift = ctx.input(|i| i.modifiers.shift);

        if response.drag_started() {
            self.finish_text_edit(true);
            self.quick_insert = None;
            let start = ctx
                .input(|i| i.pointer.press_origin())
                .or(response.interact_pointer_pos());
            let primary = response.dragged_by(PointerButton::Primary);
            self.drag = match start {
                _ if !primary || space || self.tool == Tool::Pan => Drag::Pan,
                Some(start) => self.begin_drag(start, origin, shift),
                None => Drag::None,
            };
        }

        if response.dragged()
            && let Some(pos) = response.interact_pointer_pos()
        {
            let p = self.page_at(origin, pos);
            self.continue_drag(ctx, response, p);
        }

        if response.drag_stopped() {
            self.end_drag();
        }

        if response.clicked()
            && let Some(pos) = response.interact_pointer_pos()
        {
            self.finish_text_edit(true);
            self.quick_insert = None;
            let p = self.page_at(origin, pos);
            self.click(p, shift);
        }

        if response.double_clicked()
            && self.tool == Tool::Select
            && let Some(pos) = response.interact_pointer_pos()
        {
            let p = self.page_at(origin, pos);
            self.double_click(p, pos, origin);
        }
    }

    fn click(&mut self, p: Point, shift: bool) {
        match self.tool.clone() {
            Tool::Shape(shape) => {
                self.insert_shape_at(shape, p);
                self.tool = Tool::Select;
            }
            Tool::Text => {
                if let Some(id) = self.insert_shape_at(ShapeRef::new("basic", "text"), p) {
                    self.start_text_edit(id);
                }
                self.tool = Tool::Select;
            }
            Tool::Connector | Tool::Pan => {}
            Tool::Select => match self.hit(p) {
                Some(hit) => {
                    self.leave_scope_unless_inside(hit);
                    let target = self.selectable(hit);
                    if shift {
                        self.toggle_selected(target);
                    } else {
                        self.select_only(target);
                    }
                }
                None => {
                    if !shift {
                        self.selection.clear();
                    }
                    if self
                        .scope
                        .is_some_and(|s| !self.scene.bounds_of(s).is_some_and(|b| b.contains(p)))
                    {
                        self.scope = None;
                    }
                }
            },
        }
    }

    fn double_click(&mut self, p: Point, pos: Pos2, origin: Pos2) {
        // Double-clicking a waypoint removes it.
        if let Some(Grab::Waypoint(id, k)) = self.grab_at(pos, origin)
            && let Some(c) = self.doc.elements.get(&id).and_then(|e| e.as_connector())
        {
            let mut waypoints = c.waypoints.clone();
            waypoints.remove(k);
            self.apply(
                "Remove waypoint",
                [Command::Set {
                    id,
                    prop: Prop::Waypoints(waypoints),
                }],
            );
            return;
        }
        match self.hit(p) {
            Some(hit) => {
                // Enter any groups down to the shape, then edit its text.
                while let Some(group) = self.group_to_enter(hit) {
                    self.scope = Some(group);
                }
                let target = self.selectable(hit);
                self.select_only(target);
                let has_text = self
                    .doc
                    .elements
                    .get(&target)
                    .is_some_and(|e| e.text().is_some());
                if has_text {
                    self.start_text_edit(target);
                }
            }
            None => {
                if let Some(id) = self.insert_shape_at(ShapeRef::new("basic", "text"), p) {
                    self.start_text_edit(id);
                }
            }
        }
    }

    /// Starts a primary-button drag at screen position `start`.
    fn begin_drag(&mut self, start: Pos2, origin: Pos2, shift: bool) -> Drag {
        let p = self.page_at(origin, start);
        match self.tool.clone() {
            Tool::Pan => Drag::Pan,
            Tool::Shape(shape) => Drag::Create {
                shape,
                start: self.snap_to_grid(p),
                current: self.snap_to_grid(p),
            },
            Tool::Text => Drag::Create {
                shape: ShapeRef::new("basic", "text"),
                start: self.snap_to_grid(p),
                current: self.snap_to_grid(p),
            },
            Tool::Connector => {
                let (source, _) = self.connect_target(p, None);
                Drag::Connect {
                    source,
                    target: Endpoint::Free(p),
                }
            }
            Tool::Select => {
                if let Some(grab) = self.grab_at(start, origin) {
                    return self.begin_grab(grab, p);
                }
                match self.hit(p) {
                    Some(hit) => {
                        self.leave_scope_unless_inside(hit);
                        let target = self.selectable(hit);
                        if !self.is_selected(target) {
                            if shift {
                                self.toggle_selected(target);
                            } else {
                                self.select_only(target);
                            }
                        }
                        let ids = self.editable_selection();
                        if ids.is_empty() {
                            return Drag::None;
                        }
                        let bounds = edit::top_level(&self.doc, &ids)
                            .iter()
                            .filter_map(|id| self.scene.bounds_of(*id))
                            .reduce(|a, b| a.union(b));
                        self.history.begin("Move");
                        Drag::Move {
                            ids,
                            start: p,
                            bounds,
                            snapshot: Box::new(self.doc.clone()),
                        }
                    }
                    None => {
                        if !shift {
                            self.selection.clear();
                        }
                        Drag::Marquee {
                            start: p,
                            current: p,
                            additive: shift,
                        }
                    }
                }
            }
        }
    }

    fn begin_grab(&mut self, grab: Grab, p: Point) -> Drag {
        match grab {
            Grab::Resize(handle) => {
                let Some(start_bounds) = self.resize_bounds() else {
                    return Drag::None;
                };
                self.history.begin("Resize");
                Drag::Resize {
                    ids: self.editable_selection(),
                    handle,
                    start_bounds,
                    snapshot: Box::new(self.doc.clone()),
                }
            }
            Grab::ConnectorEnd(id, end) => {
                self.history.begin("Reconnect");
                Drag::MoveEnd { id, end }
            }
            Grab::Waypoint(id, index) => {
                self.history.begin("Move waypoint");
                Drag::MoveWaypoint { id, index }
            }
            Grab::NewWaypoint(id, index) => {
                let Some(c) = self.doc.elements.get(&id).and_then(|e| e.as_connector()) else {
                    return Drag::None;
                };
                let mut waypoints = c.waypoints.clone();
                waypoints.insert(index, p);
                self.history.begin("Add waypoint");
                self.apply(
                    "",
                    [Command::Set {
                        id,
                        prop: Prop::Waypoints(waypoints),
                    }],
                );
                Drag::MoveWaypoint { id, index }
            }
            Grab::Segment(id, k) => {
                // Pin the current route as waypoints, then move the two
                // corners of segment k together.
                let Some(g) = self.scene.connector(id) else {
                    return Drag::None;
                };
                let interior: Vec<Point> = g.points[1..g.points.len() - 1].to_vec();
                let (a, b) = (g.points[k], g.points[k + 1]);
                let horizontal = (a.y - b.y).abs() < 1e-6;
                self.history.begin("Move segment");
                self.apply(
                    "",
                    [Command::Set {
                        id,
                        prop: Prop::Waypoints(interior.clone()),
                    }],
                );
                Drag::MoveSegment {
                    id,
                    corners: [k - 1, k],
                    horizontal,
                    start: p,
                    original: interior,
                }
            }
            Grab::Label(id) => {
                self.history.begin("Move label");
                Drag::MoveLabel { id }
            }
            Grab::Port(shape, port) => Drag::Connect {
                source: Endpoint::Glued {
                    element: shape,
                    port: Some(port),
                },
                target: Endpoint::Free(p),
            },
        }
    }

    /// What a connector end dropped at `p` attaches to: a port within
    /// reach, else the shape under the pointer (floating), else nothing.
    /// Returns the endpoint and the shape it attaches to.
    fn connect_target(
        &self,
        p: Point,
        exclude: Option<ElementId>,
    ) -> (Endpoint, Option<ElementId>) {
        let ok =
            |id: ElementId| Some(id) != exclude && self.is_hittable(id) && !self.doc.is_locked(id);
        if let Some((id, port)) = self.scene.port_near(p, self.units(PORT_SNAP), ok) {
            return (
                Endpoint::Glued {
                    element: id,
                    port: Some(port.id),
                },
                Some(id),
            );
        }
        let shape = self.scene.hit(p, self.units(HIT), |id| {
            ok(id) && self.doc.elements.get(&id).is_some_and(|e| e.is_shape())
        });
        match shape {
            Some(id) => (
                Endpoint::Glued {
                    element: id,
                    port: None,
                },
                Some(id),
            ),
            None => (Endpoint::Free(self.snap_to_grid(p)), None),
        }
    }

    fn continue_drag(&mut self, ctx: &egui::Context, response: &Response, p: Point) {
        let no_snap = Self::snap_disabled(ctx);
        let tolerance = self.units(SNAP);
        let gridded = if no_snap { p } else { self.snap_to_grid(p) };
        match &mut self.drag {
            Drag::None => {}
            Drag::Pan => self.view.pan += response.drag_delta(),
            Drag::Marquee { current, .. } => *current = p,
            Drag::Create { current, .. } => *current = gridded,
            Drag::Move { .. } => self.drag_move(p, no_snap, tolerance),
            Drag::Resize { .. } => self.drag_resize(ctx, p, no_snap, tolerance),
            Drag::Connect { source, .. } => {
                let exclude = match source {
                    Endpoint::Glued {
                        port: Some(port), ..
                    } if port.column_id().is_some() => None,
                    _ => source.element(),
                };
                let (end, _) = self.connect_target(p, exclude);
                if let Drag::Connect { target, .. } = &mut self.drag {
                    *target = end;
                }
            }
            Drag::MoveEnd { id, end } => {
                let (id, end) = (*id, *end);
                self.drag_connector_end(id, end, p);
            }
            Drag::MoveWaypoint { id, index } => {
                let (id, index) = (*id, *index);
                let p = self.snap_point_to_targets(p, &[id], no_snap, tolerance);
                if let Some(c) = self.doc.elements.get(&id).and_then(|e| e.as_connector())
                    && index < c.waypoints.len()
                {
                    let mut waypoints = c.waypoints.clone();
                    waypoints[index] = p;
                    self.apply(
                        "",
                        [Command::Set {
                            id,
                            prop: Prop::Waypoints(waypoints),
                        }],
                    );
                }
            }
            Drag::MoveSegment { .. } => self.drag_segment(p, no_snap),
            Drag::MoveLabel { id } => {
                let id = *id;
                if let Some(g) = self.scene.connector(id) {
                    let t = nearest_fraction(&g.points, p);
                    self.apply(
                        "",
                        [Command::Set {
                            id,
                            prop: Prop::LabelPosition(t),
                        }],
                    );
                }
            }
        }
    }

    fn drag_connector_end(&mut self, id: ElementId, end: End, p: Point) {
        let Some(c) = self.doc.elements.get(&id).and_then(|e| e.as_connector()) else {
            return;
        };
        let other = match end {
            End::Source => &c.target,
            End::Target => &c.source,
        };
        let other = match other {
            Endpoint::Glued {
                port: Some(port), ..
            } if port.column_id().is_some() => None,
            _ => other.element(),
        };
        let (endpoint, _) = self.connect_target(p, other);
        let prop = match end {
            End::Source => Prop::Source(endpoint),
            End::Target => Prop::Target(endpoint),
        };
        self.apply("", [Command::Set { id, prop }]);
    }

    fn drag_segment(&mut self, p: Point, no_snap: bool) {
        let Drag::MoveSegment {
            id,
            corners,
            horizontal,
            start,
            original,
        } = &self.drag
        else {
            return;
        };
        let (id, horizontal) = (*id, *horizontal);
        let delta = p - *start;
        let grid = (!no_snap).then(|| self.grid_step()).flatten();
        let mut waypoints = original.clone();
        for &k in corners {
            if let Some(w) = waypoints.get_mut(k) {
                let v = if horizontal { &mut w.y } else { &mut w.x };
                *v += if horizontal { delta.y } else { delta.x };
                if let Some(g) = grid {
                    *v = (*v / g).round() * g;
                }
            }
        }
        self.apply(
            "",
            [Command::Set {
                id,
                prop: Prop::Waypoints(waypoints),
            }],
        );
    }

    fn snap_point_to_targets(
        &mut self,
        p: Point,
        moving: &[ElementId],
        no_snap: bool,
        tolerance: f64,
    ) -> Point {
        if no_snap {
            self.guides.clear();
            return p;
        }
        let targets = self.snap_targets(moving, Rect::from_points(p, p));
        let result = snap_point(p, &targets, self.grid_step(), tolerance);
        self.guides = result.guides;
        p + result.delta
    }

    fn drag_move(&mut self, p: Point, no_snap: bool, tolerance: f64) {
        let Drag::Move {
            ids,
            start,
            bounds,
            snapshot,
        } = &self.drag
        else {
            return;
        };
        let mut delta = p - *start;
        let mut guides = Vec::new();
        if let Some(b) = *bounds
            && !no_snap
        {
            let targets = self.snap_targets(ids, b + delta);
            let result = snap_rect(
                b + delta,
                Features::ALL,
                &targets,
                self.grid_step(),
                tolerance,
            );
            delta += result.delta;
            guides = result.guides;
        }
        // Always relative to the snapshot, so moving back to the start
        // restores every value exactly.
        let commands = edit::transform(snapshot, ids, Affine::translate(delta));
        self.guides = guides;
        self.apply("", commands);
    }

    fn drag_resize(&mut self, ctx: &egui::Context, p: Point, no_snap: bool, tolerance: f64) {
        let Drag::Resize {
            ids,
            handle,
            start_bounds,
            snapshot,
        } = &self.drag
        else {
            return;
        };
        let (keep_aspect, from_center) = ctx.input(|i| (i.modifiers.shift, i.modifiers.alt));
        let cloud = ids
            .iter()
            .copied()
            .flat_map(|id| std::iter::once(id).chain(snapshot.descendants(id)))
            .any(|id| {
                snapshot
                    .elements
                    .get(&id)
                    .and_then(bp_model::Element::as_shape)
                    .is_some_and(|shape| shape.shape.is_cloud())
            });
        let (fx, fy) = handle.anchor();
        let sb = *start_bounds;
        let moves_x = fx != 0.5;
        let moves_y = fy != 0.5;
        // Snap the dragged edges.
        let mut p = p;
        if !no_snap && !from_center {
            let features = Features {
                x: [moves_x && fx == 0.0, false, moves_x && fx == 1.0],
                y: [moves_y && fy == 0.0, false, moves_y && fy == 1.0],
            };
            let probe = Rect::from_points(p, p);
            let targets = self.snap_targets(ids, sb);
            let result = snap_rect(probe, features, &targets, self.grid_step(), tolerance);
            p += result.delta;
            self.guides = result.guides;
        } else {
            self.guides.clear();
        }
        let min = 1.0;
        let center = sb.center();
        let anchor = if from_center {
            center
        } else {
            handle_opposite(*handle).at(sb)
        };
        let mut x0 = sb.x0;
        let mut x1 = sb.x1;
        let mut y0 = sb.y0;
        let mut y1 = sb.y1;
        if moves_x {
            let half = (p.x - anchor.x).abs().max(min / 2.0);
            if from_center {
                x0 = center.x - half;
                x1 = center.x + half;
            } else if fx == 1.0 {
                x0 = anchor.x;
                x1 = p.x.max(anchor.x + min);
            } else {
                x1 = anchor.x;
                x0 = p.x.min(anchor.x - min);
            }
        }
        if moves_y {
            let half = (p.y - anchor.y).abs().max(min / 2.0);
            if from_center {
                y0 = center.y - half;
                y1 = center.y + half;
            } else if fy == 1.0 {
                y0 = anchor.y;
                y1 = p.y.max(anchor.y + min);
            } else {
                y1 = anchor.y;
                y0 = p.y.min(anchor.y - min);
            }
        }
        let mut new = Rect::new(x0, y0, x1, y1);
        if (keep_aspect || cloud) && (cloud || (moves_x && moves_y)) && sb.height() > 0.0 {
            let aspect = sb.width() / sb.height();
            let (w, h) = if !moves_y {
                (new.width(), new.width() / aspect)
            } else if !moves_x || new.width() / new.height() > aspect {
                (new.height() * aspect, new.height())
            } else {
                (new.width(), new.width() / aspect)
            };
            new = if from_center {
                Rect::from_center_size(center, (w, h))
            } else {
                let x0 = if !moves_x {
                    center.x - w / 2.0
                } else if fx == 1.0 {
                    anchor.x
                } else {
                    anchor.x - w
                };
                let y0 = if !moves_y {
                    center.y - h / 2.0
                } else if fy == 1.0 {
                    anchor.y
                } else {
                    anchor.y - h
                };
                Rect::new(x0, y0, x0 + w, y0 + h)
            };
        }
        let commands = edit::transform(snapshot, ids, rect_to_rect(sb, new));
        self.apply("", commands);
    }

    fn end_drag(&mut self) {
        let drag = std::mem::take(&mut self.drag);
        self.guides.clear();
        match drag {
            Drag::Move { .. }
            | Drag::Resize { .. }
            | Drag::MoveEnd { .. }
            | Drag::MoveWaypoint { .. }
            | Drag::MoveSegment { .. }
            | Drag::MoveLabel { .. } => self.history.commit(),
            Drag::Marquee {
                start,
                current,
                additive,
            } => {
                let rect = Rect::from_points(start, current);
                let mut picked: Vec<ElementId> = Vec::new();
                for hit in self.scene.enclosed(rect) {
                    if !self.is_hittable(hit) || !self.inside_scope(hit) {
                        continue;
                    }
                    let target = self.selectable(hit);
                    // A group joins only when all of it is inside.
                    let inside = self
                        .scene
                        .bounds_of(target)
                        .is_some_and(|b| rect.contains_rect(b));
                    if inside && !picked.contains(&target) {
                        picked.push(target);
                    }
                }
                if additive {
                    for id in picked {
                        if !self.is_selected(id) {
                            self.toggle_selected(id);
                        }
                    }
                } else {
                    self.selection = picked;
                }
            }
            Drag::Create {
                shape,
                start,
                current,
            } => {
                let bounds = Rect::from_points(start, current);
                let is_text = shape == ShapeRef::new("basic", "text");
                let id = if bounds.width() < 4.0 && bounds.height() < 4.0 {
                    self.insert_shape_at(shape, start)
                } else {
                    self.insert_shape(shape, bounds)
                };
                if is_text && let Some(id) = id {
                    self.start_text_edit(id);
                }
                self.tool = Tool::Select;
            }
            Drag::Connect { source, target } => {
                let distinct_rows = match (&source, &target) {
                    (
                        Endpoint::Glued { port: Some(a), .. },
                        Endpoint::Glued { port: Some(b), .. },
                    ) => {
                        a.column_id().is_some()
                            && b.column_id().is_some()
                            && a.column_id() != b.column_id()
                    }
                    _ => false,
                };
                let same_shape = source.element().is_some()
                    && source.element() == target.element()
                    && !distinct_rows;
                let too_short = match (&source, &target) {
                    (Endpoint::Free(a), Endpoint::Free(b)) => (*a - *b).hypot() < self.units(6.0),
                    _ => false,
                };
                if !same_shape && !too_short {
                    self.insert_connector(source, target);
                }
            }
            Drag::Pan | Drag::None => {}
        }
    }

    /// Inserts a shape dragged in from the palette where it is dropped.
    fn palette_drop(&mut self, ctx: &egui::Context, response: &Response, origin: Pos2) {
        let Some(shape) = self.palette.dragging.clone() else {
            return;
        };
        let (released, pos) = ctx.input(|i| (i.pointer.any_released(), i.pointer.interact_pos()));
        if !released {
            return;
        }
        self.palette.dragging = None;
        if let Some(pos) = pos
            && response.rect.contains(pos)
            && ctx.layer_id_at(pos) == Some(response.layer_id)
        {
            self.insert_shape_at(shape, self.page_at(origin, pos));
            self.tool = Tool::Select;
        }
    }

    // ----- Painting -------------------------------------------------------

    fn paint_overlays(&self, painter: &egui::Painter, origin: Pos2, response: &Response) {
        let at = |p: Point| self.view.to_screen(origin, p);
        let accent = Stroke::new(1.5, ACCENT);

        // The group being edited.
        if let Some(bounds) = self.scope.and_then(|s| self.scene.bounds_of(s)) {
            let r = self.view.rect_to_screen(origin, bounds).expand(6.0);
            dashed_rect(painter, r, Stroke::new(1.0, Color32::from_gray(150)));
        }

        // Hover highlight.
        if !self.drag.is_active()
            && response.hovered()
            && self.tool == Tool::Select
            && let Some(hit) = self.pointer.and_then(|p| self.hit(p))
        {
            let target = self.selectable(hit);
            if !self.is_selected(target)
                && let Some(b) = self.scene.bounds_of(target)
            {
                painter.rect_stroke(
                    self.view.rect_to_screen(origin, b).expand(2.0),
                    2.0,
                    Stroke::new(1.0, ACCENT.gamma_multiply(0.5)),
                    StrokeKind::Outside,
                );
            }
        }

        // Selection outlines.
        for id in &self.selection {
            let locked = self.doc.is_locked(*id);
            match self.scene.geometry(*id) {
                Some(Geometry::Shape(g)) => {
                    let r = self.view.rect_to_screen(origin, g.bounds);
                    if locked {
                        dashed_rect(painter, r.expand(1.0), accent);
                    } else {
                        painter.rect_stroke(r, 0.0, accent, StrokeKind::Outside);
                    }
                }
                Some(Geometry::Group { bounds }) => {
                    dashed_rect(
                        painter,
                        self.view.rect_to_screen(origin, *bounds).expand(2.0),
                        accent,
                    );
                }
                Some(Geometry::Connector(g)) => {
                    let pts: Vec<Pos2> = g.points.iter().map(|p| at(*p)).collect();
                    painter.add(Shape::line(
                        pts,
                        Stroke::new(3.0, ACCENT.gamma_multiply(0.35)),
                    ));
                }
                None => {}
            }
        }

        // Connector handles.
        if let Some((id, c)) = self.selected_connector()
            && !self.doc.is_locked(id)
            && let Some(g) = self.scene.connector(id)
        {
            for (end, point) in [(&c.source, g.points.first()), (&c.target, g.points.last())] {
                if let Some(p) = point {
                    let glued = end.element().is_some();
                    let fill = if glued { PORT } else { Color32::WHITE };
                    painter.circle(at(*p), 5.0, fill, Stroke::new(1.5, ACCENT));
                }
            }
            if c.routing == Routing::Orthogonal {
                let n = g.points.len();
                for k in 1..n.saturating_sub(2) {
                    let m = at(g.points[k].midpoint(g.points[k + 1]));
                    let horizontal = (g.points[k].y - g.points[k + 1].y).abs() < 1e-6;
                    let size = if horizontal {
                        ScreenVec::new(12.0, 6.0)
                    } else {
                        ScreenVec::new(6.0, 12.0)
                    };
                    let r = egui::Rect::from_center_size(m, size);
                    painter.rect_filled(r, 3.0, Color32::WHITE);
                    painter.rect_stroke(r, 3.0, accent, StrokeKind::Inside);
                }
            } else if let Some(pts) = self.control_points(id, c) {
                for w in &c.waypoints {
                    let r = egui::Rect::from_center_size(at(*w), ScreenVec::splat(HANDLE));
                    painter.rect_filled(r, 2.0, Color32::WHITE);
                    painter.rect_stroke(r, 2.0, accent, StrokeKind::Inside);
                }
                for k in 0..pts.len() - 1 {
                    let m = at(pts[k].midpoint(pts[k + 1]));
                    painter.circle(
                        m,
                        3.5,
                        Color32::WHITE,
                        Stroke::new(1.0, ACCENT.gamma_multiply(0.7)),
                    );
                }
            }
        }

        // Resize handles.
        if let Some(bounds) = self.resize_bounds() {
            if self.selection.len() > 1 {
                painter.rect_stroke(
                    self.view.rect_to_screen(origin, bounds),
                    0.0,
                    Stroke::new(1.0, ACCENT.gamma_multiply(0.6)),
                    StrokeKind::Outside,
                );
            }
            for handle in Handle::ALL {
                let r =
                    egui::Rect::from_center_size(at(handle.at(bounds)), ScreenVec::splat(HANDLE));
                painter.rect_filled(r, 2.0, Color32::WHITE);
                painter.rect_stroke(r, 2.0, accent, StrokeKind::Inside);
            }
        }

        // Connect arrows beside a selected shape.
        for (_, _, pos, dir) in self.connect_arrows(origin) {
            paint_arrow(painter, pos, dir);
        }

        // Ports of the hovered shape, or the target while connecting.
        let port_shape = match &self.drag {
            Drag::Connect { target, .. } => target.element(),
            Drag::MoveEnd { id, end } => self
                .doc
                .elements
                .get(id)
                .and_then(|e| e.as_connector())
                .and_then(|c| match end {
                    End::Source => c.source.element(),
                    End::Target => c.target.element(),
                }),
            _ => self.hovered_shape().filter(|s| {
                !self.is_selected(*s) || self.scene.shape(*s).is_some_and(|g| g.erd.is_some())
            }),
        };
        if let Some(g) = port_shape.and_then(|s| self.scene.shape(s)) {
            if self.drag.is_active() {
                painter.rect_stroke(
                    self.view.rect_to_screen(origin, g.bounds).expand(3.0),
                    3.0,
                    Stroke::new(2.0, PORT),
                    StrokeKind::Outside,
                );
            }
            for port in g.visible_ports().filter(|p| {
                self.drag.is_active()
                    || port_shape.is_none_or(|id| !self.is_selected(id))
                    || p.id.column_id().is_some()
            }) {
                painter.circle(at(port.at), 4.0, Color32::WHITE, Stroke::new(1.5, PORT));
            }
        }

        // Marquee.
        if let Drag::Marquee { start, current, .. } = &self.drag {
            let r = egui::Rect::from_two_pos(at(*start), at(*current));
            painter.rect_filled(r, 0.0, ACCENT.gamma_multiply(0.08));
            painter.rect_stroke(r, 0.0, Stroke::new(1.0, ACCENT), StrokeKind::Inside);
        }

        // Smart guides.
        for guide in &self.guides {
            let (a, b) = match guide.axis {
                Axis::X => (
                    Point::new(guide.at, guide.from),
                    Point::new(guide.at, guide.to),
                ),
                Axis::Y => (
                    Point::new(guide.from, guide.at),
                    Point::new(guide.to, guide.at),
                ),
            };
            painter.line_segment([at(a), at(b)], Stroke::new(1.0, GUIDE));
        }

        // Previews.
        match &self.drag {
            Drag::Create {
                shape,
                start,
                current,
            } => {
                let def = self.libraries.resolve(shape);
                let rect = Rect::from_points(*start, *current);
                let path = def.outline(rect, def.default_style().corner_radius);
                let preview = preview_list(vec![DisplayItem {
                    element: ElementId::new(),
                    bbox: rect,
                    primitive: bp_scene::Primitive::Path {
                        path,
                        fill: Some(bp_model::Color::rgba(37, 99, 235, 24)),
                        stroke: Some(bp_scene::Stroke {
                            color: bp_model::Color::rgb(37, 99, 235),
                            width: 1.5 / f64::from(self.view.zoom),
                            dash: None,
                        }),
                    },
                }]);
                paint(painter, origin, &self.view, &preview, None);
            }
            Drag::Connect { source, target } => {
                let mut c = Connector::new(source.clone(), target.clone());
                self.configure_connection(&mut c);
                c.style.stroke = Some(Paint::Color(bp_model::Color::rgb(37, 99, 235)));
                let items = bp_scene::connector_preview(&self.doc, self.page, self.libraries, &c);
                paint(painter, origin, &self.view, &preview_list(items), None);
            }
            _ => {}
        }
    }

    fn update_cursor(&self, ctx: &egui::Context, response: &Response, origin: Pos2) {
        let space = ctx.input(|i| i.key_down(Key::Space));
        let icon = match &self.drag {
            Drag::Pan => CursorIcon::Grabbing,
            Drag::Move { .. } => CursorIcon::Move,
            Drag::Resize { handle, .. } => handle.cursor(),
            Drag::Create { .. } | Drag::Connect { .. } | Drag::MoveEnd { .. } => {
                CursorIcon::Crosshair
            }
            Drag::MoveWaypoint { .. } | Drag::MoveLabel { .. } => CursorIcon::Grabbing,
            Drag::MoveSegment { horizontal, .. } => {
                if *horizontal {
                    CursorIcon::ResizeVertical
                } else {
                    CursorIcon::ResizeHorizontal
                }
            }
            Drag::Marquee { .. } => CursorIcon::Default,
            Drag::None if !response.hovered() => return,
            Drag::None if space || self.tool == Tool::Pan => CursorIcon::Grab,
            Drag::None => match self.tool {
                Tool::Shape(_) | Tool::Connector => CursorIcon::Crosshair,
                Tool::Text => CursorIcon::Text,
                _ => {
                    let hover = response.hover_pos();
                    match hover.and_then(|p| self.grab_at(p, origin)) {
                        Some(Grab::Resize(h)) => h.cursor(),
                        Some(Grab::Port(..)) | Some(Grab::ConnectorEnd(..)) => {
                            CursorIcon::Crosshair
                        }
                        Some(Grab::Segment(..))
                        | Some(Grab::Waypoint(..))
                        | Some(Grab::NewWaypoint(..))
                        | Some(Grab::Label(..)) => CursorIcon::Grab,
                        None if hover
                            .is_some_and(|p| self.hit(self.page_at(origin, p)).is_some()) =>
                        {
                            CursorIcon::Move
                        }
                        None => CursorIcon::Default,
                    }
                }
            },
        };
        ctx.set_cursor_icon(icon);
    }

    // ----- Text editing -----------------------------------------------------

    /// Draws the in-place text editor over the element being edited.
    pub fn text_editor(&mut self, ctx: &egui::Context) {
        let Some(id) = self.editing.as_ref().map(|e| e.id) else {
            return;
        };
        let origin = self.canvas_rect.min;
        let (area, size, align, face, color) = match self.scene.geometry(id) {
            Some(Geometry::Shape(g)) => (
                g.text_box,
                g.style.font_size,
                g.style.text_align,
                bp_text::Face::new(g.style.bold, g.style.italic),
                g.style.text_color,
            ),
            Some(Geometry::Connector(g)) => (
                Rect::from_center_size(
                    g.label_anchor,
                    (bp_scene::LABEL_WIDTH, g.style.font_size * 1.5),
                ),
                g.style.font_size,
                TextAlign::Center,
                bp_text::Face::new(g.style.bold, g.style.italic),
                g.style.text_color,
            ),
            _ => {
                self.editing = None;
                return;
            }
        };
        let screen = self.view.rect_to_screen(origin, area);
        let font_size = (size as f32 * self.view.zoom).clamp(8.0, 64.0);
        let width = screen.width().max(120.0);
        let lines = self
            .editing
            .as_ref()
            .map_or(1, |e| e.text.lines().count().max(1)) as f32;
        let height = font_size * 1.3 * lines;
        let top_left = Pos2::new(
            screen.center().x - width / 2.0,
            screen.center().y - height / 2.0 - 4.0,
        );
        let halign = match align {
            TextAlign::Left => egui::Align::Min,
            TextAlign::Center => egui::Align::Center,
            TextAlign::Right => egui::Align::Max,
        };

        let Some(edit) = self.editing.as_mut() else {
            return;
        };
        let mut finish = None;
        egui::Area::new(egui::Id::new("bp-text-editor"))
            .order(egui::Order::Foreground)
            .fixed_pos(top_left)
            .show(ctx, |ui| {
                let frame = egui::Frame::new()
                    .fill(Color32::from_white_alpha(235))
                    .stroke(Stroke::new(1.0, ACCENT))
                    .corner_radius(3.0)
                    .inner_margin(2.0);
                frame.show(ui, |ui| {
                    let output = egui::TextEdit::multiline(&mut edit.text)
                        .font(text_font(ui.ctx(), face, font_size))
                        .text_color(color32(color))
                        .horizontal_align(halign)
                        .desired_width(width)
                        .desired_rows(1)
                        .frame(egui::Frame::NONE)
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
            });
        if let Some(keep) = finish {
            self.finish_text_edit(keep);
        }
    }
}

fn handle_opposite(h: Handle) -> Handle {
    match h {
        Handle::N => Handle::S,
        Handle::NE => Handle::SW,
        Handle::E => Handle::W,
        Handle::SE => Handle::NW,
        Handle::S => Handle::N,
        Handle::SW => Handle::NE,
        Handle::W => Handle::E,
        Handle::NW => Handle::SE,
    }
}

/// The fraction along `points` nearest to `p`.
fn nearest_fraction(points: &[Point], p: Point) -> f64 {
    let total = polyline_length(points);
    if total == 0.0 {
        return 0.5;
    }
    let mut best = (f64::INFINITY, 0.5);
    let mut walked = 0.0;
    for w in points.windows(2) {
        let seg = w[1] - w[0];
        let len = seg.hypot();
        if len == 0.0 {
            continue;
        }
        let t = ((p - w[0]).dot(seg) / (len * len)).clamp(0.0, 1.0);
        let d = (w[0] + seg * t - p).hypot();
        if d < best.0 {
            best = (d, (walked + t * len) / total);
        }
        walked += len;
    }
    best.1
}

fn preview_list(items: Vec<DisplayItem>) -> DisplayList {
    DisplayList {
        groups: vec![Arc::from(items)],
        background: None,
    }
}

fn dashed_rect(painter: &egui::Painter, r: egui::Rect, stroke: Stroke) {
    let corners = [
        r.left_top(),
        r.right_top(),
        r.right_bottom(),
        r.left_bottom(),
        r.left_top(),
    ];
    painter.extend(Shape::dashed_line(&corners, stroke, 4.0, 3.0));
}

/// A small arrow button pointing `dir`, for drawing a connector out of a
/// selected shape.
fn paint_arrow(painter: &egui::Painter, at: Pos2, dir: Dir) {
    let v = dir.vec();
    let d = ScreenVec::new(v.x as f32, v.y as f32);
    let n = ScreenVec::new(-d.y, d.x);
    let tip = at + d * 6.0;
    let base = at - d * 4.0;
    painter.circle_filled(at, 9.0, Color32::from_white_alpha(230));
    painter.add(Shape::convex_polygon(
        vec![tip, base + n * 5.0, base - n * 5.0],
        PORT,
        Stroke::NONE,
    ));
}
