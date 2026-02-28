export function QuickstartTab({
  apiBaseUrl,
  pipCommand,
  uvCommand,
  copyToClipboard,
}: {
  apiBaseUrl: string;
  pipCommand: string;
  uvCommand: string;
  copyToClipboard: (value: string, successMessage: string) => void;
}) {
  return (
    <section className="rounded-xl border border-border bg-surface p-4 sm:p-5 space-y-4">
      <div>
        <h2 className="text-lg font-semibold text-text-primary">Quickstart</h2>
        <p className="text-sm text-text-secondary mt-1">
          Use these commands on your training machine or CI runner. Browser UI access uses session login, not API keys.
        </p>
      </div>

      <div className="rounded-lg border border-border bg-surface-secondary p-3">
        <div className="flex flex-col sm:flex-row sm:items-center sm:justify-between gap-2">
          <p className="text-xs text-text-muted">API Base URL</p>
          <button
            type="button"
            onClick={() => copyToClipboard(apiBaseUrl, 'Copied API base URL to clipboard.')}
            className="px-3 py-1.5 rounded-md border border-border text-xs font-medium text-text-secondary hover:bg-surface"
          >
            Copy URL
          </button>
        </div>
        <code className="block mt-2 text-sm text-text-primary break-all">{apiBaseUrl}</code>
      </div>

      <div className="space-y-3">
        <div className="rounded-lg border border-border bg-surface-secondary p-3">
          <div className="flex items-center justify-between gap-2">
            <p className="text-xs text-text-muted">pip quickstart</p>
            <button
              type="button"
              onClick={() => copyToClipboard(pipCommand, 'Copied pip quickstart command.')}
              className="px-3 py-1.5 rounded-md border border-border text-xs font-medium text-text-secondary hover:bg-surface"
            >
              Copy
            </button>
          </div>
          <pre className="mt-2 overflow-x-auto text-xs sm:text-sm text-text-primary">{pipCommand}</pre>
        </div>

        <div className="rounded-lg border border-border bg-surface-secondary p-3">
          <div className="flex items-center justify-between gap-2">
            <p className="text-xs text-text-muted">uv quickstart</p>
            <button
              type="button"
              onClick={() => copyToClipboard(uvCommand, 'Copied uv quickstart command.')}
              className="px-3 py-1.5 rounded-md border border-border text-xs font-medium text-text-secondary hover:bg-surface"
            >
              Copy
            </button>
          </div>
          <pre className="mt-2 overflow-x-auto text-xs sm:text-sm text-text-primary">{uvCommand}</pre>
        </div>
      </div>
    </section>
  );
}
