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
pub struct ErdKey {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub columns: Vec<ColumnId>,
}

/// A column reference in a stored SQL expression. Byte offsets delimit the
/// original identifier; rendering substitutes the current column name.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ErdIndexReference {
    pub column: ColumnId,
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ErdIndexExpression {
    pub sql: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub references: Vec<ErdIndexReference>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ErdIndex {
    pub name: String,
    pub unique: bool,
    pub columns: Vec<ErdIndexExpression>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub predicate: Option<ErdIndexExpression>,
}

/// SQL ownership is independent of visual end swapping. Composite mappings
/// refer to stable column ids in the owner and referenced tables.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ErdForeignKey {
    /// Visual end swapping does not change which table owns the SQL key.
    #[serde(default, skip_serializing_if = "is_false")]
    pub owner_at_target: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub columns: Vec<ColumnId>,
    pub referenced_columns: Vec<ColumnId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_delete: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_update: Option<String>,
}

fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ErdTable {
    #[serde(default)]
    pub dialect: SqlDialect,
    #[serde(default)]
    pub display: TableDisplay,
    #[serde(default)]
    pub columns: Vec<ErdColumn>,
    /// Schema qualifiers, separate from the editable table title.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub schema: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary_key: Option<ErdKey>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unique_keys: Vec<ErdKey>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub indexes: Vec<ErdIndex>,
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
            schema: Vec::new(),
            primary_key: None,
            unique_keys: Vec::new(),
            indexes: Vec::new(),
        }
    }
}

impl ErdTable {
    /// Reflect table constraints in the row badges. Older diagrams store
    /// only badges, so absence of key metadata keeps that representation.
    pub fn sync_key_flags(&mut self) {
        if let Some(key) = &self.primary_key {
            for column in &mut self.columns {
                column.primary_key = key.columns.contains(&column.id);
            }
        }
        for column in &mut self.columns {
            if self
                .unique_keys
                .iter()
                .any(|key| key.columns == [column.id])
            {
                column.unique = true;
            }
        }
    }

    /// Drop constraints and indexes that require a removed column; silently
    /// shortening a composite constraint would change its meaning.
    pub fn remove_column_metadata(&mut self, id: ColumnId) {
        if self
            .primary_key
            .as_ref()
            .is_some_and(|key| key.columns.contains(&id))
        {
            self.primary_key = None;
            for column in &mut self.columns {
                column.primary_key = false;
            }
        }
        self.unique_keys.retain(|key| !key.columns.contains(&id));
        self.indexes.retain(|index| {
            !index
                .columns
                .iter()
                .chain(index.predicate.iter())
                .any(|expression| {
                    expression
                        .references
                        .iter()
                        .any(|reference| reference.column == id)
                })
        });
    }

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
