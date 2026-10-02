# Blueprint

A native desktop diagram editor for ERDs, AWS/Azure architecture diagrams and
flowcharts, written in Rust. Free and open source (MIT OR Apache-2.0).

**Status: Phase 1 (editor core and flowcharts) in progress.** You can draw
flowcharts with glued, auto-routed connectors, smart guides, groups, pages
and layers, save them, and export SVG. See [docs/phase-1.md](docs/phase-1.md)
for what is done and what is left.

## Run it

```sh
cargo run -p bp-app --release                     # the desktop app
cargo run -p bp-app --release -- plan.blueprint
cargo run -p bp-cli -- export plan.blueprint plan.svg [--page 2] [--embed-fonts]
cargo run -p bp-cli -- info plan.blueprint
cargo test --workspace
cargo test --release -p bp-scene --test bench -- --nocapture   # performance budgets
```

Linux needs a few system libraries for windowing (Debian/Ubuntu names):

```sh
sudo apt-get install libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev libxkbcommon-dev
```

The app renders with wgpu and falls back to OpenGL if wgpu cannot start.
Set `BLUEPRINT_RENDERER=glow` or `=wgpu` to force one.

## Using the editor

- Shapes: click or drag them from the palette, press `/` on the canvas to
  insert one by name, or use the tools (`R` rectangle, `O` ellipse,
  `D` decision, `N` sticky note, `T` text).
- Connectors: hover a shape and drag from one of its ports, drag the blue
  arrows beside a selected shape, or use the connector tool (`C`). Drop on a
  port to glue there, or anywhere on a shape to float on its outline.
  Connectors follow their shapes; drag a selected connector's segments,
  waypoints, ends or label to adjust it.
- Double-click to edit text (or to edit inside a group); `Ctrl+G` groups,
  `Ctrl+D` duplicates, `Ctrl+L` locks; hold `Alt` while dragging to turn
  snapping off. The inspector lists the other shortcuts when nothing is
  selected.

## Workspace

| Crate | Owns |
| --- | --- |
| `bp-model` | Document, pages, layers, elements (shapes, connectors, groups), sparse styles, ids, fractional order keys |
| `bp-geom` | Ray and hit tests, the R-tree index, snapping and smart guides, orthogonal connector routing (A*) |
| `bp-text` | Bundled Inter fonts, measurement (harfrust shaping) and line breaking |
| `bp-shapes` | Shape definitions in TOML and the built-in libraries (basic, flowchart) |
| `bp-commands` | Undoable property-level commands, undo/redo history, and edits built on them (delete, move, group, align, copy/paste) |
| `bp-io` | `.blueprint` (zip) and `.blueprint.json` files, atomic saves, schema migration |
| `bp-scene` | Document → resolved geometry and a render-agnostic display list, with per-element caching |
| `bp-export` | Display list → SVG |
| `bp-render-egui` | Display list → egui painter (with lyon for concave fills), pan/zoom viewport |
| `bp-app` | The desktop app (`blueprint` binary) |
| `bp-cli` | Headless export (`blueprint-cli` binary) |

Only `bp-app` and `bp-render-egui` depend on egui. The screen, hit-testing
and every export draw from the same scene, so exports match the screen.

## File format

A `.blueprint` file is a zip holding `document.json`. Save as
`name.blueprint.json` to get plain JSON for readable Git diffs. Every file
carries a `schema_version` (currently 2); `bp-io` migrates older files on
open. Files store only what the user set: styles are sparse overrides of the
shape's defaults, and connector routes, text layout and group bounds are
recomputed on load.

Shape libraries live in [`crates/bp-shapes/libraries`](crates/bp-shapes/libraries);
the format is described at the top of `basic.toml`.

## Third-party assets

- [Inter](https://rsms.me/inter/) 4.1 (SIL Open Font License 1.1), bundled
  in `crates/bp-text/fonts` with its licence.
- [Phosphor](https://phosphoricons.com/) icons via `egui-phosphor` (MIT).
