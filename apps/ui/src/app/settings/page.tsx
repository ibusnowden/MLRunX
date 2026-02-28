'use client';

import { useRouter, useSearchParams } from 'next/navigation';
import { Suspense, useCallback, useEffect, useMemo, useState } from 'react';
import {
  ApiKeyInfo,
  CreateApiKeyResponse,
  MetricAlias,
  ProjectInfo,
  UiAuthSessionResult,
  api,
} from '@/lib/api';
import { ProjectsTab } from './ProjectsTab';
import { ApiKeysTab } from './ApiKeysTab';
import { QuickstartTab } from './QuickstartTab';
import { MetricAliasesTab } from './MetricAliasesTab';

type TabId = 'projects' | 'metric-aliases' | 'api-keys' | 'quickstart';
type KeyCreationMode = 'recommended' | 'advanced';

const DAY_SECONDS = 24 * 60 * 60;
const DEFAULT_UI_KEY_MAX_TTL_SECONDS = 90 * DAY_SECONDS;
const DEFAULT_RECOMMENDED_KEY_NAME = 'sdk-agent';

const TABS: { id: TabId; label: string }[] = [
  { id: 'projects', label: 'Projects' },
  { id: 'metric-aliases', label: 'Metrics' },
  { id: 'api-keys', label: 'API Keys' },
  { id: 'quickstart', label: 'Quickstart' },
];

function SettingsContent() {
  const router = useRouter();
  const searchParams = useSearchParams();
  const activeTab = (TABS.find((t) => t.id === searchParams.get('tab'))?.id ?? 'projects') as TabId;

  const setActiveTab = useCallback(
    (tab: TabId) => {
      const params = new URLSearchParams(searchParams.toString());
      params.set('tab', tab);
      router.replace(`/settings?${params.toString()}`);
    },
    [router, searchParams],
  );

  const [apiBaseUrl, setApiBaseUrl] = useState('');
  const [session, setSession] = useState<UiAuthSessionResult | null>(null);
  const [projects, setProjects] = useState<ProjectInfo[]>([]);
  const [keys, setKeys] = useState<ApiKeyInfo[]>([]);
  const [newProjectName, setNewProjectName] = useState('');
  const [newProjectDescription, setNewProjectDescription] = useState('');
  const [metricAliases, setMetricAliases] = useState<MetricAlias[]>([]);
  const [selectedAliasProjectId, setSelectedAliasProjectId] = useState('');
  const [newAliasMetricName, setNewAliasMetricName] = useState('');
  const [newAliasDisplayName, setNewAliasDisplayName] = useState('');
  const [newKeyName, setNewKeyName] = useState(DEFAULT_RECOMMENDED_KEY_NAME);
  const [newKeyProjectId, setNewKeyProjectId] = useState('');
  const [keyTtlDays, setKeyTtlDays] = useState('30');
  const [scopeRead, setScopeRead] = useState(true);
  const [scopeWrite, setScopeWrite] = useState(true);
  const [createdKey, setCreatedKey] = useState<CreateApiKeyResponse | null>(null);
  const [loadingProjects, setLoadingProjects] = useState(false);
  const [loadingAliases, setLoadingAliases] = useState(false);
  const [loadingKeys, setLoadingKeys] = useState(false);
  const [submittingProject, setSubmittingProject] = useState(false);
  const [submittingAlias, setSubmittingAlias] = useState(false);
  const [deletingProjectId, setDeletingProjectId] = useState<string | null>(null);
  const [deletingAliasMetricName, setDeletingAliasMetricName] = useState<string | null>(null);
  const [submittingKey, setSubmittingKey] = useState(false);
  const [status, setStatus] = useState<string | null>(null);

  const refreshKeys = useCallback(async () => {
    if (!session) {
      setKeys([]);
      return;
    }

    setLoadingKeys(true);
    try {
      const result = await api.listApiKeys();
      setKeys(result.keys);
    } catch (error) {
      const message = error instanceof Error ? error.message : 'Failed to load API keys.';
      setStatus(message);
    } finally {
      setLoadingKeys(false);
    }
  }, [session]);

  const refreshProjects = useCallback(async () => {
    if (!session) {
      setProjects([]);
      return;
    }

    setLoadingProjects(true);
    try {
      const result = await api.listProjects();
      setProjects(result.projects);
    } catch (error) {
      const message = error instanceof Error ? error.message : 'Failed to load projects.';
      setStatus(message);
    } finally {
      setLoadingProjects(false);
    }
  }, [session]);

  const refreshSession = useCallback(async () => {
    try {
      const sessionResult = await api.getUiSession();
      setSession(sessionResult);
      setStatus(null);
    } catch {
      setSession(null);
      setProjects([]);
      setMetricAliases([]);
      setKeys([]);
    }
  }, []);

  const refreshAliases = useCallback(async () => {
    if (!session || !selectedAliasProjectId) {
      setMetricAliases([]);
      return;
    }

    setLoadingAliases(true);
    try {
      const result = await api.listMetricAliases(selectedAliasProjectId);
      setMetricAliases(result.aliases);
    } catch (error) {
      const message = error instanceof Error ? error.message : 'Failed to load metric labels.';
      setStatus(message);
    } finally {
      setLoadingAliases(false);
    }
  }, [selectedAliasProjectId, session]);

  useEffect(() => {
    if (typeof window !== 'undefined') {
      setApiBaseUrl(window.location.origin);
    }
    void refreshSession();
  }, [refreshSession]);

  const availableProjects = useMemo(() => {
    if (projects.length > 0) return projects;
    return (session?.project_ids || []).map((projectId) => ({
      project_id: projectId,
      name: projectId,
      description: null,
      created_at: '',
      updated_at: '',
    }));
  }, [projects, session]);

  const availableProjectIds = useMemo(
    () => availableProjects.map((project) => project.project_id),
    [availableProjects],
  );

  const keyPolicyMaxTtlSeconds = session?.ui_key_max_ttl_seconds ?? DEFAULT_UI_KEY_MAX_TTL_SECONDS;
  const keyPolicyMaxTtlDays = useMemo(
    () => Math.max(1, Math.floor(keyPolicyMaxTtlSeconds / DAY_SECONDS)),
    [keyPolicyMaxTtlSeconds],
  );
  const recommendedKeyTtlDays = useMemo(
    () => Math.min(90, keyPolicyMaxTtlDays),
    [keyPolicyMaxTtlDays],
  );
  const keyTtlOptionsDays = useMemo(() => {
    const base = [7, 30, 60, 90].filter((days) => days <= keyPolicyMaxTtlDays);
    if (!base.includes(keyPolicyMaxTtlDays)) {
      base.push(keyPolicyMaxTtlDays);
    }
    return Array.from(new Set(base)).sort((a, b) => a - b);
  }, [keyPolicyMaxTtlDays]);

  const projectNameById = useMemo(() => {
    const index = new Map<string, string>();
    for (const project of availableProjects) {
      index.set(project.project_id, project.name);
    }
    return index;
  }, [availableProjects]);

  const formatProjectRef = (projectId: string | null) => {
    if (!projectId) return 'global';
    const name = projectNameById.get(projectId);
    if (!name) return projectId;
    if (name === projectId) return projectId;
    return `${name} (${projectId})`;
  };

  useEffect(() => {
    if (!session) {
      setNewKeyProjectId('');
      return;
    }

    if (availableProjectIds.length === 1) {
      setNewKeyProjectId(availableProjectIds[0]);
      return;
    }

    if (!availableProjectIds.includes(newKeyProjectId)) {
      setNewKeyProjectId('');
    }
  }, [availableProjectIds, newKeyProjectId, session]);

  useEffect(() => {
    if (!session) {
      setSelectedAliasProjectId('');
      setMetricAliases([]);
      return;
    }

    if (availableProjectIds.length === 1) {
      setSelectedAliasProjectId(availableProjectIds[0]);
      return;
    }

    if (!availableProjectIds.includes(selectedAliasProjectId)) {
      setSelectedAliasProjectId(availableProjectIds[0] ?? '');
    }
  }, [availableProjectIds, selectedAliasProjectId, session]);

  useEffect(() => {
    if (!session) return;
    const selectedTtlDays = Number(keyTtlDays);
    if (
      !Number.isFinite(selectedTtlDays)
      || selectedTtlDays <= 0
      || selectedTtlDays > keyPolicyMaxTtlDays
    ) {
      setKeyTtlDays(String(recommendedKeyTtlDays));
    }
  }, [session, keyPolicyMaxTtlDays, keyTtlDays, recommendedKeyTtlDays]);

  useEffect(() => {
    if (session) {
      void Promise.all([refreshKeys(), refreshProjects()]);
    }
  }, [session, refreshKeys, refreshProjects]);

  useEffect(() => {
    if (session && selectedAliasProjectId) {
      void refreshAliases();
    }
  }, [refreshAliases, selectedAliasProjectId, session]);

  const sdkApiKey = useMemo(() => createdKey?.api_key || '<paste-api-key>', [createdKey]);
  const sdkProjectId = useMemo(() => {
    if (newKeyProjectId) return newKeyProjectId;
    if (availableProjectIds.length === 1) return availableProjectIds[0];
    return '<project-id>';
  }, [availableProjectIds, newKeyProjectId]);
  const pipCommand = useMemo(
    () =>
      `pip install --upgrade mlrunx\nexport MLRUNX_SERVER_URL=${apiBaseUrl || 'https://mlrunx.example.com'}\nexport MLRUNX_API_KEY=${sdkApiKey}\nexport MLRUNX_PROJECT_ID=${sdkProjectId}`,
    [apiBaseUrl, sdkApiKey, sdkProjectId],
  );
  const uvCommand = useMemo(
    () =>
      `uv pip install --upgrade mlrunx\nMLRUNX_SERVER_URL=${apiBaseUrl || 'https://mlrunx.example.com'} MLRUNX_API_KEY=${sdkApiKey} MLRUNX_PROJECT_ID=${sdkProjectId} python train.py`,
    [apiBaseUrl, sdkApiKey, sdkProjectId],
  );

  const copyToClipboard = async (value: string, successMessage: string) => {
    try {
      await navigator.clipboard.writeText(value);
      setStatus(successMessage);
    } catch {
      setStatus('Copy failed. Clipboard permission may be blocked.');
    }
  };

  const handleCreateProject = async () => {
    if (!session) {
      setStatus('Sign in before creating projects.');
      return;
    }

    const name = newProjectName.trim();
    if (!name) {
      setStatus('Project name is required.');
      return;
    }

    setSubmittingProject(true);
    try {
      const result = await api.createProject({
        name,
        description: newProjectDescription.trim() || undefined,
      });
      setNewProjectName('');
      setNewProjectDescription('');
      setNewKeyProjectId(result.project_id);
      await Promise.all([refreshSession(), refreshProjects()]);
      setStatus(`Created project ${result.name} (${result.project_id}).`);
    } catch (error) {
      const message = error instanceof Error ? error.message : 'Failed to create project.';
      setStatus(message);
    } finally {
      setSubmittingProject(false);
    }
  };

  const handleDeleteProject = async (project: ProjectInfo) => {
    if (!session) {
      setStatus('Sign in before deleting projects.');
      return;
    }

    const confirmed = window.confirm(
      `Delete project '${project.name}' (${project.project_id}) and all runs/keys in it? This cannot be undone.`,
    );
    if (!confirmed) return;

    setDeletingProjectId(project.project_id);
    try {
      await api.deleteProject(project.project_id);
      if (newKeyProjectId === project.project_id) {
        setNewKeyProjectId('');
      }
      if (selectedAliasProjectId === project.project_id) {
        setSelectedAliasProjectId('');
        setMetricAliases([]);
      }
      await Promise.all([refreshSession(), refreshProjects(), refreshKeys()]);
      setStatus(`Deleted project ${project.name} (${project.project_id}).`);
    } catch (error) {
      const message = error instanceof Error ? error.message : 'Failed to delete project.';
      setStatus(message);
    } finally {
      setDeletingProjectId(null);
    }
  };

  const handleCreateKey = async (mode: KeyCreationMode) => {
    if (!session) {
      setStatus('Sign in before creating API keys.');
      return;
    }

    if (!newKeyProjectId) {
      setStatus('Choose a project for this key.');
      return;
    }

    const keyName =
      mode === 'recommended'
        ? DEFAULT_RECOMMENDED_KEY_NAME
        : (newKeyName.trim() || DEFAULT_RECOMMENDED_KEY_NAME);

    const scopes: string[] = mode === 'recommended'
      ? ['read', 'write']
      : [
          ...(scopeRead ? ['read'] : []),
          ...(scopeWrite ? ['write'] : []),
        ];
    if (scopes.length === 0) {
      setStatus('Choose at least one scope.');
      return;
    }

    const rawTtlDays = mode === 'recommended' ? recommendedKeyTtlDays : Number(keyTtlDays);
    if (!Number.isFinite(rawTtlDays) || rawTtlDays <= 0) {
      setStatus('Choose a valid key TTL.');
      return;
    }
    const ttlDays = Math.min(Math.floor(rawTtlDays), keyPolicyMaxTtlDays);
    const expiresInSeconds = ttlDays * DAY_SECONDS;

    setSubmittingKey(true);
    try {
      const payload = {
        project_id: newKeyProjectId,
        name: keyName,
        scopes,
        expires_in_seconds: expiresInSeconds,
      };
      const result = await api.createApiKey(payload);
      setCreatedKey(result);
      await refreshKeys();
      if (mode === 'recommended') {
        setStatus(
          `Created SDK key ${result.key_prefix} (${ttlDays} days). Copy it now (shown once).`,
        );
      } else {
        setStatus(`Created key ${result.key_prefix}. Copy it now (shown once).`);
      }
    } catch (error) {
      const message = error instanceof Error ? error.message : 'Failed to create API key.';
      setStatus(message);
    } finally {
      setSubmittingKey(false);
    }
  };

  const handleRevokeKey = async (keyId: string) => {
    try {
      await api.revokeApiKey(keyId);
      await refreshKeys();
      setStatus(`Revoked key ${keyId}.`);
    } catch (error) {
      const message = error instanceof Error ? error.message : 'Failed to revoke API key.';
      setStatus(message);
    }
  };

  const handleUpsertAlias = async () => {
    if (!session) {
      setStatus('Sign in before updating metric labels.');
      return;
    }
    if (!selectedAliasProjectId) {
      setStatus('Choose a project first.');
      return;
    }

    const metricName = newAliasMetricName.trim();
    const displayName = newAliasDisplayName.trim();
    if (!metricName || !displayName) {
      setStatus('Both raw metric key and display label are required.');
      return;
    }

    setSubmittingAlias(true);
    try {
      await api.upsertMetricAlias(selectedAliasProjectId, {
        metric_name: metricName,
        display_name: displayName,
      });
      setNewAliasMetricName('');
      setNewAliasDisplayName('');
      await refreshAliases();
      setStatus(`Saved metric label '${displayName}' for '${metricName}'.`);
    } catch (error) {
      const message = error instanceof Error ? error.message : 'Failed to save metric label.';
      setStatus(message);
    } finally {
      setSubmittingAlias(false);
    }
  };

  const handleDeleteAlias = async (metricName: string) => {
    if (!selectedAliasProjectId) {
      setStatus('Choose a project first.');
      return;
    }

    setDeletingAliasMetricName(metricName);
    try {
      await api.deleteMetricAlias(selectedAliasProjectId, metricName);
      await refreshAliases();
      setStatus(`Deleted metric label for '${metricName}'.`);
    } catch (error) {
      const message = error instanceof Error ? error.message : 'Failed to delete metric label.';
      setStatus(message);
    } finally {
      setDeletingAliasMetricName(null);
    }
  };

  return (
    <main className="min-h-screen">
      <div className="border-b border-border bg-surface">
        <div className="max-w-5xl mx-auto px-4 sm:px-6 py-5 sm:py-6">
          <h1 className="text-2xl font-bold text-text-primary">Settings</h1>
          <p className="text-sm text-text-secondary mt-1">Projects, metric labels, API keys, and SDK quickstart.</p>
        </div>
      </div>

      <div className="border-b border-border bg-surface">
        <div className="max-w-5xl mx-auto px-4 sm:px-6">
          <nav className="flex gap-0 -mb-px" aria-label="Settings tabs">
            {TABS.map((tab) => (
              <button
                key={tab.id}
                type="button"
                onClick={() => setActiveTab(tab.id)}
                className={`px-4 py-3 text-sm font-medium border-b-2 transition-colors ${
                  activeTab === tab.id
                    ? 'border-accent text-accent'
                    : 'border-transparent text-text-secondary hover:text-text-primary hover:border-border'
                }`}
              >
                {tab.label}
              </button>
            ))}
          </nav>
        </div>
      </div>

      <div className="max-w-5xl mx-auto px-4 sm:px-6 py-5 sm:py-6 space-y-4">
        {status && (
          <div className="rounded-lg border border-border bg-surface px-4 py-3 text-sm text-text-secondary">{status}</div>
        )}

        {activeTab === 'projects' && (
          <ProjectsTab
            session={session}
            availableProjects={availableProjects}
            newProjectName={newProjectName}
            setNewProjectName={setNewProjectName}
            newProjectDescription={newProjectDescription}
            setNewProjectDescription={setNewProjectDescription}
            submittingProject={submittingProject}
            loadingProjects={loadingProjects}
            deletingProjectId={deletingProjectId}
            handleCreateProject={() => void handleCreateProject()}
            handleDeleteProject={(project) => void handleDeleteProject(project)}
            refreshProjects={() => void refreshProjects()}
          />
        )}

        {activeTab === 'api-keys' && (
          <ApiKeysTab
            session={session}
            availableProjects={availableProjects}
            newKeyName={newKeyName}
            setNewKeyName={setNewKeyName}
            newKeyProjectId={newKeyProjectId}
            setNewKeyProjectId={setNewKeyProjectId}
            scopeRead={scopeRead}
            setScopeRead={setScopeRead}
            scopeWrite={scopeWrite}
            setScopeWrite={setScopeWrite}
            keyTtlDays={keyTtlDays}
            setKeyTtlDays={setKeyTtlDays}
            submittingKey={submittingKey}
            loadingKeys={loadingKeys}
            createdKey={createdKey}
            keys={keys}
            keyPolicyMaxTtlSeconds={keyPolicyMaxTtlSeconds}
            recommendedKeyTtlDays={recommendedKeyTtlDays}
            keyTtlOptionsDays={keyTtlOptionsDays}
            formatProjectRef={formatProjectRef}
            handleCreateKey={(mode) => void handleCreateKey(mode)}
            handleRevokeKey={(keyId) => void handleRevokeKey(keyId)}
            refreshKeys={() => void refreshKeys()}
            copyToClipboard={(value, msg) => void copyToClipboard(value, msg)}
          />
        )}

        {activeTab === 'metric-aliases' && (
          <MetricAliasesTab
            session={session}
            availableProjects={availableProjects}
            selectedProjectId={selectedAliasProjectId}
            setSelectedProjectId={setSelectedAliasProjectId}
            newAliasMetricName={newAliasMetricName}
            setNewAliasMetricName={setNewAliasMetricName}
            newAliasDisplayName={newAliasDisplayName}
            setNewAliasDisplayName={setNewAliasDisplayName}
            aliases={metricAliases}
            loadingAliases={loadingAliases}
            submittingAlias={submittingAlias}
            deletingAliasMetricName={deletingAliasMetricName}
            formatProjectRef={formatProjectRef}
            handleUpsertAlias={() => void handleUpsertAlias()}
            handleDeleteAlias={(metricName) => void handleDeleteAlias(metricName)}
            refreshAliases={() => void refreshAliases()}
          />
        )}

        {activeTab === 'quickstart' && (
          <QuickstartTab
            apiBaseUrl={apiBaseUrl}
            pipCommand={pipCommand}
            uvCommand={uvCommand}
            copyToClipboard={(value, msg) => void copyToClipboard(value, msg)}
          />
        )}
      </div>
    </main>
  );
}

export default function SettingsPage() {
  return (
    <Suspense
      fallback={
        <main className="min-h-screen">
          <div className="border-b border-border bg-surface">
            <div className="max-w-5xl mx-auto px-4 sm:px-6 py-5 sm:py-6">
              <h1 className="text-2xl font-bold text-text-primary">Settings</h1>
              <p className="text-sm text-text-secondary mt-1">Projects, metric labels, API keys, and SDK quickstart.</p>
            </div>
          </div>
          <div className="max-w-5xl mx-auto px-4 sm:px-6 py-5 sm:py-6">
            <div className="rounded-lg border border-border bg-surface px-4 py-3 text-sm text-text-secondary">
              Loading...
            </div>
          </div>
        </main>
      }
    >
      <SettingsContent />
    </Suspense>
  );
}
