'use client';

import { useEffect, useRef, useState, useCallback, useMemo } from 'react';
import uPlot from 'uplot';
import 'uplot/dist/uPlot.min.css';
import { colorToRgba, createSeriesColorScale } from './chartColors';

export interface ChartSeries {
  label: string;
  data: number[];
  color?: string;
  /** Upper bound data (e.g. max values) for band shading */
  upper?: number[];
  /** Lower bound data (e.g. min values) for band shading */
  lower?: number[];
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

  // Keep chart theme tied directly to the theme state to avoid DOM class timing races on toggle.
  const bgColor = darkTheme ? '#000000' : '#ffffff';
  const gridColor = darkTheme ? 'rgba(255,255,255,0.05)' : 'rgba(0,0,0,0.06)';
  const axisColor = darkTheme ? '#8a909b' : '#9ca3af';
  const textColor = darkTheme ? '#dce2ea' : '#374151';
  const seriesColorScale = useMemo(
    () => createSeriesColorScale(series.map((entry, index) => entry.label || `series-${index}`), darkTheme),
    [series, darkTheme]
  );
  const resolvedSeries = useMemo(
    () => series.map((entry, index) => ({ ...entry, resolvedColor: entry.color || seriesColorScale(entry.label, index) })),
    [series, seriesColorScale]
  );

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
      const legendHeight = showLegend && series.length > 0 ? 52 : 0;
      // Use wrapper height only when a parent stretches us (fullscreen/expanded).
      // Keep fixed-height behavior for normal inline charts.
      const effectiveHeight =
        wrapperHeight > height + 60
          ? Math.max(220, Math.floor(wrapperHeight - legendHeight))
          : height;
      setDimensions({ width, height: effectiveHeight });
    });

    observer.observe(containerRef.current);
    if (wrapperEl) observer.observe(wrapperEl);
    return () => observer.disconnect();
  }, [height, showLegend, series.length]);

  // Create/update chart
  useEffect(() => {
    if (!containerRef.current || dimensions.width === 0) return;

    // Destroy existing chart
    if (chartRef.current) {
      chartRef.current.destroy();
      chartRef.current = null;
    }

    // Apply smoothing to data
    const processedSeries = resolvedSeries.map(s => ({
      ...s,
      data: smoothing > 0 ? smoothData(s.data, smoothing) : s.data,
      upper: s.upper && smoothing > 0 ? smoothData(s.upper, smoothing) : s.upper,
      lower: s.lower && smoothing > 0 ? smoothData(s.lower, smoothing) : s.lower,
    }));

    // Build data array and series config, interleaving band series
    // Layout: [x, line1, upper1?, lower1?, line2, upper2?, lower2?, ...]
    const dataArrays: (number[] | null[])[] = [xData];
    const seriesConfig: uPlot.Series[] = [{ label: xLabel }];
    const bands: uPlot.Band[] = [];

    // Track which uPlot-series-index each visible line lives at
    let dataIdx = 1; // next index into the data/series arrays

    processedSeries.forEach((s, i) => {
      const color = s.resolvedColor;
      const isActive = hoveredSeries === null || hoveredSeries === i + 1;

      // 1. Main line series
      dataArrays.push(s.data);
      seriesConfig.push({
        label: s.label,
        stroke: color,
        width: 1.1,
        points: { show: false },
        alpha: isActive ? 1 : 0.3,
        fill: areaFill ? `${color}15` : undefined,
      });
      dataIdx++;

      // 2. Band (upper/lower) series — only if bounds are provided
      if (s.upper && s.lower) {
        // Upper bound (hidden line)
        dataArrays.push(s.upper);
        seriesConfig.push({
          label: `${s.label} upper`,
          stroke: 'transparent',
          width: 0,
          points: { show: false },
          show: true,         // must be true for band to render
          alpha: 0,           // visually invisible line
        });
        const upperIdx = dataIdx;
        dataIdx++;

        // Lower bound (hidden line)
        dataArrays.push(s.lower);
        seriesConfig.push({
          label: `${s.label} lower`,
          stroke: 'transparent',
          width: 0,
          points: { show: false },
          show: true,
          alpha: 0,
        });
        const lowerIdx = dataIdx;
        dataIdx++;

        // Band between upper and lower
        bands.push({
          series: [upperIdx, lowerIdx] as [number, number],
          fill: colorToRgba(color, isActive ? 0.12 : 0.05),
        });
      }
    });

    const data: uPlot.AlignedData = dataArrays as uPlot.AlignedData;

    // Chart options
    const opts: uPlot.Options = {
      width: dimensions.width,
      height: dimensions.height,
      title: title,
      series: seriesConfig,
      bands: bands.length > 0 ? bands : undefined,
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
          size: 5,
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
  }, [xData, resolvedSeries, title, xLabel, yLabel, dimensions, interactive, onViewportChange, darkTheme, smoothing, bgColor, gridColor, axisColor, textColor, hoveredSeries, logScale, yMin, yMax, areaFill]);

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
    <div className="h-full rounded-lg overflow-hidden flex flex-col bg-chart-bg">
      {/* Chart container */}
      <div
        ref={containerRef}
        className="w-full flex-shrink-0"
        style={{ height: dimensions.height || height, backgroundColor: bgColor }}
      />

      {/* Custom Legend */}
      {showLegend && series.length > 0 && (
        <div className="px-4 py-3 border-t border-border">
          <div className="flex flex-wrap gap-x-5 gap-y-1.5">
            {resolvedSeries.map((s, i) => {
              const color = s.resolvedColor;
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
                  <span className="text-xs font-medium text-text-secondary">
                    {s.label}
                  </span>
                  <span className="text-xs font-mono text-text-muted">
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
