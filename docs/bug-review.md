# Code review and bug fixes

Reviewed October 3, 2026 against the supplied Blueprint project plan and
the repository's [Phase 1 status](phase-1.md).

The workspace follows the plan's core architecture: a serializable document
model, property commands with undo/redo, reusable geometry and text layout,
a shared scene for screen rendering and SVG export, native project files,
and a headless CLI. The baseline test suite passed; the fixes below cover
cases that the existing tests did not exercise.

## Fixed

| Area | Confirmed problem | Result |
| --- | --- | --- |
| Unsaved edits | Active canvas text and inline page/layer renames were absent from dirty-state detection. A clean document could be replaced or closed without protecting those edits; saves could miss a rename. | Dirty detection includes editor buffers. Save, document replacement and close commit inline edits before checking or writing. Keyboard quit uses the same unsaved-changes handling. |
| Undo state | An open drag did not change the saved-state ID. Discarding old undo entries also made the oldest reachable state incorrectly appear to be the initial state. | Each successful open-step change gets a distinct state ID, and bounded history retains its actual baseline ID. Cancel, commit, undo and redo preserve the correct state identity. |
| Locked objects | Single-element inspector fields directly edited locked shapes/connectors and objects on locked layers. | Property controls respect inherited locks while the arrange controls remain available to unlock an object. |
| Page duplication | Copying each layer separately detached relationships between layers, including hidden layers. | The entire page is remapped together, preserving layers, parent relationships, styles, stacking keys and glued endpoints. Ancestors and shapes are inserted before dependent elements. |
| Paste | A connector painted below its target shapes could be inserted before those shapes, causing an otherwise valid paste to fail. | Insert dependencies are ordered independently from the preserved stacking keys. |
| Project writes | Every save used the same temporary filename, overwriting unrelated temporary data and allowing concurrent saves to interfere. Invalid documents could replace a valid file with one the loader rejected. | Temporary files are created exclusively with distinct names. Documents are validated before serialization or replacing an existing file. |
| CLI export | Default output names lost part of names containing dots. A content-detected project with an SVG extension could overwrite itself. | Output naming preserves the full project stem. Canonical source/output aliases are rejected, and SVG writes use the atomic writer. |
| Connector markers | Marker trimming failed on short terminal segments; strokes could run through markers. | Trimming follows total path length across segments and removes the stroke when the markers cover the entire route. |
| Hit testing | Thick strokes extended beyond the spatial-index bounds, making their outer edges unselectable. | Shape index bounds include half the stroke width. |

## Validation

- `cargo test --workspace`: 152 tests passed, including 17 added regressions,
  the headless mouse/keyboard UI suite, undo/redo property test, save/reopen/CLI
  export gate, schema migrations and the 5,000-element performance smoke test.
- `cargo clippy --workspace --all-targets -- -D warnings`.
- `cargo fmt --all --check`.

Checks were run on Linux. Native Windows/macOS runtime behavior and a manual
desktop session were not exercised in this review.

## Remaining plan work

This review covered the Phase 1 flowchart editor. ERD notation and smart tables,
cloud icon packs, raster/PDF export, autosave/recovery and release packaging
are later work. The Phase 1 document also tracks unrelated-obstacle routing,
line jumps, rotation, containers/swimlanes, named styles and dockable panels.
Those feature gaps remain outside this bug-fix pass.

The toolchain file follows `stable`; the project plan calls for a pinned
toolchain. CI currently checks formatting, linting, tests and performance;
the planned dependency-license/security checks and release pipeline are
still absent.
