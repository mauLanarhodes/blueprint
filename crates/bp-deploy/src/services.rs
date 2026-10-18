//! Small, complete service generators for the AWS trial. Keep service-specific
//! validation and supporting resources together when extending this registry.

use crate::{literal, logical_name, reference, reference_attribute};
use bp_model::{AwsResource, ElementId};
use serde_json::{Map, Value, json};
use std::collections::BTreeSet;

pub(crate) fn generate(
    id: ElementId,
    resource: &AwsResource,
    topics: &BTreeSet<ElementId>,
    resources: &mut Map<String, Value>,
) -> Result<(&'static str, Value), String> {
    let logical = logical_name(id);
    match resource {
        AwsResource::S3Bucket { name, versioning } => {
            let alphanumeric = |b: u8| b.is_ascii_lowercase() || b.is_ascii_digit();
            if !(3..=63).contains(&name.len())
                || !name.bytes().all(|b| alphanumeric(b) || b == b'-')
                || !name.bytes().next().is_some_and(alphanumeric)
                || !name.bytes().last().is_some_and(alphanumeric)
                || ["xn--", "sthree-", "amzn-s3-demo-"]
                    .iter()
                    .any(|p| name.starts_with(p))
                || ["-s3alias", "--ol-s3", "--x-s3", "--table-s3"]
                    .iter()
                    .any(|s| name.ends_with(s))
            {
                return Err("Bucket name must use 3–63 lowercase letters, digits or hyphens, start/end with a letter or digit, and avoid reserved S3 names".into());
            }
            insert(
                resources,
                "aws_s3_bucket_public_access_block",
                &logical,
                json!({
                    "bucket": reference("aws_s3_bucket", id),
                    "block_public_acls": true,
                    "block_public_policy": true,
                    "ignore_public_acls": true,
                    "restrict_public_buckets": true
                }),
            );
            insert(
                resources,
                "aws_s3_bucket_versioning",
                &logical,
                json!({
                    "bucket": reference("aws_s3_bucket", id),
                    "versioning_configuration": [{"status": if *versioning { "Enabled" } else { "Suspended" }}]
                }),
            );
            Ok((
                "aws_s3_bucket",
                json!({"bucket": name, "force_destroy": false}),
            ))
        }
        AwsResource::DynamoDbTable {
            name,
            partition_key,
            sort_key,
        } => {
            valid_name(name, 3, 255, true)?;
            key_name(partition_key)?;
            let mut attributes = vec![json!({"name": literal(partition_key), "type": "S"})];
            let mut body = json!({
                "name": name,
                "billing_mode": "PAY_PER_REQUEST",
                "hash_key": literal(partition_key)
            });
            if !sort_key.is_empty() {
                key_name(sort_key)?;
                if partition_key == sort_key {
                    return Err("Partition and sort keys must have different names".into());
                }
                attributes.push(json!({"name": literal(sort_key), "type": "S"}));
                body["range_key"] = literal(sort_key).into();
            }
            body["attribute"] = attributes.into();
            Ok(("aws_dynamodb_table", body))
        }
        AwsResource::SnsTopic { name } => {
            valid_name(name, 1, 256, false)?;
            Ok(("aws_sns_topic", json!({"name": name})))
        }
        AwsResource::SqsQueue {
            name,
            visibility_timeout,
            retention_seconds,
            topic,
        } => {
            valid_name(name, 1, 80, false)?;
            if *visibility_timeout > 43_200 {
                return Err("Visibility timeout must be 0–43,200 seconds".into());
            }
            if !(60..=1_209_600).contains(retention_seconds) {
                return Err("Message retention must be 60–1,209,600 seconds".into());
            }
            if let Some(topic) = topic {
                if !topics.contains(topic) {
                    return Err(
                        "Choose a configured SNS topic on this page, or clear the subscription"
                            .into(),
                    );
                }
                let queue_arn = reference_attribute("aws_sqs_queue", id, "arn");
                let topic_arn = reference_attribute("aws_sns_topic", *topic, "arn");
                let policy = json!({
                    "Version": "2012-10-17",
                    "Statement": [{
                        "Sid": "BlueprintSnsDelivery",
                        "Effect": "Allow",
                        "Principal": {"Service": "sns.amazonaws.com"},
                        "Action": "sqs:SendMessage",
                        "Resource": queue_arn,
                        "Condition": {"ArnEquals": {"aws:SourceArn": topic_arn}}
                    }]
                });
                insert(
                    resources,
                    "aws_sqs_queue_policy",
                    &logical,
                    json!({
                        "queue_url": reference("aws_sqs_queue", id),
                        "policy": policy.to_string()
                    }),
                );
                insert(
                    resources,
                    "aws_sns_topic_subscription",
                    &logical,
                    json!({
                        "topic_arn": topic_arn,
                        "protocol": "sqs",
                        "endpoint": queue_arn,
                        "raw_message_delivery": true,
                        "depends_on": [format!("aws_sqs_queue_policy.{logical}")]
                    }),
                );
            }
            Ok((
                "aws_sqs_queue",
                json!({
                    "name": name,
                    "fifo_queue": false,
                    "sqs_managed_sse_enabled": true,
                    "visibility_timeout_seconds": visibility_timeout,
                    "message_retention_seconds": retention_seconds
                }),
            ))
        }
        _ => Err("Resource is handled by the network generator".into()),
    }
}

fn insert(resources: &mut Map<String, Value>, kind: &str, logical: &str, value: Value) {
    resources.entry(kind).or_insert_with(|| json!({}))[logical] = value;
}

fn valid_name(name: &str, min: usize, max: usize, dots: bool) -> Result<(), String> {
    if !(min..=max).contains(&name.len())
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-') || (dots && b == b'.'))
    {
        Err(format!(
            "Name must use {min}–{max} ASCII letters, digits, underscores or hyphens{}",
            if dots { " (dots also allowed)" } else { "" }
        ))
    } else {
        Ok(())
    }
}

fn key_name(name: &str) -> Result<(), String> {
    if name.is_empty() || name.len() > 255 || name.chars().any(char::is_control) {
        Err("Key names must contain 1–255 bytes without control characters".into())
    } else {
        Ok(())
    }
}
