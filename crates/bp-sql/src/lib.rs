//! SQL schema transport, with dialect-specific import and export implementations.
//!
//! Only implemented dialects are exposed by [`SUPPORTED_DIALECTS`]. Import never
//! executes SQL and reports unsupported definitions in the preview.

mod diagram;
mod export;
mod postgres;

pub use diagram::{bind_index_expression, current_index_expression, import_commands};
pub use export::{ExportPreview, export_page, export_schema};

pub use bp_model::SqlDialect;
use serde::{Deserialize, Serialize};

pub const SUPPORTED_DIALECTS: &[SqlDialect] = &[SqlDialect::PostgreSql];

/// Identifier components, decoded from SQL quoting and folded when unquoted.
pub type QualifiedName = Vec<String>;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Schema {
    pub tables: Vec<Table>,
    pub foreign_keys: Vec<ForeignKey>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Table {
    pub name: QualifiedName,
    pub columns: Vec<Column>,
    pub primary_key: Option<Key>,
    pub unique_keys: Vec<Key>,
    pub indexes: Vec<Index>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Column {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
    pub default_value: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Key {
    pub name: Option<String>,
    pub columns: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Index {
    pub name: String,
    pub unique: bool,
    /// SQL index expressions, including quoting, ordering, and operator classes.
    pub columns: Vec<String>,
    pub method: Option<String>,
    pub predicate: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForeignKey {
    pub name: Option<String>,
    pub table: QualifiedName,
    pub columns: Vec<String>,
    pub referenced_table: QualifiedName,
    pub referenced_columns: Vec<String>,
    pub on_delete: Option<String>,
    pub on_update: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Warning {
    /// One-based source line; export warnings use zero when there is no source.
    pub line: usize,
    pub message: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportPreview {
    pub schema: Schema,
    pub warnings: Vec<Warning>,
}

pub fn parse(sql: &str, dialect: SqlDialect) -> Result<ImportPreview, String> {
    match dialect {
        SqlDialect::PostgreSql => Ok(postgres::parse(sql)),
        _ => Err(format!("{} SQL import is not implemented", dialect.label())),
    }
}
