import { MetricAlias, ProjectInfo, UiAuthSessionResult } from '@/lib/api';

export function MetricAliasesTab({
  session,
  availableProjects,
  selectedProjectId,
  setSelectedProjectId,
  newAliasMetricName,
  setNewAliasMetricName,
  newAliasDisplayName,
  setNewAliasDisplayName,
  aliases,
  loadingAliases,
  submittingAlias,
  deletingAliasMetricName,
  formatProjectRef,
  handleUpsertAlias,
  handleDeleteAlias,
  refreshAliases,
}: {
  session: UiAuthSessionResult | null;
  availableProjects: ProjectInfo[];
  selectedProjectId: string;
  setSelectedProjectId: (v: string) => void;
  newAliasMetricName: string;
  setNewAliasMetricName: (v: string) => void;
  newAliasDisplayName: string;
  setNewAliasDisplayName: (v: string) => void;
  aliases: MetricAlias[];
  loadingAliases: boolean;
  submittingAlias: boolean;
  deletingAliasMetricName: string | null;
  formatProjectRef: (projectId: string | null) => string;
  handleUpsertAlias: () => void;
  handleDeleteAlias: (metricName: string) => void;
  refreshAliases: () => void;
}) {
  return (
    <section className="rounded-xl border border-border bg-surface p-4 sm:p-5 space-y-4">
      <div>
        <h2 className="text-lg font-semibold text-text-primary">Metric Labels</h2>
        <p className="mt-1 text-sm text-text-secondary">
          Rename raw metric keys for UI clarity without changing logged training data.
        </p>
      </div>

      <div className="rounded-lg border border-border bg-surface-secondary p-3 space-y-3">
        <div>
          <label htmlFor="metric-alias-project" className="block text-sm font-medium text-text-primary mb-1.5">
            Project
          </label>
          <select
            id="metric-alias-project"
            value={selectedProjectId}
            onChange={(event) => setSelectedProjectId(event.target.value)}
            className="w-full rounded-lg border border-border bg-surface px-3 py-2.5 text-sm text-text-primary focus:outline-none focus:ring-2 focus:ring-accent focus:border-transparent"
            disabled={!session || availableProjects.length === 0}
          >
            <option value="">{availableProjects.length ? 'Select project' : 'No active session'}</option>
            {availableProjects.map((project) => (
              <option key={project.project_id} value={project.project_id}>
                {formatProjectRef(project.project_id)}
              </option>
            ))}
          </select>
        </div>

        <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
          <div>
            <label htmlFor="metric-alias-name" className="block text-sm font-medium text-text-primary mb-1.5">
              Raw Metric Key
            </label>
            <input
              id="metric-alias-name"
              type="text"
              value={newAliasMetricName}
              onChange={(event) => setNewAliasMetricName(event.target.value)}
              placeholder="train/loss"
              className="w-full rounded-lg border border-border bg-surface px-3 py-2.5 text-sm text-text-primary placeholder:text-text-muted focus:outline-none focus:ring-2 focus:ring-accent focus:border-transparent"
            />
          </div>
          <div>
            <label htmlFor="metric-alias-display" className="block text-sm font-medium text-text-primary mb-1.5">
              Display Label
            </label>
            <input
              id="metric-alias-display"
              type="text"
              value={newAliasDisplayName}
              onChange={(event) => setNewAliasDisplayName(event.target.value)}
              placeholder="Training Loss"
              className="w-full rounded-lg border border-border bg-surface px-3 py-2.5 text-sm text-text-primary placeholder:text-text-muted focus:outline-none focus:ring-2 focus:ring-accent focus:border-transparent"
            />
          </div>
        </div>

        <div className="flex flex-wrap gap-2">
          <button
            type="button"
            onClick={handleUpsertAlias}
            disabled={!session || !selectedProjectId || submittingAlias}
            className="px-4 py-2.5 rounded-lg bg-accent text-white text-sm font-medium hover:bg-accent-hover transition-colors disabled:opacity-60 disabled:cursor-not-allowed"
          >
            {submittingAlias ? 'Saving...' : 'Save Label'}
          </button>
          <button
            type="button"
            onClick={refreshAliases}
            disabled={!session || !selectedProjectId || loadingAliases}
            className="px-4 py-2.5 rounded-lg border border-border text-sm font-medium text-text-secondary hover:bg-surface hover:text-text-primary transition-colors disabled:opacity-60 disabled:cursor-not-allowed"
          >
            {loadingAliases ? 'Refreshing...' : 'Refresh Labels'}
          </button>
        </div>
      </div>

      <div className="overflow-x-auto rounded-lg border border-border">
        <table className="w-full text-left text-sm">
          <thead className="bg-surface-secondary text-text-secondary">
            <tr>
              <th className="px-3 py-2 font-medium">Raw Metric</th>
              <th className="px-3 py-2 font-medium">Display Label</th>
              <th className="px-3 py-2 font-medium">Updated</th>
              <th className="px-3 py-2 font-medium">Action</th>
            </tr>
          </thead>
          <tbody>
            {aliases.length === 0 ? (
              <tr>
                <td colSpan={4} className="px-3 py-4 text-text-muted">
                  {selectedProjectId
                    ? 'No metric labels configured for this project yet.'
                    : 'Select a project to manage metric labels.'}
                </td>
              </tr>
            ) : (
              aliases.map((alias) => (
                <tr key={alias.metric_name} className="border-t border-border">
                  <td className="px-3 py-2 font-mono text-text-primary">{alias.metric_name}</td>
                  <td className="px-3 py-2 text-text-secondary">{alias.display_name}</td>
                  <td className="px-3 py-2 text-text-secondary">{alias.updated_at || '—'}</td>
                  <td className="px-3 py-2">
                    <button
                      type="button"
                      onClick={() => handleDeleteAlias(alias.metric_name)}
                      disabled={deletingAliasMetricName === alias.metric_name}
                      className="px-2.5 py-1.5 rounded-md border border-border text-xs font-medium text-warning hover:bg-surface-secondary transition-colors disabled:opacity-60 disabled:cursor-not-allowed"
                    >
                      {deletingAliasMetricName === alias.metric_name ? 'Deleting...' : 'Delete'}
                    </button>
                  </td>
                </tr>
              ))
            )}
          </tbody>
        </table>
      </div>
    </section>
  );
}
