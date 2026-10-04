# Phase 3 — cloud architecture, first milestone

This starts the cloud work requested by the user. Phase 3 is not complete;
the remaining ERD and editor work is still tracked in phase-2.md and phase-1.md.

## Available now

- Choose **Cloud architecture** for a new document or page. It has AWS and
  Azure provider tabs, categories, service-name labels and basic shapes.
- **Manage icon packs…** in the palette or **File → Cloud icon packs…**
  links to each vendor's official download and usage terms. Accept the terms,
  choose the release date with the calendar or enter the downloaded release's
  version, and import its original ZIP.
  Importing runs in the background. No vendor artwork is bundled in this repo.
- The importer derives names, categories and service/resource/group metadata
  from vendor filenames. It prefers the largest SVG size variant and keeps
  the SVG bytes unchanged. Search includes familiar aliases such as VM,
  bucket and serverless; hover shows a larger preview and the full name.
- Click or drag an icon onto the canvas, or use `/` to search. Four connection
  ports work with the existing connector tool (`C`) and routing inspector.
- Icons keep their original colours and aspect ratio. Canvas and inspector
  resizing scale them uniformly. Labels remain editable below the icon.
- Used artwork travels inside native documents and across copy/paste.
  Reopening and headless SVG export do not require installed icon packs.
  Format 4 migrates existing files and prevents earlier app versions from
  resaving documents without their embedded artwork.
- References include provider, icon ID and pack version. Installing a new
  version preserves old packs; the palette can select an installed release.
- Canvas and palette share a bounded SVG parse/texture cache with zoom and
  DPI raster buckets. Export embeds the original SVG in an image element.

Installed packs live in `$XDG_DATA_HOME/blueprint/icon-packs` or
`~/.local/share/blueprint/icon-packs` on Linux, and
`%LOCALAPPDATA%/blueprint/icon-packs` on Windows. The standalone importer
can also write to a chosen directory:

```sh
cargo run -p bp-icons --bin icon-import -- \
  --provider aws --version RELEASE --input official-pack.zip --output ICON_DIRECTORY
```

The importer accepts static SVG vector artwork, internal gradients and
fragment references. Text that needs font rendering, embedded raster images,
active content, external resources,
ambiguous IDs, unsafe ZIP paths and excessive input sizes produce an error
instead of silently changing the original icon. Tests use original synthetic
artwork rather than vendor assets.

## Vendor sources and usage

[AWS Architecture Icons](https://aws.amazon.com/architecture/icons/) publishes
architecture diagram packages for customers and partners.
[Azure architecture icons](https://learn.microsoft.com/en-us/azure/architecture/icons/)
permits architecture diagrams, training and documentation, and prohibits
cropping, flipping, rotating or distorting its icons. The app preserves
artwork and displays service names by default. Users review current terms
on the vendor's site when importing; this milestone uses local imports.

## Remaining Phase 3 work

- Download and update official packs directly after acceptance of current terms.
- Real cloud containers and child ownership: AWS Cloud, Region, AZ, VPC,
  subnets and security groups; neutral Azure subscription/resource-group/
  virtual-network/subnet boundaries with automatic growth and nested movement.
  Imported group artwork currently behaves as an ordinary icon.
- Cloud recents/favourites, a curated alias data file, icon rename/retirement
  mapping, and cloud-specific architecture examples.
- Full-pack compatibility and performance checks for both providers on Linux
  and Windows, and a distribution review before shipping vendor artwork.

The missing embedded roadmap in the supplied plan does not expose a precise
Phase 3 gate. This milestone establishes the import-to-diagram path; it does
not claim the entire phase or its release gate has passed.
