# Blueprint

A native desktop diagram editor for ERDs, AWS/Azure architecture diagrams and
flowcharts, written in Rust. Free and open source (MIT OR Apache-2.0).

This is **Phase 0 (Foundations)**: draw basic shapes and text on a pan/zoom
canvas, undo/redo, save and open `.blueprint` files, and export SVG.

## Run it

```sh
cargo run -p bp-app --release                 # the desktop app
cargo run -p bp-app --release -- plan.blueprint
cargo run -p bp-cli -- export plan.blueprint plan.svg
cargo test --workspace
```

Linux needs a few system libraries for windowing (Debian/Ubuntu names):

```sh
sudo apt-get install libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev libxkbcommon-dev
```

The app renders with wgpu and falls back to OpenGL if wgpu cannot start.
Set `BLUEPRINT_RENDERER=glow` or `=wgpu` to force one.

## Workspace

| Crate | Owns |
| --- | --- |
| `bp-model` | Document, pages, layers, elements, styles, ids, fractional order keys |
| `bp-geom` | Shape outlines and hit-testing |
| `bp-commands` | Undoable commands and the undo/redo history |
| `bp-io` | `.blueprint` (zip) and `.blueprint.json` files, atomic saves, schema migration |
| `bp-scene` | Document → render-agnostic display list |
| `bp-export` | Display list → SVG |
| `bp-render-egui` | Display list → egui painter, pan/zoom viewport |
| `bp-app` | The desktop app (`blueprint` binary) |
| `bp-cli` | Headless export (`blueprint-cli` binary) |

Only `bp-app` and `bp-render-egui` depend on egui.

## File format

A `.blueprint` file is a zip holding `document.json`. Save as
`name.blueprint.json` to get plain JSON for readable Git diffs. Every file
carries a `schema_version`; `bp-io` migrates older files on open.