#!/usr/bin/env python3
"""Validate exported trial JSON with the real AWS schema, then mock an apply.

Requires a built blueprint-cli and Terraform >=1.7. Provider initialization may
download binaries. terraform test uses a mock provider: no AWS login or resource
operations are performed.
"""

import argparse
import ast
import base64
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile
import zipfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cli", required=True, type=Path)
    parser.add_argument("--terraform", default="terraform")
    args = parser.parse_args()
    cli = str(args.cli.resolve(strict=True))

    with tempfile.TemporaryDirectory(prefix="blueprint-aws-trial-") as tmp:
        for with_lambda in (False, True):
            root = Path(tmp) / ("lambda" if with_lambda else "base")
            root.mkdir()
            validate(args, cli, root, with_lambda)


def validate(args, cli, root, with_lambda):
    def run(*command):
        subprocess.run(command, cwd=root, check=True)

    flags = ["--lambda"] if with_lambda else []
    run(cli, "aws-trial", "trial.blueprint.json", "--account", "123456789012", *flags)
    run(cli, "aws-export", "trial.blueprint.json", "main.tf.json")
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

    extra_asserts = ""
    if with_lambda:
        function = address("aws_lambda_function")
        role = address("aws_iam_role")
        execution = address("aws_iam_role_policy")
        logs = address("aws_cloudwatch_log_group")
        trigger = address("aws_lambda_event_source_mapping")
        settings = resources["aws_lambda_function"][function.split(".")[1]]
        assert settings["depends_on"] == [execution, logs]
        archives = list(root.glob("blueprint-lambda-*.zip"))
        assert len(archives) == 1
        document = json.loads((root / "trial.blueprint.json").read_text())
        source_settings = [
            element["infrastructure"]
            for element in document["elements"].values()
            if element.get("infrastructure", {}).get("type") == "lambda_function"
        ]
        assert len(source_settings) == 1
        with zipfile.ZipFile(archives[0]) as package:
            assert package.namelist() == ["lambda_function.py"]
            source = package.read("lambda_function.py").decode("utf-8")
        assert source == source_settings[0]["source_code"]
        ast.parse(source)
        namespace = {}
        exec(compile(source, "lambda_function.py", "exec"), namespace)
        event = {"Records": [{"body": "one"}, {"body": "two"}]}
        assert namespace["lambda_handler"](event, None) == {"processed": 2}
        digest = base64.b64encode(
            hashlib.sha256(archives[0].read_bytes()).digest()
        ).decode("ascii")
        extra_asserts = f'''
  assert {{
    condition = {function}.source_code_hash == "{digest}" && {function}.runtime == "python3.13" && {function}.handler == "lambda_function.lambda_handler" && {function}.timeout == 5 && {function}.memory_size == 128
    error_message = "The Lambda must deploy the exact packaged Python worker."
  }}
  assert {{
    condition = {trigger}.event_source_arn == {queue}.arn && {trigger}.function_name == {function}.arn && {trigger}.batch_size == 10 && {trigger}.enabled
    error_message = "The trigger must connect the selected queue to the worker."
  }}
  assert {{
    condition = {function}.role == {role}.arn && jsondecode({role}.assume_role_policy).Statement[0].Principal.Service == "lambda.amazonaws.com"
    error_message = "Only Lambda may assume the generated execution role."
  }}
  assert {{
    condition = jsondecode({execution}.policy).Statement[0].Resource == "${{{logs}.arn}}:*" && jsondecode({execution}.policy).Statement[1].Resource == {queue}.arn && length(jsondecode({execution}.policy).Statement) == 2 && {logs}.retention_in_days == 14
    error_message = "Execution permissions must be scoped to the function logs and selected queue."
  }}
'''

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
  mock_resource "aws_iam_role" {{
    defaults = {{ arn = "arn:aws:iam::123456789012:role/trial-lambda" }}
  }}
  mock_resource "aws_cloudwatch_log_group" {{
    defaults = {{ arn = "arn:aws:logs:us-east-1:123456789012:log-group:trial-lambda" }}
  }}
  mock_resource "aws_lambda_function" {{
    defaults = {{ arn = "arn:aws:lambda:us-east-1:123456789012:function:trial-lambda" }}
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
{extra_asserts}
}}
''')
    run(args.terraform, "init", "-backend=false", "-input=false", "-no-color")
    run(args.terraform, "validate", "-no-color")
    run(args.terraform, "test", "-no-color")


if __name__ == "__main__":
    main()
