//! Conversion into ordinary editable ERD shapes and relationship commands.

use crate::{ImportPreview, Key};
use bp_commands::Command;
use bp_model::kurbo::Rect;
use bp_model::{
    ColumnId, Dash, Document, Element, ElementId, Endpoint, ErdColumn, ErdForeignKey, ErdIndex,
    ErdIndexExpression, ErdIndexReference, ErdKey, ErdTable, Marker, OrderKey, Parent, PortId,
    ShapeRef, SqlDialect, TableDisplay,
};
use std::collections::BTreeMap;

/// Build one transaction, inserting tables before their relationships.
/// The caller applies the returned commands with a single `History::apply`.
pub fn import_commands(
    doc: &Document,
    parent: Parent,
    preview: &ImportPreview,
) -> Result<(Vec<ElementId>, Vec<Command>), String> {
    doc.check_parent(ElementId::new(), parent)
        .map_err(|error| error.to_string())?;
    let mut tables = Vec::new();
    let mut names = BTreeMap::new();
    let mut order = doc.next_order_key(parent);
    for table in &preview.schema.tables {
        let Some(name) = table.name.last() else {
            return Err("A table has no name".into());
        };
        if names.insert(table.name.clone(), tables.len()).is_some() {
            return Err(format!("Duplicate table name {}", table.name.join(".")));
        }
        let mut column_order = OrderKey::first();
        let mut columns = Vec::new();
        for source in &table.columns {
            if columns
                .iter()
                .any(|column: &ErdColumn| column.name == source.name)
            {
                return Err(format!("Duplicate column {} in {name}", source.name));
            }
            let mut column = ErdColumn::new(&source.name, &source.data_type, column_order.clone());
            column_order = OrderKey::after(&column_order);
            column.nullable = source.nullable;
            column.default_value.clone_from(&source.default_value);
            columns.push(column);
        }
        let mut erd = ErdTable {
            dialect: SqlDialect::PostgreSql,
            display: TableDisplay::All,
            schema: table.name[..table.name.len() - 1].to_vec(),
            primary_key: table
                .primary_key
                .as_ref()
                .map(|key| bind_key(key, &columns))
                .transpose()?,
            unique_keys: table
                .unique_keys
                .iter()
                .map(|key| bind_key(key, &columns))
                .collect::<Result<_, _>>()?,
            indexes: table
                .indexes
                .iter()
                .map(|index| ErdIndex {
                    name: index.name.clone(),
                    unique: index.unique,
                    columns: index
                        .columns
                        .iter()
                        .map(|sql| bind_index_expression(sql, &columns))
                        .collect(),
                    method: index.method.clone(),
                    predicate: index
                        .predicate
                        .as_deref()
                        .map(|sql| bind_index_expression(sql, &columns)),
                })
                .collect(),
            columns,
        };
        erd.sync_key_flags();
        let mut element = Element::shape(
            ShapeRef::new("erd", "table"),
            parent,
            order.clone(),
            Rect::ZERO,
        );
        order = OrderKey::after(&order);
        let shape = element.as_shape_mut().expect("created a shape");
        shape.text.clone_from(name);
        shape.erd = Some(erd);
        tables.push(element);
    }
    layout(doc, parent, &mut tables);
    let mut relationships = Vec::new();
    for key in &preview.schema.foreign_keys {
        let source = *names
            .get(&key.table)
            .ok_or_else(|| format!("Foreign key table {} is missing", key.table.join(".")))?;
        let target = *names.get(&key.referenced_table).ok_or_else(|| {
            format!(
                "Referenced table {} is missing",
                key.referenced_table.join(".")
            )
        })?;
        let source_columns = resolve_columns(
            &key.columns,
            tables[source].as_shape().unwrap().erd.as_ref().unwrap(),
        )?;
        let target_table = tables[target].as_shape().unwrap().erd.as_ref().unwrap();
        let target_columns = if key.referenced_columns.is_empty() {
            target_table
                .primary_key
                .as_ref()
                .map(|key| key.columns.clone())
                .ok_or_else(|| {
                    format!(
                        "Referenced table {} has no primary key",
                        key.referenced_table.join(".")
                    )
                })?
        } else {
            resolve_columns(&key.referenced_columns, target_table)?
        };
        if source_columns.is_empty() || source_columns.len() != target_columns.len() {
            return Err("A foreign key must map equally many columns".into());
        }
        let (nullable, unique, identifying) = {
            let table = tables[source].as_shape_mut().unwrap().erd.as_mut().unwrap();
            for column in &mut table.columns {
                if source_columns.contains(&column.id) {
                    column.foreign_key = true;
                }
            }
            (
                source_columns
                    .iter()
                    .any(|id| table.column(*id).unwrap().nullable),
                table
                    .primary_key
                    .iter()
                    .chain(&table.unique_keys)
                    .any(|key| key.columns.iter().all(|id| source_columns.contains(id)))
                    || table.indexes.iter().any(|index| {
                        index.unique
                            && index.predicate.is_none()
                            && index.columns.iter().all(|expression| {
                                crate::export::simple_index_column(&current_index_expression(
                                    expression,
                                    &table.columns,
                                ))
                                .and_then(|name| {
                                    table.columns.iter().find(|column| column.name == name)
                                })
                                .is_some_and(|column| source_columns.contains(&column.id))
                            })
                    }),
                table
                    .primary_key
                    .as_ref()
                    .is_some_and(|key| source_columns.iter().all(|id| key.columns.contains(id))),
            )
        };
        let mut connector = Element::connector(
            Endpoint::Glued {
                element: tables[source].id,
                port: Some(PortId::column(source_columns[0], false)),
            },
            Endpoint::Glued {
                element: tables[target].id,
                port: Some(PortId::column(target_columns[0], true)),
            },
            parent,
            order.clone(),
        );
        order = OrderKey::after(&order);
        let connection = connector.as_connector_mut().unwrap();
        connection.start_marker = if unique {
            Marker::ZeroOrOne
        } else {
            Marker::ZeroOrMany
        };
        connection.end_marker = if nullable {
            Marker::ZeroOrOne
        } else {
            Marker::ExactlyOne
        };
        connection.style.dash = Some(if identifying {
            Dash::Solid
        } else {
            Dash::Dashed
        });
        connection.foreign_key = Some(ErdForeignKey {
            owner_at_target: false,
            name: key.name.clone(),
            columns: source_columns,
            referenced_columns: target_columns,
            on_delete: key.on_delete.clone(),
            on_update: key.on_update.clone(),
        });
        relationships.push(connector);
    }
    let elements: Vec<_> = tables.into_iter().chain(relationships).collect();
    let ids = elements.iter().map(|element| element.id).collect();
    let mut candidate = doc.clone();
    for element in &elements {
        candidate.elements.insert(element.id, element.clone());
    }
    candidate.validate().map_err(|error| error.to_string())?;
    Ok((
        ids,
        elements
            .into_iter()
            .map(|element| Command::Insert(Box::new(element)))
            .collect(),
    ))
}

fn bind_key(key: &Key, columns: &[ErdColumn]) -> Result<ErdKey, String> {
    Ok(ErdKey {
        name: key.name.clone(),
        columns: key
            .columns
            .iter()
            .map(|name| {
                columns
                    .iter()
                    .find(|column| &column.name == name)
                    .map(|column| column.id)
                    .ok_or_else(|| format!("Key references missing column {name}"))
            })
            .collect::<Result<_, _>>()?,
    })
}

fn resolve_columns(names: &[String], table: &ErdTable) -> Result<Vec<ColumnId>, String> {
    names
        .iter()
        .map(|name| {
            table
                .columns
                .iter()
                .find(|column| &column.name == name)
                .map(|column| column.id)
                .ok_or_else(|| format!("Foreign key references missing column {name}"))
        })
        .collect()
}

fn layout(doc: &Document, parent: Parent, tables: &mut [Element]) {
    let grid_columns = (tables.len() as f64).sqrt().ceil().clamp(1.0, 4.0) as usize;
    let mut widths = vec![0.0_f64; grid_columns];
    let mut heights = vec![0.0_f64; tables.len().div_ceil(grid_columns)];
    for (i, element) in tables.iter_mut().enumerate() {
        let shape = element.as_shape_mut().unwrap();
        let table = shape.erd.as_ref().unwrap();
        let width = table.columns.iter().fold(
            (shape.text.chars().count() as f64 * 9.0 + 30.0).max(280.0),
            |width, column| {
                width.max(
                    (column.name.chars().count()
                        + column.data_type.chars().count()
                        + column
                            .default_value
                            .as_ref()
                            .map_or(0, |value| value.chars().count())) as f64
                        * 9.0
                        + 180.0,
                )
            },
        );
        let height = 34.0 + table.columns.len() as f64 * 30.0;
        shape.bounds = Rect::new(0.0, 0.0, width, height);
        widths[i % grid_columns] = widths[i % grid_columns].max(width);
        heights[i / grid_columns] = heights[i / grid_columns].max(height);
    }
    let origin_x = doc
        .elements
        .values()
        .filter(|element| element.parent == parent)
        .filter_map(Element::as_shape)
        .map(|shape| shape.bounds.x1 + 100.0)
        .fold(40.0_f64, f64::max);
    for (i, element) in tables.iter_mut().enumerate() {
        let x = origin_x
            + widths[..i % grid_columns]
                .iter()
                .map(|width| width + 100.0)
                .sum::<f64>();
        let y = 40.0
            + heights[..i / grid_columns]
                .iter()
                .map(|height| height + 100.0)
                .sum::<f64>();
        let bounds = &mut element.as_shape_mut().unwrap().bounds;
        *bounds = Rect::new(x, y, x + bounds.width(), y + bounds.height());
    }
}

/// Bind identifier occurrences to stable columns. Literals, function names,
/// qualifier prefixes, and cast type names are kept as SQL text.
pub fn bind_index_expression(sql: &str, columns: &[ErdColumn]) -> ErdIndexExpression {
    use sqlparser::{
        ast::{Expr, visit_expressions},
        dialect::PostgreSqlDialect,
        parser::Parser,
    };
    use std::ops::ControlFlow;
    let mut wrapped = format!("SELECT {sql}");
    let mut prefix = "SELECT ".len();
    let statements = match Parser::parse_sql(&PostgreSqlDialect {}, &wrapped) {
        Ok(statements) => statements,
        Err(_) => {
            let index_prefix = "CREATE INDEX __blueprint__ ON __table__ (";
            wrapped = format!("{index_prefix}{sql});");
            prefix = index_prefix.len();
            match Parser::parse_sql(&PostgreSqlDialect {}, &wrapped) {
                Ok(statements) => statements,
                Err(_) => {
                    return ErdIndexExpression {
                        sql: sql.into(),
                        references: Vec::new(),
                    };
                }
            }
        }
    };
    let mut references = Vec::new();
    let _: ControlFlow<()> = visit_expressions(&statements, |expression| {
        let identifier = match expression {
            Expr::Identifier(identifier) => Some(identifier),
            Expr::CompoundIdentifier(identifiers) => identifiers.last(),
            _ => None,
        };
        if let Some(identifier) = identifier {
            let name = if identifier.quote_style.is_some() {
                identifier.value.clone()
            } else {
                identifier.value.to_lowercase()
            };
            if let Some(column) = columns.iter().find(|column| column.name == name)
                && let (Some(start), Some(end)) = (
                    location_offset(&wrapped, identifier.span.start),
                    location_offset(&wrapped, identifier.span.end),
                )
                && start >= prefix
                && end <= prefix + sql.len()
            {
                references.push(ErdIndexReference {
                    column: column.id,
                    start: start - prefix,
                    end: end - prefix,
                });
            }
        }
        ControlFlow::Continue(())
    });
    references.sort_by_key(|reference| reference.start);
    references.dedup_by_key(|reference| (reference.start, reference.end));
    ErdIndexExpression {
        sql: sql.into(),
        references,
    }
}

fn location_offset(sql: &str, location: sqlparser::tokenizer::Location) -> Option<usize> {
    let mut line = 1;
    let mut column = 1;
    for (offset, character) in sql.char_indices() {
        if line == location.line && column == location.column {
            return Some(offset);
        }
        if character == '\n' {
            line += 1;
            column = 1;
        } else {
            column += 1;
        }
    }
    (line == location.line && column == location.column).then_some(sql.len())
}

/// Render bound identifiers using current names, keeping untouched identifiers
/// in their original spelling. Used by both the SQL preview and inspector.
pub fn current_index_expression(expression: &ErdIndexExpression, columns: &[ErdColumn]) -> String {
    let mut sql = String::new();
    let mut at = 0;
    for reference in &expression.references {
        let Some(column) = columns.iter().find(|column| column.id == reference.column) else {
            continue;
        };
        let Some(original) = expression.sql.get(reference.start..reference.end) else {
            continue;
        };
        let decoded = if original.starts_with('"') && original.ends_with('"') {
            original[1..original.len() - 1].replace("\"\"", "\"")
        } else {
            original.to_lowercase()
        };
        sql.push_str(&expression.sql[at..reference.start]);
        if decoded == column.name {
            sql.push_str(original);
        } else {
            sql.push_str(&crate::export::quote(&column.name));
        }
        at = reference.end;
    }
    sql.push_str(&expression.sql[at..]);
    sql
}
