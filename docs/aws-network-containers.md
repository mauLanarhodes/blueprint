# AWS network containers

The AWS library's **Add starter network** creates one spacious purple VPC
boundary and two teal private subnet boundaries inside it. Each boundary is the
same resource element used by Infrastructure settings and Terraform. Services
remain directly selectable inside a subnet. Membership appears in the Navigator;
it does not require a membership arrow or entering a group.

Use **Add VPC** for another configured VPC boundary. **Add subnet** uses a selected,
visible, unlocked configured VPC when there is room. With no eligible VPC
selected, it creates an unassigned subnet on the current layer. Enter its CIDR
explicitly before deployment. The action does not allocate an address range or
alter existing network configuration. If the selected VPC is full, resize it
before adding a subnet.

## Editing the diagram

- Click a service to select it and drag it independently. Click a container's
  header or exposed border to select or move the container. Empty content space
  supports marquee selection.
- Drag services or groups over a subnet's content area. The border highlight
  and ghost preview show the receiving container and final placement. Release
  to attach or transfer the complete selection; release on open canvas to
  detach services to their layer. Escape restores the entire gesture.
- Move a subnet to carry its services; move a VPC to carry its subnets and their
  services. Selecting an ancestor and descendant together moves each once.
  Locked elements, locked ancestors and locked descendants prevent edits that
  would move protected contents.
- Resize a selected boundary using its handles or inspector dimensions.
  Width and height are independent. Contents retain their sizes and positions.
  Minimum sizes, header clearance and content padding prevent shrinking across
  contents, and a nested subnet must remain within its VPC content area.

Drops must fit the complete selection inside the content area, clear of the
header. A failed drop restores the original state and reports why it was
rejected. Overlapping targets follow visible stacking order. Each completed
gesture is one undo step, including movement, membership, order and any subnet
VPC reference change. Undo and redo restore the entire result. Authored
connectors remain relationships and stay attached across boundary moves.

## Subnet VPC changes and deployment

Dropping a configured subnet into another configured VPC updates its visual
parent and saved VPC reference together. The Infrastructure inspector's VPC
selector uses the same validation and deterministic placement rules. The target
must be visible, unlocked and on the same page; the subnet CIDR must fit the VPC
and must not overlap another assigned subnet. CIDRs are never rewritten to make
a transfer work. Dropping an assigned subnet on open canvas restores its prior
placement instead of clearing its configured VPC.

Service placement is diagram organization. It does not add Lambda VPC
configuration, IAM permissions, security groups, gateways, routing or endpoints.
S3, DynamoDB, SNS, SQS and Lambda keep their existing generation behavior and do
not acquire subnet dependencies from visual membership.

Moving or resizing boundaries and changing service membership leave generated
Terraform configuration unchanged. Explicit subnet VPC reassignment changes the
subnet dependency, preserves the resource ID and Terraform address, and
invalidates a reviewed plan. The next preview may replace an existing subnet;
review Terraform's replacement/deletion details before applying. Editing the
diagram never calls AWS. See [deployment setup](cloud-deployment.md).

## Existing documents, copies and exports

Older diagrams open with their existing layout. Choose **Convert network to
containers** in the AWS library or deployment dialog to convert configured VPCs
and subnets on the current page. Membership comes from saved subnet VPC
references, including resources that were drawn far apart. Conversion arranges
usable container spacing and preserves IDs, settings, Terraform addresses,
authored connectors and existing service relationships. It is one undo step.
Already converted networks keep their layout when conversion is repeated.
Convert older resource boxes before changing their VPC assignment in the
inspector; their other settings remain editable before conversion. Unassigned
subnet resources remain repairable through their VPC selector after conversion.

File format 9 adds an optional `network_container` shape property, with `vpc`
or `subnet` values, and uses the existing `parent` relationship as the sole
membership source. Coordinates remain absolute page coordinates. Older files
are migrated without automatic layout changes. Drop highlights and ghost
previews are transient editor state and are excluded from saved files and SVG.

Copying, duplicating or deleting a container includes its descendants. A complete
network copy gives resources new IDs and remaps internal VPC references. Copying
only a subnet clears the external VPC dependency, preserving the existing copy
rules. Page duplication also remaps internal references. SVG exports include
the nested boundaries, headers, CIDRs, availability zones and service labels.

## Verification

The implementation passes `cargo fmt --all --check`,
`cargo clippy --workspace --all-targets -- -D warnings`, and
`cargo test --workspace` (431 tests). Coverage includes real egui pointer
interactions, palette insertion, group editing, transfer validation,
cancellation, undo/redo, nested copies and page duplication, older files, SVG
output, and Terraform identity and configuration invariance for layout edits.

The existing release scene performance gate passes with 5,000 elements: cold
construction 74.23 ms, warm construction 3.63 ms, drag rebuild 6.09 ms and 1,000
hit tests 2.41 ms. The existing AWS trial provider validation and mocked apply
pass for both the base and Lambda variants, using Terraform 1.16.6 and AWS
provider 6.68.0. These checks perform no live AWS operations.

The [network screenshot gallery](screenshots/README.md#interactive-network-containers)
contains inspected application renders of the nested layout in both themes,
an active service drop, and a completed subnet transfer, alongside matching SVG
exports and reproduction commands.
