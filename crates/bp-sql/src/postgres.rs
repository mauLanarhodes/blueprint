//! PostgreSQL DDL parsing. Statements are isolated before AST parsing so a
//! procedure, an unsupported statement, or a malformed definition cannot make
//! supported statements elsewhere in the script disappear.

use crate::{Column, ForeignKey, ImportPreview, Index, Key, QualifiedName, Table, Warning};
use sqlparser::ast::{
    AlterTableOperation, ColumnDef, ColumnOption, CreateIndex, CreateTable, CreateTableOptions,
    Expr, ForeignKeyConstraint, Ident, IndexColumn, IndexType, NullsDistinctOption, ObjectName,
    SchemaName, Spanned, Statement, TableConstraint,
};
use sqlparser::dialect::PostgreSqlDialect;
use sqlparser::parser::Parser;
use sqlparser::tokenizer::{Token, TokenWithSpan, Tokenizer};

#[derive(Default)]
struct Importer {
    preview: ImportPreview,
    constraints: Vec<(QualifiedName, TableConstraint, usize)>,
    indexes: Vec<(CreateIndex, usize)>,
    foreign_keys: Vec<(ForeignKey, usize)>,
    key_lines: Vec<(QualifiedName, bool, Vec<String>, usize)>,
    recovered_column_bases: Vec<(QualifiedName, String, usize)>,
}

pub(super) fn parse(sql: &str) -> ImportPreview {
    let mut importer = Importer::default();
    let (statements, warnings) = split_statements(sql);
    importer.preview.warnings = warnings;
    for (source, base_line) in statements {
        let line = first_token_line(source, base_line);
        match Parser::parse_sql(&PostgreSqlDialect {}, source) {
            Ok(statements) => {
                for statement in statements {
                    importer.statement(statement, base_line, line);
                }
            }
            Err(error) => {
                if !importer.recover_table(source, base_line) {
                    importer.warn(line, format!("Statement was not imported: {error}"));
                }
            }
        }
    }
    importer.resolve();
    importer
        .preview
        .warnings
        .sort_by_key(|warning| warning.line);
    importer.preview
}

impl Importer {
    fn warn(&mut self, line: usize, message: impl Into<String>) {
        self.preview.warnings.push(Warning {
            line,
            message: message.into(),
        });
    }

    fn statement(&mut self, statement: Statement, base_line: usize, line: usize) {
        match statement {
            Statement::CreateTable(table) => self.table(table, base_line, line),
            Statement::CreateIndex(index) => self.indexes.push((index, line)),
            Statement::CreateSchema {
                schema_name,
                with,
                options,
                ..
            } => {
                if !matches!(schema_name, SchemaName::Simple(_))
                    || with.is_some()
                    || options.is_some()
                {
                    self.warn(
                        line,
                        "Schema authorization and options are not represented in the ERD",
                    );
                }
            }
            Statement::AlterTable(table) => {
                let name = object_name(&table.name);
                for operation in table.operations {
                    match operation {
                        AlterTableOperation::AddConstraint {
                            constraint,
                            not_valid,
                        } => {
                            let constraint_line = node_line(&constraint, base_line, line);
                            if not_valid {
                                self.warn(constraint_line, "NOT VALID constraint validation state is not represented in the ERD");
                            }
                            self.constraints
                                .push((name.clone(), constraint, constraint_line));
                        }
                        operation => self.warn(
                            line,
                            format!("Unsupported ALTER TABLE operation: {operation}"),
                        ),
                    }
                }
            }
            statement => {
                let kind = statement
                    .to_string()
                    .split_whitespace()
                    .take(3)
                    .collect::<Vec<_>>()
                    .join(" ");
                self.warn(
                    line,
                    format!(
                        "Unsupported SQL statement ({kind}); only schema definitions are imported"
                    ),
                );
            }
        }
    }

    fn table(&mut self, ast: CreateTable, base_line: usize, line: usize) {
        let name = object_name(&ast.name);
        if self
            .preview
            .schema
            .tables
            .iter()
            .any(|table| table.name == name)
        {
            self.warn(
                line,
                format!("Duplicate table {} was not imported", name.join(".")),
            );
            return;
        }
        if ast.query.is_some() || ast.like.is_some() || ast.clone.is_some() {
            self.warn(line, "CREATE TABLE AS/LIKE/CLONE cannot infer columns; only explicit column definitions are imported");
        }
        if ast.temporary || ast.on_commit.is_some() {
            self.warn(
                line,
                "Temporary table lifetime and ON COMMIT behavior are not represented in the ERD",
            );
        }
        if ast.inherits.is_some()
            || ast.partition_of.is_some()
            || ast.partition_by.is_some()
            || ast.for_values.is_some()
        {
            self.warn(line, "Table inheritance and partition definitions are not represented; only explicit columns are imported");
        }
        if ast.table_options != CreateTableOptions::None
            || ast.location.is_some()
            || ast.comment.is_some()
        {
            self.warn(
                line,
                "Table storage options and comments are not represented in the ERD",
            );
        }
        let mut table = Table {
            name: name.clone(),
            columns: vec![],
            primary_key: None,
            unique_keys: vec![],
            indexes: vec![],
        };
        for column in ast.columns {
            self.column(&mut table, column, base_line, line);
        }
        for constraint in ast.constraints {
            let constraint_line = node_line(&constraint, base_line, line);
            self.constraint(&mut table, constraint, constraint_line);
        }
        if table.columns.is_empty() {
            self.warn(
                line,
                format!("Table {} has no supported explicit columns", name.join(".")),
            );
        }
        self.preview.schema.tables.push(table);
    }

    fn column(&mut self, table: &mut Table, ast: ColumnDef, base_line: usize, line: usize) {
        let name = identifier(&ast.name);
        let column_line = ast.name.span.start.line as usize + base_line - 1;
        if table.columns.iter().any(|column| column.name == name) {
            self.warn(
                column_line,
                format!("Duplicate column {name} was not imported"),
            );
            return;
        }
        let mut column = Column {
            name: name.clone(),
            data_type: ast.data_type.to_string(),
            nullable: !matches!(
                ast.data_type.to_string().to_ascii_lowercase().as_str(),
                "serial" | "smallserial" | "bigserial" | "serial2" | "serial4" | "serial8"
            ),
            default_value: None,
        };
        for definition in ast.options {
            let option_base = self
                .recovered_column_bases
                .iter()
                .find(|(table_name, column_name, _)| {
                    table_name == &table.name && column_name == &name
                })
                .map(|(_, _, base)| *base)
                .unwrap_or(base_line);
            let option_line = node_line(&definition.option, option_base, column_line.max(line));
            let constraint_name = definition.name.as_ref().map(identifier);
            match definition.option {
                ColumnOption::Null => column.nullable = true,
                ColumnOption::NotNull => column.nullable = false,
                ColumnOption::Default(expression) => {
                    column.default_value = Some(expression.to_string())
                }
                ColumnOption::PrimaryKey(mut key) => {
                    key.name = definition.name.or(key.name);
                    key.columns = vec![ast.name.clone().into()];
                    self.constraint(table, TableConstraint::PrimaryKey(key), option_line);
                    column.nullable = false;
                }
                ColumnOption::Unique(mut key) => {
                    key.name = definition.name.or(key.name);
                    key.columns = vec![ast.name.clone().into()];
                    self.constraint(table, TableConstraint::Unique(key), option_line);
                }
                ColumnOption::ForeignKey(mut foreign_key) => {
                    foreign_key.name = definition.name.or(foreign_key.name);
                    foreign_key.columns = vec![ast.name.clone()];
                    self.foreign_key(&table.name, foreign_key, option_line);
                }
                option @ ColumnOption::Generated {
                    generation_expr: None,
                    ..
                } => {
                    // Identity columns imply NOT NULL even though sequence
                    // generation itself is currently unsupported.
                    column.nullable = false;
                    self.warn(
                        option_line,
                        format!("Column {name}: unsupported option {option}"),
                    );
                }
                option => self.warn(
                    option_line,
                    format!(
                        "Column {name}{}: unsupported option {option}",
                        constraint_name
                            .map(|name| format!(" ({name})"))
                            .unwrap_or_default()
                    ),
                ),
            }
        }
        table.columns.push(column);
    }

    fn constraint(&mut self, table: &mut Table, constraint: TableConstraint, line: usize) {
        match constraint {
            TableConstraint::PrimaryKey(key) => {
                if key.characteristics.is_some()
                    || !key.index_options.is_empty()
                    || key.index_type.is_some()
                {
                    self.warn(
                        line,
                        "Primary key timing and index options are not represented in the ERD",
                    );
                }
                if let Some(columns) = key_columns(&key.columns) {
                    self.key_lines
                        .push((table.name.clone(), true, columns.clone(), line));
                    if table.primary_key.is_some() {
                        self.warn(line, "Additional primary key was not imported; a table can have only one primary key");
                    } else {
                        table.primary_key = Some(Key {
                            name: key.name.as_ref().map(identifier),
                            columns,
                        });
                    }
                } else {
                    self.warn(
                        line,
                        "Primary key expressions are unsupported; the key was not imported",
                    );
                }
            }
            TableConstraint::Unique(key) => {
                if key.characteristics.is_some()
                    || !key.index_options.is_empty()
                    || key.index_type.is_some()
                    || key.nulls_distinct == NullsDistinctOption::NotDistinct
                {
                    self.warn(line, "Unique key timing, NULLS NOT DISTINCT, and index options are not represented in the ERD");
                }
                if let Some(columns) = key_columns(&key.columns) {
                    self.key_lines
                        .push((table.name.clone(), false, columns.clone(), line));
                    table.unique_keys.push(Key {
                        name: key.name.as_ref().map(identifier),
                        columns,
                    });
                } else {
                    self.warn(
                        line,
                        "Unique key expressions are unsupported; the key was not imported",
                    );
                }
            }
            TableConstraint::ForeignKey(foreign_key) => {
                self.foreign_key(&table.name, foreign_key, line)
            }
            constraint => self.warn(line, format!("Unsupported table constraint: {constraint}")),
        }
    }

    fn foreign_key(&mut self, table: &QualifiedName, ast: ForeignKeyConstraint, line: usize) {
        if ast.match_kind.is_some() || ast.characteristics.is_some() {
            self.warn(
                line,
                "Foreign key MATCH and constraint timing are not represented in the ERD",
            );
        }
        self.foreign_keys.push((
            ForeignKey {
                name: ast.name.as_ref().map(identifier),
                table: table.clone(),
                columns: ast.columns.iter().map(identifier).collect(),
                referenced_table: object_name(&ast.foreign_table),
                referenced_columns: ast.referred_columns.iter().map(identifier).collect(),
                on_delete: ast.on_delete.map(|action| action.to_string()),
                on_update: ast.on_update.map(|action| action.to_string()),
            },
            line,
        ));
    }

    fn resolve(&mut self) {
        // ALTER constraints and indexes may appear before the target CREATE TABLE.
        for (name, constraint, line) in std::mem::take(&mut self.constraints) {
            if let Some(index) = find_table(&self.preview.schema.tables, &name, &[]) {
                let mut table = self.preview.schema.tables.remove(index);
                self.constraint(&mut table, constraint, line);
                self.preview.schema.tables.insert(index, table);
            } else {
                self.warn(
                    line,
                    format!(
                        "ALTER TABLE target {} was not found or is ambiguous",
                        name.join(".")
                    ),
                );
            }
        }
        let mut index_names = std::collections::BTreeSet::new();
        for (ast, line) in std::mem::take(&mut self.indexes) {
            let table_name = object_name(&ast.table_name);
            let Some(table_index) = find_table(&self.preview.schema.tables, &table_name, &[])
            else {
                self.warn(
                    line,
                    format!(
                        "Index target {} was not found or is ambiguous",
                        table_name.join(".")
                    ),
                );
                continue;
            };
            let name = ast
                .name
                .as_ref()
                .map(object_name)
                .and_then(|parts| parts.last().cloned())
                .unwrap_or_else(|| {
                    format!(
                        "{}_idx_{}",
                        self.preview.schema.tables[table_index]
                            .name
                            .last()
                            .map(String::as_str)
                            .unwrap_or("table"),
                        self.preview.schema.tables[table_index].indexes.len() + 1
                    )
                });
            if ast.name.as_ref().is_some_and(|name| name.0.len() > 1) {
                self.warn(line,"Index schema qualification is not represented separately; the index uses its table's schema");
            }
            let table = &self.preview.schema.tables[table_index];
            let schema = table.name[..table.name.len().saturating_sub(1)].to_vec();
            if !index_names.insert((schema, name.clone())) {
                self.warn(line, format!("Duplicate index {name} was not imported"));
                continue;
            }
            if ast.columns.is_empty() || ast.columns.iter().any(|column|matches!(&column.column.expr, Expr::Identifier(name) if !table.columns.iter().any(|column|column.name==identifier(name)))) {
                self.warn(line,format!("Index {name} contains a missing column; index was not imported"));
                continue;
            }
            if !ast.include.is_empty()
                || ast.nulls_distinct == Some(false)
                || !ast.with.is_empty()
                || !ast.index_options.is_empty()
                || !ast.alter_options.is_empty()
            {
                self.warn(line,"Index INCLUDE, NULLS NOT DISTINCT, and storage options are not represented in the ERD");
            }
            let index = Index {
                name,
                unique: ast.unique,
                columns: ast.columns.iter().map(index_column).collect(),
                method: ast.using.map(|method| match method {
                    IndexType::Custom(name) => identifier(&name),
                    method => method.to_string().to_ascii_lowercase(),
                }),
                predicate: ast.predicate.map(|predicate| predicate.to_string()),
            };
            self.preview.schema.tables[table_index].indexes.push(index);
        }
        for mut table in std::mem::take(&mut self.preview.schema.tables) {
            let key_is_valid = |key: &Key| {
                !key.columns.is_empty()
                    && !has_duplicates(&key.columns)
                    && key
                        .columns
                        .iter()
                        .all(|name| table.columns.iter().any(|column| &column.name == name))
            };
            if table
                .primary_key
                .as_ref()
                .is_some_and(|key| !key_is_valid(key))
            {
                let key = table.primary_key.take().unwrap();
                let line = self.key_line(&table.name, true, &key.columns);
                self.warn(
                    line,
                    format!(
                        "Primary key on {} has missing or duplicate columns; key was not imported",
                        table.name.join(".")
                    ),
                );
            }
            let mut unique_keys = vec![];
            for key in std::mem::take(&mut table.unique_keys) {
                if key_is_valid(&key) {
                    unique_keys.push(key);
                } else {
                    let line = self.key_line(&table.name, false, &key.columns);
                    self.warn(line,format!("Unique key on {} has missing or duplicate columns; key was not imported",table.name.join(".")));
                }
            }
            table.unique_keys = unique_keys;
            if let Some(key) = &table.primary_key {
                for column in &mut table.columns {
                    if key.columns.contains(&column.name) {
                        column.nullable = false;
                    }
                }
            }
            self.preview.schema.tables.push(table);
        }
        for (mut foreign_key, line) in std::mem::take(&mut self.foreign_keys) {
            let Some(source) = find_table(&self.preview.schema.tables, &foreign_key.table, &[])
            else {
                self.warn(
                    line,
                    format!(
                        "Foreign key source {} was not found",
                        foreign_key.table.join(".")
                    ),
                );
                continue;
            };
            let source_table = &self.preview.schema.tables[source];
            let source_schema = &source_table.name[..source_table.name.len().saturating_sub(1)];
            let Some(target) = find_table(
                &self.preview.schema.tables,
                &foreign_key.referenced_table,
                source_schema,
            ) else {
                self.warn(line,format!("Foreign key reference {} was not found or is ambiguous; relationship was not imported",foreign_key.referenced_table.join(".")));
                continue;
            };
            let target_table = &self.preview.schema.tables[target];
            if foreign_key.referenced_columns.is_empty() {
                foreign_key.referenced_columns = target_table
                    .primary_key
                    .as_ref()
                    .map(|key| key.columns.clone())
                    .unwrap_or_default();
            }
            if foreign_key.columns.is_empty()
                || has_duplicates(&foreign_key.columns)
                || has_duplicates(&foreign_key.referenced_columns)
                || foreign_key.columns.len() != foreign_key.referenced_columns.len()
                || !foreign_key.columns.iter().all(|name| {
                    source_table
                        .columns
                        .iter()
                        .any(|column| &column.name == name)
                })
                || !foreign_key.referenced_columns.iter().all(|name| {
                    target_table
                        .columns
                        .iter()
                        .any(|column| &column.name == name)
                })
            {
                self.warn(line,"Foreign key has missing columns or mismatched key lengths; relationship was not imported");
                continue;
            }
            foreign_key.table = source_table.name.clone();
            foreign_key.referenced_table = target_table.name.clone();
            self.preview.schema.foreign_keys.push(foreign_key);
        }
    }

    fn key_line(&self, table: &[String], primary: bool, columns: &[String]) -> usize {
        self.key_lines
            .iter()
            .find(|(name, is_primary, key_columns, _)| {
                name == table && *is_primary == primary && key_columns == columns
            })
            .map(|(_, _, _, line)| *line)
            .unwrap_or(1)
    }

    /// Recover independent column/constraint definitions when a table contains
    /// syntax the AST parser cannot handle. The warning explicitly identifies
    /// each omitted fragment; supported columns still become editable rows.
    fn recover_table(&mut self, source: &str, base_line: usize) -> bool {
        let Ok(tokens) = Tokenizer::new(&PostgreSqlDialect {}, source).tokenize_with_location()
        else {
            return false;
        };
        let tokens: Vec<_> = tokens
            .into_iter()
            .filter(|token| !matches!(token.token, Token::Whitespace(_)))
            .collect();
        let Some(table_token) = tokens
            .iter()
            .position(|token| token.token.to_string().eq_ignore_ascii_case("TABLE"))
        else {
            return false;
        };
        if !tokens
            .first()
            .is_some_and(|token| token.token.to_string().eq_ignore_ascii_case("CREATE"))
        {
            return false;
        }
        let mut name_token = table_token + 1;
        if tokens
            .get(name_token)
            .is_some_and(|token| token.token.to_string().eq_ignore_ascii_case("IF"))
        {
            name_token += 3;
        }
        let Some(open) = tokens
            .iter()
            .enumerate()
            .skip(name_token)
            .find(|(_, token)| matches!(token.token, Token::LParen))
            .map(|(index, _)| index)
        else {
            return false;
        };
        let name_start = token_start(source, &tokens[name_token]);
        let name_end = token_start(source, &tokens[open]);
        let header = format!("CREATE TABLE {}", &source[name_start..name_end]);
        let Ok(mut empty) = Parser::parse_sql(&PostgreSqlDialect {}, &format!("{header} ()"))
        else {
            return false;
        };
        let Some(Statement::CreateTable(mut table)) = empty.pop() else {
            return false;
        };
        if tokens[1..table_token]
            .iter()
            .any(|token| token.token.to_string().eq_ignore_ascii_case("UNLOGGED"))
        {
            self.warn(
                first_token_line(source, base_line),
                "UNLOGGED table persistence is not represented in the ERD",
            );
        }
        let mut depth = 1;
        let mut bracket_depth = 0;
        let mut fragment_start = token_end(source, &tokens[open]);
        let mut closed = false;
        for token in tokens.iter().skip(open + 1) {
            match token.token {
                Token::LParen => depth += 1,
                Token::LBracket => bracket_depth += 1,
                Token::RBracket => bracket_depth -= 1,
                Token::RParen => {
                    depth -= 1;
                    if depth == 0 {
                        self.recover_definition(
                            source,
                            fragment_start,
                            token_start(source, token),
                            base_line,
                            &mut table,
                        );
                        let tail = source[token_end(source, token)..]
                            .trim()
                            .trim_end_matches(';')
                            .trim();
                        if !tail.is_empty() {
                            self.warn(
                                base_line + token.span.start.line as usize - 1,
                                format!("Unsupported table suffix was not imported: {tail}"),
                            );
                        }
                        closed = true;
                        break;
                    }
                }
                Token::Comma if depth == 1 && bracket_depth == 0 => {
                    self.recover_definition(
                        source,
                        fragment_start,
                        token_start(source, token),
                        base_line,
                        &mut table,
                    );
                    fragment_start = token_end(source, token);
                }
                _ => {}
            }
        }
        if !closed {
            self.warn(base_line,"Incomplete CREATE TABLE definition; only complete column definitions were imported");
        }
        self.table(table, 1, first_token_line(source, base_line));
        true
    }

    fn recover_definition(
        &mut self,
        source: &str,
        start: usize,
        end: usize,
        base_line: usize,
        table: &mut CreateTable,
    ) {
        let fragment = &source[start..end];
        if fragment.trim().is_empty() {
            return;
        }
        let line = first_token_line(
            fragment,
            base_line
                + source[..start]
                    .bytes()
                    .filter(|byte| *byte == b'\n')
                    .count(),
        );
        let wrapper = format!("CREATE TABLE \"__recovery\" ({fragment})");
        if let Ok(mut statements) = Parser::parse_sql(&PostgreSqlDialect {}, &wrapper)
            && let Some(Statement::CreateTable(mut recovered)) = statements.pop()
        {
            // AST locations are relative to the wrapper. Adjust the first
            // line (later lines already retain the fragment's newlines).
            for column in &mut recovered.columns {
                self.recovered_column_bases.push((
                    object_name(&table.name),
                    identifier(&column.name),
                    base_line
                        + source[..start]
                            .bytes()
                            .filter(|byte| *byte == b'\n')
                            .count(),
                ));
                column.name.span.start.line = line as u64;
            }
            table.columns.extend(recovered.columns);
            // Constraints are imported separately to retain source lines.
            for constraint in recovered.constraints {
                self.constraints
                    .push((object_name(&table.name), constraint, line));
            }
            return;
        }
        // An unsupported trailing column option should not erase the column.
        if let Ok(tokens) = Tokenizer::new(&PostgreSqlDialect {}, fragment).tokenize_with_location()
        {
            for token in tokens.iter().rev() {
                let cutoff = token_start(fragment, token);
                if cutoff == 0 {
                    continue;
                }
                let prefix = &fragment[..cutoff];
                if let Ok(mut statements) = Parser::parse_sql(
                    &PostgreSqlDialect {},
                    &format!("CREATE TABLE \"__recovery\" ({prefix})"),
                ) && let Some(Statement::CreateTable(mut recovered)) = statements.pop()
                    && recovered.columns.len() == 1
                    && recovered.constraints.is_empty()
                {
                    self.recovered_column_bases.push((
                        object_name(&table.name),
                        identifier(&recovered.columns[0].name),
                        base_line
                            + source[..start]
                                .bytes()
                                .filter(|byte| *byte == b'\n')
                                .count(),
                    ));
                    recovered.columns[0].name.span.start.line = line as u64;
                    table.columns.extend(recovered.columns);
                    self.warn(
                        line,
                        format!(
                            "Unsupported column suffix was not imported: {}",
                            fragment[cutoff..].trim()
                        ),
                    );
                    return;
                }
            }
        }
        self.warn(
            line,
            format!(
                "Unsupported table definition was not imported: {}",
                fragment.trim()
            ),
        );
    }
}

fn identifier(ident: &Ident) -> String {
    if ident.quote_style.is_some() {
        ident.value.clone()
    } else {
        ident.value.to_lowercase()
    }
}

fn object_name(name: &ObjectName) -> QualifiedName {
    name.0
        .iter()
        .map(|part| {
            part.as_ident()
                .map(identifier)
                .unwrap_or_else(|| part.to_string())
        })
        .collect()
}

fn key_columns(columns: &[IndexColumn]) -> Option<Vec<String>> {
    columns
        .iter()
        .map(|column| match &column.column.expr {
            Expr::Identifier(name) => Some(identifier(name)),
            _ => None,
        })
        .collect()
}

fn index_column(column: &IndexColumn) -> String {
    // SQLparser's IndexColumn Display prints the operator class after DESC/
    // NULLS ordering. PostgreSQL requires it between the expression and those
    // options, so assemble that order explicitly.
    let mut sql = column.column.expr.to_string();
    if let Some(operator_class) = &column.operator_class {
        sql.push(' ');
        sql.push_str(&operator_class.to_string());
    }
    sql.push_str(&column.column.options.to_string());
    sql
}

fn has_duplicates(names: &[String]) -> bool {
    names
        .iter()
        .enumerate()
        .any(|(index, name)| names[..index].contains(name))
}

fn node_line(node: &impl Spanned, base_line: usize, fallback: usize) -> usize {
    let line = node.span().start.line as usize;
    if line == 0 {
        fallback
    } else {
        base_line + line - 1
    }
}

fn find_table(tables: &[Table], name: &[String], schema: &[String]) -> Option<usize> {
    if let Some(index) = tables.iter().position(|table| table.name == name) {
        return Some(index);
    }
    if name.len() == 1 {
        let qualified: Vec<_> = schema.iter().cloned().chain(name.iter().cloned()).collect();
        if let Some(index) = tables.iter().position(|table| table.name == qualified) {
            return Some(index);
        }
        let public = vec!["public".to_owned(), name[0].clone()];
        if let Some(index) = tables.iter().position(|table| table.name == public) {
            return Some(index);
        }
        let mut matches = tables
            .iter()
            .enumerate()
            .filter(|(_, table)| table.name.last() == name.last());
        let first = matches.next()?.0;
        if matches.next().is_none() {
            return Some(first);
        }
    }
    None
}

fn first_token_line(source: &str, base_line: usize) -> usize {
    Tokenizer::new(&PostgreSqlDialect {}, source)
        .tokenize_with_location()
        .ok()
        .and_then(|tokens| {
            tokens
                .into_iter()
                .find(|token| !matches!(token.token, Token::Whitespace(_)))
                .map(|token| base_line + token.span.start.line as usize - 1)
        })
        .unwrap_or(base_line)
}

fn location_offset(source: &str, line: u64, column: u64) -> usize {
    let mut current_line = 1;
    let mut current_column = 1;
    for (offset, ch) in source.char_indices() {
        if current_line == line && current_column == column {
            return offset;
        }
        if ch == '\n' {
            current_line += 1;
            current_column = 1;
        } else {
            current_column += 1;
        }
    }
    source.len()
}
fn token_start(source: &str, token: &TokenWithSpan) -> usize {
    location_offset(source, token.span.start.line, token.span.start.column)
}
fn token_end(source: &str, token: &TokenWithSpan) -> usize {
    location_offset(source, token.span.end.line, token.span.end.column)
}

/// Split only at SQL statement boundaries, respecting escaped identifiers,
/// string literals, nested block comments and PostgreSQL dollar-quoted bodies.
fn split_statements(sql: &str) -> (Vec<(&str, usize)>, Vec<Warning>) {
    let bytes = sql.as_bytes();
    let mut result = vec![];
    let mut warnings = vec![];
    let mut start = 0;
    let mut start_line = 1;
    let mut line = 1;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\n' {
            line += 1;
            i += 1;
            continue;
        }
        if bytes[i..].starts_with(b"--") {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if bytes[i..].starts_with(b"/*") {
            let comment_line = line;
            i += 2;
            let mut depth = 1;
            while i < bytes.len() && depth > 0 {
                if bytes[i..].starts_with(b"/*") {
                    depth += 1;
                    i += 2;
                } else if bytes[i..].starts_with(b"*/") {
                    depth -= 1;
                    i += 2;
                } else {
                    if bytes[i] == b'\n' {
                        line += 1;
                    }
                    i += 1;
                }
            }
            if depth > 0 {
                warnings.push(Warning {
                    line: comment_line,
                    message: "Unterminated block comment".into(),
                });
            }
            continue;
        }
        if bytes[i] == b'\'' || bytes[i] == b'"' {
            let quote = bytes[i];
            let quote_line = line;
            let escapes = quote == b'\''
                && i > 0
                && matches!(bytes[i - 1], b'e' | b'E')
                && (i == 1 || !bytes[i - 2].is_ascii_alphanumeric());
            i += 1;
            let mut closed = false;
            while i < bytes.len() {
                if escapes && bytes[i] == b'\\' {
                    if bytes.get(i + 1) == Some(&b'\n') {
                        line += 1;
                    }
                    i = (i + 2).min(bytes.len());
                } else if bytes[i] == quote {
                    i += 1;
                    if i < bytes.len() && bytes[i] == quote {
                        i += 1;
                    } else {
                        closed = true;
                        break;
                    }
                } else {
                    if bytes[i] == b'\n' {
                        line += 1;
                    }
                    i += 1;
                }
            }
            if !closed {
                warnings.push(Warning {
                    line: quote_line,
                    message: "Unterminated quoted SQL value or identifier".into(),
                });
            }
            continue;
        }
        if bytes[i] == b'$' {
            let tag_end = sql[i + 1..].find('$').map(|relative| i + 1 + relative);
            if let Some(end) = tag_end.filter(|end| {
                let tag = &sql[i + 1..*end];
                tag.is_empty()
                    || (tag
                        .chars()
                        .next()
                        .is_some_and(|ch| ch.is_alphabetic() || ch == '_')
                        && tag.chars().all(|ch| ch.is_alphanumeric() || ch == '_'))
            }) {
                let delimiter = &sql[i..=end];
                let body_start = end + 1;
                if let Some(close) = sql[body_start..].find(delimiter) {
                    let next = body_start + close + delimiter.len();
                    line += bytes[i..next].iter().filter(|byte| **byte == b'\n').count();
                    i = next;
                } else {
                    warnings.push(Warning {
                        line,
                        message: "Unterminated dollar-quoted SQL body".into(),
                    });
                    i = bytes.len();
                }
                continue;
            }
        }
        if bytes[i] == b';' {
            result.push((&sql[start..=i], start_line));
            start = i + 1;
            start_line = line;
        }
        i += 1;
    }
    if start < sql.len() {
        result.push((&sql[start..], start_line));
    }
    (result, warnings)
}
