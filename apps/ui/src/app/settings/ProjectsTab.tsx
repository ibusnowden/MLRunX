import { ProjectInfo } from '@/lib/api';

export function ProjectsTab({
  session,
  availableProjects,
  newProjectName,
  setNewProjectName,
  newProjectDescription,
  setNewProjectDescription,
  submittingProject,
  loadingProjects,
  deletingProjectId,
  handleCreateProject,
  handleDeleteProject,
  refreshProjects,
}: {
  session: unknown;
  availableProjects: ProjectInfo[];
  newProjectName: string;
  setNewProjectName: (v: string) => void;
  newProjectDescription: string;
  setNewProjectDescription: (v: string) => void;
  submittingProject: boolean;
  loadingProjects: boolean;
  deletingProjectId: string | null;
  handleCreateProject: () => void;
  handleDeleteProject: (project: ProjectInfo) => void;
  refreshProjects: () => void;
}) {
  return (
    <section className="rounded-xl border border-border bg-surface p-4 sm:p-5 space-y-4">
      <div>
        <h2 className="text-lg font-semibold text-text-primary">Projects</h2>
        <p className="text-sm text-text-secondary mt-1">
          Create and delete your own projects. API keys and runs remain project-scoped.
        </p>
      </div>

      <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
        <div>
          <label htmlFor="project-name" className="block text-sm font-medium text-text-primary mb-1.5">
            Project Name
          </label>
          <input
            id="project-name"
            type="text"
            value={newProjectName}
            onChange={(event) => setNewProjectName(event.target.value)}
            placeholder="research-sandbox"
            className="w-full rounded-lg border border-border bg-surface-secondary px-3 py-2.5 text-sm text-text-primary placeholder:text-text-muted focus:outline-none focus:ring-2 focus:ring-accent focus:border-transparent"
          />
        </div>
        <div>
          <label htmlFor="project-description" className="block text-sm font-medium text-text-primary mb-1.5">
            Description (optional)
          </label>
          <input
            id="project-description"
            type="text"
            value={newProjectDescription}
            onChange={(event) => setNewProjectDescription(event.target.value)}
            placeholder="Scratch runs and experiments"
            className="w-full rounded-lg border border-border bg-surface-secondary px-3 py-2.5 text-sm text-text-primary placeholder:text-text-muted focus:outline-none focus:ring-2 focus:ring-accent focus:border-transparent"
          />
        </div>
      </div>

      <div className="flex flex-wrap gap-2">
        <button
          type="button"
          onClick={handleCreateProject}
          disabled={!session || submittingProject}
          className="px-4 py-2.5 rounded-lg bg-accent text-white text-sm font-medium hover:bg-accent-hover transition-colors disabled:opacity-60 disabled:cursor-not-allowed"
        >
          {submittingProject ? 'Creating...' : 'Create Project'}
        </button>
        <button
          type="button"
          onClick={refreshProjects}
          disabled={!session || loadingProjects}
          className="px-4 py-2.5 rounded-lg border border-border text-sm font-medium text-text-secondary hover:bg-surface-secondary hover:text-text-primary transition-colors disabled:opacity-60 disabled:cursor-not-allowed"
        >
          {loadingProjects ? 'Refreshing...' : 'Refresh Projects'}
        </button>
      </div>

      <div className="overflow-x-auto rounded-lg border border-border">
        <table className="w-full text-left text-sm">
          <thead className="bg-surface-secondary text-text-secondary">
            <tr>
              <th className="px-3 py-2 font-medium">Name</th>
              <th className="px-3 py-2 font-medium">Project ID</th>
              <th className="px-3 py-2 font-medium">Description</th>
              <th className="px-3 py-2 font-medium">Created</th>
              <th className="px-3 py-2 font-medium">Action</th>
            </tr>
          </thead>
          <tbody>
            {availableProjects.length === 0 ? (
              <tr>
                <td colSpan={5} className="px-3 py-4 text-text-muted">
                  {session ? 'No projects available.' : 'Sign in to manage projects.'}
                </td>
              </tr>
            ) : (
              availableProjects.map((project) => (
                <tr key={project.project_id} className="border-t border-border">
                  <td className="px-3 py-2 text-text-primary">{project.name}</td>
                  <td className="px-3 py-2 text-text-secondary">{project.project_id}</td>
                  <td className="px-3 py-2 text-text-secondary">{project.description || '—'}</td>
                  <td className="px-3 py-2 text-text-secondary">{project.created_at || '—'}</td>
                  <td className="px-3 py-2">
                    <button
                      type="button"
                      disabled={deletingProjectId === project.project_id}
                      onClick={() => handleDeleteProject(project)}
                      className="px-2.5 py-1.5 rounded-md border border-border text-xs font-medium text-warning hover:bg-surface-secondary transition-colors disabled:opacity-60 disabled:cursor-not-allowed"
                    >
                      {deletingProjectId === project.project_id ? 'Deleting...' : 'Delete'}
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
