//! Modal dialogs: unsaved changes and errors.

use crate::app::{BlueprintApp, PageChoice, Pending};
use bp_model::DiagramKind;

#[derive(Clone, Copy)]
enum Choice {
    Save,
    Discard,
    Cancel,
}

impl BlueprintApp {
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

        if self.pending.is_none() && self.error.is_none() {
            self.page_kind_dialog(ctx);
        }
    }

    pub fn page_kind_dialog(&mut self, ctx: &egui::Context) {
        let Some(choice) = &self.page_choice else {
            return;
        };
        let new_page = matches!(choice, PageChoice::New);
        let mut chosen = None;
        let mut cancel = false;
        let mut restore = false;
        let modal = egui::Modal::new(egui::Id::new("page-diagram-kind")).show(ctx, |ui| {
            ui.set_width(420.0);
            ui.heading("Choose diagram type");
            ui.add_space(6.0);
            ui.label("Each page has its own shapes and connection tools.");
            ui.add_space(16.0);
            for (kind, description) in [
                (
                    DiagramKind::Erd,
                    "Tables, schema notes, and Crow’s Foot relationships, plus basic shapes.",
                ),
                (
                    DiagramKind::Flowchart,
                    "Flowchart symbols and connectors, plus basic shapes.",
                ),
            ] {
                if ui
                    .add_sized(
                        [ui.available_width(), 38.0],
                        egui::Button::new(egui::RichText::new(kind.label()).strong()),
                    )
                    .clicked()
                {
                    chosen = Some(kind);
                }
                ui.label(egui::RichText::new(description).small().weak());
                ui.add_space(12.0);
            }
            if new_page {
                cancel = ui.button("Cancel").clicked();
            } else if self.history.can_redo() {
                restore = ui.button("Restore previous page").clicked();
            }
        });
        if restore {
            self.redo();
            if self.page_kind().is_some() {
                self.page_choice = None;
            }
            ctx.request_repaint();
        } else if let Some(kind) = chosen {
            self.choose_page_kind(kind);
            ctx.request_repaint();
        } else if new_page && (cancel || modal.should_close()) {
            self.page_choice = None;
        }
    }
}
