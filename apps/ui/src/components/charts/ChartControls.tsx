'use client';

import { useState, useRef } from 'react';

interface ChartControlsProps {
  /** Title to display */
  title?: string;
  /** Callback to trigger download */
  onDownload?: () => void;
  /** Callback to toggle fullscreen */
  onFullscreen?: () => void;
  /** Current smoothing value */
  smoothing?: number;
  /** Callback when smoothing changes */
  onSmoothingChange?: (value: number) => void;
  /** Callback to reset zoom */
  onResetZoom?: () => void;
  /** Is dark theme */
  darkTheme?: boolean;
  /** Children (the chart) */
  children: React.ReactNode;
}

const FullscreenIcon = () => (
  <svg className="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M4 8V4m0 0h4M4 4l5 5m11-1V4m0 0h-4m4 0l-5 5M4 16v4m0 0h4m-4 0l5-5m11 5l-5-5m5 5v-4m0 4h-4" />
  </svg>
);

const DownloadIcon = () => (
  <svg className="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M4 16v1a3 3 0 003 3h10a3 3 0 003-3v-1m-4-4l-4 4m0 0l-4-4m4 4V4" />
  </svg>
);

const SettingsIcon = () => (
  <svg className="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M12 6V4m0 2a2 2 0 100 4m0-4a2 2 0 110 4m-6 8a2 2 0 100-4m0 4a2 2 0 110-4m0 4v2m0-6V4m6 6v10m6-2a2 2 0 100-4m0 4a2 2 0 110-4m0 4v2m0-6V4" />
  </svg>
);

const ResetIcon = () => (
  <svg className="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15" />
  </svg>
);

export function ChartControls({
  title,
  onDownload,
  onFullscreen,
  smoothing = 0,
  onSmoothingChange,
  onResetZoom,
  children,
}: ChartControlsProps) {
  const [showSettings, setShowSettings] = useState(false);
  const [isFullscreen, setIsFullscreen] = useState(false);
  const containerRef = useRef<HTMLDivElement>(null);

  const handleFullscreen = () => {
    if (!containerRef.current) return;

    if (!document.fullscreenElement) {
      containerRef.current.requestFullscreen();
      setIsFullscreen(true);
    } else {
      document.exitFullscreen();
      setIsFullscreen(false);
    }
    onFullscreen?.();
  };

  const handleDownload = () => {
    const canvas = containerRef.current?.querySelector('canvas');
    if (canvas) {
      const link = document.createElement('a');
      link.download = `${title || 'chart'}.png`;
      link.href = canvas.toDataURL('image/png');
      link.click();
    }
    onDownload?.();
  };

  return (
    <div
      ref={containerRef}
      className={`relative ${isFullscreen ? 'fixed inset-0 z-50' : ''} bg-chart-bg`}
    >
      {/* Header with title and controls */}
      <div className="flex items-center justify-between px-4 py-2">
        <h3 className="text-sm font-medium text-text-secondary">
          {title}
        </h3>

        <div className="flex items-center gap-1">
          <div className="relative">
            <button
              onClick={() => setShowSettings(!showSettings)}
              className="p-1.5 rounded hover:bg-surface-hover text-text-muted hover:text-text-primary transition-colors"
              title="Settings"
            >
              <SettingsIcon />
            </button>

            {showSettings && (
              <div className="absolute right-0 top-8 z-10 w-64 rounded-lg shadow-lg border border-border bg-surface p-3">
                {onSmoothingChange && (
                  <div className="mb-3">
                    <label className="block text-xs font-medium mb-1 text-text-muted">
                      Smoothing: {smoothing.toFixed(2)}
                    </label>
                    <input
                      type="range"
                      min="0"
                      max="0.99"
                      step="0.01"
                      value={smoothing}
                      onChange={(e) => onSmoothingChange(parseFloat(e.target.value))}
                      className="w-full h-2 rounded-lg appearance-none cursor-pointer bg-border"
                    />
                    <div className="flex justify-between text-xs mt-1 text-text-muted">
                      <span>None</span>
                      <span>Heavy</span>
                    </div>
                  </div>
                )}

                {onResetZoom && (
                  <button
                    onClick={() => {
                      onResetZoom();
                      setShowSettings(false);
                    }}
                    className="w-full flex items-center gap-2 px-3 py-2 rounded text-sm hover:bg-surface-hover text-text-primary"
                  >
                    <ResetIcon />
                    Reset Zoom
                  </button>
                )}
              </div>
            )}
          </div>

          <button
            onClick={handleDownload}
            className="p-1.5 rounded hover:bg-surface-hover text-text-muted hover:text-text-primary transition-colors"
            title="Download PNG"
          >
            <DownloadIcon />
          </button>

          <button
            onClick={handleFullscreen}
            className="p-1.5 rounded hover:bg-surface-hover text-text-muted hover:text-text-primary transition-colors"
            title="Fullscreen"
          >
            <FullscreenIcon />
          </button>
        </div>
      </div>

      <div className={isFullscreen ? 'h-[calc(100%-48px)]' : ''}>
        {children}
      </div>

      {showSettings && (
        <div
          className="fixed inset-0 z-0"
          onClick={() => setShowSettings(false)}
        />
      )}
    </div>
  );
}
