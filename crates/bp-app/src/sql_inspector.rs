//! Editable SQL metadata uses normal validated, undoable property commands.

use crate::BlueprintApp;
use bp_commands::{ColumnProp, Command, Prop};
use bp_model::{
    ColumnId, Element, ElementId, Endpoint, ErdColumn, ErdForeignKey, ErdIndex, ErdKey, PortId,
};
use egui::{RichText, Ui};

impl BlueprintApp {
    pub(crate) fn erd_sql_metadata_inspector(&mut self, ui: &mut Ui, element: &Element) {
        let Some(original) = element.as_shape().and_then(|shape| shape.erd.as_ref()) else {
            return;
        };
        let mut table = original.clone();
        let id = element.id;
        ui.push_id(("sql-metadata", id), |ui| {
            if table.schema.len() <= 1 {
                let label = ui.label("Schema");
                let mut schema = table.schema.first().cloned().unwrap_or_default();
                if ui
                    .add(
                        egui::TextEdit::singleline(&mut schema)
                            .hint_text("Default schema")
                            .desired_width(f32::INFINITY),
                    )
                    .labelled_by(label.id)
                    .changed()
                {
                    table.schema = (!schema.is_empty()).then_some(schema).into_iter().collect();
                }
            } else {
                for (i, qualifier) in table.schema.iter_mut().enumerate() {
                    let label = ui.label(format!("Schema qualifier {}", i + 1));
                    ui.add(egui::TextEdit::singleline(qualifier).desired_width(f32::INFINITY))
                        .labelled_by(label.id);
                }
            }
            egui::CollapsingHeader::new("SQL keys and indexes")
                .id_salt("constraints")
                .show(ui, |ui| {
                    if let Some(key) = &mut table.primary_key {
                        ui.label(RichText::new("Primary key constraint").strong());
                        key_editor(ui, "pk", key, &table.columns);
                        if ui.button("Remove primary key").clicked() {
                            table.primary_key = None;
                        }
                    } else if ui
                        .add_enabled(
                            !table.columns.is_empty(),
                            egui::Button::new("Add primary key"),
                        )
                        .clicked()
                    {
                        table.primary_key = Some(ErdKey {
                            name: None,
                            columns: table
                                .columns
                                .iter()
                                .filter(|c| c.primary_key)
                                .map(|c| c.id)
                                .collect(),
                        });
                        if let Some(key) = &mut table.primary_key
                            && key.columns.is_empty()
                        {
                            key.columns.push(table.columns[0].id);
                        }
                    }
                    ui.separator();
                    let mut remove = None;
                    for (i, key) in table.unique_keys.iter_mut().enumerate() {
                        ui.push_id(("unique", i), |ui| {
                            ui.label(
                                RichText::new(format!("Unique constraint {}", i + 1)).strong(),
                            );
                            key_editor(ui, "unique", key, &table.columns);
                            if ui.button("Remove unique constraint").clicked() {
                                remove = Some(i);
                            }
                        });
                    }
                    if let Some(index) = remove {
                        table.unique_keys.remove(index);
                    }
                    if ui
                        .add_enabled(
                            !table.columns.is_empty(),
                            egui::Button::new("Add unique constraint"),
                        )
                        .clicked()
                    {
                        table.unique_keys.push(ErdKey {
                            name: None,
                            columns: vec![table.columns[0].id],
                        });
                    }
                    ui.separator();
                    let mut remove = None;
                    for (i, index) in table.indexes.iter_mut().enumerate() {
                        ui.push_id(("index", i), |ui| {
                            ui.label(RichText::new(format!("Index {}", i + 1)).strong());
                            let label = ui.label("Index name");
                            ui.add(
                                egui::TextEdit::singleline(&mut index.name)
                                    .desired_width(f32::INFINITY),
                            )
                            .labelled_by(label.id);
                            ui.checkbox(&mut index.unique, "Unique index");
                            optional_text(ui, "Index method", &mut index.method, "Default (btree)");
                            let mut remove_expression = None;
                            let multiple = index.columns.len() > 1;
                            for (j, expression) in index.columns.iter_mut().enumerate() {
                                ui.push_id(j, |ui| {
                                    let label = ui.label(format!("Index expression {}", j + 1));
                                    let mut sql = bp_sql::current_index_expression(
                                        expression,
                                        &table.columns,
                                    );
                                    if ui
                                        .add(
                                            egui::TextEdit::singleline(&mut sql)
                                                .desired_width(f32::INFINITY),
                                        )
                                        .labelled_by(label.id)
                                        .changed()
                                    {
                                        *expression =
                                            bp_sql::bind_index_expression(&sql, &table.columns);
                                    }
                                    if multiple && ui.button("Remove expression").clicked() {
                                        remove_expression = Some(j);
                                    }
                                });
                            }
                            if let Some(j) = remove_expression {
                                index.columns.remove(j);
                            }
                            if ui.button("Add index expression").clicked()
                                && let Some(column) = table.columns.first()
                            {
                                index.columns.push(bp_sql::bind_index_expression(
                                    &quote(&column.name),
                                    &table.columns,
                                ));
                            }
                            let label = ui.label("Index predicate");
                            let mut sql = index
                                .predicate
                                .as_ref()
                                .map(|expression| {
                                    bp_sql::current_index_expression(expression, &table.columns)
                                })
                                .unwrap_or_default();
                            if ui
                                .add(
                                    egui::TextEdit::singleline(&mut sql)
                                        .hint_text("None")
                                        .desired_width(f32::INFINITY),
                                )
                                .labelled_by(label.id)
                                .changed()
                            {
                                index.predicate = (!sql.trim().is_empty())
                                    .then(|| bp_sql::bind_index_expression(&sql, &table.columns));
                            }
                            if ui.button("Remove index").clicked() {
                                remove = Some(i);
                            }
                            ui.separator();
                        });
                    }
                    if let Some(i) = remove {
                        table.indexes.remove(i);
                    }
                    if ui
                        .add_enabled(!table.columns.is_empty(), egui::Button::new("Add index"))
                        .clicked()
                    {
                        table.indexes.push(ErdIndex {
                            name: format!("index_{}", table.indexes.len() + 1),
                            unique: false,
                            columns: vec![bp_sql::bind_index_expression(
                                &quote(&table.columns[0].name),
                                &table.columns,
                            )],
                            method: None,
                            predicate: None,
                        });
                    }
                });
        });
        if table.primary_key != original.primary_key {
            for column in &mut table.columns {
                column.primary_key = table
                    .primary_key
                    .as_ref()
                    .is_some_and(|key| key.columns.contains(&column.id));
                if column.primary_key {
                    column.nullable = false;
                }
            }
        }
        if table.unique_keys != original.unique_keys {
            for column in &mut table.columns {
                let formerly_single = original
                    .unique_keys
                    .iter()
                    .any(|key| key.columns == [column.id]);
                let now_single = table
                    .unique_keys
                    .iter()
                    .any(|key| key.columns == [column.id]);
                if formerly_single || now_single {
                    column.unique = now_single;
                }
            }
        }
        // Empty key lists cannot produce SQL and fail model validation; the last
        // checked column stays selected in key_editor.
        if table != *original {
            self.apply_merging(
                "Edit SQL table definition",
                format!("sql-table:{id}"),
                vec![Command::Set {
                    id,
                    prop: Prop::ErdTable(Box::new(table)),
                }],
            );
        }
    }

    pub(crate) fn erd_foreign_key_inspector(&mut self, ui: &mut Ui, element: &Element) {
        let Some(connector) = element.as_connector() else {
            return;
        };
        let Some(original) = &connector.foreign_key else {
            return;
        };
        ui.label(RichText::new("Foreign key mapping").strong());
        for mapping in self.foreign_key_mapping(element.id) {
            ui.label(mapping);
        }
        ui.add_space(4.0);
        let table = |endpoint: &bp_model::Endpoint| {
            self.doc
                .elements
                .get(&endpoint.element()?)?
                .as_shape()?
                .erd
                .as_ref()
        };
        let Some((owner_endpoint, referenced_endpoint)) = connector.foreign_key_endpoints() else {
            return;
        };
        let (Some(source), Some(target)) = (table(owner_endpoint), table(referenced_endpoint))
        else {
            return;
        };
        let mut key = original.clone();
        egui::CollapsingHeader::new("SQL foreign key")
            .id_salt(("sql-foreign-key", element.id))
            .default_open(true)
            .show(ui, |ui| {
                optional_text(ui, "Constraint name", &mut key.name, "Automatic");
                let mut remove = None;
                let multiple = key.columns.len() > 1;
                for (i, (source_id, target_id)) in key
                    .columns
                    .iter_mut()
                    .zip(&mut key.referenced_columns)
                    .enumerate()
                {
                    ui.push_id(i, |ui| {
                        ui.label(format!("Column mapping {}", i + 1));
                        column_picker(ui, "Referencing column", source_id, &source.columns);
                        column_picker(ui, "Referenced column", target_id, &target.columns);
                        if multiple && ui.button("Remove mapping").clicked() {
                            remove = Some(i);
                        }
                    });
                }
                if let Some(i) = remove {
                    key.columns.remove(i);
                    key.referenced_columns.remove(i);
                }
                if ui.button("Add column mapping").clicked()
                    && let (Some(source_id), Some(target_id)) = (
                        source
                            .columns
                            .iter()
                            .find(|c| !key.columns.contains(&c.id))
                            .map(|c| c.id),
                        target
                            .columns
                            .iter()
                            .find(|c| !key.referenced_columns.contains(&c.id))
                            .map(|c| c.id),
                    )
                {
                    key.columns.push(source_id);
                    key.referenced_columns.push(target_id);
                }
                reference_action(ui, "On delete", &mut key.on_delete);
                reference_action(ui, "On update", &mut key.on_update);
            });
        if key != *original {
            self.set_sql_foreign_key(element.id, key);
        }
    }

    /// Updates constraint metadata, the drawn first-pair ports, and row badges
    /// together. Other constraints still using an old row keep its FK badge.
    pub fn set_sql_foreign_key(&mut self, id: ElementId, key: ErdForeignKey) {
        if self.doc.is_locked(id) {
            return;
        }
        let Some(connector) = self.doc.elements.get(&id).and_then(Element::as_connector) else {
            return;
        };
        let Some(original) = &connector.foreign_key else {
            return;
        };
        let Some((owner_endpoint, referenced_endpoint)) = connector.foreign_key_endpoints() else {
            return;
        };
        let (Some(source), Some(target), Some(first_source), Some(first_target)) = (
            owner_endpoint.element(),
            referenced_endpoint.element(),
            key.columns.first(),
            key.referenced_columns.first(),
        ) else {
            return;
        };
        let side = |endpoint: &Endpoint, default| match endpoint {
            Endpoint::Glued {
                port: Some(port), ..
            } => port.as_str().ends_with(":w"),
            _ => default,
        };
        let owner = Endpoint::Glued {
            element: source,
            port: Some(PortId::column(*first_source, side(owner_endpoint, false))),
        };
        let referenced = Endpoint::Glued {
            element: target,
            port: Some(PortId::column(
                *first_target,
                side(referenced_endpoint, true),
            )),
        };
        let (physical_source, physical_target) = if original.owner_at_target {
            (referenced, owner)
        } else {
            (owner, referenced)
        };
        let mut commands = vec![
            Command::Set {
                id,
                prop: Prop::ForeignKey(None),
            },
            Command::Set {
                id,
                prop: Prop::Source(physical_source),
            },
            Command::Set {
                id,
                prop: Prop::Target(physical_target),
            },
            Command::Set {
                id,
                prop: Prop::ForeignKey(Some(key.clone())),
            },
        ];
        let mut rows = original.columns.clone();
        rows.extend(&key.columns);
        rows.sort();
        rows.dedup();
        commands.extend(self.sql_foreign_key_badges(id, source, &rows, &key.columns));
        self.apply_merging("Edit SQL foreign key", format!("sql-fk:{id}"), commands);
    }

    pub(crate) fn sql_foreign_key_badges(
        &self,
        id: ElementId,
        source: ElementId,
        rows: &[ColumnId],
        owned_columns: &[ColumnId],
    ) -> Vec<Command> {
        rows.iter()
            .map(|column| {
                let used_elsewhere = self
                    .doc
                    .elements
                    .values()
                    .filter(|element| element.id != id)
                    .filter_map(Element::as_connector)
                    .any(|other| {
                        if let Some(other_key) = &other.foreign_key {
                            other
                                .foreign_key_endpoints()
                                .and_then(|(owner, _)| owner.element())
                                == Some(source)
                                && other_key.columns.contains(column)
                        } else {
                            [&other.source, &other.target].iter().any(|endpoint| {
                                matches!(endpoint, Endpoint::Glued { element, port: Some(port) }
                                if *element == source && port.column_id() == Some(*column))
                            })
                        }
                    });
                Command::SetColumn {
                    id: source,
                    column: *column,
                    prop: ColumnProp::ForeignKey(owned_columns.contains(column) || used_elsewhere),
                }
            })
            .collect()
    }
}

fn key_editor(ui: &mut Ui, id: &str, key: &mut ErdKey, columns: &[ErdColumn]) {
    ui.push_id(id, |ui| {
        optional_text(ui, "Constraint name", &mut key.name, "Automatic");
        ui.label("Key columns (selection order)");
        for column in columns {
            let mut selected = key.columns.contains(&column.id);
            let enabled = !selected || key.columns.len() > 1;
            if ui
                .add_enabled(enabled, egui::Checkbox::new(&mut selected, &column.name))
                .changed()
            {
                if selected {
                    key.columns.push(column.id);
                } else {
                    key.columns.retain(|id| *id != column.id);
                }
            }
        }
        let names: Vec<_> = key
            .columns
            .iter()
            .filter_map(|id| {
                columns
                    .iter()
                    .find(|column| column.id == *id)
                    .map(|column| column.name.as_str())
            })
            .collect();
        ui.label(RichText::new(names.join(", ")).small().weak());
    });
}

fn optional_text(ui: &mut Ui, name: &str, value: &mut Option<String>, hint: &str) {
    let label = ui.label(name);
    let mut text = value.clone().unwrap_or_default();
    if ui
        .add(
            egui::TextEdit::singleline(&mut text)
                .hint_text(hint)
                .desired_width(f32::INFINITY),
        )
        .labelled_by(label.id)
        .changed()
    {
        *value = (!text.trim().is_empty()).then_some(text);
    }
}

fn column_picker(ui: &mut Ui, label: &str, selected: &mut ColumnId, columns: &[ErdColumn]) {
    let name = columns
        .iter()
        .find(|column| column.id == *selected)
        .map_or("Missing column", |column| column.name.as_str());
    ui.label(label);
    egui::ComboBox::from_id_salt(label)
        .selected_text(name)
        .show_ui(ui, |ui| {
            for column in columns {
                ui.selectable_value(selected, column.id, &column.name);
            }
        });
}

fn reference_action(ui: &mut Ui, label: &str, selected: &mut Option<String>) {
    ui.label(label);
    egui::ComboBox::from_id_salt(label)
        .selected_text(selected.as_deref().unwrap_or("Default (NO ACTION)"))
        .show_ui(ui, |ui| {
            ui.selectable_value(selected, None, "Default (NO ACTION)");
            for action in [
                "NO ACTION",
                "RESTRICT",
                "CASCADE",
                "SET NULL",
                "SET DEFAULT",
            ] {
                ui.selectable_value(selected, Some(action.into()), action);
            }
        });
}

fn quote(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

#[cfg(test)]
mod tests {
    use super::*;
    use bp_model::DiagramKind;

    #[test]
    fn editing_fk_mapping_moves_row_ports_and_badges_and_undoes_together() {
        let mut app = BlueprintApp::new(&egui::Context::default(), None);
        app.choose_page_kind(DiagramKind::Erd);
        app.paste_sql_schema();
        app.sql.import.as_mut().unwrap().source = "CREATE TABLE parent (id integer PRIMARY KEY, other integer UNIQUE); CREATE TABLE child (old_id integer, new_id integer, CONSTRAINT first_fk FOREIGN KEY (old_id) REFERENCES parent(id));".into();
        app.preview_sql_import();
        assert!(app.apply_sql_import());
        let connector_id = app
            .doc
            .elements
            .values()
            .find(|e| e.as_connector().is_some())
            .unwrap()
            .id;
        let connector = app.doc.elements[&connector_id].as_connector().unwrap();
        let source = connector.source.element().unwrap();
        let target = connector.target.element().unwrap();
        let old_column = connector.foreign_key.as_ref().unwrap().columns[0];
        let new_column = app.doc.elements[&source]
            .as_shape()
            .unwrap()
            .erd
            .as_ref()
            .unwrap()
            .columns
            .iter()
            .find(|c| c.name == "new_id")
            .unwrap()
            .id;
        let target_column = app.doc.elements[&target]
            .as_shape()
            .unwrap()
            .erd
            .as_ref()
            .unwrap()
            .columns
            .iter()
            .find(|c| c.name == "other")
            .unwrap()
            .id;
        let mut key = connector.foreign_key.clone().unwrap();
        key.columns = vec![new_column];
        key.referenced_columns = vec![target_column];
        key.name = Some("edited_fk".into());
        key.on_delete = Some("CASCADE".into());
        let before = app.doc.clone();
        app.set_sql_foreign_key(connector_id, key.clone());
        assert_eq!(app.doc.validate(), Ok(()));
        let updated = app.doc.elements[&connector_id].as_connector().unwrap();
        assert_eq!(updated.foreign_key, Some(key));
        let column = |endpoint: &Endpoint| match endpoint {
            Endpoint::Glued {
                port: Some(port), ..
            } => port.column_id(),
            _ => None,
        };
        assert_eq!(column(&updated.source), Some(new_column));
        assert_eq!(column(&updated.target), Some(target_column));
        let table = app.doc.elements[&source]
            .as_shape()
            .unwrap()
            .erd
            .as_ref()
            .unwrap();
        assert!(!table.column(old_column).unwrap().foreign_key);
        assert!(table.column(new_column).unwrap().foreign_key);
        let after = app.doc.clone();
        app.undo();
        assert_eq!(app.doc, before);
        app.redo();
        assert_eq!(app.doc, after);
    }

    #[test]
    fn editing_fk_keeps_badge_for_column_used_by_another_constraint() {
        let mut app = BlueprintApp::new(&egui::Context::default(), None);
        app.choose_page_kind(DiagramKind::Erd);
        app.paste_sql_schema();
        app.sql.import.as_mut().unwrap().source = "CREATE TABLE parent (id integer PRIMARY KEY); CREATE TABLE child (old_id integer, new_id integer, CONSTRAINT first_fk FOREIGN KEY (old_id) REFERENCES parent(id), CONSTRAINT second_fk FOREIGN KEY (old_id) REFERENCES parent(id));".into();
        app.preview_sql_import();
        assert!(app.apply_sql_import());
        let element = app
            .doc
            .elements
            .values()
            .find(|e| e.as_connector().is_some())
            .unwrap();
        let id = element.id;
        let connector = element.as_connector().unwrap();
        let source = connector.source.element().unwrap();
        let old_column = connector.foreign_key.as_ref().unwrap().columns[0];
        let new_column = app.doc.elements[&source]
            .as_shape()
            .unwrap()
            .erd
            .as_ref()
            .unwrap()
            .columns
            .iter()
            .find(|c| c.name == "new_id")
            .unwrap()
            .id;
        let mut key = connector.foreign_key.clone().unwrap();
        key.columns = vec![new_column];
        app.set_sql_foreign_key(id, key);
        let table = app.doc.elements[&source]
            .as_shape()
            .unwrap()
            .erd
            .as_ref()
            .unwrap();
        assert!(table.column(old_column).unwrap().foreign_key);
        assert!(table.column(new_column).unwrap().foreign_key);
    }
}
