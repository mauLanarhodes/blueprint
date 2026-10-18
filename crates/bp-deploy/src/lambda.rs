//! One-module Python Lambda deployment, with explicit queue access and logging.

use crate::{logical_name, reference, reference_attribute};
use bp_model::{AwsResource, ElementId};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::io::{Cursor, Write};
use zip::write::SimpleFileOptions;

pub(crate) fn generate(
    id: ElementId,
    resource: &AwsResource,
    queues: &BTreeMap<ElementId, u32>,
    resources: &mut Map<String, Value>,
    packages: &mut BTreeMap<String, Vec<u8>>,
) -> Result<(&'static str, Value), String> {
    let AwsResource::LambdaFunction {
        name,
        source_code,
        timeout_seconds,
        memory_mb,
        queue,
    } = resource
    else {
        return Err("Expected a Lambda function".into());
    };
    crate::services::valid_name(name, 1, 64, false)?;
    if source_code.trim().is_empty() || source_code.len() > 65_536 || source_code.contains('\0') {
        return Err("Python source must contain 1–65,536 bytes and no NUL characters".into());
    }
    if !(1..=900).contains(timeout_seconds) {
        return Err("Lambda timeout must be 1–900 seconds".into());
    }
    if !(128..=10_240).contains(memory_mb) {
        return Err("Lambda memory must be 128–10,240 MB".into());
    }
    if let Some(queue) = queue {
        let visibility = queues
            .get(queue)
            .ok_or("Choose a configured SQS queue on this page, or clear the Lambda trigger")?;
        if *visibility < 6 * timeout_seconds {
            return Err(format!(
                "SQS visibility timeout must be at least {} seconds (six times the Lambda timeout)",
                6 * timeout_seconds
            ));
        }
    }

    let logical = logical_name(id);
    let archive = format!("blueprint-lambda-{logical}.zip");
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Stored)
        .last_modified_time(zip::DateTime::default())
        .unix_permissions(0o644);
    writer
        .start_file("lambda_function.py", options)
        .map_err(|e| e.to_string())?;
    writer
        .write_all(source_code.as_bytes())
        .map_err(|e| e.to_string())?;
    packages.insert(
        archive.clone(),
        writer.finish().map_err(|e| e.to_string())?.into_inner(),
    );

    let role_name = format!("blueprint-lambda-{}", &logical[18..]);
    insert(
        resources,
        "aws_iam_role",
        &logical,
        json!({
            "name": role_name,
            "assume_role_policy": json!({
                "Version": "2012-10-17",
                "Statement": [{
                    "Effect": "Allow",
                    "Principal": {"Service": "lambda.amazonaws.com"},
                    "Action": "sts:AssumeRole"
                }]
            }).to_string()
        }),
    );
    insert(
        resources,
        "aws_cloudwatch_log_group",
        &logical,
        json!({"name": format!("/aws/lambda/{name}"), "retention_in_days": 14}),
    );
    let mut statements = vec![json!({
        "Effect": "Allow",
        "Action": ["logs:CreateLogStream", "logs:PutLogEvents"],
        "Resource": format!("{}:*", reference_attribute("aws_cloudwatch_log_group", id, "arn"))
    })];
    if let Some(queue) = queue {
        statements.push(json!({
            "Effect": "Allow",
            "Action": ["sqs:ReceiveMessage", "sqs:DeleteMessage", "sqs:GetQueueAttributes"],
            "Resource": reference_attribute("aws_sqs_queue", *queue, "arn")
        }));
        insert(
            resources,
            "aws_lambda_event_source_mapping",
            &logical,
            json!({
                "event_source_arn": reference_attribute("aws_sqs_queue", *queue, "arn"),
                "function_name": reference_attribute("aws_lambda_function", id, "arn"),
                "batch_size": 10,
                "enabled": true
            }),
        );
    }
    insert(
        resources,
        "aws_iam_role_policy",
        &logical,
        json!({
            "name": "blueprint-execution",
            "role": reference("aws_iam_role", id),
            "policy": json!({"Version": "2012-10-17", "Statement": statements}).to_string()
        }),
    );
    Ok((
        "aws_lambda_function",
        json!({
            "function_name": name,
            "role": reference_attribute("aws_iam_role", id, "arn"),
            "runtime": "python3.13",
            "handler": "lambda_function.lambda_handler",
            "filename": format!("${{path.module}}/{archive}"),
            "source_code_hash": format!("${{filebase64sha256(\"${{path.module}}/{archive}\")}}"),
            "timeout": timeout_seconds,
            "memory_size": memory_mb,
            "architectures": ["x86_64"],
            "depends_on": [
                format!("aws_iam_role_policy.{logical}"),
                format!("aws_cloudwatch_log_group.{logical}")
            ]
        }),
    ))
}

fn insert(resources: &mut Map<String, Value>, kind: &str, logical: &str, value: Value) {
    resources.entry(kind).or_insert_with(|| json!({}))[logical] = value;
}
