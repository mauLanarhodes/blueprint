//! Structured ERD table data. Columns have stable ids and fractional order
//! keys so edits never invalidate relationships by changing array positions.

use crate::{ColumnId, OrderKey};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SqlDialect {
    #[default]
    PostgreSql,
    MySql,
    SqlServer,
    Sqlite,
    Oracle,
}

impl SqlDialect {
    pub const ALL: [Self; 5] = [
        Self::PostgreSql,
        Self::MySql,
        Self::SqlServer,
        Self::Sqlite,
        Self::Oracle,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::PostgreSql => "PostgreSQL",
            Self::MySql => "MySQL",
            Self::SqlServer => "SQL Server",
            Self::Sqlite => "SQLite",
            Self::Oracle => "Oracle",
        }
    }

    /// Common suggestions; arbitrary type names remain supported.
    pub fn data_types(self) -> &'static [&'static str] {
        match self {
            Self::PostgreSql => &[
                "BIGINT",
                "INTEGER",
                "TEXT",
                "VARCHAR(255)",
                "BOOLEAN",
                "NUMERIC",
                "DATE",
                "TIMESTAMP",
                "UUID",
                "JSONB",
            ],
            Self::MySql => &[
                "BIGINT",
                "INT",
                "TEXT",
                "VARCHAR(255)",
                "BOOLEAN",
                "DECIMAL",
                "DATE",
                "DATETIME",
                "JSON",
            ],
            Self::SqlServer => &[
                "BIGINT",
                "INT",
                "NVARCHAR(255)",
                "BIT",
                "DECIMAL",
                "DATE",
                "DATETIME2",
                "UNIQUEIDENTIFIER",
            ],
            Self::Sqlite => &["INTEGER", "TEXT", "REAL", "BLOB", "NUMERIC"],
            Self::Oracle => &[
                "NUMBER",
                "VARCHAR2(255)",
                "CLOB",
                "DATE",
                "TIMESTAMP",
                "RAW(16)",
            ],
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TableDisplay {
    #[default]
    All,
    KeysOnly,
    Collapsed,
}

impl TableDisplay {
    pub const ALL: [Self; 3] = [Self::All, Self::KeysOnly, Self::Collapsed];

    pub fn label(self) -> &'static str {
        match self {
            Self::All => "All columns",
            Self::KeysOnly => "Keys only",
            Self::Collapsed => "Collapsed",
        }
    }
}

fn yes() -> bool {
    true
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ErdColumn {
    pub id: ColumnId,
    pub order: OrderKey,
    pub name: String,
    pub data_type: String,
    #[serde(default)]
    pub primary_key: bool,
    #[serde(default)]
    pub foreign_key: bool,
    #[serde(default)]
    pub unique: bool,
    #[serde(default = "yes")]
    pub nullable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_value: Option<String>,
}

impl ErdColumn {
    pub fn new(name: impl Into<String>, data_type: impl Into<String>, order: OrderKey) -> Self {
        Self {
            id: ColumnId::new(),
            order,
            name: name.into(),
            data_type: data_type.into(),
            primary_key: false,
            foreign_key: false,
            unique: false,
            nullable: true,
            default_value: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ErdTable {
    #[serde(default)]
    pub dialect: SqlDialect,
    #[serde(default)]
    pub display: TableDisplay,
    #[serde(default)]
    pub columns: Vec<ErdColumn>,
}

impl Default for ErdTable {
    fn default() -> Self {
        let mut id = ErdColumn::new("id", "BIGINT", OrderKey::first());
        id.primary_key = true;
        id.nullable = false;
        Self {
            dialect: SqlDialect::default(),
            display: TableDisplay::default(),
            columns: vec![id],
        }
    }
}

impl ErdTable {
    pub fn column(&self, id: ColumnId) -> Option<&ErdColumn> {
        self.columns.iter().find(|column| column.id == id)
    }

    /// Primary keys first, with each section ordered by the stable row key.
    pub fn columns_sorted(&self) -> Vec<&ErdColumn> {
        let mut columns: Vec<_> = self.columns.iter().collect();
        columns.sort_by(|a, b| {
            (!a.primary_key, &a.order, a.id).cmp(&(!b.primary_key, &b.order, b.id))
        });
        columns
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_default_and_sorting_are_stable() {
        let mut table = ErdTable::default();
        let pk = table.columns[0].id;
        assert!(!table.columns[0].nullable);
        table.columns.push(ErdColumn::new(
            "name",
            "TEXT",
            OrderKey::before(&table.columns[0].order),
        ));
        let sorted = table.columns_sorted();
        assert_eq!(sorted[0].id, pk);
        assert_eq!(sorted[1].name, "name");
        assert_eq!(
            serde_json::from_str::<ErdTable>(&serde_json::to_string(&table).unwrap()).unwrap(),
            table
        );
    }
}
