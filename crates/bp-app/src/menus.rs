//! Menu bar, toolbar and status bar.

use crate::actions::shortcuts::*;
use crate::app::{BlueprintApp, Pending};
use crate::theme::labelled;
use bp_commands::edit::{Align, Reorder};
use egui::{Button, KeyboardShortcut, RichText, Ui, ViewportCommand};
use egui_phosphor::regular as icon;

impl BlueprintApp {
    pub fn menu_bar(&mut self, ui: &mut Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if item(ui, "New", Some(NEW), true) {
                    self.request(Pending::New);
                }
                if item(ui, "Open…", Some(OPEN), true) {
                    self.request(Pending::Open(None));
                }
                ui.separator();
                if item(ui, "Save", Some(SAVE), true) {
                    self.save();
                }
                if item(ui, "Save As…", Some(SAVE_AS), true) {
                    self.save_as();
                }
                ui.separator();
                if item(ui, "Export page as SVG…", Some(EXPORT_SVG), true) {
                    self.export_svg();
                }
                ui.separator();
                if item(ui, "Quit", Some(QUIT), true) {
                    ui.ctx().send_viewport_cmd(ViewportCommand::Close);
                }
            });

            ui.menu_button("Edit", |ui| {
                let undo = match self.history.undo_label() {
                    Some(label) => format!("Undo {label}"),
                    None => "Undo".into(),
                };
                if item(ui, &undo, Some(UNDO), self.history.can_undo()) {
                    self.undo();
                }
                let redo = match self.history.redo_label() {
                    Some(label) => format!("Redo {label}"),
                    None => "Redo".into(),
                };
                if item(ui, &redo, Some(REDO), self.history.can_redo()) {
                    self.redo();
                }
                ui.separator();
                let selected = !self.selection.is_empty();
                let ctx = ui.ctx().clone();
                if item_text(ui, "Cut", "Ctrl+X", selected) {
                    self.cut(&ctx);
                }
                if item_text(ui, "Copy", "Ctrl+C", selected) {
                    self.copy(&ctx);
                }
                if item_text(ui, "Paste", "Ctrl+V", self.clip.is_some())
                    && let Some(text) = self.clip.clone()
                {
                    self.paste_text(&text);
                }
                if item(ui, "Duplicate", Some(DUPLICATE), selected) {
                    self.duplicate();
                }
                if item_text(ui, "Delete", "Del", selected) {
                    self.delete_selection();
                }
                ui.separator();
                if item(ui, "Select all", Some(SELECT_ALL), true) {
                    self.select_all();
                }
                if item_text(ui, "Insert shape…", "/", true) {
                    self.open_quick_insert();
                }
            });

            ui.menu_button("View", |ui| {
                if item(ui, "Zoom in", Some(ZOOM_IN), true) {
                    self.zoom_by(1.25);
                }
                if item(ui, "Zoom out", Some(ZOOM_OUT), true) {
                    self.zoom_by(0.8);
                }
                if item(ui, "Actual size", Some(ZOOM_100), true) {
                    self.zoom_by(1.0 / self.view.zoom);
                }
                if item(ui, "Zoom to fit", Some(ZOOM_FIT), true) {
                    self.fit_requested = true;
                }
                ui.separator();
                ui.checkbox(&mut self.settings.show_grid, "Show grid");
                ui.checkbox(&mut self.settings.snap_to_grid, "Snap to grid");
                ui.checkbox(&mut self.settings.snap_to_shapes, "Smart guides");
            });

            ui.menu_button("Arrange", |ui| {
                let selected = !self.selection.is_empty();
                let several = self.selection.len() > 1;
                if item(ui, "Group", Some(GROUP), several) {
                    self.group_selection();
                }
                if item(ui, "Ungroup", Some(UNGROUP), selected) {
                    self.ungroup_selection();
                }
                ui.separator();
                for (label, shortcut, how) in [
                    ("Bring to front", FRONT, Reorder::Front),
                    ("Bring forward", FORWARD, Reorder::Forward),
                    ("Send backward", BACKWARD, Reorder::Backward),
                    ("Send to back", BACK, Reorder::Back),
                ] {
                    if item(ui, label, Some(shortcut), selected) {
                        self.reorder(how);
                    }
                }
                ui.separator();
                ui.add_enabled_ui(several, |ui| {
                    ui.menu_button("Align", |ui| {
                        for (label, how) in [
                            ("Left", Align::Left),
                            ("Centre", Align::CenterX),
                            ("Right", Align::Right),
                            ("Top", Align::Top),
                            ("Middle", Align::CenterY),
                            ("Bottom", Align::Bottom),
                        ] {
                            if item(ui, label, None, true) {
                                self.align(how);
                            }
                        }
                    });
                    ui.menu_button("Distribute", |ui| {
                        if item(ui, "Horizontally", None, self.selection.len() > 2) {
                            self.distribute(true);
                        }
                        if item(ui, "Vertically", None, self.selection.len() > 2) {
                            self.distribute(false);
                        }
                    });
                });
                ui.separator();
                if item(ui, "Lock / unlock", Some(LOCK), selected) {
                    self.toggle_lock();
                }
            });

            ui.menu_button("Page", |ui| {
                if item(ui, "New page", None, true) {
                    self.request_add_page();
                }
                if item(ui, "Duplicate page", None, true) {
                    self.duplicate_page(self.page);
                }
                if item(ui, "Delete page", None, self.doc.pages.len() > 1) {
                    self.delete_page(self.page);
                }
                ui.separator();
                if item(ui, "Next page", Some(NEXT_PAGE), self.doc.pages.len() > 1) {
                    self.step_page(1);
                }
                if item(
                    ui,
                    "Previous page",
                    Some(PREVIOUS_PAGE),
                    self.doc.pages.len() > 1,
                ) {
                    self.step_page(-1);
                }
            });
        });
    }

    pub fn floating_toolbar(&mut self, ctx: &egui::Context) {
        let id = egui::Id::new("floating_toolbar");
        let previous_size = ctx.memory(|memory| memory.area_rect(id).map(|rect| rect.size()));
        let response = egui::Area::new(id)
            .order(egui::Order::Foreground)
            .sense(egui::Sense::click_and_drag())
            .constrain_to(self.canvas_rect)
            .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -32.0))
            .default_width((self.canvas_rect.width() - 24.0).max(0.0))
            .show(ctx, |ui| {
                ui.set_max_width((self.canvas_rect.width() - 24.0).max(0.0));
                egui::Frame::new()
                    .fill(ui.visuals().panel_fill)
                    .stroke(egui::Stroke::new(1.0, crate::theme::CANVAS_EDGE))
                    .corner_radius(12)
                    .inner_margin(8)
                    .shadow(egui::Shadow {
                        offset: [0, 4],
                        blur: 16,
                        spread: 0,
                        color: egui::Color32::from_black_alpha(24),
                    })
                    .show(ui, |ui| self.toolbar(ui));
            });
        // Re-anchor after wrapping changes the bar's size, including window resizes.
        if previous_size != Some(response.response.rect.size()) {
            ctx.request_repaint();
        }
    }

    pub fn toolbar(&mut self, ui: &mut Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for (tool, glyph, label, key) in self.available_tools() {
                let selected = self.tool == tool;
                let button = Button::new(crate::theme::icon(glyph).size(17.0))
                    .selected(selected)
                    .min_size(egui::vec2(30.0, 28.0));
                if ui
                    .add(button)
                    .on_hover_text(format!("{label} ({})", key.name()))
                    .clicked()
                {
                    self.tool = tool;
                }
            }
            if self.page_kind() == Some(bp_model::DiagramKind::Erd) {
                self.erd_connection_toolbar(ui);
            }
            ui.separator();
            if ui
                .add_enabled(
                    self.history.can_undo(),
                    Button::new(crate::theme::icon(icon::ARROW_U_UP_LEFT).size(17.0)),
                )
                .on_hover_text("Undo")
                .clicked()
            {
                self.undo();
            }
            if ui
                .add_enabled(
                    self.history.can_redo(),
                    Button::new(crate::theme::icon(icon::ARROW_U_UP_RIGHT).size(17.0)),
                )
                .on_hover_text("Redo")
                .clicked()
            {
                self.redo();
            }
            ui.separator();
            if ui
                .button(crate::theme::icon(icon::MAGNIFYING_GLASS_MINUS).size(17.0))
                .on_hover_text("Zoom out")
                .clicked()
            {
                self.zoom_by(0.8);
            }
            if ui
                .add(
                    Button::new(format!("{:.0}%", self.view.zoom * 100.0))
                        .min_size(egui::vec2(52.0, 28.0)),
                )
                .on_hover_text("Actual size")
                .clicked()
            {
                self.zoom_by(1.0 / self.view.zoom);
            }
            if ui
                .button(crate::theme::icon(icon::MAGNIFYING_GLASS_PLUS).size(17.0))
                .on_hover_text("Zoom in")
                .clicked()
            {
                self.zoom_by(1.25);
            }
            if ui
                .button(crate::theme::icon(icon::CORNERS_OUT).size(17.0))
                .on_hover_text("Zoom to fit (Shift+1)")
                .clicked()
            {
                self.fit_requested = true;
            }
            ui.separator();
            let grid = &mut self.settings.snap_to_grid;
            if ui
                .add(Button::new(crate::theme::icon(icon::GRID_FOUR).size(17.0)).selected(*grid))
                .on_hover_text("Snap to grid")
                .clicked()
            {
                *grid = !*grid;
            }
            let guides = &mut self.settings.snap_to_shapes;
            if ui
                .add(Button::new(crate::theme::icon(icon::MAGNET).size(17.0)).selected(*guides))
                .on_hover_text("Smart guides")
                .clicked()
            {
                *guides = !*guides;
            }
            if let Some(scope) = self.scope {
                ui.separator();
                let count = self.doc.descendants(scope).len();
                if ui
                    .button(labelled(
                        icon::ARROW_BEND_UP_LEFT,
                        &format!("Editing group ({count}) — leave"),
                    ))
                    .clicked()
                {
                    self.scope = None;
                    self.selection = vec![scope];
                }
            }
        });
    }

    pub fn status_bar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            let dirty = if self.is_dirty() { " •" } else { "" };
            ui.label(format!("{}{dirty}", self.file_name()));
            ui.separator();
            let elements = self.doc.paint_order(self.page);
            let shapes = elements.iter().filter(|e| e.is_shape()).count();
            let connectors = elements.iter().filter(|e| e.is_connector()).count();
            ui.label(format!("{shapes} shapes, {connectors} connectors"));
            if !self.selection.is_empty() {
                ui.separator();
                ui.label(format!("{} selected", self.selection.len()));
            }
            if let Some(layer) = self.doc.layers.get(&self.layer) {
                ui.separator();
                ui.label(format!("Layer: {}", layer.name));
            }
            if let Some(p) = self.pointer {
                ui.separator();
                ui.label(RichText::new(format!("{:.0}, {:.0}", p.x, p.y)).monospace());
            }
            ui.separator();
            ui.label(RichText::new(&self.status).weak());
        });
    }
}

/// A menu entry with its shortcut. Closes the menu when clicked.
fn item(ui: &mut Ui, label: &str, shortcut: Option<KeyboardShortcut>, enabled: bool) -> bool {
    let mut button = Button::new(label);
    if let Some(shortcut) = shortcut {
        button = button.shortcut_text(ui.ctx().format_shortcut(&shortcut));
    }
    let clicked = ui.add_enabled(enabled, button).clicked();
    if clicked {
        ui.close();
    }
    clicked
}

/// A menu entry with a shortcut described in words.
fn item_text(ui: &mut Ui, label: &str, shortcut: &str, enabled: bool) -> bool {
    let clicked = ui
        .add_enabled(enabled, Button::new(label).shortcut_text(shortcut))
        .clicked();
    if clicked {
        ui.close();
    }
    clicked
}
