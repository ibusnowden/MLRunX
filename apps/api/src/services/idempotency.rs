//! Idempotency service for batch deduplication.
//!
//! Ensures that SDK retries don't create duplicate data by:
//! 1. Tracking batch_id per run
//! 2. Verifying payload hash matches for duplicates
//! 3. Rejecting conflicting batches (same batch_id, different payload)

use std::collections::HashMap;
use std::sync::Arc;

use sha2::{Digest, Sha256};
use tokio::sync::RwLock;
use tracing::{debug, warn};

/// Result of checking batch idempotency.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdempotencyResult {
    /// Batch is new, should be processed
    New,
    /// Batch is a duplicate (same batch_id, same payload hash)
    Duplicate,
    /// Batch conflicts (same batch_id, different payload hash)
    Conflict {
        expected_hash: String,
        actual_hash: String,
    },
    /// Sequence is out of order but accepted
    OutOfOrder { expected_seq: i64, actual_seq: i64 },
}

impl IdempotencyResult {
    /// Returns true if the batch should be processed.
    pub fn should_process(&self) -> bool {
        matches!(self, Self::New | Self::OutOfOrder { .. })
    }

    /// Returns true if this is an error condition.
    pub fn is_error(&self) -> bool {
        matches!(self, Self::Conflict { .. })
    }
}

/// Record of an ingested batch.
#[derive(Debug, Clone)]
pub struct BatchRecord {
    /// Project ID
    pub project_id: String,
    /// Run ID
    pub run_id: String,
    /// Batch ID (from SDK)
    pub batch_id: String,
    /// Sequence number
    pub seq: i64,
    /// SHA-256 hash of payload
    pub payload_hash: String,
    /// Number of metrics in batch
    pub metric_count: i32,
    /// Number of params in batch
    pub param_count: i32,
    /// Number of tags in batch
    pub tag_count: i32,
    /// Number of log events in batch
    pub event_count: i32,
    /// When the batch was received
    pub created_at: std::time::SystemTime,
}

/// In-memory idempotency store for alpha development.
/// In production, this would be backed by PostgreSQL.
#[derive(Debug, Default)]
pub struct IdempotencyStore {
    /// Map from (run_id, batch_id) to BatchRecord
    batches: RwLock<HashMap<(String, String), BatchRecord>>,
    /// Map from run_id to highest seen sequence number
    sequences: RwLock<HashMap<String, i64>>,
}

impl IdempotencyStore {
    /// Create a new idempotency store.
    pub fn new() -> Self {
        Self {
            batches: RwLock::new(HashMap::new()),
            sequences: RwLock::new(HashMap::new()),
        }
    }

    /// Check if a batch is idempotent and record it if new.
    ///
    /// Returns the idempotency result indicating whether to process the batch.
    pub async fn check_and_record(
        &self,
        project_id: &str,
        run_id: &str,
        batch_id: &str,
        seq: i64,
        payload_hash: &str,
        metric_count: i32,
        param_count: i32,
        tag_count: i32,
        event_count: i32,
    ) -> IdempotencyResult {
        let key = (run_id.to_string(), batch_id.to_string());

        // First check if batch already exists
        {
            let batches = self.batches.read().await;
            if let Some(existing) = batches.get(&key) {
                if existing.payload_hash == payload_hash {
                    debug!(
                        run_id = %run_id,
                        batch_id = %batch_id,
                        "Duplicate batch detected (same hash)"
                    );
                    return IdempotencyResult::Duplicate;
                } else {
                    warn!(
                        run_id = %run_id,
                        batch_id = %batch_id,
                        expected_hash = %existing.payload_hash,
                        actual_hash = %payload_hash,
                        "Conflicting batch detected (different hash)"
                    );
                    return IdempotencyResult::Conflict {
                        expected_hash: existing.payload_hash.clone(),
                        actual_hash: payload_hash.to_string(),
                    };
                }
            }
        }

        // Check sequence ordering
        let seq_result = {
            let mut sequences = self.sequences.write().await;
            let current_seq = *sequences.get(run_id).unwrap_or(&0);

            if seq > 0 && seq < current_seq {
                // Out of order but we accept it (could be from offline spool)
                debug!(
                    run_id = %run_id,
                    batch_id = %batch_id,
                    expected_seq = current_seq,
                    actual_seq = seq,
                    "Out of order batch accepted"
                );
                Some(IdempotencyResult::OutOfOrder {
                    expected_seq: current_seq,
                    actual_seq: seq,
                })
            } else {
                // Update sequence if this is a new highest
                if seq > current_seq {
                    sequences.insert(run_id.to_string(), seq);
                }
                None
            }
        };

        // Record the new batch
        {
            let mut batches = self.batches.write().await;
            let record = BatchRecord {
                project_id: project_id.to_string(),
                run_id: run_id.to_string(),
                batch_id: batch_id.to_string(),
                seq,
                payload_hash: payload_hash.to_string(),
                metric_count,
                param_count,
                tag_count,
                event_count,
                created_at: std::time::SystemTime::now(),
            };
            batches.insert(key, record);
        }

        // Return sequence result or New
        seq_result.unwrap_or(IdempotencyResult::New)
    }

    /// Check idempotency without recording.
    pub async fn check_only(
        &self,
        run_id: &str,
        batch_id: &str,
        payload_hash: &str,
    ) -> IdempotencyResult {
        let key = (run_id.to_string(), batch_id.to_string());
        let batches = self.batches.read().await;

        if let Some(existing) = batches.get(&key) {
            if existing.payload_hash == payload_hash {
                IdempotencyResult::Duplicate
            } else {
                IdempotencyResult::Conflict {
                    expected_hash: existing.payload_hash.clone(),
                    actual_hash: payload_hash.to_string(),
                }
            }
        } else {
            IdempotencyResult::New
        }
    }

    /// Get batch record if it exists.
    pub async fn get_batch(&self, run_id: &str, batch_id: &str) -> Option<BatchRecord> {
        let key = (run_id.to_string(), batch_id.to_string());
        let batches = self.batches.read().await;
        batches.get(&key).cloned()
    }

    /// Get the highest sequence number seen for a run.
    pub async fn get_sequence(&self, run_id: &str) -> i64 {
        let sequences = self.sequences.read().await;
        *sequences.get(run_id).unwrap_or(&0)
    }

    /// Get all batches for a run.
    pub async fn get_batches_for_run(&self, run_id: &str) -> Vec<BatchRecord> {
        let batches = self.batches.read().await;
        batches
            .values()
            .filter(|b| b.run_id == run_id)
            .cloned()
            .collect()
    }

    /// Clear all batches for a run (useful for testing or cleanup).
    pub async fn clear_run(&self, run_id: &str) {
        let mut batches = self.batches.write().await;
        batches.retain(|k, _| k.0 != run_id);

        let mut sequences = self.sequences.write().await;
        sequences.remove(run_id);
    }
}

/// Compute SHA-256 hash of a batch payload.
///
/// Uses stable serialization to ensure consistent hashing.
pub fn compute_payload_hash(
    metrics: &[MetricPayload],
    params: &[ParamPayload],
    tags: &[TagPayload],
    events: &[EventPayload],
) -> String {
    let mut hasher = Sha256::new();

    // Hash metrics (sorted by name, then step for determinism)
    let mut sorted_metrics: Vec<_> = metrics.iter().collect();
    sorted_metrics.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.step.cmp(&b.step)));

    for m in sorted_metrics {
        hasher.update(m.name.as_bytes());
        hasher.update(m.step.to_le_bytes());
        hasher.update(m.value.to_le_bytes());
    }

    // Hash params (sorted by name)
    let mut sorted_params: Vec<_> = params.iter().collect();
    sorted_params.sort_by(|a, b| a.name.cmp(&b.name));

    for p in sorted_params {
        hasher.update(p.name.as_bytes());
        hasher.update(p.value.as_bytes());
    }

    // Hash tags (sorted by key)
    let mut sorted_tags: Vec<_> = tags.iter().collect();
    sorted_tags.sort_by(|a, b| a.key.cmp(&b.key));

    for t in sorted_tags {
        hasher.update(t.key.as_bytes());
        hasher.update(t.value.as_bytes());
    }

    // Hash events in stable order.
    let mut sorted_events: Vec<_> = events.iter().collect();
    sorted_events.sort_by(|a, b| {
        a.step
            .cmp(&b.step)
            .then_with(|| a.level.cmp(&b.level))
            .then_with(|| a.source.cmp(&b.source))
            .then_with(|| a.message.cmp(&b.message))
            .then_with(|| {
                let a_bits = a.timestamp.map(f64::to_bits).unwrap_or_default();
                let b_bits = b.timestamp.map(f64::to_bits).unwrap_or_default();
                a_bits.cmp(&b_bits)
            })
    });

    for e in sorted_events {
        hasher.update(e.level.as_bytes());
        hasher.update(e.source.as_bytes());
        hasher.update(e.message.as_bytes());
        if let Some(step) = e.step {
            hasher.update(step.to_le_bytes());
        } else {
            hasher.update(0_i64.to_le_bytes());
        }
        if let Some(timestamp) = e.timestamp {
            hasher.update(timestamp.to_le_bytes());
        } else {
            hasher.update(0_f64.to_le_bytes());
        }
    }

    hex::encode(hasher.finalize())
}

/// Metric payload for hashing.
#[derive(Debug, Clone)]
pub struct MetricPayload {
    pub name: String,
    pub value: f64,
    pub step: i64,
}

/// Param payload for hashing.
#[derive(Debug, Clone)]
pub struct ParamPayload {
    pub name: String,
    pub value: String,
}

/// Tag payload for hashing.
#[derive(Debug, Clone)]
pub struct TagPayload {
    pub key: String,
    pub value: String,
}

/// Run event payload for hashing.
#[derive(Debug, Clone)]
pub struct EventPayload {
    pub level: String,
    pub source: String,
    pub message: String,
    pub step: Option<i64>,
    pub timestamp: Option<f64>,
}

/// Shared idempotency store type.
pub type SharedIdempotencyStore = Arc<IdempotencyStore>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_payload_hash() {
        let metrics = vec![
            MetricPayload {
                name: "loss".to_string(),
                value: 0.5,
                step: 1,
            },
            MetricPayload {
                name: "acc".to_string(),
                value: 0.9,
                step: 1,
            },
        ];
        let params = vec![ParamPayload {
            name: "lr".to_string(),
            value: "0.001".to_string(),
        }];
        let tags = vec![TagPayload {
            key: "env".to_string(),
            value: "dev".to_string(),
        }];

        let hash1 = compute_payload_hash(&metrics, &params, &tags, &[]);
        let hash2 = compute_payload_hash(&metrics, &params, &tags, &[]);

        // Same input should produce same hash
        assert_eq!(hash1, hash2);

        // Different input should produce different hash
        let metrics2 = vec![MetricPayload {
            name: "loss".to_string(),
            value: 0.6,
            step: 1,
        }];
        let hash3 = compute_payload_hash(&metrics2, &params, &tags, &[]);
        assert_ne!(hash1, hash3);
    }

    #[test]
    fn test_hash_order_independence() {
        // Metrics in different order should produce same hash
        let metrics1 = vec![
            MetricPayload {
                name: "b".to_string(),
                value: 1.0,
                step: 1,
            },
            MetricPayload {
                name: "a".to_string(),
                value: 2.0,
                step: 1,
            },
        ];
        let metrics2 = vec![
            MetricPayload {
                name: "a".to_string(),
                value: 2.0,
                step: 1,
            },
            MetricPayload {
                name: "b".to_string(),
                value: 1.0,
                step: 1,
            },
        ];

        let hash1 = compute_payload_hash(&metrics1, &[], &[], &[]);
        let hash2 = compute_payload_hash(&metrics2, &[], &[], &[]);

        assert_eq!(hash1, hash2);
    }

    #[tokio::test]
    async fn test_idempotency_new_batch() {
        let store = IdempotencyStore::new();

        let result = store
            .check_and_record("project-1", "run-1", "batch-1", 1, "hash123", 10, 5, 2, 1)
            .await;

        assert_eq!(result, IdempotencyResult::New);
        assert!(result.should_process());
        assert!(!result.is_error());
    }

    #[tokio::test]
    async fn test_idempotency_duplicate() {
        let store = IdempotencyStore::new();

        // First batch
        store
            .check_and_record("p", "r", "b", 1, "hash", 1, 0, 0, 0)
            .await;

        // Same batch again (same hash)
        let result = store
            .check_and_record("p", "r", "b", 1, "hash", 1, 0, 0, 0)
            .await;

        assert_eq!(result, IdempotencyResult::Duplicate);
        assert!(!result.should_process());
        assert!(!result.is_error());
    }

    #[tokio::test]
    async fn test_idempotency_conflict() {
        let store = IdempotencyStore::new();

        // First batch
        store
            .check_and_record("p", "r", "b", 1, "hash1", 1, 0, 0, 0)
            .await;

        // Same batch_id but different hash (conflict!)
        let result = store
            .check_and_record("p", "r", "b", 1, "hash2", 1, 0, 0, 0)
            .await;

        assert!(matches!(result, IdempotencyResult::Conflict { .. }));
        assert!(!result.should_process());
        assert!(result.is_error());
    }

    #[tokio::test]
    async fn test_sequence_tracking() {
        let store = IdempotencyStore::new();

        // Batch with seq 1
        store
            .check_and_record("p", "r", "b1", 1, "h1", 1, 0, 0, 0)
            .await;
        assert_eq!(store.get_sequence("r").await, 1);

        // Batch with seq 5
        store
            .check_and_record("p", "r", "b2", 5, "h2", 1, 0, 0, 0)
            .await;
        assert_eq!(store.get_sequence("r").await, 5);

        // Batch with seq 3 (out of order)
        let result = store
            .check_and_record("p", "r", "b3", 3, "h3", 1, 0, 0, 0)
            .await;
        assert!(matches!(result, IdempotencyResult::OutOfOrder { .. }));
        // Sequence should still be 5
        assert_eq!(store.get_sequence("r").await, 5);
    }

    #[tokio::test]
    async fn test_get_batch() {
        let store = IdempotencyStore::new();

        store
            .check_and_record("proj", "run", "batch", 1, "hash", 10, 5, 2, 3)
            .await;

        let batch = store.get_batch("run", "batch").await.unwrap();
        assert_eq!(batch.project_id, "proj");
        assert_eq!(batch.run_id, "run");
        assert_eq!(batch.batch_id, "batch");
        assert_eq!(batch.metric_count, 10);
        assert_eq!(batch.param_count, 5);
        assert_eq!(batch.tag_count, 2);
        assert_eq!(batch.event_count, 3);

        // Non-existent batch
        assert!(store.get_batch("run", "nonexistent").await.is_none());
    }

    #[tokio::test]
    async fn test_clear_run() {
        let store = IdempotencyStore::new();

        store
            .check_and_record("p", "r1", "b1", 1, "h1", 1, 0, 0, 0)
            .await;
        store
            .check_and_record("p", "r1", "b2", 2, "h2", 1, 0, 0, 0)
            .await;
        store
            .check_and_record("p", "r2", "b1", 1, "h3", 1, 0, 0, 0)
            .await;

        // Clear run1
        store.clear_run("r1").await;

        // Run1 batches should be gone
        assert!(store.get_batch("r1", "b1").await.is_none());
        assert!(store.get_batch("r1", "b2").await.is_none());
        assert_eq!(store.get_sequence("r1").await, 0);

        // Run2 batches should remain
        assert!(store.get_batch("r2", "b1").await.is_some());
    }
}
