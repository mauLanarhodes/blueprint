use bp_model::{ErdColumn, OrderKey};
use bp_sql::{SqlDialect, bind_index_expression, current_index_expression, export_schema, parse};

fn schema(sql: &str) -> bp_sql::Schema {
    let preview = parse(sql, SqlDialect::PostgreSql).unwrap();
    assert!(preview.warnings.is_empty(), "{:?}", preview.warnings);
    preview.schema
}

#[test]
fn index_normalization_preserves_collations_and_wraps_arithmetic() {
    let mut schema = schema(
        r#"CREATE TABLE t (id INTEGER, label TEXT, more TEXT);
        CREATE INDEX collated ON t (label COLLATE "C" text_pattern_ops DESC NULLS FIRST);
        CREATE INDEX concatenated ON t ((label || more) COLLATE "C" text_pattern_ops ASC);
        CREATE INDEX arithmetic ON t (id);"#,
    );
    schema.tables[0].indexes[2].columns = vec!["id + 1 DESC".into()];
    let output = export_schema(&schema, SqlDialect::PostgreSql).unwrap();
    assert!(output.warnings.is_empty(), "{:?}", output.warnings);
    assert!(
        output
            .sql
            .contains(r#"(label COLLATE "C" text_pattern_ops DESC NULLS FIRST)"#)
    );
    assert!(
        output
            .sql
            .contains(r#"((label || more) COLLATE "C" text_pattern_ops ASC)"#)
    );
    assert!(output.sql.contains("((id + 1) DESC)"));
    let reparsed = parse(&output.sql, SqlDialect::PostgreSql).unwrap();
    assert!(reparsed.warnings.is_empty(), "{:?}", reparsed.warnings);
    assert_eq!(
        schema.tables[0].indexes[..2],
        reparsed.schema.tables[0].indexes[..2]
    );
    assert_eq!(
        reparsed.schema.tables[0].indexes[2].columns,
        ["(id + 1) DESC"]
    );
}

#[test]
fn index_bindings_exclude_select_aliases_bare_opclasses_and_quoted_cast_types() {
    let mut columns = ["label", "alias", "text_pattern_ops", "My Type"]
        .map(|name| ErdColumn::new(name, "TEXT", OrderKey::first()));
    for expression in [
        "lower(label) alias",
        r#"CAST(label AS "My Type") alias"#,
        "label text_pattern_ops",
    ] {
        let bound = bind_index_expression(expression, &columns);
        assert_eq!(
            bound.references.len(),
            1,
            "{expression}: {:?}",
            bound.references
        );
        assert_eq!(bound.references[0].column, columns[0].id);
        for column in &mut columns[1..] {
            column.name.push_str(" edited");
        }
        assert_eq!(current_index_expression(&bound, &columns), expression);
        columns[0].name = "renamed".into();
        assert_eq!(
            current_index_expression(&bound, &columns),
            expression.replacen("label", "\"renamed\"", 1)
        );
        columns[0].name = "label".into();
        for column in &mut columns[1..] {
            column.name = column.name.trim_end_matches(" edited").into();
        }
    }
}

#[test]
fn foreign_keys_to_collated_unique_indexes_survive_export_and_reimport() {
    let original = schema(
        r#"CREATE TABLE parent (label TEXT);
        CREATE UNIQUE INDEX parent_label ON parent (label COLLATE "C" DESC NULLS FIRST);
        CREATE TABLE child (parent_label TEXT REFERENCES parent (label));"#,
    );
    assert_eq!(original.foreign_keys.len(), 1);
    let output = export_schema(&original, SqlDialect::PostgreSql).unwrap();
    assert!(output.warnings.is_empty(), "{:?}", output.warnings);
    assert!(
        output
            .sql
            .contains("FOREIGN KEY (\"parent_label\") REFERENCES \"parent\" (\"label\")")
    );
    assert!(
        output
            .sql
            .contains(r#"(label COLLATE "C" DESC NULLS FIRST)"#)
    );
    let reparsed = parse(&output.sql, SqlDialect::PostgreSql).unwrap();
    assert!(reparsed.warnings.is_empty(), "{:?}", reparsed.warnings);
    assert_eq!(reparsed.schema, original);
}

#[test]
fn named_keys_and_indexes_share_the_schema_relation_namespace() {
    let schema = schema(
        "CREATE TABLE first (id INTEGER, CONSTRAINT shared PRIMARY KEY (id));
        CREATE TABLE second (id INTEGER, CONSTRAINT shared UNIQUE (id));
        CREATE TABLE third (id INTEGER, CONSTRAINT third PRIMARY KEY (id));
        CREATE INDEX shared ON second (id);
        CREATE INDEX third ON first (id);",
    );
    let output = export_schema(&schema, SqlDialect::PostgreSql).unwrap();
    assert_eq!(output.sql.matches("CREATE TABLE").count(), 3);
    assert_eq!(output.sql.matches("CONSTRAINT \"shared\"").count(), 1);
    assert!(!output.sql.contains("CONSTRAINT \"third\""));
    assert!(!output.sql.contains("CREATE INDEX"));
    assert_eq!(output.warnings.len(), 4, "{:?}", output.warnings);
    assert!(
        parse(&output.sql, SqlDialect::PostgreSql)
            .unwrap()
            .warnings
            .is_empty()
    );
}

#[test]
fn relation_names_can_repeat_in_distinct_schemas() {
    let schema = schema(
        "CREATE TABLE one.first (id INTEGER, CONSTRAINT shared PRIMARY KEY (id));
        CREATE TABLE two.second (id INTEGER, CONSTRAINT shared PRIMARY KEY (id));
        CREATE INDEX ordinary ON one.first (id);
        CREATE INDEX ordinary ON two.second (id);",
    );
    let output = export_schema(&schema, SqlDialect::PostgreSql).unwrap();
    assert!(output.warnings.is_empty(), "{:?}", output.warnings);
    assert_eq!(output.sql.matches("CONSTRAINT \"shared\"").count(), 2);
    assert_eq!(output.sql.matches("CREATE INDEX \"ordinary\"").count(), 2);
}

#[test]
fn type_and_default_fields_cannot_escape_into_extra_statements() {
    let original = schema("CREATE TABLE t (id INTEGER PRIMARY KEY, label TEXT);");
    for fragment in [
        "INTEGER); DROP TABLE t; --",
        "INTEGER) --",
        "INTEGER, injected INTEGER",
        "INTEGER DEFAULT 1",
        "INTEGER NOT NULL",
    ] {
        let mut schema = original.clone();
        schema.tables[0].columns[0].data_type = fragment.into();
        let output = export_schema(&schema, SqlDialect::PostgreSql).unwrap();
        assert!(
            !output.sql.contains("CREATE TABLE"),
            "{fragment}: {}",
            output.sql
        );
        assert!(!output.sql.contains("DROP TABLE"));
        assert!(
            output
                .warnings
                .iter()
                .any(|warning| warning.message.contains("type"))
        );
    }
    for fragment in [
        "1); DROP TABLE t; --",
        "1) --",
        "1; SELECT 42",
        "1, injected INTEGER",
        "1 PRIMARY KEY",
    ] {
        let mut schema = original.clone();
        schema.tables[0].columns[0].default_value = Some(fragment.into());
        let output = export_schema(&schema, SqlDialect::PostgreSql).unwrap();
        assert_eq!(output.sql.matches("CREATE TABLE").count(), 1);
        assert!(
            !output.sql.contains("DEFAULT"),
            "{fragment}: {}",
            output.sql
        );
        assert!(!output.sql.contains("DROP TABLE"));
        assert!(
            output
                .warnings
                .iter()
                .any(|warning| warning.message.contains("Default"))
        );
        let reparsed = parse(&output.sql, SqlDialect::PostgreSql).unwrap();
        assert!(
            reparsed.warnings.is_empty(),
            "{fragment}: {:?}",
            reparsed.warnings
        );
        assert_eq!(reparsed.schema.tables[0].columns.len(), 2);
    }
}

#[test]
fn invalid_index_fragments_warn_and_keep_supported_tables() {
    let original = schema("CREATE TABLE t (id INTEGER); CREATE INDEX valid ON t (id);");
    for (expression, predicate) in [
        ("id); DROP TABLE t; --", None),
        ("id", Some("id > 0); DROP TABLE t; --")),
    ] {
        let mut schema = original.clone();
        schema.tables[0].indexes[0].columns = vec![expression.into()];
        schema.tables[0].indexes[0].predicate = predicate.map(String::from);
        let output = export_schema(&schema, SqlDialect::PostgreSql).unwrap();
        assert_eq!(output.sql.matches("CREATE TABLE").count(), 1);
        assert!(!output.sql.contains("CREATE INDEX"));
        assert!(!output.sql.contains("DROP TABLE"));
        assert_eq!(output.warnings.len(), 1, "{:?}", output.warnings);
    }
}
