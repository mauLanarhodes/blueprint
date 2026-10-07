# SQL schema import and export

SQL interchange currently supports **PostgreSQL**. SQLite, MySQL, SQL Server
and Oracle remain table type-suggestion choices; they are not offered as SQL
import or export dialects. The `bp-sql` crate separates the SQL schema
transport, dialect parser, diagram conversion and DDL generator so each
additional dialect can be implemented and tested separately.

## Desktop workflow

Choose an ERD page, then **File → Open SQL script…** to read a script, or
**File → Paste SQL schema…** to paste into the SQL editor. Generate a preview
and review the table, column and foreign-key counts and warnings with source
line numbers. Changing the input invalidates the preview. Apply adds tables
and Crow's Foot relationships to the current editable layer and lays out the
new tables. Cancel leaves the document unchanged. Each import is a single
undo operation, including every table and relationship; redo restores the
same element and column IDs.

Try the [orders SQL example](../examples/orders.sql) for schema-qualified
tables, composite keys, a late foreign key and a partial ordered index.
The [retail SQL example](../examples/retail.sql) provides a larger diagram
with 30 tables and 44 foreign keys, including a self-reference and a composite
reference.

Imports arrange tables by their relationships rather than script order.
Referenced tables appear to the left of dependent tables, cycles stay
together, and disconnected groups are placed separately. All columns remain
visible. Import into an existing page places the new diagram beside existing
tables without moving them.

Use **Arrange → Auto-arrange ERD** to reorganize the visible editable tables
of an existing ERD page. Locked tables stay fixed and hidden layers stay
hidden. The command resets affected editable connection routes and fits the
complete diagram; undo restores the previous positions and routes together.
Orthogonal connections avoid visible shapes and use separate corridors where
possible. Layout uses the same measured table sizes as the canvas and SVG
export, so large defaults and long column names do not overlap nearby tables.

Select a table to emphasize its direct relationships and neighboring tables.
Unrelated connections fade while table text stays readable. Selecting a
relationship highlights every mapped foreign-key column, including composite
keys, and the inspector shows its mapping. These selection effects are only
editor overlays; SVG exports retain the ordinary diagram styling.

The source script is read without modification. Import does not change the
project's save path. Table names, columns, types, flags, nullability and
defaults use the existing ERD editor. The table inspector also edits schema
names, ordered composite keys and index expressions; the relationship
inspector edits foreign-key mappings, constraint names and referential
actions. Composite keys and foreign keys retain
their ordered column mappings instead of becoming unrelated single-column
constraints. Schema qualifiers, named keys and indexes are stored in native
documents along with those stable column IDs. Format 5 upgrades older
documents using default values and prevents older readers from resaving
files without their SQL constraint metadata.

**File → Export page as SQL…** shows a dialect selector, generated DDL and
warnings. Save writes a separate `.sql` file. The current project and imported
source files are protected, including path aliases. Native Blueprint files
are never SQL output destinations, even if a project was renamed to `.sql`.

## PostgreSQL coverage and warnings

The importer recognizes `CREATE TABLE` column definitions, type names and
parameters, nullability, defaults, primary and unique keys, foreign keys and
`CREATE INDEX`. It handles double-quoted identifiers with escaped quotes,
unquoted PostgreSQL name folding, qualified names, comments and multi-statement
scripts. `ALTER TABLE … ADD CONSTRAINT` supports keys and foreign keys declared
after the tables. References resolve after the script has been parsed, so
forward references, self references and cycles can become relationships.

Unsupported statements and options produce warnings. Parse errors are local
to a statement where recovery is possible, so other supported definitions
can still import. Missing tables or columns in a relationship produce a
warning rather than a dangling diagram connection. Review warnings before
applying: the imported diagram represents the supported schema definitions,
not every behavior in the original database.

Export quotes identifiers and preserves supported column and constraint
semantics. It emits schemas and tables before foreign keys and indexes;
deferred foreign-key creation also handles cycles. Non-schema diagram
features, ambiguous relationships and incompatible dialect features produce
warnings. Layout, colors and Crow's Foot drawing choices are not SQL storage
properties.

This is schema interchange. It does not execute SQL, connect to a database,
or convert arbitrary queries, procedures, triggers or dynamic SQL into an
ERD. Database objects such as views, enum definitions, sequences and routines
outside the supported table schema are reported for manual handling.
Type names and default expressions are preserved as SQL text. Custom types,
extension functions and explicit `nextval(...)` defaults can require database
objects that are absent from the diagram. `SERIAL` types retain PostgreSQL's
implicit sequence creation; explicit identity and computed-column options
currently produce warnings.

## CLI

```sh
blueprint-cli import-sql schema.sql --dialect postgres --preview
blueprint-cli import-sql schema.sql diagram.blueprint
blueprint-cli export-sql diagram.blueprint --page 1 --preview
blueprint-cli export-sql diagram.blueprint schema-export.sql --dialect postgres
```

`--preview` does not write files. Import prints counts and line-specific
warnings and requires a new output project path. Export prints warnings and
accepts the same page number or name selection as SVG export. Without an
output argument it derives a `.sql` name from the input project. CLI exports
require a new output path so they cannot replace an original SQL script. Only
`postgres`/`postgresql` is accepted for `--dialect`.

## Validation

Parser tests cover quoted names, comments, defaults, composite keys, late
foreign keys and recovery from unsupported statements. Diagram tests cover
native persistence, editing, import undo/redo and semantic SQL round trips.
CLI tests exercise the actual binary, preview mode and source preservation.

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```
