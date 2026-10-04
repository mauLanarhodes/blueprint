# Phase 2 — ERDs

Phase 2 starts with a usable smart-table and Crow's Foot milestone. The
project plan also calls for Chen, UML and IDEF1X notation and more direct
table editing; those remain later gates. Starting this milestone does not
close the remaining [Phase 1 editor work](phase-1.md#left-for-phase-1).

## First milestone: smart tables and Crow's Foot

- [x] Choose ERD or Flowchart for each new page and preserve that type in
      native files. Palettes, search, recent shapes and tools show the
      selected notation plus basic shapes. Existing populated pages are
      inferred without changing their content.
- [x] Offer all five Crow's Foot connection presets beneath ERD shapes,
      with vector symbols and explanatory hover text. C activates the
      current connector and Shift+C cycles presets; the floating toolbar
      displays and selects the current type. Explicit tools honor the
      chosen target marker; direct Select-tool row drags retain FK inference.
- [x] Insert a smart table from the ERD palette, edit its name and typed
      columns, and show primary keys above a divider.
- [x] Edit PK, FK, UK, nullability and default values. Add, remove and
      reorder columns with undo/redo; column ids and attached endpoints
      survive renaming and reordering.
- [x] Select PostgreSQL, MySQL, SQL Server, SQLite or Oracle as the table's
      dialect, with type suggestions in the inspector.
- [x] Draw relationships between stable left/right column ports, with
      exactly one, zero or one, one or many, zero or many, or many markers.
      Solid and dashed relationship styles use the existing stroke controls.
- [x] Connect a column to another table's primary key to mark the column
      as FK and set the relationship cardinalities in the same undo step.
      Self-referencing columns are supported; a PK/FK creates a solid
      identifying relationship. Unique referencing columns default to
      zero or one instead of zero or many.
- [x] Collapse tables or show keys only. Hidden row ports move to the
      header while retaining their ids and relationships.
- [x] Store ERD data in schema 3, migrate earlier documents, and preserve
      existing flowcharts and their styles.
- [x] Save and reopen an ERD identically in both native formats, then export
      it through the real CLI with the same text and cardinality geometry
      as the canvas scene (`crates/bp-cli/tests/erd.rs`).

The inspector supplies the initial column-editing workflow. Double-click
edits the table header; the column controls edit the rows. Removing a
column removes its attached relationships in the same undo step. Undo
restores the column and the relationships together.

Open the editable [orders example](../examples/orders.blueprint.json) with
`cargo run -p bp-app --release -- examples/orders.blueprint.json`. Regenerate
it with `cargo run -p bp-app --example erd -- examples/orders.blueprint.json`.

PostgreSQL SQL import and export now build on this milestone. Preview a file
or pasted script with line-specific warnings, import editable tables and
relationships in one undo step, and preview DDL before saving a separate
SQL file. See [SQL schema interchange](erd-sql.md) for dialect coverage,
supported definitions and CLI examples. Type suggestions for other dialects
do not imply SQL interchange support.

## Remaining Phase 2 gates

- [ ] Edit column cells directly on the canvas, with Enter adding a row,
      Tab moving between name and type, and dialect-specific completion.
- [ ] Reorder column rows by dragging on the canvas.
- [ ] Complete the remaining Crow's Foot shape set: view, enum type and
      schema container behavior.
- [ ] Add Chen entities, weak entities, relationships and attributes,
      including identifying relationships and participation notation.
- [ ] Add UML class compartments, association classes and multiplicities.
- [ ] Add IDEF1X entity/category shapes and relationship notation.
- [ ] Exercise each notation's complete mouse and keyboard workflow,
      native round trip and CLI export before marking Phase 2 complete.

Additional SQL interchange dialects and live-database reverse engineering
remain future work. SQL scripts are never executed by the importer.

## Validation

The ERD CLI gate covers saved column data and stable endpoints, typed
column text and badges in SVG, and all five Crow's Foot markers. It also
checks that cached routes follow reordered rows, collapse to the header,
and return after undo. Model, command, scene and editor tests cover their
respective behavior. Run the workspace checks with:

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
```
