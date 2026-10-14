//! Menu bar, toolbar and status bar.

use crate::actions::shortcuts::*;
use crate::app::{BlueprintApp, Pending};
use bp_commands::edit::{Align, Reorder};
use egui::{Button, KeyboardShortcut, RichText, Ui, ViewportCommand};
use egui_phosphor::regular as icon;

impl BlueprintApp {
    pub fn menu_bar(&mut self, ui: &mut Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.label(
                crate::theme::icon(icon::CUBE)
                    .size(20.0)
                    .color(crate::theme::ACCENT),
            );
            ui.label(RichText::new("Blueprint").strong());
            ui.add_space(8.0);
            ui.menu_button("File", |ui| {
                if item(ui, "New", Some(NEW), true) {
                    self.request(Pending::New);
                }
                if item(ui, "Open…", Some(OPEN), true) {
                    self.request(Pending::Open(None));
                }
                let erd = self.page_kind() == Some(bp_model::DiagramKind::Erd);
                if item(ui, "Open SQL script…", None, erd) {
                    self.open_sql_file();
                }
                if item(ui, "Paste SQL schema…", None, erd) {
                    self.paste_sql_schema();
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
                if item(ui, "Export page as SQL…", None, erd) {
                    self.open_sql_export();
                }
                if item(ui, "Cloud icon packs…", None, true) {
                    self.open_cloud_manager();
                }
                if item(
                    ui,
                    "AWS network…",
                    None,
                    self.page_kind() == Some(bp_model::DiagramKind::Cloud),
                ) {
                    self.open_deployment();
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
                ui.menu_button("Theme", |ui| {
                    ui.selectable_value(
                        &mut self.preferences.theme,
                        crate::WorkspaceTheme::Dark,
                        "Dark",
                    );
                    ui.selectable_value(
                        &mut self.preferences.theme,
                        crate::WorkspaceTheme::Light,
                        "Light",
                    );
                });
                if ui.button("Library / Navigator panel").clicked() {
                    self.toggle_dock(crate::workspace::Dock::Left);
                    ui.close();
                }
                if ui.button("Inspector panel").clicked() {
                    self.toggle_dock(crate::workspace::Dock::Right);
                    ui.close();
                }
                ui.separator();
                ui.checkbox(&mut self.settings.show_grid, "Show grid");
                ui.checkbox(&mut self.settings.snap_to_grid, "Snap to grid");
                ui.checkbox(&mut self.settings.snap_to_shapes, "Smart guides");
            });

            ui.menu_button("Arrange", |ui| {
                if item(ui, "Auto-arrange ERD", None, self.can_auto_arrange_erd()) {
                    self.auto_arrange_erd();
                }
                ui.separator();
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

    pub(crate) fn document_bar(&mut self, ui: &mut Ui) {
        let compact = ui.available_width() < 1000.0;
        let actions = if compact { 150.0 } else { 420.0 };
        let title_width = (ui.available_width() - actions - 12.0).max(100.0);
        ui.horizontal(|ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(title_width, 28.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    let title = format!(
                        "{}{}",
                        self.file_name(),
                        if self.is_dirty() { " •" } else { "" }
                    );
                    ui.add(egui::Label::new(RichText::new(title).size(15.0).strong()).truncate());
                    if let Some(page) = self.doc.pages.get(&self.page) {
                        ui.label(RichText::new("/").weak());
                        ui.add(egui::Label::new(RichText::new(&page.name).weak()).truncate());
                    }
                    if crate::widgets::icon_button(ui, icon::PLUS, "New page").clicked() {
                        if let Some(kind) = self.page_kind() {
                            self.add_page_with_kind(kind);
                        } else {
                            self.request_add_page();
                        }
                    }
                },
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.menu_button("Export", |ui| {
                    if ui.button("Export page as SVG…").clicked() {
                        self.export_svg();
                        ui.close();
                    }
                    if self.page_kind() == Some(bp_model::DiagramKind::Erd)
                        && ui.button("Export page as SQL…").clicked()
                    {
                        self.open_sql_export();
                        ui.close();
                    }
                });
                if crate::widgets::primary_icon_button(ui, icon::FLOPPY_DISK, "Save").clicked() {
                    self.save();
                }
                if compact {
                    ui.menu_button("Actions", |ui| self.context_actions(ui));
                } else {
                    match self.page_kind() {
                        Some(bp_model::DiagramKind::Erd) => {
                            if ui.button("Import SQL…").clicked() {
                                self.open_sql_file();
                            }
                        }
                        Some(bp_model::DiagramKind::Cloud)
                            if ui.button("AWS network…").clicked() =>
                        {
                            self.open_deployment();
                        }
                        _ => {}
                    }
                    ui.add_enabled_ui(self.history.can_redo(), |ui| {
                        if crate::widgets::icon_button(ui, icon::ARROW_U_UP_RIGHT, "Redo").clicked()
                        {
                            self.redo();
                        }
                    });
                    ui.add_enabled_ui(self.history.can_undo(), |ui| {
                        if crate::widgets::icon_button(ui, icon::ARROW_U_UP_LEFT, "Undo").clicked()
                        {
                            self.undo();
                        }
                    });
                    if self.page_kind() == Some(bp_model::DiagramKind::Erd) {
                        self.erd_connection_toolbar(ui);
                    }
                }
            });
        });
    }

    fn context_actions(&mut self, ui: &mut Ui) {
        if ui
            .add_enabled(self.history.can_undo(), Button::new("Undo"))
            .clicked()
        {
            self.undo();
            ui.close();
        }
        if ui
            .add_enabled(self.history.can_redo(), Button::new("Redo"))
            .clicked()
        {
            self.redo();
            ui.close();
        }
        match self.page_kind() {
            Some(bp_model::DiagramKind::Erd) => {
                if ui.button("Import SQL…").clicked() {
                    self.open_sql_file();
                    ui.close();
                }
                if ui.button("Paste SQL schema…").clicked() {
                    self.paste_sql_schema();
                    ui.close();
                }
                self.erd_connection_toolbar(ui);
            }
            Some(bp_model::DiagramKind::Cloud) => {
                if ui.button("AWS network…").clicked() {
                    self.open_deployment();
                    ui.close();
                }
                if ui.button("Manage icon packs…").clicked() {
                    self.open_cloud_manager();
                    ui.close();
                }
            }
            _ => {}
        }
    }

    pub(crate) fn floating_toolbar(&mut self, ctx: &egui::Context) {
        self.workspace.tool_rect = egui::Rect::NOTHING;
        if self.page_kind().is_none()
            || self.pending.is_some()
            || self.error.is_some()
            || self.page_choice.is_some()
            || self.cloud.manager_open
            || self.deployment.open
            || self.sql.is_open()
        {
            return;
        }
        let id = egui::Id::new("floating_toolbar");
        let previous = ctx.memory(|memory| memory.area_rect(id).map(|rect| rect.size()));
        let width = (self.canvas_rect.width() - 24.0).max(0.0);
        let size = previous.unwrap_or(egui::vec2(width, 48.0));
        let position = egui::pos2(
            self.canvas_rect.center().x - size.x.min(width) * 0.5,
            self.canvas_rect.bottom() - 32.0 - size.y,
        );
        let response = egui::Area::new(id)
            .order(egui::Order::Foreground)
            .sense(egui::Sense::click_and_drag())
            .movable(false)
            .fixed_pos(position)
            .constrain_to(self.canvas_rect.shrink2(egui::vec2(12.0, 0.0)))
            .default_width(width)
            .show(ctx, |ui| {
                ui.set_max_width(width);
                egui::Frame::new()
                    .fill(ui.visuals().panel_fill)
                    .stroke(ui.visuals().window_stroke)
                    .corner_radius(12)
                    .inner_margin(8)
                    .shadow(egui::Shadow {
                        offset: [0, 4],
                        blur: 12,
                        spread: 0,
                        color: egui::Color32::from_black_alpha(72),
                    })
                    .show(ui, |ui| self.floating_toolbar_controls(ui));
            });
        self.workspace.tool_rect = response.response.rect;
        if previous != Some(response.response.rect.size()) {
            ctx.request_repaint();
        }
    }

    fn floating_toolbar_controls(&mut self, ui: &mut Ui) {
        let compact = self.canvas_rect.width() < 680.0;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for (tool, glyph, label, key) in self.available_tools() {
                let response = ui.add(
                    Button::new(crate::theme::icon(glyph).size(18.0))
                        .selected(self.tool == tool)
                        .min_size(egui::vec2(28.0, 28.0)),
                );
                response.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label)
                });
                crate::widgets::focus_outline(ui, &response);
                crate::widgets::command_focus(ui, &response);
                if response
                    .on_hover_text(format!("{label} ({})", key.name()))
                    .clicked()
                {
                    self.finish_inline_edits();
                    self.cancel_drag();
                    self.tool = tool;
                }
            }
            ui.separator();
            ui.add_enabled_ui(self.history.can_undo(), |ui| {
                if crate::widgets::icon_button(ui, icon::ARROW_U_UP_LEFT, "Undo").clicked() {
                    self.undo();
                }
            });
            ui.add_enabled_ui(self.history.can_redo(), |ui| {
                if crate::widgets::icon_button(ui, icon::ARROW_U_UP_RIGHT, "Redo").clicked() {
                    self.redo();
                }
            });
            ui.separator();
            if crate::widgets::icon_button(ui, icon::MAGNIFYING_GLASS_MINUS, "Zoom out").clicked() {
                self.zoom_by(0.8);
            }
            let actual = ui.button(format!("{:.0}%", self.view.zoom * 100.0));
            actual.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Actual size")
            });
            crate::widgets::command_focus(ui, &actual);
            crate::widgets::focus_outline(ui, &actual);
            if actual.on_hover_text("Actual size").clicked() {
                self.zoom_by(1.0 / self.view.zoom);
            }
            if crate::widgets::icon_button(ui, icon::MAGNIFYING_GLASS_PLUS, "Zoom in").clicked() {
                self.zoom_by(1.25);
            }
            if crate::widgets::icon_button(ui, icon::CORNERS_OUT, "Zoom to fit (Shift+1)").clicked()
            {
                self.fit_requested = true;
            }
            ui.separator();
            if compact {
                let menu = ui.menu_button("More", |ui| self.floating_toolbar_secondary(ui));
                crate::widgets::command_focus(ui, &menu.response);
                crate::widgets::focus_outline(ui, &menu.response);
            } else {
                self.floating_toolbar_secondary(ui);
            }
        });
    }

    fn floating_toolbar_secondary(&mut self, ui: &mut Ui) {
        if self.page_kind() == Some(bp_model::DiagramKind::Erd) {
            self.erd_connection_toolbar(ui);
        }
        for (glyph, label, selected, grid) in [
            (
                icon::GRID_FOUR,
                "Snap to grid",
                self.settings.snap_to_grid,
                true,
            ),
            (
                icon::MAGNET,
                "Smart guides",
                self.settings.snap_to_shapes,
                false,
            ),
        ] {
            let response =
                ui.add(Button::new(crate::theme::icon(glyph).size(18.0)).selected(selected));
            response.widget_info(|| {
                egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, label)
            });
            crate::widgets::command_focus(ui, &response);
            crate::widgets::focus_outline(ui, &response);
            if response.on_hover_text(label).clicked() {
                if grid {
                    self.settings.snap_to_grid = !selected;
                } else {
                    self.settings.snap_to_shapes = !selected;
                }
            }
        }
        if let Some(scope) = self.scope
            && crate::widgets::icon_button(ui, icon::ARROW_BEND_UP_LEFT, "Leave group").clicked()
        {
            self.finish_inline_edits();
            self.cancel_drag();
            self.scope = None;
            self.select_only(scope);
        }
    }

    pub fn status_bar(&mut self, ui: &mut Ui) {
        let width = ui.available_width();
        ui.horizontal(|ui| {
            ui.allocate_ui_with_layout(
                egui::vec2((width - 220.0).max(100.0), 28.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    let (glyph, label, color) = if self.is_dirty() {
                        (icon::PENCIL_SIMPLE, "Modified", ui.visuals().warn_fg_color)
                    } else if self.path.is_none() {
                        (icon::FILE, "Unsaved", ui.visuals().weak_text_color())
                    } else {
                        (
                            icon::CHECK_CIRCLE,
                            "Saved",
                            if ui.visuals().dark_mode {
                                crate::theme::SUCCESS
                            } else {
                                egui::Color32::from_rgb(15, 118, 110)
                            },
                        )
                    };
                    ui.label(crate::theme::icon(glyph).color(color));
                    ui.label(label);
                    if !self.selection.is_empty() {
                        ui.separator();
                        ui.label(format!("{} selected", self.selection.len()));
                    }
                    if width > 1050.0 {
                        ui.label(
                            RichText::new(format!("{} elements", self.scene.order.len())).weak(),
                        );
                    }
                    ui.separator();
                    ui.add(egui::Label::new(RichText::new(&self.status).weak()).truncate());
                },
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if crate::widgets::icon_button(ui, icon::CORNERS_OUT, "Zoom to fit (Shift+1)")
                    .clicked()
                {
                    self.fit_requested = true;
                }
                if crate::widgets::icon_button(ui, icon::MAGNIFYING_GLASS_PLUS, "Zoom in").clicked()
                {
                    self.zoom_by(1.25);
                }
                if ui
                    .button(format!("{:.0}%", self.view.zoom * 100.0))
                    .on_hover_text("Actual size")
                    .clicked()
                {
                    self.zoom_by(1.0 / self.view.zoom);
                }
                if crate::widgets::icon_button(ui, icon::MAGNIFYING_GLASS_MINUS, "Zoom out")
                    .clicked()
                {
                    self.zoom_by(0.8);
                }
            });
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
