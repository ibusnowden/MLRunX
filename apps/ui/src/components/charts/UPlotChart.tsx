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

export interface ChartPhaseMarker {
  label: string;
  value: number;
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
  /** Format x-axis tick labels */
  xTickFormatter?: (value: number) => string;
  /** Format y-axis tick labels */
  yTickFormatter?: (value: number) => string;
  /** Overlay vertical phase markers on the plot area */
  phaseMarkers?: ChartPhaseMarker[];
  /** Explicit line width */
  lineWidth?: number;
  /** Show the x-axis label */
  showXAxisLabel?: boolean;
  /** Show the y-axis label */
  showYAxisLabel?: boolean;
  /** Use dashed grid lines */
  dashedGrid?: boolean;
  /** Chart styling variant */
  variant?: 'default' | 'reference';
}

type PhaseOverlayMarker = ChartPhaseMarker & {
  height: number;
  left: number;
  top: number;
  visible: boolean;
};

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

function formatAxisSplits(
  splits: number[],
  formatter?: (value: number) => string
): Array<string | number | null> | undefined {
  if (!formatter) return undefined;
  return splits.map((split) => formatter(Number(split)));
}

function buildPhaseOverlayMarkers(
  plot: uPlot,
  phaseMarkers: ChartPhaseMarker[]
): PhaseOverlayMarker[] {
  if (phaseMarkers.length === 0) return [];

  const pxRatio = uPlot.pxRatio || 1;
  const plotLeft = plot.bbox.left / pxRatio;
  const plotTop = plot.bbox.top / pxRatio;
  const plotHeight = plot.bbox.height / pxRatio;
  const xMin = plot.scales.x.min ?? Number.NEGATIVE_INFINITY;
  const xMax = plot.scales.x.max ?? Number.POSITIVE_INFINITY;

  return phaseMarkers.map((marker) => ({
    ...marker,
    height: plotHeight,
    left: plotLeft + plot.valToPos(marker.value, 'x'),
    top: plotTop,
    visible: marker.value >= xMin && marker.value <= xMax,
  }));
}

function samePhaseOverlayMarkers(
  previous: PhaseOverlayMarker[],
  next: PhaseOverlayMarker[]
): boolean {
  if (previous.length !== next.length) return false;

  for (let index = 0; index < previous.length; index += 1) {
    const prev = previous[index];
    const current = next[index];
    if (
      prev.label !== current.label ||
      prev.color !== current.color ||
      prev.value !== current.value ||
      prev.visible !== current.visible ||
      Math.abs(prev.left - current.left) > 0.5 ||
      Math.abs(prev.top - current.top) > 0.5 ||
      Math.abs(prev.height - current.height) > 0.5
    ) {
      return false;
    }
  }

  return true;
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
  xTickFormatter,
  yTickFormatter,
  phaseMarkers = [],
  lineWidth,
  showXAxisLabel = true,
  showYAxisLabel = false,
  dashedGrid = false,
  variant = 'default',
}: UPlotChartProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const chartRef = useRef<uPlot | null>(null);
  const [dimensions, setDimensions] = useState({ width: 0, height });
  const [hoveredSeries, setHoveredSeries] = useState<number | null>(null);
  const [phaseOverlayMarkers, setPhaseOverlayMarkers] = useState<PhaseOverlayMarker[]>([]);

  // Keep chart theme tied directly to the theme state to avoid DOM class timing races on toggle.
  const referenceVariant = variant === 'reference';
  const bgColor = darkTheme ? '#000000' : '#ffffff';
  const gridColor = darkTheme
    ? (referenceVariant ? 'rgba(174,190,213,0.16)' : 'rgba(255,255,255,0.05)')
    : 'rgba(0,0,0,0.06)';
  const axisColor = darkTheme
    ? (referenceVariant ? '#c7ceda' : '#8a909b')
    : '#9ca3af';
  const textColor = darkTheme
    ? (referenceVariant ? '#dfe6ef' : '#dce2ea')
    : '#374151';
  const phaseMarkerColor = darkTheme ? '#4a99ff' : '#2563eb';
  const gridDash = dashedGrid || referenceVariant ? [5, 5] : undefined;
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

  useEffect(() => {
    if (phaseMarkers.length === 0) {
      setPhaseOverlayMarkers([]);
    }
  }, [phaseMarkers]);

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
        width: lineWidth ?? (referenceVariant ? 1.8 : 1.1),
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
    const syncPhaseOverlays = (plot: uPlot) => {
      if (phaseMarkers.length === 0) {
        setPhaseOverlayMarkers([]);
        return;
      }

      const next = buildPhaseOverlayMarkers(plot, phaseMarkers);
      setPhaseOverlayMarkers((previous) => (samePhaseOverlayMarkers(previous, next) ? previous : next));
    };

    const setScaleHooks: NonNullable<uPlot.Hooks.Arrays['setScale']> = [];
    if (interactive && onViewportChange) {
      setScaleHooks.push((u, key) => {
        if (key === 'x') {
          const min = u.scales.x.min ?? 0;
          const max = u.scales.x.max ?? 0;
          onViewportChange(min, max);
        }
      });
    }

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
          label: showXAxisLabel ? xLabel : undefined,
          labelSize: showXAxisLabel ? (referenceVariant ? 18 : 14) : 0,
          labelGap: showXAxisLabel ? 8 : 0,
          labelFont: referenceVariant ? '12px system-ui, sans-serif' : '10px system-ui, sans-serif',
          font: referenceVariant ? '12px system-ui, sans-serif' : '11px system-ui, sans-serif',
          stroke: axisColor,
          size: referenceVariant ? 36 : 32,
          gap: 4,
          values: xTickFormatter
            ? (_u, splits) => formatAxisSplits(splits, xTickFormatter) ?? []
            : undefined,
          grid: {
            show: true,
            stroke: gridColor,
            width: referenceVariant ? 1.1 : 1,
            dash: gridDash,
          },
          ticks: {
            show: false,
          },
        },
        {
          // Y-axis — tick values only by default, with optional label in reference layouts
          label: showYAxisLabel ? yLabel : undefined,
          labelSize: showYAxisLabel ? (referenceVariant ? 24 : 18) : 0,
          labelGap: showYAxisLabel ? 10 : 0,
          labelFont: referenceVariant ? '12px system-ui, sans-serif' : '10px system-ui, sans-serif',
          font: referenceVariant ? '12px system-ui, sans-serif' : '11px system-ui, sans-serif',
          stroke: axisColor,
          size: showYAxisLabel ? (referenceVariant ? 62 : 54) : 48,
          gap: referenceVariant ? 10 : 8,
          values: yTickFormatter
            ? (_u, splits) => formatAxisSplits(splits, yTickFormatter) ?? []
            : undefined,
          grid: {
            show: true,
            stroke: gridColor,
            width: referenceVariant ? 1.1 : 1,
            dash: gridDash,
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
      hooks:
        setScaleHooks.length > 0 || phaseMarkers.length > 0
          ? {
              draw: phaseMarkers.length > 0 ? [syncPhaseOverlays] : undefined,
              setScale: setScaleHooks.length > 0 ? setScaleHooks : undefined,
            }
          : undefined,
    };

    // Create chart
    chartRef.current = new uPlot(opts, data, containerRef.current);
    syncPhaseOverlays(chartRef.current);

    return () => {
      if (chartRef.current) {
        chartRef.current.destroy();
        chartRef.current = null;
      }
    };
  }, [xData, resolvedSeries, title, xLabel, yLabel, dimensions, interactive, onViewportChange, darkTheme, smoothing, bgColor, gridColor, axisColor, textColor, hoveredSeries, logScale, yMin, yMax, areaFill, xTickFormatter, yTickFormatter, phaseMarkers, lineWidth, showXAxisLabel, showYAxisLabel, dashedGrid, referenceVariant, gridDash]);

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
      <div className="relative w-full flex-shrink-0">
        <div
          ref={containerRef}
          className="w-full flex-shrink-0"
          style={{ height: dimensions.height || height, backgroundColor: bgColor }}
        />
        {phaseOverlayMarkers.length > 0 && (
          <div className="pointer-events-none absolute inset-0">
            {phaseOverlayMarkers
              .filter((marker) => marker.visible)
              .map((marker) => (
                <div
                  key={`${marker.label}-${marker.value}`}
                  className="absolute"
                  style={{ height: marker.height, left: marker.left, top: marker.top }}
                >
                  <div
                    className="absolute inset-y-0 left-0 border-l border-dashed"
                    style={{ borderColor: marker.color || phaseMarkerColor }}
                  />
                  <div
                    className="absolute left-[6px] top-[10px] origin-top-left -rotate-90 whitespace-nowrap text-[11px] tracking-[0.08em]"
                    style={{ color: marker.color || phaseMarkerColor }}
                  >
                    {marker.label}
                  </div>
                </div>
              ))}
          </div>
        )}
      </div>

      {/* Custom Legend */}
      {showLegend && series.length > 0 && (
        <div className={`border-t border-border ${referenceVariant ? 'px-3 py-2.5' : 'px-4 py-3'}`}>
          <div className={`flex flex-wrap ${referenceVariant ? 'gap-x-4 gap-y-1' : 'gap-x-5 gap-y-1.5'}`}>
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
