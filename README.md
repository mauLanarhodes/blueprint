# Blueprint

A native desktop diagram editor for ERDs, AWS/Azure architecture diagrams and
flowcharts, written in Rust. Free and open source (MIT OR Apache-2.0).

**Status: Phase 3 started — local AWS/Azure icon packs and cloud pages.**
Cloud diagrams can import official SVG icon ZIPs, connect service icons,
and carry their artwork through saving, copy/paste and SVG export.
See [docs/phase-3.md](docs/phase-3.md) for setup and remaining cloud work.
The ERD and editor backlogs remain in [docs/phase-2.md](docs/phase-2.md)
and [docs/phase-1.md](docs/phase-1.md).

## Run it

```sh
cargo run -p bp-app --release                     # the desktop app
cargo run -p bp-app --release -- plan.blueprint
cargo run -p bp-app --release -- examples/orders.blueprint.json # sample ERD
cargo run -p bp-cli -- export plan.blueprint plan.svg [--page 2] [--embed-fonts]
cargo run -p bp-cli -- info plan.blueprint
cargo run -p bp-cli -- import-sql schema.sql --preview
cargo run -p bp-cli -- import-sql schema.sql schema.blueprint
cargo run -p bp-cli -- export-sql schema.blueprint --dialect postgres --preview
cargo run -p bp-cli -- export-sql schema.blueprint schema-export.sql --page 1
cargo test --workspace
cargo test --release -p bp-scene --test bench -- --nocapture   # performance budgets
```

Linux needs a few system libraries for windowing (Debian/Ubuntu names):

```sh
sudo apt-get install libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev libxkbcommon-dev
```

The app renders with wgpu and falls back to OpenGL if wgpu cannot start.
Set `BLUEPRINT_RENDERER=glow` or `=wgpu` to force one.

### Windows (PowerShell)

1. Install [Git for Windows](https://git-scm.com/downloads/win).
2. Install the [Visual Studio C++ Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/).
   In the installer, select **Desktop development with C++** and include the
   MSVC C++ toolset and a Windows 10 or Windows 11 SDK.
3. Install Rust using [rustup](https://rust-lang.org/tools/install/) and accept
   the default MSVC toolchain. This project uses stable Rust and requires
   **Rust 1.95 or newer**.

Open a new PowerShell window after installation so the tools are on `PATH`,
then clone the repository and launch the desktop app:

```powershell
git clone https://github.com/mauLanarhodes/blueprint.git
cd blueprint
rustup update stable
rustc --version
cargo --version
cargo run -p bp-app --release --locked
```

The first build downloads dependencies and can take several minutes. Run all
Cargo commands from the repository root (the folder containing `Cargo.toml`).
To open the bundled sample ERD, close the app and run:

```powershell
cargo run -p bp-app --release --locked -- .\examples\orders.blueprint.json
```

To build once and launch the executable directly:

```powershell
cargo build -p bp-app --release --locked
.\target\release\blueprint.exe
# Or open an existing diagram (quote paths that contain spaces):
.\target\release\blueprint.exe "C:\Users\YourName\Documents\plan.blueprint"
```

If `cargo` or `rustc` is not recognized, reopen PowerShell and check that
`%USERPROFILE%\.cargo\bin` is on your user `PATH`. If the build reports that
`link.exe` is missing, check that the C++ workload and Windows SDK above are
installed.

If the app cannot initialize its graphics renderer, try forcing OpenGL in
PowerShell:

```powershell
$env:BLUEPRINT_RENDERER = "glow"
cargo run -p bp-app --release --locked
# After closing the app, restore automatic renderer selection:
Remove-Item Env:\BLUEPRINT_RENDERER
```

## Using the editor

- Choose **ERD**, **Flowchart** or **Cloud architecture** when starting a document or adding a page.
  Each page remembers its type: ERD shows ERD shapes and basic shapes;
  Flowchart shows flowchart symbols and basic shapes. Search and recent
  shapes follow the current page. Change its type in the page inspector.
- Cloud pages: use **Manage icon packs…**, download the official AWS/Azure
  ZIP, accept that provider's terms, and import it with its release version.
  Click or drag services from the palette, search by name or alias, and use
  `C` for connections. Icons scale uniformly with editable names below them.
  Saved diagrams include their used icons, so no pack is needed to reopen them.
- Shapes: click or drag them from the palette, press `/` on the canvas to
  insert one by name, or use the tools (`R` rectangle, `O` ellipse,
  `D` decision on flowchart pages, `N` sticky note, `T` text).
- Connectors: hover a shape and drag from one of its ports, drag the blue
  arrows beside a selected shape, or use the connector tool (`C`). Drop on a
  port to glue there, or anywhere on a shape to float on its outline.
  Connectors follow their shapes; drag a selected connector's segments,
  waypoints, ends or label to adjust it.
- Double-click to edit text (or to edit inside a group); `Ctrl+G` groups,
  `Ctrl+D` duplicates, `Ctrl+L` locks; hold `Alt` while dragging to turn
  snapping off. The inspector lists the other shortcuts when nothing is
  selected.
- ERDs: insert **Table** from the ERD palette. Select it to edit the table
  name, SQL dialect, display mode and columns in the inspector. Columns
  carry names, types, PK/FK/UK flags, nullability and optional defaults;
  use the row controls to add, remove or reorder them. Primary keys stay
  above the divider. Enter in a row field adds a column; Tab moves between
  fields. Double-click a table to edit its header.
- ERD connection tools appear directly below the ERD shapes: **Exactly
  one**, **Zero or one**, **One or many**, **Zero or many**, and **Many**.
  Hover a symbol for its meaning, then click it and drag a relationship.
  `C` activates the chosen tool; `Shift+C` cycles through the five types.
  The floating toolbar shows the chosen symbol and name, with a menu to
  change it. The chosen cardinality applies to the end you drag toward;
  the starting end is exactly one. Selection-tool row-port drags still
  infer cardinalities from the table columns.
- Each table column has left and right ports. Connect a column to another
  table's primary key to flag the column as FK and create a Crow's Foot
  relationship. Choose either end's cardinality and a solid or dashed line
  in the connector inspector. Keys-only and collapsed tables keep their
  connector attachments when rows are hidden. Self-referencing tables are
  supported. A foreign key that is also a primary key gets a solid
  identifying relationship by default.
- SQL schemas: on an ERD page use **File → Open SQL script…** or
  **Paste SQL schema…**, review the table counts and line-specific warnings,
  then apply. Tables and relationships are ordinary editable elements, added
  in one undo step. **Export page as SQL…** previews PostgreSQL DDL and its
  warnings before saving a separate `.sql` file. Only PostgreSQL is currently
  implemented for SQL import/export; the other table dialects offer type
  suggestions. See [SQL schema interchange](docs/erd-sql.md) for coverage.
- Readable ERDs: imports arrange related tables from left to right with all
  columns visible. Use **Arrange → Auto-arrange ERD** to reorganize an existing
  page in one undo step, then zoom to fit. Orthogonal relationships avoid
  other tables and separate shared routing corridors. Select a table to trace
  its direct relationships, or select a relationship to highlight its mapped
  columns. Try the [dense retail schema](examples/retail.sql).

## Workspace

| Crate | Owns |
| --- | --- |
| `bp-model` | Document, pages, layers, elements (shapes, connectors, groups), ERD columns, sparse styles, ids, fractional order keys |
| `bp-sql` | SQL schema parsing, import previews, editable ERD conversion, relationship-aware layout and dialect-specific DDL generation |
| `bp-geom` | Ray and hit tests, the R-tree index, snapping and smart guides, orthogonal connector routing (A*) |
| `bp-text` | Bundled Inter fonts, measurement (harfrust shaping) and line breaking |
| `bp-shapes` | Shape definitions in TOML and the built-in libraries (basic, flowchart, ERD) |
| `bp-icons` | Official cloud SVG ZIP imports, versioned catalogs and search aliases (`icon-import` binary) |
| `bp-commands` | Undoable property-level commands, undo/redo history, and edits built on them (delete, move, group, align, copy/paste) |
| `bp-io` | `.blueprint` (zip) and `.blueprint.json` files, atomic saves, schema migration |
| `bp-scene` | Document → resolved geometry and a render-agnostic display list, with per-element caching |
| `bp-export` | Display list → SVG |
| `bp-render-egui` | Display list → egui painter (with lyon for concave fills), pan/zoom viewport |
| `bp-app` | The desktop app (`blueprint` binary) |
| `bp-cli` | Headless SVG/SQL export, SQL schema import and file checks (`blueprint-cli` binary) |

Only `bp-app` and `bp-render-egui` depend on egui. The screen, hit-testing
and every export draw from the same scene, so exports match the screen.

## File format

A `.blueprint` file is a zip holding `document.json` and used cloud SVGs in `icons/`. Save as
`name.blueprint.json` to get plain JSON for readable Git diffs. Every file
carries a `schema_version` (currently 5); `bp-io` migrates older files on
open. Files store only what the user set: styles are sparse overrides of the
shape's defaults, and connector routes, text layout and group bounds are
recomputed on load. ERD column ids remain stable when rows are renamed or
reordered, so column-to-column connections survive those edits.

Shape libraries live in [`crates/bp-shapes/libraries`](crates/bp-shapes/libraries);
the format is described at the top of `basic.toml`.

## Third-party assets

- [Inter](https://rsms.me/inter/) 4.1 (SIL Open Font License 1.1), bundled
  in `crates/bp-text/fonts` with its licence.
- [Phosphor](https://phosphoricons.com/) icons via `egui-phosphor` (MIT).
- AWS/Azure architecture artwork is imported locally from official packages
  under each provider's terms; it is not included in this repository.
