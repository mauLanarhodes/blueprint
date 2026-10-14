//! Contextual inspector navigation and shared relationship details.

use crate::BlueprintApp;
use bp_commands::{Command, Prop};
use bp_model::{Dash, DiagramKind, Element, Endpoint, Marker};
use egui::{RichText, Ui};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InspectorTab {
    #[default]
    Design,
    Arrange,
    Configuration,
    Appearance,
    Table,
    Columns,
}

impl InspectorTab {
    pub fn label(self) -> &'static str {
        match self {
            Self::Design => "Design",
            Self::Arrange => "Arrange",
            Self::Configuration => "Configuration",
            Self::Appearance => "Appearance",
            Self::Table => "Table",
            Self::Columns => "Columns",
        }
    }
}

impl BlueprintApp {
    pub(crate) fn inspector_tabs(&mut self, ui: &mut Ui, tabs: &[InspectorTab]) {
        if !tabs.contains(&self.workspace.inspector_tab) {
            self.workspace.inspector_tab = tabs[0];
        }
        ui.horizontal_wrapped(|ui| {
            for &tab in tabs {
                let response =
                    ui.selectable_label(self.workspace.inspector_tab == tab, tab.label());
                crate::widgets::focus_outline(ui, &response);
                crate::widgets::command_focus(ui, &response);
                if response.clicked() {
                    self.workspace.inspector_tab = tab;
                }
            }
        });
        ui.separator();
    }

    /// Details are derived from semantic FK ownership, independent of line direction.
    /// The shell reuses this section in its drawer or the compact-window inspector.
    pub(crate) fn relationship_details(&mut self, ui: &mut Ui) {
        if self.page_kind() != Some(DiagramKind::Erd) {
            return;
        }
        let [id] = self.selection[..] else { return };
        let Some(element) = self.doc.elements.get(&id).cloned() else {
            return;
        };
        let Some(connector) = element.as_connector() else {
            return;
        };
        let name = |endpoint: &Endpoint| {
            endpoint
                .element()
                .and_then(|id| self.doc.elements.get(&id))
                .and_then(Element::text)
                .filter(|name| !name.is_empty())
                .unwrap_or("Unattached")
                .to_owned()
        };
        let (from, to) = connector
            .foreign_key_endpoints()
            .unwrap_or((&connector.source, &connector.target));
        let (from_name, to_name) = (name(from), name(to));
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(if connector.foreign_key.is_some() {
                    "Referencing table"
                } else {
                    "Source table"
                })
                .weak(),
            );
            ui.add(egui::Label::new(RichText::new(&from_name).strong()).truncate())
                .on_hover_text(&from_name);
            ui.label("→");
            ui.label(
                RichText::new(if connector.foreign_key.is_some() {
                    "Referenced table"
                } else {
                    "Target table"
                })
                .weak(),
            );
            ui.add(egui::Label::new(RichText::new(&to_name).strong()).truncate())
                .on_hover_text(&to_name);
        });
        ui.add_enabled_ui(!self.doc.is_locked(id), |ui| {
            if ui.available_width() >= 680.0 && connector.foreign_key.is_some() {
                ui.columns(2, |columns| {
                    self.relationship_type_section(&mut columns[0], &element);
                    self.erd_foreign_key_inspector(&mut columns[1], &element);
                });
            } else {
                self.relationship_type_section(ui, &element);
                if connector.foreign_key.is_some() {
                    self.erd_foreign_key_inspector(ui, &element);
                } else {
                    ui.label(RichText::new("Diagram relationship. Connect table column ports to define an SQL foreign key.").small().weak());
                }
            }
        });
    }

    fn relationship_type_section(&mut self, ui: &mut Ui, element: &Element) {
        let Some(connector) = element.as_connector() else {
            return;
        };
        self.relationship_cardinalities(ui, element);
        let mut identifying = connector.style.dash.unwrap_or(Dash::Solid) == Dash::Solid;
        if ui
            .checkbox(&mut identifying, "Identifying relationship")
            .changed()
        {
            self.apply(
                "Relationship type",
                [Command::Set {
                    id: element.id,
                    prop: Prop::Dash(Some(if identifying {
                        Dash::Solid
                    } else {
                        Dash::Dashed
                    })),
                }],
            );
        }
        ui.label(RichText::new("Identifying relationships use a solid line; other relationships use a dashed line.").small().weak());
    }

    pub(crate) fn relationship_cardinalities(&mut self, ui: &mut Ui, element: &Element) {
        let Some(connector) = element.as_connector() else {
            return;
        };
        let id = element.id;
        let owner_at_target = connector
            .foreign_key
            .as_ref()
            .is_some_and(|key| key.owner_at_target);
        let labels = if connector.foreign_key.is_some() {
            if owner_at_target {
                ["Referenced cardinality", "Referencing cardinality"]
            } else {
                ["Referencing cardinality", "Referenced cardinality"]
            }
        } else {
            ["Source cardinality", "Target cardinality"]
        };
        for (label, marker, start) in [
            (labels[0], connector.start_marker, true),
            (labels[1], connector.end_marker, false),
        ] {
            let mut controls = |ui: &mut Ui| {
                let label = ui.label(label);
                egui::ComboBox::from_id_salt(("relationship-cardinality", id, start))
                    .width(ui.available_width().min(160.0))
                    .truncate()
                    .selected_text(marker.label())
                    .show_ui(ui, |ui| {
                        for candidate in [
                            Marker::None,
                            Marker::ExactlyOne,
                            Marker::ZeroOrOne,
                            Marker::OneOrMany,
                            Marker::ZeroOrMany,
                            Marker::Many,
                        ] {
                            if ui
                                .selectable_label(marker == candidate, candidate.label())
                                .clicked()
                                && marker != candidate
                            {
                                self.apply(
                                    "Change cardinality",
                                    [Command::Set {
                                        id,
                                        prop: if start {
                                            Prop::StartMarker(candidate)
                                        } else {
                                            Prop::EndMarker(candidate)
                                        },
                                    }],
                                );
                            }
                        }
                    })
                    .response
                    .labelled_by(label.id);
            };
            if ui.available_width() < 320.0 {
                ui.vertical(&mut controls);
            } else {
                ui.horizontal(&mut controls);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relationship_details_fit_the_compact_inspector_and_leave_sql_unchanged() {
        let ctx = egui::Context::default();
        let mut app = BlueprintApp::new_with_workspace(&ctx, None, Default::default(), Vec::new());
        app.choose_page_kind(DiagramKind::Erd);
        app.paste_sql_schema();
        let owner = "customer_order_identifier_with_a_long_name";
        app.sql.import.as_mut().unwrap().source = format!(
            "CREATE TABLE parent (id integer PRIMARY KEY); CREATE TABLE child ({owner} integer, FOREIGN KEY ({owner}) REFERENCES parent(id));"
        );
        app.preview_sql_import();
        assert!(app.apply_sql_import());
        let connector = app
            .doc
            .elements
            .values()
            .find(|element| element.is_connector())
            .unwrap()
            .id;
        app.selection = vec![connector];
        let before = app.doc.clone();
        let revision = app.history.revision();
        for _ in 0..2 {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(260.0, 1000.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        app.relationship_details(ui);
                        assert!(
                            ui.min_rect().right() <= ui.max_rect().right() + 1.0,
                            "relationship details must fit the minimum inspector width"
                        );
                    });
                },
            );
            output.textures_delta.clear();
        }
        assert_eq!(app.doc, before);
        assert_eq!(app.history.revision(), revision);
    }
}
