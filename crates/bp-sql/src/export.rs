//! PostgreSQL DDL generation, separate from parsing and diagram conversion.

use crate::{Column, ForeignKey, Index, Key, QualifiedName, Schema, Table, Warning};
use bp_model::{
    ColumnId, Document, ElementId, Endpoint, ErdIndexExpression, ErdTable, Marker, PageId,
    SqlDialect,
};
use sqlparser::{
    ast::{ArrayElemTypeDef, DataType, Expr, Statement},
    dialect::PostgreSqlDialect,
    parser::Parser,
    tokenizer::Token,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExportPreview {
    pub sql: String,
    pub warnings: Vec<Warning>,
}

/// Export every table on a page, including tables on hidden layers.
pub fn export_page(
    doc: &Document,
    page: PageId,
    dialect: SqlDialect,
) -> Result<ExportPreview, String> {
    if !doc.pages.contains_key(&page) {
        return Err("The page does not exist".into());
    }
    if dialect != SqlDialect::PostgreSql {
        return Err(format!("{} SQL export is not implemented", dialect.label()));
    }
    doc.validate().map_err(|error| error.to_string())?;
    let mut schema = Schema::default();
    let mut warnings = Vec::new();
    let mut names = BTreeMap::<ElementId, QualifiedName>::new();
    let elements: Vec<_> = doc
        .elements
        .values()
        .filter(|element| doc.page_of(element.id) == Some(page))
        .collect();
    let mut tables: Vec<_> = elements
        .iter()
        .filter_map(|element| {
            element
                .as_shape()
                .and_then(|shape| shape.erd.as_ref().map(|table| (*element, shape, table)))
        })
        .collect();
    tables.sort_by(|(a, _, _), (b, _, _)| {
        (a.parent, &a.order, a.id).cmp(&(b.parent, &b.order, b.id))
    });
    for (element, shape, table) in &tables {
        let mut name = table.schema.clone();
        name.push(shape.text.clone());
        names.insert(element.id, name.clone());
        if table.dialect != dialect {
            warn(
                &mut warnings,
                format!(
                    "Table {} uses {} types; PostgreSQL export preserves their text without converting types",
                    name.join("."),
                    table.dialect.label()
                ),
            );
        }
        let mut rows: Vec<_> = table.columns.iter().collect();
        rows.sort_by(|a, b| (&a.order, a.id).cmp(&(&b.order, b.id)));
        let primary_key = table
            .primary_key
            .as_ref()
            .map(|key| Key {
                name: key.name.clone(),
                columns: column_names(table, &key.columns),
            })
            .or_else(|| {
                let columns: Vec<_> = rows
                    .iter()
                    .filter(|column| column.primary_key)
                    .map(|column| column.name.clone())
                    .collect();
                (!columns.is_empty()).then_some(Key {
                    name: None,
                    columns,
                })
            });
        let mut unique_keys: Vec<_> = table
            .unique_keys
            .iter()
            .map(|key| Key {
                name: key.name.clone(),
                columns: column_names(table, &key.columns),
            })
            .collect();
        for column in &rows {
            if column.unique
                && !unique_keys
                    .iter()
                    .any(|key| key.columns == [column.name.clone()])
            {
                unique_keys.push(Key {
                    name: None,
                    columns: vec![column.name.clone()],
                });
            }
        }
        schema.tables.push(Table {
            name,
            columns: rows
                .iter()
                .map(|column| Column {
                    name: column.name.clone(),
                    data_type: column.data_type.clone(),
                    nullable: column.nullable,
                    default_value: column.default_value.clone(),
                })
                .collect(),
            primary_key,
            unique_keys,
            indexes: table
                .indexes
                .iter()
                .map(|index| Index {
                    name: index.name.clone(),
                    unique: index.unique,
                    columns: index
                        .columns
                        .iter()
                        .map(|expression| render_expression(expression, table))
                        .collect(),
                    method: index.method.clone(),
                    predicate: index
                        .predicate
                        .as_ref()
                        .map(|expression| render_expression(expression, table)),
                })
                .collect(),
        });
    }
    let mut represented = BTreeSet::new();
    for element in &elements {
        let Some(connector) = element.as_connector() else {
            continue;
        };
        if let Some(key) = &connector.foreign_key {
            let (owner, referenced) = connector.foreign_key_endpoints().unwrap();
            let source = owner.element().unwrap();
            let target = referenced.element().unwrap();
            let Some(source_name) = names.get(&source) else {
                warn(
                    &mut warnings,
                    format!(
                        "Relationship {} crosses outside the exported page and was omitted",
                        element.id
                    ),
                );
                continue;
            };
            let Some(target_name) = names.get(&target) else {
                warn(
                    &mut warnings,
                    format!(
                        "Relationship {} crosses outside the exported page and was omitted",
                        element.id
                    ),
                );
                continue;
            };
            let source_table = doc.elements[&source]
                .as_shape()
                .unwrap()
                .erd
                .as_ref()
                .unwrap();
            let target_table = doc.elements[&target]
                .as_shape()
                .unwrap()
                .erd
                .as_ref()
                .unwrap();
            represented.extend(key.columns.iter().map(|column| (source, *column)));
            schema.foreign_keys.push(ForeignKey {
                name: key.name.clone(),
                table: source_name.clone(),
                referenced_table: target_name.clone(),
                columns: column_names(source_table, &key.columns),
                referenced_columns: column_names(target_table, &key.referenced_columns),
                on_delete: key.on_delete.clone(),
                on_update: key.on_update.clone(),
            });
        } else if let Some((source, source_column, target, target_column)) =
            infer_foreign_key(doc, &connector.source, &connector.target)
        {
            let (Some(source_name), Some(target_name)) = (names.get(&source), names.get(&target))
            else {
                warn(
                    &mut warnings,
                    format!(
                        "Relationship {} crosses outside the exported page and was omitted",
                        element.id
                    ),
                );
                continue;
            };
            represented.insert((source, source_column));
            let source_table = doc.elements[&source]
                .as_shape()
                .unwrap()
                .erd
                .as_ref()
                .unwrap();
            let target_table = doc.elements[&target]
                .as_shape()
                .unwrap()
                .erd
                .as_ref()
                .unwrap();
            schema.foreign_keys.push(ForeignKey {
                name: None,
                table: source_name.clone(),
                columns: column_names(source_table, &[source_column]),
                referenced_table: target_name.clone(),
                referenced_columns: column_names(target_table, &[target_column]),
                on_delete: None,
                on_update: None,
            });
            warn(
                &mut warnings,
                format!(
                    "Relationship {} was inferred as a foreign key from its column ports; SQL does not express all diagram cardinalities",
                    element.id
                ),
            );
        } else {
            warn(
                &mut warnings,
                format!(
                    "Relationship {} has no unambiguous foreign key column mapping and was omitted",
                    element.id
                ),
            );
        }
        if !connector.text.is_empty() {
            warn(
                &mut warnings,
                format!(
                    "Relationship label {:?} has no SQL DDL representation",
                    connector.text
                ),
            );
        }
        if matches!(connector.start_marker, Marker::OneOrMany)
            || matches!(connector.end_marker, Marker::OneOrMany)
        {
            warn(
                &mut warnings,
                format!(
                    "Relationship {} requires a related row on the many side; a foreign key cannot enforce that requirement",
                    element.id
                ),
            );
        }
    }
    for (element, shape, table) in &tables {
        for column in &table.columns {
            if column.foreign_key && !represented.contains(&(element.id, column.id)) {
                warn(
                    &mut warnings,
                    format!(
                        "{}.{} is marked FK but has no exported foreign key relationship",
                        shape.text, column.name
                    ),
                );
            }
        }
    }
    let omitted = elements
        .iter()
        .filter(|element| element.as_shape().is_some_and(|shape| shape.erd.is_none()))
        .count();
    if omitted > 0 {
        warn(
            &mut warnings,
            format!("{omitted} non-table diagram shapes have no SQL DDL representation"),
        );
    }
    let mut output = export_schema(&schema, dialect)?;
    warnings.append(&mut output.warnings);
    output.warnings = warnings;
    Ok(output)
}

/// Generate tables and keys first, then indexes, then foreign keys. Deferring
/// every foreign key allows both forward references and dependency cycles.
pub fn export_schema(schema: &Schema, dialect: SqlDialect) -> Result<ExportPreview, String> {
    match dialect {
        SqlDialect::PostgreSql => Ok(postgres(schema)),
        _ => Err(format!("{} SQL export is not implemented", dialect.label())),
    }
}

fn postgres(schema: &Schema) -> ExportPreview {
    let mut output = ExportPreview::default();
    let mut emitted = BTreeMap::new();
    let mut schemas = BTreeSet::new();
    let mut indexes = String::new();
    // Tables and named primary/unique constraints occupy the same PostgreSQL
    // relation namespace as explicitly created indexes.
    let mut index_names = BTreeSet::new();
    for table in &schema.tables {
        let canonical = canonical_name(&table.name);
        if canonical.len() == 2 {
            index_names.insert((canonical[0].clone(), canonical[1].clone()));
        }
    }
    output
        .sql
        .push_str("-- PostgreSQL schema exported by Blueprint\n\n");
    for table in &schema.tables {
        let display = table.name.join(".");
        for name in table
            .name
            .iter()
            .chain(table.columns.iter().map(|column| &column.name))
            .chain(
                table
                    .primary_key
                    .iter()
                    .chain(&table.unique_keys)
                    .filter_map(|key| key.name.as_ref()),
            )
            .chain(table.indexes.iter().map(|index| &index.name))
        {
            if name.len() > 63 {
                warn(
                    &mut output.warnings,
                    format!(
                        "Identifier {name:?} on {display} exceeds PostgreSQL's 63-byte limit and will be truncated by PostgreSQL"
                    ),
                );
            }
        }
        if table.name.is_empty()
            || table.name.len() > 2
            || table.name.iter().any(|name| !valid_identifier(name))
        {
            warn(
                &mut output.warnings,
                format!(
                    "Table {display:?} has an invalid PostgreSQL qualified name and was omitted"
                ),
            );
            continue;
        }
        let canonical = canonical_name(&table.name);
        if emitted.contains_key(&canonical) {
            warn(
                &mut output.warnings,
                format!("Duplicate table {display} was omitted"),
            );
            continue;
        }
        let mut column_names = BTreeSet::new();
        let mut effective_columns = BTreeSet::new();
        if table.columns.iter().any(|column| {
            !valid_identifier(&column.name)
                || !column_names.insert(column.name.clone())
                || !effective_columns.insert(effective_identifier(&column.name))
        }) {
            warn(
                &mut output.warnings,
                format!("Table {display} has empty or duplicate column names and was omitted"),
            );
            continue;
        }
        if table
            .columns
            .iter()
            .any(|column| !valid_type(&column.data_type))
        {
            warn(
                &mut output.warnings,
                format!(
                    "Table {display} has a type that is not valid PostgreSQL column syntax and was omitted"
                ),
            );
            continue;
        }
        if table.name.len() == 2 && schemas.insert(table.name[0].clone()) {
            writeln!(
                output.sql,
                "CREATE SCHEMA IF NOT EXISTS {};\n",
                quote(&table.name[0])
            )
            .unwrap();
        }
        let mut definitions = Vec::new();
        for column in &table.columns {
            let parsed_type = parse_type(&column.data_type).expect("types validated above");
            let mut definition = format!("    {} {}", quote(&column.name), parsed_type);
            if external_type(&parsed_type) {
                warn(
                    &mut output.warnings,
                    format!(
                        "{display}.{} uses type {}; its external type or extension definition is not included",
                        column.name, parsed_type
                    ),
                );
            }
            let primary = table
                .primary_key
                .as_ref()
                .is_some_and(|key| key.columns.contains(&column.name));
            if !column.nullable || primary {
                definition.push_str(" NOT NULL");
            }
            if column.nullable && primary {
                warn(
                    &mut output.warnings,
                    format!(
                        "{display}.{} is nullable in the diagram; PostgreSQL primary keys require NOT NULL",
                        column.name
                    ),
                );
            }
            if let Some(default) = &column.default_value {
                if let Some(parsed_default) = parse_default(default) {
                    definition.push_str(" DEFAULT ");
                    definition.push_str(&parsed_default.to_string());
                    if has_sequence_dependency(&parsed_default) {
                        warn(
                            &mut output.warnings,
                            format!(
                                "Default for {display}.{} references a sequence; its external sequence definition is not included",
                                column.name
                            ),
                        );
                    }
                } else {
                    warn(
                        &mut output.warnings,
                        format!(
                            "Default for {display}.{} is not valid PostgreSQL syntax and was omitted",
                            column.name
                        ),
                    );
                }
            }
            definitions.push(definition);
        }
        let mut constraint_names = BTreeSet::new();
        let mut valid_keys = Vec::new();
        for (kind, key) in table
            .primary_key
            .iter()
            .map(|key| ("PRIMARY KEY", key))
            .chain(table.unique_keys.iter().map(|key| ("UNIQUE", key)))
        {
            if !valid_key(key, &column_names) {
                warn(
                    &mut output.warnings,
                    format!(
                        "{kind} on {display} refers to missing or duplicate columns and was omitted"
                    ),
                );
                continue;
            }
            if let Some(name) = &key.name
                && (!valid_identifier(name) || !constraint_names.insert(effective_identifier(name)))
            {
                warn(
                    &mut output.warnings,
                    format!("Duplicate or empty constraint name on {display} was omitted"),
                );
                continue;
            }
            if let Some(name) = &key.name
                && !index_names.insert((canonical[0].clone(), effective_identifier(name)))
            {
                warn(
                    &mut output.warnings,
                    format!(
                        "Constraint {name:?} on {display} would create an index colliding with another table or index and was omitted"
                    ),
                );
                continue;
            }
            let prefix = key
                .name
                .as_ref()
                .map_or_else(String::new, |name| format!("CONSTRAINT {} ", quote(name)));
            definitions.push(format!(
                "    {prefix}{kind} ({})",
                quoted_columns(&key.columns)
            ));
            valid_keys.push(key.columns.clone());
        }
        writeln!(
            output.sql,
            "CREATE TABLE {} (\n{}\n);\n",
            qualified(&table.name),
            definitions.join(",\n")
        )
        .unwrap();
        for index in &table.indexes {
            let schema_name = table
                .name
                .first()
                .filter(|_| table.name.len() == 2)
                .cloned()
                .unwrap_or_else(|| "public".into());
            if !valid_identifier(&index.name)
                || !index_names.insert((
                    effective_identifier(&schema_name),
                    effective_identifier(&index.name),
                ))
            {
                warn(
                    &mut output.warnings,
                    format!(
                        "Duplicate or empty index name {:?} on {display} was omitted",
                        index.name
                    ),
                );
                continue;
            }
            let method = index
                .method
                .as_ref()
                .map_or_else(String::new, |method| format!(" USING {}", quote(method)));
            let normalized_columns = index
                .columns
                .iter()
                .map(|sql| normalize_index_expression(sql))
                .collect::<Option<Vec<_>>>();
            let normalized_predicate = index
                .predicate
                .as_ref()
                .map(|predicate| parse_default(predicate));
            if normalized_columns.is_none() || matches!(normalized_predicate, Some(None)) {
                warn(
                    &mut output.warnings,
                    format!(
                        "Index {:?} on {display} has invalid PostgreSQL expression syntax and was omitted",
                        index.name
                    ),
                );
                continue;
            }
            let predicate = normalized_predicate
                .flatten()
                .map_or_else(String::new, |predicate| format!(" WHERE {predicate}"));
            let statement = format!(
                "CREATE {}INDEX {} ON {}{method} ({}){predicate};\n",
                if index.unique { "UNIQUE " } else { "" },
                quote(&index.name),
                qualified(&table.name),
                normalized_columns.unwrap().join(", ")
            );
            if !valid_index(&statement) {
                warn(
                    &mut output.warnings,
                    format!(
                        "Index {:?} on {display} has unsupported PostgreSQL expression syntax and was omitted",
                        index.name
                    ),
                );
                continue;
            }
            if index.unique && index.predicate.is_none() {
                let index_columns: Vec<_> = index
                    .columns
                    .iter()
                    .filter_map(|expression| simple_index_column(expression))
                    .collect();
                if index_columns.len() == index.columns.len() {
                    valid_keys.push(index_columns);
                }
            }
            indexes.push_str(&statement);
        }
        emitted.insert(canonical, (column_names, valid_keys, constraint_names));
    }
    if !indexes.is_empty() {
        output.sql.push_str(&indexes);
        output.sql.push('\n');
    }
    let mut foreign_keys = BTreeSet::new();
    for key in &schema.foreign_keys {
        let display = key.table.join(".");
        if let Some(name) = &key.name
            && name.len() > 63
        {
            warn(
                &mut output.warnings,
                format!(
                    "Foreign key identifier {name:?} on {display} exceeds PostgreSQL's 63-byte limit and will be truncated by PostgreSQL"
                ),
            );
        }
        let (Some((source_columns, _, _)), Some((target_columns, target_keys, _))) = (
            emitted.get(&canonical_name(&key.table)),
            emitted.get(&canonical_name(&key.referenced_table)),
        ) else {
            warn(
                &mut output.warnings,
                format!(
                    "Foreign key on {display} refers to a table absent from the export and was omitted"
                ),
            );
            continue;
        };
        if key.columns.is_empty()
            || key.columns.len() != key.referenced_columns.len()
            || !valid_column_list(&key.columns, source_columns)
            || !valid_column_list(&key.referenced_columns, target_columns)
        {
            warn(
                &mut output.warnings,
                format!("Foreign key on {display} has an invalid column mapping and was omitted"),
            );
            continue;
        }
        if !target_keys.iter().any(|columns| {
            columns.len() == key.referenced_columns.len()
                && columns
                    .iter()
                    .all(|column| key.referenced_columns.contains(column))
        }) {
            warn(
                &mut output.warnings,
                format!(
                    "Foreign key on {display} does not reference a primary or unique key and was omitted"
                ),
            );
            continue;
        }
        let signature = (
            key.table.clone(),
            key.columns.clone(),
            key.referenced_table.clone(),
            key.referenced_columns.clone(),
            key.on_delete.clone(),
            key.on_update.clone(),
        );
        if !foreign_keys.insert(signature) {
            warn(
                &mut output.warnings,
                format!("Duplicate foreign key on {display} was omitted"),
            );
            continue;
        }
        if key
            .name
            .as_ref()
            .is_some_and(|name| !valid_identifier(name))
        {
            warn(
                &mut output.warnings,
                format!("Foreign key on {display} has an empty name and was omitted"),
            );
            continue;
        }
        if let Some(name) = &key.name
            && !emitted
                .get_mut(&canonical_name(&key.table))
                .unwrap()
                .2
                .insert(effective_identifier(name))
        {
            warn(
                &mut output.warnings,
                format!(
                    "Foreign key constraint name {name:?} on {display} is already used and was omitted"
                ),
            );
            continue;
        }
        let prefix = key
            .name
            .as_ref()
            .map_or_else(String::new, |name| format!("CONSTRAINT {} ", quote(name)));
        let mut statement = format!(
            "ALTER TABLE {} ADD {prefix}FOREIGN KEY ({}) REFERENCES {} ({})",
            qualified(&key.table),
            quoted_columns(&key.columns),
            qualified(&key.referenced_table),
            quoted_columns(&key.referenced_columns)
        );
        for (kind, action) in [("DELETE", &key.on_delete), ("UPDATE", &key.on_update)] {
            if let Some(action) = action {
                let action = action.to_uppercase();
                if matches!(
                    action.as_str(),
                    "NO ACTION" | "RESTRICT" | "CASCADE" | "SET NULL" | "SET DEFAULT"
                ) {
                    write!(statement, " ON {kind} {action}").unwrap();
                } else {
                    warn(
                        &mut output.warnings,
                        format!(
                            "Foreign key ON {kind} action {action:?} on {display} is unsupported and was omitted"
                        ),
                    );
                }
            }
        }
        statement.push_str(";\n");
        output.sql.push_str(&statement);
    }
    output
}

fn warn(warnings: &mut Vec<Warning>, message: String) {
    warnings.push(Warning { line: 0, message });
}
fn valid_identifier(name: &str) -> bool {
    !name.is_empty() && !name.contains('\0')
}
pub(crate) fn quote(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}
fn qualified(name: &[String]) -> String {
    name.iter()
        .map(|name| quote(name))
        .collect::<Vec<_>>()
        .join(".")
}
fn quoted_columns(columns: &[String]) -> String {
    columns
        .iter()
        .map(|name| quote(name))
        .collect::<Vec<_>>()
        .join(", ")
}
fn canonical_name(name: &[String]) -> Vec<String> {
    if name.len() == 1 {
        vec!["public".into(), effective_identifier(&name[0])]
    } else {
        name.iter().map(|name| effective_identifier(name)).collect()
    }
}
fn valid_column_list(columns: &[String], names: &BTreeSet<String>) -> bool {
    columns.iter().all(|name| names.contains(name))
        && columns.iter().collect::<BTreeSet<_>>().len() == columns.len()
}
fn valid_key(key: &Key, names: &BTreeSet<String>) -> bool {
    !key.columns.is_empty() && valid_column_list(&key.columns, names)
}
fn column_names(table: &ErdTable, ids: &[ColumnId]) -> Vec<String> {
    ids.iter()
        .map(|id| {
            table
                .column(*id)
                .expect("validated stable column id")
                .name
                .clone()
        })
        .collect()
}

fn render_expression(expression: &ErdIndexExpression, table: &ErdTable) -> String {
    crate::current_index_expression(expression, &table.columns)
}

fn parse_type(data_type: &str) -> Option<DataType> {
    let dialect = PostgreSqlDialect {};
    let mut parser = Parser::new(&dialect).try_with_sql(data_type).ok()?;
    let data_type = parser.parse_data_type().ok()?;
    (parser.peek_token().token == Token::EOF).then_some(data_type)
}
fn valid_type(data_type: &str) -> bool {
    parse_type(data_type).is_some()
}
fn parse_default(default: &str) -> Option<Expr> {
    let dialect = PostgreSqlDialect {};
    let mut parser = Parser::new(&dialect).try_with_sql(default).ok()?;
    let expression = parser.parse_expr().ok()?;
    (parser.peek_token().token == Token::EOF).then_some(expression)
}
fn external_type(data_type: &DataType) -> bool {
    match data_type {
        DataType::Custom(name, _) => {
            let name = name.to_string().to_lowercase();
            !matches!(
                name.as_str(),
                "serial"
                    | "bigserial"
                    | "smallserial"
                    | "serial2"
                    | "serial4"
                    | "serial8"
                    | "money"
                    | "inet"
                    | "cidr"
                    | "macaddr"
                    | "macaddr8"
                    | "point"
                    | "line"
                    | "lseg"
                    | "box"
                    | "path"
                    | "polygon"
                    | "circle"
                    | "tsvector"
                    | "tsquery"
                    | "pg_lsn"
                    | "pg_snapshot"
                    | "txid_snapshot"
                    | "xml"
                    | "int2"
                    | "int4"
                    | "int8"
                    | "float4"
                    | "float8"
                    | "bool"
                    | "bpchar"
                    | "timestamptz"
                    | "timetz"
            )
        }
        DataType::Array(
            ArrayElemTypeDef::AngleBracket(inner)
            | ArrayElemTypeDef::SquareBracket(inner, _)
            | ArrayElemTypeDef::Parenthesis(inner),
        ) => external_type(inner),
        _ => false,
    }
}
fn has_sequence_dependency(expression: &Expr) -> bool {
    use sqlparser::ast::visit_expressions;
    use std::ops::ControlFlow;
    matches!(
        visit_expressions(expression, |expression| {
            if matches!(expression, Expr::Function(function) if function.name.to_string().trim_matches('"').eq_ignore_ascii_case("nextval"))
            {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        }),
        ControlFlow::Break(())
    )
}
fn valid_index(sql: &str) -> bool {
    matches!(
        Parser::parse_sql(&PostgreSqlDialect {}, sql).as_deref(),
        Ok([Statement::CreateIndex(_)])
    )
}
pub(crate) fn simple_index_column(sql: &str) -> Option<String> {
    let parsed = Parser::parse_sql(
        &PostgreSqlDialect {},
        &format!("CREATE INDEX __idx__ ON __table__ ({sql});"),
    )
    .ok()?;
    let [Statement::CreateIndex(index)] = parsed.as_slice() else {
        return None;
    };
    let [column] = index.columns.as_slice() else {
        return None;
    };
    // COLLATE is an option on a plain indexed column in PostgreSQL. SQLparser
    // stores it as an expression wrapper, which must not hide that column's
    // eligibility as the target of a foreign key to a unique index.
    let mut expression = &column.column.expr;
    while let Expr::Collate { expr, .. } = expression {
        expression = expr;
    }
    let Expr::Identifier(identifier) = expression else {
        return None;
    };
    Some(if identifier.quote_style.is_some() {
        identifier.value.clone()
    } else {
        identifier.value.to_lowercase()
    })
}

fn infer_foreign_key(
    doc: &Document,
    source: &Endpoint,
    target: &Endpoint,
) -> Option<(ElementId, ColumnId, ElementId, ColumnId)> {
    let row = |endpoint: &Endpoint| {
        let Endpoint::Glued {
            element,
            port: Some(port),
        } = endpoint
        else {
            return None;
        };
        let column = port.column_id()?;
        let table = doc.elements.get(element)?.as_shape()?.erd.as_ref()?;
        let data = table.column(column)?;
        let primary = table.primary_key.as_ref().map_or_else(
            || {
                data.primary_key
                    && table
                        .columns
                        .iter()
                        .filter(|column| column.primary_key)
                        .count()
                        == 1
            },
            |key| key.columns == [column],
        );
        let unique = data.unique || table.unique_keys.iter().any(|key| key.columns == [column]);
        Some((*element, column, primary || unique, data.foreign_key))
    };
    let (source_id, source_column, source_key, source_fk) = row(source)?;
    let (target_id, target_column, target_key, target_fk) = row(target)?;
    if source_id == target_id && source_column == target_column {
        return None;
    }
    if target_key && (!source_key || source_fk && !target_fk) {
        Some((source_id, source_column, target_id, target_column))
    } else if source_key && (!target_key || target_fk && !source_fk) {
        Some((target_id, target_column, source_id, source_column))
    } else {
        None
    }
}

fn effective_identifier(name: &str) -> String {
    let mut end = name.len().min(63);
    while !name.is_char_boundary(end) {
        end -= 1;
    }
    name[..end].into()
}

fn normalize_index_expression(sql: &str) -> Option<String> {
    let parsed = Parser::parse_sql(
        &PostgreSqlDialect {},
        &format!("CREATE INDEX __idx__ ON __table__ ({sql});"),
    )
    .ok()?;
    let [Statement::CreateIndex(index)] = parsed.as_slice() else {
        return None;
    };
    let [column] = index.columns.as_slice() else {
        return None;
    };
    let mut sql = index_value(&column.column.expr);
    if let Some(operator_class) = &column.operator_class {
        write!(sql, " {operator_class}").ok()?;
    }
    sql.push_str(&column.column.options.to_string());
    Some(sql)
}

fn index_value(expression: &Expr) -> String {
    match expression {
        Expr::Identifier(_) | Expr::Function(_) | Expr::Nested(_) => expression.to_string(),
        Expr::Collate { expr, collation } => {
            format!("{} COLLATE {collation}", index_value(expr))
        }
        _ => format!("({expression})"),
    }
}
