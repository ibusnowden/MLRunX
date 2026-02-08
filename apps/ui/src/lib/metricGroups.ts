/**
 * Metric grouping logic for the research dashboard.
 *
 * Groups metrics by category based on naming patterns and provides
 * chart configuration defaults for different metric types.
 */

export interface MetricGroupConfig {
  /** Display name for the section */
  title: string;
  /** Patterns to match metric names (supports * wildcard) */
  patterns: string[];
  /** Default chart settings for this group */
  chartDefaults?: {
    logScale?: boolean;
    areaFill?: boolean;
    height?: number;
  };
  /** Order in the dashboard (lower = higher) */
  order: number;
}

export interface ChartConfig {
  /** Chart title */
  title: string;
  /** Metric patterns to include (exact names or patterns with *) */
  metrics: string[];
  /** Use log scale for Y-axis */
  logScale?: boolean;
  /** Chart type */
  chartType?: 'line' | 'area';
  /** Y-axis label */
  yLabel?: string;
  /** Group related metrics on single chart */
  groupOnChart?: boolean;
}

// Metric group definitions ordered by importance
export const METRIC_GROUPS: Record<string, MetricGroupConfig> = {
  loss: {
    title: 'Loss Metrics',
    patterns: ['loss/*', 'loss', '*_loss', 'epoch/*_loss', 'train_loss', 'val_loss', 'test_loss', '*cross_entropy*'],
    chartDefaults: {
      logScale: true,
    },
    order: 1,
  },
  debugging: {
    title: 'Debugging Metrics',
    patterns: [
      'grad_norm/*', 'grad_norm', 'gradient/*', '*_grad',
      'activation/*', 'activation_*',
    ],
    chartDefaults: {
      logScale: false,
    },
    order: 2,
  },
  evaluation: {
    title: 'Evaluation Metrics',
    patterns: ['entropy/*', '*_entropy', '*_acc', '*_accuracy', 'accuracy', 'f1', '*_f1', 'precision', 'recall', 'auc', 'epoch/*_acc', 'train_acc', 'test_acc'],
    chartDefaults: {
      logScale: false,
    },
    order: 3,
  },
  weight: {
    title: 'Weight Metrics',
    patterns: ['weight/*', 'singular/*', 'weight_norm/*', '*_weight', 'param/*'],
    chartDefaults: {
      logScale: false,
    },
    order: 4,
  },
  learning_rate: {
    title: 'Learning Rate',
    patterns: ['lr', 'learning_rate', 'lr/*'],
    chartDefaults: {
      logScale: true,
    },
    order: 5,
  },
  timing: {
    title: 'Timing Metrics',
    patterns: ['time/*', '*_time', 'throughput', 'samples_per_sec', 'steps_per_sec', 'iteration_time'],
    chartDefaults: {
      logScale: false,
    },
    order: 6,
  },
  system: {
    title: 'System Metrics',
    patterns: [
      'gpu/*', 'gpu_*', '*_gpu',
      'cpu/*', 'cpu_*', 'cpu_utilization',
      'memory/*', 'memory_*', '*_memory',
      'disk/*', 'disk_*', '*_disk',
      'network/*', 'network_*', '*_network',
      'system/*', 'device/*',
      'io/*', '*_io',
    ],
    chartDefaults: {
      logScale: false,
    },
    order: 7,
  },
  other: {
    title: 'Other Metrics',
    patterns: ['*'],  // Catch-all
    chartDefaults: {
      logScale: false,
    },
    order: 99,
  },
};

/**
 * Check if a metric name matches a pattern.
 * Supports * as wildcard for any characters.
 */
function matchesPattern(metricName: string, pattern: string): boolean {
  // Exact match
  if (pattern === metricName) return true;

  // Wildcard matching
  if (pattern.includes('*')) {
    // Convert pattern to regex
    // Escape special regex chars except *
    const regexPattern = pattern
      .replace(/[.+?^${}()|[\]\\]/g, '\\$&')
      .replace(/\*/g, '.*');
    const regex = new RegExp(`^${regexPattern}$`);
    return regex.test(metricName);
  }

  return false;
}

/**
 * Find the best matching group for a metric name.
 */
function findGroupForMetric(metricName: string): string {
  for (const [groupKey, config] of Object.entries(METRIC_GROUPS)) {
    // Skip catch-all 'other' group in first pass
    if (groupKey === 'other') continue;

    for (const pattern of config.patterns) {
      if (matchesPattern(metricName, pattern)) {
        return groupKey;
      }
    }
  }

  return 'other';
}

export interface GroupedMetrics {
  groupKey: string;
  title: string;
  metrics: string[];
  chartDefaults?: MetricGroupConfig['chartDefaults'];
  order: number;
}

/**
 * Group a list of metric names into categories.
 */
export function groupMetrics(metricNames: string[]): GroupedMetrics[] {
  const groups: Record<string, string[]> = {};

  // Assign each metric to a group
  for (const name of metricNames) {
    const groupKey = findGroupForMetric(name);
    if (!groups[groupKey]) {
      groups[groupKey] = [];
    }
    groups[groupKey].push(name);
  }

  // Convert to array with config
  const result: GroupedMetrics[] = [];
  for (const [groupKey, metrics] of Object.entries(groups)) {
    const config = METRIC_GROUPS[groupKey];
    if (config) {
      result.push({
        groupKey,
        title: config.title,
        metrics: metrics.sort(),
        chartDefaults: config.chartDefaults,
        order: config.order,
      });
    }
  }

  // Sort by order
  return result.sort((a, b) => a.order - b.order);
}

/**
 * Filter metrics by a search query.
 * Matches against metric name (case-insensitive).
 */
export function filterMetrics(metricNames: string[], query: string): string[] {
  if (!query.trim()) return metricNames;

  const lowerQuery = query.toLowerCase();
  return metricNames.filter((name) =>
    name.toLowerCase().includes(lowerQuery)
  );
}

/**
 * Get related metrics that should be displayed on the same chart.
 * Groups metrics by their prefix (e.g., grad_norm/fc1, grad_norm/fc2 -> grad_norm).
 */
export function getRelatedMetrics(
  metricName: string,
  allMetrics: string[]
): string[] {
  // Check if metric has a prefix (e.g., grad_norm/fc1)
  const parts = metricName.split('/');
  if (parts.length < 2) {
    return [metricName];
  }

  const prefix = parts[0];
  return allMetrics.filter((m) => m.startsWith(prefix + '/'));
}

/**
 * Determine if a metric should use log scale based on its name.
 */
export function shouldUseLogScale(metricName: string): boolean {
  const logScalePatterns = [
    'loss',
    'entropy',
    'lr',
    'learning_rate',
    'perplexity',
  ];

  const lowerName = metricName.toLowerCase();
  return logScalePatterns.some((pattern) => lowerName.includes(pattern));
}

/**
 * GPU metric sub-group title mappings.
 * Keys are the leaf metric names (after gpu/{id}/...).
 */
const GPU_METRIC_TITLES: Record<string, string> = {
  'memory_used_percent': 'GPU Memory Utilization (%)',
  'memory_allocated_gb': 'GPU Memory Allocated (GB)',
  'memory_reserved_gb': 'GPU Memory Reserved (GB)',
  'memory_total_gb': 'GPU Memory Total (GB)',
  'memory_max_allocated_gb': 'GPU Peak Memory Allocated (GB)',
  'memory_max_reserved_gb': 'GPU Peak Memory Reserved (GB)',
  'utilization_percent': 'GPU Compute Utilization (%)',
  'power_w': 'GPU Power (W)',
  'temperature_c': 'GPU Temperature (°C)',
};

/**
 * Get a display-friendly title for a metric or group key.
 */
export function getMetricDisplayTitle(metricName: string): string {
  // Handle special group keys from smart grouping
  const specialTitles: Record<string, string> = {
    'grad_norm_global': 'Gradient Norm (Global)',
    'grad_norm_layers': 'Gradient Norm (Per Layer)',
    'activation': 'Activations',
    'gpu': 'GPU Metrics',
    'cpu': 'CPU Utilization',
    'memory': 'Memory Usage (GB)',
    'disk': 'Disk I/O (Mbps)',
    'network': 'Network I/O (Mbps)',
  };

  if (specialTitles[metricName]) {
    return specialTitles[metricName];
  }

  // Handle GPU sub-group keys like "gpu_memory_used_percent"
  if (metricName.startsWith('gpu_')) {
    const metricType = metricName.slice(4); // strip "gpu_"
    if (GPU_METRIC_TITLES[metricType]) {
      return GPU_METRIC_TITLES[metricType];
    }
    // Fallback: generate a readable title from the metric type
    return 'GPU ' + toTitleCase(metricType);
  }

  // Handle prefixed metrics like "grad_norm/fc1"
  const parts = metricName.split('/');
  if (parts.length > 1) {
    // Return "FC1 Gradient Norm" style
    const prefix = parts[0]
      .split('_')
      .map((w) => w.charAt(0).toUpperCase() + w.slice(1))
      .join(' ');
    const suffix = parts.slice(1).join('/').toUpperCase();
    return `${prefix} (${suffix})`;
  }

  // Handle underscored names like "train_loss"
  return metricName
    .split('_')
    .map((w) => w.charAt(0).toUpperCase() + w.slice(1))
    .join(' ');
}

function toTitleCase(value: string): string {
  return value
    .split('_')
    .map((word) => {
      const lower = word.toLowerCase();
      if (lower === 'io') return 'I/O';
      if (lower === 'cpu') return 'CPU';
      if (lower === 'gpu') return 'GPU';
      return word.charAt(0).toUpperCase() + word.slice(1);
    })
    .join(' ');
}

function formatMetricSegment(segment: string): string {
  const unitSuffixes: Array<{ suffix: string; unit: string }> = [
    { suffix: '_gb', unit: 'GB' },
    { suffix: '_mbps', unit: 'Mbps' },
    { suffix: '_w', unit: 'W' },
  ];

  const lower = segment.toLowerCase();
  for (const { suffix, unit } of unitSuffixes) {
    if (lower.endsWith(suffix)) {
      const base = segment.slice(0, -suffix.length);
      const baseLabel = base ? toTitleCase(base) : '';
      return baseLabel ? `${baseLabel} (${unit})` : unit;
    }
  }

  return toTitleCase(segment);
}

function formatMetricPath(path: string): string {
  return path
    .split('/')
    .map((segment) => formatMetricSegment(segment))
    .join(' / ');
}

export function getMetricSeriesLabel(metricName: string): string {
  const parts = metricName.split('/');
  if (parts.length <= 1) {
    return formatMetricSegment(metricName);
  }

  // GPU metrics: gpu/{id}/{metric_type} → "GPU {id}"
  if (parts[0] === 'gpu' && parts.length >= 3) {
    return `GPU ${parts[1]}`;
  }

  return formatMetricPath(parts.slice(1).join('/'));
}
