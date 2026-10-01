//! Menu bar, tool bar, inspector, status bar and dialogs.

use crate::app::{BlueprintApp, Pending, Tool, shortcuts};
use bp_commands::Command;
use bp_model::kurbo::Rect;
use bp_model::{Color, Element};
use egui::{Button, DragValue, KeyboardShortcut, RichText, Ui, ViewportCommand};

impl BlueprintApp {
    pub fn menu_bar(&mut self, ui: &mut Ui) {
        use shortcuts::*;
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
                if item(ui, "Export as SVG…", Some(EXPORT_SVG), true) {
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
                let selected = self.selection.is_some();
                if item(ui, "Bring to front", Some(FRONT), selected) {
                    self.bring_to_front();
                }
                if item(ui, "Send to back", Some(BACK), selected) {
                    self.send_to_back();
                }
                ui.separator();
                if item(ui, "Delete", None, selected) {
                    self.delete_selection();
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
                ui.checkbox(&mut self.show_grid, "Show grid");
            });
        });
    }

    pub fn tool_bar(&mut self, ui: &mut Ui) {
        ui.add_space(6.0);
        ui.label(RichText::new("Tools").strong());
        ui.add_space(4.0);
        for (tool, label, key) in Tool::ALL {
            let button = Button::new(label)
                .shortcut_text(key.name())
                .selected(self.tool == tool)
                .min_size(egui::vec2(ui.available_width(), 28.0));
            if ui.add(button).clicked() {
                self.tool = tool;
            }
        }
        ui.add_space(12.0);
        ui.label(
            RichText::new("Drag to draw a shape, or click for the default size.")
                .small()
                .weak(),
        );
    }

    pub fn status_bar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            let dirty = if self.is_dirty() { " •" } else { "" };
            ui.label(format!("{}{dirty}", self.file_name()));
            ui.separator();
            ui.label(format!(
                "{} shapes",
                self.doc.elements_on_page(self.page).len()
            ));
            ui.separator();
            ui.label(format!("{:.0}%", self.view.zoom * 100.0));
            ui.separator();
            ui.label(RichText::new(&self.status).weak());
        });
    }

    pub fn inspector(&mut self, ui: &mut Ui) {
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.add_space(6.0);
            let selected = self
                .selection
                .and_then(|id| self.doc.elements.get(&id).cloned());
            match selected {
                Some(element) => self.element_inspector(ui, element),
                None => self.page_inspector(ui),
            }
        });
    }

    fn element_inspector(&mut self, ui: &mut Ui, element: Element) {
        let id = element.id;
        ui.heading(element.kind.label());
        ui.add_space(8.0);

        if self.editing.is_none() {
            ui.label("Text");
            let mut text = element.text.clone();
            let edit = egui::TextEdit::multiline(&mut text)
                .desired_rows(2)
                .desired_width(f32::INFINITY);
            if ui.add(edit).changed() {
                self.apply_merging(
                    "Edit text",
                    format!("text:{id}"),
                    Command::SetText { id, text },
                );
            }
            ui.add_space(8.0);
        }

        ui.label(RichText::new("Position and size").strong());
        let b = element.bounds;
        let (mut x, mut y, mut w, mut h) = (b.x0, b.y0, b.width(), b.height());
        let mut moved = false;
        egui::Grid::new("geometry").num_columns(4).show(ui, |ui| {
            ui.label("X");
            moved |= ui.add(DragValue::new(&mut x)).changed();
            ui.label("Y");
            moved |= ui.add(DragValue::new(&mut y)).changed();
            ui.end_row();
            ui.label("W");
            moved |= ui
                .add(DragValue::new(&mut w).range(1.0..=100_000.0))
                .changed();
            ui.label("H");
            moved |= ui
                .add(DragValue::new(&mut h).range(1.0..=100_000.0))
                .changed();
            ui.end_row();
        });
        if moved {
            let bounds = Rect::new(x, y, x + w, y + h);
            self.apply_merging(
                "Resize",
                format!("bounds:{id}"),
                Command::SetBounds { id, bounds },
            );
        }
        ui.add_space(8.0);

        ui.label(RichText::new("Style").strong());
        let mut style = element.style.clone();
        let mut restyled = false;
        egui::Grid::new("style").num_columns(2).show(ui, |ui| {
            restyled |= optional_color(ui, "Fill", &mut style.fill, Color::WHITE);
            ui.end_row();
            restyled |= optional_color(ui, "Stroke", &mut style.stroke, Color::OUTLINE);
            ui.end_row();
            ui.label("Stroke width");
            restyled |= ui
                .add(
                    DragValue::new(&mut style.stroke_width)
                        .speed(0.1)
                        .range(0.0..=40.0),
                )
                .changed();
            ui.end_row();
            ui.label("Text colour");
            restyled |= color_button(ui, &mut style.text_color);
            ui.end_row();
            ui.label("Font size");
            restyled |= ui
                .add(
                    DragValue::new(&mut style.font_size)
                        .speed(0.2)
                        .range(4.0..=400.0),
                )
                .changed();
            ui.end_row();
        });
        if restyled {
            self.apply_merging(
                "Change style",
                format!("style:{id}"),
                Command::SetStyle { id, style },
            );
        }
        ui.add_space(8.0);

        ui.label(RichText::new("Arrange").strong());
        ui.horizontal(|ui| {
            if ui.button("Bring to front").clicked() {
                self.bring_to_front();
            }
            if ui.button("Send to back").clicked() {
                self.send_to_back();
            }
        });
        ui.add_space(8.0);
        if ui.button("Delete shape").clicked() {
            self.delete_selection();
        }
    }

    fn page_inspector(&mut self, ui: &mut Ui) {
        let name = self
            .doc
            .pages
            .get(&self.page)
            .map_or_else(|| "Page".into(), |p| p.name.clone());
        ui.heading(name);
        ui.label(RichText::new("Nothing selected").weak());
        ui.add_space(8.0);
        ui.checkbox(&mut self.show_grid, "Show grid");
        ui.add_space(12.0);
        ui.label(RichText::new("Shortcuts").strong());
        egui::Grid::new("shortcuts")
            .num_columns(2)
            .striped(true)
            .show(ui, |ui| {
                for (keys, action) in [
                    ("V H R U O D T", "Tools"),
                    ("Double-click", "Edit text / add text"),
                    ("Enter", "Edit selected text"),
                    ("Ctrl+Enter", "Finish editing"),
                    ("Space + drag", "Pan"),
                    ("Middle drag", "Pan"),
                    ("Ctrl + scroll", "Zoom"),
                    ("Shift+1", "Zoom to fit"),
                    ("Arrows", "Nudge (Shift = 10)"),
                    ("Delete", "Delete shape"),
                    ("Esc", "Cancel / deselect"),
                ] {
                    ui.label(RichText::new(keys).monospace());
                    ui.label(action);
                    ui.end_row();
                }
            });
    }

    pub fn dialogs(&mut self, ctx: &egui::Context) {
        if let Some(action) = self.pending.clone() {
            let doing = match action {
                Pending::New => "starting a new diagram",
                Pending::Open(_) => "opening another file",
                Pending::Quit => "quitting",
            };
            let mut choice = None;
            let modal = egui::Modal::new(egui::Id::new("unsaved-changes")).show(ctx, |ui| {
                ui.set_width(380.0);
                ui.heading("Unsaved changes");
                ui.add_space(4.0);
                ui.label(format!(
                    "Save changes to “{}” before {doing}?",
                    self.file_name()
                ));
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if ui.button("Save").clicked() {
                        choice = Some(Choice::Save);
                    }
                    if ui.button("Don't save").clicked() {
                        choice = Some(Choice::Discard);
                    }
                    if ui.button("Cancel").clicked() {
                        choice = Some(Choice::Cancel);
                    }
                });
            });
            if choice.is_none() && modal.should_close() {
                choice = Some(Choice::Cancel);
            }
            if let Some(choice) = choice {
                self.pending = None;
                match choice {
                    Choice::Save => {
                        if self.save() {
                            self.perform(action);
                        }
                    }
                    Choice::Discard => self.perform(action),
                    Choice::Cancel => {}
                }
                ctx.request_repaint();
            }
        }

        if let Some(message) = self.error.clone() {
            let modal = egui::Modal::new(egui::Id::new("error")).show(ctx, |ui| {
                ui.set_width(420.0);
                ui.heading("Something went wrong");
                ui.add_space(4.0);
                ui.label(message);
                ui.add_space(12.0);
                ui.button("OK").clicked()
            });
            if modal.inner || modal.should_close() {
                self.error = None;
            }
        }
    }
}

#[derive(Clone, Copy)]
enum Choice {
    Save,
    Discard,
    Cancel,
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

fn color_button(ui: &mut Ui, color: &mut Color) -> bool {
    let mut rgba = [color.r, color.g, color.b, color.a];
    let changed = ui.color_edit_button_srgba_unmultiplied(&mut rgba).changed();
    if changed {
        *color = Color::rgba(rgba[0], rgba[1], rgba[2], rgba[3]);
    }
    changed
}

/// A checkbox that turns a colour on or off, plus the colour picker.
fn optional_color(ui: &mut Ui, label: &str, value: &mut Option<Color>, fallback: Color) -> bool {
    let mut on = value.is_some();
    let mut color = value.unwrap_or(fallback);
    let mut changed = ui.checkbox(&mut on, label).changed();
    ui.add_enabled_ui(on, |ui| changed |= color_button(ui, &mut color));
    *value = on.then_some(color);
    changed
}