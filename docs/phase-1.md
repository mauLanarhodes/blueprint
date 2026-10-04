# Phase 1 — Editor core and flowcharts

Phase 1 turns the Phase 0 sketchpad into a usable flowchart editor: glued,
routed connectors, snapping, multi-select, groups, pages and layers, a
declarative shape library, text that wraps the same on screen and in exports,
and caches that keep large diagrams fast. It is the base every later diagram
family (ERD, cloud) builds on.

## Gate

Phase 1 is done when all of these hold:

- [x] A two-page flowchart with glued, orthogonally routed connectors, labels,
      a group and a hidden layer saves, reopens identically and exports SVG
      through the CLI that matches the screen line for line
      (`crates/bp-cli/tests/gate.rs`).
- [x] Every interaction above works with the mouse in the real app
      (`crates/bp-app/tests/ui.rs`, run headlessly in CI).
- [x] Phase 0 files open and migrate (`crates/bp-io/tests/migration.rs`).
- [x] Performance on a 5,000-element page (shapes with text, ~2,500 routed
      connectors), release build, checked in CI
      (`crates/bp-scene/tests/bench.rs`):

      | Scenario | Budget | Measured (desktop, loaded) |
      | --- | --- | --- |
      | Build the whole page | under 1 s (with parsing) | 60–110 ms |
      | Rebuild after a change, nothing to redo | under 16 ms | 2–4 ms |
      | Drag a shape with 50+ connectors (one frame) | under 16 ms | 3–8 ms |
      | Hit test | | ~2 µs each |

- [ ] Remaining Phase 1 items below are finished or explicitly moved.

## Done

**Model (schema 2).** Elements are shapes, connectors or groups, nested by
parent pointers with fractional order keys. Shapes reference library shapes
(`flowchart/decision`). Connector ends are free points or glued to a shape,
either at a named port or floating on the outline. Styles are sparse
overrides, so files store only what the user changed. Validation catches
cycles, dangling parents and connectors glued to missing shapes.

**Commands.** Every edit is a property-level command that returns its
inverse. Commands refuse to break references, so undo can never strand a
connector. Higher-level edits (delete with attached connectors, move,
scale, group/ungroup, z-order, align/distribute, copy/paste, delete page)
are built on them, and a property test checks that any edit sequence undoes
and redoes exactly.

**Shapes.** Shape definitions live in TOML. Outlines are SVG-like paths in
box fractions with optional fixed offsets (`1-12`). The built-in libraries
are the plan's full flowchart set (26 symbols) and a basic kit (19 shapes,
including callouts, block arrows and sticky notes). Ports default to where
the outline meets the centre lines.

**Connectors.** Straight, orthogonal and curved routing. The orthogonal
router runs A* over a sparse grid with bend penalties and prefers midlines,
so Z-routes centre between shapes; floating ends pick the best sides.
Markers: arrow, open arrow, triangle, diamond, open diamond, circle, open
circle. Labels sit at any point along the route and can be dragged. Line
segments and waypoints can be dragged, and ends can be re-glued by dragging
them.

**Editor.** Palette with thumbnails, fuzzy search and recents; drag or click
to insert; `/` quick insert at the cursor. Multi-select, marquee, eight-handle
resize (Shift keeps proportions, Alt resizes from the centre), smart guides
and grid snapping, connect arrows on the selected shape, and hover ports.
Groups select together; double-click enters a group. In-place text editing,
and text boxes grow to fit. Copy/cut/paste through the system clipboard,
duplicate, lock, nudge. Page tabs (add, rename, duplicate, reorder, delete)
and a layers panel (show/hide, lock, rename, reorder, move selection to a
layer). The inspector edits geometry, style and text for one element or the
whole selection, with reset-to-default per property.

**Text.** Bundled Inter (four faces). Widths come from harfrust shaping,
the same shaper egui uses. Line breaking happens in the core at spaces and
hyphens, and long words overflow rather than split. SVG export uses the
same line breaks and can embed the fonts.

**Rendering and performance.** egui painter with lyon tessellation for
concave fills. Dashes are cut in page units, so screen and SVG match.
Per-element scene cache keyed by element data, with connectors keyed by
their shapes' versions. An R-tree is updated in place as elements change.
Fast id hashing.

## Left for Phase 1

- **Obstacle-avoiding routes.** The router avoids the two shapes a connector
  joins, but not other shapes in the way. The router already accepts
  extra obstacles; what is missing is cache invalidation when an unrelated
  shape moves into a route's path (see the plan's libavoid note).
- **Line jumps** where connectors cross.
- **Equal-spacing hints** while dragging.
- **Rotation** (the plan's rotation handle), including rotated hit tests
  and ports.
- **Containers that auto-grow** around their children, plus **swimlanes**
  (horizontal and vertical pools with lanes) for the flowchart library.
- **Named styles** (themes) on top of the per-element overrides.
- **Autosave** every 30 s, **crash recovery** and **recent files**.
- **Dockable panels** (`egui_dock`) and a **dark theme**.
- **Insert image** and **frame** tools.
- Cache the tessellation of concave shapes (today they are tessellated
  each frame; fine for normal diagrams, wasteful for hundreds of clouds).

## Decisions worth revisiting

- Ports on library shapes are named per definition (`n`, `e`, `s`, `w`, or
  declared names) rather than carrying UUIDs; ERD column ports in Phase 2
  can use column ids. Definitions must never drop a port name.
- Deleting a shape deletes the connectors glued to it (as draw.io and
  Visio do by default).
- Locked elements can be selected (to unlock them) but not moved,
  resized, edited or deleted. Locked layers ignore clicks altogether.
- Clipboard data is JSON under a `blueprint-clip` key. Pasting plain
  text creates a text box.
