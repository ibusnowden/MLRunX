//! Cardinality guardrails for ingest operations.
//!
//! Enforces limits on:
//! - Tag keys per run
//! - Unique metric names per run
//! - Total tags per project
//!
//! Prevents high-cardinality data from overwhelming `ClickHouse`.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use tokio::sync::RwLock;
use tracing::{debug, warn};

/// Configuration for cardinality limits.
#[derive(Debug, Clone)]
#[allow(clippy::struct_field_names)]
pub struct LimitsConfig {
    /// Maximum tag keys per run
    pub max_tag_keys_per_run: usize,
    /// Maximum unique metric names per run
    pub max_metric_names_per_run: usize,
    /// Maximum total tags per project
    pub max_tags_per_project: usize,
    /// Maximum tag key length
    pub max_tag_key_length: usize,
    /// Maximum tag value length
    pub max_tag_value_length: usize,
    /// Maximum metric name length
    pub max_metric_name_length: usize,
}

impl Default for LimitsConfig {
    fn default() -> Self {
        Self {
            max_tag_keys_per_run: 100,
            max_metric_names_per_run: 1000,
            max_tags_per_project: 10000,
            max_tag_key_length: 256,
            max_tag_value_length: 1024,
            max_metric_name_length: 256,
        }
    }
}

impl LimitsConfig {
    /// Create config from environment variables.
    pub fn from_env() -> Self {
        let mut config = Self::default();

        if let Ok(val) = std::env::var("MLRUNX_MAX_TAG_KEYS_PER_RUN") {
            if let Ok(n) = val.parse() {
                config.max_tag_keys_per_run = n;
            }
        }

        if let Ok(val) = std::env::var("MLRUNX_MAX_METRIC_NAMES_PER_RUN") {
            if let Ok(n) = val.parse() {
                config.max_metric_names_per_run = n;
            }
        }

        if let Ok(val) = std::env::var("MLRUNX_MAX_TAGS_PER_PROJECT") {
            if let Ok(n) = val.parse() {
                config.max_tags_per_project = n;
            }
        }

        config
    }
}

/// Result of validating a batch against cardinality limits.
#[derive(Debug, Clone, Default)]
pub struct ValidationResult {
    /// Tags that were accepted
    pub accepted_tags: Vec<(String, String)>,
    /// Metrics that were accepted
    pub accepted_metrics: Vec<String>,
    /// Tags that were dropped due to limits
    pub dropped_tags: Vec<(String, String)>,
    /// Metrics that were dropped due to limits
    pub dropped_metrics: Vec<String>,
    /// Warning messages
    pub warnings: Vec<String>,
}

impl ValidationResult {
    /// Returns true if anything was dropped.
    pub fn has_drops(&self) -> bool {
        !self.dropped_tags.is_empty() || !self.dropped_metrics.is_empty()
    }

    /// Get a summary warning message.
    pub fn summary_warning(&self) -> Option<String> {
        if !self.has_drops() {
            return None;
        }

        let mut parts = Vec::new();

        if !self.dropped_tags.is_empty() {
            parts.push(format!("{} tags dropped", self.dropped_tags.len()));
        }

        if !self.dropped_metrics.is_empty() {
            parts.push(format!("{} metrics dropped", self.dropped_metrics.len()));
        }

        Some(format!("Cardinality limits exceeded: {}", parts.join(", ")))
    }
}

/// Per-run cardinality tracking.
#[derive(Debug, Default)]
struct RunCardinality {
    /// Unique tag keys seen for this run
    tag_keys: HashSet<String>,
    /// Unique metric names seen for this run
    metric_names: HashSet<String>,
}

/// Per-project cardinality tracking.
#[derive(Debug, Default)]
struct ProjectCardinality {
    /// Total unique (`tag_key`, `tag_value`) pairs for this project
    tag_pairs: HashSet<(String, String)>,
}

/// In-memory cardinality tracker for enforcing limits.
#[derive(Debug)]
pub struct CardinalityTracker {
    config: LimitsConfig,
    /// Per-run tracking
    runs: RwLock<HashMap<String, RunCardinality>>,
    /// Per-project tracking
    projects: RwLock<HashMap<String, ProjectCardinality>>,
}

impl Default for CardinalityTracker {
    fn default() -> Self {
        Self::new(LimitsConfig::default())
    }
}

impl CardinalityTracker {
    /// Create a new cardinality tracker with the given config.
    pub fn new(config: LimitsConfig) -> Self {
        Self {
            config,
            runs: RwLock::new(HashMap::new()),
            projects: RwLock::new(HashMap::new()),
        }
    }

    /// Create a tracker from environment configuration.
    pub fn from_env() -> Self {
        Self::new(LimitsConfig::from_env())
    }

    /// Get the limits configuration.
    pub const fn config(&self) -> &LimitsConfig {
        &self.config
    }

    /// Validate and filter a batch of tags and metrics.
    ///
    /// Returns accepted items and warnings about dropped items.
    pub async fn validate_batch(
        &self,
        project_id: &str,
        run_id: &str,
        tags: &[(String, String)],
        metric_names: &[String],
    ) -> ValidationResult {
        let mut result = ValidationResult::default();

        // Get or create run tracking
        let mut runs = self.runs.write().await;
        let run = runs.entry(run_id.to_string()).or_default();

        // Get or create project tracking
        let mut projects = self.projects.write().await;
        let project = projects.entry(project_id.to_string()).or_default();

        // Validate tags
        for (key, value) in tags {
            // Check tag key length
            if key.len() > self.config.max_tag_key_length {
                result.warnings.push(format!(
                    "Tag key '{}...' exceeds max length {}",
                    &key[..32.min(key.len())],
                    self.config.max_tag_key_length
                ));
                result.dropped_tags.push((key.clone(), value.clone()));
                continue;
            }

            // Check tag value length
            if value.len() > self.config.max_tag_value_length {
                result.warnings.push(format!(
                    "Tag value for '{}' exceeds max length {}",
                    key, self.config.max_tag_value_length
                ));
                result.dropped_tags.push((key.clone(), value.clone()));
                continue;
            }

            // Check run tag key limit (only for new keys)
            if !run.tag_keys.contains(key) && run.tag_keys.len() >= self.config.max_tag_keys_per_run
            {
                if result.dropped_tags.is_empty() {
                    result.warnings.push(format!(
                        "Run {} has reached max tag keys ({})",
                        run_id, self.config.max_tag_keys_per_run
                    ));
                }
                result.dropped_tags.push((key.clone(), value.clone()));
                continue;
            }

            // Check project tag limit (only for new pairs)
            let pair = (key.clone(), value.clone());
            if !project.tag_pairs.contains(&pair)
                && project.tag_pairs.len() >= self.config.max_tags_per_project
            {
                if result.dropped_tags.is_empty() {
                    result.warnings.push(format!(
                        "Project {} has reached max tags ({})",
                        project_id, self.config.max_tags_per_project
                    ));
                }
                result.dropped_tags.push((key.clone(), value.clone()));
                continue;
            }

            // Accept the tag
            run.tag_keys.insert(key.clone());
            project.tag_pairs.insert(pair.clone());
            result.accepted_tags.push((key.clone(), value.clone()));
        }

        // Validate metrics
        for name in metric_names {
            // Check metric name length
            if name.len() > self.config.max_metric_name_length {
                result.warnings.push(format!(
                    "Metric name '{}...' exceeds max length {}",
                    &name[..32.min(name.len())],
                    self.config.max_metric_name_length
                ));
                result.dropped_metrics.push(name.clone());
                continue;
            }

            // Check run metric name limit (only for new names)
            if !run.metric_names.contains(name)
                && run.metric_names.len() >= self.config.max_metric_names_per_run
            {
                if result.dropped_metrics.is_empty() {
                    result.warnings.push(format!(
                        "Run {} has reached max metric names ({})",
                        run_id, self.config.max_metric_names_per_run
                    ));
                }
                result.dropped_metrics.push(name.clone());
                continue;
            }

            // Accept the metric
            run.metric_names.insert(name.clone());
            result.accepted_metrics.push(name.clone());
        }

        // Log if anything was dropped
        if result.has_drops() {
            warn!(
                project_id = %project_id,
                run_id = %run_id,
                dropped_tags = result.dropped_tags.len(),
                dropped_metrics = result.dropped_metrics.len(),
                "Cardinality limits exceeded, items dropped"
            );
        } else {
            debug!(
                project_id = %project_id,
                run_id = %run_id,
                tags = result.accepted_tags.len(),
                metrics = result.accepted_metrics.len(),
                "Batch validated successfully"
            );
        }

        result
    }

    /// Get current cardinality stats for a run.
    pub async fn get_run_stats(&self, run_id: &str) -> Option<(usize, usize)> {
        let runs = self.runs.read().await;
        runs.get(run_id)
            .map(|r| (r.tag_keys.len(), r.metric_names.len()))
    }

    /// Get current cardinality stats for a project.
    pub async fn get_project_stats(&self, project_id: &str) -> Option<usize> {
        let projects = self.projects.read().await;
        projects.get(project_id).map(|p| p.tag_pairs.len())
    }

    /// Clear tracking for a run (e.g., when run finishes).
    pub async fn clear_run(&self, run_id: &str) {
        let mut runs = self.runs.write().await;
        runs.remove(run_id);
    }

    /// Clear all tracking (useful for testing).
    pub async fn clear_all(&self) {
        let mut runs = self.runs.write().await;
        runs.clear();

        let mut projects = self.projects.write().await;
        projects.clear();
    }
}

/// Shared cardinality tracker type.
pub type SharedCardinalityTracker = Arc<CardinalityTracker>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = LimitsConfig::default();
        assert_eq!(config.max_tag_keys_per_run, 100);
        assert_eq!(config.max_metric_names_per_run, 1000);
        assert_eq!(config.max_tags_per_project, 10000);
    }

    #[tokio::test]
    async fn test_validate_within_limits() {
        let tracker = CardinalityTracker::default();

        let tags = vec![
            ("env".to_string(), "prod".to_string()),
            ("version".to_string(), "1.0".to_string()),
        ];
        let metrics = vec!["loss".to_string(), "accuracy".to_string()];

        let result = tracker.validate_batch("proj", "run", &tags, &metrics).await;

        assert_eq!(result.accepted_tags.len(), 2);
        assert_eq!(result.accepted_metrics.len(), 2);
        assert!(result.dropped_tags.is_empty());
        assert!(result.dropped_metrics.is_empty());
        assert!(!result.has_drops());
    }

    #[tokio::test]
    async fn test_tag_key_limit() {
        let config = LimitsConfig {
            max_tag_keys_per_run: 2,
            ..Default::default()
        };
        let tracker = CardinalityTracker::new(config);

        let tags = vec![
            ("a".to_string(), "1".to_string()),
            ("b".to_string(), "2".to_string()),
            ("c".to_string(), "3".to_string()), // Should be dropped
        ];

        let result = tracker.validate_batch("proj", "run", &tags, &[]).await;

        assert_eq!(result.accepted_tags.len(), 2);
        assert_eq!(result.dropped_tags.len(), 1);
        assert!(result.has_drops());
        assert!(result.warnings.iter().any(|w| w.contains("max tag keys")));
    }

    #[tokio::test]
    async fn test_metric_name_limit() {
        let config = LimitsConfig {
            max_metric_names_per_run: 2,
            ..Default::default()
        };
        let tracker = CardinalityTracker::new(config);

        let metrics = vec![
            "a".to_string(),
            "b".to_string(),
            "c".to_string(), // Should be dropped
        ];

        let result = tracker.validate_batch("proj", "run", &[], &metrics).await;

        assert_eq!(result.accepted_metrics.len(), 2);
        assert_eq!(result.dropped_metrics.len(), 1);
        assert!(result.has_drops());
        assert!(
            result
                .warnings
                .iter()
                .any(|w| w.contains("max metric names"))
        );
    }

    #[tokio::test]
    async fn test_project_tag_limit() {
        let config = LimitsConfig {
            max_tags_per_project: 2,
            ..Default::default()
        };
        let tracker = CardinalityTracker::new(config);

        // First batch fills the project limit
        let tags1 = vec![
            ("a".to_string(), "1".to_string()),
            ("b".to_string(), "2".to_string()),
        ];
        tracker.validate_batch("proj", "run1", &tags1, &[]).await;

        // Second batch should be dropped (different run, same project)
        let tags2 = vec![("c".to_string(), "3".to_string())];
        let result = tracker.validate_batch("proj", "run2", &tags2, &[]).await;

        assert_eq!(result.dropped_tags.len(), 1);
        assert!(result.warnings.iter().any(|w| w.contains("max tags")));
    }

    #[tokio::test]
    async fn test_duplicate_tags_not_counted() {
        let config = LimitsConfig {
            max_tag_keys_per_run: 2,
            ..Default::default()
        };
        let tracker = CardinalityTracker::new(config);

        // First batch
        let tags1 = vec![
            ("a".to_string(), "1".to_string()),
            ("b".to_string(), "2".to_string()),
        ];
        tracker.validate_batch("proj", "run", &tags1, &[]).await;

        // Second batch with duplicates (should not count toward limit)
        let tags2 = vec![
            ("a".to_string(), "1".to_string()), // Duplicate
            ("b".to_string(), "3".to_string()), // Same key, different value
        ];
        let result = tracker.validate_batch("proj", "run", &tags2, &[]).await;

        // Both should be accepted since keys already exist
        assert_eq!(result.accepted_tags.len(), 2);
        assert!(result.dropped_tags.is_empty());
    }

    #[tokio::test]
    async fn test_tag_key_length_limit() {
        let config = LimitsConfig {
            max_tag_key_length: 5,
            ..Default::default()
        };
        let tracker = CardinalityTracker::new(config);

        let tags = vec![
            ("short".to_string(), "val".to_string()),
            ("toolong".to_string(), "val".to_string()), // Should be dropped
        ];

        let result = tracker.validate_batch("proj", "run", &tags, &[]).await;

        assert_eq!(result.accepted_tags.len(), 1);
        assert_eq!(result.dropped_tags.len(), 1);
        assert!(
            result
                .warnings
                .iter()
                .any(|w| w.contains("exceeds max length"))
        );
    }

    #[tokio::test]
    async fn test_get_stats() {
        let tracker = CardinalityTracker::default();

        let tags = vec![
            ("a".to_string(), "1".to_string()),
            ("b".to_string(), "2".to_string()),
        ];
        let metrics = vec!["loss".to_string(), "acc".to_string(), "lr".to_string()];

        tracker.validate_batch("proj", "run", &tags, &metrics).await;

        let (tag_count, metric_count) = tracker.get_run_stats("run").await.unwrap();
        assert_eq!(tag_count, 2);
        assert_eq!(metric_count, 3);

        let project_tags = tracker.get_project_stats("proj").await.unwrap();
        assert_eq!(project_tags, 2);
    }

    #[tokio::test]
    async fn test_clear_run() {
        let tracker = CardinalityTracker::default();

        let tags = vec![("a".to_string(), "1".to_string())];
        tracker.validate_batch("proj", "run", &tags, &[]).await;

        assert!(tracker.get_run_stats("run").await.is_some());

        tracker.clear_run("run").await;

        assert!(tracker.get_run_stats("run").await.is_none());
    }

    #[test]
    fn test_validation_result_summary() {
        let mut result = ValidationResult::default();
        assert!(result.summary_warning().is_none());

        result.dropped_tags.push(("a".to_string(), "1".to_string()));
        result.dropped_metrics.push("m1".to_string());

        let warning = result.summary_warning().unwrap();
        assert!(warning.contains("1 tags dropped"));
        assert!(warning.contains("1 metrics dropped"));
    }
}
