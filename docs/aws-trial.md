# Working AWS deployment trial

Open `examples/aws-trial.blueprint.json`, or create a Cloud architecture page and
click **Add AWS trial stack**. Select a shape to configure it. Enter your account,
region and optional local AWS profile in **AWS deployment…**, choose a deployment
folder, preview the changes, then apply the reviewed plan. Terraform and a local
AWS login are required for live deployment; neither is needed for diagram editing.
The existing VPC/private-subnet starter and their Terraform addresses are preserved.

| Service | Editable settings | Generated infrastructure |
| --- | --- | --- |
| S3 bucket | Globally unique name; versioning | Bucket, public-access block and versioning configuration; never force-deletes objects |
| DynamoDB | Name; string partition key; optional string sort key | On-demand table with only its key attributes declared |
| SNS | Name | Standard topic |
| SQS | Name; visibility timeout; retention; optional SNS topic | Standard queue with SQS-managed encryption |
| SNS selected on SQS | Same-page topic selection | Subscription with raw message delivery, plus a queue policy permitting only that topic |

The four-service starter produces eight Terraform resources. Its SNS-to-SQS
connector corresponds to an explicitly configured subscription. S3 and DynamoDB
are independent services; the trial does not deploy application code between them.
Imported AWS icons with recognized service names offer a one-click configuration
suggestion. Any AWS icon or basic shape can be assigned a supported type manually.
Other AWS services, FIFO messaging, non-string DynamoDB keys, IAM editing, and
Azure deployment remain outside this trial. Unsupported icons are reported as
diagram-only before planning. Drawing a connector alone creates no AWS dependency.

New stacks receive unique default names. S3 names must also be globally available.
Copies get new Terraform identities and remap internal VPC/topic references.
Physical names are preserved: rename copies before deploying a second stack.
External copy references are cleared. Same-page duplicate names, invalid settings,
missing topics and cross-page dependencies block generation. Settings, undo/redo,
native/JSON files and page duplication use the existing editing workflow. Schema 7
protects the new variants; old VPC/subnet files migrate without address changes.

## Headless trial and validation

These commands create files only and never contact AWS:

```sh
cargo run -p bp-cli -- aws-trial trial.blueprint --account YOUR_12_DIGIT_ACCOUNT_ID --region us-east-1
cargo run -p bp-cli -- aws-export trial.blueprint main.tf.json
```

Omit `--account` to create an editable draft that requires target configuration
before export or deployment. `aws-trial` refuses to overwrite a project.
`aws-export` accepts `--page 2` or `--page "Page name"` and refuses to overwrite its
input project. The generated Terraform is also visible in the deployment dialog.

CI runs formatting, Clippy and workspace tests on Linux, Windows and macOS. The
provider check validates exported JSON against AWS 6.68.0, then executes a mocked
apply using Terraform 1.16.6 and checks bucket controls, table settings, queue
encryption, subscription endpoints and topic-scoped delivery permissions:

```sh
cargo build --locked -p bp-cli
python3 tools/check_aws_trial.py --cli target/debug/blueprint-cli
```

This check requires Terraform 1.7+ for mocks, downloads the real provider schema,
and makes no AWS resource calls. Mock applies do not establish live permissions,
quotas or successful provisioning in a specific account. The editor's real
plan/apply is the live deployment path. See [deployment setup and recovery](cloud-deployment.md)
for account guards, saved-plan checks, state preservation and failed operations.

## Cleanup

The page's deployment folder holds local Terraform state. Keep it for updates and
cleanup. Removing some configured shapes can produce deletions in the next plan;
an empty page cannot generate a plan. For complete removal, run these commands
inside that page's deployment folder and review the destroy plan before applying:

```sh
terraform plan -destroy -out=cleanup.tfplan
terraform show cleanup.tfplan
terraform apply cleanup.tfplan
```

S3 buckets containing objects or versions refuse deletion; remove the intended
data yourself first. Deleting state does not delete infrastructure.

## Expanding the trial

Add saved settings and a kind in `bp-model/src/infrastructure.rs`, fields in
`bp-app/src/deployment.rs`, and a validated generator with any supporting resources
in `bp-deploy/src/services.rs`. Add the kind to `AwsResourceKind::ALL`, explicit
icon aliases, persistence/copy coverage and a provider schema/mock-apply case.
Every service shares the account guard, reviewed saved plan and deployment worker.

References: [Terraform JSON](https://developer.hashicorp.com/terraform/language/syntax/json),
[mock providers](https://developer.hashicorp.com/terraform/language/tests/mocking),
[SQS](https://registry.terraform.io/providers/hashicorp/aws/6.68.0/docs/resources/sqs_queue),
[SNS subscriptions](https://registry.terraform.io/providers/hashicorp/aws/6.68.0/docs/resources/sns_topic_subscription),
[DynamoDB](https://registry.terraform.io/providers/hashicorp/aws/6.68.0/docs/resources/dynamodb_table),
[S3 versioning](https://registry.terraform.io/providers/hashicorp/aws/6.68.0/docs/resources/s3_bucket_versioning).
