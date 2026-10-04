//! Page tabs (bottom of the window) and the layers panel.

use crate::app::{BlueprintApp, Renaming};
use crate::theme::ACCENT;
use bp_commands::LayerProp;
use egui::{Button, RichText, Sense, Ui};
use egui_phosphor::regular as icon;

impl BlueprintApp {
    pub fn page_tabs(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            let pages: Vec<(bp_model::PageId, String)> = self
                .doc
                .pages_sorted()
                .iter()
                .map(|p| (p.id, p.name.clone()))
                .collect();
            let many = pages.len() > 1;
            for (i, (id, name)) in pages.iter().enumerate() {
                if let Some(Renaming::Page(renaming, text)) = &mut self.renaming
                    && renaming == id
                {
                    let edit = ui.add(egui::TextEdit::singleline(text).desired_width(120.0));
                    edit.request_focus();
                    let done = edit.lost_focus() || ui.input(|i| i.key_pressed(egui::Key::Enter));
                    let cancel = ui.input(|i| i.key_pressed(egui::Key::Escape));
                    if cancel {
                        self.renaming = None;
                    } else if done {
                        let (page, text) = (*id, text.clone());
                        self.renaming = None;
                        self.rename_page(page, text);
                    }
                    continue;
                }
                let current = *id == self.page;
                let tab = ui.add(
                    Button::new(name.as_str())
                        .selected(current)
                        .min_size(egui::vec2(72.0, 24.0)),
                );
                if tab.clicked() {
                    self.set_page(*id);
                }
                if tab.double_clicked() {
                    self.renaming = Some(Renaming::Page(*id, name.clone()));
                }
                let id = *id;
                tab.context_menu(|ui| {
                    if ui
                        .button(crate::theme::labelled(icon::PENCIL_SIMPLE, "Rename"))
                        .clicked()
                    {
                        self.renaming = Some(Renaming::Page(id, name.clone()));
                        ui.close();
                    }
                    if ui
                        .button(crate::theme::labelled(icon::COPY, "Duplicate"))
                        .clicked()
                    {
                        self.duplicate_page(id);
                        ui.close();
                    }
                    ui.separator();
                    if ui
                        .add_enabled(
                            i > 0,
                            Button::new(crate::theme::labelled(icon::CARET_LEFT, "Move left")),
                        )
                        .clicked()
                    {
                        self.move_page(id, -1);
                        ui.close();
                    }
                    if ui
                        .add_enabled(
                            i + 1 < pages.len(),
                            Button::new(crate::theme::labelled(icon::CARET_RIGHT, "Move right")),
                        )
                        .clicked()
                    {
                        self.move_page(id, 1);
                        ui.close();
                    }
                    ui.separator();
                    if ui
                        .add_enabled(
                            many,
                            Button::new(crate::theme::labelled(icon::TRASH, "Delete page")),
                        )
                        .clicked()
                    {
                        self.delete_page(id);
                        ui.close();
                    }
                });
            }
            if ui
                .button(crate::theme::icon(icon::PLUS))
                .on_hover_text("Add page")
                .clicked()
            {
                self.request_add_page();
            }
        });
    }

    pub fn layers_panel(&mut self, ui: &mut Ui) {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Layers").strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let layer = self.layer;
                let several = self.doc.layers_of(self.page).len() > 1;
                if ui
                    .add_enabled(
                        several,
                        Button::new(crate::theme::icon(icon::TRASH)).small(),
                    )
                    .on_hover_text("Delete layer")
                    .clicked()
                {
                    self.delete_layer(layer);
                }
                if ui
                    .small_button(crate::theme::icon(icon::CARET_DOWN))
                    .on_hover_text("Move layer down")
                    .clicked()
                {
                    self.move_layer(layer, false);
                }
                if ui
                    .small_button(crate::theme::icon(icon::CARET_UP))
                    .on_hover_text("Move layer up")
                    .clicked()
                {
                    self.move_layer(layer, true);
                }
                if ui
                    .small_button(crate::theme::icon(icon::PLUS))
                    .on_hover_text("Add layer")
                    .clicked()
                {
                    self.add_layer();
                }
            });
        });
        ui.separator();
        let layers: Vec<bp_model::Layer> = self
            .doc
            .layers_of(self.page)
            .into_iter()
            .rev()
            .cloned()
            .collect();
        let tree = self.doc.tree();
        let counts: Vec<usize> = layers
            .iter()
            .map(|l| {
                let mut all = Vec::new();
                tree.subtree(bp_model::Parent::Layer(l.id), &mut all);
                all.len()
            })
            .collect();
        drop(tree);
        egui::ScrollArea::vertical()
            .id_salt("layers")
            .show(ui, |ui| {
                for (layer, count) in layers.iter().zip(counts) {
                    ui.horizontal(|ui| {
                        let eye = if layer.visible {
                            icon::EYE
                        } else {
                            icon::EYE_SLASH
                        };
                        if ui
                            .small_button(crate::theme::icon(eye))
                            .on_hover_text(if layer.visible { "Hide" } else { "Show" })
                            .clicked()
                        {
                            self.set_layer_flag(layer.id, LayerProp::Visible(!layer.visible));
                        }
                        let lock = if layer.locked {
                            icon::LOCK
                        } else {
                            icon::LOCK_OPEN
                        };
                        if ui
                            .small_button(crate::theme::icon(lock))
                            .on_hover_text(if layer.locked { "Unlock" } else { "Lock" })
                            .clicked()
                        {
                            self.set_layer_flag(layer.id, LayerProp::Locked(!layer.locked));
                        }
                        if let Some(Renaming::Layer(renaming, text)) = &mut self.renaming
                            && *renaming == layer.id
                        {
                            let edit =
                                ui.add(egui::TextEdit::singleline(text).desired_width(110.0));
                            edit.request_focus();
                            let done =
                                edit.lost_focus() || ui.input(|i| i.key_pressed(egui::Key::Enter));
                            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                                self.renaming = None;
                            } else if done {
                                let (id, text) = (layer.id, text.clone());
                                self.renaming = None;
                                self.rename_layer(id, text);
                            }
                            return;
                        }
                        let active = layer.id == self.layer;
                        let mut name = RichText::new(&layer.name);
                        if !layer.visible {
                            name = name.weak();
                        }
                        if active {
                            name = name.color(ACCENT).strong();
                        }
                        let row = ui.add(egui::Label::new(name).sense(Sense::click()));
                        if row.clicked() {
                            self.layer = layer.id;
                        }
                        if row.double_clicked() {
                            self.renaming = Some(Renaming::Layer(layer.id, layer.name.clone()));
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(RichText::new(count.to_string()).small().weak());
                        });
                    });
                }
            });
        let movable = !self.selection.is_empty()
            && self
                .selection
                .iter()
                .any(|id| self.doc.layer_of(*id).is_some_and(|l| l != self.layer));
        if movable {
            let name = self
                .doc
                .layers
                .get(&self.layer)
                .map(|l| l.name.clone())
                .unwrap_or_default();
            if ui.button(format!("Move selection to “{name}”")).clicked() {
                self.move_selection_to_layer(self.layer);
            }
        }
    }
}
