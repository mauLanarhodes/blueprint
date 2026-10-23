# Screenshot gallery

These PNGs show Blueprint's actual egui workspace, captured on `ui-changes`
and subsequently merged into `main`. They are rendered application screens.

| File | Content |
| --- | --- |
| [erd.png](erd.png) | Five-table shop schema, with the orders table selected. |
| [flowchart.png](flowchart.png) | Order-processing workflow, with inventory reservation selected. |
| [cloud.png](cloud.png) | Documented cloud network, with the container service selected. |

All three images are 1360 × 860 pixels, rendered at a 1× display scale in Dark
theme on October 14, 2026. They were copied unchanged from the local
`/tmp/blueprint-floating-review` captures produced while implementing the
floating toolbar. That implementation was subsequently committed as
`bd32764` (`float bar`). The screenshots use populated, deterministic fixtures.
The cloud artwork is original synthetic test artwork; it is not official
AWS/Azure artwork, and the diagram does not represent a deployed network.

## Regenerate

Run from the repository root on `main` (or a descendant containing its
screenshot example):

```sh
cargo run -p bp-app --example screenshot -- docs/screenshots/erd.png --fixture erd --mode shape --fit --size 1360x860 --scale 1 --theme dark
cargo run -p bp-app --example screenshot -- docs/screenshots/flowchart.png --fixture flowchart --mode shape --fit --size 1360x860 --scale 1 --theme dark
cargo run -p bp-app --example screenshot -- docs/screenshots/cloud.png --fixture cloud --mode shape --fit --size 1360x860 --scale 1 --theme dark
```

The harness renders the real application UI headlessly. It uses isolated
workspace preferences and catalogs by default, never invokes Terraform/AWS,
and does not install official artwork. Update the three files and these notes
when the captured UI changes.

See the
[workspace verification guide](../workspace-redesign.md)
for selection modes, other window sizes, display scales, Light-theme checks,
and corresponding SVG output.

## Interactive network containers

The following are actual headless egui application renders from
`feat/aws-network-containers`, captured October 23, 2026 at 1× display scale.
They use configured VPC/subnet resource elements and the same original synthetic
service artwork as the cloud fixture. No Terraform or AWS operation is involved
in generating these images.

| File | Content |
| --- | --- |
| [network-containers.png](network-containers.png) | Dark canvas: VPC, two private subnet containers, three contained services and an external regional service. |
| [network-containers-light.png](network-containers-light.png) | The same network with a light workspace and white canvas. |
| [network-drop.png](network-drop.png) | Actual pointer drag: receiving subnet border, dashed placement footprint including its label, and placement message. Membership is pending until release. |
| [network-transfer.png](network-transfer.png) | Completed subnet transfer with its service; the inspector shows the new VPC reference, preserved CIDR and availability zone. |

Matching SVG exports sit beside each PNG. Transient selection, receiving-border
and ghost overlays are excluded from SVG. The screenshot harness asserts an
active service gesture for the drop capture and a changed subnet parent for the
transfer capture.

```sh
cargo run -p bp-app --example screenshot -- docs/screenshots/network-containers.png cloud --mode network-services --fit --size 1600x1000 --theme dark --svg docs/screenshots/network-containers.svg
cargo run -p bp-app --example screenshot -- docs/screenshots/network-containers-light.png cloud --mode network-services --fit --size 1600x1000 --theme light --svg docs/screenshots/network-containers-light.svg
cargo run -p bp-app --example screenshot -- docs/screenshots/network-drop.png cloud --mode network-drop --fit --size 1600x1000 --theme dark --svg docs/screenshots/network-drop.svg
cargo run -p bp-app --example screenshot -- docs/screenshots/network-transfer.png cloud --mode network-transfer --fit --size 2000x1000 --theme dark --svg docs/screenshots/network-transfer.svg
```
