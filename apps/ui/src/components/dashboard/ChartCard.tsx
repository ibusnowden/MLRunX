'use client';

import { useState, useRef, useCallback } from 'react';

interface ChartCardProps {
  /** Chart title */
  title: string;
  /** Subtitle/description */
  subtitle?: string;
  /** Children (the chart component) */
  children: React.ReactNode;
  /** Use dark theme */
  darkTheme?: boolean;
  /** Callback when fullscreen is triggered */
  onFullscreen?: () => void;
  /** Callback when download is triggered */
  onDownload?: () => void;
  /** Callback for CSV export */
  onExportCSV?: () => void;
}

// Icons
const FullscreenIcon = () => (
  <svg className="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M4 8V4m0 0h4M4 4l5 5m11-1V4m0 0h-4m4 0l-5 5M4 16v4m0 0h4m-4 0l5-5m11 5l-5-5m5 5v-4m0 4h-4" />
  </svg>
);

const ExitFullscreenIcon = () => (
  <svg className="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M9 9V4m0 5H4m5 0L4 4m15 5h-5m5 0V4m0 5l-5-5M9 15v5m0-5H4m5 0l-5 5m15-5h-5m0 0v5m0-5l5 5" />
  </svg>
);

const MenuIcon = () => (
  <svg className="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M12 5v.01M12 12v.01M12 19v.01M12 6a1 1 0 110-2 1 1 0 010 2zm0 7a1 1 0 110-2 1 1 0 010 2zm0 7a1 1 0 110-2 1 1 0 010 2z" />
  </svg>
);

const DownloadIcon = () => (
  <svg className="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M4 16v1a3 3 0 003 3h10a3 3 0 003-3v-1m-4-4l-4 4m0 0l-4-4m4 4V4" />
  </svg>
);

const CSVIcon = () => (
  <svg className="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M9 12h6m-6 4h6m2 5H7a2 2 0 01-2-2V5a2 2 0 012-2h5.586a1 1 0 01.707.293l5.414 5.414a1 1 0 01.293.707V19a2 2 0 01-2 2z" />
  </svg>
);

export function ChartCard({
  title,
  subtitle,
  children,
  onFullscreen,
  onDownload,
  onExportCSV,
}: ChartCardProps) {
  const [showMenu, setShowMenu] = useState(false);
  const [isFullscreen, setIsFullscreen] = useState(false);
  const containerRef = useRef<HTMLDivElement>(null);

  const handleFullscreen = useCallback(() => {
    if (!containerRef.current) return;

    if (!document.fullscreenElement) {
      containerRef.current.requestFullscreen();
      setIsFullscreen(true);
    } else {
      document.exitFullscreen();
      setIsFullscreen(false);
    }
    onFullscreen?.();
  }, [onFullscreen]);

  const handleDownload = useCallback(() => {
    const canvas = containerRef.current?.querySelector('canvas');
    if (canvas) {
      const link = document.createElement('a');
      link.download = `${title.replace(/[^a-z0-9]/gi, '_').toLowerCase()}.png`;
      link.href = canvas.toDataURL('image/png');
      link.click();
    }
    onDownload?.();
    setShowMenu(false);
  }, [title, onDownload]);

  const handleExportCSV = useCallback(() => {
    onExportCSV?.();
    setShowMenu(false);
  }, [onExportCSV]);

  return (
    <div
      ref={containerRef}
      className={`bg-chart-bg rounded-lg border border-border overflow-hidden ${
        isFullscreen ? 'fixed inset-0 z-50 rounded-none' : ''
      }`}
    >
      {/* Header */}
      <div className="flex items-center justify-between px-4 py-2 border-b border-border">
        <div className="flex-1 min-w-0">
          <h3 className="text-sm font-medium truncate text-text-primary">{title}</h3>
          {subtitle && (
            <p className="text-xs truncate mt-0 text-text-muted">{subtitle}</p>
          )}
        </div>

        {/* Controls */}
        <div className="flex items-center gap-1 ml-2">
          <button
            onClick={handleFullscreen}
            className="p-1.5 rounded hover:bg-surface-hover text-text-muted hover:text-text-primary transition-colors"
            title={isFullscreen ? 'Exit fullscreen' : 'Fullscreen'}
          >
            {isFullscreen ? <ExitFullscreenIcon /> : <FullscreenIcon />}
          </button>

          <div className="relative">
            <button
              onClick={() => setShowMenu(!showMenu)}
              className="p-1.5 rounded hover:bg-surface-hover text-text-muted hover:text-text-primary transition-colors"
              title="More options"
            >
              <MenuIcon />
            </button>

            {showMenu && (
              <>
                <div
                  className="fixed inset-0 z-10"
                  onClick={() => setShowMenu(false)}
                />
                <div className="absolute right-0 top-8 z-20 w-48 rounded-lg shadow-lg border border-border bg-surface py-1">
                  <button
                    onClick={handleDownload}
                    className="w-full flex items-center gap-2 px-3 py-2 text-sm text-text-primary hover:bg-surface-hover"
                  >
                    <DownloadIcon />
                    Download PNG
                  </button>
                  {onExportCSV && (
                    <button
                      onClick={handleExportCSV}
                      className="w-full flex items-center gap-2 px-3 py-2 text-sm text-text-primary hover:bg-surface-hover"
                    >
                      <CSVIcon />
                      Export CSV
                    </button>
                  )}
                </div>
              </>
            )}
          </div>
        </div>
      </div>

      {/* Chart content */}
      <div className={`${isFullscreen ? 'h-[calc(100%-48px)]' : ''}`}>
        {children}
      </div>
    </div>
  );
}
