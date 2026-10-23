# Deploy real AWS infrastructure

Blueprint can provision S3 buckets, DynamoDB tables, standard SNS topics, SQS
queues and Python Lambda functions, plus VPCs and private IPv4 subnets, from a Cloud architecture page.
See [the AWS trial guide](aws-trial.md) for service options, the editable example,
headless validation and extension points. The app generates Terraform configuration, previews a saved
plan, and applies that exact plan when you choose **Deploy reviewed plan**.
AWS/Azure service artwork remains available for diagrams; Azure deployment and
AWS services outside this trial are not implemented yet.

## Setup

Install [Terraform](https://developer.hashicorp.com/terraform/install), version
1.5 or later in the 1.x series, and make `terraform` available on the desktop
application's PATH. Blueprint uses the HashiCorp AWS provider 6.x; the first
preview downloads it, and its lock file preserves the selected version.

Sign in to AWS using your existing local AWS profile, SSO session, or environment
credentials. The account needs permissions to describe, create, tag and update
all requested services and supporting resources. Plans that delete resources also need the
corresponding deletion permissions. Enter your intended 12-digit account ID:
the provider is restricted to that account. Blueprint does not request or store
access keys in the document.

Lambda also needs permission to manage execution roles and inline policies,
pass the generated role to Lambda (`iam:PassRole`), manage CloudWatch log groups,
and create/update/delete event source mappings when a queue is selected. The
function's execution role grants log-stream access to its own group and read,
delete and attribute access only to the selected queue. These execution
permissions are separate from your local deployment identity's permissions.

## Deploy from the editor

1. Create a **Cloud architecture** page. Click **Add AWS trial stack** for S3,
   DynamoDB, SNS and an SQS subscriber, or **Add starter network** for a VPC and
   two private subnets. **Add Lambda trial** includes a Python worker and its
   queue trigger. All starters work without installing icon packs. Alternatively,
   select an AWS icon or basic shape and choose a supported resource in its
   **Infrastructure** inspector.
2. Select each resource to edit its settings. Lambda exposes its name, Python
   source, timeout, memory and optional same-page SQS trigger. Its queue visibility
   must be at least six times the function timeout. Each subnet must reference a
   configured VPC on the same page, fit inside its CIDR, and avoid overlapping
   sibling subnets. You can specify an availability zone or let AWS choose it.
   These subnets do not assign public IPs; Blueprint does not create an internet
   gateway or NAT gateway.
   The starter uses nested VPC/subnet boundaries. Dragging a service into a
   subnet changes diagram placement only; it does not add VPC settings, routing
   or permissions to that service. Dragging a configured subnet to another VPC,
   or choosing its VPC in the inspector, validates the CIDRs and updates both
   the visual parent and deployment dependency together. This requires a new
   reviewed plan and may replace the deployed subnet. See
   [network containers](aws-network-containers.md) for editing and conversion.
3. Click **AWS deployment…** in the toolbar or File menu. Enter the
   AWS account ID, region, and optional local profile name. Choose a parent
   deployment folder. Blueprint creates `blueprint-PAGE_ID` beneath it.
4. Click **Preview deployment**. The background worker writes `main.tf.json` and
   any Lambda ZIP packages,
   initializes and validates Terraform, and saves `blueprint.tfplan`. Review the
   actual Terraform plan, including any updates, replacements, or deletions.
   Icons without resource configuration are listed as diagram-only.
5. Confirm that you reviewed the plan and click **Deploy reviewed plan**.
   This creates or changes real AWS resources and may incur charges. The app
   shows Terraform's result when the operation finishes.

Save your Blueprint document to preserve resource settings and the target.
Keep the deployment folder and choose the same parent folder for future
previews. Moving shapes does not change Terraform resource identities; copying
resources gives them new identities. Whole-network copies remap subnet
dependencies. A subnet copied without its VPC requires a new VPC selection.
Duplicated pages have separate deployment folders and infrastructure identities.
Copies preserve physical AWS names; rename copied services before deploying them
as a second stack. Topic references within a copied stack are remapped; a queue
copied without its topic has no subscription.
Lambda queue references are also remapped within a copied stack; copying the
function alone clears its trigger.

## State and recovery

Terraform state, provider files and saved plans stay in the deployment folder,
outside the Blueprint document. Do not remove or share that folder casually:
state and plans can contain sensitive cloud metadata, and deleting state does
not delete resources in AWS. This initial workflow uses local state; shared
remote state and collaborative deployment are not implemented.

Blueprint refuses unrelated Terraform source files, symbolic links, concurrent
operations, and configuration, plan or Lambda package files changed after preview. Source
edits also invalidate the reviewed plan. It uses the
default Terraform workspace. It consumes the saved plan after each apply
attempt; a failed apply can partially succeed, so preserve the state and create
a new preview before retrying. Terraform's own state locking remains enabled.
The app keeps the deployment dialog open during an operation.

If the app or machine crashes during an operation, first verify that no
Terraform process is still running. Then remove the `.blueprint-operation`
marker in that page's deployment folder and preview again. Manage any Terraform
state lock separately through Terraform's documented recovery procedure.

The generated configuration is visible in the preview dialog and available as
`main.tf.json` for use with the Terraform CLI. Blueprint has no separate Destroy
button; removing configured resources from a page can produce deletions in the
next reviewed plan. A page with no configured resources cannot generate a plan.
Keep exported Lambda ZIPs beside the JSON. Python source is saved as ordinary
document content; use the AWS runtime's credential chain rather than embedding
credentials or secrets in it.

References: [Terraform JSON configuration](https://developer.hashicorp.com/terraform/language/syntax/json),
[saved plans and apply](https://developer.hashicorp.com/terraform/tutorials/cli/plan),
[AWS subnet sizing](https://docs.aws.amazon.com/vpc/latest/userguide/subnet-sizing.html).
