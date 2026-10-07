//! ERD selection emphasis is derived from the document, never stored in it.

use crate::app::BlueprintApp;
use crate::theme::ACCENT;
use bp_model::{Color, ColumnId, DiagramKind, ElementId, Endpoint};
use bp_render_egui::ElementPresentation;
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// Tables, relationships and stable column IDs emphasized by the current selection.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ErdFocus {
    pub active: bool,
    pub tables: BTreeSet<ElementId>,
    pub connections: BTreeSet<ElementId>,
    pub columns: BTreeMap<ElementId, BTreeSet<ColumnId>>,
}

impl BlueprintApp {
    /// Whether the page has visible, editable smart tables to arrange.
    pub fn can_auto_arrange_erd(&self) -> bool {
        self.page_kind() == Some(DiagramKind::Erd)
            && self.doc.paint_order(self.page).iter().any(|element| {
                !self.doc.is_locked(element.id)
                    && element.as_shape().is_some_and(|shape| shape.erd.is_some())
            })
    }

    /// Rearranges the whole visible ERD in one undo step, then fits it on screen.
    pub fn auto_arrange_erd(&mut self) -> bool {
        if !self.can_auto_arrange_erd() {
            return false;
        }
        self.finish_inline_edits();
        self.cancel_drag();
        self.history.commit();
        let commands = match bp_sql::arrange_page(&self.doc, self.page) {
            Ok(commands) => commands,
            Err(error) => {
                self.status = format!("Auto-arrange ERD failed: {error}");
                return false;
            }
        };
        if !commands.is_empty() && !self.apply("Auto-arrange ERD", commands) {
            return false;
        }
        self.history.commit();
        self.fit_requested = true;
        self.status = "Arranged visible ERD tables; locked tables stay in place".into();
        true
    }

    pub fn erd_focus(&self) -> ErdFocus {
        if self.page_kind() != Some(DiagramKind::Erd) {
            return ErdFocus::default();
        }
        let visible = self.doc.paint_order(self.page);
        let table_ids: BTreeSet<_> = visible
            .iter()
            .filter_map(|element| element.as_shape()?.erd.as_ref().map(|_| element.id))
            .collect();
        let mut selected: BTreeSet<_> = self.selection.iter().copied().collect();
        for id in &self.selection {
            selected.extend(self.doc.descendants(*id));
        }
        let selected_tables: BTreeSet<_> = table_ids.intersection(&selected).copied().collect();
        let mut focus = ErdFocus {
            active: !selected_tables.is_empty(),
            tables: selected_tables.clone(),
            ..ErdFocus::default()
        };
        for element in visible {
            let Some(connector) = element.as_connector() else {
                continue;
            };
            let source = connector
                .source
                .element()
                .filter(|id| table_ids.contains(id));
            let target = connector
                .target
                .element()
                .filter(|id| table_ids.contains(id));
            if source.is_none() && target.is_none() {
                continue;
            }
            let selected_connection = selected.contains(&element.id);
            if !selected_connection
                && !source.is_some_and(|id| selected_tables.contains(&id))
                && !target.is_some_and(|id| selected_tables.contains(&id))
            {
                continue;
            }
            focus.active = true;
            focus.connections.insert(element.id);
            focus.tables.extend(source);
            focus.tables.extend(target);
            if selected_connection {
                if let Some(key) = &connector.foreign_key {
                    if let Some((owner, referenced)) = connector.foreign_key_endpoints() {
                        for (endpoint, columns) in
                            [(owner, &key.columns), (referenced, &key.referenced_columns)]
                        {
                            if let Some(id) = endpoint.element().filter(|id| table_ids.contains(id))
                            {
                                focus.columns.entry(id).or_default().extend(columns);
                            }
                        }
                    }
                } else {
                    for endpoint in [&connector.source, &connector.target] {
                        if let Endpoint::Glued {
                            element,
                            port: Some(port),
                        } = endpoint
                            && table_ids.contains(element)
                            && let Some(column) = port.column_id()
                        {
                            focus.columns.entry(*element).or_default().insert(column);
                        }
                    }
                }
            }
        }
        focus
    }

    pub(crate) fn erd_presentation(
        &self,
        focus: &ErdFocus,
    ) -> HashMap<ElementId, ElementPresentation> {
        if !focus.active {
            return HashMap::new();
        }
        let accent = Color::rgb(ACCENT.r(), ACCENT.g(), ACCENT.b());
        let mut presentation = HashMap::new();
        for element in self.doc.paint_order(self.page) {
            if element.as_connector().is_some() {
                let related = focus.connections.contains(&element.id);
                presentation.insert(
                    element.id,
                    ElementPresentation {
                        opacity: if related { 1.0 } else { 0.22 },
                        accent: related.then_some(accent),
                        ..ElementPresentation::default()
                    },
                );
            }
        }
        for (id, columns) in &focus.columns {
            if let Some(table) = self.scene.shape(*id).and_then(|shape| shape.erd.as_ref()) {
                let backgrounds = table
                    .rows
                    .iter()
                    .filter(|row| columns.contains(&row.column))
                    .map(|row| (row.bounds.inset(-1.0), accent.faded(0.13)))
                    .collect();
                presentation.insert(
                    *id,
                    ElementPresentation {
                        text_backgrounds: backgrounds,
                        ..ElementPresentation::default()
                    },
                );
            }
        }
        presentation
    }

    /// Complete ordered mapping using semantic FK ownership, including swapped ends.
    pub fn foreign_key_mapping(&self, id: ElementId) -> Vec<String> {
        let Some(connector) = self
            .doc
            .elements
            .get(&id)
            .and_then(|element| element.as_connector())
        else {
            return Vec::new();
        };
        let Some(key) = &connector.foreign_key else {
            return Vec::new();
        };
        let Some((owner, referenced)) = connector.foreign_key_endpoints() else {
            return Vec::new();
        };
        let column_name = |endpoint: &Endpoint, column| -> Option<String> {
            let shape = self.doc.elements.get(&endpoint.element()?)?.as_shape()?;
            let table = shape.erd.as_ref()?;
            let mut names = table.schema.clone();
            names.push(shape.text.clone());
            names.push(table.column(column)?.name.clone());
            Some(names.join("."))
        };
        key.columns
            .iter()
            .zip(&key.referenced_columns)
            .filter_map(|(source, target)| {
                Some(format!(
                    "{} → {}",
                    column_name(owner, *source)?,
                    column_name(referenced, *target)?
                ))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bp_commands::{Command, Prop};
    use bp_model::SqlDialect;

    fn imported() -> BlueprintApp {
        let mut app = BlueprintApp::new(&egui::Context::default(), None);
        app.choose_page_kind(DiagramKind::Erd);
        app.paste_sql_schema();
        app.sql.import.as_mut().unwrap().source = "CREATE TABLE parent (tenant integer, id integer, PRIMARY KEY (tenant,id)); CREATE TABLE child (tenant integer, parent_id integer, FOREIGN KEY (tenant,parent_id) REFERENCES parent(tenant,id)); CREATE TABLE other (id integer PRIMARY KEY); CREATE TABLE unrelated (other_id integer REFERENCES other(id));".into();
        app.preview_sql_import();
        assert!(app.apply_sql_import());
        app.refresh_scene();
        app.selection.clear();
        app
    }

    #[test]
    fn selected_table_focuses_neighbors_and_only_fades_unrelated_connections() {
        let mut app = imported();
        let child = app
            .doc
            .elements
            .values()
            .find(|e| e.text() == Some("child"))
            .unwrap()
            .id;
        app.selection = vec![child];
        let before = app.doc.clone();
        let focus = app.erd_focus();
        assert_eq!(focus.tables.len(), 2);
        assert_eq!(focus.connections.len(), 1);
        assert!(focus.columns.is_empty());
        let presentation = app.erd_presentation(&focus);
        assert_eq!(presentation.len(), 2);
        assert!(presentation.values().any(|item| item.opacity == 0.22));
        assert_eq!(app.doc, before);
        app.selection.clear();
        assert!(app.erd_presentation(&app.erd_focus()).is_empty());
    }

    #[test]
    fn hidden_tables_are_not_focused_and_locked_tables_disable_arrange() {
        let mut app = imported();
        let commands: Vec<_> = app
            .doc
            .elements
            .values()
            .filter(|element| element.as_shape().is_some())
            .map(|element| Command::Set {
                id: element.id,
                prop: Prop::Locked(true),
            })
            .collect();
        app.apply("Lock tables", commands);
        let before = app.doc.clone();
        assert!(!app.can_auto_arrange_erd());
        assert!(!app.auto_arrange_erd());
        assert_eq!(app.doc, before);
        app.doc.layers.get_mut(&app.layer).unwrap().visible = false;
        app.selection = app.doc.elements.keys().copied().collect();
        assert_eq!(app.erd_focus(), ErdFocus::default());
    }

    #[test]
    fn composite_rows_and_mapping_follow_fk_ownership_after_swapping_visual_ends() {
        let mut app = imported();
        let id = app
            .doc
            .elements
            .values()
            .find(|e| {
                e.as_connector().is_some_and(|c| {
                    c.foreign_key
                        .as_ref()
                        .is_some_and(|key| key.columns.len() == 2)
                })
            })
            .unwrap()
            .id;
        app.selection = vec![id];
        let before = app.doc.clone();
        let focus = app.erd_focus();
        assert_eq!(focus.columns.len(), 2);
        assert!(focus.columns.values().all(|columns| columns.len() == 2));
        assert_eq!(
            app.foreign_key_mapping(id),
            [
                "child.tenant → parent.tenant",
                "child.parent_id → parent.id"
            ]
        );
        let presentation = app.erd_presentation(&focus);
        for table in focus.columns.keys() {
            let entry = &presentation[table];
            assert_eq!(entry.opacity, 1.0);
            assert!(
                entry.accent.is_none(),
                "table text keeps its original color"
            );
            assert_eq!(entry.text_backgrounds.len(), 2);
        }
        assert_eq!(app.doc, before);
        let connector = app.doc.elements[&id].as_connector().unwrap().clone();
        let mut key = connector.foreign_key.clone().unwrap();
        key.owner_at_target = true;
        app.apply(
            "Swap visual ends",
            [
                Command::Set {
                    id,
                    prop: Prop::ForeignKey(None),
                },
                Command::Set {
                    id,
                    prop: Prop::Source(connector.target),
                },
                Command::Set {
                    id,
                    prop: Prop::Target(connector.source),
                },
                Command::Set {
                    id,
                    prop: Prop::ForeignKey(Some(key)),
                },
            ],
        );
        assert_eq!(app.erd_focus(), focus);
        assert_eq!(
            app.foreign_key_mapping(id),
            [
                "child.tenant → parent.tenant",
                "child.parent_id → parent.id"
            ]
        );
        let sql = bp_sql::export_page(&app.doc, app.page, SqlDialect::PostgreSql).unwrap();
        assert!(sql.sql.contains("FOREIGN KEY (\"tenant\", \"parent_id\")"));
    }

    #[test]
    fn arranging_is_one_undo_step_requests_fit_and_preserves_sql() {
        let mut app = imported();
        let tables: Vec<_> = app
            .doc
            .elements
            .values()
            .filter(|e| e.as_shape().is_some())
            .map(|e| e.id)
            .collect();
        app.apply(
            "Move tables",
            tables.iter().enumerate().map(|(i, id)| Command::Set {
                id: *id,
                prop: Prop::Bounds(bp_model::kurbo::Rect::new(
                    1000.0 + i as f64 * 30.0,
                    200.0,
                    1280.0 + i as f64 * 30.0,
                    400.0,
                )),
            }),
        );
        app.history.commit();
        let before = app.doc.clone();
        let sql_before = bp_sql::export_page(&before, app.page, SqlDialect::PostgreSql).unwrap();
        app.fit_requested = false;
        assert!(app.can_auto_arrange_erd());
        assert!(app.auto_arrange_erd());
        assert!(app.fit_requested);
        assert_eq!(app.history.undo_label(), Some("Auto-arrange ERD"));
        let after = app.doc.clone();
        assert_ne!(after, before);
        assert_eq!(
            bp_sql::export_page(&after, app.page, SqlDialect::PostgreSql).unwrap(),
            sql_before
        );
        app.undo();
        assert_eq!(app.doc, before);
        app.redo();
        assert_eq!(app.doc, after);
    }
}
