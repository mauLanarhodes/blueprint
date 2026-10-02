//! Modal dialogs: unsaved changes and errors.

use crate::app::{BlueprintApp, Pending};

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
    }
}
