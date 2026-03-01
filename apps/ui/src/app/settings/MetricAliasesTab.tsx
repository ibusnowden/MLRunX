import { MetricAlias, ProjectInfo, UiAuthSessionResult } from '@/lib/api';

type AliasFormErrors = {
  raw_name?: string;
  display_name?: string;
  unit?: string;
  description?: string;
};

type MetricAliasImportPreviewRow = {
  rowNumber: number;
  rawName: string;
  displayName: string;
  unit: string;
  description: string;
  isActive: boolean;
  action: 'create' | 'update' | 'deactivate' | 'reactivate' | 'noop' | 'error';
  error?: string;
};

function aliasRawName(alias: MetricAlias): string {
  return (alias.raw_name || alias.metric_name || '').trim();
}

function actionLabel(action: MetricAliasImportPreviewRow['action']): string {
  if (action === 'create') return 'Create';
  if (action === 'update') return 'Update';
  if (action === 'deactivate') return 'Deactivate';
  if (action === 'reactivate') return 'Reactivate';
  if (action === 'noop') return 'No Change';
  return 'Error';
}

export function MetricAliasesTab({
  session,
  availableProjects,
  selectedProjectId,
  setSelectedProjectId,
  newAliasRawName,
  setNewAliasRawName,
  newAliasDisplayName,
  setNewAliasDisplayName,
  newAliasUnit,
  setNewAliasUnit,
  newAliasDescription,
  setNewAliasDescription,
  newAliasActive,
  setNewAliasActive,
  editingAliasRawName,
  aliasFormErrors,
  aliases,
  aliasImportPreview,
  applyingAliasImport,
  loadingAliases,
  submittingAlias,
  deletingAliasMetricName,
  formatProjectRef,
  handleUpsertAlias,
  handleStartEditAlias,
  handleResetAliasForm,
  handleDeactivateAlias,
  handleReactivateAlias,
  handleAliasImportFile,
  handleApplyAliasImport,
  handleClearAliasImportPreview,
  handleDeleteAlias,
  refreshAliases,
}: {
  session: UiAuthSessionResult | null;
  availableProjects: ProjectInfo[];
  selectedProjectId: string;
  setSelectedProjectId: (v: string) => void;
  newAliasRawName: string;
  setNewAliasRawName: (v: string) => void;
  newAliasDisplayName: string;
  setNewAliasDisplayName: (v: string) => void;
  newAliasUnit: string;
  setNewAliasUnit: (v: string) => void;
  newAliasDescription: string;
  setNewAliasDescription: (v: string) => void;
  newAliasActive: boolean;
  setNewAliasActive: (v: boolean) => void;
  editingAliasRawName: string | null;
  aliasFormErrors: AliasFormErrors;
  aliases: MetricAlias[];
  aliasImportPreview: MetricAliasImportPreviewRow[];
  applyingAliasImport: boolean;
  loadingAliases: boolean;
  submittingAlias: boolean;
  deletingAliasMetricName: string | null;
  formatProjectRef: (projectId: string | null) => string;
  handleUpsertAlias: () => void;
  handleStartEditAlias: (alias: MetricAlias) => void;
  handleResetAliasForm: () => void;
  handleDeactivateAlias: (alias: MetricAlias) => void;
  handleReactivateAlias: (alias: MetricAlias) => void;
  handleAliasImportFile: (file: File | null) => void;
  handleApplyAliasImport: () => void;
  handleClearAliasImportPreview: () => void;
  handleDeleteAlias: (metricName: string) => void;
  refreshAliases: () => void;
}) {
  const hasImportErrors = aliasImportPreview.some((row) => row.action === 'error');
  const actionableImportCount = aliasImportPreview.filter(
    (row) => row.action !== 'error' && row.action !== 'noop'
  ).length;

  return (
    <section className="rounded-xl border border-border bg-surface p-4 sm:p-5 space-y-4">
      <div>
        <h2 className="text-lg font-semibold text-text-primary">Metric Naming</h2>
        <p className="mt-1 text-sm text-text-secondary">
          Define project-level metric aliases, units, and descriptions without rewriting historical points.
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
              value={newAliasRawName}
              onChange={(event) => setNewAliasRawName(event.target.value)}
              placeholder="train/loss"
              className="w-full rounded-lg border border-border bg-surface px-3 py-2.5 text-sm text-text-primary placeholder:text-text-muted focus:outline-none focus:ring-2 focus:ring-accent focus:border-transparent"
            />
            {aliasFormErrors.raw_name && (
              <p className="mt-1 text-xs text-danger">{aliasFormErrors.raw_name}</p>
            )}
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
            {aliasFormErrors.display_name && (
              <p className="mt-1 text-xs text-danger">{aliasFormErrors.display_name}</p>
            )}
          </div>
        </div>

        <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
          <div>
            <label htmlFor="metric-alias-unit" className="block text-sm font-medium text-text-primary mb-1.5">
              Unit (optional)
            </label>
            <input
              id="metric-alias-unit"
              type="text"
              value={newAliasUnit}
              onChange={(event) => setNewAliasUnit(event.target.value)}
              placeholder="%"
              className="w-full rounded-lg border border-border bg-surface px-3 py-2.5 text-sm text-text-primary placeholder:text-text-muted focus:outline-none focus:ring-2 focus:ring-accent focus:border-transparent"
            />
            {aliasFormErrors.unit && (
              <p className="mt-1 text-xs text-danger">{aliasFormErrors.unit}</p>
            )}
          </div>
          <div>
            <label htmlFor="metric-alias-description" className="block text-sm font-medium text-text-primary mb-1.5">
              Description (optional)
            </label>
            <input
              id="metric-alias-description"
              type="text"
              value={newAliasDescription}
              onChange={(event) => setNewAliasDescription(event.target.value)}
              placeholder="Short definition for dashboards"
              className="w-full rounded-lg border border-border bg-surface px-3 py-2.5 text-sm text-text-primary placeholder:text-text-muted focus:outline-none focus:ring-2 focus:ring-accent focus:border-transparent"
            />
            {aliasFormErrors.description && (
              <p className="mt-1 text-xs text-danger">{aliasFormErrors.description}</p>
            )}
          </div>
        </div>

        <label className="inline-flex items-center gap-2 text-sm text-text-secondary">
          <input
            type="checkbox"
            checked={newAliasActive}
            onChange={(event) => setNewAliasActive(event.target.checked)}
            className="rounded border-border bg-surface"
          />
          Active alias
        </label>

        <div className="flex flex-wrap gap-2">
          <button
            type="button"
            onClick={handleUpsertAlias}
            disabled={!session || !selectedProjectId || submittingAlias}
            className="px-4 py-2.5 rounded-lg bg-accent text-white text-sm font-medium hover:bg-accent-hover transition-colors disabled:opacity-60 disabled:cursor-not-allowed"
          >
            {submittingAlias ? 'Saving...' : editingAliasRawName ? 'Update Alias' : 'Save Alias'}
          </button>
          <button
            type="button"
            onClick={handleResetAliasForm}
            className="px-4 py-2.5 rounded-lg border border-border text-sm font-medium text-text-secondary hover:bg-surface hover:text-text-primary transition-colors"
          >
            {editingAliasRawName ? 'Cancel Edit' : 'Reset'}
          </button>
          <button
            type="button"
            onClick={refreshAliases}
            disabled={!session || !selectedProjectId || loadingAliases}
            className="px-4 py-2.5 rounded-lg border border-border text-sm font-medium text-text-secondary hover:bg-surface hover:text-text-primary transition-colors disabled:opacity-60 disabled:cursor-not-allowed"
          >
            {loadingAliases ? 'Refreshing...' : 'Refresh Aliases'}
          </button>
        </div>
      </div>

      <div className="rounded-lg border border-border bg-surface-secondary p-3 space-y-3">
        <div>
          <h3 className="text-sm font-semibold text-text-primary">Bulk Import CSV</h3>
          <p className="mt-1 text-xs text-text-muted">
            Headers: <code>raw_name</code> (or <code>metric_name</code>), <code>display_name</code>,
            optional <code>unit</code>, <code>description</code>, <code>is_active</code>.
          </p>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <input
            type="file"
            accept=".csv,text/csv"
            onChange={(event) => handleAliasImportFile(event.target.files?.[0] ?? null)}
            className="max-w-full text-xs text-text-secondary"
          />
          <button
            type="button"
            onClick={handleApplyAliasImport}
            disabled={applyingAliasImport || actionableImportCount === 0 || hasImportErrors}
            className="px-3 py-2 rounded-lg bg-accent text-white text-xs font-medium hover:bg-accent-hover transition-colors disabled:opacity-60 disabled:cursor-not-allowed"
          >
            {applyingAliasImport ? 'Applying...' : `Apply ${actionableImportCount} Change(s)`}
          </button>
          <button
            type="button"
            onClick={handleClearAliasImportPreview}
            disabled={aliasImportPreview.length === 0}
            className="px-3 py-2 rounded-lg border border-border text-xs font-medium text-text-secondary hover:bg-surface hover:text-text-primary transition-colors disabled:opacity-60 disabled:cursor-not-allowed"
          >
            Clear Preview
          </button>
        </div>

        {aliasImportPreview.length > 0 && (
          <div className="overflow-x-auto rounded-lg border border-border bg-surface">
            <table className="w-full text-left text-xs">
              <thead className="bg-surface-secondary text-text-secondary">
                <tr>
                  <th className="px-2.5 py-2 font-medium">Row</th>
                  <th className="px-2.5 py-2 font-medium">Raw Metric</th>
                  <th className="px-2.5 py-2 font-medium">Display</th>
                  <th className="px-2.5 py-2 font-medium">Active</th>
                  <th className="px-2.5 py-2 font-medium">Action</th>
                  <th className="px-2.5 py-2 font-medium">Notes</th>
                </tr>
              </thead>
              <tbody>
                {aliasImportPreview.map((row) => (
                  <tr key={`${row.rowNumber}-${row.rawName}`} className="border-t border-border">
                    <td className="px-2.5 py-1.5 text-text-muted">{row.rowNumber}</td>
                    <td className="px-2.5 py-1.5 font-mono text-text-primary">{row.rawName}</td>
                    <td className="px-2.5 py-1.5 text-text-secondary">{row.displayName}</td>
                    <td className="px-2.5 py-1.5 text-text-secondary">{row.isActive ? 'yes' : 'no'}</td>
                    <td className={`px-2.5 py-1.5 ${
                      row.action === 'error'
                        ? 'text-danger'
                        : row.action === 'noop'
                          ? 'text-text-muted'
                          : 'text-success'
                    }`}>
                      {actionLabel(row.action)}
                    </td>
                    <td className="px-2.5 py-1.5 text-text-secondary">{row.error || '—'}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </div>

      <div className="overflow-x-auto rounded-lg border border-border">
        <table className="w-full text-left text-sm">
          <thead className="bg-surface-secondary text-text-secondary">
            <tr>
              <th className="px-3 py-2 font-medium">Raw Metric</th>
              <th className="px-3 py-2 font-medium">Display Label</th>
              <th className="px-3 py-2 font-medium">Unit</th>
              <th className="px-3 py-2 font-medium">Status</th>
              <th className="px-3 py-2 font-medium">Updated</th>
              <th className="px-3 py-2 font-medium">Actions</th>
            </tr>
          </thead>
          <tbody>
            {aliases.length === 0 ? (
              <tr>
                <td colSpan={6} className="px-3 py-4 text-text-muted">
                  {selectedProjectId
                    ? 'No metric aliases configured for this project yet.'
                    : 'Select a project to manage metric aliases.'}
                </td>
              </tr>
            ) : (
              aliases.map((alias) => {
                const rawName = aliasRawName(alias);
                const isActive = alias.is_active !== false;
                return (
                  <tr key={rawName} className="border-t border-border">
                    <td className="px-3 py-2 font-mono text-text-primary">{rawName}</td>
                    <td className="px-3 py-2 text-text-secondary">{alias.display_name}</td>
                    <td className="px-3 py-2 text-text-secondary">{alias.unit || '—'}</td>
                    <td className="px-3 py-2 text-text-secondary">{isActive ? 'active' : 'inactive'}</td>
                    <td className="px-3 py-2 text-text-secondary">{alias.updated_at || '—'}</td>
                    <td className="px-3 py-2">
                      <div className="flex flex-wrap gap-1.5">
                        <button
                          type="button"
                          onClick={() => handleStartEditAlias(alias)}
                          className="px-2.5 py-1.5 rounded-md border border-border text-xs font-medium text-text-secondary hover:bg-surface-secondary hover:text-text-primary transition-colors"
                        >
                          Edit
                        </button>
                        {isActive ? (
                          <button
                            type="button"
                            onClick={() => handleDeactivateAlias(alias)}
                            disabled={deletingAliasMetricName === rawName}
                            className="px-2.5 py-1.5 rounded-md border border-warning/40 text-xs font-medium text-warning hover:bg-warning/10 transition-colors disabled:opacity-60 disabled:cursor-not-allowed"
                          >
                            {deletingAliasMetricName === rawName ? 'Working...' : 'Deactivate'}
                          </button>
                        ) : (
                          <button
                            type="button"
                            onClick={() => handleReactivateAlias(alias)}
                            disabled={deletingAliasMetricName === rawName}
                            className="px-2.5 py-1.5 rounded-md border border-success/40 text-xs font-medium text-success hover:bg-success/10 transition-colors disabled:opacity-60 disabled:cursor-not-allowed"
                          >
                            {deletingAliasMetricName === rawName ? 'Working...' : 'Reactivate'}
                          </button>
                        )}
                        <button
                          type="button"
                          onClick={() => handleDeleteAlias(rawName)}
                          disabled={deletingAliasMetricName === rawName}
                          className="px-2.5 py-1.5 rounded-md border border-danger/40 text-xs font-medium text-danger hover:bg-danger/10 transition-colors disabled:opacity-60 disabled:cursor-not-allowed"
                        >
                          {deletingAliasMetricName === rawName ? 'Deleting...' : 'Delete'}
                        </button>
                      </div>
                    </td>
                  </tr>
                );
              })
            )}
          </tbody>
        </table>
      </div>
    </section>
  );
}
