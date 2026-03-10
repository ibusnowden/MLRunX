//! Shared HTTP payload types for API handlers.

use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct UiAuthLoginRequest {
    #[serde(default)]
    pub jwt: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct UiAuthLoginResponse {
    pub status: String,
    pub user_id: String,
    pub expires_at: String,
    pub project_count: usize,
}

#[derive(Debug, Serialize)]
pub struct UiAuthSessionResponse {
    pub authenticated: bool,
    pub auth_mode: String,
    pub scopes: Vec<String>,
    pub project_ids: Vec<String>,
    pub key_prefix: String,
    pub is_dev_mode: bool,
    pub is_platform_admin: bool,
    pub ui_session_ttl_seconds: u64,
    pub ui_key_max_ttl_seconds: u64,
}

#[derive(Debug, Serialize)]
pub struct UiAuthLogoutResponse {
    pub status: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct IngestBatchHttpRequest {
    pub run_id: String,
    #[serde(default)]
    pub batch_id: Option<String>,
    #[serde(default)]
    pub seq: Option<i64>,
    #[serde(default)]
    pub metrics: Vec<MetricData>,
    #[serde(default)]
    pub params: Vec<ParamData>,
    #[serde(default)]
    pub tags: Vec<TagData>,
    #[serde(default)]
    pub events: Vec<LogEventData>,
    #[serde(default)]
    pub timestamp: Option<f64>,
    #[serde(default)]
    pub stats: Option<BatchStats>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MetricData {
    pub name: String,
    pub value: MetricValue,
    pub step: i64,
    #[serde(default)]
    pub timestamp: Option<f64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum MetricValue {
    Scalar(f64),
    Array(Vec<f64>),
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ParamData {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TagData {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LogEventData {
    #[serde(default)]
    pub level: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    pub message: String,
    #[serde(default)]
    pub step: Option<i64>,
    #[serde(default)]
    pub timestamp: Option<f64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[allow(clippy::struct_field_names)]
pub struct BatchStats {
    #[serde(default)]
    pub metric_count: Option<i64>,
    #[serde(default)]
    pub param_count: Option<i64>,
    #[serde(default)]
    pub tag_count: Option<i64>,
    #[serde(default)]
    pub coalesced_count: Option<i64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct IngestBatchHttpResponse {
    pub status: String,
    pub accepted: i64,
    pub duplicate: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct QueuedMetricData {
    pub name: String,
    pub value: f64,
    pub step: i64,
    #[serde(default)]
    pub timestamp: Option<f64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct QueuedLogEventData {
    pub level: String,
    pub source: String,
    pub message: String,
    #[serde(default)]
    pub step: Option<i64>,
    #[serde(default)]
    pub timestamp: Option<f64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct QueuedIngestBatch {
    pub project_id: String,
    pub run_id: String,
    pub batch_id: String,
    pub payload_hash: String,
    pub seq: i64,
    pub queued_at_unix_ms: i64,
    #[serde(default)]
    pub metrics: Vec<QueuedMetricData>,
    #[serde(default)]
    pub params: Vec<ParamData>,
    #[serde(default)]
    pub tags: Vec<TagData>,
    #[serde(default)]
    pub events: Vec<QueuedLogEventData>,
    #[serde(default)]
    pub warnings: Vec<String>,
}
