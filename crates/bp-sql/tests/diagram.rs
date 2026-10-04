use bp_commands::{ColumnProp, Command, History, Prop, edit};
use bp_model::kurbo::{Rect, Vec2};
use bp_model::{
    Document, Element, ElementId, Endpoint, ErdColumn, ErdKey, Marker, OrderKey, Parent, PortId,
    ShapeRef, SqlDialect,
};
use bp_sql::{
    bind_index_expression, current_index_expression, export_page, import_commands, parse,
};

const COMPOSITE: &str = r#"
CREATE TABLE "Sales"."Customer" (
    "Tenant" integer NOT NULL,
    "Id" bigint NOT NULL,
    "Display Name" text DEFAULT 'guest',
    CONSTRAINT "Customer PK" PRIMARY KEY ("Tenant", "Id"),
    CONSTRAINT "Customer Name" UNIQUE ("Tenant", "Display Name")
);
CREATE TABLE "Sales"."Invoice" (
    tenant integer NOT NULL,
    customer bigint NOT NULL,
    invoice_id bigint PRIMARY KEY,
    amount numeric(12, 2) DEFAULT 0
);
ALTER TABLE "Sales"."Invoice" ADD CONSTRAINT "Invoice Customer"
    FOREIGN KEY (tenant, customer) REFERENCES "Sales"."Customer" ("Tenant", "Id")
    ON DELETE CASCADE ON UPDATE RESTRICT;
CREATE INDEX "Invoice Amount" ON "Sales"."Invoice" USING btree (amount DESC) WHERE amount > 0;
"#;

fn imported(sql: &str) -> (Document, History, Parent) {
    let mut doc = Document::new();
    let parent = Parent::Layer(doc.layers_of(doc.first_page().unwrap())[0].id);
    let preview = parse(sql, SqlDialect::PostgreSql).unwrap();
    assert!(preview.warnings.is_empty(), "{:?}", preview.warnings);
    let (_, commands) = import_commands(&doc, parent, &preview).unwrap();
    let mut history = History::new();
    history.apply(&mut doc, "Import SQL", commands).unwrap();
    (doc, history, parent)
}

fn table_id(doc: &Document, name: &str) -> ElementId {
    doc.elements
        .values()
        .find(|element| element.as_shape().is_some_and(|shape| shape.text == name))
        .unwrap()
        .id
}

#[test]
fn import_is_one_undo_step_and_metadata_survives_native_files() {
    let mut doc = Document::new();
    let original = doc.clone();
    let page = doc.first_page().unwrap();
    let parent = Parent::Layer(doc.layers_of(page)[0].id);
    let preview = parse(COMPOSITE, SqlDialect::PostgreSql).unwrap();
    let (ids, commands) = import_commands(&doc, parent, &preview).unwrap();
    assert_eq!(ids.len(), 3);
    let mut history = History::new();
    history.apply(&mut doc, "Import SQL", commands).unwrap();
    let imported = doc.clone();
    assert!(history.undo(&mut doc));
    assert_eq!(doc, original);
    assert!(!history.undo(&mut doc));
    assert!(history.redo(&mut doc));
    assert_eq!(doc, imported);
    assert_eq!(
        bp_io::from_bytes(&bp_io::to_json_bytes(&doc).unwrap()).unwrap(),
        doc
    );
    assert_eq!(
        bp_io::from_bytes(&bp_io::to_zip_bytes(&doc).unwrap()).unwrap(),
        doc
    );
    let foreign_key = doc
        .elements
        .values()
        .find_map(|element| element.as_connector())
        .unwrap()
        .foreign_key
        .as_ref()
        .unwrap();
    assert_eq!(foreign_key.columns.len(), 2);
    assert_eq!(foreign_key.name.as_deref(), Some("Invoice Customer"));
    assert_eq!(foreign_key.on_delete.as_deref(), Some("CASCADE"));
}

#[test]
fn imported_columns_remain_editable_and_constraints_follow_renames() {
    let (mut doc, mut history, _) = imported(COMPOSITE);
    let id = table_id(&doc, "Invoice");
    let table = doc.elements[&id].as_shape().unwrap().erd.as_ref().unwrap();
    let column = table
        .columns
        .iter()
        .find(|column| column.name == "amount")
        .unwrap()
        .id;
    let local = table
        .columns
        .iter()
        .find(|column| column.name == "customer")
        .unwrap()
        .id;
    history
        .apply(
            &mut doc,
            "Edit columns",
            [
                Command::SetColumn {
                    id,
                    column,
                    prop: ColumnProp::Name("Total \"USD\"".into()),
                },
                Command::SetColumn {
                    id,
                    column,
                    prop: ColumnProp::DefaultValue(Some("42.5".into())),
                },
                Command::SetColumn {
                    id,
                    column: local,
                    prop: ColumnProp::Name("customer_id".into()),
                },
            ],
        )
        .unwrap();
    let export = export_page(&doc, doc.first_page().unwrap(), SqlDialect::PostgreSql).unwrap();
    assert!(export.warnings.is_empty(), "{:?}", export.warnings);
    assert!(export.sql.contains("\"Total \"\"USD\"\"\" DESC"));
    assert!(export.sql.contains("WHERE \"Total \"\"USD\"\"\" > 0"));
    assert!(
        export
            .sql
            .contains("FOREIGN KEY (\"tenant\", \"customer_id\")")
    );
    let parsed = parse(&export.sql, SqlDialect::PostgreSql).unwrap();
    assert!(parsed.warnings.is_empty(), "{:?}", parsed.warnings);
    assert_eq!(
        parsed.schema.tables[1].columns[3].default_value.as_deref(),
        Some("42.5")
    );
}

#[test]
fn deleting_second_composite_fk_column_removes_relation_and_undo_restores_it() {
    let (mut doc, mut history, _) = imported(COMPOSITE);
    let before = doc.clone();
    let id = table_id(&doc, "Invoice");
    let connector = doc
        .elements
        .values()
        .find(|element| element.is_connector())
        .unwrap()
        .id;
    let second = doc.elements[&connector]
        .as_connector()
        .unwrap()
        .foreign_key
        .as_ref()
        .unwrap()
        .columns[1];
    // Only the first composite column has a visible connector port.
    assert!(
        doc.connectors_attached_to_column(id, second)
            .contains(&connector)
    );
    let commands = edit::remove_column(&doc, id, second);
    history
        .apply(&mut doc, "Delete FK column", commands)
        .unwrap();
    assert!(!doc.elements.contains_key(&connector));
    assert_eq!(doc.validate(), Ok(()));
    let after = doc.clone();
    assert!(history.undo(&mut doc));
    assert_eq!(doc, before);
    assert!(history.redo(&mut doc));
    assert_eq!(doc, after);
}

#[test]
fn column_deletion_drops_affected_keys_indexes_and_restores_all_metadata() {
    let (mut doc, mut history, _) = imported(COMPOSITE);
    let before = doc.clone();
    let id = table_id(&doc, "Customer");
    let column = doc.elements[&id]
        .as_shape()
        .unwrap()
        .erd
        .as_ref()
        .unwrap()
        .columns[0]
        .id;
    history
        .apply(
            &mut doc,
            "Delete key column",
            edit::remove_column(&before, id, column),
        )
        .unwrap();
    let table = doc.elements[&id].as_shape().unwrap().erd.as_ref().unwrap();
    assert!(table.primary_key.is_none());
    assert!(table.unique_keys.is_empty());
    assert!(table.columns.iter().all(|column| !column.primary_key));
    assert!(history.undo(&mut doc));
    assert_eq!(doc, before);
    let id = table_id(&doc, "Invoice");
    let column = doc.elements[&id]
        .as_shape()
        .unwrap()
        .erd
        .as_ref()
        .unwrap()
        .columns[3]
        .id;
    let commands = edit::remove_column(&doc, id, column);
    history
        .apply(&mut doc, "Delete indexed column", commands)
        .unwrap();
    assert!(
        doc.elements[&id]
            .as_shape()
            .unwrap()
            .erd
            .as_ref()
            .unwrap()
            .indexes
            .is_empty()
    );
    assert!(history.undo(&mut doc));
    assert_eq!(doc, before);
}

#[test]
fn sql_semantic_round_trip_preserves_quoted_composite_keys_indexes_and_actions() {
    let original = parse(COMPOSITE, SqlDialect::PostgreSql).unwrap();
    let (doc, _, _) = imported(COMPOSITE);
    let output = export_page(&doc, doc.first_page().unwrap(), SqlDialect::PostgreSql).unwrap();
    assert!(output.warnings.is_empty(), "{:?}", output.warnings);
    let reparsed = parse(&output.sql, SqlDialect::PostgreSql).unwrap();
    assert!(reparsed.warnings.is_empty(), "{:?}", reparsed.warnings);
    assert_eq!(reparsed.schema, original.schema);
    let (second, _, _) = imported(&output.sql);
    assert_eq!(
        export_page(
            &second,
            second.first_page().unwrap(),
            SqlDialect::PostgreSql
        )
        .unwrap(),
        output
    );
}

#[test]
fn export_defers_cyclic_foreign_keys_until_tables_and_indexes_exist() {
    let (doc, _, _) = imported(
        "CREATE TABLE a(id integer PRIMARY KEY, b integer); CREATE TABLE b(id integer PRIMARY KEY, a integer); ALTER TABLE a ADD FOREIGN KEY(b) REFERENCES b(id); ALTER TABLE b ADD FOREIGN KEY(a) REFERENCES a(id);",
    );
    let output = export_page(&doc, doc.first_page().unwrap(), SqlDialect::PostgreSql).unwrap();
    assert!(output.warnings.is_empty(), "{:?}", output.warnings);
    assert!(output.sql.rfind("CREATE TABLE").unwrap() < output.sql.find("ALTER TABLE").unwrap());
    assert_eq!(
        parse(&output.sql, SqlDialect::PostgreSql)
            .unwrap()
            .schema
            .foreign_keys
            .len(),
        2
    );
}

#[test]
fn references_can_permute_composite_unique_keys_and_use_ordered_unique_indexes() {
    let (doc, _, _) = imported(
        "CREATE TABLE parent(a integer, b integer, c integer, UNIQUE(a,b)); CREATE UNIQUE INDEX parent_c ON parent(c DESC NULLS FIRST); CREATE TABLE child(x integer,y integer,z integer, FOREIGN KEY(x,y) REFERENCES parent(b,a),FOREIGN KEY(z) REFERENCES parent(c));",
    );
    let output = export_page(&doc, doc.first_page().unwrap(), SqlDialect::PostgreSql).unwrap();
    assert!(output.warnings.is_empty(), "{:?}", output.warnings);
    assert_eq!(
        parse(&output.sql, SqlDialect::PostgreSql)
            .unwrap()
            .schema
            .foreign_keys
            .len(),
        2
    );
    assert!(
        output.sql.find("CREATE UNIQUE INDEX").unwrap() < output.sql.find("ALTER TABLE").unwrap()
    );
}

#[test]
fn inferred_relationships_support_both_drawing_directions_and_hidden_tables() {
    let (mut doc, mut history, parent) = imported(
        "CREATE TABLE parent(id integer PRIMARY KEY); CREATE TABLE child(id integer PRIMARY KEY,parent_id integer);",
    );
    let parent_id = table_id(&doc, "parent");
    let child_id = table_id(&doc, "child");
    let parent_column = doc.elements[&parent_id]
        .as_shape()
        .unwrap()
        .erd
        .as_ref()
        .unwrap()
        .columns[0]
        .id;
    let child_column = doc.elements[&child_id]
        .as_shape()
        .unwrap()
        .erd
        .as_ref()
        .unwrap()
        .columns[1]
        .id;
    let relationship = Element::connector(
        Endpoint::Glued {
            element: parent_id,
            port: Some(PortId::column(parent_column, false)),
        },
        Endpoint::Glued {
            element: child_id,
            port: Some(PortId::column(child_column, true)),
        },
        parent,
        doc.next_order_key(parent),
    );
    history
        .apply(
            &mut doc,
            "Draw relationship",
            [Command::Insert(Box::new(relationship))],
        )
        .unwrap();
    doc.layers.values_mut().next().unwrap().visible = false;
    let output = export_page(&doc, doc.first_page().unwrap(), SqlDialect::PostgreSql).unwrap();
    assert!(output.sql.contains(
        "ALTER TABLE \"child\" ADD FOREIGN KEY (\"parent_id\") REFERENCES \"parent\" (\"id\")"
    ));
    assert!(
        output
            .warnings
            .iter()
            .any(|warning| warning.message.contains("inferred"))
    );
}

#[test]
fn copy_paste_keeps_composite_fk_and_index_bindings_consistent() {
    let (mut doc, mut history, parent) = imported(COMPOSITE);
    let ids = [table_id(&doc, "Customer"), table_id(&doc, "Invoice")];
    let original_fk = doc
        .elements
        .values()
        .find_map(|element| element.as_connector())
        .unwrap()
        .foreign_key
        .clone();
    let clip = edit::Clip::copy(&doc, &ids, |_, _| None);
    let (pasted, commands) = clip.paste(&doc, parent, Vec2::new(100.0, 100.0));
    history.apply(&mut doc, "Paste", commands).unwrap();
    assert_eq!(doc.validate(), Ok(()));
    let connector = pasted
        .iter()
        .find_map(|id| doc.elements[id].as_connector())
        .unwrap();
    assert_eq!(connector.foreign_key, original_fk);
    assert!(
        connector
            .endpoints()
            .iter()
            .all(|endpoint| pasted.contains(&endpoint.element().unwrap()))
    );
    let original_connector = doc
        .elements
        .values()
        .find(|element| {
            element
                .as_connector()
                .is_some_and(|connector| connector.source.element() == Some(ids[1]))
        })
        .unwrap()
        .id;
    let partial = edit::Clip::copy(&doc, &[original_connector, ids[1]], |_, _| None);
    assert!(
        partial
            .elements
            .iter()
            .find_map(|element| element.as_connector())
            .unwrap()
            .foreign_key
            .is_none()
    );
}

#[test]
fn table_metadata_sets_form_coalescing_barriers_and_undo_exactly() {
    let (mut doc, mut history, _) =
        imported("CREATE TABLE t(a integer,b integer,PRIMARY KEY(b,a));");
    let before = doc.clone();
    let id = table_id(&doc, "t");
    let mut table = doc.elements[&id]
        .as_shape()
        .unwrap()
        .erd
        .as_ref()
        .unwrap()
        .clone();
    let column = table.columns[0].id;
    table.columns[0].name = "first".into();
    table.schema = vec!["edited".into()];
    history
        .apply(
            &mut doc,
            "Mixed edits",
            [
                Command::SetColumn {
                    id,
                    column,
                    prop: ColumnProp::Name("first".into()),
                },
                Command::Set {
                    id,
                    prop: Prop::ErdTable(Box::new(table)),
                },
                Command::SetColumn {
                    id,
                    column,
                    prop: ColumnProp::Name("last".into()),
                },
                Command::SetColumn {
                    id,
                    column,
                    prop: ColumnProp::PrimaryKey(true),
                },
            ],
        )
        .unwrap();
    let after = doc.clone();
    let table = doc.elements[&id].as_shape().unwrap().erd.as_ref().unwrap();
    assert_eq!(
        table.primary_key.as_ref().unwrap().columns,
        [table.columns[1].id, table.columns[0].id]
    );
    assert!(history.undo(&mut doc));
    assert_eq!(doc, before);
    assert!(history.redo(&mut doc));
    assert_eq!(doc, after);
}

#[test]
fn index_binding_skips_literals_functions_casts_and_preserves_unicode_spans() {
    let mut columns = vec![
        ErdColumn::new("date", "date", OrderKey::first()),
        ErdColumn::new("lower", "text", OrderKey::first()),
        ErdColumn::new("text", "text", OrderKey::first()),
        ErdColumn::new("名", "text", OrderKey::first()),
    ];
    let source = r#"lower("名") || $tag$date 名$tag$ || E'date\n名' || 'date''名' || date::text /* date 名 */"#;
    let bound = bind_index_expression(source, &columns);
    assert_eq!(bound.references.len(), 2);
    assert_eq!(current_index_expression(&bound, &columns), source);
    columns[0].name = "created_at".into();
    columns[3].name = "新名".into();
    let rendered = current_index_expression(&bound, &columns);
    assert!(rendered.contains("lower(\"新名\")"));
    assert!(rendered.contains("\"created_at\"::text"));
    assert!(rendered.contains("$tag$date 名$tag$"));
    assert!(rendered.contains("/* date 名 */"));
}

#[test]
fn index_binding_excludes_collations_operator_classes_and_cast_type_names() {
    let mut columns = ["name", "C", "text_pattern_ops", "mytype"]
        .map(|name| ErdColumn::new(name, "text", OrderKey::first()));
    for expression in [
        r#"name COLLATE "C""#,
        "name text_pattern_ops DESC",
        "CAST(name AS mytype)",
    ] {
        let bound = bind_index_expression(expression, &columns);
        assert_eq!(
            bound.references.len(),
            1,
            "{expression}: {:?}",
            bound.references
        );
        columns[0].name = "renamed".into();
        let rendered = current_index_expression(&bound, &columns);
        assert_eq!(rendered, expression.replacen("name", "\"renamed\"", 1));
        columns[0].name = "name".into();
    }
}

#[test]
fn visual_end_swapping_preserves_foreign_key_sql_and_column_deletion() {
    let (mut doc, mut history, _) = imported(COMPOSITE);
    let before = doc.clone();
    let sql = export_page(&doc, doc.first_page().unwrap(), SqlDialect::PostgreSql).unwrap();
    let element = doc
        .elements
        .values()
        .find(|element| element.is_connector())
        .unwrap()
        .clone();
    let id = element.id;
    let connector = element.as_connector().unwrap();
    let mut key = connector.foreign_key.clone().unwrap();
    key.owner_at_target = true;
    history
        .apply(
            &mut doc,
            "Swap ends",
            [
                Command::Set {
                    id,
                    prop: Prop::ForeignKey(None),
                },
                Command::Set {
                    id,
                    prop: Prop::Source(connector.target.clone()),
                },
                Command::Set {
                    id,
                    prop: Prop::Target(connector.source.clone()),
                },
                Command::Set {
                    id,
                    prop: Prop::StartMarker(connector.end_marker),
                },
                Command::Set {
                    id,
                    prop: Prop::EndMarker(connector.start_marker),
                },
                Command::Set {
                    id,
                    prop: Prop::ForeignKey(Some(key.clone())),
                },
            ],
        )
        .unwrap();
    assert_eq!(
        export_page(&doc, doc.first_page().unwrap(), SqlDialect::PostgreSql).unwrap(),
        sql
    );
    let owner = doc.elements[&id]
        .as_connector()
        .unwrap()
        .foreign_key_endpoints()
        .unwrap()
        .0
        .element()
        .unwrap();
    assert!(
        doc.connectors_attached_to_column(owner, key.columns[1])
            .contains(&id)
    );
    assert!(history.undo(&mut doc));
    assert_eq!(doc, before);
    assert!(history.redo(&mut doc));
    assert_eq!(
        export_page(&doc, doc.first_page().unwrap(), SqlDialect::PostgreSql).unwrap(),
        sql
    );
}

#[test]
fn export_canonicalizes_comment_fragments_and_warns_external_definitions() {
    let (mut doc, _, _) = imported("CREATE TABLE t(a integer,b integer);");
    let id = table_id(&doc, "t");
    let table = doc
        .elements
        .get_mut(&id)
        .unwrap()
        .as_shape_mut()
        .unwrap()
        .erd
        .as_mut()
        .unwrap();
    table.columns[0].data_type = "integer -- comment".into();
    table.columns[0].default_value = Some("1 -- comment".into());
    table.columns[1].data_type = "external.mood[]".into();
    table.columns[1].default_value = Some("nextval('external_seq'::regclass)".into());
    let output = export_page(&doc, doc.first_page().unwrap(), SqlDialect::PostgreSql).unwrap();
    assert!(!output.sql.contains("-- comment"));
    assert!(
        output
            .warnings
            .iter()
            .any(|warning| warning.message.contains("external type"))
    );
    assert!(
        output
            .warnings
            .iter()
            .any(|warning| warning.message.contains("external sequence"))
    );
    assert!(
        parse(&output.sql, SqlDialect::PostgreSql)
            .unwrap()
            .warnings
            .is_empty()
    );
    let table = doc
        .elements
        .get_mut(&id)
        .unwrap()
        .as_shape_mut()
        .unwrap()
        .erd
        .as_mut()
        .unwrap();
    table.columns[0].data_type = "INTEGER) --".into();
    let output = export_page(&doc, doc.first_page().unwrap(), SqlDialect::PostgreSql).unwrap();
    assert!(!output.sql.contains("CREATE TABLE"));
    assert!(
        output
            .warnings
            .iter()
            .any(|warning| warning.message.contains("type"))
    );
}

#[test]
fn export_warns_unsupported_features_invalid_fragments_and_duplicate_names() {
    let (mut doc, _, parent) = imported("CREATE TABLE t(id integer PRIMARY KEY, name text);");
    let id = table_id(&doc, "t");
    let table = doc
        .elements
        .get_mut(&id)
        .unwrap()
        .as_shape_mut()
        .unwrap()
        .erd
        .as_mut()
        .unwrap();
    table.dialect = SqlDialect::MySql;
    table.columns[1].default_value = Some("'unfinished".into());
    table.columns[1].foreign_key = true;
    let decoration = Element::shape(
        ShapeRef::new("basic", "rectangle"),
        parent,
        doc.next_order_key(parent),
        Rect::new(0.0, 0.0, 100.0, 100.0),
    );
    doc.elements.insert(decoration.id, decoration);
    let output = export_page(&doc, doc.first_page().unwrap(), SqlDialect::PostgreSql).unwrap();
    assert!(output.sql.contains("CREATE TABLE \"t\""));
    assert!(output.warnings.len() >= 4);
    assert!(export_page(&doc, doc.first_page().unwrap(), SqlDialect::Sqlite).is_err());
    let mut duplicate = doc.elements[&id].clone();
    duplicate.id = ElementId::new();
    doc.elements.insert(duplicate.id, duplicate);
    let output = export_page(&doc, doc.first_page().unwrap(), SqlDialect::PostgreSql).unwrap();
    assert_eq!(output.sql.matches("CREATE TABLE").count(), 1);
    assert!(
        output
            .warnings
            .iter()
            .any(|warning| warning.message.contains("Duplicate table"))
    );
}

#[test]
fn malformed_stable_metadata_is_rejected_without_mutating_document() {
    let (mut doc, _, _) = imported("CREATE TABLE t(a integer,b integer,PRIMARY KEY(a,b));");
    let before = doc.clone();
    let id = table_id(&doc, "t");
    let mut table = doc.elements[&id]
        .as_shape()
        .unwrap()
        .erd
        .as_ref()
        .unwrap()
        .clone();
    table.primary_key = Some(ErdKey {
        name: None,
        columns: vec![bp_model::ColumnId::new()],
    });
    assert!(
        Command::Set {
            id,
            prop: Prop::ErdTable(Box::new(table))
        }
        .apply(&mut doc)
        .is_err()
    );
    assert_eq!(doc, before);
}

#[test]
fn imported_unique_foreign_keys_use_one_to_one_markers_and_layout_appends() {
    let (mut doc, _, parent) = imported(
        "CREATE TABLE parent(a integer,b integer,PRIMARY KEY(a,b)); CREATE TABLE child(a integer,b integer,PRIMARY KEY(a,b),FOREIGN KEY(a,b) REFERENCES parent(a,b));",
    );
    let connector = doc
        .elements
        .values()
        .find_map(|element| element.as_connector())
        .unwrap();
    assert_eq!(connector.start_marker, Marker::ZeroOrOne);
    assert_eq!(connector.style.dash, Some(bp_model::Dash::Solid));
    let previous_max = doc
        .elements
        .values()
        .filter_map(Element::as_shape)
        .map(|shape| shape.bounds.x1)
        .fold(0.0_f64, f64::max);
    let preview = parse(
        "CREATE TABLE appended(id integer PRIMARY KEY);",
        SqlDialect::PostgreSql,
    )
    .unwrap();
    let (ids, commands) = import_commands(&doc, parent, &preview).unwrap();
    History::new().apply(&mut doc, "Append", commands).unwrap();
    assert!(doc.elements[&ids[0]].as_shape().unwrap().bounds.x0 > previous_max);
}
