//! The shape palette (left panel) and quick insert (`/` on the canvas).

use crate::app::BlueprintApp;
use crate::theme::ACCENT;
use bp_model::kurbo::{Point, Rect};
use bp_model::{CloudIcon, DiagramKind, ShapeRef, StyleValues};
use bp_render_egui::{Viewport, paint};
use bp_scene::{DisplayItem, DisplayList, Primitive, Stroke as SceneStroke};
use bp_shapes::{Libraries, ShapeDef};
use egui::{Color32, Key, Pos2, RichText, Sense, Stroke, Ui, Vec2};
use egui_phosphor::regular as icon;
use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};
use std::collections::HashSet;
use std::sync::Arc;

/// Size of a palette cell, in screen points.
const CELL: Vec2 = Vec2::new(50.0, 44.0);
const RECENT_LIMIT: usize = 8;

#[derive(Default)]
pub struct PaletteState {
    pub query: String,
    /// Recently inserted shapes, most recent first.
    pub recent: Vec<ShapeRef>,
    /// Libraries the user folded away.
    pub collapsed: HashSet<String>,
    /// A shape being dragged from the palette onto the canvas.
    pub dragging: Option<ShapeRef>,
}

impl PaletteState {
    pub fn note_used(&mut self, shape: &ShapeRef) {
        self.recent.retain(|r| r != shape);
        self.recent.insert(0, shape.clone());
        self.recent.truncate(RECENT_LIMIT);
    }
}

/// The `/` popup: search and insert at the pointer.
pub struct QuickInsert {
    pub query: String,
    /// Where the shape goes, on the page.
    pub at: Point,
    pub selected: usize,
    pub focused: bool,
}

enum QuickChoice<'a> {
    Shape(&'a ShapeDef),
    Icon(&'a CloudIcon),
}

impl QuickChoice<'_> {
    fn name(&self) -> &str {
        match self {
            Self::Shape(def) => &def.name,
            Self::Icon(icon) => &icon.name,
        }
    }
    fn reference(&self) -> &ShapeRef {
        match self {
            Self::Shape(def) => &def.reference,
            Self::Icon(icon) => &icon.reference,
        }
    }
}

/// Shapes matching `query` (names, keywords and library names), best
/// match first.
pub fn search<'a>(libraries: &'a Libraries, query: &str) -> Vec<&'a ShapeDef> {
    let query = query.trim().trim_start_matches('/');
    if query.is_empty() {
        return libraries.shapes().collect();
    }
    let pattern = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);
    let mut matcher = Matcher::new(Config::DEFAULT);
    let mut buf = Vec::new();
    let mut scored: Vec<(u32, usize, &ShapeDef)> = libraries
        .libraries()
        .iter()
        .flat_map(|lib| lib.shapes.iter().map(move |s| (lib, s)))
        .enumerate()
        .filter_map(|(i, (lib, def))| {
            let name_score = pattern.score(Utf32Str::new(&def.name, &mut buf), &mut matcher);
            let haystack = format!("{} {} {}", def.name, def.keywords.join(" "), lib.name);
            let all_score = pattern.score(Utf32Str::new(&haystack, &mut buf), &mut matcher);
            // Name matches rank above keyword-only matches.
            let score = name_score.map(|s| s + 1000).max(all_score)?;
            Some((score, i, def))
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    scored.into_iter().map(|(_, _, def)| def).collect()
}

fn library_visible(kind: Option<DiagramKind>, library: &str) -> bool {
    match kind {
        Some(DiagramKind::Erd) => matches!(library, "basic" | "erd"),
        Some(DiagramKind::Flowchart) => matches!(library, "basic" | "flowchart"),
        Some(DiagramKind::Cloud) => library == "basic",
        None => false,
    }
}

/// Searches the shape libraries available on this kind of page.
pub fn search_for_kind<'a>(
    libraries: &'a Libraries,
    query: &str,
    kind: Option<DiagramKind>,
) -> Vec<&'a ShapeDef> {
    search(libraries, query)
        .into_iter()
        .filter(|def| library_visible(kind, def.reference.library()))
        .collect()
}

/// Draws `def` fitted into `rect` (screen points).
fn paint_thumbnail(painter: &egui::Painter, def: &ShapeDef, rect: egui::Rect, alpha: f32) {
    let (w, h) = def.default_size;
    let scale = (f64::from(rect.width()) / w).min(f64::from(rect.height()) / h);
    let size = (w * scale, h * scale);
    let center = Point::new(f64::from(rect.center().x), f64::from(rect.center().y));
    let bounds = Rect::from_center_size(center, size);
    let style: StyleValues = def.default_style();
    let fade = |c: bp_model::Color| c.faded(f64::from(alpha));
    let fill = style.fill.filter(|_| def.is_closed()).map(fade);
    let stroke = Some(SceneStroke {
        color: fade(style.stroke.unwrap_or(bp_model::Color::rgb(100, 116, 139))),
        width: 1.2,
        dash: style.dash.pattern(1.2),
    });
    let mut items = Vec::new();
    let mut push = |path, fill| {
        items.push(DisplayItem {
            element: bp_model::ElementId(Default::default()),
            bbox: bounds.inflate(2.0, 2.0),
            primitive: Primitive::Path { path, fill, stroke },
        });
    };
    for back in def.back(bounds) {
        push(back, fill);
    }
    push(def.outline(bounds, style.corner_radius * scale), fill);
    if let Some(details) = def.details(bounds) {
        push(details, None);
    }
    let list = DisplayList {
        groups: vec![Arc::from(items)],
        background: None,
    };
    let view = Viewport {
        pan: Vec2::ZERO,
        zoom: 1.0,
    };
    paint(painter, Pos2::ZERO, &view, &list, None);
}

impl BlueprintApp {
    pub fn palette(&mut self, ui: &mut Ui) {
        ui.add_space(6.0);
        let kind = self.page_kind();
        let Some(kind) = kind else {
            ui.label(RichText::new("Choose a diagram type to start").weak());
            return;
        };
        ui.label(RichText::new(kind.label()).strong().color(ACCENT));
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(crate::theme::icon(icon::MAGNIFYING_GLASS).color(Color32::from_gray(120)));
            ui.add(
                egui::TextEdit::singleline(&mut self.palette.query)
                    .hint_text(if kind == DiagramKind::Cloud {
                        "Search shapes and services"
                    } else {
                        "Search shapes"
                    })
                    .desired_width(f32::INFINITY),
            );
        });
        ui.add_space(4.0);
        let libraries = self.libraries;
        egui::ScrollArea::vertical()
            .auto_shrink(false)
            .show(ui, |ui| {
                if kind == DiagramKind::Cloud {
                    self.cloud_palette(ui);
                }
                if !self.palette.query.trim().is_empty() {
                    let results = search_for_kind(libraries, &self.palette.query, Some(kind));
                    if results.is_empty() && kind != DiagramKind::Cloud {
                        ui.label(RichText::new("No shapes match").weak());
                    }
                    self.shape_grid(ui, &results);
                    return;
                }
                let recent: Vec<&ShapeDef> = self
                    .palette
                    .recent
                    .iter()
                    .filter(|r| library_visible(Some(kind), r.library()))
                    .filter_map(|r| libraries.get(r))
                    .collect();
                if !recent.is_empty() {
                    ui.label(RichText::new("Recent").small().strong());
                    self.shape_grid(ui, &recent);
                    ui.add_space(4.0);
                }
                let specialized = match kind {
                    DiagramKind::Erd => "erd",
                    DiagramKind::Flowchart => "flowchart",
                    DiagramKind::Cloud => "basic",
                };
                let mut available: Vec<_> = libraries
                    .libraries()
                    .iter()
                    .filter(|lib| library_visible(Some(kind), &lib.id))
                    .collect();
                available.sort_by_key(|lib| lib.id != specialized);
                for lib in available {
                    let open = !self.palette.collapsed.contains(&lib.id);
                    let header = egui::CollapsingHeader::new(RichText::new(&lib.name).strong())
                        .id_salt(("palette-lib", &lib.id))
                        .open(Some(open))
                        .show(ui, |ui| {
                            let shapes: Vec<&ShapeDef> = lib.shapes.iter().collect();
                            self.shape_grid(ui, &shapes);
                            if lib.id == "erd" {
                                ui.add_space(8.0);
                                self.erd_connection_palette(ui);
                            }
                        });
                    if header.header_response.clicked() {
                        if open {
                            self.palette.collapsed.insert(lib.id.clone());
                        } else {
                            self.palette.collapsed.remove(&lib.id);
                        }
                    }
                }
                ui.add_space(8.0);
                ui.label(
                    RichText::new(
                        "Click to add, drag onto the canvas, or press / on the canvas to search.",
                    )
                    .small()
                    .weak(),
                );
            });
    }

    fn shape_grid(&mut self, ui: &mut Ui, shapes: &[&ShapeDef]) {
        let columns = ((ui.available_width() / CELL.x).floor() as usize).max(1);
        egui::Grid::new(ui.next_auto_id())
            .spacing(Vec2::splat(2.0))
            .show(ui, |ui| {
                for (i, def) in shapes.iter().enumerate() {
                    if i > 0 && i % columns == 0 {
                        ui.end_row();
                    }
                    self.shape_cell(ui, def);
                }
            });
    }

    fn shape_cell(&mut self, ui: &mut Ui, def: &ShapeDef) {
        let (rect, response) = ui.allocate_exact_size(CELL, Sense::click_and_drag());
        if response.hovered() {
            ui.painter()
                .rect_filled(rect, 4.0, ui.visuals().widgets.hovered.weak_bg_fill);
        }
        paint_thumbnail(ui.painter(), def, rect.shrink(8.0), 1.0);
        // Thumbnails have no text; name them for screen readers (and tests).
        response
            .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &def.name));
        let response = response.on_hover_text(&def.name);
        if response.clicked() {
            let at = self.view_center();
            self.insert_shape_at(def.reference.clone(), at);
        }
        if response.drag_started() {
            self.palette.dragging = Some(def.reference.clone());
        }
    }

    /// The shape following the pointer while dragging from the palette.
    pub fn palette_drag_preview(&mut self, ctx: &egui::Context) {
        let Some(shape) = &self.palette.dragging else {
            return;
        };
        if !ctx.input(|i| i.pointer.any_down()) && !ctx.input(|i| i.pointer.any_released()) {
            self.palette.dragging = None;
            return;
        }
        let Some(pos) = ctx.pointer_interact_pos() else {
            return;
        };
        let asset = self.cloud_icon(shape);
        let def = self.libraries.resolve(shape);
        let (w, h) = if asset.is_some() {
            (64.0, 64.0)
        } else {
            def.default_size
        };
        let zoom = self.view.zoom;
        let size = Vec2::new(w as f32 * zoom, h as f32 * zoom);
        let rect = egui::Rect::from_center_size(pos, size);
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Tooltip,
            egui::Id::new("palette-drag"),
        ));
        if let Some(asset) = asset {
            crate::cloud::paint_icon(&painter, asset, rect, 0.6);
        } else {
            paint_thumbnail(&painter, def, rect, 0.6);
        }
        ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
    }

    pub fn open_quick_insert(&mut self) {
        if self.page_kind().is_none() {
            return;
        }
        let at = self.pointer.unwrap_or_else(|| self.view_center());
        self.quick_insert = Some(QuickInsert {
            query: String::new(),
            at,
            selected: 0,
            focused: false,
        });
    }

    pub fn quick_insert_popup(&mut self, ctx: &egui::Context) {
        let kind = self.page_kind();
        let cloud: Vec<CloudIcon> = if kind == Some(DiagramKind::Cloud) {
            self.cloud_candidates(None, self.quick_insert.as_ref().map_or("", |qi| &qi.query))
        } else {
            Vec::new()
        };
        let Some(qi) = &mut self.quick_insert else {
            return;
        };
        let screen = self.view.to_screen(self.canvas_rect.min, qi.at);
        let results: Vec<QuickChoice<'_>> = cloud
            .iter()
            .map(QuickChoice::Icon)
            .chain(
                search_for_kind(self.libraries, &qi.query, kind)
                    .into_iter()
                    .map(QuickChoice::Shape),
            )
            .take(8)
            .collect();
        let (up, down, enter, escape) = ctx.input(|i| {
            (
                i.key_pressed(Key::ArrowUp),
                i.key_pressed(Key::ArrowDown),
                i.key_pressed(Key::Enter),
                i.key_pressed(Key::Escape),
            )
        });
        if down {
            qi.selected = (qi.selected + 1).min(results.len().saturating_sub(1));
        }
        if up {
            qi.selected = qi.selected.saturating_sub(1);
        }
        qi.selected = qi.selected.min(results.len().saturating_sub(1));
        let mut chosen: Option<ShapeRef> = None;
        egui::Area::new(egui::Id::new("quick-insert"))
            .order(egui::Order::Foreground)
            .fixed_pos(screen + Vec2::new(8.0, 8.0))
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_width(240.0);
                    let edit = ui.add(
                        egui::TextEdit::singleline(&mut qi.query)
                            .hint_text("Insert shape…")
                            .desired_width(f32::INFINITY),
                    );
                    if !qi.focused {
                        edit.request_focus();
                        qi.focused = true;
                    }
                    if edit.changed() {
                        qi.selected = 0;
                    }
                    for (i, def) in results.iter().enumerate() {
                        let selected = i == qi.selected;
                        let (rect, response) = ui.allocate_exact_size(
                            Vec2::new(ui.available_width(), 28.0),
                            Sense::click(),
                        );
                        if selected || response.hovered() {
                            ui.painter()
                                .rect_filled(rect, 4.0, ui.visuals().selection.bg_fill);
                        }
                        let thumb = egui::Rect::from_min_size(
                            rect.min + Vec2::new(4.0, 3.0),
                            Vec2::new(30.0, 22.0),
                        );
                        match def {
                            QuickChoice::Shape(def) => {
                                paint_thumbnail(ui.painter(), def, thumb, 1.0)
                            }
                            QuickChoice::Icon(icon) => {
                                crate::cloud::paint_icon(ui.painter(), icon, thumb, 1.0)
                            }
                        }
                        ui.painter().text(
                            Pos2::new(thumb.max.x + 8.0, rect.center().y),
                            egui::Align2::LEFT_CENTER,
                            def.name(),
                            egui::FontId::proportional(13.0),
                            ui.visuals().text_color(),
                        );
                        if response.clicked() {
                            chosen = Some(def.reference().clone());
                        }
                    }
                    if results.is_empty() {
                        ui.label(RichText::new("No shapes match").weak());
                    }
                    if enter && let Some(def) = results.get(qi.selected) {
                        chosen = Some(def.reference().clone());
                    }
                    ui.painter().rect_stroke(
                        ui.min_rect().expand(2.0),
                        6.0,
                        Stroke::new(1.0, ACCENT.gamma_multiply(0.3)),
                        egui::StrokeKind::Outside,
                    );
                });
            });
        if let Some(shape) = chosen {
            let at = qi.at;
            self.quick_insert = None;
            self.insert_shape_at(shape, at);
        } else if escape {
            self.quick_insert = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_ranks_names_first() {
        let libs = Libraries::builtin();
        let names = |q: &str| {
            search(libs, q)
                .iter()
                .map(|d| d.name.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(names("decision")[0], "Decision");
        assert!(
            names("db").iter().any(|n| n == "Database"),
            "keywords match"
        );
        assert!(names("cyl")[0] == "Cylinder");
        assert_eq!(search(libs, "").len(), libs.shapes().count());
        assert!(names("zzzzqqq").is_empty());
        assert_eq!(
            names("/process")[0],
            "Process",
            "a leading slash is ignored"
        );
    }
}
