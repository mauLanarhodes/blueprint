# AWS Foundation checkpoint

This checkpoint adds the foundation for broad AWS icon configuration. It does
**not** enable general AWS deployment or generated configuration forms. The
existing reviewed VPC/private-subnet workflow is preserved. That deployer refuses
a page containing general resource drafts instead of ignoring them or generating
a partial configuration that could delete existing resources.

## Inventory and icon resolution

`crates/bp-deploy/catalogs/` preserves the two supplied source lists dated
2026-10-14: 1,724 AWS 6.68.0 entries in 255 categories and 1,664 AWSCC 1.105.0
entries in 292 prefix groups. These catalogs overlap. Neither their sum nor their
category counts is a count of unique AWS services or implemented deployments.
The manifest records source URLs, exact versions, counts and the CSV SHA-256.

```sh
python3 tools/update_aws_catalog.py
python3 tools/update_aws_catalog.py --check
cargo run -p bp-cli -- aws-catalog --provider aws --search 's3 bucket'
cargo run -p bp-cli -- aws-icons diagram.blueprint
```

`Catalog::resolve_icon` accepts any validated imported icon. It returns curated
resource choices for known names, context for recognized account/region/AZ group
icons, or an explicit resource-selection requirement. Azure has a separate
unsupported-provider result. Unrecognized AWS artwork is never guessed into a
deployment. `IconMappings` accepts reviewed overrides keyed by the exact artwork
reference including pack version. Choices are checked against catalog releases;
they are not complete architecture templates. No official artwork is bundled.
`aws-icons` reports embedded document icons. The resolver API also accepts every
icon from an installed provider pack; full-pack mapping review requires that pack
and is not established by synthetic tests or provider catalog counts.

## Drafts, identity and compatibility

Schema 7 adds `AwsResource::Resources` with multiple `TerraformResource` drafts
per shape. Each stores an exact provider pin, type, stable local key, nested
properties, explicit dependencies and selected output references. An address
derives from the owner UUID and local key, independent of label and position.
Changing a key/provider changes identity; later UI stages must expose that.
Existing VPC/subnet variants keep their original serialization and Terraform
addresses. Migrations do not rewrite their resource data.

`ResourceValue` distinguishes literal JSON, lists, objects, references and inputs.
A `${...}` literal remains document data; expression generation is a later stage.
`InputReference` stores only a name and sensitivity flag, never the secret value.
The schema checker flags literals in sensitive fields. The future editor must
use deployment inputs for those fields.

Infrastructure property commands validate metadata and preserve undo/redo.
Copies and duplicated pages remap internal references and receive new addresses.
Dependencies outside a copy become explicitly unresolved instead of pointing to
the original infrastructure. Deleted targets are reported as missing; undo
restores them. Saving incomplete drafts is allowed; readiness checks block their
future planning rather than making unfinished work unsaveable.

## Provider schemas and offline checks

Install Terraform separately. Schema fetch is explicit and may download provider
binaries. Use a dedicated cache directory with an existing parent:

```sh
cargo run -p bp-cli -- aws-schema fetch aws ./aws-schemas
cargo run -p bp-cli -- aws-schema fetch awscc ./aws-schemas
cargo run -p bp-cli -- aws-schema check aws ./aws-schemas
cargo run -p bp-cli -- aws-check diagram.blueprint ./aws-schemas
```

`fetch` runs only `init -backend=false`, `version -json`, and `providers schema
-json` in separate directories per provider/release. It never runs plan/apply or
an AWS operation. `--terraform /absolute/path/to/terraform` selects an executable.
Initialization constrains exact provider versions, and version output verifies
the selections. A resource schema's `version` is kept separate from its provider
release. Terraform dependency lock files are retained.

The cache preserves the complete provider payload, including unknown minor-format
fields, nested blocks, computed outputs and sensitive attributes. Loading checks
its format and exact release with no fallback. Managed cache paths reject
symbolic links and unrelated workspace files; an exclusive marker prevents
concurrent fetches. After an interruption, verify no process is running before
removing the marker/output files. Do not edit snapshots to disguise mismatches.
Caches are local tooling data, not portable diagram contents.

`check` and `aws-check` are offline and never initialize providers. Catalog/schema
coverage reports list missing and additional types and fail on drift. Draft
diagnostics cover structural schema errors, missing snapshots, invalid or
cross-page targets, unresolved dependencies, cycles and conflicting releases.
These checks do not establish Terraform plan validity, AWS permissions/quotas,
artifact availability or deployment testing. Provider-specific rules still need
Terraform validation in the next checkpoint.

## Validation and next checkpoint

Foundation regressions cover the supplied inventory, release verification,
nested properties, cache isolation, migrations, command replay and copies. Mock
discovery accepts schema commands only. Run the ordinary formatting, Clippy and
workspace tests, plus the catalog drift check above.

Next: generated forms, relationship editing and generalized Terraform generation.
Catalog coverage, icon mappings, structural checks, plan validation and actual
deployment testing remain separate reported states.

Format references:

- https://developer.hashicorp.com/terraform/cli/commands/providers/schema
- https://developer.hashicorp.com/terraform/language/files/dependency-lock
