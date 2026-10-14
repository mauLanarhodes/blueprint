# Workspace redesign verification

The screenshot example renders the actual egui application with deterministic
workspace defaults. By default it does not load user preferences or catalogs. It never saves preferences, installs
cloud packs, accept provider terms, or invoke Terraform/AWS. Cloud fixture icons
are original synthetic artwork for UI review; they are not vendor icons.

## Render populated workspaces

```sh
cargo run -p bp-app --example screenshot -- /tmp/blueprint-flowchart.png --fixture flowchart --fit
cargo run -p bp-app --example screenshot -- /tmp/blueprint-erd.png --fixture erd connector --fit
cargo run -p bp-app --example screenshot -- /tmp/blueprint-cloud.png --fixture cloud shape --fit --size 1600x1000 --scale 1.25
```

`--size` describes logical points; `--scale` describes physical pixels per point.
For example, `--size 1360x860 --scale 2` produces a 2720×1720 PNG. `--theme light`
checks the alternate workspace theme without changing the document's styling.
`--help` lists the complete CLI.

`--preferences FILE` reads an explicit versioned workspace-preferences JSON file;
`--catalogs DIRECTORY` reads only that directory's installed packs. These opt-in
inputs allow local artwork review without bundling provider assets. `--svg FILE`
exports the same final page for geometry and artwork comparison:

```sh
target/debug/examples/screenshot /tmp/blueprint-erd.png --fixture erd --fit --svg /tmp/blueprint-erd.svg
target/debug/examples/screenshot /tmp/blueprint-aws.png cloud --catalogs /path/to/icon-packs --preferences /tmp/workspace.json
```

The existing positional usage still works:

```sh
cargo run -p bp-app --example screenshot -- /tmp/orders.png examples/orders.blueprint.json shape
cargo run -p bp-app --example screenshot -- /tmp/cloud-packs.png cloud packs
cargo run -p bp-app --example screenshot -- /tmp/network.png cloud network --fit
cargo run -p bp-app --example screenshot -- /tmp/deployment-dialog.png cloud deploy
```

Kinds supplied positionally create empty pages. `--fixture` creates populated
content: a two-page order flow with an annotation layer, a five-table PostgreSQL
schema with four mapped foreign keys and an index, or a documented cloud network.
The order flow's successful decision branch points down to reservation. The cloud
fixture places regional object storage outside the VPC/subnets and depicts RDS
through a DB subnet group referencing private subnets in two availability zones.
Its boundary shapes remain documentation elements.

`shape` selects the fixture's main process/table/service. `connector` selects its
main decision branch/mapped relationship/service connection. `all`, `drag` and
`connect` exercise multiselection and in-progress interactions. `packs`, `network`,
`deploy` and `calendar` retain the existing diagnostic screens. `deploy` only
opens the dialog and never requests a plan or deployment.
`column` selects a stable ERD column; `focus` displays keyboard focus. `--navigator`
opens navigation. `sql-warning`, `long-text`, and `error` provide additional
diagnostics. The long ERD name diagnostic invokes the existing Auto-arrange action;
editing a table name does not automatically move its neighbors.

## Visual review matrix

Build once, then render all three kinds with `none`, `shape` and `connector` at
1600×1000, 1360×860, 1024×720 and the minimum 760×480 logical size. Review at scale
1, 1.25 and 2; the following renders the complete matrix into `/tmp`:

```sh
cargo build -p bp-app --example screenshot
for kind in flowchart erd cloud; do
  for mode in none shape connector; do
    for size in 1600x1000 1360x860 1024x720 760x480; do
      for scale in 1 1.25 2; do
        target/debug/examples/screenshot "/tmp/blueprint-review/${kind}-${mode}-${size}-${scale}.png" \
          --fixture "$kind" --mode "$mode" --fit --size "$size" --scale "$scale"
      done
    done
  done
done
```

Inspect readable labels, alignment, selection handles, connector labels and
mapped-column highlights, inspector overflow, focus indicators, and reachable
panel controls. A fitted entire diagram naturally becomes smaller at the minimum
window size; also inspect ordinary zoom and pan there. Check long labels, searches
with no results, SQL warnings and errors through interaction tests or additional
screens. PNG generation and compilation alone do not establish visual acceptance.

The headless WGPU harness does not exercise native file pickers, native window
chrome, real system clipboard integration, monitor-scale transitions, or the
OpenGL fallback. Smoke-check these with the native app on supported platforms.

## Behavioral and performance checks

```sh
cargo test -p bp-app --test ui
cargo test -p bp-app --lib
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test --release -p bp-scene --test bench -- --nocapture
cargo run --release -p bp-app --example workspace_bench -- 120
```

Keep the existing command, IO, SQL, export and UI regression checks. Add focused
tests for panel collapse/resizing and preference round trips, keyboard focus and
shortcut ownership, and canvas/inspector/navigation synchronization. Workspace
changes must not enter document history or change document dirty state.

The existing release performance gate covers 5,000 scene elements, not complete
UI-frame cost. Review a large document and a full cloud catalog for smooth
pan/zoom, selection and catalog scrolling after shell or navigation changes.
The `workspace_bench` example reports median/p95 complete app/egui CPU frames and
CPU totals including tessellation for a 5,000-element scene and a synthetic
5,000-icon catalog. It includes navigation, actual wheel/pointer input, filtering,
and 1,000 favorites. It excludes startup, GPU work and native presentation, and
sets no additional CI thresholds.

## Verification record — 2026-10-13

- Formatting, Clippy with warnings denied, and all 354 workspace tests passed,
  including 34 app unit tests, 59 UI tests, the CLI save/reopen/export gate, and
  mock deployment checks. No Terraform/AWS operations were invoked.
- The release 5,000-element scene budget passed: 49.3 ms cold build, 2.15 ms warm
  rebuild, 3.42 ms drag frame, and 1.33 ms for 1,000 hit tests on this machine.
- The separate 120-frame CPU profile measured median/p95 totals of 1.15/1.91 ms
  for the warm scene/Navigator, 1.16/1.52 ms for pan, 1.46/2.06 ms for selection,
  17.51/26.77 ms with all 5,000 elements fitted onscreen, 1.58/2.54 ms for a warm
  5,000-icon catalog, 2.23/3.13 ms with 1,000 favorites, and 6.62/16.70 ms for
  changing searches. Fitting every element is a heavier tessellation case; these
  measurements are machine-specific and exclude GPU presentation.
- Rendered 108 populated core views across four logical sizes and three display
  scales, plus 31 diagnostics/Light/catalog views. Reviewed compact layouts,
  selected states, focus, long text, SQL warnings, original AWS/Azure catalog
  artwork, and dialogs; corrected field sizing, fitting and fixture semantics.
- Rendered standalone SVG previews with bundled Inter. All three fixtures produced
  byte-identical SVG across Dark/Light workspace themes. Selection emphasis and
  workspace chrome are excluded from export.
- Native Linux startup and owned-window captures passed for both WGPU and Glow,
  using isolated settings. Native pickers, real OS clipboard, monitor changes,
  and Windows/macOS behavior still require platform verification.

Local screenshots and SVGs from this run are in `/tmp/blueprint-review`; the
matrix manifest is `render-results.json`. Check logs are `/tmp/blueprint-*-tests.log`,
`/tmp/blueprint-clippy.log`, `/tmp/blueprint-release-bench.log`, and
`/tmp/blueprint-workspace-bench.log`. These are review artifacts, not repository
assets or a substitute for user visual approval.
