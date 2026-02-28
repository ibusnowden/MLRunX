//! Metrics Query Service
//!
//! Provides endpoints for querying and downsampling metric data.
//! See: API-003 for specification.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// A single metric data point.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricPoint {
    pub name: String,
    pub step: i64,
    pub value: f64,
    pub timestamp: Option<f64>,
}

/// Aggregated metric point (result of downsampling).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregatedPoint {
    /// Step or bucket index
    pub step: i64,
    /// Mean value in the bucket
    pub mean: f64,
    /// Minimum value in the bucket
    pub min: f64,
    /// Maximum value in the bucket
    pub max: f64,
    /// Number of points in the bucket
    pub count: usize,
}

/// A time series for a single metric.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricSeries {
    pub name: String,
    pub points: Vec<AggregatedPoint>,
    /// Total points before downsampling
    pub total_points: usize,
    /// Whether data was downsampled
    pub downsampled: bool,
}

/// Request for querying metrics.
#[derive(Debug, Deserialize)]
pub struct MetricsQueryRequest {
    /// Metric names to query (empty = all)
    #[serde(default)]
    pub names: Vec<String>,
    /// Maximum points to return per metric (triggers downsampling)
    #[serde(default = "default_max_points")]
    pub max_points: usize,
    /// Start step (inclusive)
    pub start_step: Option<i64>,
    /// End step (inclusive)
    pub end_step: Option<i64>,
}

const fn default_max_points() -> usize {
    1000
}

/// Response containing metric series.
#[derive(Debug, Serialize)]
pub struct MetricsQueryResponse {
    pub run_id: String,
    pub series: Vec<MetricSeries>,
    /// Available metric names for this run
    pub available_metrics: Vec<String>,
    /// Optional project alias map keyed by raw metric name.
    #[serde(default)]
    pub metric_aliases: HashMap<String, String>,
}

/// Downsample a series of points to a target number of points.
///
/// Uses bucket aggregation: divides the step range into N buckets
/// and computes mean/min/max for each bucket.
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
pub fn downsample_points(points: &[MetricPoint], max_points: usize) -> Vec<AggregatedPoint> {
    if points.is_empty() {
        return vec![];
    }

    // If already under limit, just convert
    if points.len() <= max_points {
        return points
            .iter()
            .map(|p| AggregatedPoint {
                step: p.step,
                mean: p.value,
                min: p.value,
                max: p.value,
                count: 1,
            })
            .collect();
    }

    // Find step range
    let min_step = points.iter().map(|p| p.step).min().unwrap_or(0);
    let max_step = points.iter().map(|p| p.step).max().unwrap_or(0);
    let step_range = (max_step - min_step).max(1) as f64;

    // Calculate bucket size
    let bucket_count = max_points;
    let bucket_size = step_range / bucket_count as f64;

    // Group points into buckets
    let mut buckets: HashMap<usize, Vec<f64>> = HashMap::new();

    for point in points {
        let bucket_idx = ((point.step - min_step) as f64 / bucket_size).floor() as usize;
        let bucket_idx = bucket_idx.min(bucket_count - 1); // Clamp to last bucket
        buckets.entry(bucket_idx).or_default().push(point.value);
    }

    // Aggregate each bucket
    let mut result: Vec<AggregatedPoint> = buckets
        .into_iter()
        .map(|(bucket_idx, values)| {
            let sum: f64 = values.iter().sum();
            let min = values.iter().copied().fold(f64::INFINITY, f64::min);
            let max = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            let count = values.len();

            AggregatedPoint {
                step: min_step + (bucket_idx as f64 * bucket_size) as i64,
                mean: sum / count as f64,
                min,
                max,
                count,
            }
        })
        .collect();

    // Sort by step
    result.sort_by_key(|p| p.step);
    result
}

/// In-memory metric storage for a run.
#[derive(Debug, Default, Clone)]
pub struct RunMetrics {
    /// Metrics grouped by name
    pub metrics: HashMap<String, Vec<MetricPoint>>,
}

impl RunMetrics {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a metric point.
    pub fn add_point(&mut self, point: MetricPoint) {
        self.metrics
            .entry(point.name.clone())
            .or_default()
            .push(point);
    }

    /// Get all metric names.
    pub fn metric_names(&self) -> Vec<String> {
        let mut names: Vec<_> = self.metrics.keys().cloned().collect();
        names.sort();
        names
    }

    /// Query metrics with optional downsampling.
    pub fn query(
        &self,
        names: &[String],
        max_points: usize,
        start_step: Option<i64>,
        end_step: Option<i64>,
    ) -> Vec<MetricSeries> {
        let query_names: Vec<&String> = if names.is_empty() {
            self.metrics.keys().collect()
        } else {
            names
                .iter()
                .filter(|n| self.metrics.contains_key(*n))
                .collect()
        };

        query_names
            .into_iter()
            .map(|name| {
                let points = self.metrics.get(name).cloned().unwrap_or_default();

                // Filter by step range
                let filtered: Vec<MetricPoint> = points
                    .into_iter()
                    .filter(|p| {
                        start_step.is_none_or(|s| p.step >= s)
                            && end_step.is_none_or(|e| p.step <= e)
                    })
                    .collect();

                let total_points = filtered.len();
                let downsampled = total_points > max_points;
                let aggregated = downsample_points(&filtered, max_points);

                MetricSeries {
                    name: name.clone(),
                    points: aggregated,
                    total_points,
                    downsampled,
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_downsample_empty() {
        let points: Vec<MetricPoint> = vec![];
        let result = downsample_points(&points, 10);
        assert!(result.is_empty());
    }

    #[test]
    fn test_downsample_under_limit() {
        let points: Vec<MetricPoint> = (0..5)
            .map(|i| MetricPoint {
                name: "loss".to_string(),
                step: i,
                value: i as f64 * 0.1,
                timestamp: None,
            })
            .collect();

        let result = downsample_points(&points, 10);
        assert_eq!(result.len(), 5);
        assert_eq!(result[0].step, 0);
        assert!((result[0].mean - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_downsample_over_limit() {
        // Create 100 points
        let points: Vec<MetricPoint> = (0..100)
            .map(|i| MetricPoint {
                name: "loss".to_string(),
                step: i,
                value: i as f64,
                timestamp: None,
            })
            .collect();

        // Downsample to 10 points
        let result = downsample_points(&points, 10);
        assert_eq!(result.len(), 10);

        // Check that buckets are reasonable
        for point in &result {
            assert!(point.count >= 1);
            assert!(point.min <= point.mean);
            assert!(point.mean <= point.max);
        }
    }

    #[test]
    fn test_run_metrics_query() {
        let mut metrics = RunMetrics::new();

        // Add some points
        for i in 0..50 {
            metrics.add_point(MetricPoint {
                name: "loss".to_string(),
                step: i,
                value: 1.0 - (i as f64 * 0.01),
                timestamp: None,
            });
            metrics.add_point(MetricPoint {
                name: "accuracy".to_string(),
                step: i,
                value: i as f64 * 0.02,
                timestamp: None,
            });
        }

        // Query all metrics
        let series = metrics.query(&[], 100, None, None);
        assert_eq!(series.len(), 2);

        // Query specific metric
        let series = metrics.query(&["loss".to_string()], 100, None, None);
        assert_eq!(series.len(), 1);
        assert_eq!(series[0].name, "loss");

        // Query with step range
        let series = metrics.query(&["loss".to_string()], 100, Some(10), Some(20));
        assert_eq!(series.len(), 1);
        assert_eq!(series[0].total_points, 11); // steps 10-20 inclusive
    }

    #[test]
    fn test_metric_names() {
        let mut metrics = RunMetrics::new();
        metrics.add_point(MetricPoint {
            name: "loss".to_string(),
            step: 0,
            value: 0.5,
            timestamp: None,
        });
        metrics.add_point(MetricPoint {
            name: "accuracy".to_string(),
            step: 0,
            value: 0.8,
            timestamp: None,
        });

        let names = metrics.metric_names();
        assert_eq!(names, vec!["accuracy", "loss"]);
    }
}
