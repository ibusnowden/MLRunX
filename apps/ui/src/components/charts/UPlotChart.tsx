'use client';

import { useEffect, useRef, useState, useCallback, useMemo, type CSSProperties } from 'react';
import uPlot from 'uplot';
import 'uplot/dist/uPlot.min.css';
import { colorToRgba, createSeriesColorScale } from './chartColors';
import {
  buildCompareTooltipRows,
  formatCompareTooltipMetaLine,
  type CompareTooltipMetaItem,
  type CompareTooltipRow,
} from './compareTooltip';

export interface ChartSeries {
  label: string;
  data: number[];
  color?: string;
  tooltipLabel?: string;
  hoverMeta?: CompareTooltipMetaItem[];
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
  /** Optional custom hover tooltip treatment */
  tooltipVariant?: 'none' | 'compare';
}

type PhaseOverlayMarker = ChartPhaseMarker & {
  height: number;
  left: number;
  top: number;
  visible: boolean;
};

type CompareTooltipState = {
  step: number;
  left: number;
  top: number;
  plotTop: number;
  plotHeight: number;
  rows: CompareTooltipRow[];
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

function sameCompareTooltipState(
  previous: CompareTooltipState | null,
  next: CompareTooltipState | null
): boolean {
  if (previous === next) return true;
  if (!previous || !next) return false;
  if (
    previous.step !== next.step ||
    Math.abs(previous.left - next.left) > 0.5 ||
    Math.abs(previous.top - next.top) > 0.5 ||
    Math.abs(previous.plotTop - next.plotTop) > 0.5 ||
    Math.abs(previous.plotHeight - next.plotHeight) > 0.5 ||
    previous.rows.length !== next.rows.length
  ) {
    return false;
  }

  for (let index = 0; index < previous.rows.length; index += 1) {
    const prev = previous.rows[index];
    const current = next.rows[index];
    if (
      prev.label !== current.label ||
      prev.color !== current.color ||
      prev.isActive !== current.isActive ||
      prev.value !== current.value ||
      prev.valueLabel !== current.valueLabel ||
      prev.hoverMeta.length !== current.hoverMeta.length
    ) {
      return false;
    }
    for (let metaIndex = 0; metaIndex < prev.hoverMeta.length; metaIndex += 1) {
      const prevMeta = prev.hoverMeta[metaIndex];
      const currentMeta = current.hoverMeta[metaIndex];
      if (prevMeta.label !== currentMeta.label || prevMeta.value !== currentMeta.value) {
        return false;
      }
    }
  }

  return true;
}

function truncateTooltipText(value: string, maxLength = 28): string {
  if (value.length <= maxLength) return value;
  return `${value.slice(0, Math.max(0, maxLength - 1))}…`;
}

function formatTooltipStep(value: number): string {
  if (Number.isInteger(value)) return value.toLocaleString();
  return value.toLocaleString(undefined, { maximumFractionDigits: 6 });
}

function getCompareTooltipStyle(
  tooltip: CompareTooltipState,
  width: number,
  height: number
): CSSProperties {
  const style: CSSProperties = {
    maxWidth: Math.max(160, Math.min(320, width - 16)),
  };

  const placeLeft = tooltip.left > width * 0.56;
  if (placeLeft) {
    style.right = Math.max(8, width - tooltip.left + 14);
  } else {
    style.left = Math.max(8, tooltip.left + 14);
  }

  if (tooltip.top < 96) {
    style.top = 8;
  } else if (tooltip.top > height - 96) {
    style.bottom = Math.max(8, height - tooltip.top + 8);
  } else {
    style.top = tooltip.top;
    style.transform = 'translateY(-50%)';
  }

  return style;
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
  tooltipVariant = 'none',
}: UPlotChartProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const chartRef = useRef<uPlot | null>(null);
  const [dimensions, setDimensions] = useState({ width: 0, height });
  const [hoveredSeries, setHoveredSeries] = useState<number | null>(null);
  const [phaseOverlayMarkers, setPhaseOverlayMarkers] = useState<PhaseOverlayMarker[]>([]);
  const [compareTooltip, setCompareTooltip] = useState<CompareTooltipState | null>(null);

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
  const compareCrosshairColor = darkTheme ? 'rgba(167,176,192,0.34)' : 'rgba(107,114,128,0.34)';
  const compareTooltipCardStyle = darkTheme
    ? {
        backgroundColor: 'rgba(7, 10, 16, 0.96)',
        borderColor: 'rgba(167, 176, 192, 0.18)',
        boxShadow: '0 18px 45px rgba(0, 0, 0, 0.35)',
      }
    : {
        backgroundColor: 'rgba(255, 255, 255, 0.98)',
        borderColor: 'rgba(17, 24, 39, 0.08)',
        boxShadow: '0 18px 45px rgba(15, 23, 42, 0.12)',
      };
  const compareTooltipHeaderBorder = darkTheme ? 'rgba(167, 176, 192, 0.12)' : 'rgba(17, 24, 39, 0.06)';
  const compareTooltipActiveRow = darkTheme ? 'rgba(255, 255, 255, 0.045)' : 'rgba(15, 23, 42, 0.045)';
  const gridDash = useMemo(() => (dashedGrid || referenceVariant ? [5, 5] : undefined), [dashedGrid, referenceVariant]);
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

  useEffect(() => {
    if (tooltipVariant !== 'compare' || xData.length === 0) {
      setCompareTooltip(null);
    }
  }, [tooltipVariant, xData.length]);

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
    const lineSeriesIndices: number[] = [];

    // Track which uPlot-series-index each visible line lives at
    let dataIdx = 1; // next index into the data/series arrays

    processedSeries.forEach((s, i) => {
      const color = s.resolvedColor;
      const isActive = hoveredSeries === null || hoveredSeries === i + 1;
      const lineSeriesIdx = dataIdx;
      lineSeriesIndices.push(lineSeriesIdx);

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
    const syncCompareTooltip = (plot: uPlot) => {
      if (tooltipVariant !== 'compare') {
        setCompareTooltip((previous) => (previous === null ? previous : null));
        return;
      }

      const cursorIdx = plot.cursor.idx;
      const cursorLeft = plot.cursor.left;
      const cursorTop = plot.cursor.top;
      if (
        cursorIdx == null ||
        cursorIdx < 0 ||
        cursorIdx >= xData.length ||
        cursorLeft == null ||
        cursorTop == null ||
        !Number.isFinite(cursorLeft) ||
        !Number.isFinite(cursorTop)
      ) {
        setCompareTooltip((previous) => (previous === null ? previous : null));
        return;
      }

      const cursorIdxs = plot.cursor.idxs ?? [];
      const pxRatio = uPlot.pxRatio || 1;
      const plotLeft = plot.bbox.left / pxRatio;
      const plotTop = plot.bbox.top / pxRatio;
      const plotHeight = plot.bbox.height / pxRatio;
      const rows = buildCompareTooltipRows(
        processedSeries.flatMap((entry, index) => {
          const lineSeriesIdx = lineSeriesIndices[index];
          const seriesCursorIdx = cursorIdxs[lineSeriesIdx];
          const valueIndex =
            typeof seriesCursorIdx === 'number' && seriesCursorIdx >= 0 ? seriesCursorIdx : cursorIdx;
          if (valueIndex !== cursorIdx) return [];

          const value = entry.data[valueIndex];
          if (!Number.isFinite(value)) return [];

          return [{
            label: entry.label,
            tooltipLabel: entry.tooltipLabel,
            color: entry.resolvedColor,
            value,
            hoverMeta: entry.hoverMeta,
            yDistance: Math.abs(plot.valToPos(value, 'y') - cursorTop),
          }];
        })
      );

      if (rows.length === 0) {
        setCompareTooltip((previous) => (previous === null ? previous : null));
        return;
      }

      const next: CompareTooltipState = {
        step: xData[cursorIdx] ?? cursorIdx,
        left: plotLeft + cursorLeft,
        top: plotTop + cursorTop,
        plotTop,
        plotHeight,
        rows,
      };

      setCompareTooltip((previous) => (sameCompareTooltipState(previous, next) ? previous : next));
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
    const setCursorHooks: NonNullable<uPlot.Hooks.Arrays['setCursor']> = [];
    if (tooltipVariant === 'compare') {
      setCursorHooks.push(syncCompareTooltip);
    }
    const drawHooks: NonNullable<uPlot.Hooks.Arrays['draw']> = [];
    if (phaseMarkers.length > 0) {
      drawHooks.push(syncPhaseOverlays);
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
        setScaleHooks.length > 0 || drawHooks.length > 0 || setCursorHooks.length > 0
          ? {
              draw: drawHooks.length > 0 ? drawHooks : undefined,
              setCursor: setCursorHooks.length > 0 ? setCursorHooks : undefined,
              setScale: setScaleHooks.length > 0 ? setScaleHooks : undefined,
            }
          : undefined,
    };

    // Create chart
    chartRef.current = new uPlot(opts, data, containerRef.current);
    syncPhaseOverlays(chartRef.current);
    syncCompareTooltip(chartRef.current);

    return () => {
      if (chartRef.current) {
        chartRef.current.destroy();
        chartRef.current = null;
      }
    };
  }, [xData, resolvedSeries, title, xLabel, yLabel, dimensions, interactive, onViewportChange, darkTheme, smoothing, bgColor, gridColor, axisColor, textColor, hoveredSeries, logScale, yMin, yMax, areaFill, xTickFormatter, yTickFormatter, phaseMarkers, lineWidth, showXAxisLabel, showYAxisLabel, dashedGrid, referenceVariant, gridDash, tooltipVariant]);

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
        {tooltipVariant === 'compare' && compareTooltip && (
          <div className="pointer-events-none absolute inset-0">
            <div
              className="absolute border-l border-dashed"
              style={{
                borderColor: compareCrosshairColor,
                height: compareTooltip.plotHeight,
                left: compareTooltip.left,
                top: compareTooltip.plotTop,
              }}
            />
            <div
              className="absolute z-10 overflow-hidden rounded-2xl border backdrop-blur-sm"
              style={{
                ...compareTooltipCardStyle,
                ...getCompareTooltipStyle(compareTooltip, dimensions.width, dimensions.height),
              }}
            >
              <div
                className="border-b px-3 py-2"
                style={{ borderColor: compareTooltipHeaderBorder }}
              >
                <div className="text-[11px] font-semibold uppercase tracking-[0.16em] text-text-secondary">
                  Step: <span className="font-mono normal-case tracking-normal text-text-primary">{formatTooltipStep(compareTooltip.step)}</span>
                </div>
              </div>
              <div
                className="grid grid-cols-[88px_minmax(0,1fr)] gap-3 border-b px-3 py-1.5 text-[10px] font-semibold uppercase tracking-[0.16em] text-text-muted"
                style={{ borderColor: compareTooltipHeaderBorder }}
              >
                <span className="text-right">Value</span>
                <span>Name</span>
              </div>
              <div className="max-h-[220px] overflow-y-auto py-1">
                {compareTooltip.rows.map((row, index) => {
                  const fullMetaLine = formatCompareTooltipMetaLine(row.hoverMeta);
                  const truncatedMetaLine = formatCompareTooltipMetaLine(
                    row.hoverMeta.map((entry) => ({
                      ...entry,
                      value: truncateTooltipText(entry.value),
                    }))
                  );

                  return (
                    <div
                      key={row.label}
                      className="grid grid-cols-[88px_minmax(0,1fr)] gap-3 px-3 py-2"
                      style={{ backgroundColor: row.isActive ? compareTooltipActiveRow : 'transparent' }}
                    >
                      <div className={`text-right font-mono text-[12px] ${row.isActive ? 'text-text-primary' : 'text-text-secondary'}`}>
                        {row.valueLabel}
                      </div>
                      <div className="min-w-0">
                        <div className="flex items-center gap-2">
                          <span className="h-2 w-2 flex-shrink-0 rounded-full" style={{ backgroundColor: row.color }} />
                          <span className={`truncate text-[12px] font-medium ${row.isActive ? 'text-text-primary' : 'text-text-secondary'}`}>
                            {row.label}
                          </span>
                        </div>
                        {row.hoverMeta.length > 0 && (
                          <div
                            className="mt-1 truncate text-[10px] text-text-muted"
                            title={fullMetaLine}
                          >
                            {truncatedMetaLine}
                          </div>
                        )}
                      </div>
                    </div>
                  );
                })}
              </div>
            </div>
          </div>
        )}
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
