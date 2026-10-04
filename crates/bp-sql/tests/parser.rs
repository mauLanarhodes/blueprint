use bp_sql::{SqlDialect, parse};

fn postgres(sql: &str) -> bp_sql::ImportPreview {
    parse(sql, SqlDialect::PostgreSql).unwrap()
}

#[test]
fn imports_types_defaults_nullability_and_named_composite_keys() {
    let preview = postgres(
        "CREATE TABLE accounts (\n\
          tenant_id BIGINT,\n\
          id UUID DEFAULT gen_random_uuid(),\n\
          amount NUMERIC(12, 2) NOT NULL DEFAULT 0,\n\
          label TEXT DEFAULT 'it''s; fine',\n\
          CONSTRAINT accounts_pk PRIMARY KEY (tenant_id, id),\n\
          CONSTRAINT accounts_label UNIQUE (tenant_id, label)\n\
        );",
    );
    assert!(preview.warnings.is_empty(), "{:?}", preview.warnings);
    let table = &preview.schema.tables[0];
    assert_eq!(table.name, ["accounts"]);
    assert_eq!(table.columns.len(), 4);
    assert!(!table.columns[0].nullable);
    assert!(!table.columns[1].nullable);
    assert!(!table.columns[2].nullable);
    assert!(table.columns[3].nullable);
    assert_eq!(
        table.columns[1].default_value.as_deref(),
        Some("gen_random_uuid()")
    );
    assert_eq!(table.columns[2].data_type, "NUMERIC(12,2)");
    assert_eq!(
        table.columns[3].default_value.as_deref(),
        Some("'it''s; fine'")
    );
    let key = table.primary_key.as_ref().unwrap();
    assert_eq!(key.name.as_deref(), Some("accounts_pk"));
    assert_eq!(key.columns, ["tenant_id", "id"]);
    assert_eq!(table.unique_keys[0].columns, ["tenant_id", "label"]);
}

#[test]
fn quoted_names_preserve_case_dots_spaces_and_escaped_quotes() {
    let preview = postgres(
        r#"CREATE SCHEMA IF NOT EXISTS "Sales.Data";
           CREATE TABLE "Sales.Data"."Order ""Lines" (
             "Item ID" INTEGER PRIMARY KEY,
             MixedCase TEXT,
             "Ünicode" TEXT DEFAULT '/* value; */'
           );"#,
    );
    assert!(preview.warnings.is_empty(), "{:?}", preview.warnings);
    let table = &preview.schema.tables[0];
    assert_eq!(table.name, ["Sales.Data", "Order \"Lines"]);
    assert_eq!(table.columns[0].name, "Item ID");
    assert_eq!(table.columns[1].name, "mixedcase");
    assert_eq!(table.columns[2].name, "Ünicode");
}

#[test]
fn resolves_composite_foreign_keys_after_all_tables_and_alter_statements() {
    let preview = postgres(
        "ALTER TABLE items ADD CONSTRAINT item_owner FOREIGN KEY (tenant, owner) REFERENCES users (tenant, id) ON DELETE CASCADE ON UPDATE RESTRICT;\n\
         CREATE TABLE items (tenant BIGINT, owner BIGINT);\n\
         CREATE TABLE users (tenant BIGINT, id BIGINT, PRIMARY KEY (tenant, id));",
    );
    assert!(preview.warnings.is_empty(), "{:?}", preview.warnings);
    assert_eq!(preview.schema.foreign_keys.len(), 1);
    let key = &preview.schema.foreign_keys[0];
    assert_eq!(key.name.as_deref(), Some("item_owner"));
    assert_eq!(key.columns, ["tenant", "owner"]);
    assert_eq!(key.referenced_columns, ["tenant", "id"]);
    assert_eq!(key.referenced_table, ["users"]);
    assert_eq!(key.on_delete.as_deref(), Some("CASCADE"));
    assert_eq!(key.on_update.as_deref(), Some("RESTRICT"));
}

#[test]
fn inline_references_infer_primary_keys_and_schema_resolution() {
    let preview = postgres(
        "CREATE TABLE app.children (id INTEGER PRIMARY KEY, parent_id INTEGER REFERENCES parents);\n\
         CREATE TABLE app.parents (id INTEGER PRIMARY KEY);",
    );
    assert!(preview.warnings.is_empty(), "{:?}", preview.warnings);
    assert_eq!(
        preview.schema.foreign_keys[0].referenced_table,
        ["app", "parents"]
    );
    assert_eq!(preview.schema.foreign_keys[0].referenced_columns, ["id"]);
}

#[test]
fn supports_unique_expression_ordered_and_partial_indexes_before_tables() {
    let preview = postgres(
        r#"CREATE UNIQUE INDEX "label idx" ON app.accounts USING btree (tenant DESC, lower(label)) WHERE label IS NOT NULL;
           CREATE TABLE app.accounts (tenant BIGINT, label TEXT);"#,
    );
    assert!(preview.warnings.is_empty(), "{:?}", preview.warnings);
    let index = &preview.schema.tables[0].indexes[0];
    assert_eq!(index.name, "label idx");
    assert!(index.unique);
    assert_eq!(index.method.as_deref(), Some("btree"));
    assert_eq!(index.columns, ["tenant DESC", "lower(label)"]);
    assert_eq!(index.predicate.as_deref(), Some("label IS NOT NULL"));
}

#[test]
fn index_operator_classes_precede_ordering_and_round_trip() {
    let preview = postgres(
        r#"CREATE TABLE t (label TEXT);
        CREATE INDEX ordered ON t (label text_pattern_ops DESC NULLS FIRST);
        CREATE INDEX collated ON t (label COLLATE "C" text_pattern_ops ASC);
    "#,
    );
    assert!(preview.warnings.is_empty(), "{:?}", preview.warnings);
    assert_eq!(
        preview.schema.tables[0].indexes[0].columns,
        ["label text_pattern_ops DESC NULLS FIRST"]
    );
    assert_eq!(
        preview.schema.tables[0].indexes[1].columns,
        ["label COLLATE \"C\" text_pattern_ops ASC"]
    );
    let exported = bp_sql::export_schema(&preview.schema, SqlDialect::PostgreSql).unwrap();
    assert!(exported.warnings.is_empty(), "{:?}", exported.warnings);
    let reparsed = postgres(&exported.sql);
    assert!(reparsed.warnings.is_empty(), "{:?}", reparsed.warnings);
    assert_eq!(preview.schema, reparsed.schema);
}

#[test]
fn comments_literals_and_dollar_quoted_function_bodies_do_not_split_statements() {
    let preview = postgres(
        "-- header with ;\n\
         /* outer /* nested ; */ comment */\n\
         CREATE TABLE a (id INTEGER PRIMARY KEY, body TEXT DEFAULT $value$x;-- y$value$);\n\
         CREATE FUNCTION f() RETURNS void AS $body$ BEGIN PERFORM 1; PERFORM 2; END; $body$ LANGUAGE plpgsql;\n\
         CREATE TABLE b (id INTEGER, message TEXT DEFAULT E'it\\'s; okay');",
    );
    assert_eq!(preview.schema.tables.len(), 2);
    assert_eq!(preview.warnings.len(), 1, "{:?}", preview.warnings);
    assert_eq!(preview.warnings[0].line, 4);
    assert!(
        preview.warnings[0]
            .message
            .contains("Unsupported SQL statement")
    );
    assert_eq!(
        preview.schema.tables[0].columns[1].default_value.as_deref(),
        Some("$value$x;-- y$value$")
    );
}

#[test]
fn unsupported_statements_and_constraints_warn_on_source_lines() {
    let preview = postgres(
        "CREATE TABLE first (id INTEGER);\n\
         SELECT * FROM first;\n\
         CREATE TABLE second (\n\
           id INTEGER,\n\
           CONSTRAINT positive CHECK (id > 0)\n\
         );\n\
         CREATE TABLE third (id INTEGER);",
    );
    assert_eq!(preview.schema.tables.len(), 3);
    assert_eq!(preview.warnings.len(), 2, "{:?}", preview.warnings);
    assert_eq!(preview.warnings[0].line, 2);
    assert_eq!(preview.warnings[1].line, 5);
    assert!(preview.warnings[1].message.contains("CHECK"));
}

#[test]
fn malformed_column_and_table_options_recover_supported_rows() {
    let preview = postgres(
        "CREATE UNLOGGED TABLE recovered (\n\
           id INTEGER PRIMARY KEY,\n\
           label TEXT unsupported_option,\n\
           extra BOOLEAN DEFAULT TRUE\n\
         ) unsupported_storage;\n\
         CREATE TABLE later (id INTEGER);",
    );
    assert_eq!(preview.schema.tables.len(), 2);
    assert_eq!(preview.schema.tables[0].columns.len(), 3);
    assert_eq!(preview.schema.tables[0].columns[1].data_type, "TEXT");
    assert!(!preview.schema.tables[0].columns[0].nullable);
    assert!(
        preview
            .warnings
            .iter()
            .any(|warning| warning.line == 3 && warning.message.contains("column suffix")),
        "{:?}",
        preview.warnings
    );
    assert!(
        preview
            .warnings
            .iter()
            .any(|warning| warning.message.contains("table suffix"))
    );
    assert!(
        preview
            .warnings
            .iter()
            .any(|warning| warning.message.contains("UNLOGGED"))
    );
}

#[test]
fn invalid_keys_and_relationships_warn_without_erasing_tables() {
    let preview = postgres(
        "CREATE TABLE a (id INTEGER, PRIMARY KEY (missing), UNIQUE (id,id));\n\
         CREATE TABLE b (id INTEGER PRIMARY KEY);\n\
         ALTER TABLE a ADD CONSTRAINT bad FOREIGN KEY (id,id) REFERENCES b (id,id);\n\
         ALTER TABLE a ADD FOREIGN KEY (id) REFERENCES absent (id);\n\
         CREATE INDEX bad_index ON a (missing);",
    );
    assert_eq!(preview.schema.tables.len(), 2);
    assert!(preview.schema.tables[0].primary_key.is_none());
    assert!(preview.schema.tables[0].unique_keys.is_empty());
    assert!(preview.schema.tables[0].indexes.is_empty());
    assert!(preview.schema.foreign_keys.is_empty());
    assert_eq!(preview.warnings.len(), 5, "{:?}", preview.warnings);
}

#[test]
fn generated_collated_and_check_columns_remain_editable_with_warnings() {
    let preview = postgres(
        "CREATE TABLE unsupported (id BIGINT GENERATED ALWAYS AS IDENTITY, label TEXT COLLATE \"C\", positive INTEGER CHECK (positive>0));",
    );
    assert_eq!(preview.schema.tables[0].columns.len(), 3);
    assert_eq!(preview.warnings.len(), 3, "{:?}", preview.warnings);
    assert!(!preview.schema.tables[0].columns[0].nullable);
}

#[test]
fn array_defaults_survive_recovery_and_serial_columns_are_not_nullable() {
    let preview = postgres(
        "CREATE TABLE a (id SERIAL, values INTEGER[] DEFAULT ARRAY[1,2], label TEXT unknown_option);",
    );
    assert_eq!(preview.schema.tables[0].columns.len(), 3);
    assert!(!preview.schema.tables[0].columns[0].nullable);
    assert_eq!(
        preview.schema.tables[0].columns[1].default_value.as_deref(),
        Some("ARRAY[1, 2]")
    );
    assert_eq!(preview.warnings.len(), 1, "{:?}", preview.warnings);
}

#[test]
fn escaped_newlines_keep_following_warning_lines_accurate() {
    let preview = postgres("CREATE TABLE a (body TEXT DEFAULT E'one\\\ntwo');\nSELECT 1;");
    assert_eq!(preview.schema.tables.len(), 1);
    assert_eq!(preview.warnings[0].line, 3);
}

#[test]
fn unimplemented_dialects_are_rejected_explicitly() {
    assert_eq!(bp_sql::SUPPORTED_DIALECTS, &[SqlDialect::PostgreSql]);
    assert!(
        parse("CREATE TABLE t (id INTEGER)", SqlDialect::Sqlite)
            .unwrap_err()
            .contains("not implemented")
    );
}
