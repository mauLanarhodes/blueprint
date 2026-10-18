#!/usr/bin/env python3
"""Validate exported trial JSON with the real AWS schema, then mock an apply.

Requires a built blueprint-cli and Terraform >=1.7. Provider initialization may
download binaries. terraform test uses a mock provider: no AWS login or resource
operations are performed.
"""

import argparse
import json
from pathlib import Path
import subprocess
import tempfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cli", required=True, type=Path)
    parser.add_argument("--terraform", default="terraform")
    args = parser.parse_args()
    cli = str(args.cli.resolve(strict=True))

    with tempfile.TemporaryDirectory(prefix="blueprint-aws-trial-") as tmp:
        root = Path(tmp)

        def run(*command):
            subprocess.run(command, cwd=root, check=True)

        run(cli, "aws-trial", "trial.blueprint", "--account", "123456789012")
        run(cli, "aws-export", "trial.blueprint", "main.tf.json")
        path = root / "main.tf.json"
        config = json.loads(path.read_text())
        # Select the documented test release within the generated 6.x constraint.
        config["terraform"]["required_providers"]["aws"]["version"] = "= 6.68.0"
        path.write_text(json.dumps(config, indent=2) + "\n")
        resources = config["resource"]

        def address(kind):
            names = list(resources[kind])
            assert len(names) == 1, (kind, names)
            return f"{kind}.{names[0]}"

        bucket = address("aws_s3_bucket")
        block = address("aws_s3_bucket_public_access_block")
        versioning = address("aws_s3_bucket_versioning")
        table = address("aws_dynamodb_table")
        queue = address("aws_sqs_queue")
        topic = address("aws_sns_topic")
        policy = address("aws_sqs_queue_policy")
        subscription = address("aws_sns_topic_subscription")

        tests = root / "tests"
        tests.mkdir()
        (tests / "trial.tftest.hcl").write_text(f'''
mock_provider "aws" {{
  mock_data "aws_caller_identity" {{
    defaults = {{ account_id = "123456789012" }}
  }}
  mock_resource "aws_sqs_queue" {{
    defaults = {{
      arn = "arn:aws:sqs:us-east-1:123456789012:trial-queue"
      id = "https://sqs.us-east-1.amazonaws.com/123456789012/trial-queue"
      url = "https://sqs.us-east-1.amazonaws.com/123456789012/trial-queue"
    }}
  }}
  mock_resource "aws_sns_topic" {{
    defaults = {{
      arn = "arn:aws:sns:us-east-1:123456789012:trial-topic"
      id = "arn:aws:sns:us-east-1:123456789012:trial-topic"
    }}
  }}
}}
run "apply_trial" {{
  command = apply
  assert {{
    condition = {bucket}.force_destroy == false && {block}.block_public_acls && {block}.block_public_policy && {block}.ignore_public_acls && {block}.restrict_public_buckets
    error_message = "The bucket must block public access and preserve objects on deletion."
  }}
  assert {{
    condition = {block}.bucket == {bucket}.id && {versioning}.bucket == {bucket}.id && {versioning}.versioning_configuration[0].status == "Enabled"
    error_message = "Bucket controls must refer to the correct bucket."
  }}
  assert {{
    condition = {table}.billing_mode == "PAY_PER_REQUEST" && {table}.hash_key == "id"
    error_message = "The trial table must use the configured key and on-demand billing."
  }}
  assert {{
    condition = {subscription}.topic_arn == {topic}.arn && {subscription}.endpoint == {queue}.arn && {subscription}.protocol == "sqs"
    error_message = "The subscription must connect the intended SNS topic and SQS queue."
  }}
  assert {{
    condition = jsondecode({policy}.policy).Statement[0].Resource == {queue}.arn && jsondecode({policy}.policy).Statement[0].Condition.ArnEquals["aws:SourceArn"] == {topic}.arn
    error_message = "The queue policy must allow delivery from only the selected topic."
  }}
  assert {{
    condition = {queue}.sqs_managed_sse_enabled && output.aws_account_id == "123456789012"
    error_message = "The queue must be encrypted and the target account retained."
  }}
}}
''')
        run(args.terraform, "init", "-backend=false", "-input=false", "-no-color")
        run(args.terraform, "validate", "-no-color")
        run(args.terraform, "test", "-no-color")


if __name__ == "__main__":
    main()
