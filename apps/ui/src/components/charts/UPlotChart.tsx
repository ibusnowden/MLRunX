'use client';

import { useEffect, useRef, useState, useCallback } from 'react';
import uPlot from 'uplot';
import 'uplot/dist/uPlot.min.css';

export interface ChartSeries {
  label: string;
  data: number[];
  color?: string;
}

export interface UPlotChartProps {
  /** X-axis values (typically step or time) */
  xData: number[];
  /** Series data */
  series: ChartSeries[];
  /** Chart title */
  title?: string;
  /** X-axis label */
  xLabel?: string;
  /** Y-axis label */
  yLabel?: string;
  /** Chart height in pixels */
  height?: number;
  /** Enable zoom/pan */
  interactive?: boolean;
  /** Use dark theme */
  darkTheme?: boolean;
  /** Show legend */
  showLegend?: boolean;
  /** Smoothing factor (0 = none, 0.9 = heavy) */
  smoothing?: number;
  /** Callback when viewport changes */
  onViewportChange?: (min: number, max: number) => void;
  /** Enable logarithmic Y-axis scale */
  logScale?: boolean;
  /** Minimum Y value (for clipping) */
  yMin?: number;
  /** Maximum Y value (for clipping) */
  yMax?: number;
  /** Show area fill under lines */
  areaFill?: boolean;
}

// Dark theme colors - vibrant palette inspired by W&B / modern dashboards
const DARK_THEME_COLORS = [
  '#84cc16', // lime
  '#f97316', // orange
  '#a855f7', // purple
  '#ec4899', // pink
  '#06b6d4', // cyan
  '#fbbf24', // yellow
  '#ef4444', // red
  '#3b82f6', // blue
  '#10b981', // emerald
  '#f43f5e', // rose
  '#8b5cf6', // violet
  '#14b8a6', // teal
];

// Light theme colors - deeper, more contrast on white
const LIGHT_THEME_COLORS = [
  '#2563eb', // blue
  '#dc2626', // red
  '#16a34a', // green
  '#9333ea', // purple
  '#ea580c', // orange
  '#db2777', // pink
  '#0d9488', // teal
  '#ca8a04', // yellow
  '#4f46e5', // indigo
  '#059669', // emerald
  '#e11d48', // rose
  '#7c3aed', // violet
];

// Apply exponential moving average smoothing
function smoothData(data: number[], factor: number): number[] {
  if (factor <= 0 || factor >= 1) return data;

  const smoothed: number[] = [];
  let last = data[0];

  for (let i = 0; i < data.length; i++) {
    const val = data[i];
    if (val === null || val === undefined || isNaN(val)) {
      smoothed.push(val);
    } else {
      if (last === null || last === undefined || isNaN(last)) {
        last = val;
      }
      last = factor * last + (1 - factor) * val;
      smoothed.push(last);
    }
  }

  return smoothed;
}

export function UPlotChart({
  xData,
  series,
  title,
  xLabel = 'Step',
  yLabel = 'Value',
  height = 340,
  interactive = true,
  darkTheme = true,
  showLegend = true,
  smoothing = 0,
  onViewportChange,
  logScale = false,
  yMin,
  yMax,
  areaFill = false,
}: UPlotChartProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const chartRef = useRef<uPlot | null>(null);
  const [dimensions, setDimensions] = useState({ width: 0, height });
  const [hoveredSeries, setHoveredSeries] = useState<number | null>(null);

  // Theme colors
  const colors = darkTheme ? DARK_THEME_COLORS : LIGHT_THEME_COLORS;
  const bgColor = darkTheme ? '#0d1117' : '#ffffff';
  const gridColor = darkTheme ? 'rgba(255,255,255,0.06)' : 'rgba(0,0,0,0.06)';
  const axisColor = darkTheme ? '#6e7681' : '#9ca3af';
  const textColor = darkTheme ? '#e6edf3' : '#374151';

  // Observe width from container, height from parent wrapper (for fullscreen)
  useEffect(() => {
    if (!containerRef.current) return;
    const wrapperEl = containerRef.current.parentElement;

    const observer = new ResizeObserver(() => {
      if (!containerRef.current) return;
      const width = containerRef.current.clientWidth;
      // Check if the wrapper (outer div) has an explicit height set by
      // a fullscreen parent. Only then override the default height prop.
      const wrapperHeight = wrapperEl ? wrapperEl.clientHeight : 0;
      // Use wrapper height only if it's significantly larger than the prop
      // (i.e. fullscreen is stretching it), otherwise stick to the prop.
      const effectiveHeight = wrapperHeight > height + 60 ? wrapperHeight - 50 : height;
      setDimensions({ width, height: effectiveHeight });
    });

    observer.observe(containerRef.current);
    if (wrapperEl) observer.observe(wrapperEl);
    return () => observer.disconnect();
  }, [height]);

  // Create/update chart
  useEffect(() => {
    if (!containerRef.current || dimensions.width === 0) return;

    // Destroy existing chart
    if (chartRef.current) {
      chartRef.current.destroy();
      chartRef.current = null;
    }

    // Apply smoothing to data
    const processedSeries = series.map(s => ({
      ...s,
      data: smoothing > 0 ? smoothData(s.data, smoothing) : s.data,
    }));

    // Prepare data: [xData, ...series.data]
    const data: uPlot.AlignedData = [xData, ...processedSeries.map((s) => s.data)];

    // Build series config with optional area fill
    const seriesConfig: uPlot.Series[] = [
      { label: xLabel },
      ...processedSeries.map((s, i) => {
        const color = s.color || colors[i % colors.length];
        return {
          label: s.label,
          stroke: color,
          width: 1,
          points: {
            show: false,
          },
          // Add alpha for non-hovered series
          alpha: hoveredSeries === null || hoveredSeries === i + 1 ? 1 : 0.3,
          // Area fill under line - subtle
          fill: areaFill ? `${color}15` : undefined,
        };
      }),
    ];

    // Chart options
    const opts: uPlot.Options = {
      width: dimensions.width,
      height: dimensions.height,
      title: title,
      series: seriesConfig,
      scales: {
        x: { time: false },
        y: {
          distr: logScale ? 3 : 1,
          min: yMin,
          max: yMax,
        },
      },
      axes: [
        {
          // X-axis — show tick values only, label at far right
          label: xLabel,
          labelSize: 14,
          labelFont: '10px system-ui, sans-serif',
          font: '11px system-ui, sans-serif',
          stroke: axisColor,
          size: 32,
          gap: 4,
          grid: {
            show: true,
            stroke: gridColor,
            width: 1,
          },
          ticks: {
            show: false,
          },
        },
        {
          // Y-axis — tick values only, no label
          font: '11px system-ui, sans-serif',
          stroke: axisColor,
          size: 48,
          gap: 8,
          grid: {
            show: true,
            stroke: gridColor,
            width: 1,
          },
          ticks: {
            show: false,
          },
        },
      ],
      cursor: {
        drag: interactive ? { x: true, y: false } : undefined,
        points: {
          size: 6,
          fill: bgColor,
          stroke: textColor,
          width: 1,
        },
      },
      legend: {
        show: false,
      },
      hooks: interactive && onViewportChange
        ? {
            setScale: [
              (u, key) => {
                if (key === 'x') {
                  const min = u.scales.x.min ?? 0;
                  const max = u.scales.x.max ?? 0;
                  onViewportChange(min, max);
                }
              },
            ],
          }
        : undefined,
    };

    // Create chart
    chartRef.current = new uPlot(opts, data, containerRef.current);

    return () => {
      if (chartRef.current) {
        chartRef.current.destroy();
        chartRef.current = null;
      }
    };
  }, [xData, series, title, xLabel, yLabel, dimensions, interactive, onViewportChange, darkTheme, smoothing, colors, bgColor, gridColor, axisColor, textColor, hoveredSeries, logScale, yMin, yMax, areaFill]);

  // Get last value for each series
  const getLastValue = useCallback((data: number[]): string => {
    for (let i = data.length - 1; i >= 0; i--) {
      if (data[i] !== null && data[i] !== undefined && !isNaN(data[i])) {
        return data[i].toFixed(4);
      }
    }
    return '-';
  }, []);

  return (
    <div className={`rounded-lg overflow-hidden ${darkTheme ? 'bg-[#0d1117]' : 'bg-white'}`}>
      {/* Chart container */}
      <div
        ref={containerRef}
        className="w-full"
        style={{ height, backgroundColor: bgColor }}
      />

      {/* Custom Legend */}
      {showLegend && series.length > 0 && (
        <div className={`px-4 py-3 border-t ${darkTheme ? 'border-[#21262d]' : 'border-gray-100'}`}>
          <div className="flex flex-wrap gap-x-5 gap-y-1.5">
            {series.map((s, i) => {
              const color = s.color || colors[i % colors.length];
              const lastVal = getLastValue(s.data);
              const isHovered = hoveredSeries === i + 1;

              return (
                <div
                  key={s.label}
                  className={`flex items-center gap-2 cursor-pointer transition-opacity ${
                    hoveredSeries !== null && !isHovered ? 'opacity-30' : 'opacity-100'
                  }`}
                  onMouseEnter={() => setHoveredSeries(i + 1)}
                  onMouseLeave={() => setHoveredSeries(null)}
                >
                  <div
                    className="w-3 h-0.5 rounded-full"
                    style={{ backgroundColor: color }}
                  />
                  <span className={`text-xs font-medium ${darkTheme ? 'text-gray-400' : 'text-gray-600'}`}>
                    {s.label}
                  </span>
                  <span className={`text-xs font-mono ${darkTheme ? 'text-gray-600' : 'text-gray-400'}`}>
                    {lastVal}
                  </span>
                </div>
              );
            })}
          </div>
        </div>
      )}
    </div>
  );
}
