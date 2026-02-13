'use client';

import { useCallback, useEffect, useState } from 'react';
import {
  AdminAuditEvent,
  AdminSession,
  AdminUser,
  AdminUserMembership,
  ApiError,
  api,
} from '@/lib/api';

function formatDate(value: string | null): string {
  if (!value) return '—';
  const parsed = new Date(value.replace(' ', 'T'));
  if (Number.isNaN(parsed.getTime())) return value;
  return parsed.toLocaleString();
}

function summarizeMetadata(metadata: unknown): string {
  try {
    const raw = JSON.stringify(metadata);
    if (!raw) return '{}';
    return raw.length > 160 ? `${raw.slice(0, 157)}...` : raw;
  } catch {
    return String(metadata);
  }
}

function getErrorMessage(error: unknown): string {
  if (error instanceof ApiError) return error.message;
  if (error instanceof Error) return error.message;
  return 'Unexpected error.';
}

function isForbiddenError(error: unknown): boolean {
  return error instanceof ApiError && (error.status === 401 || error.status === 403);
}

export default function AdminPage() {
  const [users, setUsers] = useState<AdminUser[]>([]);
  const [sessions, setSessions] = useState<AdminSession[]>([]);
  const [events, setEvents] = useState<AdminAuditEvent[]>([]);
  const [memberships, setMemberships] = useState<AdminUserMembership[]>([]);
  const [membershipUserId, setMembershipUserId] = useState<string | null>(null);
  const [includeRevokedSessions, setIncludeRevokedSessions] = useState(true);
  const [auditAction, setAuditAction] = useState('');
  const [auditUserId, setAuditUserId] = useState('');
  const [auditProjectId, setAuditProjectId] = useState('');
  const [auditLimit, setAuditLimit] = useState('100');
  const [loadingUsers, setLoadingUsers] = useState(false);
  const [loadingSessions, setLoadingSessions] = useState(false);
  const [loadingEvents, setLoadingEvents] = useState(false);
  const [forbidden, setForbidden] = useState(false);
  const [status, setStatus] = useState<string | null>(null);

  const refreshUsers = useCallback(async () => {
    setLoadingUsers(true);
    try {
      const result = await api.listAdminUsers();
      setUsers(result.users);
      setForbidden(false);
    } catch (error) {
      if (isForbiddenError(error)) {
        setForbidden(true);
        setStatus('Access denied. Platform admin credentials are required.');
      } else {
        setStatus(getErrorMessage(error));
      }
    } finally {
      setLoadingUsers(false);
    }
  }, []);

  const refreshSessions = useCallback(async (includeRevoked: boolean) => {
    setLoadingSessions(true);
    try {
      const result = await api.listAdminSessions({ includeRevoked });
      setSessions(result.sessions);
      setForbidden(false);
    } catch (error) {
      if (isForbiddenError(error)) {
        setForbidden(true);
        setStatus('Access denied. Platform admin credentials are required.');
      } else {
        setStatus(getErrorMessage(error));
      }
    } finally {
      setLoadingSessions(false);
    }
  }, []);

  const refreshEvents = useCallback(
    async (filters: { action?: string; userId?: string; projectId?: string; limit?: string }) => {
      setLoadingEvents(true);
      try {
        const parsedLimit = Number(filters.limit);
        const result = await api.listAdminAuditEvents({
          action: filters.action?.trim() || undefined,
          userId: filters.userId?.trim() || undefined,
          projectId: filters.projectId?.trim() || undefined,
          limit: Number.isFinite(parsedLimit) && parsedLimit > 0 ? parsedLimit : 100,
        });
        setEvents(result.events);
        setForbidden(false);
      } catch (error) {
        if (isForbiddenError(error)) {
          setForbidden(true);
          setStatus('Access denied. Platform admin credentials are required.');
        } else {
          setStatus(getErrorMessage(error));
        }
      } finally {
        setLoadingEvents(false);
      }
    },
    []
  );

  useEffect(() => {
    void refreshUsers();
    void refreshSessions(true);
    void refreshEvents({
      action: '',
      userId: '',
      projectId: '',
      limit: '100',
    });
  }, [refreshEvents, refreshSessions, refreshUsers]);

  useEffect(() => {
    void refreshSessions(includeRevokedSessions);
  }, [includeRevokedSessions, refreshSessions]);

  const refreshEventsFromInputs = useCallback(async () => {
    await refreshEvents({
      action: auditAction,
      userId: auditUserId,
      projectId: auditProjectId,
      limit: auditLimit,
    });
  }, [auditAction, auditLimit, auditProjectId, auditUserId, refreshEvents]);

  const handleDisableUser = async (userId: string) => {
    try {
      await api.disableAdminUser(userId);
      setStatus(`Disabled user ${userId}.`);
      await refreshUsers();
    } catch (error) {
      setStatus(getErrorMessage(error));
    }
  };

  const handleEnableUser = async (userId: string) => {
    try {
      await api.enableAdminUser(userId);
      setStatus(`Enabled user ${userId}.`);
      await refreshUsers();
    } catch (error) {
      setStatus(getErrorMessage(error));
    }
  };

  const handleLoadMemberships = async (userId: string) => {
    try {
      const result = await api.listAdminUserMemberships(userId, { includeRevoked: true });
      setMembershipUserId(userId);
      setMemberships(result.memberships);
    } catch (error) {
      setStatus(getErrorMessage(error));
    }
  };

  const handleRevokeSession = async (sessionId: string) => {
    try {
      await api.revokeAdminSession(sessionId);
      setStatus(`Revoked session ${sessionId}.`);
      await refreshSessions(includeRevokedSessions);
    } catch (error) {
      setStatus(getErrorMessage(error));
    }
  };

  return (
    <main className="min-h-screen">
      <div className="border-b border-border bg-surface">
        <div className="max-w-7xl mx-auto px-4 sm:px-6 py-5 sm:py-6">
          <h1 className="text-2xl font-bold text-text-primary">Platform Admin</h1>
          <p className="text-sm text-text-secondary mt-1">
            Global controls for users, sessions, and audit visibility.
          </p>
        </div>
      </div>

      <div className="max-w-7xl mx-auto px-4 sm:px-6 py-5 sm:py-6 space-y-4">
        {status && (
          <div className="rounded-lg border border-border bg-surface px-4 py-3 text-sm text-text-secondary">
            {status}
          </div>
        )}

        {forbidden && (
          <section className="rounded-xl border border-warning/30 bg-warning/10 p-4 text-sm text-warning">
            You are authenticated, but this view requires a platform-admin key or equivalent global admin access.
          </section>
        )}

        <section className="rounded-xl border border-border bg-surface p-4 sm:p-5 space-y-4">
          <div className="flex flex-wrap items-center justify-between gap-2">
            <div>
              <h2 className="text-lg font-semibold text-text-primary">Users</h2>
              <p className="text-sm text-text-secondary mt-1">Enable/disable accounts and inspect memberships.</p>
            </div>
            <button
              type="button"
              onClick={() => void refreshUsers()}
              className="px-4 py-2 rounded-lg border border-border text-sm font-medium text-text-secondary hover:bg-surface-secondary"
            >
              {loadingUsers ? 'Refreshing...' : 'Refresh Users'}
            </button>
          </div>

          <div className="overflow-x-auto rounded-lg border border-border">
            <table className="w-full text-left text-sm">
              <thead className="bg-surface-secondary text-text-secondary">
                <tr>
                  <th className="px-3 py-2 font-medium">User</th>
                  <th className="px-3 py-2 font-medium">Provider</th>
                  <th className="px-3 py-2 font-medium">Projects</th>
                  <th className="px-3 py-2 font-medium">Sessions</th>
                  <th className="px-3 py-2 font-medium">Status</th>
                  <th className="px-3 py-2 font-medium">Actions</th>
                </tr>
              </thead>
              <tbody>
                {users.length === 0 ? (
                  <tr>
                    <td colSpan={6} className="px-3 py-4 text-text-muted">
                      No users visible.
                    </td>
                  </tr>
                ) : (
                  users.map((user) => (
                    <tr key={user.user_id} className="border-t border-border">
                      <td className="px-3 py-2">
                        <div className="text-text-primary">{user.display_name || user.email || user.user_id}</div>
                        <div className="text-xs text-text-muted break-all">{user.user_id}</div>
                      </td>
                      <td className="px-3 py-2 text-text-secondary">{user.auth_provider}</td>
                      <td className="px-3 py-2 text-text-secondary">{user.active_project_count}</td>
                      <td className="px-3 py-2 text-text-secondary">{user.active_session_count}</td>
                      <td className="px-3 py-2 text-text-secondary">{user.disabled ? 'disabled' : 'active'}</td>
                      <td className="px-3 py-2">
                        <div className="flex flex-wrap gap-2">
                          <button
                            type="button"
                            onClick={() => void handleLoadMemberships(user.user_id)}
                            className="px-2.5 py-1.5 rounded-md border border-border text-xs font-medium text-text-secondary hover:bg-surface-secondary"
                          >
                            Memberships
                          </button>
                          {user.disabled ? (
                            <button
                              type="button"
                              onClick={() => void handleEnableUser(user.user_id)}
                              className="px-2.5 py-1.5 rounded-md border border-border text-xs font-medium text-success hover:bg-surface-secondary"
                            >
                              Enable
                            </button>
                          ) : (
                            <button
                              type="button"
                              onClick={() => void handleDisableUser(user.user_id)}
                              className="px-2.5 py-1.5 rounded-md border border-border text-xs font-medium text-warning hover:bg-surface-secondary"
                            >
                              Disable
                            </button>
                          )}
                        </div>
                      </td>
                    </tr>
                  ))
                )}
              </tbody>
            </table>
          </div>

          {membershipUserId && (
            <div className="rounded-lg border border-border bg-surface-secondary p-3">
              <p className="text-sm text-text-primary mb-2">Memberships for {membershipUserId}</p>
              {memberships.length === 0 ? (
                <p className="text-sm text-text-muted">No memberships found.</p>
              ) : (
                <ul className="space-y-1 text-sm text-text-secondary">
                  {memberships.map((membership) => (
                    <li key={`${membership.project_id}-${membership.created_at}`}>
                      {membership.project_name} ({membership.project_id}) - {membership.role}
                      {membership.revoked_at ? ` - revoked ${formatDate(membership.revoked_at)}` : ''}
                    </li>
                  ))}
                </ul>
              )}
            </div>
          )}
        </section>

        <section className="rounded-xl border border-border bg-surface p-4 sm:p-5 space-y-4">
          <div className="flex flex-wrap items-center justify-between gap-2">
            <div>
              <h2 className="text-lg font-semibold text-text-primary">Sessions</h2>
              <p className="text-sm text-text-secondary mt-1">List and revoke UI auth sessions.</p>
            </div>
            <div className="flex items-center gap-3">
              <label className="inline-flex items-center gap-2 text-sm text-text-secondary">
                <input
                  type="checkbox"
                  checked={includeRevokedSessions}
                  onChange={(event) => setIncludeRevokedSessions(event.target.checked)}
                  className="rounded border-border bg-surface-secondary"
                />
                include revoked
              </label>
              <button
                type="button"
                onClick={() => void refreshSessions(includeRevokedSessions)}
                className="px-4 py-2 rounded-lg border border-border text-sm font-medium text-text-secondary hover:bg-surface-secondary"
              >
                {loadingSessions ? 'Refreshing...' : 'Refresh Sessions'}
              </button>
            </div>
          </div>

          <div className="overflow-x-auto rounded-lg border border-border">
            <table className="w-full text-left text-sm">
              <thead className="bg-surface-secondary text-text-secondary">
                <tr>
                  <th className="px-3 py-2 font-medium">Session ID</th>
                  <th className="px-3 py-2 font-medium">User</th>
                  <th className="px-3 py-2 font-medium">Created</th>
                  <th className="px-3 py-2 font-medium">Last Seen</th>
                  <th className="px-3 py-2 font-medium">Expires</th>
                  <th className="px-3 py-2 font-medium">Status</th>
                  <th className="px-3 py-2 font-medium">Action</th>
                </tr>
              </thead>
              <tbody>
                {sessions.length === 0 ? (
                  <tr>
                    <td colSpan={7} className="px-3 py-4 text-text-muted">
                      No sessions found.
                    </td>
                  </tr>
                ) : (
                  sessions.map((session) => (
                    <tr key={session.session_id} className="border-t border-border">
                      <td className="px-3 py-2 text-text-primary">{session.session_id}</td>
                      <td className="px-3 py-2 text-text-secondary">{session.user_id}</td>
                      <td className="px-3 py-2 text-text-secondary">{formatDate(session.created_at)}</td>
                      <td className="px-3 py-2 text-text-secondary">{formatDate(session.last_seen_at)}</td>
                      <td className="px-3 py-2 text-text-secondary">{formatDate(session.expires_at)}</td>
                      <td className="px-3 py-2 text-text-secondary">
                        {session.revoked_at ? `revoked ${formatDate(session.revoked_at)}` : 'active'}
                      </td>
                      <td className="px-3 py-2">
                        <button
                          type="button"
                          onClick={() => void handleRevokeSession(session.session_id)}
                          disabled={Boolean(session.revoked_at)}
                          className="px-2.5 py-1.5 rounded-md border border-border text-xs font-medium text-text-secondary hover:bg-surface-secondary disabled:opacity-50 disabled:cursor-not-allowed"
                        >
                          Revoke
                        </button>
                      </td>
                    </tr>
                  ))
                )}
              </tbody>
            </table>
          </div>
        </section>

        <section className="rounded-xl border border-border bg-surface p-4 sm:p-5 space-y-4">
          <div className="flex flex-wrap items-center justify-between gap-2">
            <div>
              <h2 className="text-lg font-semibold text-text-primary">Audit Events</h2>
              <p className="text-sm text-text-secondary mt-1">Recent control-plane and data-plane actions.</p>
            </div>
            <button
              type="button"
              onClick={() => void refreshEventsFromInputs()}
              className="px-4 py-2 rounded-lg border border-border text-sm font-medium text-text-secondary hover:bg-surface-secondary"
            >
              {loadingEvents ? 'Refreshing...' : 'Refresh Events'}
            </button>
          </div>

          <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-3">
            <input
              type="text"
              value={auditAction}
              onChange={(event) => setAuditAction(event.target.value)}
              placeholder="action (e.g. run.delete)"
              className="w-full rounded-lg border border-border bg-surface-secondary px-3 py-2.5 text-sm text-text-primary placeholder:text-text-muted focus:outline-none focus:ring-2 focus:ring-accent focus:border-transparent"
            />
            <input
              type="text"
              value={auditUserId}
              onChange={(event) => setAuditUserId(event.target.value)}
              placeholder="actor user id"
              className="w-full rounded-lg border border-border bg-surface-secondary px-3 py-2.5 text-sm text-text-primary placeholder:text-text-muted focus:outline-none focus:ring-2 focus:ring-accent focus:border-transparent"
            />
            <input
              type="text"
              value={auditProjectId}
              onChange={(event) => setAuditProjectId(event.target.value)}
              placeholder="project id"
              className="w-full rounded-lg border border-border bg-surface-secondary px-3 py-2.5 text-sm text-text-primary placeholder:text-text-muted focus:outline-none focus:ring-2 focus:ring-accent focus:border-transparent"
            />
            <input
              type="number"
              min={1}
              max={500}
              value={auditLimit}
              onChange={(event) => setAuditLimit(event.target.value)}
              placeholder="limit"
              className="w-full rounded-lg border border-border bg-surface-secondary px-3 py-2.5 text-sm text-text-primary placeholder:text-text-muted focus:outline-none focus:ring-2 focus:ring-accent focus:border-transparent"
            />
          </div>

          <div className="overflow-x-auto rounded-lg border border-border">
            <table className="w-full text-left text-sm">
              <thead className="bg-surface-secondary text-text-secondary">
                <tr>
                  <th className="px-3 py-2 font-medium">When</th>
                  <th className="px-3 py-2 font-medium">Action</th>
                  <th className="px-3 py-2 font-medium">Outcome</th>
                  <th className="px-3 py-2 font-medium">Actor</th>
                  <th className="px-3 py-2 font-medium">Project</th>
                  <th className="px-3 py-2 font-medium">Metadata</th>
                </tr>
              </thead>
              <tbody>
                {events.length === 0 ? (
                  <tr>
                    <td colSpan={6} className="px-3 py-4 text-text-muted">
                      No audit events found for current filters.
                    </td>
                  </tr>
                ) : (
                  events.map((event) => (
                    <tr key={`${event.id}-${event.occurred_at}`} className="border-t border-border">
                      <td className="px-3 py-2 text-text-secondary">{formatDate(event.occurred_at)}</td>
                      <td className="px-3 py-2 text-text-primary">{event.action}</td>
                      <td className="px-3 py-2 text-text-secondary">{event.outcome}</td>
                      <td className="px-3 py-2 text-text-secondary">
                        {event.actor_user_id || event.actor_key_id || 'system'}
                      </td>
                      <td className="px-3 py-2 text-text-secondary">{event.project_id || '—'}</td>
                      <td className="px-3 py-2 text-text-secondary">{summarizeMetadata(event.metadata)}</td>
                    </tr>
                  ))
                )}
              </tbody>
            </table>
          </div>
        </section>
      </div>
    </main>
  );
}
