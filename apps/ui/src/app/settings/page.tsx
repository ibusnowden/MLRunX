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
const MAX_ALIAS_RAW_NAME_LEN = 256;
const MAX_ALIAS_DISPLAY_NAME_LEN = 128;
const MAX_ALIAS_UNIT_LEN = 32;
const MAX_ALIAS_DESCRIPTION_LEN = 512;

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

function getMetricAliasRawName(alias: MetricAlias): string {
  return (alias.raw_name || alias.metric_name || '').trim();
}

function parseCsvLine(line: string): string[] {
  const values: string[] = [];
  let current = '';
  let inQuotes = false;
  for (let idx = 0; idx < line.length; idx += 1) {
    const char = line[idx];
    if (char === '"') {
      const next = line[idx + 1];
      if (inQuotes && next === '"') {
        current += '"';
        idx += 1;
      } else {
        inQuotes = !inQuotes;
      }
      continue;
    }
    if (char === ',' && !inQuotes) {
      values.push(current.trim());
      current = '';
      continue;
    }
    current += char;
  }
  values.push(current.trim());
  return values;
}

function normalizeAliasBool(value: string): boolean | null {
  const normalized = value.trim().toLowerCase();
  if (!normalized) return true;
  if (['1', 'true', 'yes', 'y', 'active'].includes(normalized)) return true;
  if (['0', 'false', 'no', 'n', 'inactive'].includes(normalized)) return false;
  return null;
}

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
  const [newAliasRawName, setNewAliasRawName] = useState('');
  const [newAliasDisplayName, setNewAliasDisplayName] = useState('');
  const [newAliasUnit, setNewAliasUnit] = useState('');
  const [newAliasDescription, setNewAliasDescription] = useState('');
  const [newAliasActive, setNewAliasActive] = useState(true);
  const [editingAliasRawName, setEditingAliasRawName] = useState<string | null>(null);
  const [aliasFormErrors, setAliasFormErrors] = useState<AliasFormErrors>({});
  const [aliasImportPreview, setAliasImportPreview] = useState<MetricAliasImportPreviewRow[]>([]);
  const [applyingAliasImport, setApplyingAliasImport] = useState(false);
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
      setEditingAliasRawName(null);
      setNewAliasRawName('');
      setNewAliasDisplayName('');
      setNewAliasUnit('');
      setNewAliasDescription('');
      setNewAliasActive(true);
      setAliasFormErrors({});
      setAliasImportPreview([]);
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

  const resetAliasForm = useCallback(() => {
    setEditingAliasRawName(null);
    setNewAliasRawName('');
    setNewAliasDisplayName('');
    setNewAliasUnit('');
    setNewAliasDescription('');
    setNewAliasActive(true);
    setAliasFormErrors({});
  }, []);

  const validateAliasForm = useCallback((
    rawName: string,
    displayName: string,
    unit: string,
    description: string,
  ): AliasFormErrors => {
    const nextErrors: AliasFormErrors = {};
    if (!rawName.trim()) nextErrors.raw_name = 'Raw metric key is required.';
    if (rawName.trim().length > MAX_ALIAS_RAW_NAME_LEN) {
      nextErrors.raw_name = `Raw metric key must be <= ${MAX_ALIAS_RAW_NAME_LEN} characters.`;
    }
    if (!displayName.trim()) nextErrors.display_name = 'Display label is required.';
    if (displayName.trim().length > MAX_ALIAS_DISPLAY_NAME_LEN) {
      nextErrors.display_name = `Display label must be <= ${MAX_ALIAS_DISPLAY_NAME_LEN} characters.`;
    }
    if (unit.trim().length > MAX_ALIAS_UNIT_LEN) {
      nextErrors.unit = `Unit must be <= ${MAX_ALIAS_UNIT_LEN} characters.`;
    }
    if (description.trim().length > MAX_ALIAS_DESCRIPTION_LEN) {
      nextErrors.description = `Description must be <= ${MAX_ALIAS_DESCRIPTION_LEN} characters.`;
    }
    return nextErrors;
  }, []);

  const handleUpsertAlias = async () => {
    if (!session) {
      setStatus('Sign in before updating metric labels.');
      return;
    }
    if (!selectedAliasProjectId) {
      setStatus('Choose a project first.');
      return;
    }

    const metricName = newAliasRawName.trim();
    const displayName = newAliasDisplayName.trim();
    const unit = newAliasUnit.trim();
    const description = newAliasDescription.trim();
    const formErrors = validateAliasForm(metricName, displayName, unit, description);
    setAliasFormErrors(formErrors);
    if (Object.keys(formErrors).length > 0) {
      setStatus('Please fix validation errors before saving.');
      return;
    }

    setSubmittingAlias(true);
    try {
      await api.upsertMetricAlias(selectedAliasProjectId, {
        raw_name: metricName,
        display_name: displayName,
        unit: unit || undefined,
        description: description || undefined,
        is_active: newAliasActive,
      });
      resetAliasForm();
      await refreshAliases();
      setStatus(`Saved metric label '${displayName}' for '${metricName}'.`);
    } catch (error) {
      const message = error instanceof Error ? error.message : 'Failed to save metric label.';
      setStatus(message);
    } finally {
      setSubmittingAlias(false);
    }
  };

  const handleStartEditAlias = (alias: MetricAlias) => {
    setEditingAliasRawName(getMetricAliasRawName(alias));
    setNewAliasRawName(getMetricAliasRawName(alias));
    setNewAliasDisplayName(alias.display_name || '');
    setNewAliasUnit((alias.unit || '').trim());
    setNewAliasDescription((alias.description || '').trim());
    setNewAliasActive(alias.is_active !== false);
    setAliasFormErrors({});
  };

  const handleDeactivateAlias = async (alias: MetricAlias) => {
    if (!selectedAliasProjectId) {
      setStatus('Choose a project first.');
      return;
    }

    const rawName = getMetricAliasRawName(alias);
    setDeletingAliasMetricName(rawName);
    try {
      await api.upsertMetricAlias(selectedAliasProjectId, {
        raw_name: rawName,
        display_name: alias.display_name,
        unit: (alias.unit || '').trim() || undefined,
        description: (alias.description || '').trim() || undefined,
        is_active: false,
      });
      await refreshAliases();
      setStatus(`Deactivated metric label for '${rawName}'.`);
    } catch (error) {
      const message = error instanceof Error ? error.message : 'Failed to deactivate metric label.';
      setStatus(message);
    } finally {
      setDeletingAliasMetricName(null);
    }
  };

  const handleReactivateAlias = async (alias: MetricAlias) => {
    if (!selectedAliasProjectId) {
      setStatus('Choose a project first.');
      return;
    }

    const rawName = getMetricAliasRawName(alias);
    setDeletingAliasMetricName(rawName);
    try {
      await api.upsertMetricAlias(selectedAliasProjectId, {
        raw_name: rawName,
        display_name: alias.display_name,
        unit: (alias.unit || '').trim() || undefined,
        description: (alias.description || '').trim() || undefined,
        is_active: true,
      });
      await refreshAliases();
      setStatus(`Reactivated metric label for '${rawName}'.`);
    } catch (error) {
      const message = error instanceof Error ? error.message : 'Failed to reactivate metric label.';
      setStatus(message);
    } finally {
      setDeletingAliasMetricName(null);
    }
  };

  const handleDeleteAlias = async (metricName: string) => {
    if (!selectedAliasProjectId) {
      setStatus('Choose a project first.');
      return;
    }

    const confirmed = window.confirm(`Delete alias '${metricName}' permanently?`);
    if (!confirmed) return;

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

  const handleAliasImportFile = async (file: File | null) => {
    if (!file) return;
    const content = await file.text();
    const lines = content
      .split(/\r?\n/)
      .map((line) => line.trim())
      .filter((line) => line.length > 0);

    if (lines.length === 0) {
      setAliasImportPreview([]);
      setStatus('CSV is empty.');
      return;
    }

    const headerCells = parseCsvLine(lines[0]).map((cell) => cell.trim().toLowerCase());
    const headers = new Map<string, number>();
    headerCells.forEach((header, index) => headers.set(header, index));

    const rawIdx = headers.get('raw_name') ?? headers.get('metric_name');
    const displayIdx = headers.get('display_name');
    const unitIdx = headers.get('unit');
    const descriptionIdx = headers.get('description');
    const activeIdx = headers.get('is_active');

    if (rawIdx === undefined || displayIdx === undefined) {
      setAliasImportPreview([]);
      setStatus("CSV must include 'raw_name' (or 'metric_name') and 'display_name' headers.");
      return;
    }

    const existingByRaw = new Map(metricAliases.map((alias) => [getMetricAliasRawName(alias), alias]));
    const activeDisplayToRaw = new Map<string, string>();
    metricAliases.forEach((alias) => {
      if (alias.is_active === false) return;
      const rawName = getMetricAliasRawName(alias);
      activeDisplayToRaw.set(alias.display_name.trim().toLowerCase(), rawName);
    });

    const previewRows: MetricAliasImportPreviewRow[] = [];
    for (let idx = 1; idx < lines.length; idx += 1) {
      const cells = parseCsvLine(lines[idx]);
      const rowNumber = idx + 1;
      const rawName = (cells[rawIdx] || '').trim();
      const displayName = (cells[displayIdx] || '').trim();
      const unit = (unitIdx !== undefined ? (cells[unitIdx] || '').trim() : '');
      const description = (descriptionIdx !== undefined ? (cells[descriptionIdx] || '').trim() : '');
      const parsedIsActive = activeIdx !== undefined
        ? normalizeAliasBool(cells[activeIdx] || '')
        : true;
      const isActive = parsedIsActive ?? true;

      const rowErrors = validateAliasForm(rawName, displayName, unit, description);
      let action: MetricAliasImportPreviewRow['action'] = 'create';
      let error = Object.values(rowErrors)[0];

      if (parsedIsActive === null) {
        action = 'error';
        error = "is_active must be true/false (or 1/0, yes/no).";
      }

      const existingAlias = existingByRaw.get(rawName);
      const displayKey = displayName.toLowerCase();
      const displayTakenBy = activeDisplayToRaw.get(displayKey);
      if (!error && isActive && displayTakenBy && displayTakenBy !== rawName) {
        action = 'error';
        error = `display_name '${displayName}' already used by active alias '${displayTakenBy}'.`;
      } else if (!error && existingAlias) {
        const existingActive = existingAlias.is_active !== false;
        const sameDisplay = existingAlias.display_name.trim() === displayName;
        const sameUnit = (existingAlias.unit || '').trim() === unit;
        const sameDescription = (existingAlias.description || '').trim() === description;
        if (existingActive === isActive && sameDisplay && sameUnit && sameDescription) {
          action = 'noop';
        } else if (!existingActive && isActive) {
          action = 'reactivate';
        } else if (existingActive && !isActive) {
          action = 'deactivate';
        } else {
          action = 'update';
        }
      } else if (!error && !isActive) {
        action = 'deactivate';
      } else if (!error) {
        action = 'create';
      }

      previewRows.push({
        rowNumber,
        rawName,
        displayName,
        unit,
        description,
        isActive,
        action: error ? 'error' : action,
        error,
      });
    }

    setAliasImportPreview(previewRows);
    setStatus(
      `Loaded CSV preview (${previewRows.length} rows, ${
        previewRows.filter((row) => row.action === 'error').length
      } errors).`
    );
  };

  const handleApplyAliasImport = async () => {
    if (!selectedAliasProjectId) {
      setStatus('Choose a project first.');
      return;
    }
    const actionableRows = aliasImportPreview.filter(
      (row) => row.action !== 'error' && row.action !== 'noop'
    );
    if (actionableRows.length === 0) {
      setStatus('No actionable rows in import preview.');
      return;
    }

    setApplyingAliasImport(true);
    try {
      for (const row of actionableRows) {
        await api.upsertMetricAlias(selectedAliasProjectId, {
          raw_name: row.rawName,
          display_name: row.displayName,
          unit: row.unit || undefined,
          description: row.description || undefined,
          is_active: row.isActive,
        });
      }
      await refreshAliases();
      setStatus(`Applied ${actionableRows.length} metric alias updates from CSV preview.`);
      setAliasImportPreview([]);
    } catch (error) {
      const message = error instanceof Error ? error.message : 'Failed to apply CSV import.';
      setStatus(message);
    } finally {
      setApplyingAliasImport(false);
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
            newAliasRawName={newAliasRawName}
            setNewAliasRawName={setNewAliasRawName}
            newAliasDisplayName={newAliasDisplayName}
            setNewAliasDisplayName={setNewAliasDisplayName}
            newAliasUnit={newAliasUnit}
            setNewAliasUnit={setNewAliasUnit}
            newAliasDescription={newAliasDescription}
            setNewAliasDescription={setNewAliasDescription}
            newAliasActive={newAliasActive}
            setNewAliasActive={setNewAliasActive}
            editingAliasRawName={editingAliasRawName}
            aliasFormErrors={aliasFormErrors}
            aliases={metricAliases}
            aliasImportPreview={aliasImportPreview}
            applyingAliasImport={applyingAliasImport}
            loadingAliases={loadingAliases}
            submittingAlias={submittingAlias}
            deletingAliasMetricName={deletingAliasMetricName}
            formatProjectRef={formatProjectRef}
            handleUpsertAlias={() => void handleUpsertAlias()}
            handleStartEditAlias={handleStartEditAlias}
            handleResetAliasForm={resetAliasForm}
            handleDeactivateAlias={(alias) => void handleDeactivateAlias(alias)}
            handleReactivateAlias={(alias) => void handleReactivateAlias(alias)}
            handleAliasImportFile={(file) => void handleAliasImportFile(file)}
            handleApplyAliasImport={() => void handleApplyAliasImport()}
            handleClearAliasImportPreview={() => setAliasImportPreview([])}
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
