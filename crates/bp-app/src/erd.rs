//! Smart-table editing uses the same property commands as canvas editing.

use crate::app::BlueprintApp;
use bp_commands::{ColumnProp, Command, Prop, edit};
use bp_model::{
    ColumnId, Element, ElementId, Endpoint, ErdColumn, Marker, OrderKey, SqlDialect, TableDisplay,
};
use egui::{Button, Key, RichText, Ui};
use egui_phosphor::regular as icon;

impl BlueprintApp {
    /// Adds a typed column as one undo step. Column ids never change on edits.
    pub fn add_table_column(&mut self, id: ElementId) -> Option<ColumnId> {
        if self.doc.is_locked(id) {
            return None;
        }
        let table = self.doc.elements.get(&id)?.as_shape()?.erd.as_ref()?;
        let order = table
            .columns
            .iter()
            .map(|c| &c.order)
            .max()
            .map_or_else(OrderKey::first, OrderKey::after);
        let mut n = table.columns.len() + 1;
        while table
            .columns
            .iter()
            .any(|c| c.name == format!("column_{n}"))
        {
            n += 1;
        }
        let data_type = table
            .dialect
            .data_types()
            .first()
            .copied()
            .unwrap_or("INTEGER");
        let column = ErdColumn::new(format!("column_{n}"), data_type, order);
        let column_id = column.id;
        if self.apply(
            "Add column",
            [Command::InsertColumn {
                id,
                column: Box::new(column),
            }],
        ) {
            self.column_focus = Some((id, column_id));
            Some(column_id)
        } else {
            None
        }
    }

    pub fn set_table_column(&mut self, id: ElementId, column: ColumnId, prop: ColumnProp) {
        if self.doc.is_locked(id) {
            return;
        }
        let key = format!("column:{id}:{column}:{:?}", std::mem::discriminant(&prop));
        self.apply_merging(
            "Edit column",
            key,
            vec![Command::SetColumn { id, column, prop }],
        );
    }

    /// Deleting a row also deletes relationships attached to its ports.
    pub fn delete_table_column(&mut self, id: ElementId, column: ColumnId) {
        if self.doc.is_locked(id) {
            return;
        }
        let commands = edit::remove_column(&self.doc, id, column);
        if self.apply("Delete column", commands) {
            if self.column_focus == Some((id, column)) {
                self.column_focus = None;
            }
            self.prune_selection();
        }
    }

    /// Moves a column within its key/non-key section without changing its id.
    pub fn move_table_column(&mut self, id: ElementId, column: ColumnId, step: isize) {
        if self.doc.is_locked(id) {
            return;
        }
        let Some(table) = self
            .doc
            .elements
            .get(&id)
            .and_then(Element::as_shape)
            .and_then(|s| s.erd.as_ref())
        else {
            return;
        };
        let rows = table.columns_sorted();
        let Some(i) = rows.iter().position(|c| c.id == column) else {
            return;
        };
        let neighbor = match step {
            -1 if i > 0 => i - 1,
            1 if i + 1 < rows.len() => i + 1,
            _ => return,
        };
        if rows[neighbor].primary_key != rows[i].primary_key {
            return;
        }
        // Valid imported files may have tied keys. Re-key only that section
        // when there is no strict gap, rather than bisecting identical keys.
        let mut section: Vec<_> = rows
            .iter()
            .copied()
            .filter(|c| c.primary_key == rows[i].primary_key)
            .collect();
        if section
            .windows(2)
            .any(|pair| pair[0].order >= pair[1].order)
        {
            let from = section.iter().position(|c| c.id == column).unwrap();
            section.swap(from, (from as isize + step) as usize);
            let mut order = OrderKey::first();
            let commands: Vec<_> = section
                .into_iter()
                .map(|c| {
                    let command = Command::SetColumn {
                        id,
                        column: c.id,
                        prop: ColumnProp::Order(order.clone()),
                    };
                    order = OrderKey::after(&order);
                    command
                })
                .collect();
            self.apply("Move column", commands);
            return;
        }
        let order = match step {
            -1 if i > 0 && rows[i - 1].primary_key == rows[i].primary_key => {
                let below = i
                    .checked_sub(2)
                    .filter(|j| rows[*j].primary_key == rows[i].primary_key)
                    .map(|j| &rows[j].order);
                OrderKey::between(below, Some(&rows[i - 1].order))
            }
            1 if i + 1 < rows.len() && rows[i + 1].primary_key == rows[i].primary_key => {
                let above = rows
                    .get(i + 2)
                    .filter(|c| c.primary_key == rows[i].primary_key)
                    .map(|c| &c.order);
                OrderKey::between(Some(&rows[i + 1].order), above)
            }
            _ => return,
        };
        self.apply(
            "Move column",
            [Command::SetColumn {
                id,
                column,
                prop: ColumnProp::Order(order),
            }],
        );
    }

    /// Defaults for a new relationship drawn between table rows.
    /// Returns the referencing column to flag as FK in the connector's transaction.
    pub(crate) fn table_relationship(
        &self,
        source: &Endpoint,
        target: &Endpoint,
    ) -> Option<(Marker, Marker, ElementId, ColumnId)> {
        let row = |end: &Endpoint| {
            let Endpoint::Glued {
                element,
                port: Some(port),
            } = end
            else {
                return None;
            };
            let column = port.column_id()?;
            let table = self.doc.elements.get(element)?.as_shape()?.erd.as_ref()?;
            let data = table.columns.iter().find(|c| c.id == column)?;
            let unique = data.unique
                || (data.primary_key
                    && table.columns.iter().filter(|c| c.primary_key).count() == 1);
            Some((*element, data, unique))
        };
        let (source_id, source_column, source_unique) = row(source)?;
        let (target_id, target_column, target_unique) = row(target)?;
        if source_id == target_id && source_column.id == target_column.id {
            return None;
        }
        if target_column.primary_key && !self.doc.is_locked(source_id) {
            Some((
                if source_unique {
                    Marker::ZeroOrOne
                } else {
                    Marker::ZeroOrMany
                },
                if source_column.nullable {
                    Marker::ZeroOrOne
                } else {
                    Marker::ExactlyOne
                },
                source_id,
                source_column.id,
            ))
        } else if source_column.primary_key
            && !target_column.primary_key
            && !self.doc.is_locked(target_id)
        {
            Some((
                if target_column.nullable {
                    Marker::ZeroOrOne
                } else {
                    Marker::ExactlyOne
                },
                if target_unique {
                    Marker::ZeroOrOne
                } else {
                    Marker::ZeroOrMany
                },
                target_id,
                target_column.id,
            ))
        } else {
            None
        }
    }

    pub(crate) fn erd_table_inspector(&mut self, ui: &mut Ui, el: &Element) {
        let Some(shape) = el.as_shape() else {
            return;
        };
        let Some(table) = &shape.erd else {
            return;
        };
        let id = el.id;
        ui.label(RichText::new("Table name").strong());
        if self.editing.is_none() {
            let label = ui.label("Name");
            let mut name = shape.text.clone();
            let response = ui
                .add(
                    egui::TextEdit::singleline(&mut name)
                        .id_salt(("erd-name", id))
                        .desired_width(f32::INFINITY),
                )
                .labelled_by(label.id);
            if response.changed() {
                self.apply_merging(
                    "Rename table",
                    format!("text:{id}"),
                    vec![Command::Set {
                        id,
                        prop: Prop::Text(name),
                    }],
                );
            }
        }
        self.erd_sql_metadata_inspector(ui, el);
        ui.horizontal(|ui| {
            ui.label("SQL dialect");
            egui::ComboBox::from_id_salt(("dialect", id))
                .selected_text(table.dialect.label())
                .show_ui(ui, |ui| {
                    for dialect in SqlDialect::ALL {
                        if ui
                            .selectable_label(table.dialect == dialect, dialect.label())
                            .clicked()
                            && table.dialect != dialect
                        {
                            self.apply(
                                "Change SQL dialect",
                                [Command::Set {
                                    id,
                                    prop: Prop::SqlDialect(dialect),
                                }],
                            );
                        }
                    }
                });
        });
        ui.horizontal(|ui| {
            ui.label("Show");
            egui::ComboBox::from_id_salt(("table-display", id))
                .selected_text(table.display.label())
                .show_ui(ui, |ui| {
                    for display in TableDisplay::ALL {
                        if ui
                            .selectable_label(table.display == display, display.label())
                            .clicked()
                            && table.display != display
                        {
                            self.apply(
                                "Table display",
                                [Command::Set {
                                    id,
                                    prop: Prop::TableDisplay(display),
                                }],
                            );
                        }
                    }
                });
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Columns").strong());
            if ui.button("Add column").clicked() {
                self.add_table_column(id);
            }
        });
        let columns = table.columns_sorted();
        for (i, column) in columns.iter().enumerate() {
            ui.push_id(("column", id, column.id), |ui| {
                egui::Frame::group(ui.style())
                    .inner_margin(6)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(&column.name).strong());
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if row_button(
                                        ui,
                                        icon::TRASH,
                                        &format!("Delete column {}", column.name),
                                        true,
                                    ) {
                                        self.delete_table_column(id, column.id);
                                    }
                                    let down = columns
                                        .get(i + 1)
                                        .is_some_and(|c| c.primary_key == column.primary_key);
                                    if row_button(
                                        ui,
                                        icon::CARET_DOWN,
                                        &format!("Move column {} down", column.name),
                                        down,
                                    ) {
                                        self.move_table_column(id, column.id, 1);
                                    }
                                    let up =
                                        i > 0 && columns[i - 1].primary_key == column.primary_key;
                                    if row_button(
                                        ui,
                                        icon::CARET_UP,
                                        &format!("Move column {} up", column.name),
                                        up,
                                    ) {
                                        self.move_table_column(id, column.id, -1);
                                    }
                                },
                            );
                        });
                        let mut add = false;
                        egui::Grid::new("fields")
                            .num_columns(2)
                            .spacing([6.0, 4.0])
                            .show(ui, |ui| {
                                let label = ui.label("Name");
                                let mut name = column.name.clone();
                                let response = ui
                                    .add(
                                        egui::TextEdit::singleline(&mut name)
                                            .id_salt("name")
                                            .desired_width(140.0),
                                    )
                                    .labelled_by(label.id);
                                if self.column_focus == Some((id, column.id)) {
                                    response.request_focus();
                                    response.scroll_to_me(Some(egui::Align::Center));
                                    self.column_focus = None;
                                }
                                add |= (response.has_focus() || response.lost_focus())
                                    && ui.input(|i| i.key_pressed(Key::Enter));
                                if response.changed() {
                                    self.set_table_column(id, column.id, ColumnProp::Name(name));
                                }
                                ui.end_row();
                                let label = ui.label("Type");
                                let mut data_type = column.data_type.clone();
                                let response = ui
                                    .add(
                                        egui::TextEdit::singleline(&mut data_type)
                                            .id_salt("type")
                                            .desired_width(140.0),
                                    )
                                    .labelled_by(label.id);
                                add |= (response.has_focus() || response.lost_focus())
                                    && ui.input(|i| i.key_pressed(Key::Enter));
                                if response.changed() {
                                    self.set_table_column(
                                        id,
                                        column.id,
                                        ColumnProp::DataType(data_type),
                                    );
                                }
                                ui.end_row();
                                ui.label("Types");
                                egui::ComboBox::from_id_salt("type-presets")
                                    .selected_text("Choose type…")
                                    .show_ui(ui, |ui| {
                                        for data_type in table.dialect.data_types() {
                                            if ui
                                                .selectable_label(
                                                    column.data_type == *data_type,
                                                    *data_type,
                                                )
                                                .clicked()
                                                && column.data_type != *data_type
                                            {
                                                self.set_table_column(
                                                    id,
                                                    column.id,
                                                    ColumnProp::DataType((*data_type).into()),
                                                );
                                            }
                                        }
                                    });
                                ui.end_row();
                                let label = ui.label("Default");
                                let mut default = column.default_value.clone().unwrap_or_default();
                                let response = ui
                                    .add(
                                        egui::TextEdit::singleline(&mut default)
                                            .id_salt("default")
                                            .hint_text("None")
                                            .desired_width(ui.available_width()),
                                    )
                                    .labelled_by(label.id);
                                if response.changed() {
                                    self.set_table_column(
                                        id,
                                        column.id,
                                        ColumnProp::DefaultValue(
                                            (!default.trim().is_empty()).then_some(default),
                                        ),
                                    );
                                }
                                ui.end_row();
                            });
                        ui.horizontal_wrapped(|ui| {
                            let mut pk = column.primary_key;
                            if ui
                                .checkbox(&mut pk, "PK")
                                .on_hover_text("Primary key")
                                .changed()
                            {
                                self.set_table_column(id, column.id, ColumnProp::PrimaryKey(pk));
                            }
                            let mut fk = column.foreign_key;
                            if ui
                                .checkbox(&mut fk, "FK")
                                .on_hover_text("Foreign key")
                                .changed()
                            {
                                self.set_table_column(id, column.id, ColumnProp::ForeignKey(fk));
                            }
                            let mut unique = column.unique;
                            if ui
                                .checkbox(&mut unique, "UK")
                                .on_hover_text("Unique key")
                                .changed()
                            {
                                self.set_table_column(id, column.id, ColumnProp::Unique(unique));
                            }
                            let mut required = !column.nullable;
                            if ui.checkbox(&mut required, "NOT NULL").changed() {
                                self.set_table_column(
                                    id,
                                    column.id,
                                    ColumnProp::Nullable(!required),
                                );
                            }
                        });
                        if add {
                            self.add_table_column(id);
                        }
                    });
            });
        }
        ui.label(RichText::new("Drag a row port to another table’s primary key to create a relationship. Enter adds a column; Tab moves between fields.").small().weak());
        ui.add_space(6.0);
    }
}

fn row_button(ui: &mut Ui, glyph: &str, label: &str, enabled: bool) -> bool {
    let enabled = enabled && ui.is_enabled();
    let response = ui.add_enabled(enabled, Button::new(crate::theme::icon(glyph)).small());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
    response.on_hover_text(label).clicked()
}

#[cfg(test)]
mod tests {
    use super::*;
    use bp_model::kurbo::Rect;
    use bp_model::{Dash, PortId, ShapeRef};

    fn table(app: &mut BlueprintApp, x: f64) -> ElementId {
        app.insert_shape(
            ShapeRef::new("erd", "table"),
            Rect::new(x, 0.0, x + 280.0, 160.0),
        )
        .unwrap()
    }

    #[test]
    fn reordering_imported_columns_with_equal_keys_is_undoable() {
        let mut app = BlueprintApp::new(&egui::Context::default(), None);
        let id = table(&mut app, 0.0);
        let rows: Vec<_> = ["a", "b", "c"]
            .into_iter()
            .map(|name| ErdColumn::new(name, "TEXT", OrderKey::first()))
            .collect();
        app.apply(
            "Add rows",
            rows.into_iter().map(|column| Command::InsertColumn {
                id,
                column: Box::new(column),
            }),
        );
        let before = app.doc.clone();
        let mut order: Vec<_> = app.doc.elements[&id]
            .as_shape()
            .unwrap()
            .erd
            .as_ref()
            .unwrap()
            .columns_sorted()
            .into_iter()
            .map(|c| c.id)
            .collect();
        app.move_table_column(id, order[1], 1);
        order.swap(1, 2);
        let actual: Vec<_> = app.doc.elements[&id]
            .as_shape()
            .unwrap()
            .erd
            .as_ref()
            .unwrap()
            .columns_sorted()
            .into_iter()
            .map(|c| c.id)
            .collect();
        assert_eq!(actual, order);
        assert_eq!(app.doc.validate(), Ok(()));
        let after = app.doc.clone();
        app.undo();
        assert_eq!(app.doc, before);
        app.redo();
        assert_eq!(app.doc, after);
    }

    #[test]
    fn primary_key_relationships_are_identifying_and_undo_the_fk_flag() {
        let mut app = BlueprintApp::new(&egui::Context::default(), None);
        let a = table(&mut app, 0.0);
        let b = table(&mut app, 400.0);
        let column = |app: &BlueprintApp, id| {
            app.doc.elements[&id]
                .as_shape()
                .unwrap()
                .erd
                .as_ref()
                .unwrap()
                .columns[0]
                .id
        };
        let source = column(&app, a);
        let target = column(&app, b);
        let before = app.doc.clone();
        let id = app
            .insert_connector(
                Endpoint::glued(a, Some(PortId::column(source, false).as_str())),
                Endpoint::glued(b, Some(PortId::column(target, true).as_str())),
            )
            .unwrap();
        let connector = app.doc.elements[&id].as_connector().unwrap();
        assert_eq!(connector.style.dash, Some(Dash::Solid));
        assert_eq!(
            (connector.start_marker, connector.end_marker),
            (Marker::ZeroOrOne, Marker::ExactlyOne)
        );
        assert!(
            app.doc.elements[&a]
                .as_shape()
                .unwrap()
                .erd
                .as_ref()
                .unwrap()
                .column(source)
                .unwrap()
                .foreign_key
        );
        app.undo();
        assert_eq!(app.doc, before);
        app.redo();
        assert_eq!(app.doc.validate(), Ok(()));
    }
}
